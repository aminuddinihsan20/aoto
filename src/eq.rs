//! The equalizer: seven bands and a bass boost, worked out for each stream's own sample
//! rate and applied to each of its channels apart.
//!
//! - **Bands**: a peaking filter at each of `BANDS`, 12 dB either way.
//! - **Bass boost**: a low shelf on top of the bands.
//! - **Headroom**: the whole song is turned down by the highest point of the combined
//!   curve, so a boost never lifts a full-scale tone past full scale. It stays a plain
//!   gain: a limiter would be squashing a loud master all the time under a bass boost.
//! - **Changes** (a band, a preset, the switch) glide, so none is heard as a click.
//! - Switched off, or with nothing set, the samples pass through untouched.
//!
//! `Tuning` is what the listener set (the preset, the bands moved by hand); `Settings`
//! is what the audio thread is asked for; `Equalizer` does it to one stream.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The bands' centres in Hz.
pub const BANDS: [f32; 7] = [60.0, 150.0, 400.0, 1000.0, 2400.0, 6000.0, 15000.0];
/// How far a band goes either way, in dB.
pub const MAX_DB: f32 = 12.0;

/// How wide a band is: the bands sit about an octave and a third apart, and at this
/// width neighbours join into one curve rather than stand as humps of their own.
const BAND_Q: f64 = 1.0;
/// Where the bass boost's shelf is half way up, and how high it goes at 100 %.
const BASS_HZ: f64 = 100.0;
const BASS_MAX_DB: f64 = 9.0;
/// The filters in the chain: one for each band, then the shelf.
const STAGES: usize = BANDS.len() + 1;
/// A filter needs its frequency under the top of the stream (half its rate), with a
/// little room: a band at or past that is left out.
const TOP: f64 = 0.47;

/// How quickly a gain follows a change (its time constant), and how long the equalized
/// sound takes to fade in or out when it starts or stops working.
const GLIDE_SECS: f64 = 0.02;
const FADE_SECS: f64 = 0.03;
/// A gain this close to where it is going is there, in dB.
const ARRIVED: f64 = 0.001;

/// What the audio thread is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    /// Each band's gain in dB.
    pub bands: [f32; BANDS.len()],
    /// The bass boost, 0 to 1.
    pub bass: f32,
}

impl Settings {
    /// The gain asked of each filter in dB: nothing of any when switched off.
    fn gains(&self) -> [f64; STAGES] {
        let mut gains = [0.0; STAGES];
        if self.enabled {
            for (g, db) in gains.iter_mut().zip(self.bands) {
                *g = db.clamp(-MAX_DB, MAX_DB) as f64;
            }
            gains[BANDS.len()] = self.bass.clamp(0.0, 1.0) as f64 * BASS_MAX_DB;
        }
        gains
    }
}

/// The settings where every stream's equalizer reads them (see `audio::Meter`).
#[derive(Default)]
pub struct Shared {
    enabled: AtomicBool,
    /// f32 bits
    bands: [AtomicU32; BANDS.len()],
    bass: AtomicU32,
}

impl Shared {
    pub fn set(&self, s: &Settings) {
        self.enabled.store(s.enabled, Ordering::Relaxed);
        for (slot, db) in self.bands.iter().zip(s.bands) {
            slot.store(db.to_bits(), Ordering::Relaxed);
        }
        self.bass.store(s.bass.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> Settings {
        Settings {
            enabled: self.enabled.load(Ordering::Relaxed),
            bands: std::array::from_fn(|i| f32::from_bits(self.bands[i].load(Ordering::Relaxed))),
            bass: f32::from_bits(self.bass.load(Ordering::Relaxed)),
        }
    }
}

/// What of a filter follows from its frequency and the stream's rate alone.
#[derive(Clone, Copy)]
struct Tuned {
    cos: f64,
    alpha: f64,
}

/// Each filter tuned to `rate`; None for one the stream cannot carry.
fn tune(rate: u32) -> [Option<Tuned>; STAGES] {
    std::array::from_fn(|stage| {
        let band = stage < BANDS.len();
        let hz = if band { BANDS[stage] as f64 } else { BASS_HZ };
        if hz >= TOP * rate as f64 {
            return None;
        }
        let w = std::f64::consts::TAU * hz / rate as f64;
        // The shelf's slope is the steepest that doesn't overshoot (S = 1).
        let alpha = if band { w.sin() / (2.0 * BAND_Q) } else { w.sin() / std::f64::consts::SQRT_2 };
        Some(Tuned { cos: w.cos(), alpha })
    })
}

/// A second-order filter: `y = b0 x + b1 x1 + b2 x2 - a1 y1 - a2 y2`.
#[derive(Clone, Copy)]
struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coefs {
    /// The filter of `stage` at `db` (the Audio EQ Cookbook's peaking filter and low shelf).
    fn new(stage: usize, t: Tuned, db: f64) -> Self {
        let a = 10f64.powf(db / 40.0);
        if stage < BANDS.len() {
            let a0 = 1.0 + t.alpha / a;
            Self { b0: (1.0 + t.alpha * a) / a0, b1: -2.0 * t.cos / a0, b2: (1.0 - t.alpha * a) / a0, a1: -2.0 * t.cos / a0, a2: (1.0 - t.alpha / a) / a0 }
        } else {
            let r = 2.0 * a.sqrt() * t.alpha;
            let a0 = (a + 1.0) + (a - 1.0) * t.cos + r;
            Self {
                b0: a * ((a + 1.0) - (a - 1.0) * t.cos + r) / a0,
                b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * t.cos) / a0,
                b2: a * ((a + 1.0) - (a - 1.0) * t.cos - r) / a0,
                a1: -2.0 * ((a - 1.0) + (a + 1.0) * t.cos) / a0,
                a2: ((a + 1.0) + (a - 1.0) * t.cos - r) / a0,
            }
        }
    }

    /// How many times the power of a tone it lets through, the tone given as the cosine
    /// and sine of its angle a sample and of twice that.
    fn power(&self, (c1, s1, c2, s2): (f64, f64, f64, f64)) -> f64 {
        let above = (self.b0 + self.b1 * c1 + self.b2 * c2).powi(2) + (self.b1 * s1 + self.b2 * s2).powi(2);
        let below = (1.0 + self.a1 * c1 + self.a2 * c2).powi(2) + (self.a1 * s1 + self.a2 * s2).powi(2);
        above / below
    }
}

/// The filters of `gains`: None for one with nothing to do, or that the stream cannot carry.
fn chain(tuned: &[Option<Tuned>; STAGES], gains: &[f64; STAGES]) -> [Option<Coefs>; STAGES] {
    std::array::from_fn(|i| tuned[i].filter(|_| gains[i] != 0.0).map(|t| Coefs::new(i, t, gains[i])))
}

/// What the filters do together to a tone of `hz`, in dB.
fn curve_db(chain: &[Option<Coefs>; STAGES], rate: u32, hz: f64) -> f64 {
    let w = std::f64::consts::TAU * hz / rate as f64;
    let tone = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    10.0 * chain.iter().flatten().map(|c| c.power(tone)).product::<f64>().max(1e-30).log10()
}

/// The highest point of the combined curve, in dB: what a boost has to be given room
/// for. Never below zero: cuts are not made up for.
fn headroom(chain: &[Option<Coefs>; STAGES], rate: u32) -> f64 {
    if chain.iter().all(Option::is_none) {
        return 0.0;
    }
    // Looked for from 20 Hz to the top of hearing (or of the stream), a tenth of an
    // octave at a time, and at each band's own frequency, where a single peak is.
    let top = 20_000f64.min(TOP * rate as f64);
    let steps = ((top / 20.0).log2() * 10.0).ceil().max(1.0) as usize;
    let sweep = (0..=steps).map(|i| 20.0 * (top / 20.0).powf(i as f64 / steps as f64));
    let centres = BANDS.iter().map(|hz| *hz as f64).filter(|hz| *hz < top);
    sweep.chain(centres).map(|hz| curve_db(chain, rate, hz)).fold(0.0, f64::max)
}

/// The combined curve of some settings at a sample rate, to draw.
pub struct Curve {
    chain: [Option<Coefs>; STAGES],
    rate: u32,
}

impl Curve {
    pub fn new(s: &Settings, rate: u32) -> Self {
        Self { chain: chain(&tune(rate), &s.gains()), rate }
    }

    /// How much louder a tone of `hz` comes out, in dB, before the headroom is taken off.
    pub fn db(&self, hz: f32) -> f32 {
        curve_db(&self.chain, self.rate, hz as f64) as f32
    }

    /// How much everything is turned down to make room for the boosts, in dB.
    pub fn headroom_db(&self) -> f32 {
        headroom(&self.chain, self.rate) as f32
    }
}

/// What one filter remembers of one channel: its last two samples in and out.
#[derive(Clone, Copy, Default)]
struct Memory {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Memory {
    fn run(&mut self, c: &Coefs, x: f64) -> f64 {
        let y = c.b0 * x + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2;
        // A tail far too small to hear ends, rather than shrinking for ever.
        let y = if y.abs() < 1e-30 { 0.0 } else { y };
        (self.x2, self.x1, self.y2, self.y1) = (self.x1, x, self.y1, y);
        y
    }
}

/// The equalizer of one stream.
pub struct Equalizer {
    settings: Settings,
    rate: u32,
    tuned: [Option<Tuned>; STAGES],
    /// Where each filter's gain is going and where it is, in dB.
    goal: [f64; STAGES],
    gain: [f64; STAGES],
    coefs: [Coefs; STAGES],
    /// The filters that have something to do; the rest are skipped.
    live: [bool; STAGES],
    /// The headroom the same way, in dB, and as the gain it comes to.
    room_goal: f64,
    room: f64,
    pre: f64,
    /// A gain is still on its way.
    gliding: bool,
    /// Something is set: the equalized sound is wanted.
    working: bool,
    /// How much of what goes out is the equalized sound: 0 at rest, 1 at work.
    mix: f64,
    /// Each channel's memories, and the channel the next sample belongs to.
    memory: Vec<[Memory; STAGES]>,
    channel: usize,
    /// How far a gain glides, and the mix fades, in one frame.
    glide: f64,
    fade: f64,
}

impl Equalizer {
    /// A stream that starts with `settings` already in place.
    pub fn new(settings: Settings, rate: u32, channels: u16) -> Self {
        let rate = rate.max(1);
        let unit = Coefs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };
        let mut eq = Self {
            settings,
            rate,
            tuned: tune(rate),
            goal: [0.0; STAGES],
            gain: [0.0; STAGES],
            coefs: [unit; STAGES],
            live: [false; STAGES],
            room_goal: 0.0,
            room: 0.0,
            pre: 1.0,
            gliding: false,
            working: false,
            mix: 0.0,
            memory: vec![[Memory::default(); STAGES]; channels.max(1) as usize],
            channel: 0,
            glide: 1.0 - (-1.0 / (GLIDE_SECS * rate as f64)).exp(),
            fade: 1.0 / (FADE_SECS * rate as f64),
        };
        eq.aim();
        eq.mix = if eq.working { 1.0 } else { 0.0 };
        eq
    }

    /// New settings, reached by gliding from the sound of the old ones.
    pub fn set(&mut self, settings: Settings) {
        if settings != self.settings {
            self.settings = settings;
            self.aim();
        }
    }

    /// Point the gains at the settings. From rest they start there (the mix fades the
    /// sound in); at work they glide. With nothing left to do the curve stays as it is
    /// while the mix fades it out.
    fn aim(&mut self) {
        let mut goal = self.settings.gains();
        for (g, t) in goal.iter_mut().zip(&self.tuned) {
            if t.is_none() {
                *g = 0.0;
            }
        }
        self.working = goal.iter().any(|g| *g != 0.0);
        if !self.working {
            return;
        }
        self.goal = goal;
        self.room_goal = headroom(&chain(&self.tuned, &goal), self.rate);
        if self.mix == 0.0 {
            self.gain = goal;
            self.room = self.room_goal;
            self.pre = 10f64.powf(-self.room / 20.0);
            for i in 0..STAGES {
                if let Some(t) = self.tuned[i] {
                    self.coefs[i] = Coefs::new(i, t, goal[i]);
                }
            }
            self.forget();
        }
        for i in 0..STAGES {
            let live = self.gain[i] != 0.0 || goal[i] != 0.0;
            if live && !self.live[i] {
                // It joins in from silence, at a gain where that is what it would hold.
                self.memory.iter_mut().for_each(|m| m[i] = Memory::default());
            }
            self.live[i] = live;
        }
        self.gliding = true;
    }

    /// One frame on: the mix and the gains move towards where they are going. False
    /// when that brought the equalizer to rest.
    fn step(&mut self) -> bool {
        if self.working && self.mix < 1.0 {
            self.mix = (self.mix + self.fade).min(1.0);
        } else if !self.working {
            self.mix = (self.mix - self.fade).max(0.0);
            if self.mix == 0.0 {
                return false;
            }
        }
        if self.gliding {
            let toward = |from: f64, to: f64, k: f64| if (to - from).abs() < ARRIVED { to } else { from + (to - from) * k };
            self.gliding = false;
            for i in 0..STAGES {
                if self.gain[i] != self.goal[i]
                    && let Some(t) = self.tuned[i]
                {
                    self.gain[i] = toward(self.gain[i], self.goal[i], self.glide);
                    self.coefs[i] = Coefs::new(i, t, self.gain[i]);
                    self.gliding |= self.gain[i] != self.goal[i];
                    self.live[i] = self.gain[i] != 0.0 || self.goal[i] != 0.0;
                }
            }
            if self.room != self.room_goal {
                self.room = toward(self.room, self.room_goal, self.glide);
                self.pre = 10f64.powf(-self.room / 20.0);
                self.gliding |= self.room != self.room_goal;
            }
        }
        true
    }

    /// The stream goes on at another rate or channel count: tuned to it afresh.
    pub fn set_format(&mut self, rate: u32, channels: u16) {
        if rate.max(1) != self.rate || channels.max(1) as usize != self.memory.len() {
            *self = Self::new(self.settings, rate, channels);
        }
    }

    fn forget(&mut self) {
        self.memory.iter_mut().for_each(|m| *m = [Memory::default(); STAGES]);
    }

    /// Forget what was heard (after a seek).
    pub fn reset(&mut self) {
        self.forget();
        self.channel = 0;
    }

    /// The gain the headroom comes to right now: 1 at rest, less under a boost.
    pub fn headroom_gain(&self) -> f32 {
        (1.0 + self.mix * (self.pre - 1.0)) as f32
    }

    /// The stream's next sample in (its channels interleaved), the equalized one out.
    pub fn process(&mut self, x: f32) -> f32 {
        // At rest: not a sample is touched.
        if !self.working && self.mix == 0.0 {
            return x;
        }
        if self.channel == 0 && !self.step() {
            return x;
        }
        let channel = self.channel;
        self.channel = (channel + 1) % self.memory.len();
        let memory = &mut self.memory[channel];
        let dry = x as f64;
        let mut wet = dry;
        for i in 0..STAGES {
            if self.live[i] {
                wet = memory[i].run(&self.coefs[i], wet);
            }
        }
        wet *= self.pre;
        (dry + self.mix * (wet - dry)) as f32
    }
}

/// The curves on offer, and the listener's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    Flat,
    BassBoost,
    Vocal,
    Acoustic,
    Electronic,
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 6] = [Preset::Flat, Preset::BassBoost, Preset::Vocal, Preset::Acoustic, Preset::Electronic, Preset::Custom];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Flat => "Flat",
            Preset::BassBoost => "Bass boost",
            Preset::Vocal => "Vocal",
            Preset::Acoustic => "Acoustic",
            Preset::Electronic => "Electronic",
            Preset::Custom => "Custom",
        }
    }

    /// The name it is saved under.
    pub fn key(self) -> &'static str {
        match self {
            Preset::Flat => "flat",
            Preset::BassBoost => "bass",
            Preset::Vocal => "vocal",
            Preset::Acoustic => "acoustic",
            Preset::Electronic => "electronic",
            Preset::Custom => "custom",
        }
    }

    pub fn from_key(key: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Its bands in dB; the listener's own curve has none of its own.
    fn bands(self) -> Option<[f32; BANDS.len()]> {
        match self {
            Preset::Flat => Some([0.0; BANDS.len()]),
            Preset::BassBoost => Some([6.0, 4.0, 1.5, 0.0, 0.0, 0.0, 0.0]),
            Preset::Vocal => Some([-3.0, -2.0, 2.5, 4.0, 3.5, 1.0, -1.5]),
            Preset::Acoustic => Some([5.0, 3.0, 1.5, 2.0, 3.5, 4.0, 2.0]),
            Preset::Electronic => Some([4.0, 2.5, -1.0, 1.5, 1.0, 3.0, 4.5]),
            Preset::Custom => None,
        }
    }
}

/// What the listener set, as it is kept between launches.
#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    pub enabled: bool,
    pub preset: Preset,
    /// The bands as last moved by hand: what Custom plays.
    pub custom: [f32; BANDS.len()],
    /// The bass boost, 0 to 1.
    pub bass: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { enabled: false, preset: Preset::Flat, custom: [0.0; BANDS.len()], bass: 0.0 }
    }
}

impl Tuning {
    /// The bands that play: the preset's, or the listener's own.
    pub fn bands(&self) -> [f32; BANDS.len()] {
        self.preset.bands().unwrap_or(self.custom)
    }

    pub fn pick(&mut self, preset: Preset) {
        self.preset = preset;
    }

    /// A band moved by hand: the curve on screen becomes the listener's own, with that
    /// band at `db`.
    pub fn move_band(&mut self, band: usize, db: f32) {
        if band < BANDS.len() {
            self.custom = self.bands();
            self.custom[band] = db.clamp(-MAX_DB, MAX_DB);
            self.preset = Preset::Custom;
        }
    }

    pub fn set_bass(&mut self, bass: f32) {
        self.bass = bass.clamp(0.0, 1.0);
    }

    pub fn settings(&self) -> Settings {
        Settings { enabled: self.enabled, bands: self.bands(), bass: self.bass }
    }

    /// The listener's own bands as one line of text, to save.
    pub fn custom_text(&self) -> String {
        self.custom.map(|db| db.to_string()).join(",")
    }

    /// From what was saved; anything missing or damaged reads as it is on first launch.
    pub fn load(enabled: Option<&str>, preset: Option<&str>, custom: Option<&str>, bass: Option<&str>) -> Tuning {
        let bands = custom.and_then(|text| {
            let read: Vec<f32> = text.split(',').map(|v| v.trim().parse::<f32>().ok().filter(|db| db.is_finite())).collect::<Option<_>>()?;
            <[f32; BANDS.len()]>::try_from(read).ok()
        });
        Tuning {
            enabled: enabled == Some("1"),
            preset: preset.and_then(Preset::from_key).unwrap_or(Preset::Flat),
            custom: bands.unwrap_or_default().map(|db| db.clamp(-MAX_DB, MAX_DB)),
            bass: bass.and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: u32, frames: usize, amp: f32) -> Vec<f32> {
        (0..frames).map(|i| (i as f64 * hz as f64 * std::f64::consts::TAU / rate as f64).sin() as f32 * amp).collect()
    }

    fn noise(samples: usize) -> Vec<f32> {
        let mut x: u32 = 12345;
        (0..samples)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt() as f32
    }

    fn run(eq: &mut Equalizer, input: &[f32]) -> Vec<f32> {
        input.iter().map(|&x| eq.process(x)).collect()
    }

    /// The largest jump from one sample to the next.
    fn steepest(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    fn on(bands: [f32; 7]) -> Settings {
        Settings { enabled: true, bands, bass: 0.0 }
    }

    fn band(i: usize, db: f32) -> Settings {
        let mut bands = [0.0; 7];
        bands[i] = db;
        on(bands)
    }

    /// How much louder a steady tone comes out than it went in, in dB (mono, once the
    /// filters have settled).
    fn tone_gain_db(s: Settings, rate: u32, hz: f32) -> f32 {
        let mut eq = Equalizer::new(s, rate, 1);
        let input = tone(hz, rate, rate as usize, 0.1);
        let out = run(&mut eq, &input);
        let half = input.len() / 2;
        20.0 * (rms(&out[half..]) / rms(&input[half..])).log10()
    }

    /// The same without the headroom: what the curve itself does to the tone.
    fn curve_gain_db(s: Settings, rate: u32, hz: f32) -> f32 {
        tone_gain_db(s, rate, hz) + Curve::new(&s, rate).headroom_db()
    }

    #[test]
    fn a_tone_at_a_bands_centre_comes_out_with_that_bands_gain() {
        for rate in [44_100, 48_000, 96_000] {
            for (i, &hz) in BANDS.iter().enumerate() {
                for db in [6.0, 12.0, -9.0, -12.0] {
                    let heard = curve_gain_db(band(i, db), rate, hz);
                    assert!((heard - db).abs() < 0.1, "{hz} Hz at {rate} Hz set to {db} dB came out at {heard} dB");
                }
            }
        }
    }

    #[test]
    fn a_boost_is_paid_for_in_headroom_and_a_cut_is_not() {
        assert!((Curve::new(&band(3, 6.0), 48_000).headroom_db() - 6.0).abs() < 0.01);
        assert_eq!(Curve::new(&band(3, -6.0), 48_000).headroom_db(), 0.0);
        // So the boosted tone comes out as loud as it went in, and the rest quieter.
        assert!(tone_gain_db(band(3, 6.0), 48_000, 1000.0).abs() < 0.1);
        assert!((tone_gain_db(band(3, 6.0), 48_000, 60.0) + 6.0).abs() < 0.2);
    }

    #[test]
    fn the_headroom_taken_is_told_as_a_gain() {
        assert_eq!(Equalizer::new(band(3, 6.0), 48_000, 2).headroom_gain(), 10f32.powf(-6.0 / 20.0));
        assert_eq!(Equalizer::new(band(3, -6.0), 48_000, 2).headroom_gain(), 1.0);
        assert_eq!(Equalizer::new(Settings { enabled: false, ..band(3, 6.0) }, 48_000, 2).headroom_gain(), 1.0);
    }

    #[test]
    fn with_every_band_at_zero_the_song_passes_untouched() {
        let input = noise(20_000);
        let mut eq = Equalizer::new(on([0.0; 7]), 44_100, 2);
        assert!(run(&mut eq, &input) == input, "not a sample is changed");
    }

    #[test]
    fn switched_off_the_song_passes_untouched_whatever_is_set() {
        let input = noise(20_000);
        let s = Settings { enabled: false, bands: [12.0, -12.0, 6.0, 3.0, -4.0, 9.0, 12.0], bass: 1.0 };
        let mut eq = Equalizer::new(s, 48_000, 2);
        assert!(run(&mut eq, &input) == input, "not a sample is changed");
        assert_eq!(Curve::new(&s, 48_000).headroom_db(), 0.0);
    }

    #[test]
    fn a_band_far_from_a_tone_barely_moves_it() {
        // Three octaves and more away.
        for (i, db, hz) in [(0, 12.0, 2400.0), (0, -12.0, 1000.0), (6, 12.0, 400.0), (6, -12.0, 150.0), (3, 12.0, 60.0), (3, 12.0, 15000.0)] {
            let heard = curve_gain_db(band(i, db), 48_000, hz);
            assert!(heard.abs() < 0.3, "{} Hz at {db} dB moved a {hz} Hz tone by {heard} dB", BANDS[i]);
        }
    }

    #[test]
    fn the_bass_boost_lifts_the_lows_and_leaves_the_highs() {
        for rate in [44_100, 96_000] {
            let full = Settings { enabled: true, bass: 1.0, ..Default::default() };
            let low = curve_gain_db(full, rate, 40.0);
            assert!((7.5..=9.1).contains(&low), "40 Hz came out at {low} dB");
            for hz in [1000.0, 4000.0, 12_000.0] {
                let high = curve_gain_db(full, rate, hz);
                assert!(high.abs() < 0.2, "{hz} Hz came out at {high} dB");
            }
            // Half way up the slider is half the lift.
            let half = curve_gain_db(Settings { bass: 0.5, ..full }, rate, 40.0);
            assert!((3.5..=4.6).contains(&half), "40 Hz came out at {half} dB at half");
        }
    }

    #[test]
    fn the_bass_boost_goes_on_top_of_the_bands() {
        let s = Settings { enabled: true, bands: [6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], bass: 1.0 };
        let both = curve_gain_db(s, 48_000, 60.0);
        let shelf = curve_gain_db(Settings { bands: [0.0; 7], ..s }, 48_000, 60.0);
        assert!((both - shelf - 6.0).abs() < 0.1, "{both} dB with the band, {shelf} dB without");
    }

    #[test]
    fn the_curve_drawn_is_the_one_heard() {
        let s = Settings { enabled: true, bands: [6.0, 4.0, 1.5, 0.0, -3.0, 2.0, 5.0], bass: 0.4 };
        for rate in [44_100, 48_000] {
            let curve = Curve::new(&s, rate);
            for hz in [30.0, 60.0, 100.0, 250.0, 1000.0, 3000.0, 8000.0, 15_000.0] {
                let heard = curve_gain_db(s, rate, hz);
                assert!((curve.db(hz) - heard).abs() < 0.1, "{hz} Hz: drawn at {} dB, heard at {heard} dB", curve.db(hz));
            }
        }
    }

    #[test]
    fn every_band_at_either_end_stays_stable_at_any_rate() {
        for rate in [8_000, 22_050, 32_000, 44_100, 48_000, 192_000] {
            for db in [12.0, -12.0] {
                for channels in [1u16, 2] {
                    let mut eq = Equalizer::new(Settings { enabled: true, bands: [db; 7], bass: 1.0 }, rate, channels);
                    let out = run(&mut eq, &noise(rate as usize));
                    assert!(out.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{db} dB at {rate} Hz ran away");
                    // And it rings out once the music stops.
                    let tail = run(&mut eq, &vec![0.0; rate as usize * 2]);
                    assert!(tail.iter().all(|v| v.is_finite()));
                    assert!(tail[tail.len() - 64..].iter().all(|v| v.abs() < 1e-6), "{db} dB at {rate} Hz kept ringing");
                }
            }
        }
    }

    #[test]
    fn a_band_above_what_the_stream_carries_is_left_out() {
        // 15 kHz is past the top of a 22.05 kHz stream (11 kHz): nothing to filter there.
        let s = band(6, 12.0);
        assert_eq!(Curve::new(&s, 22_050).headroom_db(), 0.0);
        assert_eq!(Curve::new(&s, 22_050).db(5000.0), 0.0);
        let input = noise(10_000);
        let mut eq = Equalizer::new(s, 22_050, 1);
        assert!(run(&mut eq, &input) == input, "not a sample is changed");
        // The bands it does carry still work beside it.
        let both = Settings { enabled: true, bands: [0.0, 0.0, 0.0, 6.0, 0.0, 0.0, 12.0], bass: 0.0 };
        assert!((curve_gain_db(both, 22_050, 1000.0) - 6.0).abs() < 0.1);
    }

    #[test]
    fn the_top_band_works_where_it_is_near_the_top_of_the_stream() {
        // 15 kHz in a 32 kHz stream sits just under its top (16 kHz).
        for rate in [32_000, 44_100] {
            for db in [12.0, -12.0] {
                let heard = curve_gain_db(band(6, db), rate, 15_000.0);
                assert!((heard - db).abs() < 0.1, "{db} dB at {rate} Hz came out at {heard} dB");
            }
        }
    }

    #[test]
    fn boosts_never_push_a_full_scale_tone_past_full_scale() {
        let loudest = Settings { enabled: true, bands: [12.0; 7], bass: 1.0 };
        let presets = Preset::ALL.into_iter().filter_map(|p| p.bands()).map(|bands| Settings { enabled: true, bands, bass: 0.6 });
        for s in presets.chain([loudest, band(0, 12.0), band(6, 12.0)]) {
            for rate in [44_100, 48_000] {
                for step in 0..40 {
                    // From 20 Hz to 20 kHz, a quarter of an octave at a time.
                    let hz = 20.0 * 2f32.powf(step as f32 / 4.0);
                    let mut eq = Equalizer::new(s, rate, 1);
                    let out = run(&mut eq, &tone(hz, rate, rate as usize / 2, 1.0));
                    let peak = out[out.len() / 2..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    assert!(peak <= 1.01, "{hz} Hz at full scale came out at {peak} with {s:?}");
                }
            }
        }
    }

    #[test]
    fn a_change_while_music_plays_makes_no_step() {
        let flat = on([0.0; 7]);
        let low = on([12.0, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let cut = on([-12.0, -12.0, 6.0, 0.0, 0.0, 0.0, 0.0]);
        let off = Settings { enabled: false, ..low };
        let bass = Settings { bass: 1.0, ..flat };
        let rate = 48_000usize;
        // A 100 Hz tone: both bands and the shelf reach it.
        let input = tone(100.0, rate as u32, rate * 2, 0.25);
        for (from, to) in [(flat, low), (low, flat), (low, cut), (cut, low), (off, low), (low, off), (flat, bass), (bass, off)] {
            let mut eq = Equalizer::new(from, rate as u32, 1);
            let mut out = Vec::with_capacity(input.len());
            for (i, &x) in input.iter().enumerate() {
                if i == rate / 2 + 137 {
                    eq.set(to);
                }
                out.push(eq.process(x));
            }
            let before = &out[rate / 4..rate / 2];
            let after = &out[rate * 3 / 2..];
            assert!((rms(before) - rms(after)).abs() > 0.01, "the change was heard: {from:?} to {to:?}");
            // Through the change no sample jumps further than the tone itself does.
            let through = steepest(&out[rate / 2..rate * 3 / 2]);
            let steady = steepest(before).max(steepest(after));
            assert!(through <= steady * 1.5, "a jump of {through} against the tone's own {steady}: {from:?} to {to:?}");
        }
    }

    #[test]
    fn a_change_arrives_at_the_sound_it_asked_for() {
        let s = on([6.0, 3.0, 0.0, -4.0, 0.0, 2.0, 5.0]);
        let input = tone(1000.0, 48_000, 96_000, 0.2);
        let mut fresh = Equalizer::new(s, 48_000, 1);
        let wanted = run(&mut fresh, &input);
        for from in [on([0.0; 7]), Settings { enabled: false, ..s }, on([-12.0; 7])] {
            let mut eq = Equalizer::new(from, 48_000, 1);
            eq.set(s);
            let out = run(&mut eq, &input);
            let (a, b) = (rms(&out[48_000..]), rms(&wanted[48_000..]));
            assert!((20.0 * (a / b).log10()).abs() < 0.01, "from {from:?}: {a} against {b}");
        }
    }

    #[test]
    fn switched_off_mid_song_it_fades_back_to_the_song_untouched() {
        let s = Settings { enabled: true, bands: [9.0, 4.0, 0.0, -3.0, 0.0, 5.0, 8.0], bass: 0.7 };
        let input = noise(60_000);
        let mut eq = Equalizer::new(s, 48_000, 2);
        let out = run(&mut eq, &input[..20_000]);
        assert!(out != input[..20_000], "it was at work");
        eq.set(Settings { enabled: false, ..s });
        run(&mut eq, &input[20_000..40_000]);
        assert!(run(&mut eq, &input[40_000..]) == input[40_000..], "switched off, not a sample is changed");
        // The same when every band is brought back to zero with the switch still on.
        let mut eq = Equalizer::new(s, 48_000, 2);
        run(&mut eq, &input[..20_000]);
        eq.set(on([0.0; 7]));
        run(&mut eq, &input[20_000..40_000]);
        assert!(run(&mut eq, &input[40_000..]) == input[40_000..], "flat, not a sample is changed");
    }

    #[test]
    fn each_channel_is_filtered_on_its_own() {
        // Stereo with a tone in the left ear only.
        let s = band(3, -12.0);
        let mut eq = Equalizer::new(s, 48_000, 2);
        let left = tone(1000.0, 48_000, 48_000, 0.5);
        let input: Vec<f32> = left.iter().flat_map(|&l| [l, 0.0]).collect();
        let out = run(&mut eq, &input);
        assert!(out.chunks(2).all(|f| f[1] == 0.0), "the silent ear stays silent");
        let heard: Vec<f32> = out.chunks(2).skip(24_000).map(|f| f[0]).collect();
        assert!((20.0 * (rms(&heard) / rms(&left[24_000..])).log10() + 12.0).abs() < 0.1);
        // Six channels, the tone in the fifth.
        let mut eq = Equalizer::new(s, 48_000, 6);
        let input: Vec<f32> = left.iter().flat_map(|&l| [0.0, 0.0, 0.0, 0.0, l, 0.0]).collect();
        let out = run(&mut eq, &input);
        assert!(out.chunks(6).all(|f| f[0] == 0.0 && f[1] == 0.0 && f[2] == 0.0 && f[3] == 0.0 && f[5] == 0.0));
        let heard: Vec<f32> = out.chunks(6).skip(24_000).map(|f| f[4]).collect();
        assert!((20.0 * (rms(&heard) / rms(&left[24_000..])).log10() + 12.0).abs() < 0.1);
    }

    #[test]
    fn a_seek_forgets_what_was_heard() {
        let mut eq = Equalizer::new(Settings { enabled: true, bands: [12.0; 7], bass: 1.0 }, 48_000, 2);
        run(&mut eq, &noise(9_001));
        eq.reset();
        assert!(run(&mut eq, &[0.0; 200]).iter().all(|v| *v == 0.0), "no tail left over from before the seek");
    }

    #[test]
    fn a_stream_that_goes_on_at_another_rate_is_tuned_to_it() {
        let s = band(3, 6.0);
        let mut eq = Equalizer::new(s, 48_000, 2);
        run(&mut eq, &noise(5_000));
        eq.set_format(96_000, 1);
        let input = tone(1000.0, 96_000, 96_000, 0.1);
        let out = run(&mut eq, &input);
        // +6 dB at the band, taken off again as headroom.
        assert!((20.0 * (rms(&out[48_000..]) / rms(&input[48_000..])).log10()).abs() < 0.1);
        let input = tone(60.0, 96_000, 96_000, 0.1);
        let out = run(&mut eq, &input);
        assert!((20.0 * (rms(&out[48_000..]) / rms(&input[48_000..])).log10() + 6.0).abs() < 0.2);
    }

    #[test]
    fn settings_reach_the_audio_thread_as_they_were_set() {
        let shared = Shared::default();
        assert_eq!(shared.get(), Settings::default());
        let s = Settings { enabled: true, bands: [1.5, -2.0, 0.0, 12.0, -12.0, 0.5, 3.0], bass: 0.6 };
        shared.set(&s);
        assert_eq!(shared.get(), s);
    }

    #[test]
    fn flat_is_every_band_at_zero_and_no_preset_leaves_the_range() {
        assert_eq!(Preset::Flat.bands(), Some([0.0; 7]));
        assert_eq!(Preset::Custom.bands(), None);
        for p in Preset::ALL {
            if let Some(bands) = p.bands() {
                assert!(bands.iter().all(|db| db.abs() <= MAX_DB), "{p:?}");
                assert_eq!(p == Preset::Flat, bands.iter().all(|db| *db == 0.0), "{p:?}");
            }
        }
    }

    #[test]
    fn picking_a_preset_plays_its_bands() {
        let mut t = Tuning { enabled: true, ..Default::default() };
        t.pick(Preset::Vocal);
        assert_eq!(t.preset, Preset::Vocal);
        assert_eq!(Some(t.bands()), Preset::Vocal.bands());
        assert_eq!(t.settings(), Settings { enabled: true, bands: Preset::Vocal.bands().unwrap(), bass: 0.0 });
    }

    #[test]
    fn a_band_moved_by_hand_selects_custom_and_keeps_the_rest_of_the_curve() {
        let mut t = Tuning::default();
        t.pick(Preset::BassBoost);
        let was = t.bands();
        t.move_band(4, 5.0);
        assert_eq!(t.preset, Preset::Custom);
        let mut want = was;
        want[4] = 5.0;
        assert_eq!(t.bands(), want);
    }

    #[test]
    fn custom_remembers_the_bands_set_by_hand() {
        let mut t = Tuning::default();
        t.move_band(0, 7.0);
        t.move_band(6, -3.5);
        let mine = t.bands();
        t.pick(Preset::Electronic);
        assert_ne!(t.bands(), mine);
        t.pick(Preset::Custom);
        assert_eq!(t.bands(), mine);
    }

    #[test]
    fn a_band_and_the_bass_boost_stay_within_their_range() {
        let mut t = Tuning::default();
        t.move_band(2, 40.0);
        t.move_band(3, -40.0);
        t.move_band(9, 6.0);
        assert_eq!(t.bands(), [0.0, 0.0, 12.0, -12.0, 0.0, 0.0, 0.0]);
        t.set_bass(1.7);
        assert_eq!(t.bass, 1.0);
        t.set_bass(-0.2);
        assert_eq!(t.bass, 0.0);
    }

    #[test]
    fn the_bass_boost_is_kept_across_presets() {
        let mut t = Tuning::default();
        t.set_bass(0.6);
        t.pick(Preset::Acoustic);
        assert_eq!(t.settings().bass, 0.6);
        assert_eq!(t.preset, Preset::Acoustic, "the bass boost is not a band moved by hand");
    }

    #[test]
    fn what_was_set_survives_being_saved_and_read_back() {
        let mut t = Tuning::default();
        t.move_band(1, 4.5);
        t.move_band(5, -12.0);
        t.pick(Preset::Vocal);
        t.set_bass(0.35);
        t.enabled = true;
        let bass = t.bass.to_string();
        let back = Tuning::load(Some("1"), Some(t.preset.key()), Some(&t.custom_text()), Some(&bass));
        assert_eq!(back, t);
    }

    #[test]
    fn nothing_saved_or_a_damaged_save_starts_switched_off_and_flat() {
        assert_eq!(Tuning::load(None, None, None, None), Tuning::default());
        let t = Tuning::load(Some("1"), Some("disco"), Some("1,2,three"), Some("loud"));
        assert_eq!(t, Tuning { enabled: true, ..Default::default() });
        // Values past the range are brought back into it.
        let t = Tuning::load(Some("0"), Some("custom"), Some("99,-99,0,0,0,0,0"), Some("7"));
        assert_eq!(t, Tuning { enabled: false, preset: Preset::Custom, custom: [12.0, -12.0, 0.0, 0.0, 0.0, 0.0, 0.0], bass: 1.0 });
    }
}
