//! The Equalizer page: its switch, the presets, the seven bands in a card with the curve
//! they make, the bass boost, and the two sound settings that live beside it (spatial
//! audio and volume normalization, the same ones Settings has). The sound itself is
//! `crate::eq`.

use egui::{Align2, Color32, Rect, Sense, Ui, pos2, vec2};

use super::widgets::{self, named};
use super::{App, pages};
use crate::eq::{BANDS, Curve, MAX_DB, Preset, Tuning};
use crate::theme;

/// What the page shows and changes.
#[derive(Clone, Debug, PartialEq)]
struct Showing {
    tuning: Tuning,
    spatial: bool,
    normalize: bool,
}

/// The page, in the app: what is changed on it goes to the player at once.
pub(super) fn page(app: &mut App, ui: &mut Ui) {
    let mut s = Showing { tuning: app.eq.clone(), spatial: app.spatial, normalize: app.normalize };
    // One column, as on a phone, also where there is room for more.
    let w = ui.available_width().min(520.0);
    ui.scope(|ui| {
        ui.set_max_width(w);
        contents(ui, &mut s, app.accent_color());
    });
    if s.tuning != app.eq {
        app.eq = s.tuning;
        app.audio.set_equalizer(&app.eq.settings());
    }
    if s.spatial != app.spatial {
        app.spatial = s.spatial;
        app.set_crossfade();
    }
    if s.normalize != app.normalize {
        app.normalize = s.normalize;
        app.audio.set_normalize(app.normalize);
    }
}

fn switch_id() -> egui::Id {
    egui::Id::unique("eq-switch")
}

fn preset_id(preset: Preset) -> egui::Id {
    egui::Id::unique(("eq-preset", preset))
}

fn band_id(band: usize) -> egui::Id {
    egui::Id::unique(("eq-band", band))
}

fn bass_id() -> egui::Id {
    egui::Id::unique("eq-bass")
}

fn spatial_id() -> egui::Id {
    egui::Id::unique("eq-spatial")
}

fn normalize_id() -> egui::Id {
    egui::Id::unique("eq-normalize")
}

/// The line under "Equalizer" in Settings: what it is, and what it is doing.
pub(super) fn settings_note(tuning: &Tuning) -> String {
    let now = if tuning.enabled { format!("On: {}", tuning.preset.name()) } else { "Off".to_owned() };
    format!("Seven bands, presets and a bass boost. {now}.")
}

/// Three sliders in the icon font's line (the font has none): Lucide's, on its 24-unit
/// grid.
pub(super) fn sliders(ui: &Ui, center: egui::Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let at = |x: f32, y: f32| center + vec2(x - 12.0, y - 12.0) * k;
    let stroke = egui::Stroke::new(2.0 * k, color);
    // Each slider's line in two parts, and the knob across the end of one of them.
    for (x, upper, lower, knob) in [(4.0f32, 10.0f32, 14.0f32, 14.0f32), (12.0, 8.0, 12.0, 8.0), (20.0, 12.0, 16.0, 16.0)] {
        ui.painter().line_segment([at(x, 3.0), at(x, upper)], stroke);
        ui.painter().line_segment([at(x, lower), at(x, 21.0)], stroke);
        ui.painter().line_segment([at(x - 2.0, knob), at(x + 2.0, knob)], stroke);
    }
}

/// What the boosts cost in loudness, when they cost any.
fn headroom_note(tuning: &Tuning) -> Option<String> {
    let db = Curve::new(&tuning.settings(), DRAWN_RATE).headroom_db();
    (db >= 0.5).then(|| format!("Plays {db:.0} dB quieter, to leave the boosts room"))
}

/// How tall the room round a band's thumb is that a finger takes hold of it by.
const THUMB_REACH: f32 = 56.0;
/// How tall the band card is.
const CARD_H: f32 = 300.0;
/// The rate the drawn curve is worked out at. What plays is worked out at each song's
/// own; below the top of hearing the two look the same.
const DRAWN_RATE: u32 = 48_000;

/// A band's frequency under its slider: "60", "1k", "2.4k".
fn hz_label(hz: f32) -> String {
    if hz < 1000.0 { format!("{hz}") } else { format!("{}k", hz / 1000.0) }
}

/// Where each of `widths` goes in rows no wider than `width`, `gap` apart: its row and
/// its left edge.
fn wrap(widths: &[f32], width: f32, gap: f32) -> Vec<(usize, f32)> {
    let (mut row, mut x) = (0, 0.0);
    widths
        .iter()
        .map(|w| {
            if x > 0.0 && x + w > width {
                row += 1;
                x = 0.0;
            }
            let at = (row, x);
            x += w + gap;
            at
        })
        .collect()
}

/// The band card's sliders: where +12 and −12 dB are, and each band's column.
#[derive(Clone, Copy, Debug)]
struct Grid {
    top: f32,
    bottom: f32,
    left: f32,
    slot: f32,
}

impl Grid {
    /// The sliders of a card: room at its left for the scale, below for the frequencies.
    fn of(card: Rect) -> Grid {
        let (left, right) = (card.left() + 36.0, card.right() - 4.0);
        Grid { top: card.top() + 32.0, bottom: card.bottom() - 60.0, left, slot: (right - left) / BANDS.len() as f32 }
    }

    fn x(&self, band: usize) -> f32 {
        self.left + self.slot * (band as f32 + 0.5)
    }

    fn y(&self, db: f32) -> f32 {
        egui::remap(db, MAX_DB..=-MAX_DB, self.top..=self.bottom)
    }

    /// The gain a finger at `y` sets: the nearest half decibel, within the range.
    fn db(&self, y: f32) -> f32 {
        let db = egui::remap_clamp(y, self.top..=self.bottom, MAX_DB..=-MAX_DB);
        (db * 2.0).round() / 2.0
    }

    /// The frequency drawn at `x`: each band's own under its slider, and evenly by
    /// pitch between two.
    fn hz(&self, x: f32) -> f32 {
        let at = (x - self.x(0)) / self.slot;
        // Past the outer sliders the pitch goes on at the pace of the last pair.
        let i = (at.floor().max(0.0) as usize).min(BANDS.len() - 2);
        BANDS[i] * (BANDS[i + 1] / BANDS[i]).powf(at - i as f32)
    }
}

/// The page's title with the equalizer's switch at its right.
fn header(ui: &mut Ui, on: &mut bool) {
    ui.add_space(12.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, "Equalizer", theme::bold(34.0), theme::text());
    // A finger's room round the switch.
    let reach = Rect::from_min_max(pos2(r.right() - 64.0, r.top()), r.max);
    if named(ui.interact(reach, switch_id(), Sense::click()), "Equalizer").clicked() {
        *on = !*on;
    }
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(44.0, 26.0))));
    pages::toggle(&mut sui, on, "Equalizer");
}

/// The presets as chips, on as many rows as they need: all of them in sight, so the
/// one that plays (Custom, the moment a band is moved) is seen to light up.
fn presets(ui: &mut Ui, tuning: &mut Tuning) {
    const ROW: f32 = 44.0;
    let widths = Preset::ALL.map(|p| ui.painter().layout_no_wrap(p.name().to_owned(), theme::semibold(13.5), Color32::PLACEHOLDER).size().x + 28.0);
    let places = wrap(&widths, ui.available_width(), 8.0);
    let rows = places.last().map_or(0, |(row, _)| row + 1);
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), rows as f32 * ROW), Sense::hover());
    for ((preset, width), (row, x)) in Preset::ALL.into_iter().zip(widths).zip(places) {
        // The chip, and a finger's height round it.
        let cell = Rect::from_min_size(pos2(area.left() + x, area.top() + row as f32 * ROW), vec2(width, ROW));
        let reach = named(ui.interact(cell, preset_id(preset), Sense::click()), preset.name());
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(cell.center(), vec2(width, 34.0))));
        if widgets::chip(&mut cui, preset.name(), tuning.preset == preset).clicked() || reach.clicked() {
            tuning.pick(preset);
        }
    }
}

/// The seven bands as sliders in one card, with the curve they and the bass boost
/// make together drawn through it.
fn bands(ui: &mut Ui, tuning: &mut Tuning, accent: Color32) {
    let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), CARD_H), Sense::hover());
    ui.painter().rect_filled(card, theme::panel_radius(), theme::glass(0.06));
    let grid = Grid::of(card);
    let (left, right) = (grid.left, card.right() - 4.0);
    for (db, label) in [(MAX_DB, "+12"), (0.0, "0"), (-MAX_DB, "−12")] {
        let y = grid.y(db);
        ui.painter().hline(left..=right - 8.0, y, egui::Stroke::new(1.0, theme::glass(0.10)));
        ui.painter().text(pos2(left - 6.0, y), Align2::RIGHT_CENTER, label, theme::regular(11.5), theme::text_dim());
    }
    let shown = tuning.bands();
    for (i, hz) in BANDS.into_iter().enumerate() {
        let x = grid.x(i);
        // A band is moved by its thumb. A finger anywhere else on the card scrolls the
        // page, as it meant to, and no band jumps to where it happened to land.
        let thumb = pos2(x, grid.y(shown[i]));
        let resp = ui.interact(Rect::from_center_size(thumb, vec2(grid.slot, THUMB_REACH)), band_id(i), Sense::drag());
        if resp.drag_started() {
            // Where on the thumb the finger took hold: it stays there.
            let hold = resp.interact_pointer_pos().map_or(0.0, |p| p.y - thumb.y);
            ui.data_mut(|d| d.insert_temp(band_id(i), hold));
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let hold = ui.data(|d| d.get_temp::<f32>(band_id(i))).unwrap_or(0.0);
            let db = grid.db(p.y - hold);
            if db != shown[i] {
                tuning.move_band(i, db);
            }
        }
        let db = tuning.bands()[i];
        resp.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), db as f64, format!("{} Hz", hz_label(hz))));
        ui.painter().rect_filled(Rect::from_x_y_ranges(x - 4.0..=x + 4.0, grid.top..=grid.bottom), 4.0, theme::glass(0.10));
        // Filled from the middle to where the band is.
        let (y, zero) = (grid.y(db), grid.y(0.0));
        ui.painter().rect_filled(Rect::from_x_y_ranges(x - 4.0..=x + 4.0, y.min(zero)..=y.max(zero)), 4.0, accent);
        ui.painter().text(pos2(x, grid.bottom + 34.0), Align2::CENTER_CENTER, hz_label(hz), theme::regular(12.0), theme::text_dim());
    }
    // The curve that plays, switch aside: drawn dimmed with the rest while it is off.
    let curve = Curve::new(&crate::eq::Settings { enabled: true, ..tuning.settings() }, DRAWN_RATE);
    let reach = MAX_DB + 1.0;
    // As far to the right as hearing goes.
    let points = (0..=((right - left) / 3.0) as usize).map(|i| left + i as f32 * 3.0).take_while(|x| grid.hz(*x) <= 20_000.0).map(|x| pos2(x, grid.y(curve.db(grid.hz(x)).clamp(-reach, reach))));
    ui.painter().add(egui::Shape::line(points.collect(), egui::Stroke::new(2.0, theme::lerp_color(accent, theme::text(), 0.55))));
    for (i, db) in tuning.bands().into_iter().enumerate() {
        let thumb = Rect::from_center_size(pos2(grid.x(i), grid.y(db)), vec2(30.0, 10.0));
        ui.painter().rect_filled(thumb.translate(vec2(0.0, 1.0)), 5.0, theme::shade(0.25));
        ui.painter().rect_filled(thumb, 5.0, theme::text());
    }
}

/// The bass boost: its name, how far up it is, its slider, and under it what the
/// boosts (the bands' too) cost in loudness.
fn bass(ui: &mut Ui, tuning: &mut Tuning, accent: Color32) {
    let w = ui.available_width();
    let (head, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    let (r, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
    let resp = ui.interact(r, bass_id(), Sense::click_and_drag());
    // The handle stays whole at either end.
    let track = Rect::from_center_size(r.center(), vec2(w - 8.0, 12.0));
    // It follows a tap, and a finger moving along it: one passing over it on its way up
    // or down the page changes nothing.
    let along = ui.input(|i| i.pointer.press_origin()).zip(resp.interact_pointer_pos()).is_some_and(|(from, at)| (at.x - from.x).abs() > (at.y - from.y).abs());
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.clicked() || resp.dragged() && along)
    {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        tuning.set_bass((t * 100.0).round() / 100.0);
    }
    let value = tuning.bass;
    resp.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), (value * 100.0) as f64, "Bass boost"));
    ui.painter().text(pos2(head.left(), head.center().y), Align2::LEFT_CENTER, "Bass boost", theme::regular(17.0), theme::text());
    ui.painter().text(pos2(head.right(), head.center().y), Align2::RIGHT_CENTER, format!("{:.0}%", value * 100.0), theme::semibold(15.0), accent);
    let x = egui::lerp(track.left()..=track.right(), value);
    // A gap either side of the handle, as the design's slider has.
    let filled = Rect::from_min_max(track.min, pos2((x - 6.0).max(track.left()), track.max.y));
    let rest = Rect::from_min_max(pos2((x + 6.0).min(track.right()), track.min.y), track.max);
    ui.painter().rect_filled(filled, 6.0, accent);
    ui.painter().rect_filled(rest, 6.0, theme::glass(0.15));
    ui.painter().rect_filled(Rect::from_center_size(pos2(x, track.center().y), vec2(4.0, 30.0)), 2.0, accent);
    // Its line is kept free while there is nothing to say, so the rows below stay put.
    let (line, _) = ui.allocate_exact_size(vec2(w, 22.0), Sense::hover());
    if let Some(note) = headroom_note(tuning) {
        widgets::text_at(ui, pos2(line.left(), line.top() + 2.0), &note, theme::regular(12.5), theme::text_dim(), w);
    }
}

/// A sound setting with its note and its switch; the whole row switches it.
fn switch_row(ui: &mut Ui, id: egui::Id, title: &str, note: &str, on: &mut bool) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 64.0), Sense::hover());
    if named(ui.interact(r, id, Sense::click()), title).clicked() {
        *on = !*on;
    }
    let text_w = w - 60.0;
    widgets::text_at(ui, pos2(r.left(), r.top() + 10.0), title, theme::regular(17.0), theme::text(), text_w);
    widgets::text_at(ui, pos2(r.left(), r.top() + 34.0), note, theme::regular(13.5), theme::text_dim(), text_w);
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(44.0, 26.0))));
    pages::toggle(&mut sui, on, title);
}

fn contents(ui: &mut Ui, s: &mut Showing, accent: Color32) {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    header(ui, &mut s.tuning.enabled);
    ui.add_space(12.0);
    // Switched off, its controls are dimmed and stay as they are.
    ui.add_enabled_ui(s.tuning.enabled, |ui| {
        presets(ui, &mut s.tuning);
        ui.add_space(12.0);
        bands(ui, &mut s.tuning, accent);
        ui.add_space(20.0);
        bass(ui, &mut s.tuning, accent);
    });
    ui.add_space(2.0);
    // The app's own two, also in Settings: they are not the equalizer's to switch off.
    switch_row(ui, spatial_id(), "Spatial audio", "Stereo songs surround you, for headphones", &mut s.spatial);
    switch_row(ui, normalize_id(), "Volume normalization", "Every song plays about as loud", &mut s.normalize);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{FontFamily, Pos2};

    #[test]
    fn settings_says_whether_it_is_on_and_what_plays() {
        let mut t = Tuning::default();
        assert_eq!(settings_note(&t), "Seven bands, presets and a bass boost. Off.");
        t.enabled = true;
        assert_eq!(settings_note(&t), "Seven bands, presets and a bass boost. On: Flat.");
        t.pick(Preset::Vocal);
        assert_eq!(settings_note(&t), "Seven bands, presets and a bass boost. On: Vocal.");
        t.move_band(0, 3.0);
        assert_eq!(settings_note(&t), "Seven bands, presets and a bass boost. On: Custom.");
    }

    #[test]
    fn the_page_says_how_much_quieter_the_boosts_make_it() {
        assert_eq!(headroom_note(&Tuning::default()), None);
        let mut t = Tuning { enabled: true, ..Default::default() };
        assert_eq!(headroom_note(&t), None, "flat needs no room");
        t.move_band(3, 6.0);
        assert_eq!(headroom_note(&t).as_deref(), Some("Plays 6 dB quieter, to leave the boosts room"));
        // A cut needs none.
        t.move_band(3, -6.0);
        assert_eq!(headroom_note(&t), None);
        // The bass boost counts, and nothing is turned down while it is switched off.
        t.set_bass(1.0);
        assert_eq!(headroom_note(&t).as_deref(), Some("Plays 9 dB quieter, to leave the boosts room"));
        t.enabled = false;
        assert_eq!(headroom_note(&t), None);
    }

    #[test]
    fn frequencies_read_short() {
        assert_eq!(BANDS.map(hz_label), ["60", "150", "400", "1k", "2.4k", "6k", "15k"]);
    }

    #[test]
    fn chips_that_do_not_fit_a_row_go_on_the_next() {
        assert_eq!(wrap(&[60.0, 100.0, 70.0], 300.0, 8.0), [(0, 0.0), (0, 68.0), (0, 176.0)]);
        assert_eq!(wrap(&[60.0, 100.0, 70.0, 90.0], 300.0, 8.0), [(0, 0.0), (0, 68.0), (0, 176.0), (1, 0.0)]);
        // One that ends exactly at the edge still fits.
        assert_eq!(wrap(&[100.0, 92.0, 92.0], 300.0, 8.0), [(0, 0.0), (0, 108.0), (0, 208.0)]);
        assert_eq!(wrap(&[100.0, 92.0, 93.0], 300.0, 8.0), [(0, 0.0), (0, 108.0), (1, 0.0)]);
        // One wider than a whole row takes a row to itself.
        assert_eq!(wrap(&[400.0, 50.0], 300.0, 8.0), [(0, 0.0), (1, 0.0)]);
    }

    fn a_grid() -> Grid {
        Grid { top: 100.0, bottom: 340.0, left: 40.0, slot: 44.0 }
    }

    #[test]
    fn a_slider_runs_from_twelve_up_to_twelve_down() {
        let g = a_grid();
        assert_eq!(g.y(12.0), 100.0);
        assert_eq!(g.y(0.0), 220.0);
        assert_eq!(g.y(-12.0), 340.0);
        assert_eq!(g.db(100.0), 12.0);
        assert_eq!(g.db(220.0), 0.0);
        assert_eq!(g.db(340.0), -12.0);
    }

    #[test]
    fn a_finger_sets_the_nearest_half_decibel_and_stays_in_range() {
        let g = a_grid();
        // Ten points a decibel here.
        assert_eq!(g.db(220.0 - 34.0), 3.5);
        assert_eq!(g.db(220.0 - 36.0), 3.5);
        assert_eq!(g.db(220.0 + 21.0), -2.0);
        assert_eq!(g.db(20.0), 12.0);
        assert_eq!(g.db(900.0), -12.0);
    }

    #[test]
    fn the_curve_is_drawn_at_each_bands_own_frequency_under_its_slider() {
        let g = a_grid();
        for (i, hz) in BANDS.iter().enumerate() {
            assert!((g.hz(g.x(i)) / hz - 1.0).abs() < 1e-3, "band {i}");
        }
        // Half way between two sliders is half way in pitch.
        let mid = g.hz((g.x(3) + g.x(4)) / 2.0);
        assert!((mid - (1000.0f32 * 2400.0).sqrt()).abs() < 1.0, "{mid}");
        // Past the outer sliders it goes on at the same pace.
        assert!((g.hz(g.x(0) - 22.0) / (60.0 / 2.5f32.sqrt()) - 1.0).abs() < 1e-3);
        assert!((g.hz(g.x(6) + 22.0) / (15_000.0 * 2.5f32.sqrt()) - 1.0).abs() < 1e-3);
    }

    /// The page's contents in a headless egui, 380 points wide, with stand-ins for the
    /// app's fonts.
    struct Page {
        ctx: egui::Context,
        s: Showing,
        time: f64,
        /// How tall the screen is: the page scrolls on one too short for it.
        height: f32,
    }

    impl Page {
        fn new(tuning: Tuning) -> Page {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let plain = fonts.families[&FontFamily::Proportional].clone();
            for family in ["semibold", "bold", "icons"] {
                fonts.families.insert(FontFamily::Name(family.into()), plain.clone());
            }
            ctx.set_fonts(fonts);
            let mut p = Page { ctx, s: Showing { tuning, spatial: false, normalize: true }, time: 0.0, height: 900.0 };
            p.frame(vec![]);
            p.frame(vec![]);
            p
        }

        fn on() -> Page {
            Page::new(Tuning { enabled: true, ..Default::default() })
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(380.0, self.height))), time: Some(self.time), events, ..Default::default() };
            // In a page that scrolls, as the app's pages are.
            let out = self.ctx.run_ui(input, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| contents(ui, &mut self.s, theme::RED));
            });
            out.drop_without_applying_deltas();
        }

        /// A finger's touch at `at`, as a phone reports it beside the pointer's.
        fn touch(phase: egui::TouchPhase, at: Pos2) -> egui::Event {
            egui::Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(0), phase, pos: at, force: None }
        }

        fn rect(&self, id: egui::Id) -> Rect {
            self.ctx.read_response(id).map(|r| r.rect).expect("it is on the page")
        }

        fn press(&mut self, at: Pos2) {
            self.frame(vec![
                Self::touch(egui::TouchPhase::Start, at),
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            ]);
        }

        fn release(&mut self, at: Pos2) {
            self.frame(vec![
                Self::touch(egui::TouchPhase::End, at),
                egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
                egui::Event::PointerGone,
            ]);
            self.frame(vec![]);
        }

        fn tap_at(&mut self, at: Pos2) {
            self.press(at);
            self.release(at);
        }

        fn tap(&mut self, id: egui::Id) {
            self.tap_at(self.rect(id).center());
        }

        /// A finger put down at `from` and moved to `to` in a few steps.
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.press(from);
            for step in 1..=4 {
                let at = from + (to - from) * (step as f32 / 4.0);
                self.frame(vec![Self::touch(egui::TouchPhase::Move, at), egui::Event::PointerMoved(at)]);
            }
            self.release(to);
        }

        /// The middle of a band's thumb.
        fn thumb(&self, band: usize) -> Pos2 {
            self.rect(band_id(band)).center()
        }
    }

    /// How far a finger travels on a slider to change a band by `db`: up for more.
    fn travel(db: f32) -> egui::Vec2 {
        let grid = Grid::of(Rect::from_min_size(Pos2::ZERO, vec2(380.0, CARD_H)));
        vec2(0.0, grid.y(db) - grid.y(0.0))
    }

    #[test]
    fn it_starts_switched_off_and_flat_and_the_switch_turns_it_on() {
        let mut p = Page::new(Tuning::default());
        assert!(!p.s.tuning.enabled);
        p.tap(switch_id());
        assert!(p.s.tuning.enabled);
        assert_eq!(p.s.tuning.preset, Preset::Flat);
        p.tap(switch_id());
        assert!(!p.s.tuning.enabled);
    }

    #[test]
    fn every_preset_has_a_chip_that_picks_it() {
        for preset in Preset::ALL {
            let mut p = Page::on();
            p.s.tuning.pick(Preset::Vocal);
            p.frame(vec![]);
            p.tap(preset_id(preset));
            assert_eq!(p.s.tuning.preset, preset);
        }
    }

    #[test]
    fn a_band_follows_the_finger_and_the_curve_becomes_custom() {
        let mut p = Page::on();
        p.s.tuning.pick(Preset::BassBoost);
        p.frame(vec![]);
        let was = p.s.tuning.bands();
        let at = p.thumb(4);
        p.drag(at, at + travel(6.0));
        assert_eq!(p.s.tuning.preset, Preset::Custom);
        let mut want = was;
        want[4] = 6.0;
        assert_eq!(p.s.tuning.bands(), want);
    }

    #[test]
    fn a_thumb_stays_under_the_finger_where_it_was_taken_hold_of() {
        let mut p = Page::on();
        // Not by its middle.
        let at = p.thumb(2) + vec2(6.0, 12.0);
        p.drag(at, at + travel(-3.0));
        assert_eq!(p.s.tuning.bands()[2], -3.0);
    }

    #[test]
    fn a_touch_that_does_not_drag_never_moves_a_band() {
        let mut p = Page::on();
        // On a thumb, and further down its slider.
        p.tap_at(p.thumb(6) + vec2(0.0, 12.0));
        p.tap_at(p.thumb(6) + travel(-8.0));
        assert_eq!(p.s.tuning, Tuning { enabled: true, ..Default::default() });
    }

    #[test]
    fn a_finger_on_a_thumb_moves_the_band_and_one_beside_it_scrolls_the_page() {
        // A screen too short for the page: it scrolls.
        let mut p = Page::on();
        p.height = 420.0;
        p.frame(vec![]);
        p.frame(vec![]);
        let page = p.rect(switch_id());
        let at = p.thumb(3);
        p.drag(at, at + travel(-9.0));
        assert_eq!(p.s.tuning.bands()[3], -9.0);
        assert_eq!(p.rect(switch_id()), page, "the page stayed where it was");
        // The same slider, where its thumb no longer is: the finger scrolls the page.
        p.drag(at + vec2(0.0, 20.0), at - vec2(0.0, 100.0));
        assert!(p.rect(switch_id()).top() < page.top() - 100.0, "the page scrolled");
        assert_eq!(p.s.tuning.bands(), [0.0, 0.0, 0.0, -9.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_band_dragged_past_its_end_stops_at_twelve() {
        let mut p = Page::on();
        let at = p.thumb(0);
        p.drag(at, at + travel(12.0) - vec2(0.0, 80.0));
        assert_eq!(p.s.tuning.bands()[0], 12.0);
        let at = p.thumb(0);
        p.drag(at, at + travel(-24.0) + vec2(0.0, 80.0));
        assert_eq!(p.s.tuning.bands()[0], -12.0);
    }

    #[test]
    fn the_bass_boost_follows_the_finger_and_leaves_the_preset_alone() {
        let mut p = Page::on();
        p.s.tuning.pick(Preset::Acoustic);
        p.frame(vec![]);
        let r = p.rect(bass_id());
        p.drag(pos2(r.left() + 20.0, r.center().y), pos2(r.center().x, r.center().y));
        assert!((p.s.tuning.bass - 0.5).abs() < 0.011, "{}", p.s.tuning.bass);
        p.drag(pos2(r.center().x, r.center().y), pos2(r.right() + 60.0, r.center().y));
        assert_eq!(p.s.tuning.bass, 1.0);
        p.drag(pos2(r.center().x, r.center().y), pos2(r.left() - 60.0, r.center().y));
        assert_eq!(p.s.tuning.bass, 0.0);
        assert_eq!(p.s.tuning.preset, Preset::Acoustic);
    }

    #[test]
    fn a_tap_on_the_bass_boost_sets_it_there_and_a_finger_passing_over_it_does_not() {
        let mut p = Page::on();
        let r = p.rect(bass_id());
        p.tap_at(pos2(r.left() + r.width() * 0.25, r.center().y));
        assert!((p.s.tuning.bass - 0.25).abs() < 0.02, "{}", p.s.tuning.bass);
        // On its way up the page, a finger lands on the slider further right.
        let at = pos2(r.left() + r.width() * 0.8, r.center().y);
        p.drag(at, at - vec2(4.0, 120.0));
        assert!((p.s.tuning.bass - 0.25).abs() < 0.02, "{}", p.s.tuning.bass);
    }

    #[test]
    fn switched_off_the_presets_the_bands_and_the_bass_boost_stay_as_they_are() {
        let mut p = Page::new(Tuning::default());
        let was = p.s.clone();
        p.tap(preset_id(Preset::Vocal));
        let at = p.thumb(2);
        p.drag(at, at + travel(6.0));
        p.tap(bass_id());
        assert_eq!(p.s, was);
    }

    #[test]
    fn spatial_audio_and_volume_normalization_switch_whether_the_equalizer_is_on_or_off() {
        for tuning in [Tuning::default(), Tuning { enabled: true, ..Default::default() }] {
            let mut p = Page::new(tuning.clone());
            p.tap(spatial_id());
            assert!(p.s.spatial);
            p.tap(normalize_id());
            assert!(!p.s.normalize);
            p.tap(spatial_id());
            assert!(!p.s.spatial);
            assert_eq!(p.s.tuning, tuning, "they are not the equalizer's");
        }
    }

    #[test]
    fn nothing_overlaps_and_everything_is_big_enough_to_touch() {
        let p = Page::on();
        let ids = [switch_id(), bass_id(), spatial_id(), normalize_id()].into_iter().chain(Preset::ALL.map(preset_id)).chain((0..BANDS.len()).map(band_id));
        let rects: Vec<Rect> = ids.map(|id| p.rect(id)).collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.height() >= 44.0 && a.width() >= 44.0, "{a:?}");
            assert!(a.left() >= 0.0 && a.right() <= 380.0, "{a:?} is on the page");
            for b in &rects[i + 1..] {
                // Neighbouring sliders share an edge; none shares any room.
                let shared = a.intersect(*b);
                assert!(shared.width() < 0.5 || shared.height() < 0.5, "{a:?} and {b:?}");
            }
        }
    }
}
