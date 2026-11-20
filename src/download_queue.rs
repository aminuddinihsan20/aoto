//! The download queue: which song to fetch next when an album, a playlist or single
//! songs are asked for, and what waits (for Wi-Fi, for a connection, for Resume). Only
//! the deciding: the app starts and stops the fetches named here and reports how each
//! one ended (see `ui::downloading`).

use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::model::{Thumb, Track};
use crate::platform::Network;

/// Songs fetched at the same time. One fetch already pulls its file over four
/// connections (`stream::download`) and holds the whole song in memory until it is
/// saved, and the song playing streams beside them: two keep the phone and YouTube at ease.
pub const AT_ONCE: usize = 2;

/// Seconds between two songs starting. A fast connection brings down three songs a
/// second, each one a request to YouTube's player: no listener asks that often.
pub const GAP: f64 = 1.0;

/// This many songs of a collection failing one after another pause it: by then the
/// cause is the connection or the service, not the songs.
pub const STREAK: u32 = 5;

/// What a job is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// One song, chosen by hand.
    Song,
    Album,
    Playlist,
}

/// A song to fetch. `for_video`: the music video it is the song version of (see
/// `downloads::save`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub track: Track,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_video: Option<String>,
}

/// What is asked for: a collection's songs, or one song.
pub struct Ask {
    pub kind: Kind,
    /// The album's or playlist's id; a song's own.
    pub key: String,
    pub title: String,
    pub thumbs: Vec<Thumb>,
    pub songs: Vec<Song>,
}

/// What asking for a list of songs would do, in songs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// How many it would fetch.
    pub fetch: usize,
    /// Left out: downloaded already, or in the queue.
    pub downloaded: usize,
    pub queued: usize,
}

/// One album, playlist or song on its way down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    pub kind: Kind,
    pub key: String,
    pub title: String,
    pub thumbs: Vec<Thumb>,
    waiting: VecDeque<Song>,
    /// Being fetched now, each under the ticket its answer comes back with.
    #[serde(skip)]
    flying: Vec<(u64, Song)>,
    /// The songs it set out to fetch, and how they went so far.
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    /// Saved, but the copy into the listener's folder failed (Android).
    pub uncopied: usize,
    /// What the saved songs take on disk.
    pub bytes: u64,
    pub paused: bool,
    /// Paused by its failures, not by the listener.
    #[serde(skip)]
    stalled: bool,
    #[serde(skip)]
    streak: u32,
}

impl Job {
    /// The share of its songs that are settled, saved or failed.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 { 1.0 } else { (self.done + self.failed) as f32 / self.total as f32 }
    }
}

/// What a job is doing, for the page that shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Downloading,
    /// Its turn has not come.
    Queued,
    Paused,
    /// Paused after `STREAK` failures in a row.
    Stalled,
    WaitingForWifi,
    Offline,
}

/// A song to start fetching; `ticket` comes back with how it ended.
#[derive(Debug, PartialEq)]
pub struct Fetch {
    pub ticket: u64,
    pub song: Song,
}

/// How a fetch ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Saved { bytes: u64, copied: bool },
    Failed,
}

/// What a finished fetch means for its job.
#[derive(Debug, PartialEq)]
pub enum Update {
    /// More of it is still to come.
    Going,
    /// It paused itself after `STREAK` failures in a row.
    Stalled { title: String },
    /// That was its last song.
    Done(Summary),
}

/// A finished job.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub kind: Kind,
    pub title: String,
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub uncopied: usize,
}

pub struct Queue {
    jobs: Vec<Job>,
    last_job: u64,
    last_ticket: u64,
    wifi_only: bool,
    network: Network,
    /// When the last song started, on the clock `next` is told the time of.
    last_start: Option<f64>,
    /// Fetches to stop, until the app takes them.
    stops: Vec<u64>,
    changed: bool,
}

impl Queue {
    pub fn new(wifi_only: bool, network: Network) -> Queue {
        Queue { jobs: Vec::new(), last_job: 0, last_ticket: 0, wifi_only, network, last_start: None, stops: Vec::new(), changed: false }
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    /// The album or playlist `key`, while it is in the queue.
    pub fn collection(&self, key: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.kind != Kind::Song && j.key == key)
    }

    pub fn wifi_only(&self) -> bool {
        self.wifi_only
    }

    /// Why nothing is fetched now, when the connection is the reason.
    fn held(&self) -> Option<State> {
        match self.network {
            Network::Offline => Some(State::Offline),
            Network::Metered if self.wifi_only => Some(State::WaitingForWifi),
            Network::Metered | Network::Unmetered => None,
        }
    }

    pub fn state(&self, job: &Job) -> State {
        if job.paused {
            return if job.stalled { State::Stalled } else { State::Paused };
        }
        match self.held() {
            Some(waiting) => waiting,
            None if job.flying.is_empty() => State::Queued,
            None => State::Downloading,
        }
    }

    /// The songs in the queue, waiting or being fetched.
    fn queued(&self) -> HashSet<String> {
        self.jobs.iter().flat_map(|j| j.waiting.iter().chain(j.flying.iter().map(|(_, s)| s))).map(|s| s.track.id.clone()).collect()
    }

    /// What asking for `tracks` would do: `have` says which are downloaded.
    pub fn plan(&self, tracks: &[Track], have: impl Fn(&str) -> bool) -> Plan {
        let mut plan = Plan { fetch: 0, downloaded: 0, queued: 0 };
        let (queued, mut seen) = (self.queued(), HashSet::new());
        for t in tracks {
            if !seen.insert(t.id.as_str()) {
                continue;
            }
            if have(&t.id) {
                plan.downloaded += 1;
            } else if queued.contains(&t.id) {
                plan.queued += 1;
            } else {
                plan.fetch += 1;
            }
        }
        plan
    }

    /// Queue what is asked for, leaving out the songs that are downloaded (`have`) or
    /// in the queue already. The new job's number, when anything is left to fetch.
    pub fn add(&mut self, ask: Ask, have: impl Fn(&str) -> bool) -> Option<u64> {
        let (queued, mut seen) = (self.queued(), HashSet::new());
        let waiting: VecDeque<Song> = ask.songs.into_iter().filter(|s| seen.insert(s.track.id.clone()) && !have(&s.track.id) && !queued.contains(&s.track.id)).collect();
        if waiting.is_empty() {
            return None;
        }
        self.last_job += 1;
        self.jobs.push(Job {
            id: self.last_job,
            kind: ask.kind,
            key: ask.key,
            title: ask.title,
            thumbs: ask.thumbs,
            total: waiting.len(),
            waiting,
            flying: Vec::new(),
            done: 0,
            failed: 0,
            uncopied: 0,
            bytes: 0,
            paused: false,
            stalled: false,
            streak: 0,
        });
        self.changed = true;
        Some(self.last_job)
    }

    /// The job whose song starts next, when the connection and the places allow one: a
    /// song chosen by hand before a collection's, and collections in the order they
    /// were asked for.
    fn ready(&self) -> Option<usize> {
        if self.held().is_some() || self.jobs.iter().map(|j| j.flying.len()).sum::<usize>() >= AT_ONCE {
            return None;
        }
        let ready = |j: &Job| !j.paused && !j.waiting.is_empty();
        self.jobs.iter().position(|j| j.kind == Kind::Song && ready(j)).or_else(|| self.jobs.iter().position(ready))
    }

    /// Seconds until the next song may start (`GAP` after the last), when one waits for
    /// nothing else. `now`: seconds on a clock that only runs forward.
    pub fn due_in(&self, now: f64) -> Option<f64> {
        self.ready()?;
        Some(self.last_start.map_or(0.0, |started| (started + GAP - now).max(0.0)))
    }

    /// The next song to start fetching, if one may start now.
    pub fn next(&mut self, now: f64) -> Option<Fetch> {
        if self.due_in(now)? > 0.0 {
            return None;
        }
        let ready = self.ready()?;
        let job = &mut self.jobs[ready];
        let song = job.waiting.pop_front()?;
        self.last_ticket += 1;
        self.last_start = Some(now);
        job.flying.push((self.last_ticket, song.clone()));
        Some(Fetch { ticket: self.last_ticket, song })
    }

    /// The fetches to stop, named once: paused, cancelled, or the connection they may
    /// use is gone.
    pub fn take_stops(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.stops)
    }

    /// Stop what `job` is fetching; those songs wait at its front again.
    fn ground(job: &mut Job, stops: &mut Vec<u64>) {
        for (ticket, song) in job.flying.drain(..).rev() {
            stops.push(ticket);
            job.waiting.push_front(song);
        }
    }

    pub fn pause(&mut self, id: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.paused = true;
            Self::ground(job, &mut self.stops);
            self.changed = true;
        }
    }

    pub fn resume(&mut self, id: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.paused = false;
            job.stalled = false;
            job.streak = 0;
            self.changed = true;
        }
    }

    /// Drop a job: what it saved stays, the rest is not fetched.
    pub fn cancel(&mut self, id: u64) {
        if let Some(i) = self.jobs.iter().position(|j| j.id == id) {
            let job = self.jobs.remove(i);
            self.stops.extend(job.flying.iter().map(|(ticket, _)| *ticket));
            self.changed = true;
        }
    }

    pub fn set_wifi_only(&mut self, on: bool) {
        self.wifi_only = on;
        self.hold();
    }

    pub fn set_network(&mut self, network: Network) {
        self.network = network;
        self.hold();
    }

    /// The connection may not be used (any more): what is being fetched stops and waits.
    fn hold(&mut self) {
        if self.held().is_some() {
            for job in &mut self.jobs {
                Self::ground(job, &mut self.stops);
            }
        }
    }

    /// The fetch `ticket` of the song `video_id` ended. `None` for one that was stopped:
    /// its song waits in the queue again, unless it was saved after all.
    pub fn finished(&mut self, ticket: u64, video_id: &str, outcome: Outcome) -> Option<Update> {
        let flown = self.jobs.iter().position(|j| j.flying.iter().any(|(t, _)| *t == ticket));
        let i = match (flown, outcome) {
            (Some(i), _) => {
                self.jobs[i].flying.retain(|(t, _)| *t != ticket);
                i
            }
            (None, Outcome::Saved { .. }) => {
                let i = self.jobs.iter().position(|j| j.waiting.iter().any(|s| s.track.id == video_id))?;
                self.jobs[i].waiting.retain(|s| s.track.id != video_id);
                i
            }
            (None, Outcome::Failed) => return None,
        };
        self.changed = true;
        let job = &mut self.jobs[i];
        match outcome {
            Outcome::Saved { bytes, copied } => {
                job.done += 1;
                job.bytes += bytes;
                job.uncopied += usize::from(!copied);
                job.streak = 0;
            }
            Outcome::Failed => {
                job.failed += 1;
                job.streak += 1;
            }
        }
        if job.waiting.is_empty() && job.flying.is_empty() {
            let job = self.jobs.remove(i);
            return Some(Update::Done(Summary { kind: job.kind, title: job.title, total: job.total, done: job.done, failed: job.failed, uncopied: job.uncopied }));
        }
        if job.streak >= STREAK && !job.paused {
            job.paused = true;
            job.stalled = true;
            Self::ground(job, &mut self.stops);
            return Some(Update::Stalled { title: job.title.clone() });
        }
        Some(Update::Going)
    }

    /// The jobs as they are kept between launches: what was being fetched waits again.
    pub fn snapshot(&self) -> Vec<Job> {
        self.jobs
            .iter()
            .cloned()
            .map(|mut job| {
                Self::ground(&mut job, &mut Vec::new());
                job
            })
            .collect()
    }

    /// Take up the jobs kept from the last launch. A song saved since (`have`) counts as
    /// done, and a job with nothing left is dropped.
    pub fn restore(&mut self, jobs: Vec<Job>, have: impl Fn(&str) -> bool) {
        for mut job in jobs {
            let before = job.waiting.len();
            job.waiting.retain(|s| !have(&s.track.id));
            job.done += before - job.waiting.len();
            self.last_job = self.last_job.max(job.id);
            if !job.waiting.is_empty() {
                self.jobs.push(job);
            }
        }
    }

    /// Did what is kept between launches change since this was last asked?
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track { id: id.into(), title: format!("Song {id}"), artists: Vec::new(), album: None, duration: None, thumbs: Vec::new(), track_nr: None, plays: None, music_video: false }
    }

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter().map(|id| Song { track: track(id), for_video: None }).collect()
    }

    fn playlist(key: &str, ids: &[&str]) -> Ask {
        Ask { kind: Kind::Playlist, key: key.into(), title: format!("List {key}"), thumbs: Vec::new(), songs: songs(ids) }
    }

    fn one(id: &str) -> Ask {
        Ask { kind: Kind::Song, key: id.into(), title: format!("Song {id}"), thumbs: Vec::new(), songs: songs(&[id]) }
    }

    fn wifi() -> Queue {
        Queue::new(true, Network::Unmetered)
    }

    fn none(_: &str) -> bool {
        false
    }

    /// A clock that has moved on by the gap between two starts at every look.
    fn later() -> f64 {
        static LOOKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        LOOKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as f64 * GAP
    }

    /// Everything the queue lets start, given time, as (ticket, video id).
    fn start(q: &mut Queue) -> Vec<(u64, String)> {
        std::iter::from_fn(|| q.next(later())).map(|f| (f.ticket, f.song.track.id)).collect()
    }

    fn ids(started: &[(u64, String)]) -> Vec<&str> {
        started.iter().map(|(_, id)| id.as_str()).collect()
    }

    const SAVED: Outcome = Outcome::Saved { bytes: 1000, copied: true };

    #[test]
    fn a_collections_songs_are_fetched_in_order_two_at_a_time() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b", "c", "d"]), none).unwrap();
        let first = start(&mut q);
        assert_eq!(ids(&first), ["a", "b"]);
        assert!(q.next(later()).is_none(), "no third fetch while two are running");
        // One done: the next takes its place.
        assert_eq!(q.finished(first[0].0, "a", SAVED), Some(Update::Going));
        assert_eq!(ids(&start(&mut q)), ["c"]);
        let job = &q.jobs()[0];
        assert_eq!((job.done, job.total, job.bytes), (1, 4, 1000));
        assert_eq!(q.state(job), State::Downloading);
    }

    #[test]
    fn songs_start_a_second_apart_however_fast_they_come_down() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        let a = q.next(10.0).unwrap();
        assert!(q.next(10.4).is_none(), "a place is free, but the last song only just started");
        assert!((q.due_in(10.4).unwrap() - (GAP - 0.4)).abs() < 1e-9);
        assert!(q.next(10.0 + GAP).is_some());
        // Both places taken: nothing is due until one is free.
        assert_eq!(q.due_in(20.0), None);
        q.finished(a.ticket, "a", SAVED);
        assert_eq!(q.due_in(20.0), Some(0.0));
        assert!(q.next(20.0).is_some());
        assert_eq!(q.due_in(30.0), None, "nothing is left to start");
    }

    #[test]
    fn nothing_is_due_while_everything_waits() {
        let mut q = Queue::new(true, Network::Metered);
        let id = q.add(playlist("p", &["a"]), none).unwrap();
        assert_eq!(q.due_in(0.0), None, "waiting for Wi-Fi");
        q.set_network(Network::Unmetered);
        q.pause(id);
        assert_eq!(q.due_in(0.0), None, "paused");
    }

    #[test]
    fn songs_already_downloaded_or_in_the_queue_are_left_out_and_counted() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b"]), none).unwrap();
        let have = |id: &str| id == "c";
        let tracks: Vec<Track> = ["a", "c", "d", "e"].iter().map(|id| track(id)).collect();
        assert_eq!(q.plan(&tracks, have), Plan { fetch: 2, downloaded: 1, queued: 1 });
        // Asking for them queues just those two.
        let id = q.add(playlist("r", &["a", "c", "d", "e"]), have).unwrap();
        let job = q.jobs().iter().find(|j| j.id == id).unwrap();
        assert_eq!(job.total, 2);
    }

    #[test]
    fn a_collection_with_nothing_left_to_fetch_is_not_queued() {
        let mut q = wifi();
        assert_eq!(q.add(playlist("p", &["a", "b"]), |_| true), None);
        assert!(q.jobs().is_empty());
        assert!(q.next(later()).is_none());
    }

    #[test]
    fn a_song_twice_in_a_playlist_is_fetched_once() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "a", "b"]), none).unwrap();
        assert_eq!(q.jobs()[0].total, 2);
        let tracks = [track("x"), track("x")];
        assert_eq!(q.plan(&tracks, none).fetch, 1);
    }

    #[test]
    fn a_failed_song_is_counted_and_the_rest_go_on() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        let first = start(&mut q);
        assert_eq!(q.finished(first[0].0, "a", Outcome::Failed), Some(Update::Going));
        assert_eq!(ids(&start(&mut q)), ["c"]);
        let job = &q.jobs()[0];
        assert_eq!((job.done, job.failed), (0, 1));
        assert!((job.fraction() - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn the_last_song_ends_the_job_and_says_how_it_went() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        let first = start(&mut q);
        q.finished(first[0].0, "a", SAVED);
        q.finished(first[1].0, "b", Outcome::Failed);
        let last = start(&mut q);
        let end = q.finished(last[0].0, "c", Outcome::Saved { bytes: 5, copied: false });
        assert_eq!(end, Some(Update::Done(Summary { kind: Kind::Playlist, title: "List p".into(), total: 3, done: 2, failed: 1, uncopied: 1 })));
        assert!(q.jobs().is_empty(), "a finished job leaves the queue");
    }

    #[test]
    fn five_failures_in_a_row_pause_the_collection_until_it_is_resumed() {
        let mut q = wifi();
        let id = q.add(playlist("p", &["a", "b", "c", "d", "e", "f", "g", "h"]), none).unwrap();
        // A success in between starts the count again.
        let (t, _) = start(&mut q)[0].clone();
        q.finished(t, "a", Outcome::Failed);
        let (t, _) = start(&mut q)[0].clone();
        q.finished(t, "c", SAVED);
        let mut update = None;
        for _ in 0..STREAK {
            let (t, song) = q.jobs()[0].flying[0].clone();
            update = q.finished(t, &song.track.id, Outcome::Failed);
            start(&mut q);
        }
        assert_eq!(update, Some(Update::Stalled { title: "List p".into() }));
        let job = &q.jobs()[0];
        assert_eq!(q.state(job), State::Stalled);
        assert_eq!(job.failed, 1 + STREAK as usize);
        assert!(q.next(later()).is_none(), "nothing more is asked of the service");
        q.resume(id);
        assert_eq!(q.state(&q.jobs()[0]), State::Queued);
        assert_eq!(start(&mut q).len(), 1, "what is left goes on");
    }

    #[test]
    fn pausing_stops_its_fetches_and_resuming_starts_them_again() {
        let mut q = wifi();
        let id = q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        let first = start(&mut q);
        q.pause(id);
        let mut stops = q.take_stops();
        stops.sort_unstable();
        assert_eq!(stops, [first[0].0, first[1].0]);
        assert!(q.take_stops().is_empty(), "taken once");
        assert_eq!(q.state(&q.jobs()[0]), State::Paused);
        assert!(q.next(later()).is_none());
        q.resume(id);
        // The songs that were under way come first again, under new tickets.
        let again = start(&mut q);
        assert_eq!(ids(&again), ["a", "b"]);
        assert!(again.iter().all(|(t, _)| !stops.contains(t)));
    }

    #[test]
    fn a_stopped_fetchs_late_answer_is_ignored() {
        let mut q = wifi();
        let id = q.add(playlist("p", &["a", "b"]), none).unwrap();
        let first = start(&mut q);
        q.pause(id);
        assert_eq!(q.finished(first[0].0, "a", Outcome::Failed), None);
        let job = &q.jobs()[0];
        assert_eq!((job.done, job.failed), (0, 0));
        q.resume(id);
        assert_eq!(ids(&start(&mut q)), ["a", "b"]);
    }

    #[test]
    fn a_song_saved_while_it_was_being_stopped_is_not_fetched_again() {
        let mut q = wifi();
        let id = q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        let first = start(&mut q);
        q.pause(id);
        assert_eq!(q.finished(first[0].0, "a", SAVED), Some(Update::Going));
        assert_eq!(q.jobs()[0].done, 1);
        q.resume(id);
        assert_eq!(ids(&start(&mut q)), ["b", "c"]);
    }

    #[test]
    fn cancelling_stops_its_fetches_and_forgets_it() {
        let mut q = wifi();
        let id = q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        let first = start(&mut q);
        q.cancel(id);
        assert_eq!(q.take_stops().len(), 2);
        assert!(q.jobs().is_empty());
        assert!(q.next(later()).is_none());
        assert_eq!(q.finished(first[0].0, "a", SAVED), None);
    }

    #[test]
    fn on_mobile_data_wifi_only_holds_everything_until_wifi_is_back() {
        let mut q = Queue::new(true, Network::Metered);
        q.add(playlist("p", &["a", "b"]), none).unwrap();
        q.add(one("s"), none).unwrap();
        assert!(q.next(later()).is_none());
        assert!(q.jobs().iter().all(|j| q.state(j) == State::WaitingForWifi));
        q.set_network(Network::Unmetered);
        assert_eq!(start(&mut q).len(), 2, "it starts by itself");
    }

    #[test]
    fn losing_wifi_stops_what_is_being_fetched_and_keeps_it_for_later() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        let first = start(&mut q);
        q.set_network(Network::Metered);
        assert_eq!(q.take_stops().len(), 2);
        assert!(q.next(later()).is_none());
        assert_eq!(q.finished(first[0].0, "a", Outcome::Failed), None, "being stopped is not a failure");
        q.set_network(Network::Unmetered);
        assert_eq!(ids(&start(&mut q)), ["a", "b"]);
        assert_eq!(q.jobs()[0].failed, 0);
    }

    #[test]
    fn the_wifi_only_switch_decides_about_mobile_data() {
        let mut q = Queue::new(false, Network::Metered);
        q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        assert_eq!(start(&mut q).len(), 2, "allowed on mobile data");
        q.set_wifi_only(true);
        assert_eq!(q.take_stops().len(), 2, "switched on mid-way: it stops and waits");
        assert_eq!(q.state(&q.jobs()[0]), State::WaitingForWifi);
        q.set_wifi_only(false);
        assert_eq!(start(&mut q).len(), 2);
    }

    #[test]
    fn with_no_connection_everything_waits_whatever_the_switch_says() {
        let mut q = Queue::new(false, Network::Offline);
        q.add(playlist("p", &["a"]), none).unwrap();
        assert!(q.next(later()).is_none());
        assert_eq!(q.state(&q.jobs()[0]), State::Offline);
        q.set_network(Network::Metered);
        assert_eq!(start(&mut q).len(), 1);
    }

    #[test]
    fn a_song_chosen_by_hand_goes_before_a_collections_songs() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b", "c", "d"]), none).unwrap();
        let first = start(&mut q);
        q.add(one("s"), none).unwrap();
        q.finished(first[0].0, "a", SAVED);
        assert_eq!(ids(&start(&mut q)), ["s"]);
    }

    #[test]
    fn collections_take_their_turn_and_a_paused_one_lets_the_next_through() {
        let mut q = wifi();
        let p = q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        q.add(playlist("r", &["x", "y"]), none).unwrap();
        assert_eq!(ids(&start(&mut q)), ["a", "b"]);
        assert_eq!(q.state(&q.jobs()[1]), State::Queued);
        q.pause(p);
        assert_eq!(ids(&start(&mut q)), ["x", "y"]);
        assert_eq!(q.state(&q.jobs()[1]), State::Downloading);
    }

    #[test]
    fn a_collection_is_found_by_its_id() {
        let mut q = wifi();
        q.add(one("p"), none).unwrap();
        assert!(q.collection("p").is_none(), "a song with that id is not the collection");
        q.add(playlist("p", &["a"]), none).unwrap();
        assert_eq!(q.collection("p").map(|j| j.kind), Some(Kind::Playlist));
    }

    #[test]
    fn the_queue_is_kept_with_what_was_under_way_and_comes_back_without_what_got_saved() {
        let mut q = wifi();
        let p = q.add(playlist("p", &["a", "b", "c", "d"]), none).unwrap();
        q.add(playlist("r", &["x"]), none).unwrap();
        let first = start(&mut q);
        q.finished(first[0].0, "a", SAVED);
        start(&mut q);
        q.pause(p);
        let kept = serde_json::to_string(&q.snapshot()).unwrap();

        // A new launch: "b" had been saved just before the app went away, and "x" too.
        let mut q = wifi();
        q.restore(serde_json::from_str(&kept).unwrap(), |id| id == "b" || id == "x");
        assert_eq!(q.jobs().len(), 1, "a job with nothing left is not brought back");
        let job = &q.jobs()[0];
        assert_eq!((job.done, job.total, job.paused), (2, 4, true));
        q.resume(job.id);
        assert_eq!(ids(&start(&mut q)), ["c", "d"]);
        // A job added now doesn't take a kept one's number.
        let new = q.add(playlist("n", &["z"]), none).unwrap();
        assert_ne!(new, q.jobs()[0].id);
    }

    #[test]
    fn what_was_being_fetched_is_kept_too() {
        let mut q = wifi();
        q.add(playlist("p", &["a", "b", "c"]), none).unwrap();
        start(&mut q);
        let kept = q.snapshot();
        let mut q = wifi();
        q.restore(kept, none);
        assert_eq!(ids(&start(&mut q)), ["a", "b"]);
    }

    #[test]
    fn it_says_when_there_is_something_new_to_keep() {
        let mut q = wifi();
        assert!(!q.take_changed());
        let id = q.add(playlist("p", &["a", "b"]), none).unwrap();
        assert!(q.take_changed());
        assert!(!q.take_changed(), "asked once");
        let first = start(&mut q);
        assert!(!q.take_changed(), "starting a fetch changes nothing that is kept");
        q.finished(first[0].0, "a", SAVED);
        assert!(q.take_changed());
        q.pause(id);
        assert!(q.take_changed());
        q.cancel(id);
        assert!(q.take_changed());
    }
}
