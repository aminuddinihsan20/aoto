//! The playback speed on the screen: the row of speeds (in Settings › Playback, and in
//! a sheet over Now Playing opened from its ⋯ menu) and the "Keep the pitch" switch.
//! The speed itself is `crate::speed`; `App::speed_now` is what plays.

use egui::{Color32, Pos2, Rect, Sense, Ui, pos2, vec2};

use super::widgets::{self, named};
use super::{App, m3, pages, sheet};
use crate::material::Type;
use crate::speed::{SPEEDS, label};
use crate::theme;

/// The group of Settings › Playback the speed and its pitch switch are in: its label
/// names the buttons under it.
pub(super) const SETTINGS_GROUP: &str = "Playback speed";

/// Why the speeds can't be picked in a party.
const PARTY_NOTE: &str = "In a listening party everyone plays at 1×, to hear the same moment.";

/// What the controls show of the player.
struct Showing {
    /// The speed set, and the one that plays (1× in a party, whatever is set).
    speed: f32,
    playing: f32,
    keep_pitch: bool,
    party: bool,
    accent: Color32,
}

impl Showing {
    fn of(app: &App) -> Self {
        Self { speed: app.speed, playing: app.speed_now(), keep_pitch: app.keep_pitch, party: app.jam.active(), accent: app.accent_color() }
    }

    /// Whatever was changed in the controls goes to the player.
    fn apply(self, app: &mut App) {
        if self.speed != app.speed || self.keep_pitch != app.keep_pitch {
            app.speed = self.speed;
            app.keep_pitch = self.keep_pitch;
            app.set_speed();
        }
    }
}

fn speed_id(speed: f32) -> egui::Id {
    egui::Id::unique(("speed", speed.to_bits()))
}

fn pitch_id() -> egui::Id {
    egui::Id::unique("speed-keep-pitch")
}

/// The speeds as a row of pills across `rect`, the one that plays filled in the accent.
/// In a party they show 1× and can't be picked.
fn pills(ui: &mut Ui, rect: Rect, s: &mut Showing) {
    let gap = if rect.width() < 300.0 || m3::on() { 4.0 } else { 6.0 };
    let w = (rect.width() - gap * (SPEEDS.len() - 1) as f32) / SPEEDS.len() as f32;
    let font = theme::semibold(if w < 44.0 { 11.5 } else { 13.5 });
    let fade = if s.party { 0.45 } else { 1.0 };
    for (i, speed) in SPEEDS.into_iter().enumerate() {
        let cell = Rect::from_min_size(pos2(rect.left() + (w + gap) * i as f32, rect.top()), vec2(w, rect.height()));
        let text = label(speed);
        let resp = named(ui.interact(cell, speed_id(speed), if s.party { Sense::hover() } else { Sense::click() }), &text);
        let on = speed == s.playing;
        if m3::on() {
            // Material: connected buttons, the speed that plays a filled pill.
            let rest = crate::material::scheme().surface_container_highest;
            m3::segment(ui, cell, &text, &m3::Segment { on, first: i == 0, last: i + 1 == SPEEDS.len(), pressed: resp.is_pointer_button_down_on(), rest, fade });
            if resp.clicked() {
                s.speed = speed;
                s.playing = speed;
            }
            continue;
        }
        let t = theme::anim_bool(ui.ctx(), resp.id.with("on"), on, 0.15);
        let press = if resp.is_pointer_button_down_on() { 0.08 } else { 0.0 };
        ui.painter().rect_filled(cell, cell.height() / 2.0, theme::with_alpha(theme::lerp_color(theme::glass(0.09 + press), s.accent, t), fade));
        let ink = theme::with_alpha(theme::lerp_color(theme::text(), super::sleep::on_accent(s.accent), t), fade);
        let g = widgets::galley(ui, &text, font.clone(), ink, cell.width());
        ui.painter().galley(cell.center() - g.size() / 2.0, g, ink);
        if resp.clicked() {
            s.speed = speed;
            s.playing = speed;
        }
    }
}

/// The speed's group in the phone's Settings: the connected buttons (the group's label
/// names them and says the speed that plays), and in a party why they are at 1×.
pub(super) fn settings_buttons(app: &mut App, ui: &mut Ui) {
    if !pages::settings_row_shown(ui, "Playback speed", settings_note(app)) {
        return;
    }
    let mut s = Showing::of(app);
    if pages::settings_section(ui) != SETTINGS_GROUP {
        // Among other rows (what a search found): it names itself.
        let m = crate::material::scheme();
        let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let value = m3::galley(ui, &label(s.playing), Type::TitleMedium, m.primary, 80.0);
        m3::text(ui, pos2(head.left(), head.center().y - 12.0), "Playback speed", Type::BodyLarge, m.on_surface, head.width() - value.size().x - 12.0);
        ui.painter().galley(pos2(head.right() - value.size().x, head.center().y - value.size().y / 2.0), value, m.primary);
    }
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), m3::TOUCH), Sense::hover());
    pills(ui, rect, &mut s);
    ui.add_space(4.0);
    if s.party {
        let m = crate::material::scheme();
        let g = m3::galley_wrapped(ui, PARTY_NOTE, Type::BodyMedium, m.on_surface_variant, ui.available_width(), 4);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y + 8.0), Sense::hover());
        ui.painter().galley(r.min, g, m.on_surface_variant);
    }
    s.apply(app);
}

/// The speed that plays, as the group's label says it.
pub(super) fn settings_value(app: &App) -> String {
    label(app.speed_now())
}

/// The row of speeds as a settings row's control.
pub(super) fn settings_pills(app: &mut App, ui: &mut Ui) {
    let mut s = Showing::of(app);
    let width = if crate::platform::phone() { ui.available_width() } else { ui.available_width().min(300.0) };
    let (rect, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
    pills(ui, rect, &mut s);
    s.apply(app);
}

/// What Settings says under "Playback speed".
pub(super) fn settings_note(app: &App) -> &'static str {
    if app.jam.active() { PARTY_NOTE } else { "How fast songs play. Positions and lengths stay the song's own." }
}

/// A speedometer in the icon font's line (the font has none), for the menu row.
pub(super) fn gauge(ui: &Ui, center: Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let at = |x: f32, y: f32| center + vec2(x - 12.0, y - 12.0) * k;
    let stroke = egui::Stroke::new(2.0 * k, color);
    // The dial, open at the bottom, and the needle pointing up to the right.
    let dial = (0..=20).map(|i| {
        let a = (150.0 + 240.0 * i as f32 / 20.0).to_radians();
        at(12.0 + 9.0 * a.cos(), 13.0 + 9.0 * a.sin())
    });
    ui.painter().add(egui::Shape::line(dial.collect(), stroke));
    ui.painter().line_segment([at(12.0, 13.0), at(16.0, 8.5)], stroke);
    ui.painter().circle_filled(at(12.0, 13.0), 1.6 * k, color);
}

/// The menu row's words: the name, and the speed when it isn't 1×.
pub(super) fn menu_label(app: &App) -> String {
    match app.speed_now() {
        speed if speed == 1.0 => "Playback speed".to_owned(),
        speed => format!("Playback speed · {}", label(speed)),
    }
}

/// What the sheet holds: its title with the speed that plays, the speeds, and the
/// pitch switch.
fn contents(ui: &mut Ui, s: &mut Showing) {
    let w = ui.available_width();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

    let material = m3::on();
    let (head, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
    if material {
        let m = crate::material::scheme();
        m3::text(ui, pos2(head.left() + 8.0, head.top()), "Playback speed", Type::HeadlineSmall, m.on_surface, w - 110.0);
        let value = m3::galley(ui, &label(s.playing), Type::HeadlineSmall.spec().weight(crate::material::Weight::Medium), m.primary, 100.0);
        ui.painter().galley(pos2(head.right() - 8.0 - value.size().x, head.top()), value, m.primary);
    } else {
        widgets::text_at(ui, pos2(head.left() + 8.0, head.top()), "Playback speed", theme::regular(24.0), theme::text(), w - 110.0);
        let value = widgets::galley(ui, &label(s.playing), theme::semibold(24.0), s.accent, 100.0);
        ui.painter().galley(pos2(head.right() - 8.0 - value.size().x, head.top()), value, s.accent);
    }
    ui.add_space(12.0);

    let (row, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
    pills(ui, row, s);
    ui.add_space(12.0);

    // The pitch: the whole row switches it, and so does the switch.
    let (r, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
    let row = named(ui.interact(r, pitch_id(), Sense::click()), "Keep the pitch");
    if row.clicked() {
        s.keep_pitch = !s.keep_pitch;
    }
    let text_w = w - 16.0 - 60.0;
    let note = if s.keep_pitch { "Voices and instruments stay in tune" } else { "The pitch rises and falls with the speed" };
    if material {
        let m = crate::material::scheme();
        m3::two_lines(ui, r.left() + 8.0, r.center().y, text_w, "Keep the pitch", Type::BodyLarge, m.on_surface, note, Type::BodyMedium, m.on_surface_variant);
    } else {
        widgets::text_at(ui, pos2(r.left() + 8.0, r.top() + 8.0), "Keep the pitch", theme::regular(17.0), theme::text(), text_w);
        widgets::text_at(ui, pos2(r.left() + 8.0, r.top() + 32.0), note, theme::regular(13.5), theme::text_dim(), text_w);
    }
    let switch = if material { Rect::from_center_size(pos2(r.right() - 8.0 - pages::toggle_size().x / 2.0, r.center().y), pages::toggle_size()) } else { Rect::from_center_size(pos2(r.right() - 8.0 - 22.0, r.center().y), vec2(44.0, 26.0)) };
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(switch));
    pages::toggle(&mut sui, &mut s.keep_pitch, "Keep the pitch");

    if s.party {
        ui.add_space(4.0);
        let g = if material { m3::galley_wrapped(ui, PARTY_NOTE, Type::BodyMedium, theme::text_dim(), w - 16.0, 3) } else { widgets::galley_wrapped(ui, PARTY_NOTE, theme::regular(13.5), theme::text_dim(), w - 16.0, 3) };
        let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + 8.0), Sense::hover());
        ui.painter().galley(pos2(r.left() + 8.0, r.top()), g, theme::text_dim());
    }
}

/// Dev hooks: YTM_SPEED=<speed> plays at that speed; YTM_SPEED_SHEET opens the sheet.
pub(super) fn dev_hooks(app: &mut App) {
    if let Some(speed) = super::dev_var("YTM_SPEED").and_then(|v| v.parse().ok()) {
        app.speed = crate::speed::nearest(speed);
        app.set_speed();
    }
    if super::dev_var("YTM_SPEED_SHEET").is_some() {
        app.speed_sheet = true;
    }
}

/// The sheet, over Now Playing.
pub(super) fn sheet(app: &mut App, ctx: &egui::Context) {
    let mut s = Showing::of(app);
    let mut open = app.speed_sheet;
    sheet::show(ctx, "playback-speed", &mut open, |ui| contents(ui, &mut s));
    app.speed_sheet = open;
    s.apply(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontFamily;

    /// The sheet's contents in a headless egui, 380 points wide, with stand-ins for the
    /// app's fonts.
    struct Sheet {
        ctx: egui::Context,
        s: Showing,
        time: f64,
    }

    impl Sheet {
        fn new(s: Showing) -> Self {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let plain = fonts.families[&FontFamily::Proportional].clone();
            for family in ["semibold", "bold", "icons"] {
                fonts.families.insert(FontFamily::Name(family.into()), plain.clone());
            }
            ctx.set_fonts(fonts);
            let mut sheet = Self { ctx, s, time: 0.0 };
            sheet.frame(vec![]);
            sheet.frame(vec![]);
            sheet
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(380.0, 400.0))), time: Some(self.time), events, ..Default::default() };
            let out = self.ctx.run_ui(input, |ui| contents(ui, &mut self.s));
            out.drop_without_applying_deltas();
        }

        fn rect(&self, id: egui::Id) -> Option<Rect> {
            self.ctx.read_response(id).map(|r| r.rect)
        }

        fn tap(&mut self, id: egui::Id) {
            let at = self.rect(id).expect("the control is on the sheet").center();
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            self.frame(vec![egui::Event::PointerMoved(at), button(true)]);
            self.frame(vec![button(false)]);
            self.frame(vec![]);
        }
    }

    fn showing(speed: f32, party: bool) -> Showing {
        Showing { speed, playing: if party { 1.0 } else { speed }, keep_pitch: true, party, accent: Color32::from_rgb(250, 88, 106) }
    }

    #[test]
    fn the_six_speeds_sit_in_one_row_and_a_tap_picks_one() {
        let mut sheet = Sheet::new(showing(1.0, false));
        let cells: Vec<Rect> = SPEEDS.iter().map(|s| sheet.rect(speed_id(*s)).expect("every speed is there")).collect();
        assert!(cells.windows(2).all(|w| w[0].right() <= w[1].left() && w[0].top() == w[1].top()), "one row, left to right");
        assert!(cells.iter().all(|c| c.width() >= 40.0 && c.height() >= 44.0), "each a finger wide");
        assert!(cells[5].right() <= 380.0);
        sheet.tap(speed_id(1.5));
        assert_eq!(sheet.s.speed, 1.5);
        sheet.tap(speed_id(0.5));
        assert_eq!(sheet.s.speed, 0.5);
    }

    #[test]
    fn the_pitch_switch_turns_with_its_row() {
        let mut sheet = Sheet::new(showing(1.25, false));
        assert!(sheet.s.keep_pitch);
        sheet.tap(pitch_id());
        assert!(!sheet.s.keep_pitch);
        sheet.tap(pitch_id());
        assert!(sheet.s.keep_pitch);
    }

    #[test]
    fn in_a_party_the_speed_stays_at_1x_whatever_was_set() {
        let mut sheet = Sheet::new(showing(1.5, true));
        sheet.tap(speed_id(2.0));
        assert_eq!(sheet.s.speed, 1.5, "the listener's own speed is kept for after the party");
        assert_eq!(sheet.s.playing, 1.0);
    }
}
