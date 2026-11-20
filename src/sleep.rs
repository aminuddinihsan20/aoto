//! The sleep timer: the music stops after a while, when the song ends or when the queue
//! runs out, fading out first when asked. Only the timing: the player tells it the time
//! and what plays, and does the pausing (see `ui::sleep`).

/// How long the fade-out takes, in seconds.
pub const FADE_SECS: f64 = 30.0;

/// When the music stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stop {
    /// This many minutes after the timer started.
    After(u32),
    /// When the song playing finishes, instead of going on to the next.
    SongEnd,
    /// When the queue's last song finishes; nothing is added past it meanwhile.
    QueueEnd,
}

#[derive(Default)]
pub struct Timer {
    running: Option<Running>,
}

struct Running {
    stop: Stop,
    fade: bool,
    /// When an `After` timer is up, on the clock `now` is read from.
    ends_at: f64,
}

impl Timer {
    pub fn start(&mut self, stop: Stop, fade: bool, now: f64) {
        let ends_at = match stop {
            Stop::After(minutes) => now + minutes as f64 * 60.0,
            Stop::SongEnd | Stop::QueueEnd => f64::INFINITY,
        };
        self.running = Some(Running { stop, fade, ends_at });
    }

    pub fn cancel(&mut self) {
        self.running = None;
    }

    /// What the running timer waits for.
    pub fn running(&self) -> Option<Stop> {
        self.running.as_ref().map(|r| r.stop)
    }

    /// The running timer fades out.
    pub fn fades(&self) -> bool {
        self.running.as_ref().is_some_and(|r| r.fade)
    }

    /// Seconds until a timed one is up.
    pub fn remaining(&self, now: f64) -> Option<f64> {
        let r = self.running.as_ref().filter(|r| matches!(r.stop, Stop::After(_)))?;
        Some((r.ends_at - now).max(0.0))
    }

    /// How loud to play, as a share of the listener's own volume. `song_left`: seconds
    /// of the song still to play, when known; `last_song`: it is the queue's last.
    pub fn gain(&self, now: f64, song_left: Option<f64>, last_song: bool) -> f32 {
        let Some(r) = self.running.as_ref().filter(|r| r.fade) else { return 1.0 };
        let left = match r.stop {
            Stop::After(_) => Some(r.ends_at - now),
            Stop::SongEnd => song_left,
            Stop::QueueEnd => song_left.filter(|_| last_song),
        };
        left.map_or(1.0, fade_gain)
    }

    /// True once, when a timed one is up: the music pauses now, and the timer is off.
    pub fn due(&mut self, now: f64) -> bool {
        let up = self.running.as_ref().is_some_and(|r| now >= r.ends_at);
        if up {
            self.running = None;
        }
        up
    }

    /// The song's end belongs to the timer: nothing blends into it or starts after it.
    pub fn stops_after_song(&self, last_song: bool) -> bool {
        match self.running() {
            Some(Stop::SongEnd) => true,
            Some(Stop::QueueEnd) => last_song,
            Some(Stop::After(_)) | None => false,
        }
    }

    /// The song playing ended. True once when the music stops here, and the timer is off.
    pub fn song_ended(&mut self, last_song: bool) -> bool {
        let stops = self.stops_after_song(last_song);
        if stops {
            self.running = None;
        }
        stops
    }

    /// The queue is left to run out: no radio joins it.
    pub fn holds_queue(&self) -> bool {
        self.running() == Some(Stop::QueueEnd)
    }
}

/// The fade's level with `left` seconds to go: full until the last `FADE_SECS`, then
/// down to silence. Squared, so it sounds like a steady turn of the knob rather than
/// holding up and dropping away at the very end.
pub fn fade_gain(left: f64) -> f32 {
    let x = (left / FADE_SECS).clamp(0.0, 1.0) as f32;
    x * x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timed(minutes: u32, fade: bool, now: f64) -> Timer {
        let mut t = Timer::default();
        t.start(Stop::After(minutes), fade, now);
        t
    }

    #[test]
    fn a_timed_one_counts_down_to_nothing() {
        let t = timed(30, true, 1000.0);
        assert_eq!(t.running(), Some(Stop::After(30)));
        assert_eq!(t.remaining(1000.0), Some(1800.0));
        assert_eq!(t.remaining(1342.0), Some(1458.0));
        // Looked at late (the phone slept): never less than nothing.
        assert_eq!(t.remaining(9999.0), Some(0.0));
    }

    #[test]
    fn it_fires_once_and_turns_itself_off() {
        let mut t = timed(5, false, 0.0);
        assert!(!t.due(0.0));
        assert!(!t.due(299.9));
        assert!(t.due(300.0));
        assert_eq!(t.running(), None);
        assert!(!t.due(300.4));
        assert!(!t.due(9000.0));
        assert_eq!(t.remaining(300.4), None);
    }

    #[test]
    fn the_fade_turns_the_volume_down_over_the_last_thirty_seconds() {
        let t = timed(5, true, 0.0);
        // Full until the fade starts.
        assert_eq!(t.gain(0.0, None, false), 1.0);
        assert_eq!(t.gain(270.0, None, false), 1.0);
        // Half way through, a quarter of the level; silent when the time is up.
        assert!((t.gain(285.0, None, false) - 0.25).abs() < 1e-6);
        assert_eq!(t.gain(300.0, None, false), 0.0);
        assert_eq!(t.gain(305.0, None, false), 0.0);
        // Never louder a moment later.
        let levels: Vec<f32> = (0..=300).map(|i| t.gain(270.0 + i as f64 * 0.1, None, false)).collect();
        assert!(levels.windows(2).all(|w| w[1] <= w[0]), "{levels:?}");
        assert!(levels.windows(2).all(|w| w[0] - w[1] < 0.01), "no audible step: {levels:?}");
    }

    #[test]
    fn without_the_fade_it_plays_at_full_volume_to_the_end() {
        let t = timed(5, false, 0.0);
        assert!(!t.fades());
        assert_eq!(t.gain(285.0, None, false), 1.0);
        assert_eq!(t.gain(299.9, None, false), 1.0);
        assert!(timed(5, true, 0.0).fades());
    }

    #[test]
    fn no_timer_leaves_the_volume_and_the_queue_alone() {
        let mut t = Timer::default();
        assert_eq!(t.running(), None);
        assert_eq!(t.remaining(10.0), None);
        assert_eq!(t.gain(10.0, Some(3.0), true), 1.0);
        assert!(!t.due(10.0));
        assert!(!t.stops_after_song(true));
        assert!(!t.song_ended(true));
        assert!(!t.holds_queue());
    }

    #[test]
    fn end_of_this_song_stops_when_the_song_does() {
        let mut t = Timer::default();
        t.start(Stop::SongEnd, false, 0.0);
        assert_eq!(t.running(), Some(Stop::SongEnd));
        // No clock to run out.
        assert_eq!(t.remaining(50.0), None);
        assert!(!t.due(1e9));
        // The next song neither blends in nor starts, wherever the song is in the queue.
        assert!(t.stops_after_song(false));
        assert!(t.stops_after_song(true));
        assert!(!t.holds_queue());
        assert!(t.song_ended(false));
        assert_eq!(t.running(), None);
        // The song after that is not its business.
        assert!(!t.song_ended(false));
        assert!(!t.stops_after_song(false));
    }

    #[test]
    fn end_of_this_song_fades_over_the_songs_last_thirty_seconds() {
        let mut t = Timer::default();
        t.start(Stop::SongEnd, true, 0.0);
        assert_eq!(t.gain(5.0, Some(200.0), false), 1.0);
        assert_eq!(t.gain(5.0, Some(30.0), false), 1.0);
        assert!((t.gain(5.0, Some(15.0), false) - 0.25).abs() < 1e-6);
        assert_eq!(t.gain(5.0, Some(0.0), false), 0.0);
        // A song whose length isn't known yet plays as it is.
        assert_eq!(t.gain(5.0, None, false), 1.0);
        t.start(Stop::SongEnd, false, 0.0);
        assert_eq!(t.gain(5.0, Some(15.0), false), 1.0);
    }

    #[test]
    fn end_of_the_queue_lets_the_songs_before_the_last_go_on() {
        let mut t = Timer::default();
        t.start(Stop::QueueEnd, true, 0.0);
        assert!(t.holds_queue());
        assert_eq!(t.remaining(50.0), None);
        assert!(!t.due(1e9));
        // Songs with more after them end as usual, at full volume.
        assert!(!t.stops_after_song(false));
        assert_eq!(t.gain(5.0, Some(10.0), false), 1.0);
        assert!(!t.song_ended(false));
        assert_eq!(t.running(), Some(Stop::QueueEnd));
        // The last one fades, and ends the music.
        assert!(t.stops_after_song(true));
        assert!((t.gain(5.0, Some(15.0), true) - 0.25).abs() < 1e-6);
        assert!(t.song_ended(true));
        assert_eq!(t.running(), None);
        assert!(!t.holds_queue());
    }

    #[test]
    fn cancelling_turns_everything_off() {
        let mut t = timed(5, true, 0.0);
        t.cancel();
        assert_eq!(t.running(), None);
        assert_eq!(t.remaining(100.0), None);
        assert_eq!(t.gain(290.0, None, false), 1.0);
        assert!(!t.due(300.0));
        let mut t = Timer::default();
        t.start(Stop::QueueEnd, true, 0.0);
        t.cancel();
        assert!(!t.holds_queue());
        assert!(!t.song_ended(true));
    }

    #[test]
    fn starting_another_replaces_the_one_running() {
        let mut t = timed(5, true, 0.0);
        t.start(Stop::After(15), false, 100.0);
        assert_eq!(t.running(), Some(Stop::After(15)));
        assert_eq!(t.remaining(100.0), Some(900.0));
        assert!(!t.fades());
        assert!(!t.due(300.0));
        t.start(Stop::SongEnd, true, 200.0);
        assert!(!t.due(1000.0));
        assert!(t.song_ended(false));
    }

    #[test]
    fn pausing_and_resuming_by_hand_leaves_it_running() {
        // Nothing the listener does to the music reaches the timer: paused for two of
        // its five minutes (no song position to go by meanwhile), it is still up on time.
        let mut t = timed(5, true, 0.0);
        assert!(!t.due(60.0));
        assert_eq!(t.gain(60.0, None, false), 1.0);
        assert!(!t.due(180.0));
        assert_eq!(t.running(), Some(Stop::After(5)));
        assert_eq!(t.remaining(180.0), Some(120.0));
        assert!(t.due(300.0));
        // The same for a song paused near its end: it stops there once it has played out.
        let mut t = Timer::default();
        t.start(Stop::SongEnd, true, 0.0);
        assert!(!t.due(4000.0));
        assert_eq!(t.running(), Some(Stop::SongEnd));
        assert!(t.song_ended(false));
    }

    #[test]
    fn the_fade_curve_runs_from_full_to_silent() {
        assert_eq!(fade_gain(45.0), 1.0);
        assert_eq!(fade_gain(FADE_SECS), 1.0);
        assert!((fade_gain(15.0) - 0.25).abs() < 1e-6);
        assert!((fade_gain(3.0) - 0.01).abs() < 1e-6);
        assert_eq!(fade_gain(0.0), 0.0);
        assert_eq!(fade_gain(-2.0), 0.0);
    }
}
