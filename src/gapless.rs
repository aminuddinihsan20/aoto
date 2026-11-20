//! Gapless playback: the next song's first sample right behind the playing one's last,
//! in the same stream of samples, so no silence is put between two songs of an album.
//!
//! Without it the player runs empty at a song's end and the next song is started from
//! outside: the audio thread notices the end on its next look (every 100 ms), tells the
//! UI, which hears it on its next frame (or its next background tick, 400 ms apart,
//! with the screen off) and asks for the next song; the backend opens its decoder and
//! sends it back. That is 20 to 110 ms of silence in the audio thread alone (measured
//! by `audio`'s tests, with the next song handed over the instant it is asked for) and
//! up to half a second more in a pocket. On top of that an AAC stream's decoder plays
//! the encoder's priming (1600 samples on YouTube's) and padding as if they were the
//! song; with gapless playback on they are left out (`stream::Edited`).
//!
//! The audio thread keeps a `Slot` and puts the next song in it, decoded and ready,
//! some seconds before the playing one ends. `Joined` is what the player plays: the
//! song, and when that runs out, whatever stands by in the slot. A song that did not
//! end as songs do (its download broke off, it is being faded out) is not followed, and
//! neither is one in another format: then the player runs empty as it always did, and
//! the next song starts the usual way.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A song, and whether its running out is its end: only then does the next follow it.
pub struct Link<S> {
    pub source: S,
    pub ended_well: Box<dyn Fn() -> bool + Send>,
}

/// Where the next song stands by.
pub struct Slot<S> {
    next: Mutex<Option<Link<S>>>,
    joined: AtomicBool,
}

impl<S> Default for Slot<S> {
    fn default() -> Self {
        Self { next: Mutex::new(None), joined: AtomicBool::new(false) }
    }
}

impl<S> Slot<S> {
    /// `next` follows the playing song, in place of whatever stood by.
    pub fn put(&self, next: Link<S>) {
        *self.next.lock().unwrap() = Some(next);
    }

    /// Nothing follows the playing song.
    pub fn clear(&self) {
        *self.next.lock().unwrap() = None;
    }

    /// Whether the song that stood by has started since this was last asked.
    pub fn take_joined(&self) -> bool {
        self.joined.swap(false, Ordering::AcqRel)
    }
}

/// A song with the slot's song behind it.
pub struct Joined<S> {
    current: Link<S>,
    slot: Arc<Slot<S>>,
    /// Samples handed out of the current song: it is only followed on a whole frame.
    handed: usize,
}

impl<S: rodio::Source> Joined<S> {
    pub fn new(current: Link<S>, slot: Arc<Slot<S>>) -> Self {
        Self { current, slot, handed: 0 }
    }

    /// The song standing by, when it may follow the current one: this one ended as a
    /// song does, on a whole frame, and the next is in the same format (the samples go
    /// on at this one's rate).
    fn follower(&mut self) -> Option<Link<S>> {
        let channels = self.current.source.channels();
        if !self.handed.is_multiple_of(channels.get() as usize) || !(self.current.ended_well)() {
            return None;
        }
        let mut next = self.slot.next.lock().unwrap();
        let follower = next.take_if(|n| n.source.channels() == channels && n.source.sample_rate() == self.current.source.sample_rate())?;
        // Told while the slot is held: whoever clears the slot knows right after whether
        // the song in it got to play.
        self.slot.joined.store(true, Ordering::Release);
        Some(follower)
    }
}

impl<S: rodio::Source> Iterator for Joined<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        loop {
            if let Some(s) = self.current.source.next() {
                self.handed += 1;
                return Some(s);
            }
            self.current = self.follower()?;
            self.handed = 0;
        }
    }
}

impl<S: rodio::Source> rodio::Source for Joined<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.current.source.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.current.source.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.current.source.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.current.source.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    fn song(channels: u16, rate: u32, samples: Vec<f32>) -> Link<SamplesBuffer> {
        Link { source: SamplesBuffer::new(NonZero::new(channels).unwrap(), NonZero::new(rate).unwrap(), samples), ended_well: Box::new(|| true) }
    }

    /// Two songs no sample of which could be taken for the other's, or for silence.
    fn two() -> (Vec<f32>, Vec<f32>) {
        ((1..=2000).map(|i| i as f32).collect(), (1..=3000).map(|i| -(i as f32)).collect())
    }

    #[test]
    fn the_next_song_follows_the_last_sample_with_nothing_between() {
        let (a, b) = two();
        let slot = Arc::new(Slot::default());
        let mut joined = Joined::new(song(2, 44_100, a.clone()), slot.clone());
        let mut out: Vec<f32> = joined.by_ref().take(1000).collect();
        // The next song arrives while this one plays.
        slot.put(song(2, 44_100, b.clone()));
        assert!(!slot.take_joined());
        out.extend(joined.by_ref());
        assert_eq!(out, [a, b].concat(), "exactly the first song's samples, then the second's");
        assert!(slot.take_joined(), "the audio thread hears that the next song started");
        assert!(!slot.take_joined(), "once");
    }

    #[test]
    fn a_song_follows_the_one_that_followed() {
        let (a, b) = two();
        let slot = Arc::new(Slot::default());
        let mut joined = Joined::new(song(1, 48_000, a.clone()), slot.clone());
        slot.put(song(1, 48_000, b.clone()));
        let mut out: Vec<f32> = joined.by_ref().take(a.len() + 10).collect();
        assert!(slot.take_joined());
        slot.put(song(1, 48_000, a.clone()));
        out.extend(joined);
        assert_eq!(out, [a.clone(), b, a].concat());
    }

    #[test]
    fn with_no_song_ready_in_time_the_player_runs_empty_as_before() {
        let (a, b) = two();
        let slot = Arc::new(Slot::default());
        let mut joined = Joined::new(song(2, 44_100, a.clone()), slot.clone());
        let out: Vec<f32> = joined.by_ref().collect();
        assert_eq!(out, a);
        assert!(!slot.take_joined());
        // Too late: the song is over, and the next one starts the usual way.
        slot.put(song(2, 44_100, b));
        slot.clear();
        assert_eq!(joined.next(), None);
    }

    #[test]
    fn a_song_that_broke_off_or_is_being_faded_out_is_not_followed() {
        let (a, b) = two();
        let slot = Arc::new(Slot::default());
        let broken = Link { ended_well: Box::new(|| false), ..song(2, 44_100, a.clone()) };
        slot.put(song(2, 44_100, b));
        let out: Vec<f32> = Joined::new(broken, slot.clone()).collect();
        assert_eq!(out, a);
        assert!(!slot.take_joined());
    }

    #[test]
    fn a_song_in_another_format_is_not_followed() {
        let (a, b) = two();
        for (channels, rate) in [(2, 48_000), (1, 44_100)] {
            let slot = Arc::new(Slot::default());
            slot.put(song(channels, rate, b.clone()));
            let out: Vec<f32> = Joined::new(song(2, 44_100, a.clone()), slot.clone()).collect();
            assert_eq!(out, a, "{channels} channels at {rate} Hz");
            assert!(!slot.take_joined());
        }
    }

    #[test]
    fn a_song_that_ends_part_way_through_a_frame_is_not_followed() {
        let (a, b) = two();
        let slot = Arc::new(Slot::default());
        slot.put(song(2, 44_100, b));
        let out: Vec<f32> = Joined::new(song(2, 44_100, a[..1999].to_vec()), slot.clone()).collect();
        assert_eq!(out, a[..1999]);
    }

    #[test]
    fn a_song_taken_back_does_not_follow() {
        let (a, b) = two();
        let slot = Arc::new(Slot::default());
        let mut joined = Joined::new(song(2, 44_100, a.clone()), slot.clone());
        slot.put(song(2, 44_100, b));
        let mut out: Vec<f32> = joined.by_ref().take(500).collect();
        slot.clear();
        out.extend(joined);
        assert_eq!(out, a);
    }

    #[test]
    fn a_seek_moves_within_the_song_playing() {
        use rodio::Source;
        let slot = Arc::new(Slot::default());
        let mut joined = Joined::new(song(1, 1000, (0..2000).map(|i| i as f32).collect()), slot);
        joined.try_seek(Duration::from_millis(1500)).unwrap();
        assert_eq!(joined.next(), Some(1500.0));
    }
}
