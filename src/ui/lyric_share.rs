//! Sharing lyrics as a picture, on the phone: choosing up to four lines of the song on
//! screen, then the card with its styles and formats, and Save and Share. What goes on
//! the card is `crate::lyric_card`; Android draws it, and the picture shown here is the
//! file that is saved or shared.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::m3::{self, IconButton};
use super::phone::SIDE;
use super::widgets::{self, named};
use super::App;
use crate::lyric_card::{self, Card, Colors, Format, MAX_LINES, Selection, Style, Tap};
use crate::lyrics::{Lyrics, Sync};
use crate::material::{self, Type, Weight, shape};
use crate::model::Thumb;
use crate::theme::{self, icon};

/// The screen's bar: its close button and its name.
const BAR_H: f32 = 56.0;
/// Save, Share and Continue.
const BUTTON_H: f32 = 56.0;
/// A style's swatch, and the square a finger hits it in.
const SWATCH: f32 = 48.0;
const SWATCH_TOUCH: f32 = 56.0;
/// The line under the card that says how to change its lines.
const HINT_H: f32 = 20.0;
/// How far the card stays in from the page's margins.
const CARD_INSET: f32 = 16.0;

/// The two steps of sharing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Step {
    /// Choosing the lines.
    #[default]
    Lines,
    /// The card made of them.
    Card,
}

/// What the listener asked of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Close,
    /// A line of the lyrics (its place in them) was tapped.
    Tap(usize),
    /// On from the lines to the card.
    Next,
    /// Back from the card to the lines.
    ChangeLines,
    Style(Style),
    Format(Format),
    Save,
    Share,
}

/// What an ask needs of the app, beyond the screen's own state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    /// A fifth line was tapped.
    Full,
    Save,
    Share,
}

/// The picture of the card: what was asked of Android, and what came back.
#[derive(Default)]
struct Picture {
    /// The card last sent to be drawn (its description).
    asked: Option<String>,
    drawing: bool,
    arrived: Arc<Mutex<Option<Drawn>>>,
    shown: Option<Shown>,
    /// The card that could not be drawn, and why.
    failed: Option<(String, String)>,
}

/// A drawing come back from its thread: the card it is of, and its file and pixels.
struct Drawn {
    card: String,
    result: Result<(PathBuf, egui::ColorImage), String>,
}

/// The picture on screen: the very file Save and Share send.
struct Shown {
    card: String,
    file: PathBuf,
    texture: egui::TextureHandle,
}

/// The song's cover as a file for Android to read: which song it was asked for, and
/// where it landed.
#[derive(Default)]
struct Cover {
    asked: Option<String>,
    got: Arc<Mutex<Option<(String, PathBuf)>>>,
}

/// What saving or sharing came to.
enum Sent {
    Saved(Result<String, String>),
    Shared(Result<(), String>),
}

/// The screen between frames.
#[derive(Default)]
pub struct State {
    open: bool,
    step: Step,
    picks: Selection,
    /// The line to bring into view when the list is next drawn.
    reveal: Option<usize>,
    style: Style,
    format: Format,
    picture: Picture,
    cover: Cover,
    /// A save or a share is on its way; its outcome arrives here.
    sending: bool,
    sent: Arc<Mutex<Option<Sent>>>,
    /// The styles' gradients, as textures for their swatches.
    tints: Vec<(Colors, egui::TextureHandle)>,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Start choosing lines of `song`, the list opening at line `at`. `chosen`: that
    /// line was long-pressed, and is the choice to begin with.
    fn begin(&mut self, song: &str, at: Option<usize>, chosen: bool) {
        self.picks.follow(song);
        if let (Some(line), true) = (at, chosen) {
            self.picks.clear();
            self.picks.tap(line);
        }
        self.reveal = at.or_else(|| self.picks.lines().first().copied());
        self.step = Step::Lines;
        self.open = true;
    }

    /// Android's Back: from the card to its lines, from the lines out.
    pub fn back(&mut self) {
        match self.step {
            Step::Card => {
                self.act(Ask::ChangeLines);
            }
            Step::Lines => self.open = false,
        }
    }

    /// The song (or its lyrics source) on screen: another one's lines are not the ones
    /// chosen, so the choosing starts over.
    fn follow(&mut self, song: &str) {
        if self.picks.follow(song) || self.picks.is_empty() {
            self.step = Step::Lines;
        }
    }

    /// Do what was asked, as far as the screen alone can; the rest is for the app.
    fn act(&mut self, ask: Ask) -> Option<Effect> {
        match ask {
            Ask::Close => self.open = false,
            Ask::Tap(line) => {
                if self.picks.tap(line) == Tap::Full {
                    return Some(Effect::Full);
                }
            }
            Ask::Next => {
                if !self.picks.is_empty() {
                    self.step = Step::Card;
                    // A card that could not be drawn is tried again on the way back to it.
                    if self.picture.failed.take().is_some() {
                        self.picture.asked = None;
                    }
                }
            }
            Ask::ChangeLines => {
                self.step = Step::Lines;
                self.reveal = self.picks.lines().first().copied();
            }
            Ask::Style(style) => self.style = style,
            Ask::Format(format) => self.format = format,
            Ask::Save => return Some(Effect::Save),
            Ask::Share => return Some(Effect::Share),
        }
        None
    }
}

/// The line being sung `ms` into the song, or the last one sung before it: where the
/// list opens. Lyrics without timing open at their top.
fn sung_now(lyrics: &Lyrics, ms: i64) -> Option<usize> {
    if lyrics.sync == Sync::None {
        return None;
    }
    lyrics.lines.iter().rposition(|l| lyric_card::pickable(l) && i64::from(l.start) <= ms)
}

fn lock<T>(slot: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(|e| e.into_inner())
}

// ------------------------------------------------------------------ geometry

/// Where the lines page's parts are.
struct LinesGeom {
    /// The line that says what to do, and how many are chosen.
    hint: Rect,
    list: Rect,
    next: Rect,
}

/// `area`: the screen less the system's bars.
fn lines_geom(area: Rect) -> LinesGeom {
    let (left, right) = (area.left() + SIDE, area.right() - SIDE);
    let hint = Rect::from_min_max(pos2(left, area.top() + BAR_H), pos2(right, area.top() + BAR_H + 32.0));
    let next = Rect::from_min_max(pos2(left, area.bottom() - 16.0 - BUTTON_H), pos2(right, area.bottom() - 16.0));
    let list = Rect::from_min_max(pos2(left, hint.bottom() + 4.0), pos2(right, next.top() - 12.0));
    LinesGeom { hint, list, next }
}

/// Where the card page's parts are.
struct CardGeom {
    card: Rect,
    hint: Rect,
    /// Each style's touch target; its swatch is centred in it.
    styles: [Rect; 5],
    formats: [Rect; 3],
    save: Rect,
    share: Rect,
}

/// The card as large as fits over its controls, which follow right under it. `chips`:
/// how wide each format's chip is.
fn card_geom(area: Rect, format: Format, chips: [f32; 3]) -> CardGeom {
    let (left, right) = (area.left() + SIDE, area.right() - SIDE);
    let width = right - left;
    let top = area.top() + BAR_H + 8.0;
    // Under the card: the hint, the styles, the formats and the buttons, and their gaps.
    let under = 12.0 + HINT_H + 12.0 + SWATCH_TOUCH + 4.0 + m3::TOUCH + 12.0 + BUTTON_H + 16.0;
    let room = (area.bottom() - under - top).max(0.0);
    let (pw, ph) = format.size();
    let aspect = pw as f32 / ph as f32;
    let w = (width - 2.0 * CARD_INSET).min(room * aspect);
    let card = Rect::from_min_size(pos2(area.center().x - w / 2.0, top), vec2(w, w / aspect));
    let hint = Rect::from_min_size(pos2(left, card.bottom() + 12.0), vec2(width, HINT_H));

    // The swatches keep 8 points between their targets where the screen has the room.
    let y = hint.bottom() + 12.0;
    let pitch = (SWATCH_TOUCH + 8.0).min((width - SWATCH_TOUCH) / 4.0);
    let styles = std::array::from_fn(|i| Rect::from_center_size(pos2(area.center().x + pitch * (i as f32 - 2.0), y + SWATCH_TOUCH / 2.0), Vec2::splat(SWATCH_TOUCH)));

    let y = y + SWATCH_TOUCH + 4.0;
    let mut x = area.center().x - (chips.iter().sum::<f32>() + 2.0 * m3::CHIP_GAP) / 2.0;
    let formats = chips.map(|chip| {
        let cell = Rect::from_min_size(pos2(x, y), vec2(chip, m3::TOUCH));
        x += chip + m3::CHIP_GAP;
        cell
    });

    let y = y + m3::TOUCH + 12.0;
    let half = (width - 8.0) / 2.0;
    let save = Rect::from_min_size(pos2(left, y), vec2(half, BUTTON_H));
    let share = Rect::from_min_size(pos2(right - half, y), vec2(half, BUTTON_H));
    CardGeom { card, hint, styles, formats, save, share }
}

/// How wide each format's chip is, with `picked` the selected one.
fn chip_widths(ui: &Ui, picked: Format) -> [f32; 3] {
    Format::ALL.map(|f| m3::filter_chip_size(ui, f.label(), f == picked).x)
}

// ------------------------------------------------------------------ parts

fn close_id() -> egui::Id {
    egui::Id::unique("lyric-share-close")
}

fn line_id(line: usize) -> egui::Id {
    egui::Id::unique(("lyric-share-line", line))
}

fn next_id() -> egui::Id {
    egui::Id::unique("lyric-share-next")
}

fn card_id() -> egui::Id {
    egui::Id::unique("lyric-share-card")
}

fn style_id(style: Style) -> egui::Id {
    egui::Id::unique(("lyric-share-style", style as u8))
}

fn save_id() -> egui::Id {
    egui::Id::unique("lyric-share-save")
}

fn share_id() -> egui::Id {
    egui::Id::unique("lyric-share-share")
}

/// The share mark (three dots joined by two lines; the icon font has none), on a
/// 24-unit grid.
pub(super) fn share_mark(ui: &Ui, center: Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let at = |x: f32, y: f32| center + vec2(x - 12.0, y - 12.0) * k;
    let (from, up, down) = (at(6.0, 12.0), at(18.0, 5.0), at(18.0, 19.0));
    for end in [up, down] {
        ui.painter().line_segment([from, end], Stroke::new(1.8 * k, color));
    }
    for dot in [from, up, down] {
        ui.painter().circle_filled(dot, 3.0 * k, color);
    }
}

/// What stands before a wide button's label.
#[derive(Clone, Copy)]
enum Mark {
    None,
    Glyph(char),
    Share,
}

/// One of the screen's wide buttons, filled with the primary colour for the main
/// action and tonal for the other; faded and deaf while not `enabled`. True when tapped.
fn wide_button(ui: &Ui, rect: Rect, id: egui::Id, label: &str, mark: Mark, filled: bool, enabled: bool) -> bool {
    let m = material::scheme();
    let (fill, ink) = if filled { (m.primary, m.on_primary) } else { (m.secondary_container, m.on_secondary_container) };
    let resp = named(ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() }), label);
    let fade = if enabled { 1.0 } else { 0.38 };
    ui.painter().rect_filled(rect, rect.height() / 2.0, fill.gamma_multiply(fade));
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, rect.height() / 2.0, ink.gamma_multiply(0.1));
    }
    let ink = ink.gamma_multiply(fade);
    let lead = if matches!(mark, Mark::None) { 0.0 } else { 18.0 + 10.0 };
    let g = m3::galley(ui, label, Type::TitleMedium, ink, rect.width() - 32.0 - lead);
    let left = rect.center().x - (lead + g.size().x) / 2.0;
    match mark {
        Mark::None => {}
        Mark::Glyph(glyph) => widgets::icon_at(ui, pos2(left + 9.0, rect.center().y), glyph, 18.0, ink),
        Mark::Share => share_mark(ui, pos2(left + 9.0, rect.center().y), 20.0, ink),
    }
    ui.painter().galley(pos2(left + lead, rect.center().y - g.size().y / 2.0), g, ink);
    resp.clicked()
}

/// The screen's bar: the button that closes it, and its name. True when closed.
fn top_bar(ui: &Ui, area: Rect) -> bool {
    let m = material::scheme();
    let cy = area.top() + BAR_H / 2.0;
    let closed = IconButton::new(icon::X, "Close").show_at(ui, pos2(area.left() + SIDE + 12.0, cy), close_id()).clicked();
    let title = m3::galley(ui, "Share lyrics", Type::TitleLarge, m.on_surface, area.width() - SIDE * 2.0 - 40.0);
    ui.painter().galley(pos2(area.left() + SIDE + 40.0, cy - title.size().y / 2.0), title, m.on_surface);
    closed
}

/// A style's gradient over `rect`, from its four-pixel texture.
fn paint_tint(ui: &Ui, rect: Rect, radius: f32, tint: egui::TextureId) {
    // Between the pixels' centres, where the colours blend evenly corner to corner.
    let uv = Rect::from_min_max(pos2(0.25, 0.25), pos2(0.75, 0.75));
    ui.painter().add(egui::epaint::RectShape::filled(rect, CornerRadius::same(radius as u8), Color32::WHITE).with_texture(tint, uv));
}

// ------------------------------------------------------------------ the lines

/// What the lines page shows.
struct LinesView<'a> {
    /// The lines that can be chosen: their place in the lyrics, and their words.
    lines: &'a [(usize, String)],
    picks: &'a Selection,
}

/// The first step: the song's lines, each one a tap chooses or gives up, and the way on
/// to the card once one is chosen. `reveal`: the line to scroll to, taken when shown.
fn lines_page(ui: &mut Ui, area: Rect, v: &LinesView, reveal: &mut Option<usize>) -> Option<Ask> {
    let m = material::scheme();
    let g = lines_geom(area);
    let mut ask = top_bar(ui, area).then_some(Ask::Close);

    let count = format!("{} of {MAX_LINES}", v.picks.lines().len());
    let count = m3::galley(ui, &count, Type::LabelLarge, m.primary, 80.0);
    let count_left = g.hint.right() - count.size().x;
    m3::text(ui, pos2(g.hint.left(), g.hint.center().y - 10.0), "Tap lines to choose up to four", Type::BodyMedium, m.on_surface_variant, count_left - 12.0 - g.hint.left());
    ui.painter().galley(pos2(count_left, g.hint.center().y - count.size().y / 2.0), count, m.primary);

    if v.lines.is_empty() {
        let none = m3::galley(ui, "This song has no lyrics to choose from", Type::BodyLarge, m.on_surface_variant, g.list.width());
        ui.painter().galley(g.list.center() - none.size() / 2.0, none, m.on_surface_variant);
    }
    let mut list = ui.new_child(egui::UiBuilder::new().max_rect(g.list));
    list.set_clip_rect(g.list.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical().id_salt("lyric-share-lines").auto_shrink(false).show(&mut list, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
        let role = Type::TitleLarge.spec().weight(Weight::Bold);
        for (line, words) in v.lines {
            let on = v.picks.contains(*line);
            let ink = if on { m.on_secondary_container } else { m.on_surface };
            let width = ui.available_width();
            // The check of a chosen line stands at the row's end.
            let text = m3::galley_wrapped(ui, words, role, ink, width - 16.0 - 44.0, 8);
            let (rect, _) = ui.allocate_exact_size(vec2(width, (text.size().y + 24.0).max(BUTTON_H)), Sense::hover());
            let resp = named(ui.interact(rect, line_id(*line), Sense::click()), words);
            if on {
                ui.painter().rect_filled(rect, shape::TILE, m.secondary_container);
                widgets::icon_at(ui, pos2(rect.right() - 22.0, rect.center().y), icon::CHECK, 18.0, ink);
            }
            if resp.is_pointer_button_down_on() {
                ui.painter().rect_filled(rect, shape::TILE, ink.gamma_multiply(0.08));
            }
            // A right-to-left line starts from the right, as in the lyrics.
            let x = if crate::bidi::rtl_base(words) { rect.right() - 44.0 - text.size().x } else { rect.left() + 16.0 };
            ui.painter().galley(pos2(x, rect.center().y - text.size().y / 2.0), text, ink);
            if *reveal == Some(*line) {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                *reveal = None;
            }
            if resp.clicked() {
                ask = Some(Ask::Tap(*line));
            }
        }
    });
    if wide_button(ui, g.next, next_id(), "Continue", Mark::None, true, !v.picks.is_empty()) {
        ask = Some(Ask::Next);
    }
    ask
}

// ------------------------------------------------------------------ the card

/// What stands where the card's picture goes.
enum Preview<'a> {
    /// The picture Android drew.
    Picture(egui::TextureId),
    /// It is being drawn.
    Drawing,
    Failed(&'a str),
    /// Off Android nothing draws the picture: the card laid out here, roughly, to see
    /// the screen by on a desktop.
    StandIn { card: &'a Card, thumbs: &'a [Thumb], tint: egui::TextureId },
}

/// Whether the picture can be saved and shared.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sending {
    /// This system has no way to (a desktop).
    Unavailable,
    /// The picture is still being drawn, or one is on its way already.
    Waiting,
    Ready,
}

/// What the card page shows.
struct CardView<'a> {
    preview: Preview<'a>,
    style: Style,
    format: Format,
    /// Each style's gradient, in `Style::ALL`'s order.
    tints: [egui::TextureId; 5],
    sending: Sending,
}

/// The second step: the card, its five styles and three formats, and Save and Share.
fn card_page(ui: &mut Ui, area: Rect, v: &CardView) -> Option<Ask> {
    let m = material::scheme();
    let g = card_geom(area, v.format, chip_widths(ui, v.format));
    let mut ask = top_bar(ui, area).then_some(Ask::Close);

    // The card is the way back to its lines.
    let card = named(ui.interact(g.card, card_id(), Sense::click()), "Choose other lines");
    match &v.preview {
        Preview::Picture(texture) => {
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().add(egui::epaint::RectShape::filled(g.card, CornerRadius::same(shape::CARD as u8), Color32::WHITE).with_texture(*texture, whole));
        }
        Preview::Drawing => {
            super::m3::plate(ui, g.card, super::m3::round(shape::CARD), m.surface_container_high, super::m3::Stack::Card);
            egui::Spinner::new().color(m.primary).paint_at(ui, Rect::from_center_size(g.card.center(), Vec2::splat(32.0)));
        }
        Preview::Failed(why) => {
            super::m3::plate(ui, g.card, super::m3::round(shape::CARD), m.surface_container_high, super::m3::Stack::Card);
            let width = g.card.width() - 32.0;
            let title = m3::galley(ui, "The picture could not be drawn", Type::TitleMedium, m.on_surface, width);
            let reason = m3::galley_wrapped(ui, why, Type::BodyMedium, m.on_surface_variant, width, 4);
            let top = g.card.center().y - (title.size().y + 4.0 + reason.size().y) / 2.0;
            ui.painter().galley(pos2(g.card.center().x - reason.size().x / 2.0, top + title.size().y + 4.0), reason, m.on_surface_variant);
            ui.painter().galley(pos2(g.card.center().x - title.size().x / 2.0, top), title, m.on_surface);
        }
        Preview::StandIn { card, thumbs, tint } => stand_in(ui, g.card, card, thumbs, *tint),
    }
    if card.is_pointer_button_down_on() {
        ui.painter().rect_filled(g.card, shape::CARD, m.on_surface.gamma_multiply(0.08));
    }
    if card.clicked() {
        ask = Some(Ask::ChangeLines);
    }
    let hint = m3::galley(ui, "Tap the card to choose other lines", Type::BodyMedium, m.on_surface_variant, g.hint.width());
    ui.painter().galley(pos2(g.hint.center().x - hint.size().x / 2.0, g.hint.center().y - hint.size().y / 2.0), hint, m.on_surface_variant);

    for (i, style) in Style::ALL.into_iter().enumerate() {
        let resp = named(ui.interact(g.styles[i], style_id(style), Sense::click()), style.name());
        let center = g.styles[i].center();
        paint_tint(ui, Rect::from_center_size(center, Vec2::splat(SWATCH)), SWATCH / 2.0, v.tints[i]);
        if style == v.style {
            ui.painter().circle_stroke(center, SWATCH_TOUCH / 2.0 - 1.0, Stroke::new(2.0, m.on_surface));
        }
        if resp.is_pointer_button_down_on() {
            ui.painter().circle_filled(center, SWATCH / 2.0, m.on_surface.gamma_multiply(0.12));
        }
        if resp.clicked() {
            ask = Some(Ask::Style(style));
        }
    }
    for (i, format) in Format::ALL.into_iter().enumerate() {
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(g.formats[i]).layout(egui::Layout::left_to_right(egui::Align::Center)));
        if m3::filter_chip(&mut cell, format.label(), format == v.format).clicked() {
            ask = Some(Ask::Format(format));
        }
    }

    if v.sending == Sending::Unavailable {
        let row = g.save.union(g.share);
        let note = m3::galley_wrapped(ui, "A sketch of the card: the Android app draws, saves and shares the picture", Type::BodyMedium, m.on_surface_variant, row.width(), 2);
        ui.painter().galley(pos2(row.center().x - note.size().x / 2.0, row.center().y - note.size().y / 2.0), note, m.on_surface_variant);
        return ask;
    }
    let ready = v.sending == Sending::Ready;
    if wide_button(ui, g.save, save_id(), "Save image", Mark::Glyph(icon::DOWNLOAD), false, ready) {
        ask = Some(Ask::Save);
    }
    if wide_button(ui, g.share, share_id(), "Share", Mark::Share, true, ready) {
        ask = Some(Ask::Share);
    }
    ask
}

/// The card laid out by this app's own text (see [`Preview::StandIn`]): the same
/// content in about the same places, without Android's shaping or its fitting of the
/// text size. Never saved or shared.
fn stand_in(ui: &Ui, rect: Rect, card: &Card, thumbs: &[Thumb], tint: egui::TextureId) {
    let k = rect.width() / card.width as f32;
    let ink = Color32::WHITE;
    paint_tint(ui, rect, shape::CARD, tint);
    let painter = ui.painter().with_clip_rect(rect);
    let side = 92.0 * k;
    let size = card.sizes.first().copied().unwrap_or(96) as f32 * k;
    let mut y = rect.top() + side;
    for line in &card.lines {
        let g = widgets::galley_wrapped(ui, line, theme::bold(size), ink, rect.width() - side * 2.0, 6);
        let height = g.size().y;
        painter.galley(pos2(rect.left() + side, y), g, ink);
        y += height + size * 0.22;
    }
    let cover = Rect::from_min_size(pos2(rect.left() + side, rect.bottom() - side - 148.0 * k), Vec2::splat(148.0 * k));
    widgets::paint_cover(ui, cover, thumbs, 28.0 * k, 1.0);
    let mark = widgets::galley(ui, "Aoto", theme::regular(38.0 * k), ink, rect.width());
    let mark_left = rect.right() - side - mark.size().x;
    painter.galley(pos2(mark_left, cover.center().y - mark.size().y / 2.0), mark, theme::with_alpha(ink, 0.85));
    let left = cover.right() + 28.0 * k;
    let width = mark_left - 28.0 * k - left;
    let title = widgets::galley(ui, &card.title, theme::bold(44.0 * k), ink, width);
    let artist = widgets::galley(ui, &card.artist, theme::regular(38.0 * k), ink, width);
    let top = cover.center().y - (title.size().y + artist.size().y) / 2.0;
    let artist_top = top + title.size().y;
    painter.galley(pos2(left, top), title, ink);
    painter.galley(pos2(left, artist_top), artist, theme::with_alpha(ink, 0.8));
}

// ------------------------------------------------------------------ the app's side

/// What chosen lines belong to: the song `id` and the source its `lyrics` are from.
fn song_key(id: &str, lyrics: &Lyrics) -> String {
    format!("{id}/{}", lyrics.provider.key())
}

/// The lyrics on screen, and their song's key.
fn shown_lyrics(app: &App) -> Option<(String, &Lyrics)> {
    let (id, set) = app.lyrics.as_ref()?;
    let lyrics = set.get(app.lyric_prefs.choose(id, set)?)?;
    Some((song_key(id, lyrics), lyrics))
}

/// Begin sharing the lyrics on screen, from their menu: the list opens at the line
/// being sung.
pub(super) fn start(app: &mut App) {
    let Some((song, lyrics)) = shown_lyrics(app) else { return };
    let at = sung_now(lyrics, app.audio.status.position().as_millis() as i64);
    app.lyric_share.begin(&song, at, false);
}

/// Begin sharing with the `line` that was long-pressed as the first one chosen. The
/// lyrics view hands over the song `id` and the `lyrics` it is drawing: it has them on
/// loan from the app while it draws.
pub(super) fn start_with(app: &mut App, id: &str, lyrics: &Lyrics, line: usize) {
    // A break between verses is nothing to choose: the list opens as from the menu.
    let pressed = Some(line).filter(|&l| lyrics.lines.get(l).is_some_and(lyric_card::pickable));
    let at = pressed.or_else(|| sung_now(lyrics, app.audio.status.position().as_millis() as i64));
    app.lyric_share.begin(&song_key(id, lyrics), at, pressed.is_some());
}

/// Dev hook: YTM_LYRIC_SHARE=lines|card|story|wide opens the screen once the song's
/// lyrics are in (with YTM_NOWPLAYING): on its lines or, the first two chosen, on its
/// card (square, or in the format named).
fn dev_hook(app: &App, st: &mut State, ctx: &egui::Context) {
    let Some(step) = super::dev_var("YTM_LYRIC_SHARE") else { return };
    let done = egui::Id::unique("dev-lyric-share");
    if ctx.data(|d| d.get_temp::<bool>(done).unwrap_or(false)) {
        return;
    }
    let Some((song, lyrics)) = shown_lyrics(app) else { return };
    ctx.data_mut(|d| d.insert_temp(done, true));
    st.begin(&song, None, false);
    if step != "lines" {
        for line in (0..lyrics.lines.len()).filter(|&i| lyric_card::pickable(&lyrics.lines[i])).take(2) {
            st.act(Ask::Tap(line));
        }
        st.act(Ask::Next);
        st.format = match step {
            "story" => Format::Story,
            "wide" => Format::Wide,
            _ => Format::Square,
        };
    }
}

impl State {
    /// The gradient of `colors` as a texture: four pixels, blended by the GPU.
    fn tint(&mut self, ctx: &egui::Context, colors: Colors) -> egui::TextureId {
        if let Some((_, texture)) = self.tints.iter().find(|(c, _)| *c == colors) {
            return texture.id();
        }
        // The cover's colours change with the song: the old ones are not kept for ever.
        if self.tints.len() >= 16 {
            self.tints.clear();
        }
        let middle = theme::lerp_color(colors.from, colors.to, 0.5);
        let image = egui::ColorImage::new([2, 2], vec![colors.from, middle, middle, colors.to]);
        let texture = ctx.load_texture("lyric-card-tint", image, egui::TextureOptions::LINEAR);
        let id = texture.id();
        self.tints.push((colors, texture));
        id
    }

    /// Save or share the picture on screen, on a thread: Android answers in its own time.
    fn send(&mut self, save_as: Option<String>) {
        let Some(shown) = &self.picture.shown else { return };
        let (file, slot) = (shown.file.clone(), self.sent.clone());
        self.sending = true;
        std::thread::spawn(move || {
            let sent = match save_as {
                Some(name) => Sent::Saved(lyric_card::save(&file, &name)),
                None => Sent::Shared(lyric_card::share(&file)),
            };
            *lock(&slot) = Some(sent);
            crate::platform::wake_ui();
        });
    }
}

impl Picture {
    /// Take a drawing that came back: it is shown when it is of the card `want`.
    fn arrive(&mut self, ctx: &egui::Context, want: &str) {
        let Some(drawn) = lock(&self.arrived).take() else { return };
        self.drawing = false;
        // One of a card that has changed since is dropped, and the new card asked for.
        if drawn.card != want {
            return;
        }
        match drawn.result {
            Ok((file, pixels)) => {
                let texture = ctx.load_texture("lyric-card", pixels, egui::TextureOptions::LINEAR);
                self.shown = Some(Shown { card: drawn.card, file, texture });
                self.failed = None;
            }
            Err(why) => self.failed = Some((drawn.card, why)),
        }
    }

    /// Have Android draw `card` (described by `want`), unless that is what was last
    /// asked or another drawing is still on its way.
    fn ask(&mut self, card: &Card, want: &str) {
        if self.drawing || self.asked.as_deref() == Some(want) {
            return;
        }
        self.asked = Some(want.to_owned());
        self.drawing = true;
        let (card, want, slot) = (card.clone(), want.to_owned(), self.arrived.clone());
        std::thread::spawn(move || {
            let result = lyric_card::render(&card).and_then(|file| {
                // The preview is the file itself, read back: what is seen is what is sent.
                let rgba = image::open(&file).map_err(|e| e.to_string())?.to_rgba8();
                let pixels = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                Ok((file, pixels))
            });
            *lock(&slot) = Some(Drawn { card: want, result });
            crate::platform::wake_ui();
        });
    }
}

impl Cover {
    /// The cover of `track` as a file, once it is there; asked for once per song.
    fn file(&mut self, app: &App, track: &crate::model::Track) -> Option<PathBuf> {
        if self.asked.as_deref() != Some(track.id.as_str()) {
            self.asked = Some(track.id.clone());
            if let Some(url) = crate::backend::art_url(track) {
                self.fetch(app, track.id.clone(), url);
            }
        }
        lock(&self.got).as_ref().filter(|(id, _)| *id == track.id).map(|(_, file)| file.clone())
    }

    /// Download the cover through the app's own client into the cache, under a name
    /// of its own per song (the card's description names the file).
    fn fetch(&self, app: &App, id: String, url: String) {
        use std::hash::{Hash, Hasher};
        let (slot, backend) = (self.got.clone(), app.backend.clone());
        let dir = std::env::temp_dir().join("lyric-card");
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        id.hash(&mut hash);
        let name = format!("cover-{:016x}", hash.finish());
        app.backend.spawn(async move {
            let fetched = async {
                let bytes = backend.http().get(&url).send().await?.error_for_status()?.bytes().await?;
                // Only the song on screen needs one.
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir)?;
                let file = dir.join(name);
                std::fs::write(&file, &bytes)?;
                anyhow::Ok(file)
            }
            .await;
            match fetched {
                Ok(file) => {
                    *lock(&slot) = Some((id, file));
                    backend.wake();
                }
                // The card is drawn without it.
                Err(e) => log::debug!("lyric card cover: {e:#}"),
            }
        });
    }
}

/// The screen, over Now Playing's lyrics.
pub(super) fn overlay(app: &mut App, ctx: &egui::Context) {
    let mut st = std::mem::take(&mut app.lyric_share);
    dev_hook(app, &mut st, ctx);
    if let Some(sent) = lock(&st.sent).take() {
        st.sending = false;
        match sent {
            Sent::Saved(Ok(place)) => app.toast(format!("Saved to {place}")),
            Sent::Saved(Err(why)) => app.toast(format!("Couldn't save the picture: {why}")),
            Sent::Shared(Err(why)) => app.toast(format!("Couldn't share the picture: {why}")),
            Sent::Shared(Ok(())) => {}
        }
    }
    // It belongs to the lyrics on Now Playing, and goes when that does.
    st.open &= app.np_open;
    let shown = theme::anim_bool(ctx, "lyric-share", st.open, 0.25);
    if shown <= 0.001 {
        // Gone: the picture and the swatches are let go of.
        st.picture = Picture::default();
        st.tints.clear();
        app.lyric_share = st;
        return;
    }

    let lyrics = shown_lyrics(app);
    st.follow(lyrics.as_ref().map_or("", |(song, _)| song));
    let lines: Vec<(usize, String)> = lyrics
        .as_ref()
        .map(|(_, l)| l.lines.iter().enumerate().filter(|(_, line)| lyric_card::pickable(line)).map(|(i, line)| (i, line.text.trim().to_owned())).collect())
        .unwrap_or_default();
    let track = app.shown().cloned();

    // The first style is the cover's own colour, once the artwork gave one.
    let seed = if app.art_key.as_deref() == app.current_id() { app.accent } else { material::scheme().primary };
    // The card as it stands, and its picture.
    let card = match (&lyrics, &track, st.step) {
        (Some((_, l)), Some(track), Step::Card) => {
            let cover = if lyric_card::AVAILABLE { st.cover.file(app, track) } else { None };
            Some(Card::new(&lyric_card::card_lines(l, st.picks.lines()), &track.title, &track.artist_line(), cover.as_deref(), st.style.colors(seed), st.format))
        }
        _ => None,
    };
    let tints = Style::ALL.map(|style| st.tint(ctx, style.colors(seed)));
    let want = card.as_ref().map(Card::encode);
    if let (Some(card), Some(want), true) = (&card, &want, lyric_card::AVAILABLE) {
        st.picture.arrive(ctx, want);
        st.picture.ask(card, want);
    }

    let screen = ctx.content_rect();
    let insets = crate::platform::insets(ctx);
    let ease = theme::ease_out(shown);
    let mut asked = None;
    egui::Area::new(egui::Id::unique("lyric-share")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        // Nothing under it is touched through it.
        ui.allocate_rect(screen, Sense::click_and_drag());
        ui.set_opacity(ease);
        let rect = screen.translate(vec2(0.0, 28.0 * (1.0 - ease)));
        // The player's own backdrop: the song's colour runs on from Now Playing into this.
        super::np_phone::background(app, ui.ctx(), ui.painter(), rect);
        let area = Rect::from_min_max(pos2(rect.left() + insets.left, rect.top() + insets.top), pos2(rect.right() - insets.right, rect.bottom() - insets.bottom));
        asked = match (&card, &want) {
            (Some(card), Some(want)) => {
                let picture = &st.picture;
                let current = picture.shown.as_ref().filter(|s| s.card == *want);
                let preview = if !lyric_card::AVAILABLE {
                    Preview::StandIn { card, thumbs: track.as_ref().map_or(&[][..], |t| &t.thumbs), tint: tints[st.style as usize] }
                } else if let Some(s) = current {
                    Preview::Picture(s.texture.id())
                } else if let Some((_, why)) = picture.failed.as_ref().filter(|(of, _)| of == want) {
                    Preview::Failed(why)
                } else if let Some(s) = picture.shown.as_ref().filter(|s| s.texture.size() == [card.width as usize, card.height as usize]) {
                    // The last picture stays while the same shape in other colours is drawn.
                    Preview::Picture(s.texture.id())
                } else {
                    Preview::Drawing
                };
                let sending = if !lyric_card::AVAILABLE {
                    Sending::Unavailable
                } else if current.is_some() && !st.sending {
                    Sending::Ready
                } else {
                    Sending::Waiting
                };
                card_page(ui, area, &CardView { preview, style: st.style, format: st.format, tints, sending })
            }
            _ => lines_page(ui, area, &LinesView { lines: &lines, picks: &st.picks }, &mut st.reveal),
        };
    });
    match asked.and_then(|ask| st.act(ask)) {
        Some(Effect::Full) => app.toast(format!("A card holds {MAX_LINES} lines at most")),
        Some(Effect::Save) => st.send(Some(lyric_card::file_name(track.as_ref().map_or("", |t| &t.title)))),
        Some(Effect::Share) => st.send(None),
        None => {}
    }
    app.lyric_share = st;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::{Line, Provider};
    use egui::FontFamily;

    fn line(start: i32, text: &str) -> Line {
        Line { start, dur: 2000, text: text.to_owned(), ..Line::default() }
    }

    fn lyrics(lines: Vec<Line>) -> Lyrics {
        let sync = if lines.iter().all(|l| l.start == 0) { Sync::None } else { Sync::Line };
        Lyrics { lines, sync, provider: Provider::LrclibSynced, language: None, songwriters: vec![] }
    }

    // -------------------------------------------------------------- the steps

    #[test]
    fn a_long_pressed_line_is_the_first_one_chosen() {
        let mut s = State::default();
        s.begin("song/lrclib", Some(7), true);
        assert!(s.is_open());
        assert_eq!((s.step, s.picks.lines()), (Step::Lines, &[7][..]));
        assert_eq!(s.reveal, Some(7), "the list opens on it");
    }

    #[test]
    fn a_long_press_starts_the_choice_over() {
        let mut s = State::default();
        s.begin("song/lrclib", Some(2), true);
        s.act(Ask::Tap(3));
        s.act(Ask::Close);
        s.begin("song/lrclib", Some(9), true);
        assert_eq!(s.picks.lines(), [9]);
    }

    #[test]
    fn from_the_menu_nothing_is_chosen_yet_and_the_list_opens_at_the_line_being_sung() {
        let mut s = State::default();
        s.begin("song/lrclib", Some(4), false);
        assert!(s.is_open() && s.picks.is_empty());
        assert_eq!(s.reveal, Some(4));
    }

    #[test]
    fn opened_again_from_the_menu_the_songs_choice_is_still_there() {
        let mut s = State::default();
        s.begin("song/lrclib", None, false);
        s.act(Ask::Tap(3));
        s.act(Ask::Tap(1));
        s.act(Ask::Close);
        assert!(!s.is_open());
        s.begin("song/lrclib", Some(12), false);
        assert_eq!(s.picks.lines(), [1, 3]);
        assert_eq!(s.step, Step::Lines);
    }

    #[test]
    fn taps_choose_lines_and_a_fifth_one_is_refused() {
        let mut s = State::default();
        s.begin("song/lrclib", None, false);
        for l in [5, 1, 3, 8] {
            assert_eq!(s.act(Ask::Tap(l)), None);
        }
        assert_eq!(s.act(Ask::Tap(9)), Some(Effect::Full));
        assert_eq!(s.picks.lines(), [1, 3, 5, 8]);
        assert_eq!(s.act(Ask::Tap(3)), None, "a chosen one is given up");
        assert_eq!(s.picks.lines(), [1, 5, 8]);
    }

    #[test]
    fn the_card_needs_a_line() {
        let mut s = State::default();
        s.begin("song/lrclib", None, false);
        s.act(Ask::Next);
        assert_eq!(s.step, Step::Lines);
        s.act(Ask::Tap(2));
        s.act(Ask::Next);
        assert_eq!(s.step, Step::Card);
    }

    #[test]
    fn a_card_that_could_not_be_drawn_is_asked_for_again_on_coming_back_to_it() {
        let mut s = State::default();
        s.begin("song/lrclib", None, false);
        s.act(Ask::Tap(2));
        s.act(Ask::Next);
        s.picture.asked = Some("card".to_owned());
        s.picture.failed = Some(("card".to_owned(), "lyric-card timed out".to_owned()));
        s.act(Ask::ChangeLines);
        assert!(s.picture.failed.is_some(), "nothing is tried while the lines are up");
        s.act(Ask::Next);
        assert_eq!((s.picture.asked.as_deref(), s.picture.failed.is_none()), (None, true));
    }

    #[test]
    fn a_card_that_was_drawn_is_not_drawn_again_for_coming_back_to_it() {
        let mut s = State::default();
        s.begin("song/lrclib", None, false);
        s.act(Ask::Tap(2));
        s.act(Ask::Next);
        s.picture.asked = Some("card".to_owned());
        s.act(Ask::ChangeLines);
        s.act(Ask::Next);
        assert_eq!(s.picture.asked.as_deref(), Some("card"));
    }

    #[test]
    fn the_lines_can_be_changed_from_the_card() {
        let mut s = State::default();
        s.begin("song/lrclib", None, false);
        s.act(Ask::Tap(6));
        s.act(Ask::Tap(2));
        s.act(Ask::Next);
        s.act(Ask::ChangeLines);
        assert_eq!(s.step, Step::Lines);
        assert_eq!(s.reveal, Some(2), "the list opens on the first line chosen");
        assert_eq!(s.picks.lines(), [2, 6], "the choice is kept to change");
    }

    #[test]
    fn back_goes_from_the_card_to_its_lines_and_from_the_lines_out() {
        let mut s = State::default();
        s.begin("song/lrclib", None, false);
        s.act(Ask::Tap(2));
        s.act(Ask::Next);
        s.back();
        assert!(s.is_open());
        assert_eq!(s.step, Step::Lines);
        s.back();
        assert!(!s.is_open());
    }

    #[test]
    fn a_style_and_a_format_are_picked_and_kept() {
        let mut s = State::default();
        assert_eq!((s.style, s.format), (Style::Cover, Format::Square));
        assert_eq!(s.act(Ask::Style(Style::Violet)), None);
        assert_eq!(s.act(Ask::Format(Format::Story)), None);
        assert_eq!((s.style, s.format), (Style::Violet, Format::Story));
    }

    #[test]
    fn saving_and_sharing_are_the_apps_to_do() {
        let mut s = State::default();
        assert_eq!(s.act(Ask::Save), Some(Effect::Save));
        assert_eq!(s.act(Ask::Share), Some(Effect::Share));
    }

    #[test]
    fn another_song_drops_the_choice_and_goes_back_to_choosing() {
        let mut s = State::default();
        s.begin("song-a/lrclib", None, false);
        s.act(Ask::Tap(2));
        s.act(Ask::Next);
        s.follow("song-a/lrclib");
        assert_eq!(s.step, Step::Card, "the same song changes nothing");
        s.follow("song-b/lrclib");
        assert_eq!(s.step, Step::Lines);
        assert!(s.picks.is_empty());
        assert!(s.is_open(), "it stays up, on the new song's lines");
    }

    #[test]
    fn the_list_opens_at_the_line_being_sung() {
        let l = lyrics(vec![line(1000, "one"), line(4000, "two"), Line { instrumental: true, ..line(7000, "") }, line(20000, "three")]);
        assert_eq!(sung_now(&l, 0), None, "before the first line: the top");
        assert_eq!(sung_now(&l, 1000), Some(0));
        assert_eq!(sung_now(&l, 5500), Some(1));
        assert_eq!(sung_now(&l, 9000), Some(1), "in a break, the line sung before it");
        assert_eq!(sung_now(&l, 60000), Some(3));
    }

    #[test]
    fn lyrics_without_timing_open_at_the_top() {
        let l = lyrics(vec![line(0, "one"), line(0, "two")]);
        assert_eq!(sung_now(&l, 30000), None);
    }

    // -------------------------------------------------------------- the long press

    /// The lyrics view in a headless egui with a finger put on its middle for `held`
    /// seconds: the seek and the long press it reported, frame by frame.
    fn finger_on_the_lyrics(held: f64) -> Vec<(Option<f64>, Option<usize>)> {
        use super::super::karaoke;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "Inter");
        let l = lyrics((0..12).map(|i| line(i * 3000, "Biar hujan turun perlahan")).collect());
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(412.0, 700.0));
        let at = screen.center();
        let touch = |phase| egui::Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId(1), phase, pos: at, force: None };
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut view = karaoke::View::default();
        let (mut time, mut reported) = (0.0, Vec::new());
        let mut frame = |time: f64, events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), time: Some(time), events, ..Default::default() };
            let mut out = None;
            let run = ctx.run_ui(input, |ui| {
                let f = karaoke::Frame { lyrics: &l, key: 1, roman: vec![], trans: vec![], t: 16.0, playing: false, now: time, rect: screen, style: &crate::preset::active().lyrics, accent: Color32::WHITE, centered: false, scale: 1.0 };
                out = Some(view.show(ui, f));
            });
            run.drop_without_applying_deltas();
            let out = out.expect("drawn");
            reported.push((out.seek, out.long_press));
        };
        for _ in 0..3 {
            time += 0.1;
            frame(time, vec![]);
        }
        time += 0.1;
        let down = time;
        frame(time, vec![touch(egui::TouchPhase::Start), egui::Event::PointerMoved(at), button(true)]);
        while time - down < held {
            time += 0.1;
            frame(time, vec![]);
        }
        time += 0.02;
        frame(time, vec![touch(egui::TouchPhase::End), button(false), egui::Event::PointerGone]);
        for _ in 0..3 {
            time += 0.1;
            frame(time, vec![]);
        }
        reported
    }

    #[test]
    fn a_tap_on_a_lyric_line_seeks_and_is_no_long_press() {
        let reported = finger_on_the_lyrics(0.1);
        assert_eq!(reported.iter().filter(|(seek, _)| seek.is_some()).count(), 1, "{reported:?}");
        assert!(reported.iter().all(|(_, held)| held.is_none()), "{reported:?}");
    }

    #[test]
    fn a_finger_held_on_a_lyric_line_is_a_long_press_and_never_seeks() {
        let reported = finger_on_the_lyrics(1.5);
        assert_eq!(reported.iter().filter(|(_, held)| held.is_some()).count(), 1, "once, on the line under the finger: {reported:?}");
        assert!(reported.iter().all(|(seek, _)| seek.is_none()), "{reported:?}");
    }

    // -------------------------------------------------------------- the pages

    /// The phones the pages must fit: the screen less the system's bars, from a small
    /// 360×640 one to the designs' 412×900.
    const AREAS: [(f32, f32); 4] = [(360.0, 600.0), (360.0, 740.0), (393.0, 800.0), (412.0, 844.0)];
    const CHIPS: [f32; 3] = [102.0, 98.0, 98.0];

    fn area(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 36.0), vec2(w, h))
    }

    fn apart(a: Rect, b: Rect) -> bool {
        !a.intersects(b) || a.intersect(b).width() <= 0.01 || a.intersect(b).height() <= 0.01
    }

    #[test]
    fn the_card_page_fits_every_phone_in_every_format() {
        for (w, h) in AREAS {
            for format in Format::ALL {
                let a = area(w, h);
                let g = card_geom(a, format, CHIPS);
                let mut parts = vec![g.card, g.hint, g.save, g.share];
                parts.extend(g.styles);
                parts.extend(g.formats);
                for (i, p) in parts.iter().enumerate() {
                    assert!(a.expand(0.01).contains_rect(*p), "{format:?} on {w}x{h}: {p:?} is off the screen");
                    for q in &parts[i + 1..] {
                        assert!(apart(*p, *q), "{format:?} on {w}x{h}: {p:?} and {q:?}");
                    }
                }
                assert!(g.card.top() >= a.top() + BAR_H, "the card is under the bar");
                for target in g.styles.iter().chain(&g.formats).chain([&g.save, &g.share]) {
                    assert!(target.width() >= m3::TOUCH && target.height() >= m3::TOUCH, "{format:?} on {w}x{h}: {target:?}");
                }
            }
        }
    }

    #[test]
    fn the_card_is_shown_in_its_own_shape_and_as_large_as_fits() {
        for (w, h) in AREAS {
            for format in Format::ALL {
                let g = card_geom(area(w, h), format, CHIPS);
                let (pw, ph) = format.size();
                assert!((g.card.width() / g.card.height() - pw as f32 / ph as f32).abs() < 0.001, "{format:?} on {w}x{h}: {:?}", g.card);
                assert!(g.card.width() >= 150.0, "{format:?} on {w}x{h}: {:?}", g.card);
            }
            // A wide card takes the width there is, a little in from the page's margins.
            let wide = card_geom(area(w, h), Format::Wide, CHIPS);
            assert!((wide.card.width() - (w - 2.0 * (SIDE + CARD_INSET))).abs() < 0.01, "{:?}", wide.card);
        }
    }

    #[test]
    fn save_and_share_are_side_by_side_and_as_wide_as_each_other() {
        let g = card_geom(area(412.0, 844.0), Format::Square, CHIPS);
        assert_eq!(g.save.width(), g.share.width());
        assert_eq!((g.save.top(), g.save.height()), (g.share.top(), BUTTON_H));
        assert!(g.save.right() < g.share.left());
        assert_eq!((g.save.left(), g.share.right()), (SIDE, 412.0 - SIDE));
    }

    #[test]
    fn the_lines_page_keeps_its_button_under_the_list() {
        for (w, h) in AREAS {
            let a = area(w, h);
            let g = lines_geom(a);
            for p in [g.hint, g.list, g.next] {
                assert!(a.expand(0.01).contains_rect(p), "{p:?}");
            }
            assert!(g.hint.top() >= a.top() + BAR_H && g.hint.bottom() <= g.list.top() && g.list.bottom() <= g.next.top());
            assert_eq!(g.next.height(), BUTTON_H);
            assert!(g.list.height() >= 300.0, "room for a good few lines: {:?}", g.list);
        }
    }

    /// A page in a headless egui the size of a phone, with stand-ins for the app's fonts.
    struct Screen {
        ctx: egui::Context,
        time: f64,
        area: Rect,
        asked: Vec<Ask>,
    }

    impl Screen {
        fn new() -> Screen {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let plain = fonts.families[&FontFamily::Proportional].clone();
            for family in ["semibold", "bold", "icons"] {
                fonts.families.insert(FontFamily::Name(family.into()), plain.clone());
            }
            ctx.set_fonts(fonts);
            Screen { ctx, time: 0.0, area: area(412.0, 844.0), asked: Vec::new() }
        }

        fn frame(&mut self, events: Vec<egui::Event>, page: &mut dyn FnMut(&mut Ui, Rect) -> Option<Ask>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(412.0, 900.0))), time: Some(self.time), events, ..Default::default() };
            let mut asked = None;
            let area = self.area;
            let out = self.ctx.run_ui(input, |ui| asked = page(ui, area));
            out.drop_without_applying_deltas();
            self.asked.extend(asked);
        }

        fn tap_at(&mut self, at: Pos2, page: &mut dyn FnMut(&mut Ui, Rect) -> Option<Ask>) {
            self.frame(vec![], page);
            self.frame(vec![], page);
            self.frame(vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }], page);
            self.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }], page);
            self.frame(vec![], page);
        }

        fn tap(&mut self, id: egui::Id, page: &mut dyn FnMut(&mut Ui, Rect) -> Option<Ask>) {
            self.frame(vec![], page);
            self.frame(vec![], page);
            let at = self.ctx.read_response(id).expect("it is on the page").rect.center();
            self.tap_at(at, page);
        }
    }

    fn song() -> Vec<(usize, String)> {
        ["Lampu kota mulai menyala", "Kau masih di ujung cerita", "Biar hujan turun perlahan", "Aku pulang lewat jalan yang sama", "مرحبا بالعالم"].iter().enumerate().map(|(i, t)| (i * 2, (*t).to_owned())).collect()
    }

    fn lines_with(picks: &Selection) -> impl FnMut(&mut Ui, Rect) -> Option<Ask> + '_ {
        let lines = song();
        move |ui, area| lines_page(ui, area, &LinesView { lines: &lines, picks }, &mut None)
    }

    #[test]
    fn a_tap_on_a_line_asks_for_that_line() {
        let picks = Selection::default();
        let mut s = Screen::new();
        s.tap(line_id(4), &mut lines_with(&picks));
        assert_eq!(s.asked, [Ask::Tap(4)], "its place in the lyrics, not in the list");
    }

    #[test]
    fn a_chosen_line_can_be_tapped_again() {
        let mut picks = Selection::default();
        picks.tap(2);
        let mut s = Screen::new();
        s.tap(line_id(2), &mut lines_with(&picks));
        assert_eq!(s.asked, [Ask::Tap(2)]);
    }

    #[test]
    fn continue_waits_for_a_line_to_be_chosen() {
        let mut picks = Selection::default();
        let mut s = Screen::new();
        s.tap(next_id(), &mut lines_with(&picks));
        assert!(s.asked.is_empty(), "{:?}", s.asked);
        picks.tap(6);
        s.tap(next_id(), &mut lines_with(&picks));
        assert_eq!(s.asked, [Ask::Next]);
    }

    #[test]
    fn every_line_is_tall_enough_to_tap_and_none_overlaps_another() {
        let picks = Selection::default();
        let mut s = Screen::new();
        let mut page = lines_with(&picks);
        s.frame(vec![], &mut page);
        s.frame(vec![], &mut page);
        let rows: Vec<Rect> = song().iter().map(|(i, _)| s.ctx.read_response(line_id(*i)).expect("drawn").rect).collect();
        for (i, a) in rows.iter().enumerate() {
            assert!(a.height() >= m3::TOUCH, "{a:?}");
            for b in &rows[i + 1..] {
                assert!(apart(*a, *b), "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn the_close_button_closes_either_page() {
        let picks = Selection::default();
        let mut s = Screen::new();
        s.tap(close_id(), &mut lines_with(&picks));
        assert_eq!(s.asked, [Ask::Close]);
        let mut s = Screen::new();
        s.tap(close_id(), &mut card_with(Format::Square, Sending::Ready));
        assert_eq!(s.asked, [Ask::Close]);
    }

    /// The card page while its picture is being drawn, its swatches made as the app makes them.
    fn card_with(format: Format, sending: Sending) -> impl FnMut(&mut Ui, Rect) -> Option<Ask> {
        let mut st = State::default();
        move |ui, area| {
            let tints = Style::ALL.map(|style| st.tint(ui.ctx(), style.colors(Color32::from_rgb(220, 90, 60))));
            card_page(ui, area, &CardView { preview: Preview::Drawing, style: Style::Cover, format, tints, sending })
        }
    }

    #[test]
    fn every_style_can_be_picked() {
        for style in Style::ALL {
            let mut s = Screen::new();
            s.tap(style_id(style), &mut card_with(Format::Square, Sending::Ready));
            assert_eq!(s.asked, [Ask::Style(style)]);
        }
    }

    #[test]
    fn every_format_can_be_picked() {
        for (i, format) in Format::ALL.into_iter().enumerate() {
            let mut s = Screen::new();
            let mut page = card_with(Format::Square, Sending::Ready);
            let mut chips = [0.0; 3];
            s.frame(vec![], &mut |ui, area| {
                chips = chip_widths(ui, Format::Square);
                page(ui, area)
            });
            let at = card_geom(s.area, Format::Square, chips).formats[i].center();
            s.tap_at(at, &mut page);
            assert_eq!(s.asked, [Ask::Format(format)]);
        }
    }

    #[test]
    fn a_tap_on_the_card_goes_back_to_its_lines() {
        let mut s = Screen::new();
        s.tap(card_id(), &mut card_with(Format::Story, Sending::Ready));
        assert_eq!(s.asked, [Ask::ChangeLines]);
    }

    #[test]
    fn save_and_share_send_the_picture_once_it_is_drawn() {
        let mut s = Screen::new();
        s.tap(save_id(), &mut card_with(Format::Square, Sending::Ready));
        s.tap(share_id(), &mut card_with(Format::Square, Sending::Ready));
        assert_eq!(s.asked, [Ask::Save, Ask::Share]);
    }

    #[test]
    fn save_and_share_wait_while_the_picture_is_being_drawn() {
        let mut s = Screen::new();
        s.tap(save_id(), &mut card_with(Format::Square, Sending::Waiting));
        s.tap(share_id(), &mut card_with(Format::Square, Sending::Waiting));
        assert!(s.asked.is_empty(), "{:?}", s.asked);
    }

    #[test]
    fn where_nothing_can_draw_the_picture_there_is_no_save_or_share() {
        let mut s = Screen::new();
        let mut page = card_with(Format::Square, Sending::Unavailable);
        s.frame(vec![], &mut page);
        s.frame(vec![], &mut page);
        assert!(s.ctx.read_response(save_id()).is_none() && s.ctx.read_response(share_id()).is_none());
    }

    #[test]
    fn a_styles_gradient_is_made_once_and_kept() {
        let ctx = egui::Context::default();
        let mut st = State::default();
        let teal = Style::Teal.colors(Color32::WHITE);
        let first = st.tint(&ctx, teal);
        assert_eq!(st.tint(&ctx, teal), first);
        assert_ne!(st.tint(&ctx, Style::Dark.colors(Color32::WHITE)), first);
        assert_eq!(st.tints.len(), 2);
    }
}
