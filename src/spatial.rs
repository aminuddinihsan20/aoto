//! Spatial audio for headphones: a stereo song heard as if it came from speakers in a
//! room around you, not from inside the head.
//!
//! Three things, applied to each left/right pair:
//! - **Width**: the side signal (what differs between the ears), boosted, more in the highs.
//! - **Ambience**: the side signal again, a little later (14 and 19 ms, opposite in sign
//!   for the two ears, the highs rolled off): the early reflections of a room.
//! - **Head**: each ear also hears a little of the other side, about 0.3 ms late and
//!   dulled: the head's shadow, which is what makes sound outside it seem to be there.
//!
//! The centre (what is the same in both ears) is left alone, so voices stay put. This is
//! not head-tracked and not Dolby Atmos: it is a virtual surround for any stereo song.

/// One-pole low-pass: `y += k (x - y)`.
#[derive(Clone, Copy, Default)]
struct LowPass {
    y: f32,
}

impl LowPass {
    fn run(&mut self, x: f32, k: f32) -> f32 {
        self.y += k * (x - self.y);
        self.y
    }
}

/// A delay of whole samples.
#[derive(Clone, Default)]
struct Delay {
    buf: Vec<f32>,
    at: usize,
}

impl Delay {
    fn new(samples: usize) -> Self {
        Self { buf: vec![0.0; samples.max(1)], at: 0 }
    }

    /// Put `x` in and take out what went in `len` samples ago.
    fn run(&mut self, x: f32) -> f32 {
        let out = self.buf[self.at];
        self.buf[self.at] = x;
        self.at = (self.at + 1) % self.buf.len();
        out
    }
}

/// How strongly the effect widens the stereo image (Settings › Sound & quality › Spatial
/// audio strength). Light is how it first shipped; Strong is the widest, and the one that
/// is easy to hear on ordinary earphones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    Light,
    Medium,
    #[default]
    Strong,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Light, Level::Medium, Level::Strong];

    pub fn label(self) -> &'static str {
        match self {
            Level::Light => "Light",
            Level::Medium => "Medium",
            Level::Strong => "Strong",
        }
    }

    /// The stored form: a name, so a later reordering can't change what a saved choice means.
    pub fn code(self) -> &'static str {
        match self {
            Level::Light => "light",
            Level::Medium => "medium",
            Level::Strong => "strong",
        }
    }

    /// Anything unknown (or nothing) is the default.
    pub fn from_code(code: &str) -> Level {
        Level::ALL.into_iter().find(|l| l.code() == code.trim()).unwrap_or_default()
    }

    /// How many times louder the side signal (what differs between the ears) is made.
    fn width(self) -> f32 {
        match self {
            Level::Light => 1.35,
            Level::Medium => 1.9,
            Level::Strong => 2.5,
        }
    }

    /// The position in `ALL`, for an atomic.
    pub fn index(self) -> u8 {
        Level::ALL.iter().position(|l| *l == self).unwrap_or(0) as u8
    }

    pub fn from_index(i: u8) -> Level {
        Level::ALL.get(i as usize).copied().unwrap_or_default()
    }
}

pub struct Spatial {
    side_low: LowPass,
    ambience_a: Delay,
    ambience_b: Delay,
    ambience_low_a: LowPass,
    ambience_low_b: LowPass,
    head_l: Delay,
    head_r: Delay,
    head_low_l: LowPass,
    head_low_r: LowPass,
    /// How wide the side signal is made (`Level::width`).
    width: f32,
    /// Filter coefficients for this sample rate.
    k_air: f32,
    k_ambience: f32,
    k_head: f32,
}

/// `k` of a one-pole low-pass at `hz`.
fn coefficient(hz: f32, sample_rate: u32) -> f32 {
    1.0 - (-2.0 * std::f32::consts::PI * hz / sample_rate as f32).exp()
}

/// Keeps a peak under 1.0: untouched up to 0.8, then eased in.
fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a < 0.8 { x } else { x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh()) }
}

impl Spatial {
    pub fn new(sample_rate: u32) -> Self {
        let ms = |t: f32| ((sample_rate as f32 * t / 1000.0).round() as usize).max(1);
        Self {
            side_low: LowPass::default(),
            ambience_a: Delay::new(ms(14.0)),
            ambience_b: Delay::new(ms(19.0)),
            ambience_low_a: LowPass::default(),
            ambience_low_b: LowPass::default(),
            head_l: Delay::new(ms(0.3)),
            head_r: Delay::new(ms(0.3)),
            head_low_l: LowPass::default(),
            head_low_r: LowPass::default(),
            width: Level::default().width(),
            k_air: coefficient(1500.0, sample_rate),
            k_ambience: coefficient(6000.0, sample_rate),
            k_head: coefficient(700.0, sample_rate),
        }
    }

    /// How wide the image is made, from the next sample on.
    pub fn set_level(&mut self, level: Level) {
        self.width = level.width();
    }

    /// Forget what was heard (after a seek).
    pub fn reset(&mut self) {
        *self = Self { width: self.width, k_air: self.k_air, k_ambience: self.k_ambience, k_head: self.k_head, ..Self::with_lengths(&self.ambience_a, &self.ambience_b, &self.head_l) };
    }

    fn with_lengths(a: &Delay, b: &Delay, head: &Delay) -> Self {
        Self {
            side_low: LowPass::default(),
            ambience_a: Delay::new(a.buf.len()),
            ambience_b: Delay::new(b.buf.len()),
            ambience_low_a: LowPass::default(),
            ambience_low_b: LowPass::default(),
            head_l: Delay::new(head.buf.len()),
            head_r: Delay::new(head.buf.len()),
            head_low_l: LowPass::default(),
            head_low_r: LowPass::default(),
            width: Level::default().width(),
            k_air: 0.0,
            k_ambience: 0.0,
            k_head: 0.0,
        }
    }

    /// One left/right pair in, the spatial one out.
    pub fn frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5;
        // Width, more of it in the highs.
        let air = side - self.side_low.run(side, self.k_air);
        let wide = side * self.width + air * 0.35;
        // Ambience: the side signal later, opposite for the two ears.
        let a = self.ambience_low_a.run(self.ambience_a.run(wide), self.k_ambience) * 0.22;
        let b = self.ambience_low_b.run(self.ambience_b.run(wide), self.k_ambience) * 0.22;
        let (out_l, out_r) = (mid + wide + a, mid - wide - b);
        // The head's shadow: some of the other ear's sound, late and dulled.
        let to_l = self.head_low_l.run(self.head_r.run(out_r), self.k_head) * 0.2;
        let to_r = self.head_low_r.run(self.head_l.run(out_l), self.k_head) * 0.2;
        (soft_limit((out_l + to_l) * 0.8), soft_limit((out_r + to_r) * 0.8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sp: &mut Spatial, frames: usize, f: impl Fn(usize) -> (f32, f32)) -> Vec<(f32, f32)> {
        (0..frames).map(|i| { let (l, r) = f(i); sp.frame(l, r) }).collect()
    }

    #[test]
    fn silence_stays_silent() {
        let mut sp = Spatial::new(48_000);
        assert!(run(&mut sp, 2_000, |_| (0.0, 0.0)).iter().all(|&(l, r)| l == 0.0 && r == 0.0));
    }

    #[test]
    fn a_centred_voice_stays_in_the_middle() {
        let mut sp = Spatial::new(48_000);
        let out = run(&mut sp, 4_000, |i| { let v = (i as f32 * 0.05).sin() * 0.5; (v, v) });
        assert!(out.iter().all(|&(l, r)| (l - r).abs() < 1e-6), "mono in, the same in both ears out");
        assert!(out.iter().any(|&(l, _)| l.abs() > 0.1), "and it is still there");
    }

    #[test]
    fn a_sound_on_the_left_is_also_heard_a_little_by_the_right_ear_but_less() {
        let mut sp = Spatial::new(48_000);
        let out = run(&mut sp, 4_000, |i| ((i as f32 * 0.07).sin() * 0.6, 0.0));
        let (left, right): (f32, f32) = out.iter().skip(500).fold((0.0, 0.0), |(a, b), &(l, r)| (a + l * l, b + r * r));
        assert!(right > 0.0, "the right ear hears something");
        assert!(right < left, "but less than the left");
    }

    #[test]
    fn what_differs_between_the_ears_is_made_wider() {
        let mut sp = Spatial::new(48_000);
        let input = |i: usize| { let v = (i as f32 * 0.03).sin() * 0.3; (v, -v) };
        let out = run(&mut sp, 6_000, input);
        let (before, after): (f32, f32) = out.iter().enumerate().skip(1_000).fold((0.0, 0.0), |(b, a), (i, &(l, r))| {
            let (il, ir) = input(i);
            (b + (il - ir).abs(), a + (l - r).abs())
        });
        assert!(after > before * 0.9, "the side signal is not narrowed ({after} vs {before})");
    }

    #[test]
    fn a_loud_song_never_goes_past_full_scale() {
        let mut sp = Spatial::new(44_100);
        let mut x: u32 = 12345;
        let mut noise = move || { x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0 };
        let out: Vec<(f32, f32)> = (0..20_000).map(|_| { let (a, b) = (noise(), noise()); sp.frame(a.signum(), b.signum()) }).collect();
        assert!(out.iter().all(|&(l, r)| l.abs() <= 1.0 && r.abs() <= 1.0));
    }

    #[test]
    fn forgetting_what_was_heard_starts_from_silence() {
        let mut sp = Spatial::new(48_000);
        run(&mut sp, 3_000, |i| ((i as f32 * 0.1).sin(), (i as f32 * 0.13).sin()));
        sp.reset();
        let out = run(&mut sp, 50, |_| (0.0, 0.0));
        assert!(out.iter().all(|&(l, r)| l == 0.0 && r == 0.0), "no tail left over from before the seek");
    }

    #[test]
    fn a_rate_change_keeps_delays_in_milliseconds() {
        let slow = Spatial::new(22_050);
        let fast = Spatial::new(96_000);
        assert!(fast.ambience_a.buf.len() > 4 * slow.ambience_a.buf.len() - 4);
    }

    fn side_energy(level: Level) -> f32 {
        let mut sp = Spatial::new(48_000);
        sp.set_level(level);
        // A song with something different in each ear.
        let out = run(&mut sp, 8_000, |i| ((i as f32 * 0.03).sin() * 0.3, (i as f32 * 0.05).sin() * 0.3));
        out.iter().skip(1_000).map(|&(l, r)| (l - r).powi(2)).sum()
    }

    #[test]
    fn a_stronger_level_makes_the_image_wider() {
        let (light, medium, strong) = (side_energy(Level::Light), side_energy(Level::Medium), side_energy(Level::Strong));
        assert!(light < medium && medium < strong, "{light} < {medium} < {strong}");
        assert!(strong > light * 2.0, "strong is clearly wider than light ({strong} vs {light})");
    }

    #[test]
    fn the_strongest_level_still_never_goes_past_full_scale() {
        for level in Level::ALL {
            let mut sp = Spatial::new(44_100);
            sp.set_level(level);
            let mut x: u32 = 777;
            let mut noise = move || { x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0 };
            let out: Vec<(f32, f32)> = (0..20_000).map(|_| { let (a, b) = (noise(), noise()); sp.frame(a.signum(), b.signum()) }).collect();
            assert!(out.iter().all(|&(l, r)| l.abs() <= 1.0 && r.abs() <= 1.0), "{level:?}");
        }
    }

    #[test]
    fn a_centred_voice_stays_in_the_middle_at_every_level() {
        for level in Level::ALL {
            let mut sp = Spatial::new(48_000);
            sp.set_level(level);
            let out = run(&mut sp, 4_000, |i| { let v = (i as f32 * 0.05).sin() * 0.5; (v, v) });
            assert!(out.iter().all(|&(l, r)| (l - r).abs() < 1e-6), "{level:?}");
        }
    }

    #[test]
    fn a_seek_keeps_the_chosen_level() {
        let mut sp = Spatial::new(48_000);
        sp.set_level(Level::Light);
        sp.reset();
        assert_eq!(sp.width, Level::Light.width());
    }

    #[test]
    fn the_level_is_stored_by_name_and_unknown_means_strong() {
        for l in Level::ALL {
            assert_eq!(Level::from_code(l.code()), l);
            assert_eq!(Level::from_index(l.index()), l);
        }
        assert_eq!(Level::from_code(""), Level::Strong);
        assert_eq!(Level::from_code("2"), Level::Strong);
        assert_eq!(Level::from_index(9), Level::Strong);
        assert_eq!(Level::ALL.map(Level::label), ["Light", "Medium", "Strong"]);
    }
}
