//! Now Playing on the phone, in Material 3: the artwork over a gradient in the song's
//! colour, with its title, the wavy seek bar, the transport, the player's tools and a
//! peek at what plays next; and the lyrics and the queue as panels of their own, under
//! a compact header and over a compact transport. Where each view's parts are is a
//! plain function of the screen, so it can be checked without drawing.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};

use super::widgets::{self, named};
use super::{Action, App, NpTab, Repeat, Target, glyphs, m3, nowplaying, playon, sleep};
use crate::material::{self, Type, Weight, shape};
use crate::model::Track;
use crate::platform::Insets;
use crate::playon::Output;
use crate::theme::{self, icon};

/// The widest the player's column gets, on a screen wider than a phone's.
const MAX_W: f32 = 480.0;
/// The top row, the title's two lines, the seek bar's finger room, the times under it
/// and the row of tools: as tall on every screen.
const TOP_H: f32 = 52.0;
const TEXT_H: f32 = 60.0;
const SEEK_H: f32 = m3::TOUCH;
const TIMES_H: f32 = 16.0;
const TOOLS_H: f32 = m3::TOUCH;
/// How tall the wavy bar itself is drawn.
const WAVE_H: f32 = 22.0;
/// The play button, the peek at what is next and the gaps between the rows, on a
/// screen with room and on one without: a short screen takes them down together.
const PLAY: (f32, f32) = (88.0, 72.0);
const PEEK: (f32, f32) = (100.0, 72.0);
/// How large the marks of the transport's side buttons (shuffle, repeat) are drawn:
/// the app's own glyphs keep a margin inside their box.
const SIDE_GLYPH: f32 = 32.0;
const GAPS: ([f32; 6], [f32; 6]) = ([9.0, 22.0, 3.0, 10.0, 16.0, 16.0], [4.0, 12.0, 0.0, 6.0, 8.0, 8.0]);

// ------------------------------------------------------------------ geometry

/// The transport's five touch targets (a side button, previous, play, next, a side
/// button) and how large previous/next and play are drawn in theirs.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Transport {
    targets: [Rect; 5],
    skip: f32,
    play: f32,
}

/// Lay the transport out across `row`, its play button `play` across.
fn transport(row: Rect, play: f32) -> Transport {
    let skip = if row.width() >= 340.0 { 64.0 } else { 56.0 };
    let sizes = [m3::TOUCH, skip, play, skip, m3::TOUCH];
    let gap = ((row.width() - sizes.iter().sum::<f32>()) / 4.0).clamp(0.0, 16.0);
    let mut x = row.center().x - (sizes.iter().sum::<f32>() + 4.0 * gap) / 2.0;
    let targets = sizes.map(|s| {
        let r = Rect::from_center_size(pos2(x + s / 2.0, row.center().y), Vec2::splat(s));
        x += s + gap;
        r
    });
    Transport { targets, skip, play }
}

/// The play button's corner: a circle while paused, squarer as `playing` goes to 1.
fn play_radius(size: f32, playing: f32) -> f32 {
    egui::lerp(size / 2.0..=size * 0.32, playing.clamp(0.0, 1.0))
}

/// The column the player is laid out in: the screen less its insets, no wider than `MAX_W`.
fn column(screen: Rect, insets: Insets) -> Rect {
    let (left, right) = (screen.left() + insets.left, screen.right() - insets.right);
    let w = (right - left).min(MAX_W);
    let cx = (left + right) / 2.0;
    Rect::from_min_max(pos2(cx - w / 2.0, screen.top() + insets.top), pos2(cx + w / 2.0, screen.bottom() - insets.bottom))
}

/// Where the parts of the artwork view are.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ArtGeom {
    /// The top row: the button that closes the player, what the queue plays from, the menu.
    collapse: Rect,
    source: Rect,
    menu: Rect,
    art: Rect,
    /// The title over the artists, and the like button beside them.
    text: Rect,
    like: Rect,
    /// The wavy bar, and the two times under it.
    seek: Rect,
    times: Rect,
    transport: Transport,
    /// The row of tools (Lyrics, where the music plays, Sleep timer).
    tools: Rect,
    /// The peek at what plays next, down to the screen's bottom edge.
    peek: Rect,
    /// How much of the way from a roomy screen (0) to a short one (1) the rows were taken.
    squeeze: f32,
}

fn art_geom(screen: Rect, insets: Insets) -> ArtGeom {
    let col = column(screen, insets);
    let side = (col.width() * 0.09).round();
    let fixed = |s: f32| {
        let gaps: f32 = GAPS.0.iter().zip(GAPS.1).map(|(a, b)| egui::lerp(*a..=b, s)).sum();
        TOP_H + TEXT_H + SEEK_H + TIMES_H + TOOLS_H + egui::lerp(PLAY.0..=PLAY.1, s) + egui::lerp(PEEK.0..=PEEK.1, s) + gaps
    };
    // The rows are taken down only as far as the artwork needs to keep a fair size.
    let want = (col.width() - 2.0 * side).min(280.0);
    let squeeze = ((want - (col.height() - fixed(0.0))) / (fixed(0.0) - fixed(1.0))).clamp(0.0, 1.0);
    let gap = |i: usize| egui::lerp(GAPS.0[i]..=GAPS.1[i], squeeze);
    let room = (col.height() - fixed(squeeze)).max(0.0);
    let art_side = (col.width() - 2.0 * side).min(room);
    let spare = room - art_side;

    let target = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), Vec2::splat(m3::TOUCH));
    let top = col.top();
    let collapse = target(col.left() + 8.0, top + (TOP_H - m3::TOUCH) / 2.0);
    let menu = target(col.right() - 8.0 - m3::TOUCH, top + (TOP_H - m3::TOUCH) / 2.0);
    let source = Rect::from_min_max(pos2(collapse.right() + 4.0, top), pos2(menu.left() - 4.0, top + TOP_H));
    let mut y = top + TOP_H + gap(0) + spare / 2.0;
    let art = Rect::from_min_size(pos2(col.center().x - art_side / 2.0, y), Vec2::splat(art_side));
    y = art.bottom() + gap(1) + spare / 2.0;
    let like = target(col.right() - side - m3::TOUCH, y + (TEXT_H - m3::TOUCH) / 2.0);
    let text = Rect::from_min_max(pos2(col.left() + side, y), pos2(like.left() - 12.0, y + TEXT_H));
    y += TEXT_H + gap(2);
    let seek = Rect::from_min_max(pos2(col.left() + side - 4.0, y + (SEEK_H - WAVE_H) / 2.0), pos2(col.right() - side + 4.0, y + (SEEK_H + WAVE_H) / 2.0));
    y += SEEK_H;
    let times = Rect::from_min_max(pos2(seek.left(), y), pos2(seek.right(), y + TIMES_H));
    y += TIMES_H + gap(3);
    let play = egui::lerp(PLAY.0..=PLAY.1, squeeze);
    let row = Rect::from_min_max(pos2(col.left() + 20.0, y), pos2(col.right() - 20.0, y + play));
    y += play + gap(4);
    let tools = Rect::from_min_max(pos2(col.left() + side - 12.0, y), pos2(col.right() - side + 12.0, y + TOOLS_H));
    y += TOOLS_H + gap(5);
    let peek = Rect::from_min_max(pos2(col.left(), y), pos2(col.right(), screen.bottom()));
    ArtGeom { collapse, source, menu, art, text, like, seek, times, transport: transport(row, play), tools, peek, squeeze }
}

/// Where the parts of the peek at what plays next are, in `peek` (the gesture area
/// under it left out): the handle, the "Up next" label when there is room for it, the
/// cover, the two lines of text and the queue mark.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PeekGeom {
    handle: Rect,
    label: Option<Pos2>,
    cover: Rect,
    text: Rect,
    mark: Pos2,
}

fn peek_geom(peek: Rect) -> PeekGeom {
    let roomy = peek.height() >= 86.0;
    let handle = Rect::from_center_size(pos2(peek.center().x, peek.top() + if roomy { 12.0 } else { 9.0 }), vec2(32.0, 4.0));
    let label = roomy.then(|| pos2(peek.left() + 24.0, peek.top() + 24.0));
    let rows_top = if roomy { peek.top() + 42.0 } else { handle.bottom() + 4.0 };
    let cy = (rows_top + peek.bottom()) / 2.0;
    let cover = Rect::from_center_size(pos2(peek.left() + 24.0 + 22.0, cy), Vec2::splat(44.0));
    let mark = pos2(peek.right() - 24.0 - 12.0, cy);
    PeekGeom { handle, label, cover, text: Rect::from_min_max(pos2(cover.right() + 14.0, cy - 22.0), pos2(mark.x - 12.0 - 12.0, cy + 22.0)), mark }
}

/// The room `tools` leaves between two tools.
const TOOL_GAP: f32 = 8.0;
/// The least of the output tool's name worth showing: a name with less room than this
/// is left out and the tool is its icon alone.
const OUTPUT_WORDS: f32 = 64.0;

/// The player's tools side by side in `row`, each as wide as it asks, the group in the
/// row's middle. Tools that together ask for more than the row has share it in proportion.
fn tools(row: Rect, widths: &[f32]) -> Vec<Rect> {
    let gaps = TOOL_GAP * widths.len().saturating_sub(1) as f32;
    let asked: f32 = widths.iter().sum();
    let k = if asked > 0.0 { ((row.width() - gaps) / asked).min(1.0) } else { 1.0 };
    let mut x = row.center().x - (asked * k + gaps) / 2.0;
    widths
        .iter()
        .map(|w| {
            let r = Rect::from_min_size(pos2(x, row.top()), vec2(w * k, row.height()));
            x += w * k + TOOL_GAP;
            r
        })
        .collect()
}

/// How wide the tool for where the music plays is in a row `row` wide whose other two
/// tools ask for `others` together. It asks for `want`, which fits it with its name
/// whole; with less room than that its name is cut short, as long as `chrome` (what its
/// icon and padding take) leaves `OUTPUT_WORDS` for it; with less still it is its icon alone.
fn output_width(row: f32, others: f32, want: f32, chrome: f32) -> f32 {
    let room = row - others - 2.0 * TOOL_GAP;
    if room >= want {
        want
    } else if room - chrome >= OUTPUT_WORDS {
        room
    } else {
        m3::TOOL_ICON_W
    }
}

/// The row of tools as it is drawn: where Lyrics, where the music plays and the sleep
/// timer are, and whether the middle one has room for its name.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ToolRow {
    rects: [Rect; 3],
    words: bool,
}

/// Lay the tools out in `row`, the output tool named `output` and the sleep timer's
/// label `sleep`.
fn tool_row(ui: &Ui, row: Rect, output: &str, sleep: &str) -> ToolRow {
    let (lyrics, sleep) = (m3::tool_chip_width(ui, "Lyrics"), m3::tool_chip_width(ui, sleep));
    let chrome = m3::tool_chip_width(ui, "");
    let middle = output_width(row.width(), lyrics + sleep, m3::tool_chip_width(ui, output), chrome);
    // With no room even for the icon the other two share what it leaves.
    let k = ((row.width() - 2.0 * TOOL_GAP - middle) / (lyrics + sleep)).min(1.0);
    let rects = tools(row, &[lyrics * k, middle, sleep * k]);
    ToolRow { rects: [rects[0], rects[1], rects[2]], words: middle > m3::TOOL_ICON_W }
}

/// The tool a tap landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Lyrics,
    /// Where the music plays: the "Play on" sheet.
    Output,
    Sleep,
}

/// Draw the player's tools in `row`, and say which was tapped. `sleep` is the sleep
/// timer's label and whether a timer runs.
fn tools_view(ui: &Ui, row: Rect, id: egui::Id, output: &Output, sleep: (&str, bool)) -> Option<Tool> {
    let layout = tool_row(ui, row, output.title(), sleep.0);
    let tool = |i: usize| id.with(("tool", i));
    let mut tapped = None;
    if m3::tool_chip(ui, layout.rects[0], glyphs::LYRICS.into(), "Lyrics", false, tool(0)).clicked() {
        tapped = Some(Tool::Lyrics);
    }
    let mark = playon::mark(output.kind);
    let middle = if layout.words { m3::tool_chip(ui, layout.rects[1], mark, output.title(), false, tool(1)) } else { m3::tool_chip_icon(ui, layout.rects[1], mark, &format!("Play on, {}", output.title()), false, tool(1)) };
    if middle.clicked() {
        tapped = Some(Tool::Output);
    }
    if m3::tool_chip(ui, layout.rects[2], m3::Icon::Drawn(sleep::moon), sleep.0, sleep.1, tool(2)).clicked() {
        tapped = Some(Tool::Sleep);
    }
    tapped
}

/// Where the parts of a panel (the lyrics, the queue) are: the playing song's cover
/// and its two lines, the button back to the artwork, the panel's body, and under it
/// the wavy bar, the times and the compact transport.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PanelGeom {
    cover: Rect,
    text: Rect,
    close: Rect,
    body: Rect,
    seek: Rect,
    times: Rect,
    transport: Transport,
}

fn panel_geom(screen: Rect, insets: Insets) -> PanelGeom {
    let col = column(screen, insets);
    let side = (col.width() * 0.05).round();
    let top = col.top() + 8.0;
    let cover = Rect::from_min_size(pos2(col.left() + side, top), Vec2::splat(48.0));
    let close = Rect::from_min_size(pos2(col.right() - side + 8.0 - m3::TOUCH, top), Vec2::splat(m3::TOUCH));
    let text = Rect::from_min_max(pos2(cover.right() + 12.0, top), pos2(close.left() - 8.0, top + 48.0));
    let row = Rect::from_min_max(pos2(col.left() + 20.0, col.bottom() - 12.0 - 72.0), pos2(col.right() - 20.0, col.bottom() - 12.0));
    let times = Rect::from_min_max(pos2(col.left() + side, row.top() - 16.0 - TIMES_H), pos2(col.right() - side, row.top() - 16.0));
    let seek = Rect::from_min_max(pos2(times.left(), times.top() - (SEEK_H + WAVE_H) / 2.0), pos2(times.right(), times.top() - (SEEK_H - WAVE_H) / 2.0));
    let body_top = cover.bottom() + 12.0;
    let body = Rect::from_min_max(pos2(col.left() + side - 6.0, body_top), pos2(col.right() - side + 6.0, (times.top() - SEEK_H).max(body_top)));
    PanelGeom { cover, text, close, body, seek, times, transport: transport(row, 72.0) }
}

// ------------------------------------------------------------------ the player

/// The collection the queue plays from, by its title, when it is one the app has open.
fn playing_from(app: &App) -> Option<String> {
    match app.queue_source.as_ref()? {
        Target::Album(id) | Target::Playlist(id) => app.lists.get(id).map(|o| o.c.title.clone()).filter(|t| !t.is_empty()),
        _ => None,
    }
}

/// The gradient behind the player: the song's colour at the top, the surface below.
/// A new song's colour comes in over a moment.
pub(super) fn background(app: &App, ctx: &egui::Context, painter: &egui::Painter, rect: Rect) {
    let (top, bottom) = material::song_backdrop(app.accent);
    let ch = |i: usize, v: u8| ctx.animate_value_with_time(egui::Id::unique(("np-backdrop", i)), v as f32, 0.6).round() as u8;
    let top = Color32::from_rgb(ch(0, top.r()), ch(1, top.g()), ch(2, top.b()));
    let split = rect.top() + rect.height() * 0.6;
    super::shell::vgradient(painter, Rect::from_min_max(rect.min, pos2(rect.right(), split)), top, bottom);
    painter.rect_filled(Rect::from_min_max(pos2(rect.left(), split), rect.max), 0.0, bottom);
}

/// Draw the player over `rect` (the whole screen, sliding up): the artwork view, or
/// the panel that was asked for.
pub(super) fn show(app: &mut App, ui: &mut Ui, rect: Rect, now: f64) {
    let Some(current) = app.current().cloned() else { return };
    // An Automix handoff keeps the outgoing song here until the next one is the louder
    // of the two: its title, seek bar and lyrics lag the queue's switch.
    let held = app.mix_hold().is_some();
    let track = if held { app.shown().cloned().unwrap_or(current) } else { current };
    let heard = if held { track.clone() } else { app.playing_track().cloned().unwrap_or_else(|| track.clone()) };
    let ctx = ui.ctx().clone();
    let insets = crate::platform::insets(&ctx);
    let tab = nowplaying::view(app, &ctx);
    // The panel last shown is the one that fades out when the artwork returns.
    let last_id = egui::Id::unique("np-phone-last-panel");
    let panel = if tab == NpTab::Art {
        ctx.data(|d| d.get_temp(last_id)).unwrap_or(NpTab::Lyrics)
    } else {
        ctx.data_mut(|d| d.insert_temp(last_id, tab));
        tab
    };
    // One view at a time takes the touches: the old one fades out, then the new one in.
    let p = ctx.animate_bool_with_time(egui::Id::unique("np-phone-panel"), tab != NpTab::Art, 0.24 * theme::SPEED);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    if p < 0.5 {
        child.multiply_opacity(theme::ease_out(1.0 - 2.0 * p));
        art_view(app, &mut child, rect, insets, &track, &heard, now);
    } else {
        child.multiply_opacity(theme::ease_out(2.0 * p - 1.0));
        panel_view(app, &mut child, rect, insets, panel, &track, &heard, now);
    }
}

fn art_view(app: &mut App, ui: &mut Ui, screen: Rect, insets: Insets, track: &Track, heard: &Track, now: f64) {
    let m = material::scheme();
    let g = art_geom(screen, insets);
    let id = ui.scope_id();

    // The top row.
    if m3::IconButton::new(icon::CHEVRON_DOWN, "Close now playing").show_at(ui, g.collapse.center(), id.with("collapse")).clicked() {
        app.np_open = false;
    }
    let (over, name) = match playing_from(app) {
        Some(name) => ("PLAYING FROM", name),
        None => ("NOW PLAYING", String::new()),
    };
    let over_g = m3::galley(ui, over, Type::LabelMedium, m.on_surface_variant, g.source.width());
    let name_g = m3::galley(ui, &name, Type::TitleSmall, m.on_surface, g.source.width());
    let lines = over_g.size().y + if name.is_empty() { 0.0 } else { name_g.size().y };
    let y = g.source.center().y - lines / 2.0;
    let name_y = y + over_g.size().y;
    ui.painter().galley(pos2(g.source.center().x - over_g.size().x / 2.0, y), over_g, m.on_surface_variant);
    if !name.is_empty() {
        ui.painter().galley(pos2(g.source.center().x - name_g.size().x / 2.0, name_y), name_g, m.on_surface);
    }
    let menu = m3::IconButton::new(icon::ELLIPSIS_VERTICAL, "Options").show_at(ui, g.menu.center(), id.with("options"));
    nowplaying::options_popup(app, &menu, heard, false);

    // The artwork.
    widgets::soft_shadow(ui, g.art, shape::ARTWORK, 40.0, 0.35);
    let px = (g.art.width() * ui.ctx().pixels_per_point()) as u32;
    widgets::paint_cover_xfade_px(ui, egui::Id::unique("np-art"), g.art, &track.thumbs, shape::ARTWORK, 1.0, px);
    // Apple's motion sleeve, over the still until its first frame arrives.
    if let Some(album) = track.album.as_ref().map(|a| a.name.clone()).filter(|a| !a.is_empty()) {
        let artist = track.artists.first().map(|a| a.name.clone()).unwrap_or_else(|| track.artist_line());
        app.apple_cover(&album, &artist, Some(&track.title));
        if let Some(s) = app.apple_sprite(ui.ctx(), &album, &artist, Some(&track.title), px) {
            widgets::paint_motion(ui, g.art, shape::ARTWORK, &s);
        }
    }
    // A handoff still shows the outgoing song; the stats are the incoming one's.
    if app.mix_hold().is_none() {
        nowplaying::nerd_stats(app, ui, g.art, false);
    }

    // The title over the artists (each opens their page, and so does the album after
    // the dash), and the like button.
    let title_role = if g.squeeze > 0.5 { Type::HeadlineSmall } else { Type::HeadlineMedium }.spec().weight(Weight::Medium);
    let title = m3::galley(ui, &track.title, title_role, m.on_surface, g.text.width());
    let line_font = Type::BodyLarge.font();
    let top = g.text.center().y - (title.size().y + Type::BodyLarge.spec().line_height) / 2.0;
    let line_y = top + title.size().y;
    ui.painter().galley(pos2(g.text.left(), top), title, m.on_surface);
    let mut line = track.artist_line();
    if let Some(album) = track.album.as_ref().filter(|a| !a.name.is_empty()) {
        line = format!("{line} — {}", album.name);
    }
    let line_rect = widgets::linked_text(ui, pos2(g.text.left(), line_y), &line, &track.artists, line_font.clone(), m.on_surface_variant, g.text.width(), &mut app.actions);
    nowplaying::album_link(app, ui, track, &line, line_rect, line_font, g.text.width());
    like_button(app, ui, g.like, heard);

    // The seek bar and the two times, the quality mark between them.
    let s = seek(app, ui, g.seek, track);
    times(app, ui, g.times, &s);

    // The transport.
    let t = g.transport;
    if m3::IconButton::new(glyphs::SHUFFLE, "Shuffle up next").glyph(SIDE_GLYPH).show_at(ui, t.targets[0].center(), id.with("shuffle")).clicked() {
        app.shuffle_upcoming();
    }
    middle(app, ui, &t, now);
    let (glyph, on) = match app.queue.repeat {
        Repeat::Off => (glyphs::REPEAT, false),
        Repeat::All => (glyphs::REPEAT, true),
        Repeat::One => (glyphs::REPEAT_ONE, true),
    };
    let style = if on { m3::IconStyle::Lit } else { m3::IconStyle::Standard };
    if m3::IconButton::new(glyph, "Repeat").style(style).size(m3::TOUCH).glyph(SIDE_GLYPH).show_at(ui, t.targets[4].center(), id.with("repeat")).clicked() {
        app.queue.repeat = match app.queue.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
    }

    // The tools: the lyrics, where the music plays (its name, or its icon alone when the
    // row is short of room) and the sleep timer.
    let (sleep_label, sleeping) = sleep::tool_label(app);
    match tools_view(ui, g.tools, id, &crate::playon::output(), (&sleep_label, sleeping)) {
        Some(Tool::Lyrics) => nowplaying::ask_lyrics(app, ui.ctx()),
        Some(Tool::Output) => app.playon.open = true,
        Some(Tool::Sleep) => sleep::open_sheet(app),
        None => {}
    }

    peek(app, ui, g.peek, insets.bottom);
}

/// The like button: a quiet disc, filled with the primary colour once the song is liked.
fn like_button(app: &mut App, ui: &Ui, rect: Rect, heard: &Track) {
    let m = material::scheme();
    let liked = app.liked.contains(&heard.id);
    let resp = named(ui.interact(rect, ui.scope_id().with(("np-like", &heard.id)), Sense::click()), if liked { "Remove from favourites" } else { "Add to favourites" });
    let pop = theme::anim_bool(ui.ctx(), ("liked", &heard.id), liked, 0.3);
    let ink = theme::lerp_color(m.on_surface, m.on_primary, pop);
    ui.painter().circle_filled(rect.center(), rect.width() / 2.0, theme::lerp_color(m.glass(0.1), m.primary, pop));
    if resp.is_pointer_button_down_on() {
        ui.painter().circle_filled(rect.center(), rect.width() / 2.0, ink.gamma_multiply(0.1));
    }
    glyphs::draw(ui, rect.center(), 24.0, glyphs::STAR, theme::with_alpha(ink, 1.0 - pop));
    glyphs::draw(ui, rect.center(), 24.0, glyphs::STAR_FILLED, theme::with_alpha(ink, pop));
    if resp.clicked() {
        app.actions.push(Action::Like(heard.clone(), !liked));
    }
}

/// The wavy seek bar in `bar`: wavy while the song plays, lying flat while it is
/// paused. A tap seeks to where it lands, and a finger drags the thumb, anywhere in a
/// finger's height round the bar.
fn seek(app: &mut App, ui: &Ui, bar: Rect, track: &Track) -> nowplaying::SeekState {
    let m = material::scheme();
    let s = nowplaying::seek_state(app, track);
    let live = s.total > 0.0 && !s.held;
    let resp = ui.interact(m3::wave_target(bar), ui.scope_id().with("np-seek"), if live { Sense::click_and_drag() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::slider(live, s.pos as f64, "Seek"));
    let frac = if s.total > 0.0 { (s.pos / s.total).clamp(0.0, 1.0) } else { 0.0 };
    let playing = app.playing();
    let wave = theme::anim_bool(ui.ctx(), "np-wave", playing, 0.4);
    // The wave runs toward the thumb while the song plays, and stops where it is.
    let phase_id = egui::Id::unique("np-wave-phase");
    let mut phase: f32 = ui.ctx().data(|d| d.get_temp(phase_id)).unwrap_or(0.0);
    if playing {
        phase = (phase - ui.input(|i| i.stable_dt).min(0.1) * 2.6).rem_euclid(std::f32::consts::TAU);
        ui.ctx().data_mut(|d| d.insert_temp(phase_id, phase));
    }
    m3::wavy_progress(ui, bar, frac, phase, wave);
    // Automix: the stretch that blends into the next song is tinted, its start ticked.
    if s.mix > 0.5 {
        let g = m3::wave_geom(bar, frac);
        let edge = m3::wave_geom(bar, (s.total - s.mix) / s.total).thumb.center().x;
        if edge.max(g.rest.0) < g.rest.1 {
            ui.painter().rect_filled(Rect::from_min_max(pos2(edge.max(g.rest.0), g.y - 2.0), pos2(g.rest.1 + 2.0, g.y + 2.0)), 2.0, m.primary.gamma_multiply(0.45));
        }
        if edge >= g.rest.0 {
            ui.painter().rect_filled(Rect::from_center_size(pos2(edge, g.y), vec2(2.0, 10.0)), 1.0, m.primary);
        }
    }
    if live && let Some(p) = resp.interact_pointer_pos() {
        let at = m3::wave_fraction(bar, p.x) * s.total;
        if resp.dragged() {
            app.seek_drag = Some(at);
        }
        if resp.drag_stopped() || resp.clicked() {
            app.seek_drag = None;
            app.actions.push(Action::Seek(at));
        }
    }
    s
}

/// The time played at the left of `row` and the song's length at its right; between
/// them the quality mark, or the word for a mix in progress.
fn times(app: &App, ui: &mut Ui, row: Rect, s: &nowplaying::SeekState) {
    let m = material::scheme();
    let played = m3::galley(ui, &super::fmt_time(s.pos as u32), Type::LabelMedium, m.on_surface_variant, 80.0);
    ui.painter().galley(pos2(row.left() + 4.0, row.top()), played, m.on_surface_variant);
    let length = m3::galley(ui, &super::fmt_time(s.total as u32), Type::LabelMedium, m.on_surface_variant, 80.0);
    ui.painter().galley(pos2(row.right() - 4.0 - length.size().x, row.top()), length, m.on_surface_variant);
    let badge_w = nowplaying::audio_badge_width(ui, app, false);
    if badge_w > 0.0 {
        nowplaying::audio_badge(app, ui, pos2(row.center().x - badge_w / 2.0, row.top() + 1.0), false);
    }
    let mixed = theme::anim_bool(ui.ctx(), "np-automix-on", app.mixing(), 0.3);
    if mixed > 0.01 && badge_w == 0.0 {
        let ink = m.primary.gamma_multiply(mixed);
        let g = m3::galley(ui, "Mixing", Type::LabelMedium, ink, 80.0);
        ui.painter().galley(pos2(row.center().x - g.size().x / 2.0, row.top()), g, ink);
    }
}

/// Previous, play/pause and next, in the transport's three middle targets.
fn middle(app: &mut App, ui: &Ui, t: &Transport, now: f64) {
    let m = material::scheme();
    let id = ui.scope_id();
    if m3::IconButton::new(glyphs::PREVIOUS, "Previous").style(m3::IconStyle::Tonal).size(t.skip).glyph(t.skip * 0.6).show_at(ui, t.targets[1].center(), id.with("previous")).clicked() {
        app.prev();
    }
    // Play/pause: the one filled button, its corners squarer while the song plays.
    let on = app.playing() || app.buffering;
    let rect = t.targets[2];
    let resp = named(ui.interact(rect, id.with("play"), Sense::click()), if on { "Pause" } else { "Play" });
    let morph = theme::anim_bool(ui.ctx(), "np-play-morph", on, 0.2);
    let radius = play_radius(t.play, morph);
    widgets::soft_shadow(ui, rect, radius, 16.0, 0.25);
    ui.painter().rect_filled(rect, radius, m.primary);
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, radius, m.on_primary.gamma_multiply(0.1));
    }
    let c = rect.center();
    if app.buffering {
        let a = now as f32 * 6.0;
        let pts: Vec<Pos2> = (0..16).map(|i| c + vec2((a + i as f32 * 0.24).cos(), (a + i as f32 * 0.24).sin()) * t.play * 0.17).collect();
        ui.painter().add(egui::Shape::Path(egui::epaint::PathShape::line(pts, egui::epaint::PathStroke::new(3.0, m.on_primary))));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    } else {
        // The two marks cross-fade: a hard cut between them flickers.
        let size = t.play * 0.45;
        glyphs::draw(ui, c, size, glyphs::PLAY, theme::with_alpha(m.on_primary, 1.0 - morph));
        glyphs::draw(ui, c, size, glyphs::PAUSE, theme::with_alpha(m.on_primary, morph));
    }
    if resp.clicked() {
        app.toggle();
    }
    if m3::IconButton::new(glyphs::NEXT, "Next").style(m3::IconStyle::Tonal).size(t.skip).glyph(t.skip * 0.6).show_at(ui, t.targets[3].center(), id.with("next")).clicked() {
        app.next();
    }
}

/// The peek at what plays next, at the foot of the artwork view: a tap opens the queue.
fn peek(app: &mut App, ui: &Ui, rect: Rect, gesture_area: f32) {
    let m = material::scheme();
    let corners = CornerRadius { nw: shape::CARD as u8, ne: shape::CARD as u8, sw: 0, se: 0 };
    let resp = named(ui.interact(rect, ui.scope_id().with("np-peek"), Sense::click()), "Up next");
    ui.painter().rect_filled(rect, corners, m.surface_container_low);
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, corners, m.on_surface.gamma_multiply(0.08));
    }
    let g = peek_geom(Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - gesture_area)));
    ui.painter().rect_filled(g.handle, 2.0, m.outline);
    if let Some(at) = g.label {
        m3::text(ui, at, "UP NEXT", Type::LabelMedium, m.on_surface_variant, 200.0);
    }
    match app.queue.upcoming().cloned() {
        Some(next) => {
            widgets::paint_cover(ui, g.cover, &next.thumbs, shape::CHIP, 1.0);
            m3::two_lines(ui, g.text.left(), g.text.center().y, g.text.width(), &next.title, Type::BodyLarge, m.on_surface, &next.artist_line(), Type::BodyMedium, m.on_surface_variant);
        }
        None => {
            ui.painter().rect_filled(g.cover, shape::CHIP, m.surface_container_highest);
            glyphs::draw(ui, g.cover.center(), 22.0, glyphs::QUEUE, m.on_surface_variant);
            let note = if app.autoplay { "Similar songs follow when they are found" } else { "Nothing plays after this song" };
            m3::two_lines(ui, g.text.left(), g.text.center().y, g.text.width(), "End of the queue", Type::BodyLarge, m.on_surface, note, Type::BodyMedium, m.on_surface_variant);
        }
    }
    glyphs::draw(ui, g.mark, 24.0, glyphs::QUEUE, m.on_surface_variant);
    if resp.clicked() {
        app.np_tab = NpTab::UpNext;
    }
}

/// A panel over the player: the lyrics or the queue, with the song in a compact
/// header and the seek bar and a compact transport under it.
#[allow(clippy::too_many_arguments)]
fn panel_view(app: &mut App, ui: &mut Ui, screen: Rect, insets: Insets, tab: NpTab, track: &Track, heard: &Track, now: f64) {
    let m = material::scheme();
    let g = panel_geom(screen, insets);
    let id = ui.scope_id();

    widgets::paint_cover(ui, g.cover, &track.thumbs, shape::TILE_SMALL, 1.0);
    m3::two_lines(ui, g.text.left(), g.text.center().y, g.text.width(), &track.title, Type::TitleMedium, m.on_surface, &track.artist_line(), Type::BodyMedium, m.on_surface_variant);
    if m3::IconButton::new(icon::CHEVRON_DOWN, "Back to the player").style(m3::IconStyle::Overlay).size(m3::TOUCH).show_at(ui, g.close.center(), id.with("close")).clicked() {
        app.np_tab = NpTab::Art;
    }

    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(g.body));
    body.set_clip_rect(g.body.intersect(ui.clip_rect()));
    match tab {
        NpTab::UpNext => super::shell::queue_list(app, &mut body, "np"),
        _ => nowplaying::lyrics_panel(app, &mut body, g.body, now, false),
    }

    let s = seek(app, ui, g.seek, track);
    times(app, ui, g.times, &s);

    let t = g.transport;
    // The song's options and, with the lyrics up, theirs.
    let options = m3::IconButton::new(icon::ELLIPSIS_VERTICAL, "Options").show_at(ui, t.targets[0].center(), id.with("options"));
    nowplaying::options_popup(app, &options, heard, tab == NpTab::Lyrics && !app.no_lyrics());
    middle(app, ui, &t, now);
    // The other panel: the queue from the lyrics, the lyrics from the queue.
    if tab == NpTab::UpNext {
        if m3::IconButton::new(glyphs::LYRICS, "Lyrics").show_at(ui, t.targets[4].center(), id.with("other")).clicked() {
            nowplaying::ask_lyrics(app, ui.ctx());
        }
    } else if m3::IconButton::new(glyphs::QUEUE, "Up next").show_at(ui, t.targets[4].center(), id.with("other")).clicked() {
        app.np_tab = NpTab::UpNext;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phones from narrow to wide, short ones among them: (width, height).
    const SCREENS: [(f32, f32); 6] = [(360.0, 640.0), (360.0, 800.0), (384.0, 832.0), (393.0, 873.0), (412.0, 900.0), (412.0, 915.0)];
    /// The status bar above and the gesture area below.
    const INSETS: Insets = Insets { top: 36.0, bottom: 20.0, left: 0.0, right: 0.0 };

    fn screen(w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(w, h))
    }

    fn apart(a: Rect, b: Rect) -> bool {
        let both = a.intersect(b);
        both.width() <= 0.01 || both.height() <= 0.01
    }

    /// Every pair of `parts` is apart, and each is on `screen` clear of the insets.
    fn laid_out(parts: &[(&str, Rect)], screen: Rect, what: &str) {
        let safe = Rect::from_min_max(pos2(screen.left(), screen.top() + INSETS.top), pos2(screen.right(), screen.bottom() - INSETS.bottom));
        for (i, (name, a)) in parts.iter().enumerate() {
            assert!(a.width() > 0.0 && a.height() > 0.0, "{what}: {name} has no room: {a:?}");
            assert!(safe.expand(0.01).contains_rect(*a), "{what}: {name} is cut off: {a:?}");
            for (other, b) in &parts[i + 1..] {
                assert!(apart(*a, *b), "{what}: {name} {a:?} overlaps {other} {b:?}");
            }
        }
    }

    #[test]
    fn nothing_on_the_artwork_view_overlaps_or_is_cut_off() {
        for (w, h) in SCREENS {
            let g = art_geom(screen(w, h), INSETS);
            let t = g.transport.targets;
            let peek = Rect::from_min_max(g.peek.min, pos2(g.peek.right(), g.peek.bottom() - INSETS.bottom));
            let parts = [
                ("collapse", g.collapse),
                ("source", g.source),
                ("menu", g.menu),
                ("art", g.art),
                ("text", g.text),
                ("like", g.like),
                ("seek", m3::wave_target(g.seek)),
                ("times", g.times),
                ("shuffle", t[0]),
                ("previous", t[1]),
                ("play", t[2]),
                ("next", t[3]),
                ("repeat", t[4]),
                ("tools", g.tools),
                ("peek", peek),
            ];
            laid_out(&parts, screen(w, h), &format!("{w}x{h}"));
            assert_eq!(g.peek.bottom(), h, "the peek's surface runs under the gesture area");
        }
    }

    #[test]
    fn every_control_of_the_artwork_view_fits_a_finger() {
        for (w, h) in SCREENS {
            let g = art_geom(screen(w, h), INSETS);
            let mut targets = vec![g.collapse, g.menu, g.like, m3::wave_target(g.seek), g.tools];
            targets.extend(g.transport.targets);
            for r in targets {
                assert!(r.width() >= m3::TOUCH && r.height() >= m3::TOUCH, "{w}x{h}: {r:?}");
            }
            assert!(g.peek.height() - INSETS.bottom >= m3::TOUCH);
            assert!(g.transport.play >= 72.0 && g.transport.skip >= 56.0, "{w}x{h}");
            assert!(g.transport.targets[2].width() == g.transport.play);
        }
    }

    #[test]
    fn the_artwork_is_square_centred_and_large_even_on_a_short_screen() {
        for (w, h) in SCREENS {
            let g = art_geom(screen(w, h), INSETS);
            assert!((g.art.width() - g.art.height()).abs() < 0.01);
            assert!((g.art.center().x - w / 2.0).abs() < 0.01);
            assert!(g.art.width() >= 170.0, "{w}x{h}: the artwork is {} across", g.art.width());
            assert!(g.art.width() <= w - 2.0 * 28.0, "{w}x{h}: it keeps its margins");
        }
        // A screen with room has the designs' sizes; a short one takes them down.
        let roomy = art_geom(screen(412.0, 900.0), INSETS);
        assert_eq!(roomy.squeeze, 0.0);
        assert_eq!(roomy.transport.play, 88.0);
        assert!((roomy.art.width() - 338.0).abs() < 0.01);
        let short = art_geom(screen(360.0, 640.0), INSETS);
        assert_eq!(short.squeeze, 1.0);
        assert_eq!(short.transport.play, 72.0);
    }

    #[test]
    fn the_title_keeps_room_beside_the_like_button() {
        for (w, h) in SCREENS {
            let g = art_geom(screen(w, h), INSETS);
            assert!(g.text.width() >= 200.0, "{w}x{h}: {}", g.text.width());
            assert!(g.text.left() <= g.art.left() + 0.01, "{w}x{h}: it starts no further in than the artwork");
            assert!(g.text.right() < g.like.left(), "{w}x{h}: and stops short of the like button");
        }
    }

    #[test]
    fn the_transport_is_shuffle_previous_play_next_repeat_about_the_middle() {
        for w in [360.0, 384.0, 412.0] {
            let row = Rect::from_min_size(pos2(20.0, 600.0), vec2(w - 40.0, 88.0));
            let t = transport(row, 88.0);
            for pair in t.targets.windows(2) {
                assert!(pair[0].right() <= pair[1].left() + 0.01, "{w}: in order, apart");
            }
            assert!((t.targets[2].center().x - row.center().x).abs() < 0.01, "{w}: play is in the middle");
            assert!(((t.targets[1].center().x - row.left()) - (row.right() - t.targets[3].center().x)).abs() < 0.01, "{w}: previous and next mirror each other");
            assert!(row.expand(0.01).contains_rect(t.targets[0]) && row.expand(0.01).contains_rect(t.targets[4]), "{w}");
            for r in t.targets {
                assert_eq!(r.center().y, row.center().y);
            }
            assert!(t.play > t.skip && t.skip > m3::TOUCH, "{w}: play is the largest, the side buttons the smallest");
        }
        // The designs' own, on a 412-point phone: 64-point skips, 15 points apart.
        let t = transport(Rect::from_min_size(pos2(20.0, 0.0), vec2(372.0, 88.0)), 88.0);
        assert_eq!(t.skip, 64.0);
        assert!((t.targets[1].left() - t.targets[0].right() - 15.0).abs() < 0.01);
    }

    #[test]
    fn the_play_button_is_round_while_paused_and_squarer_while_playing() {
        assert_eq!(play_radius(88.0, 0.0), 44.0);
        assert!((play_radius(88.0, 1.0) - 28.16).abs() < 0.01);
        assert!(play_radius(72.0, 0.5) < play_radius(72.0, 0.0) && play_radius(72.0, 0.5) > play_radius(72.0, 1.0));
    }

    #[test]
    fn the_peeks_parts_stay_inside_it_and_apart() {
        for (w, h) in [(360.0, 72.0), (360.0, 80.0), (360.0, 86.0), (412.0, 100.0)] {
            let peek = Rect::from_min_size(pos2(0.0, 700.0), vec2(w, h));
            let g = peek_geom(peek);
            for part in [g.handle, g.cover, g.text] {
                assert!(peek.contains_rect(part), "{w}x{h}: {part:?}");
            }
            assert!(apart(g.handle, g.cover) && apart(g.cover, g.text));
            assert!(g.text.right() < g.mark.x - 12.0, "the text stops short of the queue mark");
            assert_eq!(g.label.is_some(), h >= 86.0, "the label needs room above the row");
            if let Some(at) = g.label {
                assert!(at.y >= g.handle.bottom() && at.y + 16.0 <= g.cover.top(), "{w}x{h}");
            }
            assert!(g.text.width() >= 180.0);
        }
    }

    #[test]
    fn tools_stand_in_the_middle_of_their_row_and_never_leave_it() {
        let row = Rect::from_min_size(pos2(20.0, 700.0), vec2(320.0, 48.0));
        let two = tools(row, &[95.0, 135.0]);
        assert_eq!(two.len(), 2);
        assert!((two[0].left() - row.left() - (row.right() - two[1].right())).abs() < 0.01, "centred");
        assert!((two[1].left() - two[0].right() - 8.0).abs() < 0.01);
        assert_eq!((two[0].width(), two[1].width()), (95.0, 135.0));
        // A third tool (where the music plays) that no longer fits: they share the row.
        let three = tools(row, &[95.0, 130.0, 135.0]);
        assert!((three[0].left() - row.left()).abs() < 0.01 && (three[2].right() - row.right()).abs() < 0.01);
        for pair in three.windows(2) {
            assert!((pair[1].left() - pair[0].right() - 8.0).abs() < 0.01);
        }
        assert!(three.iter().all(|r| r.width() >= 80.0 && r.height() == 48.0));
        assert!(tools(row, &[]).is_empty());
    }

    #[test]
    fn the_output_tool_keeps_its_name_while_the_row_has_room_and_then_is_its_icon() {
        // A name that asks for 130 beside two tools that ask for 100 and 135 together,
        // the chip's icon and padding taking 62.
        assert_eq!(output_width(420.0, 235.0, 130.0, 62.0), 130.0, "room for the whole name");
        assert_eq!(output_width(336.0, 235.0, 130.0, 62.0), m3::TOOL_ICON_W, "a 360-point phone's row: the icon alone");
        // A long name is cut short while 64 points of it still show, and given up below that.
        assert_eq!(output_width(377.0, 235.0, 200.0, 62.0), 126.0);
        assert_eq!(output_width(376.0, 235.0, 200.0, 62.0), m3::TOOL_ICON_W);
        assert!(m3::TOOL_ICON_W >= m3::TOUCH);
    }

    /// The tools drawn in a headless egui `width` points across, in the row Now Playing
    /// gives them, and tapped.
    struct Tools {
        ctx: egui::Context,
        row: Rect,
        output: Output,
        sleep: (String, bool),
        time: f64,
        layout: Option<ToolRow>,
        tapped: Vec<Tool>,
    }

    impl Tools {
        fn new(width: f32, output: Output, sleep: (&str, bool)) -> Self {
            let row = art_geom(screen(width, 800.0), INSETS).tools;
            let mut t = Self { ctx: egui::Context::default(), row, output, sleep: (sleep.0.to_owned(), sleep.1), time: 0.0, layout: None, tapped: Vec::new() };
            t.frame(vec![]);
            t.frame(vec![]);
            t
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(screen(self.row.right() + 40.0, 800.0)), time: Some(self.time), events, ..Default::default() };
            let (mut layout, mut tapped) = (None, None);
            let out = self.ctx.run_ui(input, |ui| {
                layout = Some(tool_row(ui, self.row, self.output.title(), &self.sleep.0));
                tapped = tools_view(ui, self.row, ui.scope_id(), &self.output, (&self.sleep.0, self.sleep.1));
            });
            out.drop_without_applying_deltas();
            self.layout = layout;
            self.tapped.extend(tapped);
        }

        fn tap(&mut self, at: Pos2) {
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            self.frame(vec![egui::Event::PointerMoved(at), button(true)]);
            self.frame(vec![button(false)]);
            self.frame(vec![]);
        }

        fn layout(&self) -> ToolRow {
            self.layout.expect("drawn")
        }
    }

    fn buds() -> Output {
        Output { kind: crate::playon::Kind::Bluetooth, name: "Pixel Buds Pro".into(), volume: 7, max_volume: 15, switcher: crate::playon::Switcher::System }
    }

    #[test]
    fn where_the_music_plays_is_a_tool_between_lyrics_and_the_sleep_timer() {
        for width in [360.0, 412.0, MAX_W] {
            let t = Tools::new(width, Output::default(), ("Sleep timer", false));
            let [lyrics, output, sleep] = t.layout().rects;
            assert!(lyrics.right() < output.left() && output.right() < sleep.left(), "{width}: in that order, apart");
            for r in [lyrics, output, sleep] {
                assert_eq!(r.height(), m3::TOUCH);
                assert!(t.row.expand(0.01).contains_rect(r), "{width}: {r:?} is in the row {:?}", t.row);
            }
        }
    }

    #[test]
    fn at_360_points_every_tool_fits_whatever_the_output_is_called_and_the_timer_says() {
        let names = ["This phone", "Pixel Buds Pro", "Galaxy Buds2 Pro (Aminuddin's)", "A"];
        for name in names {
            for label in ["Sleep timer", "End of queue", "End of song", "12:34"] {
                let output = Output { kind: crate::playon::Kind::Bluetooth, name: name.into(), ..buds() };
                let t = Tools::new(360.0, output, (label, true));
                let rects = t.layout().rects;
                for r in rects {
                    assert!(t.row.expand(0.01).contains_rect(r), "{name} / {label}: {r:?} leaves the row {:?}", t.row);
                    assert!(r.width() >= 44.0 && r.height() >= m3::TOUCH, "{name} / {label}: {r:?} is a finger wide");
                }
                assert!(rects[1].width() >= m3::TOOL_ICON_W - 0.01, "{name} / {label}: the output tool, an icon at least, keeps a finger's width");
                for pair in rects.windows(2) {
                    assert!(pair[0].right() + 7.99 <= pair[1].left(), "{name} / {label}: {pair:?}");
                }
            }
        }
    }

    #[test]
    fn a_short_row_shows_the_output_as_its_icon_and_a_wide_one_names_it() {
        let words = |width: f32, name: &str| Tools::new(width, Output { kind: crate::playon::Kind::Bluetooth, name: name.into(), ..buds() }, ("Sleep timer", false)).layout().words;
        assert!(!words(320.0, "Pixel Buds Pro"), "a small phone");
        assert!(!words(MAX_W, "Galaxy Buds2 Pro (Aminuddin's)"), "a long name does not squeeze the others");
        assert!(words(MAX_W, "Buds"), "a wide screen and a short name");
    }

    #[test]
    fn a_tap_on_a_tool_chooses_it_with_its_name_and_without() {
        for width in [320.0, MAX_W] {
            for (tool, at) in [(Tool::Lyrics, 0), (Tool::Output, 1), (Tool::Sleep, 2)] {
                let mut t = Tools::new(width, buds(), ("Sleep timer", false));
                let at = t.layout().rects[at].center();
                t.tap(at);
                assert_eq!(t.tapped, [tool], "{width}");
            }
        }
    }

    #[test]
    fn a_tap_beside_the_tools_chooses_none() {
        let mut t = Tools::new(MAX_W, buds(), ("Sleep timer", false));
        let [_, _, sleep] = t.layout().rects;
        t.tap(pos2(sleep.right() + 20.0, sleep.center().y));
        t.tap(pos2(t.row.center().x, t.row.top() - 30.0));
        assert!(t.tapped.is_empty());
    }

    #[test]
    fn nothing_on_a_panel_overlaps_or_is_cut_off_and_its_controls_fit_a_finger() {
        for (w, h) in SCREENS {
            let g = panel_geom(screen(w, h), INSETS);
            let t = g.transport.targets;
            let parts = [
                ("cover", g.cover),
                ("text", g.text),
                ("close", g.close),
                ("body", g.body),
                ("seek", m3::wave_target(g.seek)),
                ("times", g.times),
                ("options", t[0]),
                ("previous", t[1]),
                ("play", t[2]),
                ("next", t[3]),
                ("other", t[4]),
            ];
            laid_out(&parts, screen(w, h), &format!("{w}x{h}"));
            for r in [g.close, m3::wave_target(g.seek), t[0], t[1], t[2], t[3], t[4]] {
                assert!(r.width() >= m3::TOUCH && r.height() >= m3::TOUCH, "{w}x{h}: {r:?}");
            }
            assert!(g.body.height() >= 300.0, "{w}x{h}: the lyrics keep {}", g.body.height());
            assert!(g.text.width() >= 180.0);
            assert_eq!(g.transport.play, 72.0);
        }
    }

    #[test]
    fn a_screen_wider_than_a_phone_keeps_the_player_in_a_column() {
        let g = art_geom(screen(900.0, 900.0), INSETS);
        assert!(g.peek.width() <= MAX_W + 0.01);
        assert!((g.art.center().x - 450.0).abs() < 0.01);
        let sideways = Insets { left: 40.0, right: 0.0, ..INSETS };
        let g = art_geom(screen(400.0, 800.0), sideways);
        assert!(g.collapse.left() >= 40.0, "clear of an inset at the side");
    }
}
