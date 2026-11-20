//! Playback speed: a song played faster or slower than it was recorded, with its pitch
//! kept where it was (time stretching) or left to follow the speed (resampling).
//!
//! `Stretch` wraps a song's decoder. At 1× it hands the decoder's samples through
//! untouched. Keeping the pitch, it plays the song in short stretches taken from where
//! the song has got to at that speed, each one laid over the end of the one before at
//! the spot where the two sound most alike (WSOLA), and placed so that no drum hit or
//! consonant falls where the song is cut or repeated: those are heard once. Otherwise it reads the song through a low-pass interpolator, faster or
//! slower than it plays it. Every change of speed or of mode goes on from the input
//! sample after the last one heard, so nothing clicks and the song keeps its place.
//! Positions are counted in the song's own time.

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use rodio::source::SeekError;

/// The speeds on offer.
pub const SPEEDS: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// The speed on offer nearest to `speed` (what a stored setting is read as).
pub fn nearest(speed: f32) -> f32 {
    SPEEDS.into_iter().min_by(|a, b| (a - speed).abs().total_cmp(&(b - speed).abs())).unwrap_or(1.0)
}

/// A speed as its chip says it: "1.25×".
pub fn label(speed: f32) -> String {
    format!("{speed}×")
}

/// The speed the listener set, read by every song that plays.
pub struct Control {
    /// f32 bits
    speed: AtomicU32,
    keep_pitch: AtomicBool,
}

impl Default for Control {
    fn default() -> Self {
        Self { speed: AtomicU32::new(1.0f32.to_bits()), keep_pitch: AtomicBool::new(true) }
    }
}

impl Control {
    pub fn set(&self, speed: f32, keep_pitch: bool) {
        self.speed.store(speed.clamp(0.25, 4.0).to_bits(), Ordering::Relaxed);
        self.keep_pitch.store(keep_pitch, Ordering::Relaxed);
    }

    pub fn speed(&self) -> f32 {
        f32::from_bits(self.speed.load(Ordering::Relaxed))
    }

    pub fn keeps_pitch(&self) -> bool {
        self.keep_pitch.load(Ordering::Relaxed)
    }
}

/// Where in the song the sound being handed out is: from the decoder's start, or the
/// song's own time once it has been seeked. In microseconds.
#[derive(Default)]
pub struct Clock(AtomicU64);

impl Clock {
    pub fn get(&self) -> Duration {
        Duration::from_micros(self.0.load(Ordering::Relaxed))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// 1×: the decoder's samples as they are.
    Pass,
    /// Another speed at the song's own pitch.
    Stretch,
    /// Another speed, the pitch going with it.
    Resample,
}

/// Keeping the pitch: how long two stretches overlap, and how far either way from
/// where the song has got to the next one is looked for (20 ms in all: a whole period
/// of the lowest bass to line up on).
const OVERLAP_MS: f32 = 12.0;
const SEEK_MS: f32 = 10.0;

/// How long each stretch of the song is, keeping the pitch at `speed`. Slowed down, the
/// end of one stretch is heard again at the start of the next; sped up, the song
/// between two stretches is left out. Either is `STEP_MS` or so: short enough that what
/// is repeated or left out is a part of a sound, not a whole one. A stretch is no
/// shorter than twice the overlap and no longer than 80 ms.
fn stride_ms(speed: f32) -> f32 {
    (STEP_MS / (speed - 1.0).abs().max(0.01)).clamp(2.0 * OVERLAP_MS, 80.0)
}
const STEP_MS: f32 = 20.0;

/// Onsets (a drum hit, a consonant) are looked for in windows this long: one stands
/// out when it has this many times the treble of the 40 ms before it. Long enough that
/// the pulses of a low voice aren't taken for hits.
const ONSET_MS: f32 = 5.0;
const ONSET_RISE: f32 = 3.0;
/// Placing stretches clear of onsets, the song may run this far ahead of or behind
/// where the speed puts it; past that the next stretch goes where it must.
const SLACK_MS: f32 = 100.0;

/// Input frames kept behind the sound: what the interpolator reads on its near side.
const HISTORY: usize = 64;
/// How many frames go by at 1× between tidying the input kept.
const PASS_TIDY: usize = 4096;
/// The interpolator: a sinc cut off after this many zero crossings a side, tabulated
/// at this many points between crossings.
const ZEROS: usize = 16;
const PHASES: usize = 128;
/// Output frames the interpolator makes at a time.
const BLOCK: usize = 256;

/// One side of the windowed sinc (Blackman-Harris), from its centre outwards.
static SINC: LazyLock<Vec<f32>> = LazyLock::new(|| {
    (0..=ZEROS * PHASES + 1)
        .map(|i| {
            let x = i as f64 / PHASES as f64;
            let sinc = if i == 0 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
            let u = (x / ZEROS as f64).min(1.0) * std::f64::consts::PI;
            let window = 0.35875 + 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos() + 0.01168 * (3.0 * u).cos();
            (sinc * window) as f32
        })
        .collect()
});

/// A song at the speed `control` says, in place of its decoder.
pub struct Stretch<S> {
    inner: S,
    control: Arc<Control>,
    clock: Arc<Clock>,
    channels: usize,
    rate: u32,
    /// Input read from the decoder and still of use, interleaved.
    buf: Vec<f32>,
    /// The song's time at the start of `buf`, in seconds.
    origin: f64,
    /// The input frame (in `buf`) that carries the sound on as it was recorded.
    read: usize,
    /// The input frame the next output frame stands for; between two while resampling.
    at: f64,
    mode: Mode,
    /// Output made and not handed out yet, from `out_pos`; its first frame stands for
    /// input frame `out_from`, each one after for `out_step` more.
    out: Vec<f32>,
    out_pos: usize,
    out_from: f64,
    out_step: f64,
    /// The decoder has run out.
    ended: bool,
    /// How much treble each `ONSET_MS` of `buf` holds, and whether an onset is in it.
    flux: Vec<f32>,
    onset: Vec<bool>,
    template: Vec<f32>,
    region: Vec<f32>,
    acc: Vec<f32>,
}

impl<S: rodio::Source> Stretch<S> {
    pub fn new(inner: S, control: Arc<Control>) -> Self {
        let (channels, rate) = (inner.channels().get() as usize, inner.sample_rate().get());
        Self {
            inner,
            control,
            clock: Arc::default(),
            channels,
            rate,
            buf: Vec::new(),
            origin: 0.0,
            read: 0,
            at: 0.0,
            mode: Mode::Pass,
            out: Vec::new(),
            out_pos: 0,
            out_from: 0.0,
            out_step: 1.0,
            ended: false,
            flux: Vec::new(),
            onset: Vec::new(),
            template: Vec::new(),
            region: Vec::new(),
            acc: Vec::new(),
        }
    }

    /// The song's position as it is heard, for whoever shows or follows it.
    pub fn clock(&self) -> Arc<Clock> {
        self.clock.clone()
    }

    fn frames(&self) -> usize {
        self.buf.len() / self.channels
    }

    /// The sound has reached input frame `frame` (of `buf`).
    fn tick(&self, frame: f64) {
        let secs = (self.origin + frame / self.rate as f64).max(0.0);
        self.clock.0.store((secs * 1e6) as u64, Ordering::Relaxed);
    }

    /// How many frames an onset is looked for in at a time.
    fn hop(&self) -> usize {
        ((ONSET_MS * self.rate as f32 / 1000.0) as usize).max(1)
    }

    /// Forget the input before frame `keep` (or a little before it: whole onset windows).
    fn trim_to(&mut self, keep: usize) {
        if keep < 2048 {
            return;
        }
        let hop = self.hop();
        let keep = keep - keep % hop;
        let windows = (keep / hop).min(self.flux.len());
        self.flux.drain(..windows);
        self.onset.drain(..windows);
        if windows < keep / hop {
            // Onsets weren't being listened for (at 1×): start over from here.
            self.flux.clear();
            self.onset.clear();
        }
        self.buf.drain(..keep * self.channels);
        self.read -= keep;
        self.at -= keep as f64;
        self.out_from -= keep as f64;
        self.origin += keep as f64 / self.rate as f64;
    }

    /// A decoder can go on at another rate or channel count part way through a file:
    /// what was read before is of no use to what comes after.
    fn sync_format(&mut self) {
        let (channels, rate) = (self.inner.channels().get() as usize, self.inner.sample_rate().get());
        if channels != self.channels || rate != self.rate {
            self.origin += self.read as f64 / self.rate as f64;
            self.buf.clear();
            self.flux.clear();
            self.onset.clear();
            self.read = 0;
            self.at = 0.0;
            self.channels = channels;
            self.rate = rate;
        }
    }

    /// Listen for onsets in what `buf` has gained: a window with much more treble in it
    /// (the change from one sample to the next) than the windows before.
    fn hear_onsets(&mut self) {
        let (ch, hop) = (self.channels, self.hop());
        while (self.flux.len() + 1) * hop <= self.frames() {
            let k = self.flux.len();
            let from = (k * hop).max(1);
            let mut last: f32 = self.buf[(from - 1) * ch..from * ch].iter().sum();
            let mut treble = 0.0;
            for frame in self.buf[from * ch..(k + 1) * hop * ch].chunks_exact(ch) {
                let now: f32 = frame.iter().sum();
                treble += (now - last) * (now - last);
                last = now;
            }
            treble /= hop as f32;
            let before = &self.flux[k.saturating_sub(8)..k];
            let usual = before.iter().sum::<f32>() / before.len().max(1) as f32;
            self.onset.push(before.len() == 8 && treble > 1e-7 && treble > usual * ONSET_RISE);
            self.flux.push(treble);
        }
    }

    /// The frames around onset window `k` that must be heard whole: half a window either
    /// side of it.
    fn guarded(&self, k: usize) -> (usize, usize) {
        let hop = self.hop();
        ((k * hop).saturating_sub(hop / 2), (k + 1) * hop + hop / 2)
    }

    /// The first onset whose guarded frames reach into `from..to`, and the last.
    fn onsets_in(&self, from: usize, to: usize) -> (Option<(usize, usize)>, Option<(usize, usize)>) {
        let hop = self.hop();
        let windows = (from / hop).saturating_sub(1)..(to / hop + 2).min(self.onset.len());
        let mut hits = windows.filter(|k| self.onset[*k]).map(|k| self.guarded(k)).filter(|(start, end)| *start < to && *end > from);
        let first = hits.next();
        (first, hits.last().or(first))
    }

    /// Read on until `buf` holds `need` frames, or the song ends.
    fn fill(&mut self, need: usize) {
        while !self.ended && self.buf.len() < need * self.channels {
            match self.inner.next() {
                Some(s) => self.buf.push(s),
                None => {
                    self.ended = true;
                    let whole = self.frames() * self.channels;
                    self.buf.truncate(whole);
                }
            }
        }
    }

    /// Make the next stretch of output. False when the song is over.
    fn produce(&mut self) -> bool {
        self.out.clear();
        self.out_pos = 0;
        self.sync_format();
        let speed = self.control.speed();
        let mode = if speed == 1.0 {
            Mode::Pass
        } else if self.control.keeps_pitch() {
            Mode::Stretch
        } else {
            Mode::Resample
        };
        if mode != self.mode {
            // Whatever comes next goes on from the input sample after the last one heard.
            if self.mode == Mode::Resample {
                self.read = (self.at.ceil() as usize).min(self.frames());
            }
            self.at = self.read as f64;
            self.mode = mode;
        }
        match mode {
            Mode::Pass => {
                // What was read ahead of the sound, as it is; then the decoder's own samples.
                let from = self.read.min(self.frames());
                self.out.extend_from_slice(&self.buf[from * self.channels..]);
                self.out_from = from as f64;
                self.out_step = 1.0;
                self.read = self.frames();
                self.at = self.read as f64;
                !self.out.is_empty() || !self.ended
            }
            Mode::Stretch => self.stretch(speed),
            Mode::Resample => self.resample(speed),
        }
    }

    /// The next stretch of the song at its own pitch: taken from where the song has got
    /// to at this speed, moved to where it best carries on the sound already playing,
    /// and faded in over that.
    fn stretch(&mut self, speed: f32) -> bool {
        let ch = self.channels;
        let rate = self.rate as f32;
        let ms = move |m: f32| ((m * rate / 1000.0) as usize).max(2);
        let (stride, overlap, seek) = (ms(stride_ms(speed)), ms(OVERLAP_MS), ms(SEEK_MS));
        let target = self.at.round().max(0.0) as usize;
        // Where the speed puts the song may be well ahead of or behind the sound (a
        // stretch kept clear of an onset): it is caught up with two steps at a time.
        let reach = 2 * ms(STEP_MS);
        let centre = target.clamp(self.read.saturating_sub(reach), self.read + reach);
        let need = self.read.max(centre + seek) + stride;
        self.fill(need);
        let frames = self.frames();
        if frames < need {
            // The song's last moments as they are: it ends on its own last sample.
            let from = self.read.min(frames);
            self.out.extend_from_slice(&self.buf[from * ch..]);
            self.out_from = from as f64;
            self.out_step = 1.0;
            self.read = frames;
            self.at = frames as f64;
            return !self.out.is_empty();
        }
        self.hear_onsets();
        let s = if target == self.read {
            // Right where the sound is: that carries on as recorded, nothing to look for.
            target
        } else if target.abs_diff(self.read) > ms(SLACK_MS) {
            self.best(centre.saturating_sub(seek), centre + seek, overlap)
        } else {
            let (lo, hi) = self.clear_of_onsets(centre.saturating_sub(seek), centre + seek, overlap, seek);
            if lo == hi { lo } else { self.best(lo, hi, overlap) }
        };
        for i in 0..overlap {
            let w = 0.5 - 0.5 * (PI * (i as f32 + 0.5) / overlap as f32).cos();
            for c in 0..ch {
                let old = self.buf[(self.read + i) * ch + c];
                let new = self.buf[(s + i) * ch + c];
                self.out.push(old + (new - old) * w);
            }
        }
        self.out.extend_from_slice(&self.buf[(s + overlap) * ch..(s + stride) * ch]);
        // The position is where the sound was taken from, not where the speed aims.
        self.out_from = s as f64;
        self.out_step = 1.0;
        self.at += speed as f64 * stride as f64;
        self.read = s + stride;
        self.trim_to(self.read.min(self.at.max(0.0) as usize).saturating_sub(seek + reach + HISTORY));
        true
    }

    /// Where between `lo` and `hi` the next stretch may start so that every onset is
    /// heard once and whole: none in the song it leaves out (starting ahead of the
    /// sound), none in what it plays again (starting behind it), and none under the
    /// fade from the one to the other. With an onset just ahead, the sound carries on
    /// as recorded; with one in the way, the stretch starts as near as the onset allows.
    fn clear_of_onsets(&self, lo: usize, hi: usize, overlap: usize, seek: usize) -> (usize, usize) {
        let read = self.read;
        // An onset in what is about to fade out would be lost with it.
        if self.onsets_in(read, read + overlap).0.is_some() {
            return (read, read);
        }
        // Ahead: up to where the next onset still comes after the fade in.
        let ahead = self.onsets_in(read + overlap, hi.max(read) + overlap).0.map_or(usize::MAX, |(start, _)| start.saturating_sub(overlap).max(read));
        // Behind: from after the last onset heard, so only its ring plays again.
        let behind = self.onsets_in(lo.min(read), read).1.map_or(0, |(_, end)| end.min(read));
        let (from, to) = (lo.max(behind), hi.min(ahead));
        if from <= to {
            (from, to)
        } else if lo > ahead {
            (ahead.saturating_sub(2 * seek).max(read), ahead)
        } else {
            (behind, (behind + 2 * seek).min(read))
        }
    }

    /// Where between input frames `lo` and `hi` a stretch sounds most like the `overlap`
    /// frames that would carry the playing sound on: the two are compared as mono, first
    /// coarsely and then to the sample.
    fn best(&mut self, lo: usize, hi: usize, overlap: usize) -> usize {
        let ch = self.channels;
        let step = ((self.rate / 12_000).max(1) as usize).min(overlap / 4).max(1);
        let (mut template, mut region) = (std::mem::take(&mut self.template), std::mem::take(&mut self.region));
        mono(&self.buf, ch, self.read, overlap, step, &mut template);
        mono(&self.buf, ch, lo, hi - lo + overlap, step, &mut region);
        // Nothing to tell the places apart by (silence) leaves it where the song is.
        let middle = (hi - lo) / 2 / step;
        let coarse = most_alike(&template, &region, middle);
        let mut found = lo + coarse * step;
        if step > 1 {
            let from = found.saturating_sub(step - 1).max(lo);
            let to = (found + step - 1).min(hi);
            mono(&self.buf, ch, self.read, overlap, 1, &mut template);
            mono(&self.buf, ch, from, to - from + overlap, 1, &mut region);
            found = from + most_alike(&template, &region, found - from);
        }
        self.template = template;
        self.region = region;
        found
    }

    /// The song read through the interpolator at `speed` frames of it for each one out:
    /// nothing above what the new rate can carry gets through to fold back.
    fn resample(&mut self, speed: f32) -> bool {
        let ch = self.channels;
        let step = speed as f64;
        let cut = 0.9 * (1.0 / speed).min(1.0);
        let half = (ZEROS as f32 / cut).ceil() as usize;
        self.fill((self.at + step * BLOCK as f64).ceil() as usize + half + 1);
        let frames = self.frames();
        self.acc.resize(ch, 0.0);
        let mut made = 0;
        while made < BLOCK {
            let p = self.at + step * made as f64;
            if p >= frames as f64 {
                break;
            }
            let centre = p.floor() as isize;
            self.acc.fill(0.0);
            let mut norm = 0.0;
            for n in centre - half as isize + 1..=centre + half as isize {
                let x = (p - n as f64).abs() as f32 * cut * PHASES as f32;
                let i = x as usize;
                if i >= ZEROS * PHASES {
                    continue;
                }
                // At the song's two ends the interpolator reads what there is of it.
                if n < 0 || n as usize >= frames {
                    continue;
                }
                let w = SINC[i] + (SINC[i + 1] - SINC[i]) * (x - i as f32);
                norm += w;
                let frame = &self.buf[n as usize * ch..(n as usize + 1) * ch];
                for (a, s) in self.acc.iter_mut().zip(frame) {
                    *a += s * w;
                }
            }
            self.out.extend(self.acc.iter().map(|a| a / norm));
            made += 1;
        }
        self.out_from = self.at;
        self.out_step = step;
        self.at += step * made as f64;
        self.read = (self.at.ceil() as usize).min(frames);
        self.trim_to((self.at as usize).saturating_sub(half + HISTORY));
        made > 0
    }
}

/// `len` frames of `buf` from frame `from` as mono, `step` frames to a value.
fn mono(buf: &[f32], channels: usize, from: usize, len: usize, step: usize, out: &mut Vec<f32>) {
    out.clear();
    out.extend(buf[from * channels..(from + len) * channels].chunks_exact(step * channels).map(|c| c.iter().sum::<f32>()));
}

/// Where in `region` the sound is most like `template` (their normalized correlation);
/// `fallback` when no place is any better than another.
fn most_alike(template: &[f32], region: &[f32], fallback: usize) -> usize {
    let n = template.len();
    if n == 0 || region.len() < n {
        return 0;
    }
    let last = region.len() - n;
    let score = |at: usize| {
        let window = &region[at..at + n];
        let alike: f32 = template.iter().zip(window).map(|(a, b)| a * b).sum();
        let energy: f32 = window.iter().map(|b| b * b).sum();
        alike / (energy + 1e-9).sqrt()
    };
    let mut best = (fallback.min(last), score(fallback.min(last)));
    for at in 0..=last {
        let s = score(at);
        if s > best.1 * 1.0001 + 1e-9 {
            best = (at, s);
        }
    }
    best.0
}

impl<S: rodio::Source> Iterator for Stretch<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        loop {
            if let Some(&s) = self.out.get(self.out_pos) {
                self.out_pos += 1;
                if self.out_pos.is_multiple_of(self.channels) {
                    self.tick(self.out_from + (self.out_pos / self.channels) as f64 * self.out_step);
                }
                return Some(s);
            }
            // At 1× with nothing read ahead (or part way through a frame): the decoder's
            // own samples, kept only as the history another speed would start from.
            let part_way = !self.buf.len().is_multiple_of(self.channels);
            if part_way || (self.mode == Mode::Pass && !self.ended && self.read == self.frames() && self.control.speed() == 1.0) {
                let Some(s) = self.inner.next() else {
                    self.ended = true;
                    return None;
                };
                self.buf.push(s);
                if self.buf.len().is_multiple_of(self.channels) {
                    self.read += 1;
                    self.at = self.read as f64;
                    self.tick(self.at);
                    if self.read >= PASS_TIDY {
                        self.trim_to(self.read - HISTORY);
                        self.sync_format();
                    }
                }
                return Some(s);
            }
            if !self.produce() {
                return None;
            }
        }
    }
}

impl<S: rodio::Source> rodio::Source for Stretch<S> {
    /// Never `None` (rodio's resampler would keep this song's rate for the next one),
    /// and `Some(0)` once the song is over, as rodio's own sources say it.
    fn current_span_len(&self) -> Option<usize> {
        if self.out_pos < self.out.len() {
            Some(self.out.len() - self.out_pos)
        } else if self.mode == Mode::Pass && self.read == self.frames() {
            self.inner.current_span_len()
        } else if self.ended && self.read >= self.frames() {
            Some(0)
        } else {
            Some(BLOCK * self.channels)
        }
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

    /// `pos` is in the song's own time, whatever the speed.
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)?;
        self.buf.clear();
        self.flux.clear();
        self.onset.clear();
        self.out.clear();
        self.out_pos = 0;
        self.read = 0;
        self.at = 0.0;
        self.origin = pos.as_secs_f64();
        self.ended = false;
        self.tick(0.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    const RATE: u32 = 48_000;

    /// `secs` of a tone of `hz`, the same in every channel.
    fn tone(hz: f32, secs: f32, channels: usize) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize).flat_map(|i| std::iter::repeat_n((i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * 0.5, channels)).collect()
    }

    fn control(speed: f32, keep_pitch: bool) -> Arc<Control> {
        let c = Arc::new(Control::default());
        c.set(speed, keep_pitch);
        c
    }

    fn stretched(samples: Vec<f32>, channels: u16, control: &Arc<Control>) -> Stretch<SamplesBuffer> {
        Stretch::new(SamplesBuffer::new(NonZero::new(channels).unwrap(), NonZero::new(RATE).unwrap(), samples), control.clone())
    }

    /// The pitch of a mono tone, from where it crosses zero going up.
    fn pitch(x: &[f32]) -> f32 {
        let ups: Vec<f32> = x.windows(2).enumerate().filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0).map(|(i, w)| i as f32 + w[0] / (w[0] - w[1])).collect();
        (ups.len() - 1) as f32 / (ups[ups.len() - 1] - ups[0]) * RATE as f32
    }

    /// The furthest one sample is from the one before it.
    fn steepest(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn at_1x_a_song_comes_out_sample_for_sample() {
        for channels in [1, 2] {
            let input: Vec<f32> = (0..30_001).map(|i| (i as f32 * 0.37).sin()).collect();
            let c = control(1.0, true);
            let out: Vec<f32> = stretched(input.clone(), channels, &c).collect();
            assert!(out == input, "{channels} channels");
        }
    }

    #[test]
    fn keeping_the_pitch_a_tone_stays_in_tune_while_the_song_gets_shorter_or_longer() {
        let input = tone(440.0, 4.0, 1);
        for speed in SPEEDS {
            let out: Vec<f32> = stretched(input.clone(), 1, &control(speed, true)).collect();
            // Four seconds of song, but for its last tenth of a second, which plays as it is.
            let secs = out.len() as f32 / RATE as f32;
            assert!((secs - 4.0 / speed).abs() < 0.12, "at {speed}× the song took {secs} s");
            let hz = pitch(&out);
            assert!((hz - 440.0).abs() < 1.0, "at {speed}× the tone came out at {hz} Hz");
            assert!(steepest(&out) <= steepest(&input) * 1.02, "at {speed}× a sample jumps: {} against {}", steepest(&out), steepest(&input));
        }
    }

    #[test]
    fn a_stereo_song_keeps_its_two_channels_apart() {
        // A tone in the left ear, silence in the right.
        let input: Vec<f32> = tone(330.0, 2.0, 1).into_iter().flat_map(|s| [s, 0.0]).collect();
        for (speed, keep) in [(1.5, true), (0.75, true), (1.5, false), (0.75, false)] {
            let out: Vec<f32> = stretched(input.clone(), 2, &control(speed, keep)).collect();
            assert_eq!(out.len() % 2, 0);
            assert!(out.chunks(2).all(|f| f[1] == 0.0), "the right ear hears something at {speed}×");
            let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
            let want = if keep { 330.0 } else { 330.0 * speed };
            assert!((pitch(&left) - want).abs() < 1.5, "{} Hz at {speed}×", pitch(&left));
        }
    }

    #[test]
    fn music_with_more_than_one_note_in_it_plays_without_a_click() {
        // Three notes that share no period a stretch could line up on, and a slow swell.
        let n = RATE as usize * 3;
        let input: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let swell = 0.6 + 0.4 * (t * 2.3).sin();
                swell * 0.2 * ((t * 220.0 * std::f32::consts::TAU).sin() + (t * 277.2 * std::f32::consts::TAU).sin() + (t * 329.6 * std::f32::consts::TAU).sin())
            })
            .collect();
        for speed in SPEEDS {
            let out: Vec<f32> = stretched(input.clone(), 1, &control(speed, true)).collect();
            assert!(steepest(&out) <= steepest(&input) * 1.1, "at {speed}×: {} against {}", steepest(&out), steepest(&input));
            let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
            assert!((rms(&out) / rms(&input) - 1.0).abs() < 0.1, "at {speed}× it came out {} as loud", rms(&out) / rms(&input));
        }
    }

    #[test]
    fn without_keeping_the_pitch_the_tone_moves_with_the_speed() {
        let input = tone(440.0, 3.0, 1);
        for speed in [0.5, 0.75, 1.25, 1.5, 2.0] {
            let out: Vec<f32> = stretched(input.clone(), 1, &control(speed, false)).collect();
            let length = out.len() as f32 / input.len() as f32 * speed;
            assert!((length - 1.0).abs() < 0.01, "at {speed}× the song came out {length} of its length");
            // Clear of the song's two ends, where the interpolator has one side of it to read.
            let body = &out[2000..out.len() - 2000];
            let hz = pitch(body);
            assert!((hz - 440.0 * speed).abs() < 1.0, "at {speed}× the tone came out at {hz} Hz");
            let peak = body.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - 0.5).abs() < 0.01, "at {speed}× the tone peaks at {peak}");
            assert!(steepest(body) <= steepest(&input) * speed.max(1.0) * 1.02);
        }
    }

    #[test]
    fn resampling_faster_keeps_out_what_the_new_rate_cannot_carry() {
        // 15 kHz at 2× would be 30 kHz, past what 48 kHz holds: it must not come back as 18 kHz.
        let input = tone(15_000.0, 1.0, 1);
        let out: Vec<f32> = stretched(input, 1, &control(2.0, false)).collect();
        let peak = out[2000..out.len() - 2000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.005, "{peak} of the folded tone got through");
    }

    #[test]
    fn the_position_is_the_songs_own_time_at_every_speed() {
        let input = tone(440.0, 4.0, 2);
        for keep in [true, false] {
            for speed in SPEEDS {
                let mut s = stretched(input.clone(), 2, &control(speed, keep));
                let clock = s.clock();
                assert_eq!(clock.get(), Duration::ZERO);
                // One second of sound is `speed` seconds of the song.
                assert_eq!(s.by_ref().take(RATE as usize * 2).count(), RATE as usize * 2);
                let at = clock.get().as_secs_f32();
                // (Keeping the pitch, to within the step one stretch takes.)
                assert!((at - speed).abs() < 0.04, "a second in at {speed}× (pitch kept: {keep}) the song is at {at} s");
                s.by_ref().count();
                let end = clock.get().as_secs_f32();
                assert!((end - 4.0).abs() < 0.001, "at {speed}× the song ends at {end} s");
            }
        }
    }

    #[test]
    fn a_change_of_speed_mid_song_makes_no_step_and_keeps_its_place() {
        let input = tone(440.0, 6.0, 1);
        let c = control(1.0, true);
        let mut s = stretched(input.clone(), 1, &c);
        let clock = s.clock();
        let mut out: Vec<f32> = Vec::new();
        // Each change, and how far the song's position moved across it.
        for (speed, keep) in [(1.0, true), (1.5, true), (0.5, true), (2.0, true), (1.0, true), (0.75, true), (1.25, true), (1.0, true)] {
            let before = clock.get().as_secs_f32();
            c.set(speed, keep);
            out.extend(s.by_ref().take(1));
            // No further than the step every stretch takes.
            let moved = clock.get().as_secs_f32() - before;
            assert!(moved.abs() < 0.06, "changing to {speed}× moved the song by {moved} s");
            out.extend(s.by_ref().take(RATE as usize / 3));
        }
        assert!(steepest(&out) <= steepest(&input) * 1.02, "a sample jumps: {} against {}", steepest(&out), steepest(&input));
        assert!((pitch(&out) - 440.0).abs() < 1.0);
        // Back at 1× it is the decoder's own samples again, to the last one.
        let rest: Vec<f32> = s.collect();
        assert!(rest.len() > 1000);
        assert!(rest[..] == input[input.len() - rest.len()..]);
    }

    #[test]
    fn switching_the_pitch_keeping_mid_song_makes_no_step() {
        let input = tone(440.0, 4.0, 1);
        let c = control(1.5, true);
        let mut s = stretched(input.clone(), 1, &c);
        let clock = s.clock();
        let mut out: Vec<f32> = Vec::new();
        for (speed, keep) in [(1.5, true), (1.5, false), (1.0, true), (0.5, false), (0.5, true), (2.0, false)] {
            let before = clock.get().as_secs_f32();
            c.set(speed, keep);
            out.extend(s.by_ref().take(1));
            let moved = clock.get().as_secs_f32() - before;
            assert!(moved.abs() < 0.06, "changing to {speed}× (pitch kept: {keep}) moved the song by {moved} s");
            out.extend(s.by_ref().take(RATE as usize / 4));
        }
        // The fastest it gets is the tone an octave up, at 2× without its pitch kept.
        assert!(steepest(&out) <= steepest(&input) * 2.0 * 1.02, "a sample jumps: {} against {}", steepest(&out), steepest(&input));
    }

    #[test]
    fn at_any_speed_a_song_starts_on_its_first_sample_and_ends_on_its_last() {
        // A song that is its own position, so each sample says where it came from.
        let n = RATE as usize * 2;
        let input: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        for speed in SPEEDS {
            let out: Vec<f32> = stretched(input.clone(), 1, &control(speed, true)).collect();
            assert_eq!(out[..100], input[..100], "the start at {speed}×");
            assert_eq!(out[out.len() - 100..], input[n - 100..], "the end at {speed}×");
        }
    }

    #[test]
    fn seeking_lands_where_asked_at_any_speed() {
        let n = RATE as usize * 4;
        let input: Vec<f32> = (0..n).map(|i| i as f32 / RATE as f32).collect();
        for (speed, keep) in [(1.0, true), (1.5, true), (0.5, true), (2.0, false), (0.75, false)] {
            let mut s = stretched(input.clone(), 1, &control(speed, keep));
            let clock = s.clock();
            s.by_ref().take(10_000).count();
            s.try_seek(Duration::from_secs(3)).unwrap();
            assert_eq!(clock.get(), Duration::from_secs(3));
            // The song says its own time: the first thing heard is the third second.
            let heard = s.next().unwrap();
            assert!((heard - 3.0).abs() < 0.001, "at {speed}× a seek to 3 s landed at {heard} s");
            s.by_ref().take(RATE as usize / 4).count();
            let at = clock.get().as_secs_f32();
            assert!((at - (3.0 + 0.25 * speed)).abs() < 0.04, "a quarter second later at {speed}× it is at {at} s");
        }
    }

    #[test]
    fn silence_comes_out_as_silence_of_the_right_length() {
        // A second of it, in stereo.
        let input = vec![0.0f32; RATE as usize * 2];
        for speed in SPEEDS {
            let out: Vec<f32> = stretched(input.clone(), 2, &control(speed, true)).collect();
            assert!(out.iter().all(|s| *s == 0.0));
            let secs = out.len() as f32 / (RATE * 2) as f32;
            assert!((secs - 1.0 / speed).abs() < 0.12, "at {speed}× a second of silence took {secs} s");
        }
    }

    #[test]
    fn a_song_shorter_than_one_stretch_still_plays_whole() {
        let input: Vec<f32> = (0..500).map(|i| (i as f32 * 0.1).sin()).collect();
        for (speed, keep) in [(0.5, true), (2.0, true), (2.0, false)] {
            let out: Vec<f32> = stretched(input.clone(), 1, &control(speed, keep)).collect();
            assert!(!out.is_empty() && out.len() <= 1100, "{} samples at {speed}×", out.len());
        }
    }

    #[test]
    fn the_span_is_never_unknown_and_says_when_the_song_is_over() {
        for (speed, keep) in [(1.0, true), (1.5, true), (0.75, false)] {
            let mut s = stretched(tone(440.0, 0.5, 2), 2, &control(speed, keep));
            while s.next().is_some() {
                assert!(s.current_span_len().is_some());
            }
            assert_eq!(s.current_span_len(), Some(0), "at {speed}×");
        }
    }

    #[test]
    fn a_stretch_repeats_or_leaves_out_about_a_fiftieth_of_a_second() {
        for speed in SPEEDS.into_iter().filter(|s| *s != 1.0) {
            let stride = stride_ms(speed);
            assert!((24.0..=80.0).contains(&stride), "{stride} ms at {speed}×");
            assert!(stride * (speed - 1.0).abs() <= 24.0, "{} ms at {speed}×", stride * (speed - 1.0).abs());
        }
        assert_eq!(stride_ms(1.0), 80.0);
    }

    /// Drums over a held chord: a hit of noise every `beat` seconds, the first half a
    /// beat in.
    fn beats(secs: f32, beat: f32) -> Vec<f32> {
        let mut seed = 12345u32;
        (0..(RATE as f32 * secs) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
                let since = (t + beat / 2.0) % beat;
                let hit = if since < 0.03 { noise * (-since / 0.006).exp() * 0.6 } else { 0.0 };
                hit + 0.08 * ((t * 196.0 * std::f32::consts::TAU).sin() + (t * 246.9 * std::f32::consts::TAU).sin())
            })
            .collect()
    }

    /// When the hits in `x` start, in seconds: where 10 ms are suddenly much louder than
    /// the 30 ms before them.
    fn onsets(x: &[f32]) -> Vec<f32> {
        let hop = RATE as usize / 200;
        let energy: Vec<f32> = x.chunks(hop).map(|c| c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).collect();
        let mut found: Vec<f32> = Vec::new();
        for i in 6..energy.len() - 1 {
            let before = energy[i - 6..i].iter().sum::<f32>() / 6.0;
            let now = (energy[i] + energy[i + 1]) / 2.0;
            let t = i as f32 * hop as f32 / RATE as f32;
            if now > 0.01 && now > before * 3.0 && found.last().is_none_or(|last| t - last > 0.045) {
                found.push(t);
            }
        }
        found
    }

    #[test]
    fn every_drum_hit_is_heard_once_and_on_the_beat_at_any_speed() {
        let (beat, secs) = (0.25, 8.0);
        let input = beats(secs, beat);
        assert_eq!(onsets(&input).len(), 32, "the detector hears the 32 hits as recorded");
        for speed in SPEEDS {
            let out: Vec<f32> = stretched(input.clone(), 1, &control(speed, true)).collect();
            let hits = onsets(&out);
            // None doubled into a flam, none left out.
            // None doubled into a flam, none left out.
            assert_eq!(hits.len(), 32, "{} hits at {speed}×: {hits:?}", hits.len());
            // Each close to where the beat puts it, and the beat itself steady: slowed
            // down, a hit is heard the first of the two times its place in the song is
            // played, so all of them come a little early, by much the same.
            for (n, at) in hits.iter().enumerate() {
                let want = (n as f32 + 0.5) * beat / speed;
                assert!((at - want).abs() < 0.08, "hit {n} at {speed}× came at {at} s, not {want} s");
            }
            for pair in hits.windows(2) {
                let gap = pair[1] - pair[0];
                assert!((gap - beat / speed).abs() < 0.045, "at {speed}× two hits came {gap} s apart, not {} s", beat / speed);
            }
        }
    }

    #[test]
    fn a_stored_speed_reads_as_one_on_offer() {
        assert_eq!(nearest(1.25), 1.25);
        assert_eq!(nearest(1.3), 1.25);
        assert_eq!(nearest(9.0), 2.0);
        assert_eq!(nearest(0.0), 0.5);
        assert_eq!(nearest(f32::NAN), 0.5);
        assert_eq!(SPEEDS.map(label), ["0.5×", "0.75×", "1×", "1.25×", "1.5×", "2×"]);
    }
}
