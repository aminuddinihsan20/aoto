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

/// The least a finger needs: every control can be hit in a square this size.
pub const TOUCH: f32 = 48.0;
/// How strongly a pressed control is washed with its content colour.
const PRESSED: f32 = 0.1;

impl From<Type> for Spec {
    fn from(t: Type) -> Spec {
        t.spec()
    }
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
    let m = material::scheme();
    let ink = if selected { m.on_secondary_container } else { m.on_surface_variant };
    let label_g = galley(ui, label, Type::LabelLarge, ink, 240.0);
    let lead = chip_lead(selected);
    let (rect, resp) = ui.allocate_exact_size(vec2(lead + label_g.size().x + 16.0, TOUCH), Sense::click());
    let resp = named(resp, label);
    let chip = Rect::from_center_size(rect.center(), vec2(rect.width(), CHIP_H));
    if selected {
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
}

/// A labelled button, 40 points tall in a touch target of 48, with an optional icon
/// before its label.
pub fn button(ui: &mut Ui, label: &str, glyph: Option<char>, style: ButtonStyle) -> Response {
    let m = material::scheme();
    let (fill, ink) = match style {
        ButtonStyle::Filled => (m.primary, m.on_primary),
        ButtonStyle::Tonal => (m.secondary_container, m.on_secondary_container),
    };
    let g = galley(ui, label, Type::LabelLarge, ink, 280.0);
    let lead = if glyph.is_some() { 16.0 + 18.0 + 8.0 } else { 24.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(lead + g.size().x + 24.0, TOUCH), Sense::click());
    let resp = named(resp, label);
    let body = Rect::from_center_size(rect.center(), vec2(rect.width(), 40.0));
    let fade = if ui.is_enabled() { 1.0 } else { 0.38 };
    ui.painter().rect_filled(body, 20.0, fill.gamma_multiply(fade));
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(body, 20.0, ink.gamma_multiply(PRESSED));
    }
    let ink = ink.gamma_multiply(fade);
    if let Some(glyph) = glyph {
        widgets::icon_at(ui, pos2(body.left() + 16.0 + 9.0, body.center().y), glyph, 17.0, ink);
    }
    ui.painter().galley(pos2(body.left() + lead, body.center().y - g.size().y / 2.0), g, ink);
    resp
}

/// How an icon button stands out: no container, a tonal one, or the primary colour.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconStyle {
    Standard,
    /// The next stages' round actions (a collection's header, the transport).
    #[allow(dead_code)]
    Tonal,
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
}

impl<'a> IconButton<'a> {
    pub fn new(icon: impl Into<Icon>, name: &'a str) -> Self {
        Self { icon: icon.into(), name, style: IconStyle::Standard, size: 40.0, radius: None }
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

    /// Draw it in the layout. The next stages' rows of actions are laid out with it.
    #[allow(dead_code)]
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
        let radius = self.radius.unwrap_or(self.size / 2.0);
        let (fill, ink) = match self.style {
            IconStyle::Standard => (None, m.on_surface_variant),
            IconStyle::Tonal => (Some(m.secondary_container), m.on_secondary_container),
            IconStyle::Filled => (Some(m.primary), m.on_primary),
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(body, radius, fill);
        }
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(body, radius, ink.gamma_multiply(PRESSED));
        }
        paint_icon(ui, center, (self.size * 0.6).min(28.0), self.icon, ink);
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

/// An on/off switch, centred in a row tall enough for a finger. Flips `on` when tapped.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> Response {
    let m = material::scheme();
    let (rect, mut resp) = ui.allocate_exact_size(SWITCH, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), value, name));
    let t = theme::anim_bool(ui.ctx(), resp.id.with("t"), value, 0.16);
    let fade = if ui.is_enabled() { 1.0 } else { 0.38 };
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
}

/// What was done to a list row.
#[allow(dead_code)]
pub struct ListRowOut {
    pub row: Response,
    pub more: Option<Response>,
}

/// Draw a list row across the layout's width. The next stages' pages are built from it.
#[allow(dead_code)]
pub fn list_row(ui: &mut Ui, r: &ListRow) -> ListRowOut {
    let m = material::scheme();
    let (height, cover) = row_size(r.dense);
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let row = named(row, r.title);
    let trailing = r.trailing.map(|t| galley(ui, t, Type::BodyMedium, m.on_surface_variant, 120.0));
    let g = row_geom(rect, cover, trailing.as_ref().map_or(0.0, |t| t.size().x), r.more.is_some());
    if r.active || row.is_pointer_button_down_on() {
        let wash = if r.active { m.surface_container } else { m.on_surface.gamma_multiply(PRESSED) };
        ui.painter().rect_filled(rect.expand2(vec2(8.0, 0.0)), shape::CARD_SMALL, wash);
    }
    let radius = if r.round { cover / 2.0 } else { shape::TILE_SMALL };
    widgets::paint_cover(ui, g.cover, r.thumbs, radius, 1.0);
    let title = if r.active { m.primary } else { m.on_surface };
    two_lines(ui, g.text.left(), rect.center().y, g.text.width(), r.title, Type::BodyLarge, title, r.subtitle, Type::BodyMedium, m.on_surface_variant);
    if let Some(t) = trailing {
        ui.painter().galley(pos2(g.trailing_right - t.size().x, rect.center().y - t.size().y / 2.0), t, m.on_surface_variant);
    }
    let more = r.more.zip(g.more).map(|(name, at)| IconButton::new(icon::ELLIPSIS_VERTICAL, name).show_at(ui, at.center(), row.id.with("more")));
    ListRowOut { row, more }
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
#[allow(dead_code)]
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
    let radius = shape::CARD.min(rect.width() / 2.0);
    widgets::paint_cover_cropped(ui, rect, &c.thumbs, CornerRadius::same(radius as u8));
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
    let r = shape::TILE as u8;
    ui.painter().rect_filled(rect, shape::TILE, m.surface_container_high);
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
    ui.painter().rect_filled(rect, shape::CARD, m.primary_container);
    // A paler disc behind the cover; it leaves the card through its straight edges only.
    let disc = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    disc.circle_filled(pos2(rect.right() - 150.0, rect.center().y), 140.0, m.on_primary_container.gamma_multiply(0.09));
    widgets::soft_shadow(ui, g.cover, 24.0, 18.0, 0.35);
    widgets::paint_cover(ui, g.cover, &c.thumbs, 24.0, 1.0);
    ui.painter().rect_stroke(g.cover, 24.0, Stroke::new(2.0, m.primary_container), StrokeKind::Outside);

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
}
