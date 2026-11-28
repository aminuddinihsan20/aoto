//! Audio info: the path a song takes from its file to the ears, stage by stage, as the
//! player really does it (the track, its decoder, what is done to the sound, the
//! conversion to the output's rate, and the output itself).

use egui::{Sense, Ui, pos2, vec2};

use super::{App, m3, sheet};
use crate::material::{self, Type};

/// One stage of the path: its name and what there is to say about it.
#[derive(Debug, PartialEq)]
pub(super) struct Stage {
    pub title: &'static str,
    pub lines: Vec<String>,
}

/// What the path is made from: the stream's numbers and the listener's sound settings.
pub(super) struct Facts<'a> {
    pub stats: &'a crate::stream::Stats,
    /// Present for a lossless or addon stream: its mark and where it is from.
    pub info: Option<&'a crate::hires::Info>,
    /// The output device's sample rate; 0 while none is open.
    pub output_rate: u32,
    pub output: String,
    /// What the output does to a lossless song, where there is something to say.
    pub output_note: Option<&'static str>,
    pub latency_ms: u32,
    pub eq: Option<&'static str>,
    /// The bass boost, 0 to 1.
    pub bass: f32,
    pub spatial: Option<&'static str>,
    pub normalize: bool,
    pub speed: f32,
    pub keep_pitch: bool,
    /// Automix, or a crossfade of so many seconds.
    pub between: Option<String>,
}

fn rate_text(hz: u32) -> String {
    if hz % 1000 == 0 { format!("{} kHz", hz / 1000) } else { format!("{:.1} kHz", hz as f64 / 1000.0) }
}

fn channels_text(n: u16) -> String {
    match n {
        1 => "Mono".to_owned(),
        2 => "Stereo".to_owned(),
        _ => format!("{n} channels"),
    }
}

/// The path, in the order the sound takes it.
pub(super) fn stages(f: &Facts) -> Vec<Stage> {
    let s = f.stats;
    // The track: what the file or stream is.
    let mut track = vec![match s.container {
        Some(container) => format!("{} in {container} · {}", s.codec, channels_text(s.channels)),
        None => format!("{} · {}", s.codec, channels_text(s.channels)),
    }];
    let mut numbers = vec![rate_text(s.sample_rate)];
    numbers.extend(s.bit_depth.map(|b| format!("{b}-bit")));
    // A lossless file's own bitrate is what its size says; a lossy stream has just the one.
    numbers.extend(s.file_kbps.or(s.bitrate_kbps).map(|k| format!("{k} kbps")));
    track.push(numbers.join(" · "));
    track.push(match f.info {
        Some(i) if i.source.is_empty() => i.badge().to_owned(),
        Some(i) => format!("{} · {}", i.badge(), i.source),
        None => "Lossy · YouTube Music".to_owned(),
    });

    // The decoder: everything after it is 32-bit float.
    let decoder = vec![s.decoder.to_owned(), match s.bit_depth {
        Some(bits) => format!("{bits}-bit → 32-bit float"),
        None => "To 32-bit float".to_owned(),
    }];

    // What is done to the sound, at the song's own rate.
    let mut dsp = Vec::new();
    if let Some(preset) = f.eq {
        dsp.push(format!("Equalizer: {preset}"));
    }
    if f.bass > 0.005 {
        dsp.push(format!("Bass boost: {:.0}%", f.bass * 100.0));
    }
    if let Some(level) = f.spatial {
        dsp.push(format!("Spatial audio: {level}"));
    }
    if f.normalize {
        dsp.push("Volume normalization".to_owned());
    }
    if (f.speed - 1.0).abs() > 0.001 {
        dsp.push(format!("Speed: {}, {}", crate::speed::label(f.speed), if f.keep_pitch { "pitch kept" } else { "pitch follows" }));
    }
    dsp.extend(f.between.clone());
    if dsp.is_empty() {
        dsp.push("Nothing is changed".to_owned());
    }
    dsp.insert(0, format!("{} · 32-bit float", rate_text(s.sample_rate)));

    // To the output's rate, when it is not the song's.
    let resample = if f.output_rate == 0 {
        vec!["No output is open".to_owned()]
    } else if f.output_rate == s.sample_rate {
        vec![format!("None: the output runs at {} too", rate_text(s.sample_rate))]
    } else {
        vec![format!("{} → {}", rate_text(s.sample_rate), rate_text(f.output_rate))]
    };

    let mut output = vec![f.output.clone()];
    if f.output_rate > 0 {
        output.push(format!("{} · about {} ms behind", rate_text(f.output_rate), f.latency_ms));
    }
    output.extend(f.output_note.map(str::to_owned));

    vec![
        Stage { title: "Track", lines: track },
        Stage { title: "Decoder", lines: decoder },
        Stage { title: "Sound", lines: dsp },
        Stage { title: "Resampling", lines: resample },
        Stage { title: "Output", lines: output },
    ]
}

/// The facts as the app has them now, once the decoder has said what it reads.
fn facts(app: &App) -> Option<(crate::stream::Stats, Option<crate::hires::Info>)> {
    let (_, stats) = app.stream_stats.as_ref()?;
    Some((stats.clone(), app.audio_info.as_ref().map(|(_, i)| i.clone())))
}

/// Dev hook: YTM_AUDIO_INFO opens the sheet, for screenshots.
pub(super) fn dev_hooks(app: &mut App) {
    app.audio_info_sheet = super::dev_var("YTM_AUDIO_INFO").is_some();
}

pub(super) fn sheet(app: &mut App, ctx: &egui::Context) {
    let mut open = app.audio_info_sheet;
    let now = facts(app);
    let out = crate::playon::output();
    let lossless = now.as_ref().is_some_and(|(_, i)| i.as_ref().is_some_and(|i| i.lossless() && !i.atmos));
    sheet::show(ctx, "audio-info", &mut open, |ui| {
        m3::sheet_header(ui, "Audio info", "From the file to your ears");
        let Some((stats, info)) = &now else {
            let m = material::scheme();
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
            m3::text(ui, pos2(r.left() + 8.0, r.top() + 12.0), "Nothing is playing yet.", Type::BodyLarge, m.on_surface_variant, r.width() - 16.0);
            return;
        };
        let name = if out.name.is_empty() { out.detail().to_owned() } else { format!("{} · {}", out.detail(), out.name) };
        let between = if app.automix { Some("Automix between songs".to_owned()) } else { app.crossfade.then(|| format!("Crossfade: {} s", app.crossfade_secs)) };
        let f = Facts {
            stats,
            info: info.as_ref(),
            output_rate: app.audio.status.output_rate(),
            output: name,
            output_note: lossless.then(|| out.kind.lossless_note()).flatten(),
            latency_ms: app.audio.status.latency().as_millis() as u32,
            eq: app.eq.enabled.then(|| app.eq.preset.name()),
            bass: if app.eq.enabled { app.eq.bass } else { 0.0 },
            spatial: app.spatial.then(|| app.spatial_level.label()),
            normalize: app.normalize,
            speed: app.speed,
            keep_pitch: app.keep_pitch,
            between,
        };
        contents(ui, &stages(&f));
    });
    app.audio_info_sheet = open;
}

/// The stages down the sheet: a dot for each on a line that runs through them all.
fn contents(ui: &mut Ui, stages: &[Stage]) {
    let m = material::scheme();
    let w = ui.available_width();
    let text_left = 36.0;
    let mut dots = Vec::with_capacity(stages.len());
    for stage in stages {
        let title = m3::galley(ui, stage.title, Type::TitleMedium, m.on_surface, w - text_left - 8.0);
        let lines: Vec<_> = stage.lines.iter().map(|l| m3::galley_wrapped(ui, l, Type::BodyMedium, m.on_surface_variant, w - text_left - 8.0, 3)).collect();
        let height = title.size().y + lines.iter().map(|g| g.size().y).sum::<f32>() + 14.0;
        let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
        dots.push(pos2(rect.left() + 18.0, rect.top() + title.size().y / 2.0));
        let mut y = rect.top();
        ui.painter().galley(pos2(rect.left() + text_left, y), title.clone(), m.on_surface);
        y += title.size().y;
        for g in lines {
            let h = g.size().y;
            ui.painter().galley(pos2(rect.left() + text_left, y), g, m.on_surface_variant);
            y += h;
        }
    }
    // The line through the stages, then a dot on it at each.
    if let (Some(first), Some(last)) = (dots.first(), dots.last()) {
        ui.painter().line_segment([*first, *last], egui::Stroke::new(2.0, m.outline_variant));
    }
    let look = material::style();
    for (i, dot) in dots.iter().enumerate() {
        let fill = if i == 0 { look.accent } else { m.surface_container_highest };
        ui.painter().circle_filled(*dot, 7.0, fill);
        ui.painter().circle_stroke(*dot, 7.0, egui::Stroke::new(1.5, m.outline));
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::Stats;

    fn flac() -> Stats {
        Stats { codec: "FLAC".into(), bit_depth: Some(16), sample_rate: 44_100, channels: 2, bitrate_kbps: Some(1411), file_kbps: Some(966), container: None, decoder: "Symphonia" }
    }

    fn plain(stats: &Stats) -> Facts<'_> {
        Facts { stats, info: None, output_rate: 48_000, output: "Wired".into(), output_note: None, latency_ms: 60, eq: None, bass: 0.0, spatial: None, normalize: false, speed: 1.0, keep_pitch: true, between: None }
    }

    fn lines<'a>(stages: &'a [Stage], title: &str) -> &'a [String] {
        &stages.iter().find(|s| s.title == title).unwrap().lines
    }

    #[test]
    fn the_path_runs_in_the_order_the_sound_takes_it() {
        let s = flac();
        let titles: Vec<_> = stages(&plain(&s)).iter().map(|s| s.title).collect();
        // The sound is changed at the song's rate, before the conversion to the output's.
        assert_eq!(titles, ["Track", "Decoder", "Sound", "Resampling", "Output"]);
    }

    #[test]
    fn a_flac_file_states_its_own_bitrate_and_depth() {
        let s = flac();
        let info = crate::probe::probe(b"fLaC").unwrap().info("This phone", 1, 0);
        let st = stages(&Facts { info: Some(&info), ..plain(&s) });
        assert_eq!(lines(&st, "Track"), ["FLAC · Stereo", "44.1 kHz · 16-bit · 966 kbps", "Lossless · This phone"]);
        assert_eq!(lines(&st, "Decoder"), ["Symphonia", "16-bit → 32-bit float"]);
    }

    #[test]
    fn a_youtube_stream_is_said_to_be_lossy() {
        let s = Stats { codec: "Opus".into(), bit_depth: None, sample_rate: 48_000, channels: 2, bitrate_kbps: Some(133), file_kbps: Some(133), container: Some("WebM"), decoder: "Opus (built in)" };
        let st = stages(&plain(&s));
        assert_eq!(lines(&st, "Track"), ["Opus in WebM · Stereo", "48 kHz · 133 kbps", "Lossy · YouTube Music"]);
        assert_eq!(lines(&st, "Decoder"), ["Opus (built in)", "To 32-bit float"]);
    }

    #[test]
    fn resampling_is_named_only_when_the_rates_differ() {
        let s = flac();
        assert_eq!(lines(&stages(&plain(&s)), "Resampling"), ["44.1 kHz → 48 kHz"]);
        assert_eq!(lines(&stages(&Facts { output_rate: 44_100, ..plain(&s) }), "Resampling"), ["None: the output runs at 44.1 kHz too"]);
        assert_eq!(lines(&stages(&Facts { output_rate: 0, ..plain(&s) }), "Resampling"), ["No output is open"]);
    }

    #[test]
    fn the_sound_stage_lists_only_what_is_switched_on() {
        let s = flac();
        assert_eq!(lines(&stages(&plain(&s)), "Sound"), ["44.1 kHz · 32-bit float", "Nothing is changed"]);
        let all = Facts { eq: Some("Bass boost"), bass: 0.4, spatial: Some("Strong"), normalize: true, speed: 1.25, keep_pitch: true, between: Some("Crossfade: 5 s".into()), ..plain(&s) };
        assert_eq!(
            lines(&stages(&all), "Sound"),
            ["44.1 kHz · 32-bit float", "Equalizer: Bass boost", "Bass boost: 40%", "Spatial audio: Strong", "Volume normalization", "Speed: 1.25×, pitch kept", "Crossfade: 5 s"]
        );
    }

    #[test]
    fn the_output_says_what_it_does_to_a_lossless_song() {
        let s = flac();
        let st = stages(&Facts { output: "Bluetooth · Buds".into(), output_note: Some("Bluetooth compresses it."), ..plain(&s) });
        assert_eq!(lines(&st, "Output"), ["Bluetooth · Buds", "48 kHz · about 60 ms behind", "Bluetooth compresses it."]);
    }
}
