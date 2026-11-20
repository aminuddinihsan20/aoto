//! Android Auto's side of the app: while a car is connected it keeps the tree the car
//! browses (`crate::auto`) up to date, and it plays what the car asks for. It runs from
//! `pump`, so it works with the activity away, as a phone in a car is.

use std::time::{Duration, Instant};

use super::{Action, App, Page};
use crate::auto::{self, Anything, MediaId, Source};
use crate::model::{ShelfKind, Target, Track};

/// How often the tree is looked over while a car is connected, and how long a listing
/// of the downloads is kept (it looks at every file).
const EVERY: Duration = Duration::from_millis(500);
const DOWNLOADS_EVERY: Duration = Duration::from_secs(20);

/// What is remembered between two looks.
#[derive(Default)]
pub(super) struct State {
    published: Option<Instant>,
    /// The downloaded songs, and when they were listed.
    downloads: Vec<Track>,
    listed: Option<Instant>,
}

/// Dev hook: YTM_AUTO stands in for a connected car, to check the tree and what a tap
/// does without one. It logs every list as it changes, and plays a script:
/// `YTM_AUTO=<secs>:<node>:<row>[,…]` taps that row of that list then (`12:picks:0`),
/// and `<secs>:say:<words>` asks by voice (`20:say:` for anything). `YTM_AUTO=1` only
/// connects.
fn dev_car() {
    use std::sync::Mutex;
    /// When the car connected, the tree's counts last logged, and how far the script is.
    static CAR: Mutex<Option<(Instant, String, usize)>> = Mutex::new(None);
    let Some(script) = super::dev_var("YTM_AUTO") else { return };
    let mut car = CAR.lock().unwrap_or_else(|e| e.into_inner());
    let (since, seen, done) = car.get_or_insert_with(|| {
        auto::set_wanted(true);
        (Instant::now(), String::new(), 0)
    });
    let versions = auto::versions();
    if versions != *seen {
        for node in versions.lines().filter(|l| !seen.lines().any(|s| s == *l)).filter_map(|l| l.split('\u{1f}').next()) {
            let rows = auto::rows(node);
            log::info!("android auto: {node} has {} rows{}", rows.lines().count(), rows.lines().take(4).map(|r| format!("\n    {}", r.replace('\u{1f}', " | "))).collect::<String>());
        }
        *seen = versions;
    }
    let steps: Vec<(f64, &str, &str)> = script.split(',').filter_map(|s| {
        let mut f = s.splitn(3, ':');
        Some((f.next()?.parse().ok()?, f.next()?, f.next()?))
    }).collect();
    while let Some(&(at, what, arg)) = steps.get(*done).filter(|s| since.elapsed().as_secs_f64() >= s.0) {
        *done += 1;
        if what == "say" {
            log::info!("android auto: {at} s, asked for {arg:?}");
            auto::push_search(arg);
        } else if let Some(id) = arg.parse().ok().and_then(|row: usize| auto::rows(what).lines().nth(row).and_then(|r| r.split('\u{1f}').next().map(str::to_owned))) {
            log::info!("android auto: {at} s, tapped {id}");
            auto::push_play(&id);
        } else {
            log::warn!("android auto: {at} s, no row {arg} in {what}");
        }
    }
}

impl App {
    /// The car's turn, drawn or in the background.
    pub(super) fn auto_tick(&mut self) {
        dev_car();
        for id in auto::take_plays() {
            self.auto_play(id);
        }
        for query in auto::take_searches() {
            self.auto_search(query.trim());
        }
        if !auto::wanted() {
            if self.auto.published.is_some() {
                self.auto = State::default();
            }
            return;
        }
        if self.auto.published.is_some_and(|at| at.elapsed() < EVERY) {
            return;
        }
        if self.auto.listed.is_none_or(|at| at.elapsed() >= DOWNLOADS_EVERY) {
            self.auto.downloads = crate::downloads::list();
            self.auto.listed = Some(Instant::now());
        }
        self.auto.published = Some(Instant::now());
        auto::publish(&Source {
            queue: &self.queue.tracks,
            index: self.queue.index,
            downloads: &self.auto.downloads,
            // Last launch's playlists until the library has arrived.
            playlists: self.library.as_ref().map(|l| l.playlists.as_slice()).or(self.sidebar_cache.as_deref()).unwrap_or_default(),
            signed_in: self.logged_in,
            picks: self.quick_picks(),
        });
    }

    /// With a car connected the screen is drawn again this soon, however idle it is: the
    /// tree is looked over from there. (Away, the background tick does it.)
    pub(super) fn auto_wake(&self) -> Option<Duration> {
        auto::wanted().then_some(EVERY)
    }

    /// Home's quick picks, once home has loaded.
    fn quick_picks(&self) -> &[Track] {
        let shelves = self.home.as_ref().map(|h| h.shelves.as_slice()).unwrap_or_default();
        shelves.iter().find_map(|s| match &s.kind {
            ShelfKind::Songs { tracks, .. } => Some(tracks.as_slice()),
            ShelfKind::Cards(_) => None,
        }).unwrap_or_default()
    }

    /// A tap on a row in the car.
    fn auto_play(&mut self, id: MediaId) {
        log::info!("android auto: play {id:?}");
        if self.jam.locked() {
            return self.toast("Only the host can change the party");
        }
        match id {
            MediaId::Queue(at, song) => match auto::queue_place(&self.queue.tracks, at, &song) {
                // The song that is loaded already: it plays on, or starts if it was paused.
                Some(i) if Some(i) == self.queue.index => {
                    if !self.playing() && !self.buffering {
                        self.toggle();
                    }
                }
                Some(i) => self.actions.push(Action::QueueJump(i)),
                None => {}
            },
            MediaId::Download(song) => self.auto_play_from(crate::downloads::list(), &song),
            MediaId::Pick(song) => self.auto_play_from(self.quick_picks().to_vec(), &song),
            MediaId::Playlist(id) => self.actions.push(Action::PlayCollection(Target::Playlist(id), false)),
        }
    }

    /// Asked for by voice ("play … on Aoto"): the first song YouTube Music finds for
    /// the words, and the other songs found after it. With no words: anything.
    fn auto_search(&mut self, query: &str) {
        log::info!("android auto: asked for {query:?}");
        if self.jam.locked() {
            return self.toast("Only the host can change the party");
        }
        if query.is_empty() {
            let downloads = crate::downloads::list();
            match auto::anything(self.queue.index.is_some(), downloads.len(), self.quick_picks().len()) {
                Anything::Resume => {
                    if !self.playing() && !self.buffering {
                        self.toggle();
                    }
                }
                Anything::Downloads => self.auto_play_all(downloads),
                Anything::Picks => self.auto_play_all(self.quick_picks().to_vec()),
                Anything::Nothing => {}
            }
        } else if let Some(found) = self.search.as_ref().filter(|s| s.query == query) {
            // Searched for already: its songs are here.
            self.auto_play_all(found.tracks.clone());
        } else {
            // As a search typed on the phone, whose first song then plays: the page's own
            // path, the one the YTM_SMOKE check drives (`smoke`, where the search's answer arrives).
            self.go(Page::Search(query.to_owned()));
            self.smoke = Some(query.to_owned());
        }
    }

    fn auto_play_all(&mut self, tracks: Vec<Track>) {
        self.queue_source = None;
        self.play_list(tracks, 0);
    }

    /// Play `song` of `tracks`, and the ones after it.
    fn auto_play_from(&mut self, tracks: Vec<Track>, song: &str) {
        if let Some(i) = tracks.iter().position(|t| t.id == song) {
            self.queue_source = None;
            self.play_list(tracks, i);
        }
    }
}
