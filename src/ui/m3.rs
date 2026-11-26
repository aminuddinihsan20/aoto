//! Material 3 components for the phone, drawn in the roles of `material::scheme()`:
//! text set in the type scale, chips, section headers, buttons, the switch, the list
//! row, the wavy progress indicator, the carousel and the tiles of Home. What each
//! one measures is a plain function, so it can be checked without drawing.

use std::collections::HashSet;
use std::sync::Arc;

use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use super::widgets::{self, named};
use super::{Action, glyphs};
use crate::material::{self, Spec, Type, Weight, shape};
use crate::model::{Card, Target, Thumb, Track};
use crate::theme::{self, icon};

/// Is this the phone, drawn in Material You? Then pages shared with the desktop draw
/// their Material layout.
pub fn on() -> bool {
    crate::platform::phone() && material::on()
}

/// The least a finger needs: every control can be hit in a square this size.
pub const TOUCH: f32 = 48.0;
/// How strongly a pressed control is washed with its content colour.
const PRESSED: f32 = 0.1;

impl From<Type> for Spec {
    fn from(t: Type) -> Spec {
        t.spec()
    }
}

// ------------------------------------------------------------------ plates

/// What stands behind a plate in a look that stacks its cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    /// Nothing: the plate alone (with its outline, in a look that draws one).
    None,
    /// A second card of the card colour: a card, a field, a quiet button.
    Card,
    /// A second card of the line colour: the one thing to press.
    Ink,
}

/// Where a plate's second card is: `rect` set off right and down by the look's stack,
/// or `None` in a look that draws none (or for a plate that asks for none).
pub fn stack_rect(rect: Rect, stack: Stack, style: &material::Style) -> Option<Rect> {
    (stack != Stack::None && style.stack > 0.0).then(|| rect.translate(Vec2::splat(style.stack.min(rect.height() / 8.0).max(2.0))))
}

/// The shapes of a filled plate as the look draws it: just the fill in Indigo; in
/// Sticker the fill with an outline, over a second card when `stack` asks for one.
pub fn plate_shape(rect: Rect, radius: impl Into<CornerRadius>, fill: Color32, stack: Stack) -> egui::Shape {
    let (m, style) = (material::scheme(), material::style());
    let radius = radius.into();
    if !style.outlined() {
        return egui::epaint::RectShape::filled(rect, radius, fill).into();
    }
    let line = Stroke::new(style.outline, style.line);
    let mut shapes = Vec::with_capacity(4);
    if let Some(back) = stack_rect(rect, stack, &style) {
        let behind = if stack == Stack::Ink { style.line } else { m.surface_container };
        shapes.push(egui::epaint::RectShape::filled(back, radius, behind).into());
        shapes.push(egui::epaint::RectShape::stroke(back, radius, line, StrokeKind::Inside).into());
    }
    shapes.push(egui::epaint::RectShape::filled(rect, radius, fill).into());
    shapes.push(egui::epaint::RectShape::stroke(rect, radius, line, StrokeKind::Inside).into());
    egui::Shape::Vec(shapes)
}

/// Paint a plate (see [`plate_shape`]).
pub fn plate(ui: &Ui, rect: Rect, radius: impl Into<CornerRadius>, fill: Color32, stack: Stack) {
    ui.painter().add(plate_shape(rect, radius, fill, stack));
}

/// The look's line round `rect`, in a look that draws one: the frame of a cover or a tile.
pub fn outline(ui: &Ui, rect: Rect, radius: impl Into<CornerRadius>) {
    let style = material::style();
    if style.outlined() {
        ui.painter().rect_stroke(rect, radius, Stroke::new(style.outline, style.line), StrokeKind::Inside);
    }
}

/// The ground under the row that is playing (or the one chosen): a tonal wash, and in a
/// look that draws lines an outlined card, since its wash alone is too near the page.
pub fn row_wash(ui: &Ui, rect: Rect, fill: Color32) {
    if material::style().outlined() {
        // Kept inside what the list shows, so the line's sides are not cut off.
        let clip = ui.clip_rect();
        let rect = Rect::from_min_max(pos2(rect.left().max(clip.left() + 1.0), rect.top()), pos2(rect.right().min(clip.right() - 1.0), rect.bottom()));
        plate(ui, rect, round(shape::CARD_SMALL), fill, Stack::None);
    } else {
        ui.painter().rect_filled(rect, shape::CARD_SMALL, fill);
    }
}

/// A corner as the look rounds it (Sticker's are squarer).
pub fn round(corner: f32) -> f32 {
    material::style().round(corner)
}

// ------------------------------------------------------------------ text

/// One line in a type role, cut with an ellipsis at `width`.
pub fn galley(ui: &Ui, text: &str, role: impl Into<Spec>, color: Color32, width: f32) -> Arc<egui::Galley> {
    widgets::galley_fmt(ui, text, role.into().format(color), width)
}

/// Wrapped text in a type role, at most `rows` lines.
pub fn galley_wrapped(ui: &Ui, text: &str, role: impl Into<Spec>, color: Color32, width: f32, rows: usize) -> Arc<egui::Galley> {
    widgets::galley_wrapped_fmt(ui, text, role.into().format(color), width, rows)
}

/// Wrapped text in a type role with every line centred, at most `rows` lines. Painted
/// at a point, its lines are centred on that point's x.
pub fn galley_centered(ui: &Ui, text: &str, role: impl Into<Spec>, color: Color32, width: f32, rows: usize) -> Arc<egui::Galley> {
    let wrapped = galley_wrapped(ui, text, role, color, width, rows);
    let mut job = (*wrapped.job).clone();
    job.halign = egui::Align::Center;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Paint one line in a type role with its top left at `pos`; returns where it is.
pub fn text(ui: &Ui, pos: Pos2, text: &str, role: impl Into<Spec>, color: Color32, width: f32) -> Rect {
    let g = galley(ui, text, role, color, width);
    let rect = Rect::from_min_size(pos, g.size());
    ui.painter().galley(pos, g, color);
    rect
}

/// A title over its supporting line, the pair centred on `cy`, from `left` to `left + width`.
#[allow(clippy::too_many_arguments)]
pub fn two_lines(ui: &Ui, left: f32, cy: f32, width: f32, title: &str, title_role: impl Into<Spec>, title_color: Color32, sub: &str, sub_role: impl Into<Spec>, sub_color: Color32) {
    let (t, s) = (galley(ui, title, title_role, title_color, width), galley(ui, sub, sub_role, sub_color, width));
    let top = cy - (t.size().y + s.size().y) / 2.0;
    let sub_top = top + t.size().y;
    ui.painter().galley(pos2(left, top), t, title_color);
    ui.painter().galley(pos2(left, sub_top), s, sub_color);
}

// ------------------------------------------------------------------ icons

/// An icon from either of the app's sets: the icon font, or its own drawn glyphs.
#[derive(Clone, Copy)]
pub enum Icon {
    Font(char),
    Glyph(&'static [u8]),
    /// One of `drawn`'s marks, which neither set has.
    Drawn(fn(&Ui, Pos2, f32, Color32)),
}

impl From<char> for Icon {
    fn from(c: char) -> Icon {
        Icon::Font(c)
    }
}

impl From<&'static [u8]> for Icon {
    fn from(g: &'static [u8]) -> Icon {
        Icon::Glyph(g)
    }
}

/// Draw `icon` centred on `center`, `size` points across.
pub fn paint_icon(ui: &Ui, center: Pos2, size: f32, icon: Icon, color: Color32) {
    match icon {
        // The font's glyphs fill less of their box than the drawn ones.
        Icon::Font(c) => widgets::icon_at(ui, center, c, size * 0.85, color),
        Icon::Glyph(g) => glyphs::draw(ui, center, size, g, color),
        Icon::Drawn(draw) => draw(ui, center, size, color),
    }
}

/// Marks the icon font lacks, drawn in its line on its 24-unit grid: `center` is the
/// middle of the grid and `size` its side.
pub mod drawn {
    use egui::{Color32, Pos2, Stroke, Ui, vec2};

    /// The point (`x`, `y`) of the 24-unit grid.
    fn at(center: Pos2, size: f32, x: f32, y: f32) -> Pos2 {
        center + (vec2(x, y) - vec2(12.0, 12.0)) * (size / 24.0)
    }

    /// A line through the grid's `points`, two units thick with round ends.
    fn line(ui: &Ui, center: Pos2, size: f32, color: Color32, points: &[(f32, f32)]) {
        let width = 2.0 * size / 24.0;
        let points: Vec<Pos2> = points.iter().map(|&(x, y)| at(center, size, x, y)).collect();
        for end in [points.first(), points.last()].into_iter().flatten() {
            ui.painter().circle_filled(*end, width / 2.0, color);
        }
        ui.painter().add(egui::Shape::line(points, Stroke::new(width, color)));
    }

    pub fn plus(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        line(ui, center, size, color, &[(5.0, 12.0), (19.0, 12.0)]);
        line(ui, center, size, color, &[(12.0, 5.0), (12.0, 19.0)]);
    }

    pub fn minus(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        line(ui, center, size, color, &[(5.0, 12.0), (19.0, 12.0)]);
    }

    /// Three sliders: sound, and the settings of a page.
    pub fn tune(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        for (y, knob) in [(6.0, 15.0), (12.0, 8.0), (18.0, 13.0)] {
            line(ui, center, size, color, &[(4.0, y), (knob - 3.0, y)]);
            line(ui, center, size, color, &[(knob + 3.0, y), (20.0, y)]);
            line(ui, center, size, color, &[(knob, y - 2.5), (knob, y + 2.5)]);
        }
    }

    /// Three bars of a chart: listening stats.
    pub fn bars(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        for (x, top) in [(6.0, 11.0), (12.0, 5.0), (18.0, 14.0)] {
            line(ui, center, size, color, &[(x, 19.0), (x, top)]);
        }
    }

    /// An "i" in a ring.
    pub fn info(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        let k = size / 24.0;
        ui.painter().circle_stroke(center, 9.0 * k, Stroke::new(2.0 * k, color));
        ui.painter().circle_filled(at(center, size, 12.0, 8.0), 1.3 * k, color);
        line(ui, center, size, color, &[(12.0, 11.5), (12.0, 16.5)]);
    }

    /// Two arrows passing each other: music brought over from elsewhere.
    pub fn swap(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        line(ui, center, size, color, &[(4.0, 8.0), (20.0, 8.0)]);
        line(ui, center, size, color, &[(16.0, 4.0), (20.0, 8.0), (16.0, 12.0)]);
        line(ui, center, size, color, &[(20.0, 16.0), (4.0, 16.0)]);
        line(ui, center, size, color, &[(8.0, 12.0), (4.0, 16.0), (8.0, 20.0)]);
    }

    /// A check over a line: this is downloaded.
    pub fn saved(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        line(ui, center, size, color, &[(6.0, 10.0), (10.0, 14.0), (18.0, 5.0)]);
        line(ui, center, size, color, &[(5.0, 19.0), (19.0, 19.0)]);
    }

    /// An arc that turns: something is on its way (a song that is loading).
    pub fn spinner(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        let a = ui.input(|i| i.time) as f32 * 6.0;
        let arc = (0..16).map(|i| center + vec2((a + i as f32 * 0.24).cos(), (a + i as f32 * 0.24).sin()) * size * 0.38);
        ui.painter().add(egui::Shape::line(arc.collect(), Stroke::new(2.4 * size / 24.0, color)));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }

    /// Wi-Fi: three arcs over the dot they share as their centre.
    pub fn wifi(ui: &Ui, center: Pos2, size: f32, color: Color32) {
        let k = size / 24.0;
        let dot = center + vec2(0.0, 7.5) * k;
        let stroke = Stroke::new(2.0 * k, color);
        for (radius, half) in [(5.0, 44.4_f32), (10.0, 44.4), (15.0, 41.8)] {
            let arc = (0..=12).map(|i| {
                let a = (-90.0 - half + 2.0 * half * i as f32 / 12.0).to_radians();
                dot + vec2(a.cos(), a.sin()) * radius * k
            });
            ui.painter().add(egui::Shape::line(arc.collect(), stroke));
        }
        ui.painter().circle_filled(dot, 1.3 * k, color);
    }
}

// ------------------------------------------------------------------ chips

/// How tall a chip is drawn (its touch target is `TOUCH`).
pub const CHIP_H: f32 = 32.0;
/// The room between two chips in a row.
pub const CHIP_GAP: f32 = 8.0;

/// The room before a chip's label: its padding, and the check of a selected one.
fn chip_lead(selected: bool) -> f32 {
    if selected { 8.0 + 18.0 + 8.0 } else { 16.0 }
}

/// How much room `filter_chip` takes, for a row that places its chips itself.
pub fn filter_chip_size(ui: &Ui, label: &str, selected: bool) -> Vec2 {
    vec2(chip_lead(selected) + galley(ui, label, Type::LabelLarge, Color32::PLACEHOLDER, 240.0).size().x + 16.0, TOUCH)
}

/// A filter chip: selected it is filled with the secondary container and led by a
/// check, otherwise it is an outline.
pub fn filter_chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let (m, style) = (material::scheme(), material::style());
    let ink = if selected { if style.outlined() { style.on_accent } else { m.on_secondary_container } } else if style.outlined() { m.on_surface } else { m.on_surface_variant };
    let label_g = galley(ui, label, Type::LabelLarge, ink, 240.0);
    let lead = chip_lead(selected);
    let (rect, resp) = ui.allocate_exact_size(vec2(lead + label_g.size().x + 16.0, TOUCH), Sense::click());
    let resp = named(resp, label);
    let chip = Rect::from_center_size(rect.center(), vec2(rect.width(), CHIP_H));
    if style.outlined() {
        // Sticker: the chosen chip is the accent on a second card, the others plain cards.
        plate(ui, chip, shape::CHIP, if selected { style.accent } else { m.surface_container }, if selected { Stack::Ink } else { Stack::None });
        if selected {
            widgets::icon_at(ui, pos2(chip.left() + 8.0 + 9.0, chip.center().y), icon::CHECK, 16.0, ink);
        }
    } else if selected {
        ui.painter().rect_filled(chip, shape::CHIP, m.secondary_container);
        widgets::icon_at(ui, pos2(chip.left() + 8.0 + 9.0, chip.center().y), icon::CHECK, 16.0, ink);
    } else {
        ui.painter().rect_stroke(chip, shape::CHIP, Stroke::new(1.0, m.outline_variant), StrokeKind::Inside);
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(chip, shape::CHIP, ink.gamma_multiply(PRESSED));
    }
    ui.painter().galley(pos2(chip.left() + lead, chip.center().y - label_g.size().y / 2.0), label_g, ink);
    resp
}

/// A row of chips that reaches both sides of the screen and follows a finger sideways.
/// `side` is the page's margin: the first chip starts there, and the row runs on past it.
pub fn chip_row<R>(ui: &mut Ui, id: &str, side: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
    let wide = row.expand2(vec2(side, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(wide));
    child.set_clip_rect(Rect::from_x_y_ranges(wide.x_range(), ui.clip_rect().y_range()));
    widgets::h_strip(&mut child, id, |ui| {
        ui.spacing_mut().item_spacing.x = CHIP_GAP;
        ui.add_space(side - CHIP_GAP);
        let out = add(ui);
        ui.add_space(side - CHIP_GAP);
        out
    })
}

// ------------------------------------------------------------------ section header

/// What stands at the right of a section header.
pub enum Trailing<'a> {
    None,
    /// A text action ("Play all").
    Text(&'a str),
    /// An icon action, with its accessible name.
    Icon(char, &'a str),
}

/// A section's header: its title, optionally an overline and a round picture before
/// it, and a trailing action.
pub struct SectionHeader<'a> {
    pub title: &'a str,
    pub overline: Option<&'a str>,
    pub avatar: Option<&'a str>,
    pub trailing: Trailing<'a>,
}

/// Draw a section header; true when its trailing action was tapped.
pub fn section_header(ui: &mut Ui, h: &SectionHeader) -> bool {
    let m = material::scheme();
    ui.add_space(8.0);
    // Its action's touch target is taller than the row and reaches into the room round it.
    let height = if h.overline.is_some() { 52.0 } else { 44.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let mut tapped = false;
    let mut right = rect.right();
    match h.trailing {
        Trailing::None => {}
        Trailing::Text(label) => {
            let g = galley(ui, label, Type::LabelLarge, m.primary, 160.0);
            let hit = Rect::from_min_max(pos2(rect.right() - g.size().x - 16.0, rect.center().y - TOUCH / 2.0), pos2(rect.right(), rect.center().y + TOUCH / 2.0));
            let resp = named(ui.interact(hit, ui.scope_id().with(("section-action", h.title)), Sense::click()), label);
            if resp.is_pointer_button_down_on() {
                ui.painter().rect_filled(hit.shrink2(vec2(0.0, 6.0)), 18.0, m.primary.gamma_multiply(PRESSED));
            }
            ui.painter().galley(pos2(hit.right() - 4.0 - g.size().x, hit.center().y - g.size().y / 2.0), g, m.primary);
            tapped = resp.clicked();
            right = hit.left();
        }
        Trailing::Icon(glyph, name) => {
            let center = pos2(rect.right() - 16.0, rect.center().y);
            tapped = IconButton::new(glyph, name).show_at(ui, center, ui.scope_id().with(("section-action", h.title))).clicked();
            right = center.x - TOUCH / 2.0;
        }
    }
    let mut left = rect.left();
    if let Some(url) = h.avatar {
        let pic = Rect::from_center_size(pos2(left + 20.0, rect.center().y), Vec2::splat(40.0));
        ui.painter().circle_filled(pic.center(), 20.0, m.surface_container_high);
        widgets::paint_remote(ui, url, pic, 20.0, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        left = pic.right() + 12.0;
    }
    let width = right - 8.0 - left;
    match h.overline {
        Some(over) => two_lines(ui, left, rect.center().y, width, &over.to_uppercase(), Type::LabelMedium, m.on_surface_variant, h.title, Type::TitleLarge, m.on_surface),
        None => {
            let g = galley(ui, h.title, Type::TitleLarge, m.on_surface, width);
            ui.painter().galley(pos2(left, rect.center().y - g.size().y / 2.0), g, m.on_surface);
        }
    }
    tapped
}

// ------------------------------------------------------------------ buttons

/// How a button stands out: filled with the primary colour for the one main action,
/// tonal (the secondary container) for the others.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Filled,
    Tonal,
    /// Only its label, in the primary colour: a dialog's way out.
    Text,
    /// Filled with the error colour: an action that destroys something.
    Danger,
}

/// A labelled button, 40 points tall in a touch target of 48, with an optional icon
/// before its label.
pub fn button(ui: &mut Ui, label: &str, glyph: Option<char>, style: ButtonStyle) -> Response {
    let (m, look) = (material::scheme(), material::style());
    let (fill, ink) = match style {
        ButtonStyle::Filled => (look.accent, look.on_accent),
        ButtonStyle::Tonal => (m.secondary_container, m.on_secondary_container),
        ButtonStyle::Text => (Color32::TRANSPARENT, m.primary),
        ButtonStyle::Danger => (m.error, m.on_error),
    };
    let g = galley(ui, label, Type::LabelLarge, ink, 280.0);
    // A text button has less room round its label: there is no container to fill.
    let pad = if style == ButtonStyle::Text { 12.0 } else { 24.0 };
    let lead = if glyph.is_some() { pad - 8.0 + 18.0 + 8.0 } else { pad };
    let (rect, resp) = ui.allocate_exact_size(vec2((lead + g.size().x + pad).max(TOUCH), TOUCH), Sense::click());
    let resp = named(resp, label);
    let body = Rect::from_center_size(rect.center(), vec2(rect.width(), 40.0));
    let fade = if ui.is_enabled() { 1.0 } else { 0.38 };
    let radius = look.round(20.0);
    if style == ButtonStyle::Text {
        ui.painter().rect_filled(body, radius, fill.gamma_multiply(fade));
    } else {
        plate(ui, body, radius, fill.gamma_multiply(fade), if style == ButtonStyle::Filled { Stack::Ink } else { Stack::Card });
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(body, radius, ink.gamma_multiply(PRESSED));
    }
    let ink = ink.gamma_multiply(fade);
    if let Some(glyph) = glyph {
        widgets::icon_at(ui, pos2(body.left() + lead - 8.0 - 9.0, body.center().y), glyph, 17.0, ink);
    }
    ui.painter().galley(pos2(body.left() + lead, body.center().y - g.size().y / 2.0), g, ink);
    resp
}

/// A page's one large button, filling `rect`: the Play of an album. Its label follows
/// its icon, the pair centred.
pub fn big_button(ui: &Ui, rect: Rect, label: &str, icon: impl Into<Icon>, id: egui::Id) -> Response {
    let look = material::style();
    let (fill, ink) = (look.accent, look.on_accent);
    let resp = named(ui.interact(rect, id, Sense::click()), label);
    let radius = look.round(rect.height() / 2.0);
    plate(ui, rect, radius, fill, Stack::Ink);
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, radius, ink.gamma_multiply(PRESSED));
    }
    let g = galley(ui, label, Type::TitleMedium, ink, rect.width() - 52.0);
    let left = rect.center().x - (22.0 + 8.0 + g.size().x) / 2.0;
    paint_icon(ui, pos2(left + 11.0, rect.center().y), 22.0, icon.into(), ink);
    ui.painter().galley(pos2(left + 30.0, rect.center().y - g.size().y / 2.0), g, ink);
    resp
}

/// How an icon button stands out: no container, a tonal one, or the primary colour.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconStyle {
    Standard,
    /// A round action beside the main one (a collection's header, the transport).
    Tonal,
    /// A quieter container, the colour of a card: an action among several.
    Surface,
    /// A thin container over artwork or a gradient, where a surface colour would stand out.
    Overlay,
    /// The same, its icon in the primary colour: a mode that is on (repeat).
    Lit,
    Filled,
}

/// An icon button. Its container is `size` across (40 unless said) and round unless
/// given a `radius`; whatever its size, a finger hits it in a square of at least 48.
pub struct IconButton<'a> {
    icon: Icon,
    name: &'a str,
    style: IconStyle,
    size: f32,
    radius: Option<f32>,
    glyph: Option<f32>,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: impl Into<Icon>, name: &'a str) -> Self {
        Self { icon: icon.into(), name, style: IconStyle::Standard, size: 40.0, radius: None, glyph: None }
    }

    /// How large its icon is drawn, when not the size that goes with its container.
    pub fn glyph(mut self, size: f32) -> Self {
        self.glyph = Some(size);
        self
    }

    pub fn style(mut self, style: IconStyle) -> Self {
        self.style = style;
        self
    }

    /// How wide and tall its container is.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// A squarer corner than a circle's.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// The square a finger hits it in.
    fn target(&self) -> f32 {
        self.size.max(TOUCH)
    }

    /// Draw it in the layout.
    pub fn show(self, ui: &mut Ui) -> Response {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(self.target()), Sense::hover());
        let id = ui.scope_id().with(("icon-button", self.name, rect.min.x as i32, rect.min.y as i32));
        self.show_at(ui, rect.center(), id)
    }

    /// Draw it centred on `center`, for a place the caller measured itself.
    pub fn show_at(self, ui: &Ui, center: Pos2, id: egui::Id) -> Response {
        let m = material::scheme();
        let resp = named(ui.interact(Rect::from_center_size(center, Vec2::splat(self.target())), id, Sense::click()), self.name);
        let body = Rect::from_center_size(center, Vec2::splat(self.size));
        let look = material::style();
        let radius = look.round(self.radius.unwrap_or(self.size / 2.0));
        let (fill, ink) = match self.style {
            IconStyle::Standard => (None, m.on_surface_variant),
            IconStyle::Tonal => (Some(m.secondary_container), m.on_secondary_container),
            IconStyle::Surface => (Some(m.surface_container_high), m.on_surface_variant),
            IconStyle::Overlay => (Some(m.glass(0.1)), m.on_surface),
            IconStyle::Lit => (Some(m.glass(0.1)), m.primary),
            IconStyle::Filled => (Some(look.accent), look.on_accent),
        };
        if let Some(fill) = fill {
            // Sticker: a container is a small card; the filled one is the thing to press.
            let fill = if look.outlined() && matches!(self.style, IconStyle::Surface | IconStyle::Overlay | IconStyle::Lit) { m.surface_container } else { fill };
            plate(ui, body, radius, fill, if self.style == IconStyle::Filled { Stack::Ink } else if look.outlined() && self.size >= 40.0 { Stack::Card } else { Stack::None });
        }
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(body, radius, ink.gamma_multiply(PRESSED));
        }
        paint_icon(ui, center, self.glyph.unwrap_or((self.size * 0.6).min(28.0)), self.icon, ink);
        resp
    }
}

// ------------------------------------------------------------------ switch

/// The switch's track.
pub const SWITCH: Vec2 = vec2(52.0, 32.0);

/// Where the switch's handle is and how big, `t` of the way from off (0) to on (1):
/// it grows as it crosses, and a finger on it swells it.
pub fn switch_handle(track: Rect, t: f32, pressed: bool) -> (Pos2, f32) {
    let t = t.clamp(0.0, 1.0);
    let radius = if pressed { 14.0 } else { 8.0 + 4.0 * t };
    let half = track.height() / 2.0;
    (pos2(egui::lerp(track.left() + half..=track.right() - half, t), track.center().y), radius)
}

/// The room a switch takes: its track, in a target a finger's height.
pub const SWITCH_TARGET: Vec2 = vec2(SWITCH.x, TOUCH);

/// An on/off switch, its track centred in `SWITCH_TARGET`. Flips `on` when tapped.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> Response {
    let m = material::scheme();
    let (target, mut resp) = ui.allocate_exact_size(SWITCH_TARGET, Sense::click());
    let rect = Rect::from_center_size(target.center(), SWITCH);
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), value, name));
    let t = theme::anim_bool(ui.ctx(), resp.id.with("t"), value, 0.16);
    let fade = if ui.is_enabled() { 1.0 } else { 0.38 };
    let look = material::style();
    if look.outlined() {
        // Sticker: an outlined track that fills with the accent, and an outlined handle.
        let line = Stroke::new(look.outline, look.line.gamma_multiply(fade));
        ui.painter().rect_filled(rect, rect.height() / 2.0, theme::lerp_color(m.surface_container, look.accent, t).gamma_multiply(fade));
        ui.painter().rect_stroke(rect, rect.height() / 2.0, line, StrokeKind::Inside);
        let (center, radius) = switch_handle(rect, t, resp.is_pointer_button_down_on());
        ui.painter().circle_filled(center, radius, theme::lerp_color(m.outline, m.surface_container_lowest, t).gamma_multiply(fade));
        ui.painter().circle_stroke(center, radius, line);
        return resp;
    }
    let track = theme::lerp_color(m.surface_container_highest, m.primary, t);
    ui.painter().rect_filled(rect, rect.height() / 2.0, track.gamma_multiply(fade));
    if t < 1.0 {
        ui.painter().rect_stroke(rect, rect.height() / 2.0, Stroke::new(2.0, m.outline.gamma_multiply((1.0 - t) * fade)), StrokeKind::Inside);
    }
    let (center, radius) = switch_handle(rect, t, resp.is_pointer_button_down_on());
    ui.painter().circle_filled(center, radius, theme::lerp_color(m.outline, m.on_primary, t).gamma_multiply(fade));
    resp
}

// ------------------------------------------------------------------ list row

/// Where a list row's parts are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowGeom {
    pub cover: Rect,
    /// The two lines of text: from its left to its right, the row's whole height.
    pub text: Rect,
    /// The right edge of the trailing text (a duration, a count).
    pub trailing_right: f32,
    /// The touch target of the trailing "more" button.
    pub more: Option<Rect>,
}

/// A list row's height and the size of its cover: roomy for things (playlists,
/// albums, artists), tighter for songs.
pub fn row_size(dense: bool) -> (f32, f32) {
    if dense { (64.0, 48.0) } else { (72.0, 56.0) }
}

/// Lay a list row out in `rect`: the cover at its left, a "more" button at its right
/// when it has one, trailing text `trailing_w` wide before that, and the text between.
pub fn row_geom(rect: Rect, cover: f32, trailing_w: f32, more: bool) -> RowGeom {
    let cover = Rect::from_min_size(pos2(rect.left(), rect.center().y - cover / 2.0), Vec2::splat(cover));
    let more = more.then(|| Rect::from_center_size(pos2(rect.right() - TOUCH / 2.0, rect.center().y), Vec2::splat(TOUCH)));
    let trailing_right = more.map_or(rect.right(), |r| r.left());
    let text_right = if trailing_w > 0.0 { trailing_right - trailing_w - 12.0 } else { trailing_right - 4.0 };
    let text_left = cover.right() + 16.0;
    RowGeom { cover, text: Rect::from_min_max(pos2(text_left, rect.top()), pos2(text_right.max(text_left), rect.bottom())), trailing_right, more }
}

/// A list row: a cover, a title over a supporting line, and at its right an optional
/// short text and an optional "more" button.
#[derive(Default)]
pub struct ListRow<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub thumbs: &'a [Thumb],
    /// A round cover: an artist.
    pub round: bool,
    /// The tighter row of a song list.
    pub dense: bool,
    /// The row of what is playing: on a tonal container, its title in the primary colour.
    pub active: bool,
    pub trailing: Option<&'a str>,
    /// The accessible name of its "more" button, when it has one.
    pub more: Option<&'a str>,
    /// In place of a cover: an icon on a tonal tile (Liked songs, a folder).
    pub tile: Option<Icon>,
    /// A small mark before the supporting line: downloaded, pinned.
    pub mark: Option<Icon>,
    /// A chevron at the right: the row opens a page of its own.
    pub chevron: bool,
}

/// What was done to a list row.
pub struct ListRowOut {
    pub row: Response,
    pub more: Option<Response>,
}

/// Draw a list row across the layout's width.
pub fn list_row(ui: &mut Ui, r: &ListRow) -> ListRowOut {
    let m = material::scheme();
    let (height, cover) = row_size(r.dense);
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let row = named(row, r.title);
    if !ui.is_rect_visible(rect) {
        return ListRowOut { row, more: None };
    }
    let trailing = r.trailing.map(|t| galley(ui, t, Type::BodyMedium, m.on_surface_variant, 120.0));
    let trailing_w = trailing.as_ref().map_or(if r.chevron { 20.0 } else { 0.0 }, |t| t.size().x);
    let g = row_geom(rect, cover, trailing_w, r.more.is_some());
    if r.active || row.is_pointer_button_down_on() {
        let ground = rect.expand2(vec2(8.0, 0.0));
        if r.active {
            row_wash(ui, ground, m.surface_container);
        } else {
            ui.painter().rect_filled(ground, shape::CARD_SMALL, m.on_surface.gamma_multiply(PRESSED));
        }
    }
    let radius = if r.round { cover / 2.0 } else { shape::TILE_SMALL };
    match r.tile {
        Some(icon) => {
            ui.painter().rect_filled(g.cover, radius, m.primary_container);
            outline(ui, g.cover, radius);
            // A lined look's primary is its text colour, which the pastel tile would lose in the dark.
            let ink = if material::style().outlined() { m.on_primary_container } else { m.primary };
            paint_icon(ui, g.cover.center(), cover * 0.43, icon, ink);
        }
        None => widgets::paint_cover(ui, g.cover, r.thumbs, radius, 1.0),
    }
    let title_color = if r.active { m.primary } else { m.on_surface };
    let title = galley(ui, r.title, Type::BodyLarge, title_color, g.text.width());
    let lead = if r.mark.is_some() { 22.0 } else { 0.0 };
    let sub = galley(ui, r.subtitle, Type::BodyMedium, m.on_surface_variant, g.text.width() - lead);
    let top = rect.center().y - (title.size().y + sub.size().y) / 2.0;
    let sub_top = top + title.size().y;
    if let Some(mark) = r.mark {
        paint_icon(ui, pos2(g.text.left() + 8.0, sub_top + sub.size().y / 2.0), 16.0, mark, m.primary);
    }
    ui.painter().galley(pos2(g.text.left(), top), title, title_color);
    ui.painter().galley(pos2(g.text.left() + lead, sub_top), sub, m.on_surface_variant);
    if let Some(t) = trailing {
        ui.painter().galley(pos2(g.trailing_right - t.size().x, rect.center().y - t.size().y / 2.0), t, m.on_surface_variant);
    } else if r.chevron {
        widgets::icon_at(ui, pos2(g.trailing_right - 10.0, rect.center().y), icon::CHEVRON_RIGHT, 20.0, m.on_surface_variant);
    }
    let more = r.more.zip(g.more).map(|(name, at)| IconButton::new(icon::ELLIPSIS_VERTICAL, name).show_at(ui, at.center(), row.id.with("more")));
    ListRowOut { row, more }
}

// ------------------------------------------------------------------ search bar

/// A search bar's height.
pub const SEARCH_BAR_H: f32 = 48.0;

/// Where a search bar's parts are: the magnifier, the text, and the clear button's
/// touch target once there is something to clear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchBarGeom {
    pub icon: Pos2,
    pub text: Rect,
    pub clear: Option<Rect>,
}

/// Lay a search bar out in `rect`; `trailing` is room kept free at its right end, for
/// what the caller stands there (the account's picture).
pub fn search_bar_geom(rect: Rect, clearable: bool, trailing: f32) -> SearchBarGeom {
    let cy = rect.center().y;
    let right = rect.right() - trailing;
    let clear = clearable.then(|| Rect::from_center_size(pos2(right - TOUCH / 2.0, cy), Vec2::splat(TOUCH)));
    let text_right = clear.map_or(right - 16.0, |c| c.left());
    let left = rect.left() + 52.0;
    SearchBarGeom { icon: pos2(rect.left() + 26.0, cy), text: Rect::from_min_max(pos2(left, rect.top() + 4.0), pos2(text_right.max(left), rect.bottom() - 4.0)), clear }
}

/// What was done to a search bar.
pub struct SearchBarOut {
    pub edit: Response,
    /// Its clear button was tapped (the text is empty now).
    pub cleared: bool,
}

/// A search bar in `rect`: a pill of a tonal surface with the magnifier, the text being
/// typed (or `hint`) and, once there is text, a button that clears it.
pub fn search_bar(ui: &mut Ui, rect: Rect, text: &mut String, hint: &str, trailing: f32) -> SearchBarOut {
    let m = material::scheme();
    let g = search_bar_geom(rect, !text.is_empty(), trailing);
    if material::style().outlined() {
        plate(ui, rect, round(rect.height() / 2.0), m.surface_container, Stack::Card);
    } else {
        ui.painter().rect_filled(rect, rect.height() / 2.0, m.surface_container_high);
    }
    widgets::icon_at(ui, g.icon, icon::SEARCH, 20.0, m.on_surface);
    let edit = ui.put(
        g.text,
        egui::TextEdit::singleline(text)
            .hint_text(egui::RichText::new(hint).color(m.on_surface_variant))
            .font(Type::BodyLarge.font())
            .text_color(m.on_surface)
            .align(egui::Align2::LEFT_CENTER)
            .frame(egui::Frame::NONE)
            .desired_width(g.text.width()),
    );
    let mut cleared = false;
    if let Some(c) = g.clear
        && IconButton::new(icon::X, "Clear search").show_at(ui, c.center(), edit.id.with("clear")).clicked()
    {
        text.clear();
        cleared = true;
    }
    SearchBarOut { edit, cleared }
}

// ------------------------------------------------------------------ assist chip, FAB

/// An assist chip: an outline round a label, led by an icon. A tap does what it says.
pub fn assist_chip(ui: &mut Ui, label: &str, glyph: Option<char>) -> Response {
    let m = material::scheme();
    let label_g = galley(ui, label, Type::LabelLarge, m.on_surface, 200.0);
    let lead = if glyph.is_some() { 8.0 + 18.0 + 8.0 } else { 16.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(lead + label_g.size().x + 16.0, TOUCH), Sense::click());
    let resp = named(resp, label);
    let chip = Rect::from_center_size(rect.center(), vec2(rect.width(), CHIP_H));
    if material::style().outlined() {
        plate(ui, chip, shape::CHIP, m.surface_container, Stack::None);
    } else {
        ui.painter().rect_stroke(chip, shape::CHIP, Stroke::new(1.0, m.outline_variant), StrokeKind::Inside);
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(chip, shape::CHIP, m.on_surface.gamma_multiply(PRESSED));
    }
    if let Some(glyph) = glyph {
        widgets::icon_at(ui, pos2(chip.left() + 8.0 + 9.0, chip.center().y), glyph, 16.0, m.on_surface_variant);
    }
    ui.painter().galley(pos2(chip.left() + lead, chip.center().y - label_g.size().y / 2.0), label_g, m.on_surface);
    resp
}

/// A floating action button's side.
pub const FAB: f32 = 56.0;

/// Where a floating action button stands over a page whose viewport is `view`: in its
/// bottom right corner, above the mini player when a song is loaded (`lifted` by that much).
pub fn fab_rect(view: Rect, lifted: f32) -> Rect {
    Rect::from_min_size(pos2(view.right() - FAB, view.bottom() - 16.0 - lifted - FAB), Vec2::splat(FAB))
}

/// A floating action button in `rect`: the page's one main action, over its content.
pub fn fab(ui: &Ui, rect: Rect, icon: impl Into<Icon>, name: &str, id: egui::Id) -> Response {
    let m = material::scheme();
    let resp = named(ui.interact(rect, id, Sense::click()), name);
    if material::style().outlined() {
        plate(ui, rect, round(shape::TILE), m.primary_container, Stack::Ink);
    } else {
        widgets::soft_shadow(ui, rect, shape::TILE, 12.0, 0.35);
        ui.painter().rect_filled(rect, shape::TILE, m.primary_container);
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, shape::TILE, m.on_primary_container.gamma_multiply(PRESSED));
    }
    paint_icon(ui, rect.center(), 24.0, icon.into(), m.on_primary_container);
    resp
}

// ------------------------------------------------------------------ cards and their rows

/// A card's padding at its sides, and above and below what it holds.
pub const CARD_PAD: Vec2 = vec2(20.0, 4.0);

/// A group's label and what stands at its right, above the group's card.
pub struct GroupLabel<'a> {
    pub title: &'a str,
    pub value: Option<&'a str>,
}

/// A group of rows on a card of `fill`, under an optional label in the primary colour.
/// A group that put nothing on its card draws nothing and takes no room: a filtered
/// page shows only the groups that kept a row.
pub fn card<R>(ui: &mut Ui, label: Option<GroupLabel>, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let m = material::scheme();
    let (bg, text, value) = (ui.painter().add(egui::Shape::Noop), ui.painter().add(egui::Shape::Noop), ui.painter().add(egui::Shape::Noop));
    let min = ui.cursor().min;
    let width = ui.available_width();
    let label_h = if label.is_some() { 36.0 } else { 0.0 };
    let top = min.y + label_h;
    let inner = Rect::from_min_max(pos2(min.x + CARD_PAD.x, top + CARD_PAD.y), pos2(min.x + width - CARD_PAD.x, ui.max_rect().bottom().max(top + CARD_PAD.y)));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    let out = add(&mut child);
    let used = child.min_rect();
    if used.height() <= 0.0 {
        return out;
    }
    let body = Rect::from_min_max(pos2(min.x, top), pos2(min.x + width, used.bottom() + CARD_PAD.y));
    ui.painter().set(bg, plate_shape(body, round(shape::CARD), fill, Stack::Card));
    if let Some(l) = label {
        let g = galley(ui, l.title, Type::LabelLarge, m.primary, width - 16.0);
        ui.painter().set(text, egui::epaint::TextShape::new(pos2(min.x + 8.0, min.y + 8.0), g, m.primary));
        if let Some(v) = l.value {
            let g = galley(ui, v, Type::LabelLarge, m.on_surface_variant, width / 2.0);
            ui.painter().set(value, egui::epaint::TextShape::new(pos2(min.x + width - 8.0 - g.size().x, min.y + 8.0), g, m.on_surface_variant));
        }
    }
    ui.allocate_rect(Rect::from_min_max(min, body.max), Sense::hover());
    out
}

/// A navigation row's height: a category of Settings.
pub const NAV_ROW_H: f32 = 60.0;

/// A row that opens a page: an icon on a tonal disc, a title over a one-line summary,
/// and a chevron.
pub fn nav_row(ui: &mut Ui, icon: Icon, title: &str, summary: &str) -> Response {
    let m = material::scheme();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), NAV_ROW_H), Sense::click());
    let resp = named(resp, title);
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect.expand2(vec2(12.0, 0.0)), shape::CARD_SMALL, m.on_surface.gamma_multiply(PRESSED));
    }
    let disc = pos2(rect.left() + 16.0, rect.center().y);
    ui.painter().circle_filled(disc, 20.0, m.secondary_container);
    paint_icon(ui, disc, 20.0, icon, m.on_secondary_container);
    let left = disc.x + 20.0 + 16.0;
    two_lines(ui, left, rect.center().y, rect.right() - 28.0 - left, title, Type::BodyLarge, m.on_surface, summary, Type::BodySmall, m.on_surface_variant);
    widgets::icon_at(ui, pos2(rect.right() - 6.0, rect.center().y), icon::CHEVRON_RIGHT, 20.0, m.on_surface_variant);
    resp
}

/// A settings row's text: its title over a description of up to `rows` lines, in a
/// column `width` wide. Returns the two, laid out.
pub fn row_text(ui: &Ui, title: &str, desc: Option<&str>, width: f32, rows: usize) -> (Arc<egui::Galley>, Option<Arc<egui::Galley>>) {
    let m = material::scheme();
    (galley(ui, title, Type::BodyLarge, m.on_surface, width), desc.map(|d| galley_wrapped(ui, d, Type::BodyMedium, m.on_surface_variant, width, rows)))
}

// ------------------------------------------------------------------ segmented buttons

/// The segments of a row of connected buttons across `rect`: equal widths, a small gap.
pub fn segments(rect: Rect, count: usize) -> Vec<Rect> {
    const GAP: f32 = 4.0;
    let w = (rect.width() - GAP * count.saturating_sub(1) as f32) / count.max(1) as f32;
    (0..count).map(|i| Rect::from_min_size(pos2(rect.left() + i as f32 * (w + GAP), rect.top()), vec2(w, rect.height()))).collect()
}

/// A row of connected buttons, one of them chosen: a choice among a few (Light, Dark).
/// Returns the one tapped.
pub fn segmented(ui: &mut Ui, labels: &[&str], selected: usize) -> Option<usize> {
    segmented_any(ui, labels, Some(selected), material::scheme().surface_container_high).filter(|i| *i != selected)
}

/// The same row where none need be chosen: returns the one tapped, the chosen one
/// too, so a second tap on it can let it go. `rest` fills the ones not chosen (a
/// step above the surface the row stands on).
pub fn segmented_any(ui: &mut Ui, labels: &[&str], selected: Option<usize>, rest: Color32) -> Option<usize> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::hover());
    let mut tapped = None;
    let last = labels.len().saturating_sub(1);
    for (i, (label, rect)) in labels.iter().zip(segments(row, labels.len())).enumerate() {
        let resp = named(ui.interact(rect, ui.scope_id().with(("segment", *label, row.min.y as i32)), Sense::click()), label);
        let body = Rect::from_center_size(rect.center(), vec2(rect.width(), 44.0));
        segment(ui, body, label, &Segment { on: Some(i) == selected, first: i == 0, last: i == last, pressed: resp.is_pointer_button_down_on(), rest, fade: 1.0 });
        if resp.clicked() {
            tapped = Some(i);
        }
    }
    tapped
}

/// How one of a row of connected buttons is drawn: chosen or not, at an end of its
/// row, under a finger, the fill of one not chosen, and how present it is (a row that
/// cannot be used is faded).
pub struct Segment {
    pub on: bool,
    pub first: bool,
    pub last: bool,
    pub pressed: bool,
    pub rest: Color32,
    pub fade: f32,
}

/// Paint one of a row of connected buttons in `body`, for a row that takes its taps
/// itself. The chosen one is a pill in the primary colour; the others keep their round
/// ends at the row's ends only.
pub fn segment(ui: &Ui, body: Rect, label: &str, s: &Segment) {
    let m = material::scheme();
    let round = (body.height() / 2.0).min(255.0) as u8;
    let flat = shape::CHIP as u8;
    let corners = if s.on {
        CornerRadius::same(round)
    } else {
        let (l, r) = (if s.first { round } else { flat }, if s.last { round } else { flat });
        CornerRadius { nw: l, sw: l, ne: r, se: r }
    };
    let look = material::style();
    if look.outlined() {
        // Sticker: every segment is a small card; the chosen one is sky blue.
        let (fill, ink) = if s.on { (m.secondary_container, m.on_secondary_container) } else { (m.surface_container, m.on_surface) };
        plate(ui, body.shrink2(vec2(1.0, 0.0)), shape::CHIP, fill.gamma_multiply(s.fade), Stack::None);
        if s.pressed {
            ui.painter().rect_filled(body, shape::CHIP, ink.gamma_multiply(PRESSED));
        }
        let ink = ink.gamma_multiply(s.fade);
        let role = if body.width() < 46.0 { Type::LabelMedium } else { Type::LabelLarge };
        let g = galley(ui, label, role, ink, body.width() - 4.0);
        ui.painter().galley(body.center() - g.size() / 2.0, g, ink);
        return;
    }
    let (fill, ink) = if s.on { (m.primary, m.on_primary) } else { (s.rest, m.on_surface) };
    ui.painter().rect_filled(body, corners, fill.gamma_multiply(s.fade));
    if s.pressed {
        ui.painter().rect_filled(body, corners, ink.gamma_multiply(PRESSED));
    }
    let ink = ink.gamma_multiply(s.fade);
    // A narrow segment (six speeds on a small phone) takes the smaller label.
    let role = if body.width() < 46.0 { Type::LabelMedium } else { Type::LabelLarge };
    let g = galley(ui, label, role, ink, body.width() - 4.0);
    ui.painter().galley(body.center() - g.size() / 2.0, g, ink);
}

// ------------------------------------------------------------------ dialog

/// The card a dialog stands on.
pub fn dialog_frame() -> egui::Frame {
    let look = material::style();
    if look.outlined() {
        // Sticker: a card with its outline, and a hard shadow where the second card would be.
        return egui::Frame::new()
            .fill(material::scheme().surface_container)
            .corner_radius(round(shape::CARD) as u8)
            .stroke(Stroke::new(look.outline, look.line))
            .shadow(egui::epaint::Shadow { offset: [5, 5], blur: 0, spread: 0, color: look.line });
    }
    egui::Frame::new()
        .fill(material::scheme().surface_container_high)
        .corner_radius(shape::CARD as u8)
        .shadow(egui::epaint::Shadow { offset: [0, 12], blur: 40, spread: 0, color: theme::shade(0.5) })
}

/// A dialog's heading.
pub fn dialog_title(ui: &mut Ui, title: &str) {
    let m = material::scheme();
    let g = galley_wrapped(ui, title, Type::HeadlineSmall, m.on_surface, ui.available_width(), 2);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
    ui.painter().galley(rect.min, g, m.on_surface);
}

/// What a dialog says under its heading.
pub fn dialog_text(ui: &mut Ui, body: &str) {
    let m = material::scheme();
    let g = galley_wrapped(ui, body, Type::BodyMedium, m.on_surface_variant, ui.available_width(), 12);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
    ui.painter().galley(rect.min, g, m.on_surface_variant);
}

/// A text field across the layout's width: an outline that takes the primary colour
/// while it is typed in. `multiline`: a few rows tall.
pub fn text_field(ui: &mut Ui, text: &mut String, hint: &str, multiline: bool) -> Response {
    let m = material::scheme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), if multiline { 112.0 } else { 56.0 }), Sense::hover());
    // Painted first, so the text draws over it.
    let outline = ui.painter().add(egui::Shape::Noop);
    let inner = rect.shrink2(vec2(16.0, if multiline { 12.0 } else { 4.0 }));
    let edit = if multiline { egui::TextEdit::multiline(text).desired_rows(4) } else { egui::TextEdit::singleline(text).align(egui::Align2::LEFT_CENTER) };
    let resp = ui.put(inner, edit.hint_text(egui::RichText::new(hint).color(m.on_surface_variant)).font(Type::BodyLarge.font()).text_color(m.on_surface).frame(egui::Frame::NONE).desired_width(inner.width()));
    let focus = theme::anim_bool(ui.ctx(), resp.id.with("focus"), resp.has_focus(), 0.15);
    let look = material::style();
    if look.outlined() {
        // Sticker: a card with the look's own line, a little heavier while typed in.
        ui.painter().set(outline, egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, shape::TILE_SMALL, m.surface_container),
            egui::Shape::rect_stroke(rect, shape::TILE_SMALL, Stroke::new(look.outline + focus * 0.5, look.line), StrokeKind::Inside),
        ]));
        return resp;
    }
    ui.painter().set(outline, egui::Shape::rect_stroke(rect, shape::TILE_SMALL, Stroke::new(1.0 + focus, theme::lerp_color(m.outline, m.primary, focus)), StrokeKind::Inside));
    resp
}

// ------------------------------------------------------------------ slider, stepper, dropdown

/// Where a slider's parts are: the filled part of its track, its upright handle, and
/// the rest of the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderGeom {
    pub active: Rect,
    pub handle: Rect,
    pub rest: Rect,
}

/// Lay a slider out in `rect`, `t` (0..1) of the way along.
pub fn slider_geom(rect: Rect, t: f32) -> SliderGeom {
    const TRACK: f32 = 16.0;
    const HANDLE: f32 = 4.0;
    const GAP: f32 = 6.0;
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let x = egui::lerp(rect.left() + HANDLE / 2.0 + GAP..=rect.right() - HANDLE / 2.0 - GAP, t);
    let (top, bottom) = (rect.center().y - TRACK / 2.0, rect.center().y + TRACK / 2.0);
    SliderGeom {
        active: Rect::from_min_max(pos2(rect.left(), top), pos2(x - HANDLE / 2.0 - GAP, bottom)),
        handle: Rect::from_center_size(pos2(x, rect.center().y), vec2(HANDLE, rect.height().min(44.0))),
        rest: Rect::from_min_max(pos2(x + HANDLE / 2.0 + GAP, top), pos2(rect.right(), bottom)),
    }
}

/// The whole number between `lo` and `hi` a finger at `x` on a slider in `rect` sets.
pub fn slider_value(rect: Rect, x: f32, lo: u32, hi: u32) -> u32 {
    let t = ((x - rect.left() - 8.0) / (rect.width() - 16.0).max(1.0)).clamp(0.0, 1.0);
    (lo as f32 + t * hi.saturating_sub(lo) as f32).round() as u32
}

/// A slider across the layout's width for a whole number in `range`: tap or drag to set.
pub fn slider(ui: &mut Ui, value: &mut u32, range: std::ops::RangeInclusive<u32>, name: &str) -> Response {
    let m = material::scheme();
    let (lo, hi) = (*range.start(), *range.end());
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::click_and_drag());
    let old = *value;
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
    {
        *value = slider_value(rect, p.x, lo, hi);
    }
    if *value != old {
        resp.mark_changed();
    }
    let shown = *value;
    resp.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), shown as f64, name));
    let g = slider_geom(rect, (shown.saturating_sub(lo)) as f32 / hi.saturating_sub(lo).max(1) as f32);
    let p = ui.painter();
    let look = material::style();
    if look.outlined() {
        // Sticker: one outlined track, filled with the accent up to an outlined handle.
        let line = Stroke::new(look.outline, look.line);
        let track = Rect::from_min_max(pos2(g.active.left(), g.active.center().y - 7.0), pos2(g.rest.right().max(g.active.right()), g.active.center().y + 7.0));
        p.rect_filled(track, 7.0, m.surface_container);
        p.rect_filled(Rect::from_min_max(track.min, pos2(g.handle.center().x, track.bottom())), 7.0, look.accent);
        p.rect_stroke(track, 7.0, line, StrokeKind::Inside);
        p.rect_filled(g.handle.expand2(vec2(4.0, 0.0)), 6.0, m.secondary_container);
        p.rect_stroke(g.handle.expand2(vec2(4.0, 0.0)), 6.0, line, StrokeKind::Inside);
        return resp;
    }
    if g.active.width() > 0.0 {
        p.rect_filled(g.active, CornerRadius { nw: 8, sw: 8, ne: 2, se: 2 }, m.primary);
    }
    if g.rest.width() > 0.0 {
        p.rect_filled(g.rest, CornerRadius { nw: 2, sw: 2, ne: 8, se: 8 }, m.secondary_container);
        p.circle_filled(pos2(g.rest.right() - 8.0, g.rest.center().y), 2.0, m.primary);
    }
    p.rect_filled(g.handle, 2.0, m.primary);
    resp
}

/// A value between a "less" and a "more" button (named `names`), each there to tap
/// while the value can still go that way. Returns -1 or 1 when one was tapped.
pub fn stepper(ui: &mut Ui, text: &str, less: bool, more: bool, names: (&str, &str)) -> i32 {
    let m = material::scheme();
    let (rect, _) = ui.allocate_exact_size(vec2(TOUCH + 76.0 + TOUCH, TOUCH), Sense::hover());
    let mut step = 0;
    let id = ui.scope_id().with(("stepper", names.0, rect.min.y as i32));
    for (enabled, name, x, draw, by) in [(less, names.0, rect.left() + TOUCH / 2.0, drawn::minus as fn(&Ui, Pos2, f32, Color32), -1), (more, names.1, rect.right() - TOUCH / 2.0, drawn::plus, 1)] {
        let center = pos2(x, rect.center().y);
        if enabled {
            if IconButton::new(Icon::Drawn(draw), name).style(IconStyle::Surface).show_at(ui, center, id.with(by)).clicked() {
                step = by;
            }
        } else {
            ui.painter().circle_filled(center, 20.0, m.surface_container_high.gamma_multiply(0.5));
            draw(ui, center, 24.0, m.on_surface_variant.gamma_multiply(0.38));
        }
    }
    let g = galley(ui, text, Type::TitleSmall, m.on_surface, 76.0);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, m.on_surface);
    step
}

/// A dropdown's closed state: the chosen value and a chevron, on a tonal field.
pub fn dropdown(ui: &mut Ui, value: &str, width: f32, name: &str) -> Response {
    let m = material::scheme();
    let (rect, resp) = ui.allocate_exact_size(vec2(width.min(ui.available_width()), TOUCH), Sense::click());
    let resp = named(resp, name);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    if material::style().outlined() {
        plate(ui, rect, shape::TILE_SMALL, m.surface_container, Stack::Card);
    } else {
        ui.painter().rect_filled(rect, shape::TILE_SMALL, m.surface_container_highest);
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, shape::TILE_SMALL, m.on_surface.gamma_multiply(PRESSED));
    }
    let g = galley(ui, value, Type::BodyLarge, m.on_surface, rect.width() - 16.0 - 40.0);
    ui.painter().galley(pos2(rect.left() + 16.0, rect.center().y - g.size().y / 2.0), g, m.on_surface);
    widgets::icon_at(ui, pos2(rect.right() - 20.0, rect.center().y), if open { icon::CHEVRON_UP } else { icon::CHEVRON_DOWN }, 18.0, m.on_surface_variant);
    resp
}

// ------------------------------------------------------------------ wavy progress

/// The wave's stroke, how far it swings either way, and the length of one swing.
const WAVE_THICK: f32 = 4.0;
const WAVE_AMP: f32 = 3.0;
const WAVE_LEN: f32 = 22.0;
/// The thumb's width, and the room kept clear on each side of it.
const THUMB_W: f32 = 4.0;
const THUMB_GAP: f32 = 6.0;

/// Where the parts of a wavy progress indicator are: the wavy played part, the upright
/// thumb, the flat remainder. A part with no room (`from >= to`) is not drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveGeom {
    pub wave: (f32, f32),
    pub thumb: Rect,
    pub rest: (f32, f32),
    /// The line the wave swings about and the remainder lies on.
    pub y: f32,
}

/// Lay the indicator out in `rect` at `fraction` (0..1) played.
pub fn wave_geom(rect: Rect, fraction: f32) -> WaveGeom {
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let x = egui::lerp(rect.left() + THUMB_W / 2.0..=rect.right() - THUMB_W / 2.0, f);
    let y = rect.center().y;
    WaveGeom {
        wave: (rect.left() + WAVE_THICK / 2.0, x - THUMB_W / 2.0 - THUMB_GAP),
        thumb: Rect::from_center_size(pos2(x, y), vec2(THUMB_W, rect.height())),
        rest: (x + THUMB_W / 2.0 + THUMB_GAP, rect.right() - WAVE_THICK / 2.0),
        y,
    }
}

/// The wave from `from` to `to` about the line `y`, swinging `amp` either way; `phase`
/// (radians) slides it along, which is how it moves while a song plays.
pub fn wave_points(from: f32, to: f32, y: f32, amp: f32, phase: f32) -> Vec<Pos2> {
    if to <= from {
        return Vec::new();
    }
    let steps = ((to - from) / 1.5).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|i| {
            let x = from + (to - from) * i as f32 / steps as f32;
            pos2(x, y + amp * ((x - from) / WAVE_LEN * std::f32::consts::TAU + phase).sin())
        })
        .collect()
}

/// The wavy progress indicator in `rect`: `fraction` played, the wave slid along by
/// `phase` and flattened to a line as `wave` goes from 1 to 0 (a paused song lies
/// still). Now Playing, the widgets and the downloads list draw theirs with it.
pub fn wavy_progress(ui: &Ui, rect: Rect, fraction: f32, phase: f32, wave: f32) {
    let m = material::scheme();
    let g = wave_geom(rect, fraction);
    let p = ui.painter();
    if g.rest.0 < g.rest.1 {
        p.rect_filled(Rect::from_min_max(pos2(g.rest.0 - WAVE_THICK / 2.0, g.y - WAVE_THICK / 2.0), pos2(g.rest.1 + WAVE_THICK / 2.0, g.y + WAVE_THICK / 2.0)), WAVE_THICK / 2.0, m.secondary_container);
    }
    let points = wave_points(g.wave.0, g.wave.1, g.y, WAVE_AMP * wave.clamp(0.0, 1.0), phase);
    if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) {
        p.circle_filled(first, WAVE_THICK / 2.0, m.primary);
        p.circle_filled(last, WAVE_THICK / 2.0, m.primary);
        p.add(egui::Shape::line(points, Stroke::new(WAVE_THICK, m.primary)));
    }
    p.rect_filled(g.thumb, THUMB_W / 2.0, m.primary);
}

/// How far along (0..1) a finger at `x` is on the indicator in `rect`: where
/// `wave_geom` puts the thumb, read the other way, so a held thumb stays under the finger.
pub fn wave_fraction(rect: Rect, x: f32) -> f32 {
    let (from, to) = (rect.left() + THUMB_W / 2.0, rect.right() - THUMB_W / 2.0);
    if to <= from { 0.0 } else { ((x - from) / (to - from)).clamp(0.0, 1.0) }
}

/// Where a finger takes hold of the indicator in `rect` to seek: its whole length, a
/// finger tall.
pub fn wave_target(rect: Rect) -> Rect {
    Rect::from_center_size(rect.center(), vec2(rect.width(), rect.height().max(TOUCH)))
}

// ------------------------------------------------------------------ tool chip, option pill

/// How tall a tool chip is drawn (its touch target is `TOUCH`).
pub const TOOL_H: f32 = 40.0;

/// How wide `tool_chip` is with `label`.
pub fn tool_chip_width(ui: &Ui, label: &str) -> f32 {
    16.0 + 20.0 + 8.0 + galley(ui, label, Type::LabelLarge, Color32::PLACEHOLDER, 200.0).size().x + 18.0
}

/// How wide `tool_chip_icon` is: a finger across.
pub const TOOL_ICON_W: f32 = TOUCH;

/// The container of a tool chip in `rect` and the colour its content wears.
fn tool_chip_body(ui: &Ui, rect: Rect, label: &str, on: bool, id: egui::Id) -> (Response, Rect, Color32) {
    let m = material::scheme();
    let resp = named(ui.interact(rect, id, Sense::click()), label);
    let body = Rect::from_center_size(rect.center(), vec2(rect.width(), TOOL_H));
    if material::style().outlined() {
        // Sticker: a small card; at work it is sky blue.
        plate(ui, body, round(TOOL_H / 2.0), if on { m.secondary_container } else { m.surface_container }, Stack::None);
    } else {
        ui.painter().rect_filled(body, TOOL_H / 2.0, m.glass(0.1));
    }
    let ink = if material::style().outlined() { if on { m.on_secondary_container } else { m.on_surface } } else if on { m.primary } else { m.on_surface };
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(body, TOOL_H / 2.0, ink.gamma_multiply(PRESSED));
    }
    (resp, body, ink)
}

/// A tool of the player (Lyrics, Sleep timer): an icon and a label on a round-ended
/// tonal container, drawn `TOOL_H` tall in the middle of `rect`. `on`: it is at work
/// (a timer runs), and wears the primary colour.
pub fn tool_chip(ui: &Ui, rect: Rect, icon: Icon, label: &str, on: bool, id: egui::Id) -> Response {
    let m = material::scheme();
    let (resp, body, ink) = tool_chip_body(ui, rect, label, on, id);
    paint_icon(ui, pos2(body.left() + 16.0 + 10.0, body.center().y), 20.0, icon, if on { m.primary } else { m.on_surface_variant });
    let g = galley(ui, label, Type::LabelLarge, ink, (body.width() - 62.0).max(0.0));
    ui.painter().galley(pos2(body.left() + 44.0, body.center().y - g.size().y / 2.0), g, ink);
    resp
}

/// A tool chip with its icon alone, for a row with no room for the label (`name` is
/// what a screen reader says): the same container, the icon in its middle.
pub fn tool_chip_icon(ui: &Ui, rect: Rect, icon: Icon, name: &str, on: bool, id: egui::Id) -> Response {
    let m = material::scheme();
    let (resp, body, _) = tool_chip_body(ui, rect, name, on, id);
    paint_icon(ui, body.center(), 20.0, icon, if on { m.primary } else { m.on_surface_variant });
    resp
}

/// How tall a sheet's option pills and its main button are.
pub const OPTION_H: f32 = 56.0;

/// One of a sheet's options in `rect`. Chosen (`on`) it is filled with the primary
/// colour and its corners are squarer; otherwise it is a round-ended tonal container,
/// or only an outline when `outlined` (a secondary option). True when tapped.
pub fn option_pill(ui: &Ui, rect: Rect, id: egui::Id, label: &str, icon: Option<Icon>, on: bool, outlined: bool) -> bool {
    let m = material::scheme();
    let resp = named(ui.interact(rect, id, Sense::click()), label);
    let t = theme::anim_bool(ui.ctx(), id.with("on"), on, 0.15);
    let lined = material::style().outlined();
    let radius = if lined { round(shape::TILE) } else { egui::lerp(rect.height() / 2.0..=shape::TILE, t) };
    let ink = if lined {
        // A lined look: every option is a card, and the one chosen is the tonal colour.
        plate(ui, rect, radius, theme::lerp_color(m.surface_container, m.secondary_container, t), Stack::None);
        theme::lerp_color(m.on_surface, m.on_secondary_container, t)
    } else {
        let off = if outlined { Color32::TRANSPARENT } else { m.surface_container_highest };
        ui.painter().rect_filled(rect, radius, theme::lerp_color(off, m.primary, t));
        if outlined && t < 1.0 {
            ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, m.outline.gamma_multiply(1.0 - t)), StrokeKind::Inside);
        }
        theme::lerp_color(m.on_surface, m.on_primary, t)
    };
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, radius, ink.gamma_multiply(PRESSED));
    }
    let lead = if icon.is_some() { 26.0 } else { 0.0 };
    let room = (rect.width() - 20.0 - lead).max(0.0);
    // A label too long for a narrow pill (two side by side on a small phone) is set smaller
    // before it is cut short.
    let fits = galley(ui, label, Type::TitleMedium, ink, f32::INFINITY).size().x <= room;
    let g = galley(ui, label, if fits { Type::TitleMedium } else { Type::LabelLarge }, ink, room);
    let left = rect.center().x - (lead + g.size().x) / 2.0;
    if let Some(icon) = icon {
        paint_icon(ui, pos2(left + 9.0, rect.center().y), 20.0, icon, ink);
    }
    ui.painter().galley(pos2(left + lead, rect.center().y - g.size().y / 2.0), g, ink);
    resp.clicked()
}

/// A sheet's heading: its title over one line that says what it is for.
pub fn sheet_header(ui: &mut Ui, title: &str, note: &str) {
    let m = material::scheme();
    let w = ui.available_width();
    let (head, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
    text(ui, pos2(head.left() + 8.0, head.top()), title, Type::HeadlineSmall, m.on_surface, w - 16.0);
    text(ui, pos2(head.left() + 8.0, head.top() + 36.0), note, Type::BodyMedium, m.on_surface_variant, w - 16.0);
}

// ------------------------------------------------------------------ menu

/// A menu item's height.
pub const MENU_ITEM_H: f32 = 48.0;
/// How large a menu item's leading icon is drawn.
pub const MENU_ICON: f32 = 20.0;

/// One item of a menu: `draw` paints its leading icon at the point and in the colour
/// it is given, and the label follows. The menu closes when it is tapped.
pub fn menu_item(ui: &mut Ui, label: &str, draw: impl FnOnce(&Ui, Pos2, Color32)) -> Response {
    let m = material::scheme();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), MENU_ITEM_H), Sense::click());
    let resp = named(resp, label);
    if resp.is_pointer_button_down_on() || resp.hovered() {
        ui.painter().rect_filled(rect, shape::CHIP, m.on_surface.gamma_multiply(PRESSED));
    }
    draw(ui, pos2(rect.left() + 12.0 + 12.0, rect.center().y), m.on_surface_variant);
    let g = galley(ui, label, Type::BodyLarge, m.on_surface, rect.width() - 48.0 - 12.0);
    ui.painter().galley(pos2(rect.left() + 48.0, rect.center().y - g.size().y / 2.0), g, m.on_surface);
    if resp.clicked() {
        ui.close();
    }
    resp
}

/// The line between two groups of a menu's items.
pub fn menu_divider(ui: &mut Ui) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, material::scheme().outline_variant);
    ui.add_space(4.0);
}

// ------------------------------------------------------------------ carousel

/// A carousel's height, and the room between its items.
pub const CAROUSEL_H: f32 = 132.0;
pub const CAROUSEL_GAP: f32 = 8.0;

/// One item of a carousel as it is drawn: which, where from the carousel's left, how wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub index: usize,
    pub x: f32,
    pub width: f32,
}

/// The widths of a carousel's large, medium and small items in a row `width` wide:
/// together with their gaps they fill it.
pub fn carousel_sizes(width: f32) -> [f32; 3] {
    let small = TOUCH;
    let medium = (width * 0.295).round().max(small);
    let large = (width - medium - small - 2.0 * CAROUSEL_GAP).max(medium);
    [large, medium, small]
}

/// How wide the item `places` after the leading one is. The leading item is large, the
/// next medium, the ones after small; an item on its way out at the left shrinks to nothing.
fn slot_width([large, medium, small]: [f32; 3], places: f32) -> f32 {
    match places {
        r if r <= -1.0 => 0.0,
        r if r < 0.0 => large * (1.0 + r),
        r if r < 1.0 => egui::lerp(large..=medium, r),
        r if r < 2.0 => egui::lerp(medium..=small, r - 1.0),
        _ => small,
    }
}

/// The items on screen in a carousel `width` wide with `count` items, scrolled `lead`
/// items along (a fraction while it moves): the item at `lead` is the large one. The
/// last item reaches the right edge, so the end of the row leaves no hole.
pub fn carousel_slots(width: f32, count: usize, lead: f32) -> Vec<Slot> {
    let sizes = carousel_sizes(width);
    let lead = lead.clamp(0.0, count.saturating_sub(1) as f32);
    let mut slots = Vec::new();
    let mut x = 0.0;
    for index in lead.floor() as usize..count {
        let room = width - x;
        if room <= 0.0 {
            break;
        }
        let natural = slot_width(sizes, index as f32 - lead);
        let w = if index + 1 == count { room } else { natural.min(room) };
        if w > 0.0 {
            slots.push(Slot { index, x, width: w });
        }
        // An item nearly gone takes its gap with it.
        x += natural + CAROUSEL_GAP * (natural / sizes[2]).min(1.0);
    }
    slots
}

/// How far the carousel scrolls for one item: two items make a page of it.
pub fn carousel_pitch(width: f32) -> f32 {
    (width + CAROUSEL_GAP) / 2.02
}

/// A darkening that grows from nothing at `top` to `strength` at the bottom of the
/// rounded rect `rect`, for a label over a picture.
pub fn scrim_bottom(ui: &Ui, rect: Rect, radius: f32, top: f32, strength: f32) {
    let radius = radius.min(rect.width() / 2.0).min(rect.bottom() - top);
    let alpha = |y: f32| Color32::from_black_alpha((255.0 * strength * ((y - top) / (rect.bottom() - top)).clamp(0.0, 1.0)) as u8);
    // The outline: across the top, down the right side, round the two bottom corners, up the left.
    let mut outline = vec![pos2(rect.left(), top), pos2(rect.right(), top)];
    let arc = |outline: &mut Vec<Pos2>, c: Pos2, from: f32| {
        for i in 0..=8 {
            let a = from + i as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
            outline.push(c + radius * vec2(a.cos(), a.sin()));
        }
    };
    arc(&mut outline, pos2(rect.right() - radius, rect.bottom() - radius), 0.0);
    arc(&mut outline, pos2(rect.left() + radius, rect.bottom() - radius), std::f32::consts::FRAC_PI_2);
    // A fan: the darkening is linear down the picture, so every triangle carries it exactly.
    let center = pos2(rect.center().x, (top + rect.bottom()) / 2.0);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, alpha(center.y));
    for p in &outline {
        mesh.colored_vertex(*p, alpha(p.y));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A shelf of cards as a Material carousel: a large item, a medium and a small one,
/// which take each other's place as the shelf turns. The large one carries its title.
/// A tap opens a card; a long press brings its menu.
pub fn carousel(ui: &mut Ui, id: &str, nudge: i32, cards: &[Card], liked: &HashSet<String>, acts: &mut Vec<Action>) {
    let width = ui.available_width();
    let left = ui.cursor().left();
    let pitch = carousel_pitch(width);
    let [large, medium, _] = carousel_sizes(width);
    widgets::h_scroll(ui, id, nudge, cards.len(), pitch - CAROUSEL_GAP, CAROUSEL_GAP, |ui| {
        // The shelf scrolls under its window; the items are placed in the window itself.
        let lead = (left - ui.cursor().left()) / pitch;
        let (band, _) = ui.allocate_exact_size(vec2(1.0, CAROUSEL_H), Sense::hover());
        let window = Rect::from_min_size(pos2(left, band.top()), vec2(width, CAROUSEL_H));
        let mut view = ui.new_child(egui::UiBuilder::new().max_rect(window));
        view.set_clip_rect(window.intersect(ui.clip_rect()));
        for slot in carousel_slots(width, cards.len(), lead) {
            let c = &cards[slot.index];
            let rect = Rect::from_min_size(pos2(left + slot.x, band.top()), vec2(slot.width, CAROUSEL_H));
            let focus = ((slot.width - medium) / (large - medium).max(1.0)).clamp(0.0, 1.0);
            carousel_item(&mut view, egui::Id::unique(("carousel", id, slot.index)), rect, c, focus, liked, acts);
        }
    });
}

/// One carousel item in `rect`; `focus` is how far it is the large one (0..1), which
/// is how clearly its label shows.
fn carousel_item(ui: &mut Ui, id: egui::Id, rect: Rect, c: &Card, focus: f32, liked: &HashSet<String>, acts: &mut Vec<Action>) {
    let resp = named(ui.interact(rect, id, Sense::click()), &c.title);
    let radius = round(shape::CARD).min(rect.width() / 2.0);
    widgets::paint_cover_cropped(ui, rect, &c.thumbs, CornerRadius::same(radius as u8));
    outline(ui, rect, radius);
    if focus > 0.01 {
        scrim_bottom(ui, rect, radius, rect.top() + rect.height() * 0.35, 0.75 * focus);
        let ink = Color32::WHITE.gamma_multiply(focus);
        let width = rect.width() - 32.0;
        let bottom = rect.bottom() - 14.0;
        let sub = galley(ui, &c.subtitle, Type::BodyMedium, ink, width);
        let title = galley(ui, &c.title, Type::TitleMedium, ink, width);
        let sub_top = bottom - sub.size().y;
        let title_top = sub_top - title.size().y;
        ui.painter().galley(pos2(rect.left() + 16.0, title_top), title, ink);
        ui.painter().galley(pos2(rect.left() + 16.0, sub_top), sub, ink);
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, radius, Color32::WHITE.gamma_multiply(PRESSED));
    }
    let is_liked = matches!(&c.target, Target::Song(t) if liked.contains(&t.id));
    egui::Popup::context_menu(&resp).show(|ui| widgets::card_menu_contents(ui, c, is_liked, acts));
    if resp.clicked() {
        acts.push(Action::Open(c.target.clone()));
    }
}

// ------------------------------------------------------------------ home

/// A song tile's height (Quick picks), and the room between two tiles.
pub const TILE_H: f32 = 56.0;
pub const TILE_GAP: f32 = 8.0;

/// How wide a tile is when two share a row `width` wide: a hair under half, so the
/// shelf counts two columns to a page whatever the width rounds to.
pub fn tile_width(width: f32) -> f32 {
    (width - TILE_GAP) / 2.0 - 0.01
}

/// A song as a tile: its cover flush in the left end, its title and artist beside it.
/// The song playing has its title in the primary colour and the bars over its cover.
pub fn song_tile(ui: &mut Ui, width: f32, t: &Track, o: &widgets::RowOpts, acts: &mut Vec<Action>) -> Response {
    let m = material::scheme();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, TILE_H), Sense::click());
    let resp = named(resp, &t.title);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let r = round(shape::TILE) as u8;
    plate(ui, rect, r, if material::style().outlined() { m.surface_container } else { m.surface_container_high }, Stack::None);
    let cover = Rect::from_min_size(rect.min, Vec2::splat(TILE_H));
    let corners = CornerRadius { nw: r, sw: r, ne: 0, se: 0 };
    widgets::paint_cover_cropped(ui, cover, &t.thumbs, corners);
    if o.current {
        ui.painter().rect_filled(cover, corners, theme::shade(0.5));
        widgets::equalizer(ui, cover.shrink(18.0), o.time, o.playing, m.primary);
    }
    let left = cover.right() + 12.0;
    let title = if o.current { m.primary } else { m.on_surface };
    two_lines(ui, left, rect.center().y, rect.right() - 12.0 - left, &t.title, Type::TitleSmall, title, &t.artist_line(), Type::BodySmall, m.on_surface_variant);
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, shape::TILE, m.on_surface.gamma_multiply(PRESSED));
    }
    egui::Popup::context_menu(&resp).show(|ui| widgets::song_menu(ui, t, o.liked, o.remove, o.queue_move, acts));
    resp
}

/// The hero card's height.
pub const HERO_H: f32 = 184.0;

/// Where the hero card's parts are in `rect`: the cover at its right, the text column
/// at its left, the play button under the text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroGeom {
    pub cover: Rect,
    /// The column the overline and the title are set in.
    pub text: Rect,
    /// The play button's container.
    pub play: Rect,
    /// The lines beside the play button.
    pub note: Rect,
}

pub fn hero_geom(rect: Rect) -> HeroGeom {
    let pad = 24.0;
    let side = (rect.height() - 2.0 * 28.0).min(rect.width() * 0.34);
    let cover = Rect::from_min_size(pos2(rect.right() - pad - side, rect.center().y - side / 2.0), Vec2::splat(side));
    let play = Rect::from_min_size(pos2(rect.left() + pad, rect.bottom() - 16.0 - TOUCH), Vec2::splat(TOUCH));
    let text_right = cover.left() - 12.0;
    HeroGeom {
        cover,
        text: Rect::from_min_max(pos2(rect.left() + pad, rect.top() + 20.0), pos2(text_right, play.top() - 8.0)),
        play,
        note: Rect::from_min_max(pos2(play.right() + 12.0, play.top()), pos2(text_right, play.bottom())),
    }
}

/// A card's subtitle ("Dadang R. • 48 songs") as the hero's two lines beside its
/// button: what comes first, then the rest.
pub fn hero_note(subtitle: &str) -> (&str, String) {
    let mut parts = subtitle.split('\u{2022}').map(str::trim).filter(|p| !p.is_empty());
    let lead = parts.next().unwrap_or("");
    (lead, parts.collect::<Vec<_>>().join(" \u{2022} "))
}

/// The hero card: one album or playlist of the feed, large. `overline` says where in
/// the feed it is from. Its button plays it (or pauses it once it is what plays); a
/// tap anywhere else opens it.
pub fn hero_card(ui: &mut Ui, overline: &str, c: &Card, acts: &mut Vec<Action>) {
    let m = material::scheme();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), HERO_H), Sense::click());
    let resp = named(resp, &c.title);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let g = hero_geom(rect);
    plate(ui, rect, round(shape::CARD), m.primary_container, Stack::Card);
    // A paler disc behind the cover; it leaves the card through its straight edges only.
    let disc = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    disc.circle_filled(pos2(rect.right() - 150.0, rect.center().y), 140.0, m.on_primary_container.gamma_multiply(0.09));
    if material::style().outlined() {
        widgets::paint_cover(ui, g.cover, &c.thumbs, round(24.0), 1.0);
    } else {
        widgets::soft_shadow(ui, g.cover, 24.0, 18.0, 0.35);
        widgets::paint_cover(ui, g.cover, &c.thumbs, 24.0, 1.0);
        ui.painter().rect_stroke(g.cover, 24.0, Stroke::new(2.0, m.primary_container), StrokeKind::Outside);
    }

    let ink = m.on_primary_container;
    let over = text(ui, g.text.min, &overline.to_uppercase(), Type::LabelMedium, ink.gamma_multiply(0.8), g.text.width());
    let title_role = Type::HeadlineMedium.spec().weight(Weight::Medium);
    let rows = (((g.text.bottom() - over.bottom() - 2.0) / title_role.line_height).floor() as usize).clamp(1, 2);
    let title = galley_wrapped(ui, &c.title, title_role, ink, g.text.width(), rows);
    ui.painter().galley(pos2(g.text.left(), over.bottom() + 2.0), title, ink);
    let (lead, rest) = hero_note(&c.subtitle);
    if rest.is_empty() {
        let note = galley(ui, lead, Type::TitleSmall, ink, g.note.width());
        ui.painter().galley(pos2(g.note.left(), g.note.center().y - note.size().y / 2.0), note, ink);
    } else {
        two_lines(ui, g.note.left(), g.note.center().y, g.note.width(), lead, Type::TitleSmall, ink, &rest, Type::BodySmall, ink.gamma_multiply(0.85));
    }

    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, shape::CARD, ink.gamma_multiply(PRESSED * 0.6));
    }
    let active = widgets::active_state(ui, &c.target);
    let playing = active == Some(true);
    let name = format!("{} {}", if playing { "Pause" } else { "Play" }, c.title);
    let glyph = if playing { glyphs::PAUSE } else { glyphs::PLAY };
    let play = IconButton::new(glyph, &name).style(IconStyle::Filled).size(TOUCH).radius(shape::TILE).show_at(ui, g.play.center(), resp.id.with("play"));
    if play.clicked() {
        acts.push(match &c.target {
            _ if active.is_some() => Action::TogglePlay,
            Target::Song(t) => Action::Radio(t.clone()),
            t => Action::PlayCollection(t.clone(), false),
        });
    } else if resp.clicked() {
        acts.push(Action::Open(c.target.clone()));
    }
    egui::Popup::context_menu(&resp).show(|ui| widgets::card_menu_contents(ui, c, false, acts));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The widths a phone's page has, between its 16-point margins, from a 360-point
    /// screen to a 412-point one.
    const WIDTHS: [f32; 4] = [328.0, 344.0, 361.0, 380.0];

    fn apart(a: Rect, b: Rect) -> bool {
        !a.intersects(b) || a.intersect(b).width() <= 0.0 || a.intersect(b).height() <= 0.0
    }

    // -------------------------------------------------------------- carousel

    #[test]
    fn a_carousel_at_rest_is_a_large_a_medium_and_a_small_item_filling_the_row() {
        for width in WIDTHS {
            let slots = carousel_slots(width, 9, 0.0);
            assert_eq!(slots.iter().map(|s| s.index).collect::<Vec<_>>(), [0, 1, 2], "{width}");
            assert!(slots[0].width > slots[1].width && slots[1].width > slots[2].width, "{slots:?}");
            assert_eq!(slots[0].x, 0.0);
            let end = slots[2].x + slots[2].width;
            assert!((end - width).abs() < 0.01, "{end} of {width}");
        }
    }

    #[test]
    fn the_carousels_sizes_are_the_designs_on_a_412_point_phone() {
        assert_eq!(carousel_sizes(380.0), [204.0, 112.0, 48.0]);
    }

    #[test]
    fn every_carousel_item_at_rest_is_wide_enough_for_a_finger() {
        for width in WIDTHS {
            for lead in 0..7 {
                for s in carousel_slots(width, 9, lead as f32) {
                    assert!(s.width >= TOUCH - 0.01, "{width} at {lead}: {s:?}");
                }
            }
        }
    }

    #[test]
    fn carousel_items_keep_their_order_and_never_overlap_while_it_turns() {
        for width in WIDTHS {
            for step in 0..=80 {
                let lead = step as f32 / 10.0;
                let slots = carousel_slots(width, 9, lead);
                assert!(!slots.is_empty());
                for pair in slots.windows(2) {
                    assert!(pair[0].index + 1 == pair[1].index, "{pair:?}");
                    assert!(pair[0].x + pair[0].width <= pair[1].x + 0.01, "{width} at {lead}: {pair:?}");
                }
                let last = slots.last().unwrap();
                assert!(last.x + last.width <= width + 0.01, "{width} at {lead}: {last:?}");
                assert!(slots[0].x >= 0.0);
            }
        }
    }

    #[test]
    fn turning_one_item_makes_the_next_one_the_large_one() {
        let rest = carousel_slots(380.0, 9, 0.0);
        let turned = carousel_slots(380.0, 9, 1.0);
        assert_eq!(turned[0].index, 1);
        assert_eq!(turned.iter().map(|s| s.width).collect::<Vec<_>>(), rest.iter().map(|s| s.width).collect::<Vec<_>>());
    }

    #[test]
    fn halfway_through_a_turn_the_items_are_between_their_sizes() {
        let [large, medium, small] = carousel_sizes(380.0);
        let slots = carousel_slots(380.0, 9, 0.5);
        assert_eq!(slots[0].index, 0);
        assert!((slots[0].width - large / 2.0).abs() < 0.01, "the one leaving is half gone");
        assert!((slots[1].width - (large + medium) / 2.0).abs() < 0.01);
        assert!((slots[2].width - (medium + small) / 2.0).abs() < 0.01);
        assert_eq!(slots[3].index, 3, "the next one is coming in at the right");
    }

    #[test]
    fn the_last_item_reaches_the_right_edge_so_the_end_leaves_no_hole() {
        for width in WIDTHS {
            // Two items a page: the shelf's last page starts two items before its end.
            let slots = carousel_slots(width, 9, 7.0);
            assert_eq!(slots.iter().map(|s| s.index).collect::<Vec<_>>(), [7, 8]);
            assert!((slots[1].x + slots[1].width - width).abs() < 0.01);
        }
        let alone = carousel_slots(380.0, 1, 0.0);
        assert_eq!(alone, [Slot { index: 0, x: 0.0, width: 380.0 }]);
        assert!(carousel_slots(380.0, 0, 0.0).is_empty());
    }

    #[test]
    fn two_carousel_items_make_a_page_of_the_shelf() {
        for width in WIDTHS {
            let pitch = carousel_pitch(width);
            assert_eq!(((width + CAROUSEL_GAP) / pitch).floor(), 2.0, "{width}");
            // Also when the shelf measures itself a point narrower or wider than the page.
            assert_eq!(((width - 1.0 + CAROUSEL_GAP) / pitch).floor(), 2.0, "{width}");
            assert_eq!(((width + 1.0 + CAROUSEL_GAP) / pitch).floor(), 2.0, "{width}");
        }
    }

    // -------------------------------------------------------------- wavy progress

    fn bar() -> Rect {
        Rect::from_min_size(pos2(31.0, 545.0), vec2(350.0, 22.0))
    }

    #[test]
    fn the_wave_the_thumb_and_the_remainder_follow_each_other_with_room_between() {
        for f in [0.1, 0.35, 0.5, 0.8] {
            let g = wave_geom(bar(), f);
            assert!(g.wave.0 < g.wave.1 && g.rest.0 < g.rest.1, "{f}: {g:?}");
            assert!(g.wave.1 + WAVE_THICK / 2.0 < g.thumb.left(), "the wave's round end stops short of the thumb");
            assert!(g.thumb.right() < g.rest.0 - WAVE_THICK / 2.0, "and the remainder starts clear of it");
            assert!(bar().contains_rect(g.thumb));
        }
    }

    #[test]
    fn a_finger_on_the_thumb_reads_back_what_has_played() {
        for f in [0.0, 0.1, 0.35, 0.5, 0.8, 1.0] {
            let x = wave_geom(bar(), f).thumb.center().x;
            assert!((wave_fraction(bar(), x) - f).abs() < 1e-4, "{f}");
        }
        assert_eq!(wave_fraction(bar(), bar().left() - 40.0), 0.0, "left of the bar is the start");
        assert_eq!(wave_fraction(bar(), bar().right() + 40.0), 1.0, "right of it is the end");
        assert_eq!(wave_fraction(Rect::from_min_size(pos2(0.0, 0.0), vec2(2.0, 22.0)), 1.0), 0.0, "a bar with no room has no place to seek to");
        // Halfway along the thumb's run is half the song.
        assert!((wave_fraction(bar(), bar().center().x) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_seek_bar_is_held_in_a_fingers_height_along_its_whole_length() {
        let hit = wave_target(bar());
        assert_eq!(hit.height(), TOUCH);
        assert_eq!(hit.x_range(), bar().x_range());
        assert_eq!(hit.center(), bar().center());
    }

    #[test]
    fn the_thumb_is_upright_and_as_tall_as_the_indicator() {
        let g = wave_geom(bar(), 0.35);
        assert_eq!(g.thumb.height(), 22.0);
        assert!(g.thumb.width() < g.thumb.height() / 3.0);
        assert_eq!(g.y, bar().center().y);
    }

    #[test]
    fn the_thumb_moves_with_what_has_played_and_stays_inside() {
        let (start, mid, end) = (wave_geom(bar(), 0.0), wave_geom(bar(), 0.5), wave_geom(bar(), 1.0));
        assert_eq!(start.thumb.left(), bar().left());
        assert_eq!(end.thumb.right(), bar().right());
        assert!((mid.thumb.center().x - bar().center().x).abs() < 0.01);
        assert_eq!(wave_geom(bar(), 7.0), end, "more than all of it is all of it");
        assert_eq!(wave_geom(bar(), -1.0), start);
        assert_eq!(wave_geom(bar(), f32::NAN), start, "a song of unknown length has not started");
    }

    #[test]
    fn nothing_played_has_no_wave_and_all_played_has_no_remainder() {
        let start = wave_geom(bar(), 0.0);
        assert!(start.wave.0 >= start.wave.1);
        assert!(wave_points(start.wave.0, start.wave.1, start.y, WAVE_AMP, 0.0).is_empty());
        let end = wave_geom(bar(), 1.0);
        assert!(end.rest.0 >= end.rest.1);
    }

    #[test]
    fn the_wave_runs_left_to_right_within_its_swing() {
        let g = wave_geom(bar(), 0.6);
        let points = wave_points(g.wave.0, g.wave.1, g.y, WAVE_AMP, 1.0);
        assert_eq!(points.first().unwrap().x, g.wave.0);
        assert!((points.last().unwrap().x - g.wave.1).abs() < 0.01);
        assert!(points.windows(2).all(|w| w[1].x > w[0].x && w[1].x - w[0].x <= 1.6), "fine enough to look round");
        let (lo, hi) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        assert!(lo >= g.y - WAVE_AMP - 0.01 && hi <= g.y + WAVE_AMP + 0.01);
        assert!(hi - lo > WAVE_AMP * 1.8, "it does swing: {lo}..{hi}");
        // The stroke stays inside the indicator's height.
        assert!(g.y - WAVE_AMP - WAVE_THICK / 2.0 >= bar().top() && g.y + WAVE_AMP + WAVE_THICK / 2.0 <= bar().bottom());
    }

    #[test]
    fn a_wave_with_no_swing_is_a_flat_line_and_a_phase_slides_it() {
        let flat = wave_points(0.0, 100.0, 50.0, 0.0, 2.0);
        assert!(flat.iter().all(|p| p.y == 50.0));
        let (a, b) = (wave_points(0.0, 100.0, 50.0, 3.0, 0.0), wave_points(0.0, 100.0, 50.0, 3.0, 1.0));
        assert_ne!(a[5].y, b[5].y);
    }

    // -------------------------------------------------------------- list row

    #[test]
    fn a_list_rows_parts_do_not_overlap_and_its_button_fits_a_finger() {
        for width in WIDTHS {
            for dense in [false, true] {
                let (height, cover) = row_size(dense);
                let rect = Rect::from_min_size(pos2(16.0, 300.0), vec2(width, height));
                let g = row_geom(rect, cover, 36.0, true);
                let more = g.more.unwrap();
                assert!(more.width() >= TOUCH && more.height() >= TOUCH);
                assert!(rect.contains_rect(g.cover) && rect.expand(0.01).contains_rect(more.intersect(rect)));
                assert!(g.cover.right() < g.text.left());
                assert!(g.text.right() + 36.0 < more.left(), "the trailing text has its room");
                assert_eq!(g.trailing_right, more.left());
                assert!(g.text.width() > 120.0, "{width}: the text keeps a useful width");
                assert!(apart(g.cover, more));
            }
        }
    }

    #[test]
    fn a_list_row_without_a_button_or_trailing_text_gives_the_room_to_its_text() {
        let rect = Rect::from_min_size(pos2(16.0, 0.0), vec2(380.0, 72.0));
        let full = row_geom(rect, 56.0, 0.0, false);
        let busy = row_geom(rect, 56.0, 36.0, true);
        assert!(full.more.is_none());
        assert!(full.text.right() > busy.text.right() + 80.0);
        assert!(full.text.right() <= rect.right());
    }

    #[test]
    fn rows_are_tall_enough_for_a_finger() {
        assert!(row_size(true).0 >= TOUCH && row_size(false).0 >= TOUCH);
        assert!(row_size(true).1 < row_size(true).0 && row_size(false).1 < row_size(false).0);
    }

    // -------------------------------------------------------------- switch, tiles, hero

    #[test]
    fn the_switchs_handle_crosses_the_track_inside_it_and_grows() {
        let track = Rect::from_min_size(pos2(300.0, 100.0), SWITCH);
        let (off, r_off) = switch_handle(track, 0.0, false);
        let (on, r_on) = switch_handle(track, 1.0, false);
        assert!(off.x < on.x && off.y == track.center().y);
        assert!(r_on > r_off);
        for (c, r) in [(off, r_off), (on, r_on), switch_handle(track, 0.0, true), switch_handle(track, 1.0, true)] {
            assert!(c.x - r >= track.left() && c.x + r <= track.right(), "{c:?} {r}");
            assert!(r <= track.height() / 2.0);
        }
    }

    #[test]
    fn two_tiles_and_their_gap_fit_a_row_and_count_as_a_page() {
        for width in WIDTHS {
            let tile = tile_width(width);
            assert!(2.0 * tile + TILE_GAP <= width);
            assert_eq!(((width + TILE_GAP) / (tile + TILE_GAP)).floor(), 2.0, "{width}");
            assert_eq!(((width - 1.0 + TILE_GAP) / (tile + TILE_GAP)).floor(), 1.0, "a narrower shelf turns one column");
            assert!(tile - TILE_H - 24.0 > 70.0, "{width}: room for a title beside the cover");
        }
        assert!(TILE_H >= TOUCH);
    }

    #[test]
    fn the_hero_cards_parts_stay_inside_it_and_apart() {
        for width in WIDTHS {
            let rect = Rect::from_min_size(pos2(16.0, 150.0), vec2(width, HERO_H));
            let g = hero_geom(rect);
            for part in [g.cover, g.text, g.play, g.note] {
                assert!(rect.contains_rect(part), "{width}: {part:?}");
            }
            assert!(apart(g.text, g.cover) && apart(g.play, g.cover) && apart(g.note, g.cover));
            assert!(apart(g.text, g.play) && apart(g.play, g.note));
            assert!(g.play.width() >= TOUCH && g.play.height() >= TOUCH);
            assert!(g.text.width() >= 150.0, "{width}: {}", g.text.width());
            assert!(g.text.height() >= 16.0 + 2.0 + 2.0 * 36.0, "an overline and two lines of title: {}", g.text.height());
            assert!(g.note.width() >= 80.0, "{width}: {}", g.note.width());
        }
    }

    #[test]
    fn the_heros_note_is_the_subtitles_first_part_over_the_rest() {
        assert_eq!(hero_note("Dadang Rustiawan \u{2022} 18M views"), ("Dadang Rustiawan", "18M views".to_owned()));
        assert_eq!(hero_note("Playlist \u{2022} Arunika \u{2022} 48 songs"), ("Playlist", "Arunika \u{2022} 48 songs".to_owned()));
        assert_eq!(hero_note("Arunika"), ("Arunika", String::new()));
        assert_eq!(hero_note(""), ("", String::new()));
    }

    // -------------------------------------------------------------- drawn

    /// Draw `add` in a headless egui the width of a phone's page; what it returns.
    fn drawn<R>(width: f32, mut add: impl FnMut(&mut Ui) -> R) -> R {
        let ctx = egui::Context::default();
        let mut out = None;
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 800.0))), ..Default::default() };
            let run = ctx.run_ui(input, |ui| out = Some(add(ui)));
            run.drop_without_applying_deltas();
        }
        out.unwrap()
    }

    #[test]
    fn a_chip_is_drawn_32_tall_in_a_target_of_48_and_a_check_widens_it() {
        let (off, on) = drawn(360.0, |ui| ui.horizontal(|ui| (filter_chip(ui, "Relax", false).rect, filter_chip(ui, "Relax", true).rect)).inner);
        assert_eq!(off.height(), TOUCH);
        assert_eq!(on.height(), TOUCH);
        assert!(off.width() >= TOUCH);
        assert!((on.width() - off.width() - 18.0).abs() < 0.01, "the check and its gap: {} and {}", on.width(), off.width());
        let said = drawn(360.0, |ui| (filter_chip_size(ui, "Relax", false), filter_chip_size(ui, "Relax", true)));
        assert_eq!(said, (off.size(), on.size()), "a row that places chips itself is told the same");
        assert!(CHIP_H < TOUCH);
    }

    #[test]
    fn a_tool_chip_is_as_wide_as_it_says_and_a_menu_item_fits_a_finger() {
        let (said, chip, item) = drawn(360.0, |ui| {
            let w = tool_chip_width(ui, "Sleep timer");
            let (rect, _) = ui.allocate_exact_size(vec2(w, TOUCH), Sense::hover());
            (w, tool_chip(ui, rect, Icon::Font(icon::MUSIC), "Sleep timer", false, ui.id().with("tool")).rect, menu_item(ui, "Add to playlist", |_, _, _| {}).rect)
        });
        assert_eq!(chip.width(), said);
        assert!(said > 100.0 && said < 180.0, "an icon, the label and its padding: {said}");
        assert_eq!(chip.height(), TOUCH);
        assert!(TOOL_H < TOUCH);
        assert_eq!(item.height(), MENU_ITEM_H);
        assert!(MENU_ITEM_H >= TOUCH);
    }

    #[test]
    fn a_tool_chip_with_its_icon_alone_is_a_finger_across_and_narrower_than_one_with_a_label() {
        let (chip, with_label) = drawn(360.0, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(TOOL_ICON_W, TOUCH), Sense::hover());
            (tool_chip_icon(ui, rect, Icon::Font(icon::MUSIC), "Play on", false, ui.id().with("tool")).rect, tool_chip_width(ui, "Lyrics"))
        });
        assert_eq!(chip.size(), vec2(TOOL_ICON_W, TOUCH));
        assert!(TOOL_ICON_W >= TOUCH && TOOL_ICON_W < with_label, "{with_label}");
    }

    #[test]
    fn a_tap_on_an_option_pill_picks_it() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(16.0, 100.0), vec2(120.0, OPTION_H));
        let mut picked = false;
        let at = rect.center();
        let frames = [vec![], vec![egui::Event::PointerMoved(at)], vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]];
        for events in frames {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(360.0, 800.0))), events, ..Default::default() };
            let run = ctx.run_ui(input, |ui| picked |= option_pill(ui, rect, ui.id().with("opt"), "30 min", None, false, false));
            run.drop_without_applying_deltas();
        }
        assert!(picked);
        assert!(OPTION_H >= TOUCH);
    }

    #[test]
    fn buttons_are_hit_in_at_least_48_points() {
        let (filled, icon, big) = drawn(360.0, |ui| {
            (
                button(ui, "Play", Some(icon::PLAY), ButtonStyle::Filled).rect,
                IconButton::new(icon::SEARCH, "Search").show(ui).rect,
                IconButton::new(glyphs::PLAY, "Play").style(IconStyle::Filled).size(64.0).show(ui).rect,
            )
        });
        assert_eq!(filled.height(), TOUCH);
        assert!(filled.width() > TOUCH);
        assert_eq!(icon.size(), Vec2::splat(TOUCH));
        assert_eq!(big.size(), Vec2::splat(64.0));
    }

    #[test]
    fn the_switch_flips_when_tapped() {
        let ctx = egui::Context::default();
        let mut on = false;
        // One frame; where the switch's middle is.
        let mut frame = |events: Vec<egui::Event>, on: &mut bool| {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(360.0, 800.0))), events, ..Default::default() };
            let mut at = Pos2::ZERO;
            let run = ctx.run_ui(input, |ui| at = switch(ui, on, "Wi-Fi only").rect.center());
            run.drop_without_applying_deltas();
            at
        };
        let at = frame(vec![], &mut on);
        frame(vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], &mut on);
        assert!(!on, "not before the finger lifts");
        frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], &mut on);
        assert!(on);
    }

    /// Draw `add` on a phone's page in a headless egui, tap where `at` says (given the
    /// page's top left and width), and return what the taps asked for.
    fn tapped(at: impl Fn(Pos2, f32) -> Pos2, mut add: impl FnMut(&mut Ui, &mut Vec<Action>)) -> Vec<Action> {
        let ctx = egui::Context::default();
        let mut acts = Vec::new();
        let mut origin = (Pos2::ZERO, 0.0);
        let mut frame = |events: Vec<egui::Event>, acts: &mut Vec<Action>| {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(412.0, 900.0))), events, ..Default::default() };
            let run = ctx.run_ui(input, |ui| {
                origin = (ui.cursor().min, ui.available_width());
                add(ui, acts);
            });
            run.drop_without_applying_deltas();
            origin
        };
        frame(vec![], &mut acts);
        let (min, width) = frame(vec![], &mut acts);
        let at = at(min, width);
        frame(vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], &mut acts);
        frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], &mut acts);
        frame(vec![], &mut acts);
        acts
    }

    fn playlists(n: usize) -> Vec<Card> {
        (0..n).map(|i| Card { title: format!("Playlist {i}"), subtitle: "Someone \u{2022} 48 songs".into(), thumbs: vec![], target: Target::Playlist(format!("PL{i}")), round: false, links: vec![] }).collect()
    }

    #[test]
    fn a_tap_on_a_carousel_item_opens_that_card() {
        let cards = playlists(6);
        let draw = |ui: &mut Ui, acts: &mut Vec<Action>| carousel(ui, "test-carousel", 0, &cards, &HashSet::new(), acts);
        let large = tapped(|min, width| min + vec2(carousel_sizes(width)[0] / 2.0, CAROUSEL_H / 2.0), draw);
        assert!(matches!(large.as_slice(), [Action::Open(Target::Playlist(id))] if id == "PL0"), "the large one");
        let medium = tapped(|min, width| min + vec2(carousel_sizes(width)[0] + CAROUSEL_GAP + 20.0, CAROUSEL_H / 2.0), draw);
        assert!(matches!(medium.as_slice(), [Action::Open(Target::Playlist(id))] if id == "PL1"), "the medium one");
        let small = tapped(|min, width| min + vec2(width - 20.0, CAROUSEL_H / 2.0), draw);
        assert!(matches!(small.as_slice(), [Action::Open(Target::Playlist(id))] if id == "PL2"), "the small one");
    }

    #[test]
    fn the_heros_button_plays_it_and_a_tap_elsewhere_opens_it() {
        let cards = playlists(1);
        let draw = |ui: &mut Ui, acts: &mut Vec<Action>| hero_card(ui, "Mixed for you", &cards[0], acts);
        let hero = |min: Pos2, width: f32| hero_geom(Rect::from_min_size(min, vec2(width, HERO_H)));
        let played = tapped(|min, width| hero(min, width).play.center(), draw);
        assert!(matches!(played.as_slice(), [Action::PlayCollection(Target::Playlist(id), false)] if id == "PL0"));
        let opened = tapped(|min, width| hero(min, width).cover.center(), draw);
        assert!(matches!(opened.as_slice(), [Action::Open(Target::Playlist(id))] if id == "PL0"));
    }

    #[test]
    fn a_tap_on_a_song_tile_is_the_tiles_and_asks_for_nothing_by_itself() {
        let track = Track { id: "v1".into(), title: "Teh Hijau".into(), artists: vec![], album: None, duration: None, thumbs: vec![], track_nr: None, plays: None, music_video: false };
        let opts = widgets::RowOpts { number: None, show_album: false, current: false, playing: false, accent: Color32::WHITE, time: 0.0, liked: false, added_by: None, pick: false, selected: None, remove: None, queue_remove: None, queue_move: None };
        let clicks = std::cell::Cell::new(0);
        let acts = tapped(
            |min, _| min + vec2(100.0, TILE_H / 2.0),
            |ui, acts| {
                if song_tile(ui, 186.0, &track, &opts, acts).clicked() {
                    clicks.set(clicks.get() + 1);
                }
            },
        );
        assert_eq!(clicks.get(), 1);
        assert!(acts.is_empty(), "what a tap does is the page's to say (Home starts the song's radio)");
    }

    // -------------------------------------------------------------- stage 2's components

    #[test]
    fn a_search_bars_parts_follow_each_other_and_its_clear_button_fits_a_finger() {
        for width in WIDTHS {
            let rect = Rect::from_min_size(pos2(16.0, 40.0), vec2(width, SEARCH_BAR_H));
            for trailing in [0.0, TOUCH, TOUCH + 28.0] {
                let empty = search_bar_geom(rect, false, trailing);
                let typed = search_bar_geom(rect, true, trailing);
                assert!(empty.clear.is_none());
                let clear = typed.clear.unwrap();
                assert!(clear.width() >= TOUCH && clear.height() >= TOUCH);
                assert!(rect.contains(typed.icon) && typed.icon.x < typed.text.left());
                assert!(typed.text.right() <= clear.left(), "the text stops at the button");
                assert!(clear.right() <= rect.right() - trailing + 0.01, "{width} {trailing}: the end is left free");
                assert!(empty.text.right() <= rect.right() - trailing);
                assert!(empty.text.width() > typed.text.width());
                assert!(typed.text.width() >= 120.0, "{width} {trailing}: room to type: {}", typed.text.width());
            }
        }
    }

    #[test]
    fn a_floating_button_stands_in_the_pages_corner_above_the_mini_player() {
        for width in WIDTHS {
            let view = Rect::from_min_size(pos2(16.0, 88.0), vec2(width, 700.0));
            let (free, lifted) = (fab_rect(view, 0.0), fab_rect(view, 72.0));
            for r in [free, lifted] {
                assert!(view.contains_rect(r));
                assert!(r.width() >= TOUCH && r.height() >= TOUCH);
                assert_eq!(r.right(), view.right());
            }
            assert_eq!(free.bottom(), view.bottom() - 16.0);
            assert_eq!(lifted.bottom(), free.bottom() - 72.0, "a mini player lifts it clear");
        }
    }

    #[test]
    fn segments_share_the_row_and_each_fits_a_finger() {
        for width in WIDTHS {
            let row = Rect::from_min_size(pos2(36.0, 200.0), vec2(width - 40.0, TOUCH));
            for count in [2, 3] {
                let parts = segments(row, count);
                assert_eq!(parts.len(), count);
                assert_eq!(parts[0].left(), row.left());
                assert!((parts[count - 1].right() - row.right()).abs() < 0.01);
                for pair in parts.windows(2) {
                    assert!(pair[0].right() < pair[1].left(), "a gap between them");
                    assert!((pair[0].width() - pair[1].width()).abs() < 0.01);
                }
                assert!(parts.iter().all(|p| p.width() >= TOUCH && p.height() >= TOUCH));
            }
        }
    }

    #[test]
    fn a_sliders_handle_stays_on_its_track_between_the_two_parts() {
        let rect = Rect::from_min_size(pos2(36.0, 300.0), vec2(288.0, TOUCH));
        for t in [0.0, 0.25, 0.5, 1.0] {
            let g = slider_geom(rect, t);
            assert!(rect.contains_rect(g.handle), "{t}: {g:?}");
            assert!(g.active.right() < g.handle.left() && g.handle.right() < g.rest.left(), "{t}: a gap either side of the handle");
            assert_eq!((g.active.left(), g.rest.right()), (rect.left(), rect.right()));
            assert!(g.handle.height() > g.active.height(), "the handle stands taller than the track");
        }
        assert!(slider_geom(rect, 0.75).handle.center().x > slider_geom(rect, 0.25).handle.center().x);
        assert_eq!(slider_geom(rect, f32::NAN), slider_geom(rect, 0.0));
        assert_eq!(slider_geom(rect, 4.0), slider_geom(rect, 1.0));
    }

    #[test]
    fn a_finger_on_a_slider_sets_a_whole_number_in_its_range() {
        let rect = Rect::from_min_size(pos2(36.0, 300.0), vec2(288.0, TOUCH));
        assert_eq!(slider_value(rect, rect.left() - 40.0, 1, 12), 1);
        assert_eq!(slider_value(rect, rect.left(), 1, 12), 1);
        assert_eq!(slider_value(rect, rect.right(), 1, 12), 12);
        assert_eq!(slider_value(rect, rect.right() + 99.0, 1, 12), 12);
        let mid = slider_value(rect, rect.center().x, 1, 12);
        assert!(mid == 6 || mid == 7, "{mid}");
        assert_eq!(slider_value(rect, rect.center().x, 5, 5), 5, "a range of one value");
    }

    #[test]
    fn a_switch_is_hit_in_a_fingers_height_with_its_track_inside() {
        let target = drawn(360.0, |ui| switch(ui, &mut true, "Autoplay").rect);
        assert_eq!(target.size(), SWITCH_TARGET);
        assert!(target.width() >= TOUCH && target.height() >= TOUCH);
        assert!(SWITCH.y < SWITCH_TARGET.y && SWITCH.x == SWITCH_TARGET.x);
    }

    #[test]
    fn a_card_takes_no_room_when_nothing_was_put_on_it() {
        let (empty, full, labelled) = drawn(360.0, |ui| {
            let m = material::scheme();
            let top = ui.cursor().top();
            card(ui, Some(GroupLabel { title: "Sound", value: None }), m.surface_container, |_| {});
            let empty = ui.cursor().top() - top;
            card(ui, None, m.surface_container, |ui| {
                ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
            });
            let full = ui.cursor().top() - top - empty;
            let before = ui.cursor().top();
            card(ui, Some(GroupLabel { title: "Sound", value: Some("3 of 5") }), m.surface_container, |ui| {
                ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
            });
            (empty, full, ui.cursor().top() - before)
        });
        assert_eq!(empty, 0.0, "a group the search box emptied is not there at all");
        assert!(full >= 72.0 + 2.0 * CARD_PAD.y);
        assert!(labelled > full + 20.0, "its label stands above it: {labelled} and {full}");
    }

    #[test]
    fn a_tap_on_a_row_of_a_card_is_the_rows() {
        let opened = std::cell::Cell::new(0);
        let flipped = std::cell::Cell::new(false);
        tapped(
            |min, _| min + vec2(120.0, 36.0 + CARD_PAD.y + NAV_ROW_H / 2.0),
            |ui, _| {
                card(ui, Some(GroupLabel { title: "Sound", value: None }), material::scheme().surface_container, |ui| {
                    if nav_row(ui, icon::PLAY.into(), "Playback", "Autoplay, crossfade").clicked() {
                        opened.set(opened.get() + 1);
                    }
                });
            },
        );
        assert_eq!(opened.get(), 1, "the card itself takes no taps from what is on it");
        // Also inside a page that scrolls, as every page does.
        tapped(
            |min, _| min + vec2(CARD_PAD.x + SWITCH.x / 2.0, CARD_PAD.y + TOUCH / 2.0),
            |ui, _| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    card(ui, None, material::scheme().surface_container, |ui| {
                        let mut on = flipped.get();
                        switch(ui, &mut on, "Autoplay");
                        flipped.set(on);
                    });
                });
            },
        );
        assert!(flipped.get(), "nor from a switch");
    }

    #[test]
    fn a_cards_rows_are_as_wide_as_the_card_less_its_padding() {
        let (page, row) = drawn(360.0, |ui| {
            let page = ui.available_width();
            let row = card(ui, None, material::scheme().surface_container, |ui| ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover()).0);
            (page, row)
        });
        assert!((row.width() - (page - 2.0 * CARD_PAD.x)).abs() < 0.01);
    }

    #[test]
    fn a_navigation_row_a_chip_and_a_dropdown_fit_a_finger() {
        let (nav, chip, drop, step) = drawn(360.0, |ui| {
            let nav = nav_row(ui, icon::PLAY.into(), "Playback", "Autoplay, crossfade").rect;
            let chip = assist_chip(ui, "senja kala", Some(icon::HISTORY)).rect;
            let drop = dropdown(ui, "English", 200.0, "Translate into").rect;
            let top = ui.cursor().top();
            stepper(ui, "100%", true, true, ("Smaller", "Larger"));
            (nav, chip, drop, ui.cursor().top() - top)
        });
        assert_eq!(nav.height(), NAV_ROW_H);
        assert!(nav.height() >= TOUCH && chip.height() >= TOUCH && drop.height() >= TOUCH && step >= TOUCH);
        assert!(chip.width() > TOUCH && drop.width() == 200.0);
    }

    #[test]
    fn a_dropdown_is_never_wider_than_its_row() {
        let drop = drawn(200.0, |ui| dropdown(ui, "English", 320.0, "Translate into").rect);
        assert!(drop.width() <= 200.0);
    }

    #[test]
    fn a_tap_on_a_segment_chooses_it_and_the_chosen_one_takes_no_tap() {
        let tap = |at: f32, chosen: usize| {
            let picked = std::cell::Cell::new(None);
            tapped(
                |min, width| min + vec2(width * at, TOUCH / 2.0),
                |ui, _| {
                    if let Some(i) = segmented(ui, &["System", "Light", "Dark"], chosen) {
                        picked.set(Some(i));
                    }
                },
            );
            picked.get()
        };
        assert_eq!(tap(0.5, 0), Some(1));
        assert_eq!(tap(0.9, 0), Some(2));
        assert_eq!(tap(0.1, 0), None, "it is the one chosen already");
    }

    #[test]
    fn where_none_need_be_chosen_a_tap_on_the_chosen_segment_is_told_too() {
        let tap = |at: f32, chosen: Option<usize>| {
            let picked = std::cell::Cell::new(None);
            tapped(
                |min, width| min + vec2(width * at, TOUCH / 2.0),
                |ui, _| {
                    if let Some(i) = segmented_any(ui, &["Private", "Unlisted", "Public"], chosen, Color32::BLACK) {
                        picked.set(Some(i));
                    }
                },
            );
            picked.get()
        };
        assert_eq!(tap(0.5, None), Some(1));
        assert_eq!(tap(0.1, Some(0)), Some(0), "a second tap on it, so it can be let go");
        assert_eq!(tap(0.9, Some(0)), Some(2));
    }

    #[test]
    fn a_dialogs_buttons_and_text_field_fit_a_finger() {
        let (text, danger, field) = drawn(360.0, |ui| {
            let mut name = String::new();
            (button(ui, "OK", None, ButtonStyle::Text).rect, button(ui, "Delete", None, ButtonStyle::Danger).rect, text_field(ui, &mut name, "My playlist", false).rect)
        });
        for r in [text, danger] {
            assert!(r.width() >= TOUCH && r.height() >= TOUCH, "{r:?}");
        }
        assert!(field.height() >= 40.0 && field.width() > 200.0, "the text itself, inside its 56-point outline: {field:?}");
    }

    #[test]
    fn a_list_row_with_a_chevron_keeps_its_text_clear_of_it() {
        let rect = Rect::from_min_size(pos2(16.0, 0.0), vec2(328.0, 72.0));
        let g = row_geom(rect, 56.0, 20.0, false);
        assert!(g.text.right() + 20.0 <= rect.right());
        assert!(g.text.width() > 150.0);
    }

    #[test]
    fn a_section_header_is_as_tall_as_its_action_needs() {
        let (plain, over) = drawn(360.0, |ui| {
            let top = ui.cursor().top();
            section_header(ui, &SectionHeader { title: "Quick picks", overline: None, avatar: None, trailing: Trailing::Text("Play all") });
            let mid = ui.cursor().top();
            section_header(ui, &SectionHeader { title: "Listen again", overline: Some("You"), avatar: None, trailing: Trailing::Icon(icon::CHEVRON_RIGHT, "Next") });
            (mid - top, ui.cursor().top() - mid)
        });
        assert!(plain >= TOUCH && plain < 80.0, "with the room above it, its action has a finger's height: {plain}");
        assert!(over > plain, "an overline takes a line more");
    }

    #[test]
    fn a_look_without_stacks_puts_nothing_behind_a_plate() {
        let indigo = material::Style::of(material::Look::Indigo, &material::Scheme::DARK);
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 48.0));
        for stack in [Stack::None, Stack::Card, Stack::Ink] {
            assert_eq!(stack_rect(rect, stack, &indigo), None);
        }
    }

    #[test]
    fn stickers_second_card_is_the_same_size_set_off_right_and_down() {
        let sticker = material::Style::of(material::Look::Sticker, &material::Scheme::STICKER_LIGHT);
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 48.0));
        assert_eq!(stack_rect(rect, Stack::None, &sticker), None, "a plate can ask for none");
        let back = stack_rect(rect, Stack::Card, &sticker).unwrap();
        assert_eq!(back.size(), rect.size());
        assert_eq!(back.min - rect.min, Vec2::splat(4.0));
    }

    #[test]
    fn a_small_plates_second_card_peeks_out_less() {
        let sticker = material::Style::of(material::Look::Sticker, &material::Scheme::STICKER_LIGHT);
        let chip = Rect::from_min_size(pos2(0.0, 0.0), vec2(80.0, 24.0));
        let off = stack_rect(chip, Stack::Ink, &sticker).unwrap().min - chip.min;
        assert!(off.x >= 2.0 && off.x < 4.0, "{off:?}");
    }
}
