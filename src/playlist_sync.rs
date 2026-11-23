//! Keeping a downloaded playlist up to date: when a playlist that was downloaded whole has
//! gained songs since, the new ones are downloaded too. Only songs that are new to the
//! playlist count: one the listener took off the phone by hand is not fetched again, and a
//! song taken out of the playlist keeps its file until the listener removes it.

use std::collections::HashSet;

use crate::downloads::Kept;
use crate::model::Track;

/// How long between looks at the playlists, in seconds.
pub const EVERY: f64 = 6.0 * 3600.0;
/// The first look comes this long after launch, so a start-up is not slowed by it.
pub const FIRST: f64 = 45.0;

/// What the playlist's current songs ask for.
#[derive(Debug, PartialEq)]
pub struct Plan {
    /// Songs that are in the playlist now and were not when it was downloaded, in
    /// playlist order, each once.
    pub new: Vec<Track>,
}

/// The songs of `current` that `kept` did not have. An empty `current` (a page that came
/// back with nothing) is no news, never "everything was removed".
pub fn plan(kept: &Kept, current: &[Track]) -> Plan {
    let had: HashSet<&str> = kept.songs.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    let new = current.iter().filter(|t| !had.contains(t.id.as_str()) && seen.insert(t.id.as_str())).cloned().collect();
    Plan { new }
}

/// Is it time to look? `checked`: when the last look was (app time, seconds), if there was one.
pub fn due(now: f64, checked: Option<f64>) -> bool {
    match checked {
        None => now >= FIRST,
        Some(t) => now - t >= EVERY,
    }
}

/// When to wake up for the next look (app time, seconds).
pub fn next_at(checked: Option<f64>) -> f64 {
    checked.map_or(FIRST, |t| t + EVERY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str) -> Track {
        Track { id: id.into(), title: format!("Song {id}"), artists: Vec::new(), album: None, duration: None, thumbs: Vec::new(), track_nr: None, plays: None, music_video: false }
    }

    fn kept(songs: &[&str]) -> Kept {
        Kept { id: "PL1".into(), title: "santai".into(), is_album: false, thumbs: Vec::new(), songs: songs.iter().map(|s| (*s).to_owned()).collect() }
    }

    fn ids(p: &Plan) -> Vec<&str> {
        p.new.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn songs_added_to_the_playlist_are_new() {
        let p = plan(&kept(&["a", "b"]), &[song("a"), song("b"), song("c"), song("d")]);
        assert_eq!(ids(&p), ["c", "d"]);
    }

    #[test]
    fn nothing_added_is_nothing_to_do() {
        assert!(plan(&kept(&["a", "b"]), &[song("a"), song("b")]).new.is_empty());
    }

    #[test]
    fn a_song_taken_out_of_the_playlist_is_not_news() {
        assert!(plan(&kept(&["a", "b", "c"]), &[song("a"), song("c")]).new.is_empty());
    }

    #[test]
    fn a_song_the_listener_removed_from_the_phone_stays_known_and_is_not_fetched_again() {
        // "b" is still in the playlist and in what was kept; its file is gone (removed by hand).
        let p = plan(&kept(&["a", "b"]), &[song("a"), song("b"), song("c")]);
        assert_eq!(ids(&p), ["c"]);
    }

    #[test]
    fn new_songs_keep_the_playlists_order_and_come_once() {
        let p = plan(&kept(&["a"]), &[song("z"), song("a"), song("y"), song("z"), song("x")]);
        assert_eq!(ids(&p), ["z", "y", "x"]);
    }

    #[test]
    fn a_page_that_came_back_empty_is_not_news() {
        assert!(plan(&kept(&["a", "b"]), &[]).new.is_empty());
    }

    #[test]
    fn a_playlist_downloaded_with_nothing_kept_would_take_everything() {
        assert_eq!(ids(&plan(&kept(&[]), &[song("a"), song("b")])), ["a", "b"]);
    }

    #[test]
    fn the_first_look_waits_for_the_start_up_and_later_ones_for_the_interval() {
        assert!(!due(10.0, None));
        assert!(due(FIRST, None));
        assert!(!due(FIRST + 100.0, Some(FIRST)));
        assert!(due(FIRST + EVERY, Some(FIRST)));
    }

    #[test]
    fn the_app_wakes_for_the_next_look() {
        assert_eq!(next_at(None), FIRST);
        assert_eq!(next_at(Some(100.0)), 100.0 + EVERY);
    }

    /// A real public playlist read the way the sync reads it: every page, and a playlist that
    /// was downloaded when it had fewer songs asks for exactly the rest.
    /// `cargo test --lib live_playlist_gains -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_playlist_gains_songs() {
        let id = std::env::var("YTM_PLAYLIST_TEST").unwrap_or_else(|_| "PL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i".to_owned());
        let api = crate::ytmapi::Api::new(&std::env::temp_dir().join("ytm-rs-sync-test"));
        let c = crate::ytm::playlist(&api, &id, false, None).await.expect("the playlist");
        println!("{}: {} songs", c.title, c.tracks.len());
        assert!(c.tracks.len() > 20, "a real playlist has pages of songs");
        let first: Vec<&str> = c.tracks.iter().take(10).map(|t| t.id.as_str()).collect();
        let p = plan(&kept(&first), &c.tracks);
        let mut unique = HashSet::new();
        let distinct = c.tracks.iter().filter(|t| unique.insert(t.id.as_str())).count();
        assert!(!p.new.is_empty());
        assert!(p.new.len() <= distinct - first.len(), "no song is asked for twice");
        println!("{} songs are new since the first 10", p.new.len());
    }
}
