//! "Play on" on the screen: the chip on Now Playing that names where the sound goes,
//! and the sheet it opens, with the output Android plays through, the way to Android's
//! output switcher and the phone's media volume. What it shows is `crate::playon`'s; the
//! switching itself is Android's.

use egui::{Color32, Pos2, Rect, Sense, Ui, pos2, vec2};

use super::widgets::{self, named};
use super::{App, glyphs, m3, sheet};
use crate::material::{self, Type, shape};
use crate::playon::{Kind, Output, Slider, Switcher};
use crate::theme::{self, icon};

/// The sheet: whether it is up, and its slider's memory.
#[derive(Default)]
pub(super) struct View {
    pub open: bool,
    slider: Slider,
}

/// What the sheet shows of the phone and the player.
struct Showing {
    output: Output,
    playing: bool,
}

/// What the listener asked the sheet for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Android's output switcher (or the Bluetooth settings).
    Switch,
    /// The media volume on this step.
    Volume(u32),
}

fn switch_id() -> egui::Id {
    egui::Id::unique("playon-switch")
}

/// The line under the output's name.
fn status(output: &Output, playing: bool) -> String {
    match (output.kind, playing) {
        (Kind::Phone, true) => "Playing now".to_owned(),
        (_, true) => format!("{} \u{b7} Playing now", output.detail()),
        (_, false) => output.detail().to_owned(),
    }
}

/// What the row that opens the switcher says: its title and the line under it.
fn switch_words(switcher: Switcher) -> Option<(&'static str, &'static str)> {
    match switcher {
        Switcher::None => None,
        Switcher::System => Some(("Switch device", "Android's output switcher")),
        Switcher::BluetoothSettings => Some(("Bluetooth settings", "Connect another device")),
    }
}

/// A phone, in the icon font's line (the font has none): Lucide's, on its 24-unit grid.
fn phone(ui: &Ui, center: Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let at = |x: f32, y: f32| center + vec2(x - 12.0, y - 12.0) * k;
    let body = Rect::from_min_max(at(5.0, 2.0), at(19.0, 22.0));
    ui.painter().rect_stroke(body, 2.5 * k, egui::Stroke::new(2.0 * k, color), egui::StrokeKind::Middle);
    ui.painter().circle_filled(at(12.0, 18.0), 1.2 * k, color);
}

/// The picture of a kind of output: the phone, headphones, or a speaker.
pub(super) fn mark(kind: Kind) -> m3::Icon {
    match kind {
        Kind::Phone => m3::Icon::Drawn(phone),
        Kind::Bluetooth | Kind::Wired | Kind::HearingAid => m3::Icon::Font(icon::HEADPHONES),
        Kind::Usb | Kind::External | Kind::Other => m3::Icon::Glyph(glyphs::VOLUME[3]),
    }
}

fn kind_icon(ui: &Ui, center: Pos2, size: f32, kind: Kind, color: Color32) {
    m3::paint_icon(ui, center, size, mark(kind), color);
}

/// Sound playing: five bars, tallest in the middle.
fn bars(ui: &Ui, center: Pos2, color: Color32) {
    for (i, h) in [6.0, 14.0, 22.0, 14.0, 6.0].into_iter().enumerate() {
        let x = center.x + (i as f32 - 2.0) * 5.0;
        ui.painter().rect_filled(Rect::from_center_size(pos2(x, center.y), vec2(2.5, h)), 1.25, color);
    }
}

/// The room a chip's icon and padding take beside its words.
const CHIP_CHROME: f32 = 12.0 + 18.0 + 7.0 + 14.0;
/// The chip is 36 points tall, as the sleep timer's beside it.
const CHIP_H: f32 = 36.0;

/// What the chip's words may take of the room from `left` to `right`, or `None` when
/// that is too little for words: then it is its icon alone.
fn chip_words(left: f32, right: f32) -> Option<f32> {
    let room = right - left - CHIP_CHROME;
    (room >= 64.0).then(|| room.min(170.0))
}

/// The chip on Now Playing that names where the sound goes and opens the sheet: it
/// starts at `left`, ends before `right` and is centred on `cy`.
pub(super) fn chip(app: &mut App, ui: &mut Ui, left: f32, right: f32, cy: f32) {
    if right - left < CHIP_H {
        return;
    }
    let output = crate::playon::output();
    let ink = theme::text();
    let words = chip_words(left, right).map(|room| m3::galley(ui, output.title(), Type::LabelLarge, ink, room));
    let width = words.as_ref().map_or(CHIP_H, |g| CHIP_CHROME + g.size().x);
    let rect = Rect::from_min_size(pos2(left, cy - CHIP_H / 2.0), vec2(width, CHIP_H));
    // A finger's height to tap, a little more than what is drawn.
    let resp = named(ui.interact(rect.expand2(vec2(0.0, 6.0)), ui.scope_id().with("np-playon"), Sense::click()), "Play on");
    let press = if resp.is_pointer_button_down_on() { 0.08 } else { 0.0 };
    ui.painter().rect_filled(rect, CHIP_H / 2.0, theme::glass(0.09 + press));
    match words {
        Some(g) => {
            kind_icon(ui, pos2(rect.left() + 12.0 + 9.0, cy), 17.0, output.kind, ink);
            ui.painter().galley(pos2(rect.left() + 12.0 + 18.0 + 7.0, cy - g.size().y / 2.0), g, ink);
        }
        None => kind_icon(ui, rect.center(), 17.0, output.kind, ink),
    }
    if resp.clicked() {
        app.playon.open = true;
    }
}

/// The volume row's height, and the room left of its slider for the speaker.
const VOLUME_H: f32 = 56.0;
const SPEAKER_W: f32 = 48.0;

/// The gap between the sheet's rows.
const GAP: f32 = 8.0;

/// The slider in the volume `row`: right of the speaker, a finger tall.
fn volume_slider(row: Rect) -> Rect {
    Rect::from_min_max(pos2(row.left() + SPEAKER_W, row.center().y - m3::TOUCH / 2.0), pos2(row.right() - 8.0, row.center().y + m3::TOUCH / 2.0))
}

/// The media volume: a speaker and a slider on the phone's own steps. The step the
/// listener put it on, when that is to be asked of the phone.
fn volume(ui: &mut Ui, output: &Output, slider: &mut Slider, now: f64) -> Option<u32> {
    let m = material::scheme();
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), VOLUME_H), Sense::hover());
    let before = slider.shown(output.volume, now);
    let mut step = before;
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(volume_slider(row)));
    m3::slider(&mut bar, &mut step, 0..=output.max_volume, "Media volume");
    let ask = (step != before && slider.set(step, now)).then_some(step);
    let fraction = step as f32 / output.max_volume as f32;
    glyphs::draw(ui, pos2(row.left() + 8.0 + 12.0, row.center().y), 24.0, glyphs::VOLUME[glyphs::volume_step(fraction)], m.on_surface_variant);
    if slider.waiting() {
        // The phone hasn't followed yet: look again shortly.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    ask
}

/// What the sheet holds: its title, the output the music plays through, the row that
/// opens Android's output switcher, and the media volume.
fn contents(ui: &mut Ui, s: &Showing, slider: &mut Slider, now: f64) -> Option<Ask> {
    let m = material::scheme();
    let w = ui.available_width();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let mut ask = None;

    m3::sheet_header(ui, "Play on", "Where the music plays, and how loud");
    ui.add_space(12.0);

    // Where the music plays now.
    let (card, _) = ui.allocate_exact_size(vec2(w, 80.0), Sense::hover());
    super::m3::plate(ui, card, super::m3::round(shape::CARD), m.primary_container, super::m3::Stack::Card);
    let tile = Rect::from_center_size(pos2(card.left() + 16.0 + 24.0, card.center().y), vec2(48.0, 48.0));
    ui.painter().rect_filled(tile, shape::TILE, m.primary);
    kind_icon(ui, tile.center(), 24.0, s.output.kind, m.on_primary);
    let text_left = tile.right() + 16.0;
    let text_right = card.right() - if s.playing { 16.0 + 24.0 + 12.0 } else { 20.0 };
    m3::two_lines(ui, text_left, card.center().y, text_right - text_left, s.output.title(), Type::TitleMedium, m.on_primary_container, &status(&s.output, s.playing), Type::BodyMedium, m.on_primary_container);
    if s.playing {
        bars(ui, pos2(card.right() - 16.0 - 12.0, card.center().y), m.primary);
    }

    if let Some((title, note)) = switch_words(s.output.switcher) {
        ui.add_space(GAP);
        let (row, _) = ui.allocate_exact_size(vec2(w, 72.0), Sense::hover());
        let resp = named(ui.interact(row, switch_id(), Sense::click()), title);
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(row, shape::CARD_SMALL, m.on_surface.gamma_multiply(0.1));
        }
        let lead = pos2(row.left() + 8.0 + 22.0, row.center().y);
        ui.painter().circle_filled(lead, 22.0, m.surface_container_highest);
        widgets::icon_at(ui, lead, icon::HEADPHONES, 20.0, m.on_surface_variant);
        let text_left = lead.x + 22.0 + 16.0;
        m3::two_lines(ui, text_left, row.center().y, row.right() - 40.0 - text_left, title, Type::BodyLarge, m.on_surface, note, Type::BodyMedium, m.on_surface_variant);
        widgets::icon_at(ui, pos2(row.right() - 20.0, row.center().y), icon::CHEVRON_RIGHT, 18.0, m.on_surface_variant);
        if resp.clicked() {
            ask = Some(Ask::Switch);
        }
    }

    if s.output.max_volume > 0 {
        ui.add_space(GAP);
        if let Some(step) = volume(ui, &s.output, slider, now) {
            ask = Some(Ask::Volume(step));
        }
    }
    ask
}

/// Dev hooks: YTM_PLAYON=<kind>,<name>,<volume>,<most>,<switcher> stands in for the
/// activity's report (the numbers as in `Output::from_line`); YTM_PLAYON_SHEET opens the sheet.
pub(super) fn dev_hooks(app: &mut App) {
    if let Some(report) = super::dev_var("YTM_PLAYON") {
        crate::playon::set_output_line(&report.replace(',', "\u{1f}"));
    }
    if super::dev_var("YTM_PLAYON_SHEET").is_some() {
        app.playon.open = true;
    }
}

/// The sheet, over Now Playing.
pub(super) fn sheet(app: &mut App, ctx: &egui::Context) {
    let showing = Showing { output: crate::playon::output(), playing: app.playing() || app.buffering };
    let now = ctx.input(|i| i.time);
    let (mut open, mut ask) = (app.playon.open, None);
    let mut slider = app.playon.slider;
    sheet::show(ctx, "play-on", &mut open, |ui| ask = contents(ui, &showing, &mut slider, now));
    app.playon.open = open;
    app.playon.slider = slider;
    match ask {
        Some(Ask::Switch) => crate::playon::open_switcher(),
        Some(Ask::Volume(step)) => crate::playon::set_volume(step),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontFamily;

    fn output(kind: Kind, name: &str, volume: u32, max_volume: u32, switcher: Switcher) -> Output {
        Output { kind, name: name.into(), volume, max_volume, switcher }
    }

    #[test]
    fn the_card_says_what_plays_through_the_output() {
        let phone = Output::default();
        assert_eq!(status(&phone, true), "Playing now");
        assert_eq!(status(&phone, false), "Phone speaker");
        let buds = output(Kind::Bluetooth, "Pixel Buds Pro", 7, 15, Switcher::System);
        assert_eq!(status(&buds, true), "Bluetooth \u{b7} Playing now");
        assert_eq!(status(&buds, false), "Bluetooth");
    }

    #[test]
    fn the_row_says_what_it_opens() {
        assert_eq!(switch_words(Switcher::System).map(|w| w.0), Some("Switch device"));
        assert_eq!(switch_words(Switcher::BluetoothSettings).map(|w| w.0), Some("Bluetooth settings"));
        assert_eq!(switch_words(Switcher::None), None, "nothing to open, no row");
    }

    #[test]
    fn the_chip_has_words_when_there_is_room_for_them() {
        // A 412-point phone: the sleep chip ends near 134, the lyrics button starts at 299.
        let room = chip_words(142.0, 291.0).expect("room for words");
        assert!((90.0..=149.0 - 51.0).contains(&room), "{room}");
        // A 360-point phone with the timer running: the icon alone.
        assert_eq!(chip_words(160.0, 239.0), None);
        // However wide the screen, a long name doesn't run on for ever.
        assert!(chip_words(142.0, 700.0).unwrap() <= 170.0);
    }

    #[test]
    fn a_finger_on_the_handle_of_a_step_is_on_that_step() {
        // The phone's own numbers of steps: 15 for most streams, 25 for some.
        let rect = volume_slider(Rect::from_min_size(pos2(16.0, 100.0), vec2(348.0, VOLUME_H)));
        assert!(rect.height() >= 44.0, "a finger's height");
        for most in [7, 15, 25] {
            for step in 0..=most {
                let handle = m3::slider_geom(rect, step as f32 / most as f32).handle.center().x;
                assert_eq!(m3::slider_value(rect, handle, 0, most), step, "of {most}");
            }
        }
    }

    /// The sheet's contents in a headless egui, 380 points wide, with stand-ins for the
    /// app's fonts.
    struct Sheet {
        ctx: egui::Context,
        s: Showing,
        slider: Slider,
        time: f64,
        asks: Vec<Ask>,
        /// What the contents took up in the last frame.
        content: Rect,
    }

    impl Sheet {
        fn new(output: Output) -> Self {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let plain = fonts.families[&FontFamily::Proportional].clone();
            for family in ["semibold", "bold", "icons"] {
                fonts.families.insert(FontFamily::Name(family.into()), plain.clone());
            }
            ctx.set_fonts(fonts);
            let mut sheet = Self { ctx, s: Showing { output, playing: true }, slider: Slider::default(), time: 0.0, asks: Vec::new(), content: Rect::NOTHING };
            sheet.frame(vec![]);
            sheet.frame(vec![]);
            sheet
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(380.0, 500.0))), time: Some(self.time), events, ..Default::default() };
            let (mut ask, mut content) = (None, Rect::NOTHING);
            let out = self.ctx.run_ui(input, |ui| {
                ask = contents(ui, &self.s, &mut self.slider, self.time);
                content = ui.min_rect();
            });
            out.drop_without_applying_deltas();
            self.asks.extend(ask);
            self.content = content;
        }

        fn rect(&self, id: egui::Id) -> Option<Rect> {
            self.ctx.read_response(id).map(|r| r.rect)
        }

        fn button(at: Pos2, pressed: bool) -> egui::Event {
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
        }

        fn tap(&mut self, at: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at), Self::button(at, true)]);
            self.frame(vec![Self::button(at, false)]);
            self.frame(vec![]);
        }

        /// The volume slider: the sheet's last row.
        fn slider(&self) -> Rect {
            volume_slider(Rect::from_min_max(pos2(self.content.left(), self.content.bottom() - VOLUME_H), self.content.right_bottom()))
        }

        /// Where on the slider a finger is on `step` of the phone's.
        fn at_step(&self, step: u32) -> Pos2 {
            m3::slider_geom(self.slider(), step as f32 / self.s.output.max_volume as f32).handle.center()
        }
    }

    fn buds() -> Output {
        output(Kind::Bluetooth, "Pixel Buds Pro", 7, 15, Switcher::System)
    }

    #[test]
    fn a_tap_on_the_row_asks_for_androids_switcher() {
        let mut sheet = Sheet::new(buds());
        let row = sheet.rect(switch_id()).expect("the row is there");
        assert!(row.height() >= 48.0 && row.width() >= 300.0, "{row:?}");
        sheet.tap(row.center());
        assert_eq!(sheet.asks, [Ask::Switch]);
    }

    #[test]
    fn with_nothing_to_open_there_is_no_row() {
        let (without, with) = (Sheet::new(output(Kind::Phone, "", 7, 15, Switcher::None)), Sheet::new(buds()));
        assert_eq!(without.rect(switch_id()), None);
        assert_eq!(without.content.height() + 72.0 + GAP, with.content.height(), "only the row is missing");
        let mut without = without;
        let at = without.at_step(3);
        without.tap(at);
        assert_eq!(without.asks, [Ask::Volume(3)], "the slider is there");
    }

    #[test]
    fn a_volume_that_cannot_be_set_has_no_slider() {
        let (fixed, settable) = (Sheet::new(output(Kind::External, "", 0, 0, Switcher::System)), Sheet::new(buds()));
        assert!(fixed.rect(switch_id()).is_some());
        assert_eq!(fixed.content.height() + VOLUME_H + GAP, settable.content.height(), "only the slider is missing");
    }

    #[test]
    fn a_tap_on_the_slider_asks_for_the_step_under_the_finger() {
        let mut sheet = Sheet::new(buds());
        let at = sheet.at_step(11);
        sheet.tap(at);
        assert_eq!(sheet.asks, [Ask::Volume(11)]);
    }

    #[test]
    fn a_drag_along_the_slider_asks_for_each_step_once() {
        let mut sheet = Sheet::new(buds());
        let (from, to) = (sheet.at_step(7), sheet.at_step(10));
        sheet.frame(vec![egui::Event::PointerMoved(from), Sheet::button(from, true)]);
        for i in 1..=30 {
            sheet.frame(vec![egui::Event::PointerMoved(from + (to - from) * (i as f32 / 30.0))]);
        }
        sheet.frame(vec![Sheet::button(to, false)]);
        sheet.frame(vec![]);
        assert_eq!(sheet.asks, [Ask::Volume(8), Ask::Volume(9), Ask::Volume(10)], "not the step it was on, and none twice");
    }

    #[test]
    fn a_finger_resting_on_the_step_the_phone_is_on_asks_for_nothing() {
        let mut sheet = Sheet::new(buds());
        let at = sheet.at_step(7);
        sheet.tap(at);
        assert_eq!(sheet.asks, []);
    }

    #[test]
    fn the_row_and_the_slider_do_not_overlap_and_stay_on_the_sheet() {
        let sheet = Sheet::new(buds());
        let (row, slider) = (sheet.rect(switch_id()).unwrap(), sheet.slider());
        assert!(row.bottom() <= slider.top(), "{row:?} {slider:?}");
        assert!(slider.height() >= 44.0, "a finger's height");
        for r in [row, slider] {
            assert!(r.left() >= 0.0 && r.right() <= 380.0, "{r:?}");
        }
    }

    /// The sheet as the phone shows it: up over a 380 x 800 screen, with what it holds.
    struct Up {
        ctx: egui::Context,
        open: bool,
        output: Output,
        slider: Slider,
        time: f64,
        asks: Vec<Ask>,
        /// Where the sheet is, and what it holds.
        sheet: Option<Rect>,
        content: Rect,
    }

    impl Up {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let plain = fonts.families[&FontFamily::Proportional].clone();
            for family in ["semibold", "bold", "icons"] {
                fonts.families.insert(FontFamily::Name(family.into()), plain.clone());
            }
            ctx.set_fonts(fonts);
            let mut up = Self { ctx, open: true, output: buds(), slider: Slider::default(), time: 0.0, asks: Vec::new(), sheet: None, content: Rect::NOTHING };
            // Slid all the way up before a finger comes down.
            for _ in 0..60 {
                up.frame(vec![]);
            }
            up
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(380.0, 800.0))), time: Some(self.time), events, ..Default::default() };
            let showing = Showing { output: self.output.clone(), playing: true };
            let (mut open, mut ask, mut content, mut sheet) = (self.open, None, self.content, None);
            let out = self.ctx.run_ui(input, |ui| {
                sheet = sheet::show(ui.ctx(), "play-on", &mut open, |ui| {
                    ask = contents(ui, &showing, &mut self.slider, self.time);
                    content = ui.min_rect();
                });
            });
            out.drop_without_applying_deltas();
            self.open = open;
            self.sheet = sheet;
            self.content = content;
            self.asks.extend(ask);
        }

        fn down(&mut self, at: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at), Sheet::button(at, true)]);
        }

        fn up(&mut self, at: Pos2) {
            self.frame(vec![Sheet::button(at, false)]);
            self.frame(vec![]);
        }

        fn tap(&mut self, at: Pos2) {
            self.down(at);
            self.up(at);
        }

        /// A finger slides from `from` to `to` over `frames` frames, then lifts.
        fn swipe(&mut self, from: Pos2, to: Pos2, frames: usize) {
            self.down(from);
            for i in 1..=frames {
                self.frame(vec![egui::Event::PointerMoved(from + (to - from) * (i as f32 / frames as f32))]);
            }
            self.up(to);
        }

        /// A point on the output's card.
        fn card(&self) -> Pos2 {
            pos2(self.content.center().x, self.content.top() + 60.0 + 12.0 + 40.0)
        }

        /// Where on the volume slider a finger is on `step`.
        fn at_step(&self, step: u32) -> Pos2 {
            let row = Rect::from_min_max(pos2(self.content.left(), self.content.bottom() - VOLUME_H), self.content.right_bottom());
            m3::slider_geom(volume_slider(row), step as f32 / self.output.max_volume as f32).handle.center()
        }
    }

    #[test]
    fn a_tap_outside_the_sheet_closes_it_and_a_tap_on_it_does_not() {
        let mut up = Up::new();
        assert!(up.sheet.is_some());
        let card = up.card();
        up.tap(card);
        assert!(up.open, "a tap on the card");
        up.tap(pos2(190.0, 100.0));
        assert!(!up.open, "a tap on the dimmed screen");
        for _ in 0..60 {
            up.frame(vec![]);
        }
        assert!(up.sheet.is_none(), "it slid away");
    }

    #[test]
    fn a_swipe_down_and_androids_back_close_the_sheet() {
        let mut up = Up::new();
        let from = up.card();
        up.swipe(from, from + vec2(0.0, 220.0), 12);
        assert!(!up.open, "a swipe down from the card");
        assert!(up.asks.is_empty(), "and it pressed nothing");

        let mut up = Up::new();
        assert!(sheet::is_open(&up.ctx));
        sheet::dismiss(&up.ctx);
        up.frame(vec![]);
        assert!(!up.open, "Back");
    }

    #[test]
    fn a_drag_along_the_volume_slider_sets_the_volume_and_leaves_the_sheet_up() {
        let mut up = Up::new();
        let (from, to) = (up.at_step(7), up.at_step(12));
        up.swipe(from, to, 20);
        assert!(up.open);
        assert_eq!(up.asks.last(), Some(&Ask::Volume(12)));
        // A finger that slides down off the slider still only sets the volume.
        let mut up = Up::new();
        let from = up.at_step(7);
        up.swipe(from, from + vec2(40.0, 200.0), 12);
        assert!(up.open, "the slider keeps its finger");
    }
}
