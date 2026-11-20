//! Audio engine: one thread owns the output device and a rodio `Player`.
//!
//! The UI talks to it through `AudioHandle`; status flows back through `AudioStatus`
//! (lock-free reads for the progress bar) plus a `Finished` event for auto-advance.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::mixer::{Mixer, MixerSource};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

use crate::gapless::{Joined, Link, Slot};
use crate::speed::{Clock, Stretch};
use crate::stream::{self, Shared};

pub use crate::stream::TrackDecoder;

pub enum AudioCmd {
    /// A decoder is ready for playback generation `generation`; stale generations are dropped.
    /// A decoder started mid-song (`at` > 0) is the seekable kind.
    Start { generation: u64, decoder: TrackDecoder, shared: Arc<Shared>, at: Duration },
    /// A decoder positioned at `at`, replacing the current one; only the latest seek's lands.
    Replace { generation: u64, seek: u64, decoder: TrackDecoder, at: Duration, seekable: bool },
    /// Seamlessly upgrade the current stream to hi-res in place.
    Upgrade { generation: u64, decoder: TrackDecoder, shared: Arc<Shared>, at: Duration, seekable: bool },
    Pause,
    Resume,
    Stop,
    /// Another song is on its way: with crossfade on, fade this one out instead of
    /// stopping it, and fade the next one in when it starts.
    Handoff,
    /// The song after generation `after`, decoded ahead: it follows that song's last
    /// sample without a gap (see `crate::gapless`), unless something else is asked first.
    Standby { after: u64, decoder: TrackDecoder, shared: Arc<Shared> },
    /// Nothing follows the playing song on its own after all; a song that already has
    /// stops.
    Unstandby,
    /// The song that ended is followed by the one that stood by, whose `Start` is on
    /// its way: when it is playing already, it stays. Otherwise a `Stop`.
    Carry,
    Seek(Duration),
    /// Seek to where `pos` will have moved on to by the time the seek lands (`pos` was
    /// current at `at`): following someone else's playback (listen together).
    SeekLive { pos: Duration, at: std::time::Instant },
    Volume(f32),
    /// The output device failed (unplugged, headphones disconnected): sent by the
    /// device's error callback so the audio thread reopens one.
    DeviceLost,
    /// Play through this device (cpal's id) whenever it's connected; None follows the
    /// system default.
    SetDevice(Option<String>),
}

pub enum AudioEvent {
    Finished { generation: u64 },
    /// Close enough to the end that the next song should start, to crossfade into it.
    Ending { generation: u64 },
    /// The song's download failed, so playback stopped at `at` (or a seek to `at`
    /// couldn't be served). Reopening the song there resolves a fresh stream.
    Interrupted { generation: u64, at: Duration, reason: String },
    Error(String),
}

static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

fn now_ms() -> u64 {
    EPOCH.elapsed().as_millis() as u64
}

#[derive(Default)]
pub struct AudioStatus {
    pub position_ms: AtomicU64,
    /// When `position_ms` was sampled (ms since EPOCH), for smooth extrapolation.
    sampled_at: AtomicU64,
    pub duration_ms: AtomicU64,
    pub playing: AtomicBool,
    /// Generation in the player, 0 when none. `playing` on its own can still be the last
    /// song's for a moment after a stop, which is not the new one having started.
    pub playing_gen: AtomicU64,
    /// Generation the UI most recently asked for; the engine ignores older decoders.
    pub wanted_gen: AtomicU64,
    /// f32 bits
    volume: AtomicU32,
    /// Turn loud tracks down to YouTube's loudness target.
    pub normalize: AtomicBool,
    /// The volume slider follows loudness (cubic) rather than amplitude.
    log_volume: AtomicBool,
    /// How much of the volume the sleep timer's fade takes off (f32 bits; 0 = none).
    fade_cut: AtomicU32,
    /// Smoothed output loudness 0..1 (f32 bits), for audio-reactive visuals.
    level: AtomicU32,
    /// How far the output device runs behind the position (its buffer), in ms.
    latency_ms: AtomicU32,
    /// Crossfade length in ms; 0 = off.
    crossfade_ms: AtomicU32,
    /// DJ-style handoff rather than a straight-line crossfade (see `Ramp`).
    automix: AtomicBool,
    /// Spatial audio for headphones (see `crate::spatial`).
    spatial: AtomicBool,
    /// The equalizer's settings (see `crate::eq`), read by every song that plays.
    eq: crate::eq::Shared,
    /// The playback speed (see `crate::speed`), read by every song that plays.
    speed: Arc<crate::speed::Control>,
    /// The generation whose next song stands by to follow it without a gap; 0 when none.
    standby_for: AtomicU64,
    /// Tempo estimate of the playing song in BPM (f32 bits); 0 until one is heard.
    bpm: AtomicU32,
    mix_generation: AtomicU64,
    mix_duration_ms: AtomicU32,
    mix_active: AtomicBool,
    outgoing_position_ms: AtomicU64,
    outgoing_sampled_at: AtomicU64,
    outgoing_playing: AtomicBool,
}

impl AudioStatus {
    pub fn position(&self) -> Duration {
        Duration::from_millis(self.position_ms.load(Ordering::Relaxed))
    }

    /// Position extrapolated to now while playing, so progress moves smoothly between
    /// the engine's 100 ms updates.
    pub fn position_smooth(&self) -> Duration {
        let pos = self.position_ms.load(Ordering::Relaxed);
        if !self.playing.load(Ordering::Relaxed) {
            return Duration::from_millis(pos);
        }
        Duration::from_millis(pos + self.played_since(self.sampled_at.load(Ordering::Relaxed)))
    }

    pub fn outgoing_position(&self) -> Duration {
        let pos = self.outgoing_position_ms.load(Ordering::Relaxed);
        let age = if self.outgoing_playing() { self.played_since(self.outgoing_sampled_at.load(Ordering::Relaxed)) } else { 0 };
        Duration::from_millis(pos + age)
    }

    /// How many ms of a song play between `sampled` (ms since EPOCH) and now: the time
    /// gone by, at the speed it plays.
    fn played_since(&self, sampled: u64) -> u64 {
        (now_ms().saturating_sub(sampled).min(250) as f32 * self.speed()) as u64
    }

    /// The speed songs play at: 1 as recorded. Positions and lengths are the song's own
    /// time; this is how fast the clock on the wall goes through them.
    pub fn speed(&self) -> f32 {
        self.speed.speed()
    }

    /// The generation whose next song stands by to follow it without a gap; 0 when none.
    pub fn standby_for(&self) -> u64 {
        self.standby_for.load(Ordering::Relaxed)
    }

    pub fn outgoing_playing(&self) -> bool {
        self.outgoing_playing.load(Ordering::Relaxed)
    }

    fn sample_outgoing(&self, pos: Duration, playing: bool) {
        self.outgoing_position_ms.store(pos.as_millis() as u64, Ordering::Relaxed);
        self.outgoing_sampled_at.store(now_ms(), Ordering::Relaxed);
        self.outgoing_playing.store(playing, Ordering::Relaxed);
    }

    pub fn duration(&self) -> Option<Duration> {
        let d = self.duration_ms.load(Ordering::Relaxed);
        (d > 0).then(|| Duration::from_millis(d))
    }

    /// Automix's handoff window for a song of this length, in seconds of the song: how
    /// much of the end is given to the next song coming in. 0 when Automix is off.
    pub fn mix_secs(&self, length: Option<Duration>) -> f32 {
        if !self.automix.load(Ordering::Relaxed) {
            return 0.0;
        }
        mix_ms(self, length) as f32 / 1000.0 * self.speed()
    }

    /// How long the mix into `generation` runs, in seconds of the song.
    pub fn mix_duration(&self, generation: u64) -> Option<f32> {
        (self.mix_generation.load(Ordering::Acquire) == generation)
            .then(|| self.mix_duration_ms.load(Ordering::Relaxed) as f32 / 1000.0 * self.speed())
    }

    pub fn mixing(&self) -> bool {
        self.mix_active.load(Ordering::Relaxed)
    }

    /// How far what is heard runs behind the position, in the song's time.
    pub fn latency(&self) -> Duration {
        Duration::from_millis(self.latency_ms.load(Ordering::Relaxed) as u64).mul_f32(self.speed())
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    fn set_position(&self, ms: u64) {
        self.position_ms.store(ms, Ordering::Relaxed);
        self.sampled_at.store(now_ms(), Ordering::Relaxed);
    }
}

/// Sample-counted envelopes; Automix exchanges the lead at 65% of the blend.
#[derive(Clone, Copy)]
enum Ramp {
    In { at: u32, len: u32 },
    /// Down from `from` (a fade-in may have been cut short) to silence, then the source ends.
    Out { from: f32, at: u32, len: u32 },
    MixIn { at: u32, len: u32 },
    /// Same `from` as `Out`: the song may still be easing in when it's mixed out.
    MixOut { from: f32, at: u32, len: u32 },
}

/// A smoothstep from 0 to 1, slow at both ends.
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Ramp {
    fn gain(self) -> f32 {
        match self {
            Ramp::In { at, len } => at as f32 / len as f32,
            Ramp::Out { from, at, len } => from * (1.0 - at as f32 / len as f32),
            // The two Automix gains are the square roots of a mirrored pair, so their
            // powers sum to one: the level holds through the handoff instead of dipping.
            Ramp::MixIn { at, len } => mix_handoff(at as f32 / len as f32).sqrt(),
            Ramp::MixOut { from, at, len } => from * (1.0 - mix_handoff(at as f32 / len as f32)).sqrt(),
        }
    }
}

/// How far through an Automix handoff the songs are: nothing for its first 30% (the
/// outgoing song keeps the floor), then a smooth swap over the rest.
fn mix_handoff(x: f32) -> f32 {
    smoothstep((x - 0.3) / 0.7)
}

const AUTOMIX_FADE: u32 = 1 << 31;
const FX_MAX_FRAMES: usize = 48_000;

struct MixFx {
    ring: Vec<f32>,
    channels: usize,
    frame: usize,
    channel: usize,
    rate: f32,
    low: [f32; 8],
    body: [f32; 8],
    low_k: f32,
    body_k: f32,
    beat: f32,
    rhythmic: bool,
    bass: f32,
    treble: f32,
    wet: f32,
    flange: f32,
    tail: Option<(f32, bool)>,
    ticks: u64,
}

impl MixFx {
    fn new(rate: u32, channels: u16) -> Option<Self> {
        if channels > 8 {
            return None;
        }
        let frames = ((rate as usize * 3) / 4).clamp(2, FX_MAX_FRAMES);
        Some(Self {
            ring: vec![0.0; frames * channels as usize],
            channels: channels as usize,
            frame: 0,
            channel: 0,
            rate: rate as f32,
            low: [0.0; 8],
            body: [0.0; 8],
            low_k: 1.0 - (-std::f32::consts::TAU * 180.0 / rate as f32).exp(),
            body_k: 1.0 - (-std::f32::consts::TAU * 4000.0 / rate as f32).exp(),
            beat: 0.5 * rate as f32,
            rhythmic: false,
            bass: 1.0,
            treble: 1.0,
            wet: 0.0,
            flange: 0.0,
            tail: None,
            ticks: 0,
        })
    }

    fn begin(&mut self, bpm: f32) {
        self.rhythmic = (70.0..=180.0).contains(&bpm);
        self.beat = self.rate * if self.rhythmic { 60.0 / bpm } else { 0.5 };
        while self.beat > (self.ring.len() / self.channels - 1) as f32 {
            self.beat *= 0.5;
        }
        self.beat = self.beat.max(1.0);
        self.ticks = 0;
    }

    fn tap(&self, delay: f32) -> f32 {
        let frames = self.ring.len() / self.channels;
        let delay = delay.clamp(1.0, (frames - 1) as f32);
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let a = (self.frame + frames - whole) % frames;
        let b = (a + frames - 1) % frames;
        self.ring[a * self.channels + self.channel] * (1.0 - frac) + self.ring[b * self.channels + self.channel] * frac
    }

    fn process(&mut self, input: f32, ramp: Option<Ramp>) -> f32 {
        let c = self.channel;
        self.low[c] += self.low_k * (input - self.low[c]);
        self.body[c] += self.body_k * (input - self.body[c]);
        let blend = match ramp {
            Some(Ramp::MixIn { at, len }) => Some((at as f32 / len as f32, false)),
            Some(Ramp::MixOut { at, len, .. }) => Some((at as f32 / len as f32, true)),
            Some(Ramp::Out { .. }) => self.tail,
            _ => None,
        };
        self.tail = blend;
        let mut result = input;
        let mut history = input;
        if let Some((x, outgoing)) = blend {
            if c == 0 && self.ticks % 32 == 0 {
                let handoff = mix_handoff(x);
                self.bass = if outgoing { 1.0 - 0.85 * smoothstep((x - 0.35) / 0.3) } else { 0.15 + 0.85 * smoothstep((x - 0.58) / 0.25) };
                self.treble = if outgoing { 1.0 - 0.5 * handoff } else { 0.7 + 0.3 * handoff };
                self.wet = if outgoing { 0.4 * smoothstep((x - 0.3) / 0.35) } else { 0.0 };
                // The comb sweeps over two beats, fast enough to hear as a flanger.
                let phase = self.ticks as f32 / (self.beat * 2.0) * std::f32::consts::TAU;
                self.flange = self.rate * (0.0008 + 0.0037 * (0.5 + 0.5 * phase.sin()));
            }
            result = input + (self.bass - 1.0) * self.low[c] + (self.treble - 1.0) * (input - self.body[c]);
            if outgoing {
                // Dry plus two swept combs at an uneven ratio, so the notch family is
                // denser and no frequency sits dead. The right channel's combs sit a
                // quarter of the sweep farther back, which moves the image as they pass.
                let spread = (c as f32) * 0.25 * self.flange;
                let flanged = (input + self.tap(self.flange + spread) + self.tap(self.flange * 0.618 + spread * 0.5)) / 3.0;
                let tail = if self.rhythmic {
                    // A half-beat repeat stutters the last bar under the flanged comb.
                    0.45 * self.tap(self.beat * 0.5) + 0.55 * flanged
                } else {
                    let reverb = (self.tap(self.rate * 0.071) + self.tap(self.rate * 0.113) + self.tap(self.rate * 0.173)) / 3.0;
                    0.4 * reverb + 0.6 * flanged
                };
                history = input * 0.7 + tail * 0.3;
                result = result * (1.0 - self.wet) + tail * self.wet;
            }
        }
        self.ring[self.frame * self.channels + c] = history;
        self.channel += 1;
        if self.channel == self.channels {
            self.channel = 0;
            self.frame = (self.frame + 1) % (self.ring.len() / self.channels);
            self.ticks += 1;
        }
        result
    }

    fn reset(&mut self) {
        self.ring.fill(0.0);
        self.low.fill(0.0);
        self.body.fill(0.0);
        self.frame = 0;
        self.channel = 0;
        self.tail = None;
        self.ticks = 0;
    }
}

/// Pass-through source that tracks loudness (RMS over ~23 ms windows, smoothed),
/// equalizes the song as it comes off the decoder (at its own rate, so the song fading
/// out and the one coming in each have theirs) and applies crossfade ramps. `fade_out`
/// is set by the audio thread to the fade-out length in ms (0 = keep playing); a new
/// value restarts it from the current gain.
struct Meter<S> {
    inner: S,
    status: Arc<AudioStatus>,
    fade_out: Arc<AtomicU32>,
    seen_out: u32,
    ramp: Option<Ramp>,
    /// Automix's handoff curves rather than the straight-line fade.
    mix: bool,
    acc: f32,
    n: u32,
    smooth: f32,
    tempo: Tempo,
    analysis: Arc<OnceLock<Option<f32>>>,
    analysis_acc: f32,
    fx: Option<MixFx>,
    eq: crate::eq::Equalizer,
    spatial: crate::spatial::Spatial,
    /// The right ear's sample of a pair already worked out, for the next call.
    spatial_right: Option<f32>,
    /// This song's volume normalization (f32 bits), set by the audio thread: in the
    /// song rather than on the player, so that a song following another without a gap
    /// has its own from its first sample.
    gain: Arc<AtomicU32>,
    gain_now: f32,
}

/// How many samples go by between looks at the equalizer's settings.
const EQ_POLL: u32 = 256;

/// How many windows Automix reads for a beat before it settles for the plain length.
const TEMPO_PASSES: u8 = 4;

/// Rough tempo tracking for Automix's phrase length: the spacing that repeats most in
/// the music's loudness impulses over the last few seconds.
struct Tempo {
    prev: f32,
    hist: [f32; 300],
    count: usize,
    /// The song's tempo is settled; nothing more to analyse in it.
    done: bool,
    /// How many windows have been read without a beat in them.
    passes: u8,
    /// How many ms one loudness reading covers.
    hop_ms: f32,
}

impl Tempo {
    fn new(hop_ms: f32) -> Self {
        Self { prev: 0.0, hist: [0.0; 300], count: 0, done: false, passes: 0, hop_ms }
    }

    // Bounded: a few windows of roughly six seconds each, and it stops the moment one of
    // them answers, whether or not a reliable beat was found in it.
    fn push(&mut self, e: f32) -> Option<f32> {
        if self.done {
            return None;
        }
        let flux = (e - self.prev).max(0.0);
        self.prev = e;
        self.hist[self.count] = flux;
        self.count += 1;
        if self.count < self.hist.len() {
            return None;
        }
        if let Some(bpm) = self.estimate() {
            self.done = true;
            return Some(bpm);
        }
        // An intro that says nothing yet is not a song without a beat: try the next
        // window, and stop while the mix still has time to use the answer.
        self.passes += 1;
        if self.passes >= TEMPO_PASSES {
            self.done = true;
        } else {
            self.hist.fill(0.0);
            self.count = 0;
        }
        None
    }

    fn estimate(&self) -> Option<f32> {
        let n = self.hist.len();
        let mean = self.hist.iter().sum::<f32>() / n as f32;
        let power = self.hist.iter().map(|e| (e - mean).powi(2)).sum::<f32>() / n as f32;
        if power <= 1e-12 {
            return None;
        }
        // Beats of 0.30–1.20 s, with the middle of that range preferred so half- and
        // double-time patterns land on the tempo people would tap.
        let lo = ((0.30 * 1000.0 / self.hop_ms) as usize).max(4);
        let hi = ((1.20 * 1000.0 / self.hop_ms) as usize).min(n / 2);
        let mut best = (0.0f32, 0usize);
        for lag in lo..=hi {
            let c: f32 = (lag..n).map(|i| (self.hist[i] - mean) * (self.hist[i - lag] - mean)).sum::<f32>() / (n - lag) as f32;
            let secs = lag as f32 * self.hop_ms / 1000.0;
            let prior = (-(secs / 0.5).ln().powi(2) / (2.0 * 0.4 * 0.4)).exp();
            let score = c / power * prior;
            if score > best.0 {
                best = (score, lag);
            }
        }
        if best.0 < 0.08 {
            return None;
        }
        let mut bpm = 60_000.0 / (best.1 as f32 * self.hop_ms);
        while bpm < 70.0 {
            bpm *= 2.0;
        }
        while bpm > 180.0 {
            bpm /= 2.0;
        }
        Some(bpm)
    }
}

impl<S: rodio::Source> Meter<S> {
    fn new(inner: S, status: Arc<AudioStatus>, fade_in_ms: u32, fade_out: Arc<AtomicU32>, mix: bool, analysis: Arc<OnceLock<Option<f32>>>) -> Self {
        let hop_ms = 2048.0 / (inner.sample_rate().get() as f32 * inner.channels().get() as f32) * 1000.0;
        let mut tempo = Tempo::new(hop_ms);
        tempo.done = analysis.get().is_some();
        let mut fx = if status.automix.load(Ordering::Relaxed) { MixFx::new(inner.sample_rate().get(), inner.channels().get()) } else { None };
        if let Some(fx) = &mut fx {
            fx.begin(analysis.get().copied().flatten().unwrap_or(0.0));
        }
        let spatial = crate::spatial::Spatial::new(inner.sample_rate().get());
        let eq = crate::eq::Equalizer::new(status.eq.get(), inner.sample_rate().get(), inner.channels().get());
        let gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let mut m = Self { inner, status, fade_out, seen_out: 0, ramp: None, mix, acc: 0.0, n: 0, smooth: 0.0, tempo, analysis, analysis_acc: 0.0, fx, eq, spatial, spatial_right: None, gain, gain_now: 1.0 };
        if fade_in_ms > 0 {
            m.ramp = Some(if mix { Ramp::MixIn { at: 0, len: m.samples(fade_in_ms) } } else { Ramp::In { at: 0, len: m.samples(fade_in_ms) } });
        }
        m
    }

    /// Played at `gain` (f32 bits) of its level, wherever that is moved to.
    fn with_gain(mut self, gain: Arc<AtomicU32>) -> Self {
        self.gain_now = f32::from_bits(gain.load(Ordering::Relaxed));
        self.gain = gain;
        self
    }

    fn samples(&self, ms: u32) -> u32 {
        let per_sec = self.inner.sample_rate().get() as u64 * self.inner.channels().get() as u64;
        (per_sec * ms as u64 / 1000).max(1) as u32
    }

    /// The decoder's next sample, equalized.
    fn pull(&mut self) -> Option<f32> {
        let s = self.inner.next()?;
        Some(self.eq.process(s))
    }
}

impl<S: rodio::Source> Iterator for Meter<S> {
    type Item = rodio::Sample;
    fn next(&mut self) -> Option<Self::Item> {
        let out = self.fade_out.load(Ordering::Relaxed);
        if out != self.seen_out {
            self.seen_out = out;
            let from = self.ramp.map_or(1.0, Ramp::gain);
            let len = self.samples(out & !AUTOMIX_FADE);
            self.mix = out & AUTOMIX_FADE != 0;
            if self.mix && let Some(fx) = &mut self.fx {
                fx.begin(self.analysis.get().copied().flatten().unwrap_or(0.0));
            }
            self.ramp = Some(if self.mix { Ramp::MixOut { from, at: 0, len } } else { Ramp::Out { from, at: 0, len } });
        }
        if let Some(Ramp::Out { at, len, .. } | Ramp::MixOut { at, len, .. }) = self.ramp
            && at >= len
        {
            return None;
        }
        // The equalizer hears of a change a few milliseconds later, and glides to it.
        if self.n.is_multiple_of(EQ_POLL) {
            self.eq.set(self.status.eq.get());
            self.gain_now = f32::from_bits(self.gain.load(Ordering::Relaxed));
        }
        let mut s = match self.spatial_right.take() {
            Some(right) => right,
            None => {
                let left = self.pull()?;
                // Stereo with spatial audio on: the pair is worked out together.
                if self.status.spatial.load(Ordering::Relaxed) && self.inner.channels().get() == 2 {
                    match self.pull() {
                        Some(right) => {
                            let (l, r) = self.spatial.frame(left, right);
                            self.spatial_right = Some(r);
                            l
                        }
                        None => left,
                    }
                } else {
                    left
                }
            }
        };
        if !self.tempo.done {
            self.analysis_acc += s * s;
        }
        if let Some(fx) = &mut self.fx {
            s = fx.process(s, self.ramp);
        }
        if let Some(ramp) = &mut self.ramp {
            s *= ramp.gain();
            match ramp {
                Ramp::In { at, len } | Ramp::MixIn { at, len } => {
                    *at += 1;
                    if *at >= *len {
                        self.ramp = None;
                    }
                }
                Ramp::Out { at, .. } | Ramp::MixOut { at, .. } => *at += 1,
            }
        }
        self.acc += s * s;
        self.n += 1;
        if self.n == 2048 {
            // As loud as the song is before the equalizer's headroom comes off it: the
            // visuals don't dim because a boost needed room.
            let rms = (self.acc / 2048.0).sqrt() / self.eq.headroom_gain();
            // Fast attack, slow release, like a VU meter.
            let k = if rms > self.smooth { 0.6 } else { 0.08 };
            self.smooth += (rms - self.smooth) * k;
            // A song fading out is no longer the one the visuals follow.
            if self.seen_out == 0 {
                self.status.level.store(self.smooth.min(1.0).to_bits(), Ordering::Relaxed);
                if self.status.automix.load(Ordering::Relaxed) && !self.tempo.done {
                    let bpm = self.tempo.push((self.analysis_acc / 2048.0).sqrt());
                    if self.tempo.done {
                        let _ = self.analysis.set(bpm);
                        self.status.bpm.store(bpm.unwrap_or(0.0).to_bits(), Ordering::Relaxed);
                        log::debug!("audio: tempo analysis complete: {bpm:?}");
                    }
                }
            }
            // A decoder can go on at another rate or channel count part way through a file.
            self.eq.set_format(self.inner.sample_rate().get(), self.inner.channels().get());
            self.acc = 0.0;
            self.analysis_acc = 0.0;
            self.n = 0;
        }
        Some(s * self.gain_now)
    }
}

impl<S: rodio::Source> rodio::Source for Meter<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)?;
        self.spatial_right = None;
        self.spatial.reset();
        self.eq.reset();
        self.ramp = None;
        self.acc = 0.0;
        self.analysis_acc = 0.0;
        self.n = 0;
        if let Some(fx) = &mut self.fx {
            fx.reset();
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<AudioCmd>,
    pub status: Arc<AudioStatus>,
}

impl AudioHandle {
    pub fn send(&self, cmd: AudioCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.status.volume.load(Ordering::Relaxed))
    }

    pub fn set_normalize(&self, on: bool) {
        self.status.normalize.store(on, Ordering::Relaxed);
        self.send(AudioCmd::Volume(self.volume()));
    }

    /// Switches the slider's curve, moving it so the music stays as loud as it was.
    pub fn set_log_volume(&self, on: bool) {
        if self.status.log_volume.swap(on, Ordering::Relaxed) != on {
            let v = self.volume();
            self.status.volume.store(if on { v.cbrt() } else { v.powi(3) }.to_bits(), Ordering::Relaxed);
        }
        self.send(AudioCmd::Volume(self.volume()));
    }

    pub fn set_crossfade(&self, secs: u32) {
        self.status.crossfade_ms.store(secs * 1000, Ordering::Relaxed);
    }

    /// Automix's handoff shape for the crossfade (see `Ramp::MixIn`).
    pub fn set_automix(&self, on: bool) {
        self.status.automix.store(on, Ordering::Relaxed);
    }

    /// Spatial audio for headphones, from the next sound on.
    pub fn set_spatial(&self, on: bool) {
        self.status.spatial.store(on, Ordering::Relaxed);
    }

    /// The equalizer, for the songs playing and every one after: they glide to it.
    pub fn set_equalizer(&self, settings: &crate::eq::Settings) {
        self.status.eq.set(settings);
    }

    /// The speed songs play at, and whether they keep their pitch at it: for the songs
    /// playing, from where they are, and every one after.
    pub fn set_speed(&self, speed: f32, keep_pitch: bool) {
        self.status.speed.set(speed, keep_pitch);
    }

    pub fn set_volume(&self, v: f32) {
        self.status.volume.store(v.to_bits(), Ordering::Relaxed);
        self.send(AudioCmd::Volume(v));
    }

    /// The sleep timer's fade: play at `level` (0..1) of the listener's volume, which
    /// stays where they set it.
    pub fn set_fade(&self, level: f32) {
        let cut = 1.0 - level.clamp(0.0, 1.0);
        if self.status.fade_cut.swap(cut.to_bits(), Ordering::Relaxed) != cut.to_bits() {
            self.send(AudioCmd::Volume(self.volume()));
        }
    }
}

/// Volume normalization the way YouTube Music does it: tracks mastered louder than
/// the -14 LUFS target are turned down by the difference; quieter ones are left alone
/// (boosting would clip).
/// A seek that needs a new decoder built over the stream buffer (off the audio thread).
struct SeekJob {
    shared: Arc<Shared>,
    pos: Duration,
    /// For a live seek (a jam): the host position and when it was, to catch up after the build.
    live: Option<(Duration, std::time::Instant)>,
    generation: u64,
    seek: u64,
}

/// Build a decoder for `j` (it may wait for the download) and hand it to the audio thread.
fn build_seek(j: SeekJob, tx: &Sender<AudioCmd>, events: &Sender<AudioEvent>) {
    let SeekJob { shared, pos, live, generation, seek } = j;
    let seekable = !pos.is_zero();
    let res = stream::decoder(shared.clone(), seekable).map_err(|e| e.to_string()).and_then(|mut d| {
        // Building it may have waited for the download: catch up.
        let pos = live.map_or(pos, |(p, at)| p + at.elapsed());
        if seekable {
            rodio::Source::try_seek(&mut d, pos).map_err(|e| e.to_string())?;
        }
        Ok((d, pos))
    });
    match res {
        Ok((decoder, pos)) => {
            let _ = tx.send(AudioCmd::Replace { generation, seek, decoder, at: pos, seekable });
        }
        Err(e) => match shared.error() {
            Some(reason) => {
                let _ = events.send(AudioEvent::Interrupted { generation, at: pos, reason });
            }
            None => {
                let _ = events.send(AudioEvent::Error(format!("Seek failed: {e}")));
            }
        },
    }
}

/// A song as a player plays it: at the listener's speed, metered and equalized.
type Song = Meter<Stretch<TrackDecoder>>;

/// The audio thread's hold on a song it gave a player: its fade-out control (see
/// `Meter`), where in the song the sound is, and its volume normalization.
#[derive(Clone)]
struct Deck {
    fade_out: Arc<AtomicU32>,
    clock: Arc<Clock>,
    gain: Arc<AtomicU32>,
}

impl Default for Deck {
    fn default() -> Self {
        Self { fade_out: Arc::default(), clock: Arc::default(), gain: Arc::new(AtomicU32::new(1.0f32.to_bits())) }
    }
}

/// `decoder` as a player plays it, and the audio thread's hold on it. Its running out
/// is the song's end unless it was told to fade out or its download failed.
fn song(decoder: TrackDecoder, status: &Arc<AudioStatus>, fade_in_ms: u32, mix: bool, shared: &Arc<Shared>) -> (Link<Song>, Deck) {
    let stretch = Stretch::new(decoder, status.speed.clone());
    let deck = Deck { fade_out: Arc::default(), clock: stretch.clock(), gain: Arc::new(AtomicU32::new(norm_gain(status, Some(shared)).to_bits())) };
    let source = Meter::new(stretch, status.clone(), fade_in_ms, deck.fade_out.clone(), mix, shared.tempo.clone()).with_gain(deck.gain.clone());
    let (out, stream) = (deck.fade_out.clone(), shared.clone());
    (Link { source, ended_well: Box::new(move || out.load(Ordering::Relaxed) == 0 && stream.error().is_none()) }, deck)
}

/// The next song, decoded ahead: standing by to follow the playing one without a gap,
/// or already following it.
struct Standby {
    shared: Arc<Shared>,
    deck: Deck,
}

/// A song fading out under the next one, kept until it's silent.
struct Fading {
    player: Player,
    offset: Duration,
    shared: Option<Arc<Shared>>,
    deck: Deck,
    /// Still at full volume, waiting for the next song to start (as Morphe does): when
    /// that was asked for, and how much of this song was left then.
    waiting: Option<(Instant, Option<Duration>)>,
    /// The song ended on its own rather than being skipped: this handoff gets the whole
    /// musical phrase.
    natural: bool,
}

impl Fading {
    /// Start the fade-out, over what's left of the song if that's shorter. Returns its length.
    fn start(&mut self, crossfade: u32, automix: bool) -> u32 {
        let left = self.waiting.take().and_then(|(at, left)| Some(left?.saturating_sub(at.elapsed())));
        let len = crossfade.min(left.map_or(u32::MAX, |l| l.as_millis() as u32)).max(QUICK_FADE_MS);
        self.deck.fade_out.store(len | if automix { AUTOMIX_FADE } else { 0 }, Ordering::Relaxed);
        len
    }
}

impl Drop for Fading {
    fn drop(&mut self) {
        if let Some(s) = &self.shared {
            s.cancel();
        }
    }
}

/// Fade-outs shorter than this would click: a crossfade cut short by another skip, or
/// one too close to the end of the song.
const QUICK_FADE_MS: u32 = 40;

/// How long a song keeps playing at full volume for the next one to load before it
/// fades out anyway.
const HANDOFF_WAIT: Duration = Duration::from_secs(3);

/// How long a song that followed another without a gap plays on unclaimed: its `Start`
/// comes within moments, or something went wrong and it stops.
const CARRY_WAIT: Duration = Duration::from_secs(10);

/// Automix counts its mix in bars of four beats: eight bars normally, sixteen on a song
/// long enough to hold them, and never more than 32 — nor longer than this, or than a
/// third of the song.
const AUTOMIX_BARS: f32 = 8.0;
const AUTOMIX_BARS_LONG: f32 = 16.0;
const AUTOMIX_MAX_BARS: f32 = 32.0;
const AUTOMIX_MAX_MS: u32 = 45_000;
const AUTOMIX_MIN_MS: u32 = 4_000;

/// How long the mix runs, in ms as heard. Automix works it out from the bars and the
/// tempo it hears in the outgoing song; a plain crossfade uses its setting.
fn mix_ms(status: &AudioStatus, length: Option<Duration>) -> u32 {
    // How long the song takes to play, at the speed it plays.
    let length = length.map(|d| d.div_f32(status.speed()));
    let crossfade = status.crossfade_ms.load(Ordering::Relaxed);
    if !status.automix.load(Ordering::Relaxed) {
        return crossfade;
    }
    let bpm = f32::from_bits(status.bpm.load(Ordering::Relaxed));
    if !(40.0..=240.0).contains(&bpm) {
        // Nothing to count bars with yet: the crossfade length stands in.
        return crossfade;
    }
    let bars = if length.is_some_and(|d| d >= Duration::from_secs(240)) { AUTOMIX_BARS_LONG } else { AUTOMIX_BARS };
    let ms = (bars.min(AUTOMIX_MAX_BARS) * 4.0 * 60_000.0 / bpm) as u32;
    let ms = ms.clamp(AUTOMIX_MIN_MS, AUTOMIX_MAX_MS);
    // A short song saves room for its own mix: a third of it at most.
    length.map_or(ms, |d| ms.min((d.as_millis() as u32 / 3).max(QUICK_FADE_MS)))
}

/// Gain for a volume slider position. Ears hear amplitude roughly logarithmically,
/// so on a linear slider nearly all the change is in its bottom few pixels; the cubic
/// curve (PulseAudio's) spreads it out: half way is -18 dB. The sleep timer's fade
/// comes off the result.
fn slider_gain(status: &AudioStatus, v: f32) -> f32 {
    let gain = if status.log_volume.load(Ordering::Relaxed) { v.powi(3) } else { v };
    gain * (1.0 - f32::from_bits(status.fade_cut.load(Ordering::Relaxed)))
}

fn norm_gain(status: &AudioStatus, shared: Option<&Shared>) -> f32 {
    match shared.and_then(|s| s.loudness_db) {
        Some(db) if db > 0.0 && status.normalize.load(Ordering::Relaxed) => 10f32.powf(-db / 20.0),
        _ => 1.0,
    }
}

/// The output device. Players play into a mixer of our own that the device pulls
/// from, so when the device goes away (Bluetooth headphones disconnecting) or the
/// default changes (them reconnecting), a new one carries on from the same samples
/// without touching the players or the songs in them.
struct Output {
    mixer: Mixer,
    /// Our mixer's output while no device has it; the device's `Bridge` holds it.
    slot: Arc<std::sync::Mutex<Option<MixerSource>>>,
    sink: Option<MixerDeviceSink>,
    /// The device in use (cpal's id), to notice the default changing.
    device: Option<String>,
    /// The device the user picked; the default stands in while it's not connected.
    preferred: Option<String>,
    /// Set by the device's error callback.
    lost: Arc<AtomicBool>,
    checked: Instant,
    /// No device is ever opened: a test pulls the mixer's samples itself.
    #[cfg(test)]
    headless: bool,
}

impl Output {
    fn new(tx: Sender<AudioCmd>, status: &AudioStatus, preferred: Option<String>) -> Self {
        // Mix at the device's own format, so the usual case needs no conversion.
        let config = target(preferred.as_deref()).and_then(|d| d.default_output_config().ok());
        let channels = config.as_ref().and_then(|c| std::num::NonZero::new(c.channels())).unwrap_or(std::num::NonZero::new(2).unwrap());
        let rate = config.as_ref().and_then(|c| std::num::NonZero::new(c.sample_rate())).unwrap_or(std::num::NonZero::new(48_000).unwrap());
        let (mixer, source) = rodio::mixer::mixer(channels, rate);
        let lost = Arc::new(AtomicBool::new(false));
        let mut out = Self {
            mixer,
            slot: Arc::new(std::sync::Mutex::new(Some(source))),
            sink: None,
            device: None,
            preferred,
            lost,
            checked: Instant::now(),
            #[cfg(test)]
            headless: false,
        };
        out.open(tx, status);
        out
    }

    /// A 48 kHz stereo mixer no device plays, and its output for the test to pull.
    #[cfg(test)]
    fn headless() -> (Self, MixerSource) {
        let (mixer, source) = rodio::mixer::mixer(std::num::NonZero::new(2).unwrap(), std::num::NonZero::new(48_000).unwrap());
        (Self { mixer, slot: Arc::default(), sink: None, device: None, preferred: None, lost: Arc::default(), checked: Instant::now(), headless: true }, source)
    }

    /// Opens the picked device, or the default one. False if there's none to open.
    fn open(&mut self, tx: Sender<AudioCmd>, status: &AudioStatus) -> bool {
        // The old device's stream gives our mixer back when it's dropped.
        self.sink = None;
        self.device = None;
        self.checked = Instant::now();
        #[cfg(test)]
        if self.headless {
            return false;
        }
        let Some(device) = target(self.preferred.as_deref()) else {
            return false;
        };
        let id = device.id().map(|i| i.to_string()).ok();
        let lost = self.lost.clone();
        lost.store(false, Ordering::Relaxed);
        let on_error = move |e: rodio::cpal::StreamError| {
            log::warn!("audio: output device error: {e}");
            if !lost.swap(true, Ordering::Relaxed) {
                let _ = tx.send(AudioCmd::DeviceLost);
            }
        };
        let sink = match DeviceSinkBuilder::from_device(device).and_then(|b| b.with_error_callback(on_error).open_sink_or_fallback()) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("audio: couldn't open the output device: {e}");
                return false;
            }
        };
        // A stream that failed on its own hands the mixer back as its thread exits,
        // which can be just after it reported the error.
        let waited = Instant::now();
        let source = loop {
            if let Some(source) = self.slot.lock().unwrap().take() {
                break source;
            }
            if waited.elapsed() > Duration::from_secs(1) {
                log::warn!("audio: mixer still held by the old device");
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let mut sink = sink;
        sink.log_on_drop(false);
        let channels = rodio::Source::channels(&source).get();
        sink.mixer().add(Bridge { inner: Some(source), slot: self.slot.clone(), channels, n: 0 });
        // The position counts samples handed to the device; it plays them a buffer later
        // (rodio asks for ~50 ms), plus the shared-mode device period (~10 ms).
        let rate = sink.config().sample_rate().get().max(1);
        let buffer_ms = match sink.config().buffer_size() {
            rodio::cpal::BufferSize::Fixed(n) => *n as u64 * 1000 / rate as u64,
            rodio::cpal::BufferSize::Default => 0,
        };
        status.latency_ms.store((buffer_ms + 10) as u32, Ordering::Relaxed);
        log::info!("audio: output device {:?} at {} Hz", id, rate);
        self.sink = Some(sink);
        self.device = id;
        true
    }

    /// Whether the device to play through (the picked one if it's connected, else the
    /// default) differs from the one in use.
    fn default_changed(&mut self) -> bool {
        self.checked = Instant::now();
        let want = target(self.preferred.as_deref()).and_then(|d| d.id().ok()).map(|i| i.to_string());
        want.is_some() && want != self.device
    }
}

/// The picked device if it's connected, else the system default.
fn target(preferred: Option<&str>) -> Option<rodio::cpal::Device> {
    let host = rodio::cpal::default_host();
    if let Some(want) = preferred
        && let Some(d) = host.output_devices().ok().and_then(|mut all| all.find(|d| d.id().is_ok_and(|i| i.to_string() == want)))
    {
        return Some(d);
    }
    host.default_output_device()
}

/// An output device to offer in settings.
#[derive(Clone, PartialEq)]
pub struct OutputDevice {
    /// cpal's id: stable across reconnects.
    pub id: String,
    pub name: String,
}

/// The connected output devices, by the names Windows' sound settings show.
pub fn output_devices() -> Vec<OutputDevice> {
    let Ok(all) = rodio::cpal::default_host().output_devices() else { return Vec::new() };
    let mut out: Vec<OutputDevice> = all
        .filter_map(|d| {
            let id = d.id().ok()?.to_string();
            let desc = d.description().ok()?;
            // "Speakers (Realtek(R) Audio)" rather than just "Speakers".
            let name = desc.extended().first().cloned().unwrap_or_else(|| desc.name().to_owned());
            Some(OutputDevice { id, name })
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Our mixer's output, as a source in the device's mixer. Never ends, so the device
/// keeps it; gives the mixer back when the device's stream drops it.
struct Bridge {
    inner: Option<MixerSource>,
    slot: Arc<std::sync::Mutex<Option<MixerSource>>>,
    channels: u16,
    /// Samples handed out, to give the mixer back on a frame boundary (else the next
    /// device would get the channels swapped).
    n: u64,
}

impl Iterator for Bridge {
    type Item = rodio::Sample;
    fn next(&mut self) -> Option<Self::Item> {
        self.n += 1;
        Some(self.inner.as_mut().and_then(Iterator::next).unwrap_or(0.0))
    }
}

impl rodio::Source for Bridge {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.as_ref().map_or(std::num::NonZero::new(self.channels.max(1)).unwrap(), |s| s.channels())
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.as_ref().map_or(std::num::NonZero::new(48_000).unwrap(), |s| s.sample_rate())
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if let Some(mut source) = self.inner.take() {
            while !self.n.is_multiple_of(self.channels.max(1) as u64) {
                source.next();
                self.n += 1;
            }
            *self.slot.lock().unwrap() = Some(source);
        }
    }
}

/// How often to look for a new default device while playing.
const DEVICE_CHECK: Duration = Duration::from_secs(1);

pub fn spawn(volume: f32, log_volume: bool, device: Option<String>, events: Sender<AudioEvent>, wake: impl Fn() + Send + 'static) -> AudioHandle {
    let (tx, rx) = unbounded();
    let status = Arc::new(AudioStatus::default());
    status.volume.store(volume.to_bits(), Ordering::Relaxed);
    status.log_volume.store(log_volume, Ordering::Relaxed);
    let st = status.clone();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn({
            let tx = tx.clone();
            move || run(rx, tx, st, volume, device, events, wake)
        })
        .expect("spawn audio thread");
    AudioHandle { tx, status }
}

fn run(
    rx: Receiver<AudioCmd>,
    tx: Sender<AudioCmd>,
    status: Arc<AudioStatus>,
    volume: f32,
    device: Option<String>,
    events: Sender<AudioEvent>,
    wake: impl Fn(),
) {
    let output = Output::new(tx.clone(), &status, device);
    if output.sink.is_none() {
        let _ = events.send(AudioEvent::Error("No audio output device".into()));
        wake();
    }
    play(rx, tx, status, volume, output, events, wake)
}

/// The audio thread's loop: the commands it is sent, and every 100 ms while a song
/// plays, where that has got to.
fn play(rx: Receiver<AudioCmd>, tx: Sender<AudioCmd>, status: Arc<AudioStatus>, volume: f32, mut output: Output, events: Sender<AudioEvent>, wake: impl Fn()) {
    let mut player = Player::connect_new(&output.mixer);
    player.set_volume(slider_gain(&status, volume));
    // The playing song (see `Deck`).
    let mut deck = Deck::default();
    // Where the next song stands by to follow the playing one without a gap (see
    // `crate::gapless`), the audio thread's hold on it, and the one that has followed
    // and plays until its `Start` claims it.
    let slot: Arc<Slot<Song>> = Arc::default();
    let mut standby: Option<Standby> = None;
    let mut carried: Option<(Standby, Instant)> = None;
    // Songs fading out under the current one (more than one only for a moment).
    let mut fading: Vec<Fading> = Vec::new();
    // Whether `Ending` went out for this generation.
    let mut ending_sent = false;
    // The playing song's length. Not `status.duration()` at a handoff: the UI has
    // already put the next song's there.
    let mut length: Option<Duration> = None;

    // Generation of the source currently in the player (0 = nothing loaded).
    let mut current: u64 = 0;
    let mut shared: Option<Arc<Shared>> = None;
    // The player counts position from when a source was appended; a swapped-in
    // seeked decoder starts at `offset`.
    let mut offset = Duration::ZERO;
    // Whether the playing decoder can seek in place (it indexed the whole file).
    let mut seekable = false;
    // Latest seek; a slower, older seek's decoder must not land after a newer one.
    let mut seek_seq: u64 = 0;
    let seek_job: Arc<std::sync::Mutex<Option<SeekJob>>> = Default::default();
    let seek_busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Dev hook: YTM_DEVICE_LOST=<secs> acts as if the output device disconnected that
    // long into the first song (the error callback firing for a device that's gone).
    let mut dev_lost = std::env::var("YTM_DEVICE_LOST").ok().and_then(|v| v.parse::<f32>().ok()).map(Duration::from_secs_f32);
    loop {
        // Poll while playing so the position stays fresh and the end is noticed; idle otherwise.
        let timeout = if (current != 0 || !fading.is_empty() || carried.is_some()) && !player.is_paused() { Duration::from_millis(100) } else { Duration::from_secs(3600) };
        let received = rx.recv_timeout(timeout);
        // The playing song ran into the one standing by: that song is over, as far as
        // anyone is told, and the next one plays on until its `Start` claims it.
        if slot.take_joined()
            && let Some(next) = standby.take()
        {
            log::debug!("audio: generation {current} ran into the next song without a gap");
            let _ = events.send(AudioEvent::Finished { generation: current });
            carried = Some((next, Instant::now()));
            current = 0;
            status.playing_gen.store(0, Ordering::Relaxed);
            status.standby_for.store(0, Ordering::Relaxed);
            shared = None;
            wake();
        }
        match received {
            Ok(AudioCmd::Start { generation, decoder, shared: s, at }) => {
                if generation != status.wanted_gen.load(Ordering::Relaxed) {
                    log::debug!("audio: dropping stale generation {generation}");
                    continue;
                }
                log::debug!("audio: start generation {generation}");
                let followed = carried.take().filter(|(c, _)| at.is_zero() && Arc::ptr_eq(&c.shared, &s));
                let gain = norm_gain(&status, Some(&s));
                log::info!("audio: loudness {:?} dB, normalization gain {gain:.3}", s.loudness_db);
                let mut fade_in = 0;
                if let Some((next, _)) = followed {
                    // Playing already: it followed the last song without a gap, and goes
                    // on as it is. The decoder opened for this start isn't needed.
                    log::debug!("audio: generation {generation} is the song that followed without a gap");
                    deck = next.deck;
                    deck.gain.store(gain.to_bits(), Ordering::Relaxed);
                } else {
                    if output.sink.is_none() || output.default_changed() {
                        output.open(tx.clone(), &status);
                    }
                    slot.clear();
                    player.clear();
                    slot.take_joined();
                    standby = None;
                    status.standby_for.store(0, Ordering::Relaxed);
                    player.set_volume(slider_gain(&status, f32::from_bits(status.volume.load(Ordering::Relaxed))));
                    // Fade in only over a song still playing, both over the same length.
                    fading.retain(|f| {
                        if f.player.empty() {
                            if let Some(s) = &f.shared {
                                s.cancel();
                            }
                            false
                        } else {
                            true
                        }
                    });
                    let skip_len = status.crossfade_ms.load(Ordering::Relaxed);
                    let natural_len = mix_ms(&status, length);
                    for f in fading.iter_mut().filter(|f| f.waiting.is_some()) {
                        // A song that ended on its own gets the full phrase; a skip stays quick.
                        fade_in = f.start(if f.natural { natural_len } else { skip_len }, status.automix.load(Ordering::Relaxed));
                        log::debug!("audio: crossfade into generation {generation} over {fade_in} ms");
                    }
                    let mix = fade_in > 0 && status.automix.load(Ordering::Relaxed);
                    let (source, new) = song(decoder, &status, fade_in, mix, &s);
                    deck = new;
                    player.append(Joined::new(source, slot.clone()));
                    player.play();
                }
                let mix = fade_in > 0 && status.automix.load(Ordering::Relaxed);
                status.bpm.store(s.tempo.get().copied().flatten().unwrap_or(0.0).to_bits(), Ordering::Relaxed);
                status.mix_duration_ms.store(fade_in, Ordering::Relaxed);
                status.mix_active.store(mix, Ordering::Relaxed);
                status.mix_generation.store(generation, Ordering::Release);
                // 0 means the stream didn't say; keep the length the UI already knows.
                if s.duration_ms > 0 {
                    status.duration_ms.store(s.duration_ms, Ordering::Relaxed);
                }
                length = status.duration();
                if let Some(old) = shared.replace(s.clone())
                    && !Arc::ptr_eq(&old, &s)
                {
                    old.cancel();
                }
                current = generation;
                status.playing_gen.store(generation, Ordering::Relaxed);
                offset = at;
                seekable = !at.is_zero();
                ending_sent = false;
                // The new song's needle, claimed before `playing`: the first sample from a
                // fresh decoder is a moment away, and until then the reader — the progress
                // bar, the party's announce — would take the song that just ended for this one.
                // (A song that followed without a gap is as far in as it has played.)
                status.set_position((at + deck.clock.get()).as_millis() as u64);
                status.playing.store(!player.is_paused(), Ordering::Relaxed);
            }
            Ok(AudioCmd::Replace { generation, seek, decoder, at, seekable: s }) => {
                if generation != current || seek != seek_seq {
                    continue;
                }
                log::debug!("audio: seek to {at:?} via new decoder (seekable: {s})");
                seekable = s;
                let paused = player.is_paused();
                player.clear();
                // (The song standing by stays: this is the same song, somewhere else in it.)
                let (source, new) = song(decoder, &status, 0, false, shared.as_ref().expect("a current generation has a stream"));
                deck = new;
                player.append(Joined::new(source, slot.clone()));
                if !paused {
                    player.play();
                }
                offset = at;
            }
            Ok(AudioCmd::Upgrade { generation, decoder, shared: s, at, seekable: s_seekable }) => {
                if generation != current || generation != status.wanted_gen.load(Ordering::Relaxed) {
                    s.cancel();
                    continue;
                }
                log::debug!("audio: upgrade generation {generation} to hi-res stream at {at:?}");
                seek_seq += 1;
                if output.sink.is_none() || output.default_changed() {
                    output.open(tx.clone(), &status);
                }
                let paused = player.is_paused();
                if !paused && !player.empty() {
                    // (Fading out, it isn't followed by the song standing by: the new copy is.)
                    deck.fade_out.store(QUICK_FADE_MS, Ordering::Relaxed);
                    let old = std::mem::replace(&mut player, Player::connect_new(&output.mixer));
                    fading.push(Fading { player: old, offset, shared: shared.take(), deck: deck.clone(), waiting: None, natural: false });
                } else {
                    player.clear();
                    if let Some(old) = shared.take() {
                        old.cancel();
                    }
                }
                let gain = norm_gain(&status, Some(&*s));
                log::info!("audio: hi-res loudness {:?} dB, normalization gain {gain:.3}", s.loudness_db);
                player.set_volume(slider_gain(&status, f32::from_bits(status.volume.load(Ordering::Relaxed))));
                let fade_in = if !paused { QUICK_FADE_MS } else { 0 };
                status.bpm.store(s.tempo.get().copied().flatten().unwrap_or(0.0).to_bits(), Ordering::Relaxed);
                let (source, new) = song(decoder, &status, fade_in, false, &s);
                deck = new;
                player.append(Joined::new(source, slot.clone()));
                if !paused {
                    player.play();
                }
                if s.duration_ms > 0 {
                    status.duration_ms.store(s.duration_ms, Ordering::Relaxed);
                }
                length = status.duration();
                shared = Some(s);
                offset = at;
                seekable = s_seekable;
                ending_sent = false;
                status.set_position(at.as_millis() as u64);
            }
            Ok(AudioCmd::Pause) => {
                log::debug!("audio: pause at {:?}", offset + deck.clock.get());
                player.pause();
                fading.iter().for_each(|f| f.player.pause());
                status.playing.store(false, Ordering::Relaxed);
                // So the UI (and the system media controls it syncs) see it even when idle.
                wake();
            }
            Ok(AudioCmd::Resume) => {
                log::debug!("audio: resume (generation {current})");
                if current != 0 {
                    // Headphones may have come back (or gone) while paused.
                    if output.sink.is_none() || output.default_changed() {
                        output.open(tx.clone(), &status);
                    }
                    player.play();
                    fading.iter().for_each(|f| f.player.play());
                    status.playing.store(true, Ordering::Relaxed);
                    wake();
                }
            }
            // One crossfade at a time: skipping again mid-fade cuts over (below) instead
            // of piling a third song on.
            Ok(AudioCmd::Handoff)
                if mix_ms(&status, length) > 0
                    && current != 0
                    && !player.is_paused()
                    && !player.empty()
                    && fading.iter().all(|f| f.player.empty()) =>
            {
                // Keeps playing at full volume until the next song starts (see `Start`):
                // for as long as what is left of it takes at this speed.
                let left = length.map(|d| d.saturating_sub(offset + deck.clock.get()).div_f32(status.speed()));
                log::debug!("audio: handing off from generation {current}, {left:?} left");
                // The two songs blend: nothing follows this one on its own.
                slot.clear();
                slot.take_joined();
                standby = None;
                status.standby_for.store(0, Ordering::Relaxed);
                let old = std::mem::replace(&mut player, Player::connect_new(&output.mixer));
                fading.push(Fading { player: old, offset, shared: shared.take(), deck: deck.clone(), waiting: Some((Instant::now(), left)), natural: ending_sent });
                current = 0;
                status.playing_gen.store(0, Ordering::Relaxed);
                status.playing.store(false, Ordering::Relaxed);
                status.set_position(0);
            }
            // The song that stood by is playing already, and its `Start` is on its way.
            Ok(AudioCmd::Carry) if carried.is_some() => {}
            Ok(cmd @ (AudioCmd::Stop | AudioCmd::Handoff | AudioCmd::Carry)) => {
                slot.clear();
                if matches!(cmd, AudioCmd::Handoff) {
                    // Skipping again mid-crossfade: everything playing goes, quickly enough
                    // to sound like a cut but without a click, and the next song comes in
                    // at full volume.
                    for f in fading.iter_mut() {
                        f.waiting = None;
                        f.deck.fade_out.store(QUICK_FADE_MS, Ordering::Relaxed);
                    }
                    log::debug!("audio: skipped mid-crossfade, cutting over");
                    if current != 0 && !player.is_paused() && !player.empty() {
                        deck.fade_out.store(QUICK_FADE_MS, Ordering::Relaxed);
                        let old = std::mem::replace(&mut player, Player::connect_new(&output.mixer));
                        fading.push(Fading { player: old, offset, shared: shared.take(), deck: deck.clone(), waiting: None, natural: false });
                    } else {
                        player.clear();
                    }
                } else {
                    player.clear();
                    fading.clear();
                }
                // Nothing stands by any more, and a song that followed is gone with the rest.
                slot.take_joined();
                standby = None;
                carried = None;
                status.standby_for.store(0, Ordering::Relaxed);
                current = 0;
                status.playing_gen.store(0, Ordering::Relaxed);
                status.mix_active.store(false, Ordering::Relaxed);
                status.mix_generation.store(0, Ordering::Release);
                // Also stops a seek still waiting on this download.
                if let Some(old) = shared.take() {
                    old.cancel();
                }
                status.playing.store(false, Ordering::Relaxed);
                status.set_position(0);
            }
            Ok(cmd @ (AudioCmd::Seek(_) | AudioCmd::SeekLive { .. })) => {
                let (pos, live) = match cmd {
                    AudioCmd::SeekLive { pos, at } => (pos + at.elapsed(), Some((pos, at))),
                    AudioCmd::Seek(pos) => (pos, None),
                    _ => unreachable!(),
                };
                seek_seq += 1;
                if current == 0 {
                    continue;
                }
                // Seeking ends a crossfade: the new song alone from here.
                for f in &fading {
                    f.deck.fade_out.store(QUICK_FADE_MS, Ordering::Relaxed);
                }
                // A seekable decoder already indexed the (fully downloaded) file.
                if seekable && player.try_seek(pos).is_ok() {
                    log::debug!("audio: seek to {pos:?} in place");
                    offset = Duration::ZERO;
                    status.set_position(pos.as_millis() as u64);
                    continue;
                }
                // The playing decoder was opened non-seekable for a fast start. Build a
                // seekable one over the same buffer off-thread (it needs every fragment
                // header, so it may wait for the download) and swap it in. Back to the
                // start, a fresh non-seekable decoder is enough and opens at once.
                if let Some(s) = shared.clone() {
                    // One builder at a time, always for the latest seek: a burst of seeks
                    // (a jam catching up) mustn't pile up threads waiting on the download.
                    *seek_job.lock().unwrap() = Some(SeekJob { shared: s, pos, live, generation: current, seek: seek_seq });
                    if !seek_busy.swap(true, Ordering::AcqRel) {
                        let (job, busy, tx, events) = (seek_job.clone(), seek_busy.clone(), tx.clone(), events.clone());
                        std::thread::spawn(move || {
                            loop {
                                let next = job.lock().unwrap().take();
                                match next {
                                    Some(j) => build_seek(j, &tx, &events),
                                    None => {
                                        busy.store(false, Ordering::Release);
                                        // A seek that came in just now finds `busy` already false, or is taken here.
                                        if job.lock().unwrap().is_none() || busy.swap(true, Ordering::AcqRel) {
                                            break;
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
            Ok(AudioCmd::DeviceLost) => {
                let was = output.device.clone();
                output.open(tx.clone(), &status);
                // The device itself went away, not a hiccup on the same one: pause, as
                // unplugging headphones does, rather than carry on out of the speakers.
                if output.device != was && !player.is_paused() && (current != 0 || !fading.is_empty()) {
                    log::info!("audio: output device gone, pausing");
                    player.pause();
                    fading.iter().for_each(|f| f.player.pause());
                    status.playing.store(false, Ordering::Relaxed);
                    wake();
                }
            }
            Ok(AudioCmd::SetDevice(id)) => {
                log::info!("audio: output device set to {id:?}");
                output.preferred = id;
                if output.default_changed() || output.sink.is_none() {
                    output.open(tx.clone(), &status);
                }
            }
            Ok(AudioCmd::Volume(v)) => {
                let v = slider_gain(&status, v);
                player.set_volume(v);
                deck.gain.store(norm_gain(&status, shared.as_deref()).to_bits(), Ordering::Relaxed);
                for f in &fading {
                    f.player.set_volume(v);
                    f.deck.gain.store(norm_gain(&status, f.shared.as_deref()).to_bits(), Ordering::Relaxed);
                }
                for next in standby.iter().chain(carried.iter().map(|(next, _)| next)) {
                    next.deck.gain.store(norm_gain(&status, Some(&next.shared)).to_bits(), Ordering::Relaxed);
                }
            }
            Ok(AudioCmd::Standby { after, decoder, shared: s }) => {
                // Behind the song it was opened for, while that is still the one wanted,
                // and only where nothing blends the two songs: a crossfade or Automix
                // hands over its own way.
                if after != current || after != status.wanted_gen.load(Ordering::Relaxed) || mix_ms(&status, length) > 0 {
                    continue;
                }
                log::debug!("audio: the next song stands by behind generation {after}");
                let (source, new) = song(decoder, &status, 0, false, &s);
                slot.put(source);
                standby = Some(Standby { shared: s, deck: new });
                status.standby_for.store(after, Ordering::Relaxed);
            }
            Ok(AudioCmd::Unstandby) => {
                slot.clear();
                // It had started already: nothing more is heard of it, and the song it
                // followed is over.
                if slot.take_joined() || carried.is_some() {
                    log::debug!("audio: the song that followed without a gap isn't wanted");
                    player.clear();
                    if current != 0 {
                        let _ = events.send(AudioEvent::Finished { generation: current });
                        current = 0;
                        status.playing_gen.store(0, Ordering::Relaxed);
                        shared = None;
                    }
                    status.playing.store(false, Ordering::Relaxed);
                    wake();
                }
                standby = None;
                carried = None;
                status.standby_for.store(0, Ordering::Relaxed);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        }

        if current != 0 && dev_lost.is_some_and(|at| offset + deck.clock.get() >= at) {
            dev_lost = None;
            log::info!("audio: dev hook: output device lost");
            output.sink = None;
            output.device = Some("dev-hook-gone".into());
            output.lost.store(true, Ordering::Relaxed);
            let _ = tx.send(AudioCmd::DeviceLost);
        }
        // Follow the default device while playing (headphones connecting take over).
        // A failed device is `DeviceLost`'s to handle (it decides whether to pause).
        if (current != 0 || !fading.is_empty())
            && !player.is_paused()
            && output.checked.elapsed() >= DEVICE_CHECK
            && !output.lost.load(Ordering::Relaxed)
            && (output.sink.is_none() || output.default_changed())
        {
            output.open(tx.clone(), &status);
        }
        if !fading.is_empty() {
            if let Some(f) = fading.iter().find(|f| f.natural) {
                status.sample_outgoing(f.offset + f.deck.clock.get(), !f.player.is_paused() && !f.player.empty());
            }
            // The next song is taking too long: don't hold this one up any longer.
            for f in fading.iter_mut().filter(|f| !status.automix.load(Ordering::Relaxed) && f.waiting.is_some_and(|(at, _)| at.elapsed() > HANDOFF_WAIT)) {
                let len = if f.natural { mix_ms(&status, length) } else { status.crossfade_ms.load(Ordering::Relaxed) };
                log::debug!("audio: next song not ready, fading out over {} ms", f.start(len, status.automix.load(Ordering::Relaxed)));
            }
            fading.retain(|f| {
                if f.player.empty() {
                    if let Some(s) = &f.shared {
                        s.cancel();
                    }
                    false
                } else {
                    true
                }
            });
            if fading.is_empty() {
                status.mix_active.store(false, Ordering::Relaxed);
                wake();
                log::debug!("audio: crossfade done");
            }
        }
        if carried.as_ref().is_some_and(|(_, since)| since.elapsed() > CARRY_WAIT) {
            log::warn!("audio: nothing claimed the song that followed without a gap; stopping it");
            carried = None;
            player.clear();
            status.playing.store(false, Ordering::Relaxed);
            wake();
        }
        if current != 0 {
            let pos = offset + deck.clock.get();
            status.set_position(pos.as_millis() as u64);
            // Start the next song early, to crossfade into it. Not for songs barely
            // longer than the fade. (The fade's length is as heard: at another speed it
            // takes more or less of the song.)
            let crossfade = Duration::from_millis(mix_ms(&status, length) as u64).mul_f32(status.speed());
            if !ending_sent
                && !crossfade.is_zero()
                && let Some(d) = length
                && d > crossfade * 2
                && pos + crossfade >= d
                && shared.as_ref().is_some_and(|s| s.error().is_none())
            {
                ending_sent = true;
                status.sample_outgoing(pos, !player.is_paused());
                let _ = events.send(AudioEvent::Ending { generation: current });
                wake();
            }
            if player.empty() {
                // A failed download ends the decoder early: that's not the song ending.
                let event = match shared.as_ref().and_then(|s| s.error()) {
                    Some(reason) => AudioEvent::Interrupted { generation: current, at: offset + deck.clock.get(), reason },
                    None => AudioEvent::Finished { generation: current },
                };
                let _ = events.send(event);
                // A song that stood by and couldn't follow (another format, not ready in
                // time) starts the usual way.
                slot.clear();
                standby = None;
                status.standby_for.store(0, Ordering::Relaxed);
                current = 0;
                status.playing_gen.store(0, Ordering::Relaxed);
                shared = None;
                status.playing.store(false, Ordering::Relaxed);
                wake();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stereo buffer with a tone in the left ear only, 48 kHz.
    fn left_only(frames: usize) -> rodio::buffer::SamplesBuffer {
        use std::num::NonZero;
        let samples: Vec<f32> = (0..frames).flat_map(|i| [(i as f32 * 0.07).sin() * 0.6, 0.0]).collect();
        rodio::buffer::SamplesBuffer::new(NonZero::new(2).unwrap(), NonZero::new(48_000).unwrap(), samples)
    }

    fn right_ear_energy(status: Arc<AudioStatus>) -> f32 {
        let mut meter = Meter::new(left_only(4_000), status, 0, Arc::new(AtomicU32::new(0)), false, Default::default());
        let out: Vec<f32> = std::iter::from_fn(|| meter.next()).collect();
        assert_eq!(out.len(), 8_000, "as many samples out as in");
        out.chunks(2).skip(500).map(|f| f[1] * f[1]).sum()
    }

    #[test]
    fn spatial_audio_off_leaves_a_stereo_song_as_it_is() {
        assert_eq!(right_ear_energy(Arc::new(AudioStatus::default())), 0.0);
    }

    #[test]
    fn spatial_audio_on_lets_the_other_ear_hear_a_left_sound_and_keeps_every_sample() {
        let status = Arc::new(AudioStatus::default());
        status.spatial.store(true, Ordering::Relaxed);
        assert!(right_ear_energy(status) > 0.0);
    }

    #[test]
    fn spatial_audio_leaves_a_mono_file_alone() {
        use std::num::NonZero;
        let status = Arc::new(AudioStatus::default());
        status.spatial.store(true, Ordering::Relaxed);
        let mono = rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(48_000).unwrap(), vec![0.5; 100]);
        let mut meter = Meter::new(mono, status, 0, Arc::new(AtomicU32::new(0)), false, Default::default());
        assert!(std::iter::from_fn(|| meter.next()).all(|s| s == 0.5));
    }

    /// A mono tone of `hz` at `rate`, a second long.
    fn mono_tone(hz: f32, rate: u32) -> Vec<f32> {
        (0..rate).map(|i| (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * 0.2).collect()
    }

    fn metered(samples: Vec<f32>, rate: u32, status: &Arc<AudioStatus>) -> Meter<rodio::buffer::SamplesBuffer> {
        use std::num::NonZero;
        let source = rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(rate).unwrap(), samples);
        Meter::new(source, status.clone(), 0, Arc::new(AtomicU32::new(0)), false, Default::default())
    }

    /// How much louder the second half of `out` is than that of `input`, in dB.
    fn louder_db(out: &[f32], input: &[f32]) -> f32 {
        let rms = |x: &[f32]| (x[x.len() / 2..].iter().map(|v| v * v).sum::<f32>() / (x.len() / 2) as f32).sqrt();
        20.0 * (rms(out) / rms(input)).log10()
    }

    /// The 1 kHz band of the equalizer, 12 dB down.
    fn one_khz_cut() -> crate::eq::Settings {
        crate::eq::Settings { enabled: true, bands: [0.0, 0.0, 0.0, -12.0, 0.0, 0.0, 0.0], bass: 0.0 }
    }

    #[test]
    fn with_the_equalizer_off_a_song_comes_out_sample_for_sample() {
        let status = Arc::new(AudioStatus::default());
        let input = mono_tone(1000.0, 44_100);
        let out: Vec<f32> = metered(input.clone(), 44_100, &status).collect();
        assert!(out == input);
        // Set but switched off is the same.
        status.eq.set(&crate::eq::Settings { enabled: false, ..one_khz_cut() });
        let out: Vec<f32> = metered(input.clone(), 44_100, &status).collect();
        assert!(out == input);
    }

    #[test]
    fn the_equalizer_shapes_a_song_at_whatever_rate_it_plays() {
        let status = Arc::new(AudioStatus::default());
        status.eq.set(&one_khz_cut());
        // YouTube's 44.1 and 48 kHz, a hi-res source's 96 and 192 kHz.
        for rate in [44_100, 48_000, 96_000, 192_000] {
            let input = mono_tone(1000.0, rate);
            let out: Vec<f32> = metered(input.clone(), rate, &status).collect();
            assert_eq!(out.len(), input.len(), "as many samples out as in");
            let db = louder_db(&out, &input);
            assert!((db + 12.0).abs() < 0.1, "1 kHz came out at {db} dB at {rate} Hz");
        }
    }

    #[test]
    fn both_songs_of_a_crossfade_are_equalized() {
        let status = Arc::new(AudioStatus::default());
        status.eq.set(&one_khz_cut());
        let fade = Arc::new(AtomicU32::new(0));
        let input = mono_tone(1000.0, 48_000);
        let source = |samples: Vec<f32>| rodio::buffer::SamplesBuffer::new(std::num::NonZero::new(1).unwrap(), std::num::NonZero::new(48_000).unwrap(), samples);
        // The song going out, a long fade ahead of it, and the one coming in at another rate.
        let mut outgoing = Meter::new(source(input.clone()), status.clone(), 0, fade.clone(), false, Default::default());
        let incoming = metered(mono_tone(1000.0, 44_100), 44_100, &status);
        let before: Vec<f32> = outgoing.by_ref().take(24_000).collect();
        assert!((louder_db(&before, &input[..24_000]) + 12.0).abs() < 0.1);
        fade.store(60_000, Ordering::Relaxed);
        let fading: Vec<f32> = outgoing.by_ref().take(2_000).collect();
        // Barely into the fade, it is still the equalized song: a quarter of the tone's level.
        let peak = fading.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((0.045..=0.051).contains(&peak), "the outgoing song peaks at {peak}");
        let out: Vec<f32> = incoming.collect();
        assert!((louder_db(&out, &mono_tone(1000.0, 44_100)) + 12.0).abs() < 0.1);
    }

    #[test]
    fn an_equalizer_change_reaches_the_song_already_playing_without_a_step() {
        let status = Arc::new(AudioStatus::default());
        let input = mono_tone(1000.0, 48_000);
        let mut meter = metered(input.clone(), 48_000, &status);
        let before: Vec<f32> = meter.by_ref().take(12_000).collect();
        assert!(before == input[..12_000]);
        status.eq.set(&one_khz_cut());
        let after: Vec<f32> = meter.collect();
        assert!((louder_db(&after, &input[12_000..]) + 12.0).abs() < 0.1);
        let steepest = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(steepest(&after) <= steepest(&before) * 1.01, "no sample jumps further than the tone itself does");
    }

    #[test]
    fn the_level_the_visuals_follow_does_not_sink_with_the_equalizers_headroom() {
        let level = |settings: crate::eq::Settings| {
            let status = Arc::new(AudioStatus::default());
            status.eq.set(&settings);
            metered(mono_tone(1000.0, 48_000), 48_000, &status).count();
            status.level()
        };
        let plain = level(crate::eq::Settings::default());
        // A 12 dB boost far below the tone turns the whole song down by 12 dB to make room.
        let boosted = level(crate::eq::Settings { enabled: true, bands: [12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], bass: 0.0 });
        assert!(plain > 0.1);
        assert!((boosted / plain - 1.0).abs() < 0.05, "{boosted} with the boost, {plain} without");
    }

    #[test]
    fn outgoing_clock_survives_incoming_position_reset() {
        let status = AudioStatus::default();
        status.sample_outgoing(Duration::from_secs(180), false);
        status.set_position(0);
        assert_eq!(status.outgoing_position(), Duration::from_secs(180));
        status.set_position(2500);
        assert_eq!(status.outgoing_position(), Duration::from_secs(180));
    }

    #[test]
    fn paused_outgoing_clock_does_not_extrapolate() {
        let status = AudioStatus::default();
        status.sample_outgoing(Duration::from_secs(42), false);
        status.outgoing_sampled_at.store(0, Ordering::Relaxed);
        assert!(!status.outgoing_playing());
        assert_eq!(status.outgoing_position(), Duration::from_secs(42));
    }

    #[test]
    fn crossfade_ramps_mirror_each_other_and_end_the_fading_song() {
        use std::num::NonZero;
        // 1 kHz mono of full-scale samples: 1 ms = 1 sample.
        let ones = || rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(1000).unwrap(), vec![1.0; 1000]);
        let status = Arc::new(AudioStatus::default());
        let out = Arc::new(AtomicU32::new(0));
        let mut incoming = Meter::new(ones(), status.clone(), 100, Arc::new(AtomicU32::new(0)), false, Default::default());
        let mut outgoing = Meter::new(ones(), status, 0, out.clone(), false, Default::default());
        assert_eq!(outgoing.next(), Some(1.0));
        out.store(100, Ordering::Relaxed);
        let (a, b): (Vec<f32>, Vec<f32>) = (0..100).map(|_| (incoming.next().unwrap(), outgoing.next().unwrap())).unzip();
        assert_eq!(a[0], 0.0);
        assert_eq!(b[0], 1.0);
        // One comes up exactly as the other goes down.
        for (x, y) in a.iter().zip(&b) {
            assert!((x + y - 1.0).abs() < 1e-4);
        }
        assert!((b[50] - 0.5).abs() < 1e-4);
        assert_eq!(incoming.next(), Some(1.0));
        assert_eq!(outgoing.next(), None);
    }

    #[test]
    fn automix_envelopes_hold_then_exchange_at_sixty_five_percent() {
        use std::num::NonZero;
        let ones = || rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(1000).unwrap(), vec![1.0; 1000]);
        let status = Arc::new(AudioStatus::default());
        let out = Arc::new(AtomicU32::new(0));
        let mut incoming = Meter::new(ones(), status.clone(), 100, Arc::new(AtomicU32::new(0)), true, Default::default());
        let mut outgoing = Meter::new(ones(), status, 0, out.clone(), false, Default::default());
        assert_eq!(outgoing.next(), Some(1.0));
        out.store(100 | AUTOMIX_FADE, Ordering::Relaxed);
        let (a, b): (Vec<f32>, Vec<f32>) = (0..100).map(|_| (incoming.next().unwrap(), outgoing.next().unwrap())).unzip();
        // The outgoing song keeps the floor through the first third of the overlap.
        assert_eq!(a[25], 0.0);
        assert_eq!(b[25], 1.0);
        // Then an equal-power swap: no gap and no step at either end, and the combined
        // loudness of the two songs holds through the whole handoff.
        for (x, y) in a.iter().zip(&b) {
            assert!((x * x + y * y - 1.0).abs() < 1e-4);
        }
        assert!((a[65] - 0.5_f32.sqrt()).abs() < 1e-4);
        assert!((b[65] - 0.5_f32.sqrt()).abs() < 1e-4);
        assert_eq!(incoming.next(), Some(1.0));
        assert_eq!(outgoing.next(), None);
    }

    #[test]
    fn transition_fx_are_dry_outside_the_blend_and_keep_channels_separate() {
        let mut fx = MixFx::new(48_000, 2).unwrap();
        for i in 0..48_000 {
            let input = (i as f32 * 0.03).sin() * 0.5;
            assert_eq!(fx.process(input, None), input);
            assert_eq!(fx.process(0.0, None), 0.0);
        }
        fx.begin(120.0);
        let mut difference = 0.0;
        for frame in 0..48_000 {
            let input = (frame as f32 * 0.03).sin() * 0.5;
            let ramp = Some(Ramp::MixOut { from: 1.0, at: frame * 2, len: 96_000 });
            let left = fx.process(input, ramp);
            let right = fx.process(0.0, ramp);
            assert!(left.is_finite() && left.abs() < 1.0);
            assert_eq!(right, 0.0);
            difference += (left - input).abs();
        }
        assert!(difference > 10.0);
        assert_eq!(fx.process(0.25, None), 0.25);
        assert_eq!(fx.process(-0.25, None), -0.25);
    }

    #[test]
    fn automix_repeat_and_flanger_are_audible() {
        let mut fx = MixFx::new(48_000, 1).unwrap();
        fx.begin(120.0);
        // Two thirds through the blend: the wet path is up.
        let ramp = Some(Ramp::MixOut { from: 1.0, at: 60_000, len: 100_000 });
        let out: Vec<f32> = (0..30_000).map(|i| fx.process(if i == 0 { 1.0 } else { 0.0 }, ramp)).collect();
        // The flanger's short comb copy sits a few dozen frames behind the impulse.
        let comb: f32 = out[20..200].iter().map(|x| x.abs()).sum();
        assert!(comb > 0.01, "no flanged comb after the impulse: {comb}");
        // The beat repeat brings the impulse back a half beat later (12,000 frames).
        let echo: f32 = out[11_950..12_060].iter().map(|x| x.abs()).sum();
        assert!(echo > 0.01, "no half-beat repeat: {echo}");
    }

    #[test]
    fn enabled_effects_finish_without_changing_the_incoming_position() {
        use std::num::NonZero;
        let rate = 48_000;
        let source = |step: f32| rodio::buffer::SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(rate).unwrap(),
            (0..rate * 2).map(|i| (i as f32 * step).sin() * 0.3).collect::<Vec<_>>(),
        );
        let status = Arc::new(AudioStatus::default());
        status.automix.store(true, Ordering::Relaxed);
        let out = Arc::new(AtomicU32::new(0));
        let mut incoming = Meter::new(source(0.04), status.clone(), 1000, Default::default(), true, Default::default());
        let mut outgoing = Meter::new(source(0.03), status, 0, out.clone(), false, Arc::new(OnceLock::from(Some(120.0))));
        assert!(incoming.fx.is_some() && outgoing.fx.is_some());
        assert_eq!(outgoing.by_ref().take(12_000).count(), 12_000);
        out.store(1000 | AUTOMIX_FADE, Ordering::Relaxed);
        for _ in 0..rate {
            let mixed = incoming.next().unwrap() + outgoing.next().unwrap();
            assert!(mixed.is_finite() && mixed.abs() < 1.0);
        }
        assert_eq!(outgoing.next(), None);
        assert_eq!(incoming.next(), Some((rate as f32 * 0.04).sin() * 0.3));
        assert!(incoming.ramp.is_none());
    }

    #[test]
    fn a_quick_fade_retains_the_effect_instead_of_jumping_to_dry() {
        let mut fx = MixFx::new(48_000, 1).unwrap();
        fx.begin(120.0);
        let mut before = 0.0;
        for at in 0..36_000 {
            before = fx.process(0.4, Some(Ramp::MixOut { from: 1.0, at, len: 48_000 }));
        }
        let after = fx.process(0.4, Some(Ramp::Out { from: 0.3, at: 0, len: 1920 }));
        assert!((after - before).abs() < 0.01);
        assert!((after - 0.4).abs() > 0.05);
    }

    #[test]
    fn transition_buffers_are_bounded_and_do_not_grow() {
        for rate in [8_000, 44_100, 48_000, 96_000, 192_000] {
            let mut fx = MixFx::new(rate, 2).unwrap();
            let bytes = fx.ring.capacity() * std::mem::size_of::<f32>();
            assert!(bytes <= FX_MAX_FRAMES * 2 * 4);
            let address = fx.ring.as_ptr();
            fx.begin(70.0);
            assert!(fx.beat <= (fx.ring.len() / fx.channels - 1) as f32);
            for at in 0..rate * 2 {
                fx.process(0.1, Some(Ramp::MixOut { from: 1.0, at, len: rate * 2 }));
            }
            assert_eq!(fx.ring.as_ptr(), address);
            assert_eq!(fx.ring.capacity() * 4, bytes);
        }
        assert!(MixFx::new(48_000, 9).is_none());
    }

    #[test]
    fn unknown_tempo_uses_a_finite_tail_and_seek_clears_history() {
        let mut fx = MixFx::new(48_000, 1).unwrap();
        fx.begin(0.0);
        assert!(!fx.rhythmic);
        let mut tail = 0.0;
        for at in 0..48_000 {
            let input = if at == 24_000 { 0.5 } else { 0.0 };
            let sample = fx.process(input, Some(Ramp::MixOut { from: 1.0, at, len: 48_000 }));
            assert!(sample.is_finite() && sample.abs() <= 0.5);
            if at > 24_001 {
                tail += sample.abs();
            }
        }
        assert!(tail > 0.001);
        fx.reset();
        assert!(fx.ring.iter().all(|v| *v == 0.0));
        assert_eq!(fx.process(0.0, None), 0.0);
    }

    #[test]
    fn tempo_analysis_stops_after_its_windows_quiet_or_not() {
        for silence in [false, true] {
            let mut tempo = Tempo::new(20.0);
            let mut result = None;
            let mut windows = 0;
            while !tempo.done {
                for i in 0..300 {
                    result = tempo.push(if !silence && (i + windows * 300) % 25 == 0 { 1.0 } else { 0.0 });
                }
                windows += 1;
                assert!(windows <= TEMPO_PASSES as usize, "the analysis never stops");
            }
            // A beat answers in the first window; nothing but silence takes all four.
            assert_eq!(windows, if silence { TEMPO_PASSES as usize } else { 1 });
            if silence {
                assert!(result.is_none());
            } else {
                assert!((result.unwrap() - 120.0).abs() < 1.0);
            }
            let history = tempo.hist;
            for _ in 0..1000 {
                assert!(tempo.push(0.8).is_none());
            }
            assert_eq!(tempo.hist, history);
            assert_eq!(tempo.count, 300);
        }
    }

    #[test]
    fn replacing_a_decoder_reuses_its_completed_analysis() {
        use std::num::NonZero;
        for result in [Some(120.0), None] {
            let analysis = Arc::new(OnceLock::from(result));
            let source = rodio::buffer::SamplesBuffer::new(NonZero::new(2).unwrap(), NonZero::new(48_000).unwrap(), vec![0.0; 4096]);
            let status = Arc::new(AudioStatus::default());
            status.automix.store(true, Ordering::Relaxed);
            let mut meter = Meter::new(source, status, 0, Default::default(), false, analysis.clone());
            assert!(meter.tempo.done);
            assert_eq!(meter.by_ref().count(), 4096);
            assert_eq!(meter.tempo.count, 0);
            assert_eq!(analysis.get(), Some(&result));
        }
    }

    #[test]
    fn mix_status_is_bound_to_the_started_generation() {
        let status = AudioStatus::default();
        status.mix_duration_ms.store(12_345, Ordering::Relaxed);
        status.mix_active.store(true, Ordering::Relaxed);
        status.mix_generation.store(7, Ordering::Release);
        assert_eq!(status.mix_duration(6), None);
        assert_eq!(status.mix_duration(7), Some(12.345));
        assert!(status.mixing());
        status.mix_active.store(false, Ordering::Relaxed);
        assert!(!status.mixing());
    }

    #[test]
    fn normalization_only_turns_loud_tracks_down() {
        let status = AudioStatus::default();
        let loud = Shared::new(1, 0, Some(7.29), crate::stream::Format::Aac, None, false);
        let quiet = Shared::new(1, 0, Some(-4.0), crate::stream::Format::Aac, None, false);
        let unknown = Shared::new(1, 0, None, crate::stream::Format::Aac, None, false);
        status.normalize.store(true, Ordering::Relaxed);
        assert!((norm_gain(&status, Some(&loud)) - 0.432).abs() < 0.001);
        assert_eq!(norm_gain(&status, Some(&quiet)), 1.0);
        assert_eq!(norm_gain(&status, Some(&unknown)), 1.0);
        assert_eq!(norm_gain(&status, None), 1.0);
        status.normalize.store(false, Ordering::Relaxed);
        assert_eq!(norm_gain(&status, Some(&loud)), 1.0);
    }

    #[test]
    fn a_fade_turns_the_music_down_without_moving_the_listeners_volume() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let audio = AudioHandle { tx, status: Arc::new(AudioStatus::default()) };
        assert_eq!(slider_gain(&audio.status, 1.0), 1.0, "nothing is faded until asked");
        audio.set_volume(0.8);
        audio.set_fade(0.25);
        assert_eq!(audio.volume(), 0.8);
        assert!((slider_gain(&audio.status, audio.volume()) - 0.2).abs() < 1e-6);
        // The player is told to play at its volume again, which now includes the fade.
        assert!(matches!(rx.try_iter().last(), Some(AudioCmd::Volume(v)) if v == 0.8));
        audio.set_fade(0.0);
        assert_eq!(slider_gain(&audio.status, audio.volume()), 0.0);
        audio.set_fade(1.0);
        assert!((slider_gain(&audio.status, audio.volume()) - 0.8).abs() < 1e-6);
        // The same level again is not worth a message.
        rx.try_iter().count();
        audio.set_fade(1.0);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn log_volume_keeps_loudness_when_switched() {
        let (tx, _rx) = crossbeam_channel::unbounded();
        let audio = AudioHandle { tx, status: Arc::new(AudioStatus::default()) };
        audio.set_volume(0.125);
        audio.set_log_volume(true);
        assert!((audio.volume() - 0.5).abs() < 1e-6);
        assert!((slider_gain(&audio.status, audio.volume()) - 0.125).abs() < 1e-6);
        audio.set_log_volume(true);
        assert!((audio.volume() - 0.5).abs() < 1e-6);
        audio.set_log_volume(false);
        assert!((audio.volume() - 0.125).abs() < 1e-6);
    }

    /// A song of 48 kHz stereo PCM, every sample of it `level` (of 32768), `frames` long.
    fn pcm_song(level: i16, frames: usize, loudness_db: Option<f32>) -> Arc<Shared> {
        let data: Vec<u8> = std::iter::repeat_n(level.to_le_bytes(), frames * 2).flatten().collect();
        Shared::from_bytes_with_format(data, frames as u64 * 1000 / 48_000, loudness_db, crate::stream::Format::Pcm { rate: 48_000, channels: 2 }, None)
    }

    fn pcm_decoder(shared: &Arc<Shared>) -> TrackDecoder {
        stream::decoder(shared.clone(), false).unwrap()
    }

    /// A sample of a `pcm_song` as it plays.
    fn heard(level: i16) -> f32 {
        level as f32 / 32768.0
    }

    #[test]
    fn the_song_standing_by_follows_the_playing_ones_last_sample_with_nothing_between() {
        let status = Arc::new(AudioStatus::default());
        let (a, b) = (pcm_song(8000, 3000, None), pcm_song(-6000, 2000, None));
        let slot: Arc<Slot<Song>> = Arc::default();
        let (first, _) = song(pcm_decoder(&a), &status, 0, false, &a);
        let (next, deck) = song(pcm_decoder(&b), &status, 0, false, &b);
        let mut playing = Joined::new(first, slot.clone());
        let mut out: Vec<f32> = playing.by_ref().take(1000).collect();
        slot.put(next);
        out.extend(playing);
        let want: Vec<f32> = [vec![heard(8000); 6000], vec![heard(-6000); 4000]].concat();
        assert!(out == want, "the first song's samples, then the second's, and nothing else");
        assert!(slot.take_joined());
        // The second song's own clock ran for it alone.
        assert_eq!(deck.clock.get(), Duration::from_micros(2000 * 1_000_000 / 48_000));
    }

    #[test]
    fn a_song_that_follows_without_a_gap_is_normalized_as_itself_from_its_first_sample() {
        let status = Arc::new(AudioStatus::default());
        status.normalize.store(true, Ordering::Relaxed);
        // The first is as loud as the target; the second 6 dB over it.
        let (a, b) = (pcm_song(8000, 1000, Some(0.0)), pcm_song(8000, 1000, Some(6.0)));
        let slot: Arc<Slot<Song>> = Arc::default();
        let (first, _) = song(pcm_decoder(&a), &status, 0, false, &a);
        slot.put(song(pcm_decoder(&b), &status, 0, false, &b).0);
        let out: Vec<f32> = Joined::new(first, slot).collect();
        assert_eq!(out.len(), 4000);
        assert!(out[..2000].iter().all(|s| *s == heard(8000)));
        let turned_down = heard(8000) * 10f32.powf(-6.0 / 20.0);
        assert!(out[2000..].iter().all(|s| *s == turned_down), "{} then {}", out[2000], out[3999]);
    }

    #[test]
    fn a_song_fading_out_or_broken_off_is_not_followed() {
        let status = Arc::new(AudioStatus::default());
        let (a, b) = (pcm_song(8000, 48_000, None), pcm_song(-6000, 2000, None));
        let slot: Arc<Slot<Song>> = Arc::default();
        let (first, deck) = song(pcm_decoder(&a), &status, 0, false, &a);
        slot.put(song(pcm_decoder(&b), &status, 0, false, &b).0);
        let mut playing = Joined::new(first, slot.clone());
        playing.by_ref().take(1000).count();
        // A skip cuts it over quickly: it ends in silence, not in the next song.
        deck.fade_out.store(QUICK_FADE_MS, Ordering::Relaxed);
        let rest: Vec<f32> = playing.collect();
        assert_eq!(rest.len(), 48 * QUICK_FADE_MS as usize * 2);
        assert!(rest.iter().all(|s| *s > 0.0 || *s == 0.0), "only the first song is heard");
        assert!(!slot.take_joined());
    }

    #[test]
    fn at_another_speed_the_whole_chain_still_ends_one_song_and_starts_the_next_on_their_own_samples() {
        let status = Arc::new(AudioStatus::default());
        status.speed.set(1.5, true);
        let (a, b) = (pcm_song(8000, 48_000, None), pcm_song(-6000, 48_000, None));
        let slot: Arc<Slot<Song>> = Arc::default();
        let (first, _) = song(pcm_decoder(&a), &status, 0, false, &a);
        slot.put(song(pcm_decoder(&b), &status, 0, false, &b).0);
        let out: Vec<f32> = Joined::new(first, slot).collect();
        // Two seconds of song in about a second and a third, and not one sample of silence.
        assert!((out.len() as f32 / 96_000.0 - 2.0 / 1.5).abs() < 0.2, "{} samples", out.len());
        let turn = out.iter().position(|s| *s != heard(8000)).unwrap();
        assert!(out[turn..].iter().all(|s| *s == heard(-6000)));
    }

    #[test]
    fn the_progress_shown_between_updates_moves_at_the_speed_the_song_plays() {
        for (speed, want) in [(1.0, 10_250), (2.0, 10_500), (0.5, 10_125)] {
            let status = AudioStatus::default();
            status.speed.set(speed, true);
            status.playing.store(true, Ordering::Relaxed);
            status.position_ms.store(10_000, Ordering::Relaxed);
            // Sampled long ago: the extrapolation is capped at 250 ms of the clock on the wall.
            status.sampled_at.store(0, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1));
            LazyLock::force(&EPOCH);
            std::thread::sleep(Duration::from_millis(260));
            assert_eq!(status.position_smooth(), Duration::from_millis(want), "at {speed}×");
        }
    }

    #[test]
    fn a_mix_takes_more_or_less_of_the_song_at_another_speed() {
        let status = AudioStatus::default();
        status.crossfade_ms.store(6000, Ordering::Relaxed);
        let length = Some(Duration::from_secs(200));
        // As heard, the crossfade is the length it was set to at any speed.
        for speed in [0.5, 1.0, 2.0] {
            status.speed.set(speed, true);
            assert_eq!(mix_ms(&status, length), 6000);
        }
        // Automix shows its window in the song's own time: twice as much of it at 2×.
        status.automix.store(true, Ordering::Relaxed);
        status.speed.set(1.0, true);
        let plain = status.mix_secs(length);
        status.speed.set(2.0, true);
        assert!((status.mix_secs(length) - plain * 2.0).abs() < 1e-3);
        // A short song gives Automix a third of the time it takes to play, not of its length.
        status.bpm.store(120.0f32.to_bits(), Ordering::Relaxed);
        assert_eq!(mix_ms(&status, Some(Duration::from_secs(12))), 2000);
        status.speed.set(1.0, true);
        assert_eq!(mix_ms(&status, Some(Duration::from_secs(12))), 4000);
    }

    /// The audio thread with no device: what it plays is pulled at the pace a device
    /// would, and kept.
    struct Rig {
        audio: AudioHandle,
        events: Receiver<AudioEvent>,
        heard: Arc<std::sync::Mutex<Vec<f32>>>,
        stop: Arc<AtomicBool>,
    }

    impl Rig {
        fn new() -> Self {
            let (tx, rx) = unbounded();
            let (events_tx, events) = unbounded();
            let status = Arc::new(AudioStatus::default());
            status.volume.store(1.0f32.to_bits(), Ordering::Relaxed);
            let (tap_tx, tap_rx) = unbounded();
            std::thread::spawn({
                let (tx, status) = (tx.clone(), status.clone());
                move || {
                    let (output, tap) = Output::headless();
                    tap_tx.send(tap).unwrap();
                    play(rx, tx, status, 1.0, output, events_tx, || {});
                }
            });
            let mut tap = tap_rx.recv().unwrap();
            let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            std::thread::spawn({
                let (heard, stop) = (heard.clone(), stop.clone());
                move || {
                    // 10 ms of sound every 10 ms.
                    let (started, mut pulled) = (Instant::now(), 0u64);
                    while !stop.load(Ordering::Relaxed) {
                        let due = started.elapsed().as_millis() as u64 / 10;
                        while pulled < due {
                            let block: Vec<f32> = tap.by_ref().take(960).collect();
                            heard.lock().unwrap().extend(block);
                            pulled += 1;
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
            });
            let rig = Self { audio: AudioHandle { tx, status }, events, heard, stop };
            // The first song after launch has its first 512 samples taken for mono (the
            // silence rodio's queue idles on has no format until a song gave it one): a
            // moment of stereo silence plays first, as in an app that has played before.
            rig.start(u64::MAX, &pcm_song(0, 480, None));
            assert_eq!(rig.finished().0, u64::MAX);
            rig
        }

        fn start(&self, generation: u64, shared: &Arc<Shared>) {
            self.audio.status.wanted_gen.store(generation, Ordering::Relaxed);
            self.audio.send(AudioCmd::Start { generation, decoder: pcm_decoder(shared), shared: shared.clone(), at: Duration::ZERO });
        }

        /// The next `Finished`, and how long it took to come.
        fn finished(&self) -> (u64, Duration) {
            let asked = Instant::now();
            loop {
                match self.events.recv_timeout(Duration::from_secs(10)).expect("the song never finished") {
                    AudioEvent::Finished { generation } => return (generation, asked.elapsed()),
                    _ => continue,
                }
            }
        }

        fn wait(&self, ms: u64) {
            std::thread::sleep(Duration::from_millis(ms));
        }

        /// What was heard so far, as runs of one level: (level, frames).
        fn runs(&self) -> Vec<(f32, usize)> {
            let heard = self.heard.lock().unwrap();
            let mut runs: Vec<(f32, usize)> = Vec::new();
            for frame in heard.chunks(2) {
                match runs.last_mut() {
                    Some((level, n)) if *level == frame[0] => *n += 1,
                    _ => runs.push((frame[0], 1)),
                }
            }
            runs
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    /// What plays today when one song ends and the next starts with no crossfade: the
    /// player runs empty, the audio thread notices on its next look, and the next song
    /// starts when it is asked for. Here it is asked for the instant `Finished` comes, as
    /// a UI that answered at once with the decoder already open would; the real one adds
    /// a frame (or the 400 ms background tick) and the decoder's opening to the silence.
    /// `cargo test the_next_song_starts_after_a_silence -- --nocapture` prints it.
    #[test]
    fn without_a_song_standing_by_the_next_song_starts_after_a_silence() {
        let rig = Rig::new();
        let (a, b) = (pcm_song(8000, 24_000, None), pcm_song(-6000, 24_000, None));
        rig.start(1, &a);
        assert_eq!(rig.finished().0, 1);
        rig.audio.send(AudioCmd::Carry);
        rig.start(2, &b);
        rig.wait(400);
        let runs: Vec<(f32, usize)> = rig.runs().into_iter().skip_while(|(level, _)| *level == 0.0).collect();
        assert_eq!(runs[0], (heard(8000), 24_000), "the first song, whole");
        assert_eq!(runs[1].0, 0.0, "then silence");
        assert_eq!(runs[2].0, heard(-6000), "then the second");
        println!("silence between the two songs: {} frames ({:.1} ms)", runs[1].1, runs[1].1 as f32 / 48.0);
        assert!(runs[1].1 > 0);
    }

    #[test]
    fn a_song_standing_by_plays_right_behind_the_last_and_its_start_claims_it_as_it_plays() {
        let rig = Rig::new();
        let status = rig.audio.status.clone();
        let (a, b) = (pcm_song(8000, 24_000, None), pcm_song(-6000, 48_000, None));
        rig.start(1, &a);
        rig.wait(150);
        rig.audio.send(AudioCmd::Standby { after: 1, decoder: stream::decoder_ahead(b.clone()).unwrap(), shared: b.clone() });
        rig.wait(100);
        assert_eq!(status.standby_for(), 1);
        // The first song ends: the UI hears so, and asks for the next one the usual way.
        assert_eq!(rig.finished().0, 1);
        assert_eq!(status.standby_for(), 0);
        rig.audio.send(AudioCmd::Carry);
        rig.start(2, &b);
        rig.wait(250);
        assert_eq!(status.playing_gen.load(Ordering::Relaxed), 2);
        assert!(status.playing.load(Ordering::Relaxed));
        // It has been playing since the first one's last sample: about 350 ms by now.
        let at = status.position().as_millis();
        assert!((250..=700).contains(&at), "the second song is {at} ms in");
        rig.wait(800);
        assert_eq!(rig.finished().0, 2);
        let runs: Vec<(f32, usize)> = rig.runs().into_iter().filter(|(level, _)| *level != 0.0).collect();
        assert_eq!(runs, [(heard(8000), 24_000), (heard(-6000), 48_000)], "each song once, whole");
        let between: Vec<(f32, usize)> = rig.runs().into_iter().skip_while(|(level, _)| *level == 0.0).take(2).collect();
        assert_eq!(between[1].0, heard(-6000), "nothing between the two");
    }

    #[test]
    fn a_song_taken_back_before_the_end_does_not_follow() {
        let rig = Rig::new();
        let (a, b) = (pcm_song(8000, 24_000, None), pcm_song(-6000, 48_000, None));
        rig.start(1, &a);
        rig.audio.send(AudioCmd::Standby { after: 1, decoder: stream::decoder_ahead(b.clone()).unwrap(), shared: b.clone() });
        rig.wait(150);
        assert_eq!(rig.audio.status.standby_for(), 1);
        rig.audio.send(AudioCmd::Unstandby);
        assert_eq!(rig.finished().0, 1);
        rig.wait(200);
        assert!(rig.runs().iter().all(|(level, _)| *level != heard(-6000)), "the second song is never heard");
        assert!(!rig.audio.status.playing.load(Ordering::Relaxed));
        // Its download wasn't given up: the song's own play still finds the buffer whole.
        assert!(b.is_complete() && b.error().is_none());
        assert!(pcm_decoder(&b).next().is_some());
    }

    #[test]
    fn a_song_that_followed_and_is_not_wanted_after_all_stops() {
        let rig = Rig::new();
        let (a, b) = (pcm_song(8000, 12_000, None), pcm_song(-6000, 96_000, None));
        rig.start(1, &a);
        rig.audio.send(AudioCmd::Standby { after: 1, decoder: stream::decoder_ahead(b.clone()).unwrap(), shared: b.clone() });
        assert_eq!(rig.finished().0, 1);
        // The queue changed in the song's last moment: something else is to play.
        rig.audio.send(AudioCmd::Unstandby);
        rig.wait(200);
        let followed: usize = rig.runs().iter().filter(|(level, _)| *level == heard(-6000)).map(|(_, n)| n).sum();
        assert!(followed > 0 && followed < 48_000, "{followed} frames of it were heard");
        assert!(!rig.audio.status.playing.load(Ordering::Relaxed));
        rig.wait(200);
        let later: usize = rig.runs().iter().filter(|(level, _)| *level == heard(-6000)).map(|(_, n)| n).sum();
        assert_eq!(later, followed, "and no more of it");
    }

    #[test]
    fn nothing_stands_by_while_songs_crossfade_or_for_a_song_no_longer_playing() {
        let rig = Rig::new();
        let (a, b) = (pcm_song(8000, 96_000, None), pcm_song(-6000, 48_000, None));
        rig.audio.set_crossfade(1);
        rig.start(1, &a);
        rig.wait(100);
        rig.audio.send(AudioCmd::Standby { after: 1, decoder: stream::decoder_ahead(b.clone()).unwrap(), shared: b.clone() });
        rig.wait(100);
        assert_eq!(rig.audio.status.standby_for(), 0, "a crossfade hands over its own way");
        rig.audio.set_crossfade(0);
        rig.audio.send(AudioCmd::Standby { after: 7, decoder: stream::decoder_ahead(b.clone()).unwrap(), shared: b.clone() });
        rig.wait(100);
        assert_eq!(rig.audio.status.standby_for(), 0, "that was for another play");
        rig.audio.send(AudioCmd::Standby { after: 1, decoder: stream::decoder_ahead(b.clone()).unwrap(), shared: b.clone() });
        rig.wait(100);
        assert_eq!(rig.audio.status.standby_for(), 1);
        // Skipping on: the song standing by was for the one skipped.
        rig.audio.send(AudioCmd::Stop);
        rig.wait(100);
        assert_eq!(rig.audio.status.standby_for(), 0);
    }

    #[test]
    fn the_position_and_the_songs_end_go_by_at_the_speed_set() {
        let rig = Rig::new();
        let status = rig.audio.status.clone();
        rig.audio.set_speed(2.0, true);
        // Three seconds of song: a second and a half at 2×.
        let a = pcm_song(8000, 144_000, None);
        rig.start(1, &a);
        rig.wait(600);
        let at = status.position().as_millis();
        assert!((900..=1500).contains(&at), "0.6 s in, the song is at {at} ms");
        // A seek lands where it was asked to, in the song's own time.
        rig.audio.send(AudioCmd::Seek(Duration::from_millis(2000)));
        rig.wait(250);
        let at = status.position().as_millis();
        assert!((2100..=2900).contains(&at), "a quarter second after seeking to 2 s it is at {at} ms");
        let (_, took) = rig.finished();
        assert!(took < Duration::from_millis(600), "the last second of the song took {took:?}");
    }

    #[test]
    fn a_crossfade_starts_as_far_before_the_end_as_it_takes_at_the_speed_set() {
        let rig = Rig::new();
        let status = rig.audio.status.clone();
        rig.audio.set_speed(2.0, true);
        rig.audio.set_crossfade(1);
        // Six seconds of song; a one-second fade is its last two seconds at 2×.
        let a = pcm_song(8000, 288_000, None);
        rig.start(1, &a);
        let ending = loop {
            match rig.events.recv_timeout(Duration::from_secs(10)).expect("the song never neared its end") {
                AudioEvent::Ending { generation: 1 } => break status.position().as_millis(),
                _ => continue,
            }
        };
        assert!((4000..=4500).contains(&ending), "the next song was asked for at {ending} ms");
    }
}
