//! Reusable, hand-painted widgets. Everything custom-painted gets an accessible name
//! (`named`), so screen readers and UI Automation can see and invoke it.

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Ui, Vec2, pos2, vec2};

use super::Action;
use crate::model::{Card, Link, Target, Thumb, Track, pick_thumb, sized};
use crate::theme::{self, icon};

pub fn named(resp: Response, name: &str) -> Response {
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    resp
}

/// Single-line text truncated with an ellipsis to `width`. Right-to-left text is put in
/// display order (`bidi`), and cut at its end (the left) when it doesn't fit.
pub fn galley(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    galley_fmt(ui, text, egui::TextFormat::simple(font, color), width)
}

/// [`galley`] set in a whole text format: a type role's line height and tracking too.
pub fn galley_fmt(ui: &Ui, text: &str, format: egui::TextFormat, width: f32) -> std::sync::Arc<egui::Galley> {
    let width = width.max(10.0);
    let (font, color) = (format.font_id.clone(), format.color);
    let mut job = if !crate::bidi::has_rtl(text) {
        egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color)
    } else if crate::bidi::rtl_base(text) {
        crate::bidi::job(&rtl_fit(ui, text, &font, width), font, color)
    } else {
        crate::bidi::job(text, font, color)
    };
    set_format(&mut job, &format);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Give every run of `job` this format.
fn set_format(job: &mut egui::text::LayoutJob, format: &egui::TextFormat) {
    for section in &mut job.sections {
        section.format = format.clone();
    }
}

/// Right-to-left `text`, shortened at its logical end with "…" until it fits `width`
/// (egui would cut the right side, the start of the line).
fn rtl_fit(ui: &Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let fits = |s: &str| ui.fonts_mut(|f| f.layout_job(crate::bidi::job(s, font.clone(), Color32::WHITE)).size().x) <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let cut = |n: usize| format!("{}…", text[..chars.get(n).map_or(text.len(), |c| c.0)].trim_end());
    // The most characters that fit with the ellipsis.
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(&cut(mid)) { lo = mid } else { hi = mid - 1 }
    }
    cut(lo)
}

/// Wrapped text, at most `rows` lines.
pub fn galley_wrapped(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> std::sync::Arc<egui::Galley> {
    galley_wrapped_fmt(ui, text, egui::TextFormat::simple(font, color), width, rows)
}

/// [`galley_wrapped`] set in a whole text format: a type role's line height and tracking too.
pub fn galley_wrapped_fmt(ui: &Ui, text: &str, format: egui::TextFormat, width: f32, rows: usize) -> std::sync::Arc<egui::Galley> {
    let (font, color) = (format.font_id.clone(), format.color);
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font.clone(), color, width.max(10.0));
    set_format(&mut job, &format);
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    let g = ui.fonts_mut(|f| f.layout_job(job.clone()));
    if !crate::bidi::has_rtl(text) {
        return g;
    }
    // Wrapped in reading order, then each row put in display order on its own (ordering
    // the whole text first would start the first row with the end of the sentence).
    let mut rows_text = Vec::with_capacity(g.rows.len());
    let mut chars = text.chars();
    for (i, row) in g.rows.iter().enumerate() {
        let mut s: String = chars.by_ref().take(row.char_count_including_newline().0).collect();
        if s.ends_with('\n') {
            s.pop();
        }
        if i + 1 == g.rows.len() && g.elided {
            s = format!("{}…", s.trim_end());
        }
        rows_text.push(s.trim_end().to_owned());
    }
    let mut job = crate::bidi::job(&rows_text.join("\n"), font, color);
    set_format(&mut job, &format);
    job.halign = g.job.halign;
    ui.fonts_mut(|f| f.layout_job(job))
}

pub fn text_at(ui: &Ui, pos: Pos2, text: &str, font: egui::FontId, color: Color32, width: f32) -> Rect {
    let g = galley(ui, text, font, color, width);
    let rect = Rect::from_min_size(pos, g.size());
    ui.painter().galley(pos, g, color);
    rect
}

/// Single-line text whose artist names are links: underlined on hover, and a click
/// opens the artist's page. Names without a channel id stay plain text.
#[allow(clippy::too_many_arguments)]
pub fn linked_text(
    ui: &Ui,
    pos: Pos2,
    text: &str,
    links: &[Link],
    font: egui::FontId,
    color: Color32,
    width: f32,
    acts: &mut Vec<Action>,
) -> Rect {
    let g = galley(ui, text, font, color, width);
    let rect = Rect::from_min_size(pos, g.size());
    // Names are found in the text as shown: right-to-left text is reordered, its names
    // with it, and they no longer come in reading order.
    let shown = g.job.text.clone();
    let rtl = crate::bidi::has_rtl(text);
    let mut search_from = 0;
    for (i, l) in links.iter().enumerate() {
        let Some(id) = &l.id else { continue };
        let name = crate::bidi::visual(&l.name);
        let from = if rtl { 0 } else { search_from };
        let Some(off) = shown[from..].find(name.as_ref()) else { continue };
        let start = from + off;
        let end = start + name.len();
        search_from = end;
        let c0 = shown[..start].chars().count();
        let c1 = shown[..end].chars().count();
        let x0 = g.pos_from_cursor(egui::text::CCursor::new(c0)).min.x;
        let x1 = g.pos_from_cursor(egui::text::CCursor::new(c1)).min.x;
        if x1 - x0 < 2.0 {
            continue; // truncated away
        }
        let r = Rect::from_min_max(pos2(pos.x + x0, rect.top()), pos2(pos.x + x1, rect.bottom()));
        let resp = ui.interact(r, ui.scope_id().with(("artist-link", text, i, pos.x as i32, pos.y as i32)), Sense::click());
        let resp = named(resp, &l.name).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.hovered() {
            ui.painter().hline(r.x_range(), r.bottom() - 1.0, egui::Stroke::new(1.0, color));
        }
        if resp.clicked() {
            acts.push(Action::Open(Target::Artist(id.clone())));
        }
    }
    ui.painter().galley(pos, g, color);
    rect
}

pub fn icon_at(ui: &Ui, center: Pos2, glyph: char, size: f32, color: Color32) {
    // Centred on the drawn shape: egui places these glyphs flush left in their box
    // (dropping the font's side bearing) and a little high, so box-centring is off.
    let g = ui.painter().layout_no_wrap(glyph.to_string(), theme::icon_font(size), color);
    let ink = g.mesh_bounds;
    let pos = if ink.is_positive() { center - ink.center().to_vec2() } else { center - g.size() / 2.0 };
    ui.painter().galley(pos, g, color);
}

/// Heart drawn from its parametric curve: an outline, filled by `fill` (0..1). The
/// icon font only has the outline, and a liked song needs to read at a glance.
pub fn heart(ui: &Ui, center: Pos2, size: f32, fill: f32, outline: Color32, fill_color: Color32) {
    use std::f32::consts::TAU;
    // x = 16 sin^3 t, y = 13 cos t - 5 cos 2t - 2 cos 3t - cos 4t spans x ±16, y -17..12;
    // shift so the box is centred, flip y for screen space.
    static UNIT: std::sync::OnceLock<Vec<Vec2>> = std::sync::OnceLock::new();
    let unit = UNIT.get_or_init(|| {
        (0..64)
            .map(|i| {
                let t = i as f32 / 64.0 * TAU;
                let x = 16.0 * t.sin().powi(3);
                let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
                vec2(x, -y - 2.5)
            })
            .collect()
    });
    let s = size / 34.0;
    let pts: Vec<Pos2> = unit.iter().map(|v| center + *v * s).collect();
    if fill > 0.0 {
        // The heart is star-shaped around a point just below its middle, so a fan fills it.
        let mut mesh = egui::Mesh::default();
        let c = theme::with_alpha(fill_color, fill * fill_color.a() as f32 / 255.0);
        mesh.colored_vertex(center + vec2(0.0, 1.0 * s), c);
        for p in &pts {
            mesh.colored_vertex(*p, c);
        }
        let n = pts.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
    }
    // Separate segments with round joins: a closed polyline mitres the tip and the
    // notch into long spikes.
    let w = 1.7;
    for (i, p) in pts.iter().enumerate() {
        let q = pts[(i + 1) % pts.len()];
        ui.painter().line_segment([*p, q], egui::Stroke::new(w, outline));
        ui.painter().circle_filled(*p, w / 2.0, outline);
    }
}

/// Circular icon button with an animated hover halo.
pub fn icon_button(ui: &mut Ui, glyph: char, size: f32, name: &str, active: bool) -> Response {
    let d = size + 16.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d), Sense::click());
    let resp = named(resp, name).on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered(), 0.15);
    if h > 0.0 {
        ui.painter().circle_filled(rect.center(), d / 2.0, theme::glass(0.10 * h));
    }
    let color = if active { theme::text() } else { theme::lerp_color(theme::text_dim(), theme::text(), h) };
    let press = if resp.is_pointer_button_down_on() { 0.9 } else { 1.0 };
    icon_at(ui, rect.center(), glyph, size * press, color);
    resp
}

/// Rounded pill button. `primary` = white with dark text (YouTube Music's Play button).
pub fn pill(ui: &mut Ui, label: &str, glyph: Option<char>, primary: bool) -> Response {
    if crate::material::on() {
        let style = if primary { super::m3::ButtonStyle::Filled } else { super::m3::ButtonStyle::Tonal };
        return super::m3::button(ui, label, glyph, style);
    }
    // Laid out once; the colour is given when it's painted.
    let text = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(14.0), Color32::PLACEHOLDER);
    let icon_w = if glyph.is_some() { 24.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(text.size().x + icon_w + 36.0, 38.0), Sense::click());
    let resp = named(resp, label).on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered(), 0.15);
    let (fill, fg) = if primary {
        if theme::light() { (theme::lerp_color(theme::text(), Color32::BLACK, h), Color32::WHITE) } else { (theme::lerp_color(Color32::from_gray(240), Color32::WHITE, h), Color32::from_gray(12)) }
    } else {
        (theme::glass(0.10 + 0.08 * h), theme::text())
    };
    let r = rect.shrink(if resp.is_pointer_button_down_on() { 1.0 } else { 0.0 });
    ui.painter().rect_filled(r, 19.0, fill);
    let mut x = r.left() + 18.0;
    if let Some(g) = glyph {
        icon_at(ui, pos2(x + 8.0, r.center().y), g, 17.0, fg);
        x += icon_w;
    }
    ui.painter().galley(pos2(x, r.center().y - text.size().y / 2.0), text, fg);
    resp
}

/// How much room `chip` takes, for a row that places its chips itself.
pub fn chip_size(ui: &Ui, label: &str, selected: bool) -> Vec2 {
    if crate::material::on() {
        return super::m3::filter_chip_size(ui, label, selected);
    }
    vec2(ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(13.5), Color32::PLACEHOLDER).size().x + 28.0, 34.0)
}

/// Selectable chip (home moods, library tabs).
pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    if crate::material::on() {
        return super::m3::filter_chip(ui, label, selected);
    }
    let text = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(13.5), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(vec2(text.size().x + 28.0, 34.0), Sense::click());
    let resp = named(resp, label).on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered(), 0.15);
    let s = theme::anim_bool(ui.ctx(), resp.id.with("s"), selected, 0.2);
    use crate::preset::ChipStyle;
    let style = crate::preset::active().app.chip;
    let radius = (crate::preset::active().chrome.radius * 0.55).clamp(4.0, 17.0);
    let (on_fill, on_fg) = match style {
        ChipStyle::Accent => {
            let a = ui.ctx().data(|d| d.get_temp::<Color32>(egui::Id::unique("accent"))).unwrap_or(theme::RED);
            (a, if a.r() as u32 + a.g() as u32 + a.b() as u32 > 480 { Color32::from_gray(15) } else { Color32::WHITE })
        }
        _ => (theme::text(), theme::on_text()),
    };
    let off_fill = if style == ChipStyle::Outline { theme::glass(0.04 * h) } else { theme::glass(0.09 + 0.06 * h) };
    let fill = theme::lerp_color(off_fill, on_fill, s);
    ui.painter().rect_filled(rect, radius, fill);
    if style == ChipStyle::Outline {
        ui.painter().rect_stroke(rect, radius, egui::Stroke::new(1.0, theme::glass(0.3 * (1.0 - s))), egui::StrokeKind::Inside);
    }
    let fg = theme::lerp_color(theme::text(), on_fg, s);
    ui.painter().galley(rect.center() - text.size() / 2.0, text, fg);
    resp
}

/// The size to fetch an image at for `px` pixels: rounded up to a few fixed steps, so
/// resizing the window doesn't fetch a new size every frame and small covers (rows,
/// sidebar, player bar) share one download.
fn fetch_px(px: u32) -> u32 {
    const STEPS: [u32; 9] = [60, 120, 180, 240, 360, 544, 720, 960, 1200];
    STEPS.into_iter().find(|&s| s >= px).unwrap_or(1200)
}

/// The cover URL to paint at `px`: that size once it has loaded, else the size shown
/// last for the same cover. A cover that changes size (going full screen, resizing the
/// window) keeps showing instead of blanking until the new size arrives, or for good
/// if that download fails.
fn steady_url(ctx: &egui::Context, base: &str, px: u32) -> String {
    let want = sized(base, px);
    let id = egui::Id::unique(("cover-shown", base));
    let ready = matches!(egui::Image::new(want.as_str()).load_for_size(ctx, Vec2::splat(px as f32)), Ok(egui::load::TexturePoll::Ready { .. }));
    if ready {
        ctx.data_mut(|d| d.insert_temp(id, px));
        return want;
    }
    match ctx.data(|d| d.get_temp::<u32>(id)) {
        Some(shown) if shown != px => sized(base, shown),
        _ => want,
    }
}

/// Paint a cover image (square-cropped, CDN-resized) with a placeholder underneath.
pub fn paint_cover(ui: &Ui, rect: Rect, thumbs: &[Thumb], radius: f32, zoom: f32) {
    let px = (rect.width() * ui.ctx().pixels_per_point()) as u32;
    paint_cover_px(ui, rect, thumbs, radius, zoom, px);
}

/// A [`paint_cover`] that dissolves from the sleeve this slot showed last when the
/// track changes, the way the reference hands the artwork over. `site` is a stable id
/// per sleeve.
pub fn paint_cover_xfade(ui: &Ui, site: egui::Id, rect: Rect, thumbs: &[Thumb], radius: f32, zoom: f32) {
    let px = (rect.width() * ui.ctx().pixels_per_point()) as u32;
    paint_cover_xfade_px(ui, site, rect, thumbs, radius, zoom, px);
}

/// [`paint_cover_xfade`] fetching the image at `px` rather than the drawn size (see
/// [`paint_cover_px`]).
pub fn paint_cover_xfade_px(ui: &Ui, site: egui::Id, rect: Rect, thumbs: &[Thumb], radius: f32, zoom: f32, px: u32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let px = fetch_px(px);
    ui.painter().rect_filled(rect, radius, theme::glass(0.07));
    let Some(t) = pick_thumb(thumbs, px.min(544)) else {
        icon_at(ui, rect.center(), icon::MUSIC, rect.width() * 0.3, theme::text_faint());
        return;
    };
    let mut uv = t.square_uv();
    if zoom > 1.0 {
        let c = uv.center();
        let half = uv.size() / (2.0 * zoom);
        uv = Rect::from_center_size(c, half * 2.0);
    }
    let url = steady_url(ui.ctx(), &t.url, px);
    cover_xfade(ui, site, rect, radius, &url, uv, Color32::WHITE);
}

/// [`paint_cover`] fetching the image at `px` rather than the drawn size. A cover that
/// animates in size must ask for one fixed size: every new size is a new download,
/// blank until it arrives.
pub fn paint_cover_px(ui: &Ui, rect: Rect, thumbs: &[Thumb], radius: f32, zoom: f32, px: u32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let px = fetch_px(px);
    ui.painter().rect_filled(rect, radius, theme::glass(0.07));
    let Some(t) = pick_thumb(thumbs, px.min(544)) else {
        icon_at(ui, rect.center(), icon::MUSIC, rect.width() * 0.3, theme::text_faint());
        return;
    };
    let mut uv = t.square_uv();
    if zoom > 1.0 {
        // Zoom by cropping the UV rect: stays inside the rounded corners.
        let c = uv.center();
        let half = uv.size() / (2.0 * zoom);
        uv = Rect::from_center_size(c, half * 2.0);
    }
    let url = steady_url(ui.ctx(), &t.url, px);
    paint_remote(ui, &url, rect, radius, uv, Color32::WHITE);
}

/// Paint the cover cropped to fill `rect` (any aspect), tinted, for full-bleed backdrops.
pub fn paint_cover_fill(ui: &Ui, rect: Rect, thumbs: &[Thumb], tint: Color32) {
    let ppp = ui.ctx().pixels_per_point();
    let px = fetch_px((rect.width().max(rect.height()) * ppp) as u32).clamp(200, 1200);
    let Some(t) = pick_thumb(thumbs, px) else { return };
    let uv = t.square_uv();
    // Crop the square to the rect's aspect ratio, keeping the centre.
    let aspect = rect.width() / rect.height().max(1.0);
    let size = if aspect >= 1.0 { vec2(uv.width(), uv.height() / aspect) } else { vec2(uv.width() * aspect, uv.height()) };
    let url = steady_url(ui.ctx(), &t.url, px);
    paint_remote(ui, &url, rect, 0.0, Rect::from_center_size(uv.center(), size), tint);
}

/// Paint the cover cropped to fill `rect` whatever its shape (a carousel item, a hero
/// card), keeping its centre, with each corner rounded as `corners` says.
pub fn paint_cover_cropped(ui: &Ui, rect: Rect, thumbs: &[Thumb], corners: CornerRadius) {
    if !ui.is_rect_visible(rect) || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    ui.painter().rect_filled(rect, corners, theme::glass(0.07));
    let px = fetch_px((rect.width().max(rect.height()) * ui.ctx().pixels_per_point()) as u32);
    let Some(t) = pick_thumb(thumbs, px.min(544)) else {
        icon_at(ui, rect.center(), icon::MUSIC, rect.height().min(rect.width()) * 0.3, theme::text_faint());
        return;
    };
    let uv = t.square_uv();
    let aspect = rect.width() / rect.height();
    let size = if aspect >= 1.0 { vec2(uv.width(), uv.height() / aspect) } else { vec2(uv.width() * aspect, uv.height()) };
    let url = steady_url(ui.ctx(), &t.url, px);
    paint_remote_rounded(ui, &url, rect, corners, Rect::from_center_size(uv.center(), size), Color32::WHITE);
}

/// Start loading a cover at `px` without drawing it, so it's ready when it appears.
pub fn preload_cover(ctx: &egui::Context, thumbs: &[Thumb], px: u32) {
    let px = fetch_px(px);
    if let Some(t) = pick_thumb(thumbs, px.min(544)) {
        let _ = egui::Image::new(sized(&t.url, px)).load_for_size(ctx, Vec2::splat(px as f32));
    }
}

/// The cover as a disc rotated by `angle` (radians): a textured triangle fan, since
/// egui's image rotation doesn't round corners. Fetched at `px` (see [`paint_cover_px`]).
/// Dissolves between tracks like [`paint_cover_xfade`].
#[allow(clippy::too_many_arguments)]
pub fn paint_disc(ui: &Ui, site: egui::Id, center: Pos2, r: f32, thumbs: &[Thumb], angle: f32, tint: Color32, px: u32) {
    ui.painter().circle_filled(center, r, theme::glass(0.07));
    let px = fetch_px(px);
    let Some(t) = pick_thumb(thumbs, px.min(544)) else { return };
    let url = steady_url(ui.ctx(), &t.url, px);
    let ready = matches!(egui::Image::new(&url).load_for_size(ui.ctx(), Vec2::splat(r * 2.0)), Ok(egui::load::TexturePoll::Ready { .. }));
    let uv = t.square_uv();
    dissolve(ui, site, &url, uv, ready, &mut |u, uv, a| {
        disc_at(ui, center, r, u, uv, angle, tint.gamma_multiply(a));
    });
}

/// One disc, if its texture is ready; false while it downloads (the loader wakes the UI).
fn disc_at(ui: &Ui, center: Pos2, r: f32, url: &str, uv: Rect, angle: f32, tint: Color32) -> bool {
    let Ok(egui::load::TexturePoll::Ready { texture }) = egui::Image::new(url).load_for_size(ui.ctx(), Vec2::splat(r * 2.0)) else { return false };
    let (uc, uh) = (uv.center(), uv.width() / 2.0);
    let mut mesh = egui::Mesh::with_texture(texture.id);
    mesh.colored_vertex(center, tint);
    mesh.vertices[0].uv = uc;
    let n = 96;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let (s, c) = a.sin_cos();
        let (sr, cr) = (a - angle).sin_cos();
        mesh.vertices.push(egui::epaint::Vertex { pos: center + vec2(c, s) * r, uv: uc + vec2(cr * uh, sr * uv.height() / 2.0), color: tint });
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    true
}

/// Seconds a new sleeve takes to dissolve over the last one.
const COVER_XFADE: f64 = 0.45;

/// What a sleeve slot last painted: the image on screen, the one dissolving in over it,
/// and the timestamps that drive the dissolve.
#[derive(Clone)]
struct Shown {
    url: String,
    uv: Rect,
    to: Option<String>,
    /// When the change was first seen, to give up on a cover that never downloads.
    swap_at: f64,
    /// When the dissolve started; None while the incoming image downloads.
    fade_from: Option<f64>,
}

/// Paint `url` into this slot, dissolving from the cover `site` showed last when it
/// changes. The last cover stays up until the new one has downloaded, so a track
/// change never blinks through the placeholder.
fn dissolve(ui: &Ui, site: egui::Id, url: &str, uv: Rect, ready: bool, paint: &mut dyn FnMut(&str, Rect, f32)) {
    let ctx = ui.ctx();
    let now = ctx.input(|i| i.time);
    let key = site.with("cover-shown");
    let Some(mut s) = ctx.data(|d| d.get_temp::<Shown>(key)) else {
        paint(url, uv, 1.0);
        ctx.data_mut(|d| d.insert_temp(key, Shown { url: url.to_owned(), uv, to: None, swap_at: now, fade_from: None }));
        return;
    };
    if s.url == url {
        // Back to what's on screen (or never left): drop any dissolve in flight.
        paint(url, uv, 1.0);
        if s.to.is_some() {
            ctx.data_mut(|d| d.insert_temp(key, Shown { url: url.to_owned(), uv, to: None, swap_at: now, fade_from: None }));
        }
        return;
    }
    if s.to.as_deref() != Some(url) {
        // A new cover arrived (possibly mid-dissolve): dissolve from what is on screen.
        s.to = Some(url.to_owned());
        s.swap_at = now;
        s.fade_from = None;
    }
    if !ready {
        if now - s.swap_at < 3.0 {
            paint(&s.url, s.uv, 1.0);
            ctx.data_mut(|d| d.insert_temp(key, s));
            ctx.request_repaint();
            return;
        }
        // A cover that never arrives: its placeholder beats freezing the old one.
        paint(url, uv, 1.0);
        ctx.data_mut(|d| d.insert_temp(key, Shown { url: url.to_owned(), uv, to: None, swap_at: now, fade_from: None }));
        return;
    }
    let from = *s.fade_from.get_or_insert(now);
    let p = (((now - from) / COVER_XFADE) as f32).clamp(0.0, 1.0);
    let e = theme::ease_in_out(p);
    paint(&s.url, s.uv, 1.0 - e);
    paint(url, uv, e);
    if p >= 1.0 {
        s = Shown { url: url.to_owned(), uv, to: None, swap_at: now, fade_from: None };
    } else {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(key, s));
}

/// [`paint_remote`] for a sleeve that dissolves between tracks instead of swapping.
fn cover_xfade(ui: &Ui, site: egui::Id, rect: Rect, radius: f32, url: &str, uv: Rect, tint: Color32) {
    let ready = matches!(egui::Image::new(url).load_for_size(ui.ctx(), rect.size()), Ok(egui::load::TexturePoll::Ready { .. }));
    dissolve(ui, site, url, uv, ready, &mut |u, uv, a| paint_remote(ui, u, rect, radius, uv, tint.gamma_multiply(a)));
}

/// Album/playlist/artist/song card with hover zoom, scrim and a play button.
pub fn card(ui: &mut Ui, c: &Card, width: f32, liked: bool, acts: &mut Vec<Action>, sprite: Option<&MotionSprite>) {
    let height = width + 58.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let resp = named(resp, &c.title).on_hover_cursor(egui::CursorIcon::PointingHand);
    // contains_pointer, not hovered: the play button on top takes the hover, and the
    // card must stay "hovered" while the pointer is on it or the button would vanish.
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.contains_pointer(), 0.22);
    let img = Rect::from_min_size(rect.min, Vec2::splat(width));
    let radius = if c.round { width / 2.0 } else { theme::card_radius().min(width / 2.0) };
    paint_cover(ui, img, &c.thumbs, radius, 1.0 + 0.06 * h * theme::hover_k());
    // The motion sleeve plays over the still, under the hover shade and play button.
    if let Some(s) = sprite {
        paint_motion(ui, img, radius, s);
    }
    if h > 0.0 && !c.round {
        ui.painter().rect_filled(img, radius, theme::shade(0.28 * h));
    }
    // Play button rises into the corner on hover; on the playing item it stays up
    // and shows pause.
    let active = active_state(ui, &c.target);
    let h = if active.is_some() { 1.0 } else { h };
    let mut play_hit = false;
    if h > 0.01 {
        let center = img.right_bottom() + vec2(-30.0, -30.0 + 8.0 * (1.0 - h));
        let prect = Rect::from_center_size(center, Vec2::splat(44.0));
        let p = ui.interact(prect, resp.id.with("play"), Sense::click());
        let playing = active == Some(true);
        let p = named(p, &format!("{} {}", if playing { "Pause" } else { "Play" }, c.title)).on_hover_cursor(egui::CursorIcon::PointingHand);
        let ph = theme::anim_bool(ui.ctx(), p.id.with("h"), p.hovered(), 0.12);
        ui.painter().circle_filled(center, 22.0 + 2.0 * ph, theme::shade(0.62 * h));
        play_pause_glyph(ui, center, 10.0, theme::with_alpha(Color32::WHITE, h), playing);
        play_hit = p.clicked();
    }
    let tcolor = theme::text();
    let title = galley_wrapped(ui, &c.title, theme::semibold(14.5), tcolor, width, 2);
    let title_h = title.size().y;
    ui.painter().galley(pos2(rect.left(), img.bottom() + 10.0), title, tcolor);
    let sub_y = img.bottom() + 14.0 + title_h;
    linked_text(ui, pos2(rect.left(), sub_y), &c.subtitle, &c.links, theme::regular(13.0), theme::text_dim(), width - 36.0, acts);
    // On the subtitle line, whose width leaves room for it, so a two-line title never sits under it.
    let more_rect = Rect::from_center_size(pos2(rect.right() - 16.0, sub_y + 8.0), Vec2::splat(30.0));
    let more = more_button(ui, more_rect, resp.id.with("more"), "Options");
    egui::Popup::menu(&more).show(|ui| card_menu_contents(ui, c, liked, acts));
    // Right-click anywhere on the tile opens the same menu.
    egui::Popup::context_menu(&resp).show(|ui| card_menu_contents(ui, c, liked, acts));

    if play_hit {
        match &c.target {
            _ if active.is_some() => acts.push(Action::TogglePlay),
            Target::Song(t) => acts.push(Action::Radio(t.clone())),
            t => acts.push(Action::PlayCollection(t.clone(), false)),
        }
    } else if resp.clicked() && !more.clicked() {
        acts.push(Action::Open(c.target.clone()));
    }
}

pub(crate) fn card_menu_contents(ui: &mut Ui, c: &Card, liked: bool, acts: &mut Vec<Action>) {
    match &c.target {
        Target::Song(t) => song_menu(ui, t, liked, None, None, acts),
        t => {
            ui.set_width(226.0);
            ui.add_space(3.0);
            if menu_action(ui, Some(icon::PLAY), "Play") {
                acts.push(Action::PlayCollection(t.clone(), false));
            }
            if menu_action(ui, Some(icon::SHUFFLE), "Shuffle play") {
                acts.push(Action::PlayCollection(t.clone(), true));
            }
            if menu_action(ui, None, "Open") {
                acts.push(Action::Open(t.clone()));
            }
            ui.separator();
            if menu_action(ui, None, "Copy link") {
                acts.push(Action::Share(t.clone()));
            }
            ui.add_space(3.0);
        }
    }
}

fn more_button(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str) -> Response {
    let response = named(ui.interact(rect, id, Sense::click()), label)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) {
        ui.painter().rect_filled(rect, 7.0, theme::glass(0.16));
    }
    let color = if response.hovered() { theme::text() } else { theme::text_dim() };
    icon_at(ui, rect.center(), icon::ELLIPSIS_VERTICAL, 18.0, color);
    response
}

fn menu_row(ui: &mut Ui, label: &str, draw: impl FnOnce(&Ui, Pos2, Color32)) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let response = named(response, label).on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_filled(rect, 7.0, theme::glass(0.12));
    }
    let color = if response.hovered() { theme::text() } else { theme::text_dim() };
    draw(ui, pos2(rect.right() - 19.0, rect.center().y), color);
    ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, label, theme::regular(14.0), theme::text());
    if response.clicked() { ui.close(); }
    response.clicked()
}

pub fn menu_action(ui: &mut Ui, glyph: Option<char>, label: &str) -> bool {
    menu_row(ui, label, |ui, at, color| match glyph {
        Some(glyph) => icon_at(ui, at, glyph, 16.0, color),
        None => {
            ui.painter().text(at, Align2::CENTER_CENTER, "↗", theme::semibold(17.0), color);
        }
    })
}

/// A menu row whose mark is drawn by hand, for one the icon font doesn't have.
pub fn menu_action_drawn(ui: &mut Ui, label: &str, draw: impl FnOnce(&Ui, Pos2, Color32)) -> bool {
    menu_row(ui, label, draw)
}

/// A menu row marked with the app's own star, filled once the song is liked.
fn menu_action_star(ui: &mut Ui, liked: bool, label: &str) -> bool {
    menu_row(ui, label, |ui, at, color| {
        let glyph = if liked { super::glyphs::STAR_FILLED } else { super::glyphs::STAR };
        super::glyphs::draw(ui, at, 16.0, glyph, color);
    })
}

/// `remove`: the song's row in the open playlist, when it's yours and the song can come out.
pub fn song_menu(ui: &mut Ui, t: &Track, liked: bool, remove: Option<usize>, queue_move: Option<(usize, bool, bool)>, acts: &mut Vec<Action>) {
    ui.set_width(226.0);
    ui.add_space(3.0);
    song_actions(ui, t, liked, remove, true, acts);
    ui.separator();
    if menu_action(ui, Some(icon::LIST_PLUS), "Play next") {
        acts.push(Action::PlayNext(t.clone()));
    }
    if menu_action(ui, Some(icon::LIST_MUSIC), "Add to queue") {
        acts.push(Action::Enqueue(t.clone()));
    }
    if menu_action(ui, Some(icon::LIST_MUSIC), "View queue") {
        acts.push(Action::ViewQueue);
    }
    if let Some((i, up, down)) = queue_move {
        if up && menu_action(ui, Some(icon::CHEVRON_UP), "Move up") {
            acts.push(Action::QueueMove(i, -1));
        }
        if down && menu_action(ui, Some(icon::CHEVRON_DOWN), "Move down") {
            acts.push(Action::QueueMove(i, 1));
        }
    }
    if menu_action(ui, Some(icon::RADIO), "Start radio") {
        acts.push(Action::Radio(t.clone()));
    }
    ui.add_space(3.0);
}

/// The song menu's items, for menus that add their own (now playing's options).
/// `like`: whether this menu may change the song's like. The app's row menus do; the
/// player's own ⋯ leaves it to them, so the two menus aren't the same list twice.
pub fn song_actions(ui: &mut Ui, t: &Track, liked: bool, remove: Option<usize>, like: bool, acts: &mut Vec<Action>) {
    if like {
        if menu_action_star(ui, liked, if liked { "Remove like" } else { "Like song" }) {
            acts.push(Action::Like(t.clone(), !liked));
        }
        if menu_action(ui, Some(icon::THUMBS_DOWN), "Dislike") {
            acts.push(Action::Dislike(t.clone()));
        }
        ui.separator();
    }
    if menu_action(ui, Some(icon::LIST_PLUS), "Add to playlist") {
        acts.push(Action::AddToPlaylist(vec![t.clone()]));
    }
    if let Some(i) = remove
        && menu_action(ui, Some(icon::LIST_MINUS), "Remove from playlist")
    {
        acts.push(Action::RemoveFromPlaylist(vec![i]));
    }
    match crate::downloads::path(&t.id) {
        Some(path) => {
            if menu_action(ui, Some(icon::FOLDER_OPEN), "Show in folder") {
                crate::downloads::show_file(&path);
            }
        }
        None => {
            if menu_action(ui, Some(icon::DOWNLOAD), "Download") {
                acts.push(Action::Download(t.clone()));
            }
        }
    }
}

/// Soft drop shadow under a rounded rect (e.g. artwork).
pub fn soft_shadow(ui: &Ui, rect: Rect, radius: f32, blur: f32, alpha: f32) {
    let s = egui::epaint::Shadow { offset: [0, (blur * 0.35) as i8], blur: blur as u8, spread: 0, color: theme::shade(alpha) };
    ui.painter().add(s.as_shape(rect, CornerRadius::same(radius as u8)));
}

/// What's playing, published by the app each frame so cards can show a pause button
/// on the item that's playing.
#[derive(Clone, Default)]
pub struct NowPlaying {
    pub song: Option<String>,
    pub source: Option<Target>,
    pub playing: bool,
}

impl NowPlaying {
    pub fn id() -> egui::Id {
        egui::Id::unique("now-playing")
    }
}

/// `Some(playing)` when `target` is what's loaded: the current song, or the album /
/// playlist / artist the queue came from.
pub fn active_state(ui: &Ui, target: &Target) -> Option<bool> {
    let np = ui.ctx().data(|d| d.get_temp::<NowPlaying>(NowPlaying::id()))?;
    let hit = match target {
        Target::Song(t) => np.song.as_deref() == Some(t.id.as_str()),
        t => np.source.as_ref() == Some(t),
    };
    hit.then_some(np.playing)
}

/// Round button glyph: pause bars while `active` is playing, otherwise a play triangle.
pub fn play_pause_glyph(ui: &Ui, center: Pos2, r: f32, color: Color32, playing: bool) {
    if playing {
        for dx in [-0.45, 0.45] {
            let bar = Rect::from_center_size(center + vec2(dx * r, 0.0), vec2(r * 0.42, r * 1.7));
            ui.painter().rect_filled(bar, r * 0.1, color);
        }
    } else {
        play_triangle(ui, center, r, color);
    }
}

pub fn play_triangle(ui: &Ui, center: Pos2, r: f32, color: Color32) {
    let pts = vec![
        center + vec2(-r * 0.6, -r),
        center + vec2(r * 1.0, 0.0),
        center + vec2(-r * 0.6, r),
    ];
    ui.painter().add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

pub fn pause_bars(ui: &Ui, center: Pos2, r: f32, color: Color32) {
    for dx in [-r * 0.45, r * 0.45] {
        let bar = Rect::from_center_size(center + vec2(dx, 0.0), vec2(r * 0.42, r * 1.7));
        ui.painter().rect_filled(bar, 1.5, color);
    }
}

// ------------------------------------------------------------------ playing bars

/// The playing row's bars: three rounded bars rising and falling at their own slow
/// pace, settling low when paused. They tell the frame pacing they're on screen, so
/// they move at a steady rate instead of the page's slower one.
pub fn equalizer(ui: &Ui, rect: Rect, time: f64, playing: bool, color: Color32) {
    let ctx = ui.ctx();
    let on = theme::anim_bool(ctx, ui.scope_id().with("eq"), playing, 0.35);
    if on > 0.0 {
        let frame = ctx.cumulative_frame_nr();
        ctx.data_mut(|d| d.insert_temp(equalizer_id(), frame));
    }
    let px = ctx.pixels_per_point();
    let snap = |v: f32| (v * px).round() / px;
    let bar_w = snap((rect.width() * 0.22).max(2.0));
    let gap = snap(bar_w * 0.75);
    let total = bar_w * 3.0 + gap * 2.0;
    let left = snap(rect.center().x - total / 2.0);
    let t = time as f32;
    // Two slow waves per bar, so the motion doesn't visibly repeat.
    const WAVES: [(f32, f32, f32); 3] = [(4.1, 2.3, 0.0), (3.3, 5.2, 1.9), (4.7, 2.9, 3.7)];
    for (i, (a, b, off)) in WAVES.into_iter().enumerate() {
        let wave = 0.5 + 0.3 * (t * a + off).sin() + 0.2 * (t * b + off * 1.7).sin();
        let rest = [0.35, 0.6, 0.45][i];
        let amp = rest + (0.3 + 0.7 * wave.clamp(0.0, 1.0) - rest) * on;
        // The height stays fractional (and the rect unrounded): whole-pixel steps
        // read as stutter on bars this small.
        let h = (rect.height() * amp).max(bar_w);
        let x = left + i as f32 * (bar_w + gap);
        let bar = Rect::from_min_max(pos2(x, snap(rect.bottom()) - h), pos2(x + bar_w, snap(rect.bottom())));
        ui.painter().add(egui::epaint::RectShape::filled(bar, bar_w / 2.0, color).with_round_to_pixels(false));
    }
}

/// Last frame the playing bars were drawn on (frame pacing reads it).
pub fn equalizer_id() -> egui::Id {
    egui::Id::unique("equalizer_frame")
}

/// Whether the playing bars were on screen last frame.
pub fn equalizer_shown(ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<u64>(equalizer_id())).is_some_and(|f| f + 1 >= now)
}

pub struct RowOpts {
    pub number: Option<usize>,
    pub show_album: bool,
    pub current: bool,
    pub playing: bool,
    pub accent: Color32,
    pub time: f64,
    pub liked: bool,
    /// In a jam: who added the song.
    pub added_by: Option<String>,
    /// Choosing a match for the Spotify transfer: a click picks the song and its cover plays it.
    pub pick: bool,
    /// Selecting songs: a checkbox leads the row; `Some(true)` when this one is picked.
    pub selected: Option<bool>,
    /// The row's index in your own open playlist, so its menu can take it out.
    pub remove: Option<usize>,
    /// A queue row: its index, so hovering swaps the duration for a remove button.
    pub queue_remove: Option<usize>,
    /// A queue row that may be reordered: its index and whether it can move up and down.
    pub queue_move: Option<(usize, bool, bool)>,
}

const ROW_H: f32 = 56.0;

/// A long list of song rows: only the ones on screen are built; the rest is empty space
/// of the same height. `row(ui, i)` draws row `i`, under an id of its own so its hover
/// and menu state don't jump to another song as the list scrolls.
pub fn visible_rows(ui: &mut Ui, n: usize, row: impl FnMut(&mut Ui, usize)) {
    visible_rows_of(ui, n, ROW_H, row);
}

/// How tall `track_row` draws a song: the Material phone's rows are roomier.
pub fn track_row_height() -> f32 {
    if super::m3::on() { super::m3::row_size(true).0 } else { ROW_H }
}

/// [`visible_rows`] for rows `height` tall.
pub fn visible_rows_of(ui: &mut Ui, n: usize, height: f32, mut row: impl FnMut(&mut Ui, usize)) {
    let h = height + ui.spacing().item_spacing.y;
    let top = ui.cursor().top();
    let clip = ui.clip_rect();
    let first = (((clip.top() - top) / h).floor().max(0.0) as usize).min(n);
    let last = ((((clip.bottom() - top) / h).ceil().max(0.0)) as usize + 1).clamp(first, n);
    ui.add_space(first as f32 * h);
    for i in first..last {
        ui.push_id(i, |ui| row(ui, i));
    }
    ui.add_space((n - last) as f32 * h);
}

/// Song row: cover (or index), title, artists, album, duration. Click plays.
pub fn track_row(ui: &mut Ui, t: &Track, o: &RowOpts, acts: &mut Vec<Action>) -> Response {
    if super::m3::on() {
        return track_row_material(ui, t, o, acts);
    }
    let (rect, base) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return base;
    }
    // Picking: the cover (left 54 px) plays, the rest of the row picks.
    let hit_left = if o.pick && o.number.is_none() { rect.left() + 54.0 } else { rect.left() };
    let hit_rect = Rect::from_min_max(pos2(hit_left, rect.top()), pos2(rect.right() - 42.0, rect.bottom()));
    let resp = named(ui.interact(hit_rect, base.id.with("play"), Sense::click()), &t.title).on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(ui.ctx(), base.id.with("h"), base.contains_pointer(), 0.15);
    if h > 0.0 || o.current {
        let a = if o.current { 0.07 + 0.05 * h } else { 0.07 * h };
        ui.painter().rect_filled(rect, 8.0, theme::glass(a));
    }
    // While selecting, the cover only shows the song (clicks pick) and picked rows are tinted.
    let h = if o.selected.is_some() { 0.0 } else { h };
    if o.selected == Some(true) {
        ui.painter().rect_filled(rect, 8.0, theme::with_alpha(o.accent, 0.14));
    }
    if o.pick {
        let p = theme::anim_bool(ui.ctx(), base.id.with("p"), resp.hovered(), 0.12);
        if p > 0.0 {
            ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(1.5, theme::with_alpha(o.accent, p)), egui::StrokeKind::Inside);
        }
    }
    let mut x = rect.left() + 10.0;
    let cy = rect.center().y;
    if let Some(on) = o.selected {
        checkbox(ui, pos2(x + 9.0, cy), on);
        x += 30.0;
    }
    if let Some(n) = o.number {
        let slot = Rect::from_center_size(pos2(x + 12.0, cy), vec2(24.0, 24.0));
        if o.current {
            equalizer(ui, slot.shrink(4.0), o.time, o.playing, o.accent);
        } else if h > 0.5 {
            play_triangle(ui, slot.center(), 6.0, theme::text());
        } else {
            ui.painter().text(slot.center(), Align2::CENTER_CENTER, n.to_string(), theme::regular(14.0), theme::text_dim());
        }
        x += 36.0;
    } else {
        let img = Rect::from_min_size(pos2(x, cy - 20.0), Vec2::splat(40.0));
        let tr = theme::thumb_radius();
        paint_cover(ui, img, &t.thumbs, tr, 1.0);
        if o.pick {
            let play = named(ui.interact(img, base.id.with("cover"), Sense::click()), "Play").on_hover_text("Play").on_hover_cursor(egui::CursorIcon::PointingHand);
            if play.clicked() {
                acts.push(Action::Play(vec![t.clone()], 0));
            }
        }
        if o.current {
            ui.painter().rect_filled(img, tr, theme::shade(0.45));
            equalizer(ui, img.shrink(10.0), o.time, o.playing, Color32::WHITE);
        } else if h > 0.0 {
            ui.painter().rect_filled(img, tr, theme::shade(0.5 * h));
            play_triangle(ui, img.center(), 7.0, theme::with_alpha(Color32::WHITE, h));
        }
        x += 54.0;
    }
    let right = rect.right() - 51.0;
    let dur_w = 52.0;
    let avail = right - dur_w - x;
    let wide = avail > 520.0;
    // Album rows (numbered) are just song name and plays; other rows are
    // title + artists, then album, then plays when there's room.
    let album_row = o.number.is_some();
    let (title_w, plays_x, album_x) = match (wide, o.show_album, t.plays) {
        (true, true, Some(_)) => (avail * 0.45, Some(x + avail * 0.82), Some(x + avail * 0.5)),
        (true, true, None) => (avail * 0.55, None, Some(x + avail * 0.6)),
        (true, false, Some(_)) => (avail * 0.78, Some(x + avail * 0.82), None),
        _ => (avail, None, None),
    };
    let plays = t.plays.map(|n| format!("{} plays", crate::backend::compact(n)));
    let title_col = if o.current { o.accent } else { theme::text() };
    let sub = match (album_row, &plays, plays_x) {
        (true, Some(p), None) => Some(p.clone()),
        (true, _, _) => None,
        (false, Some(p), None) => Some(format!("{} \u{2022} {p}", t.artist_line())),
        (false, _, _) => Some(t.artist_line()),
    };
    let sub = match (sub, &o.added_by) {
        (Some(s), Some(by)) if !s.is_empty() => Some(format!("{s} \u{2022} Added by {by}")),
        (_, Some(by)) => Some(format!("Added by {by}")),
        (sub, _) => sub,
    };
    let title_y = if sub.is_some() { cy - 18.0 } else { cy - 9.0 };
    text_at(ui, pos2(x, title_y), &t.title, theme::semibold(14.5), title_col, title_w);
    if let Some(sub) = &sub {
        linked_text(ui, pos2(x, cy + 2.0), sub, &t.artists, theme::regular(13.0), theme::text_dim(), title_w, acts);
    }
    if let (Some(px), Some(p)) = (plays_x, &plays) {
        ui.painter().text(pos2(px, cy), Align2::LEFT_CENTER, p, theme::regular(13.5), theme::text_dim());
    }
    if let (Some(ax), Some(album)) = (album_x, &t.album) {
        let w = plays_x.unwrap_or(right - dur_w) - ax - 10.0;
        let g = galley(ui, &album.name, theme::regular(13.5), theme::text_dim(), w);
        let ar = Rect::from_min_size(pos2(ax, cy - 9.0), g.size());
        ui.painter().galley(ar.min, g, theme::text_dim());
        // Album name opens the album, like the artist names.
        if let Some(id) = &album.id {
            let r = named(ui.interact(ar, resp.id.with("album"), Sense::click()), &album.name);
            let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
            if r.hovered() {
                ui.painter().hline(ar.x_range(), ar.bottom() - 1.0, egui::Stroke::new(1.0, theme::text_dim()));
            }
            if r.clicked() {
                acts.push(Action::Open(Target::Album(id.clone())));
            }
        }
    }
    let more_rect = Rect::from_center_size(pos2(rect.right() - 20.0, cy), Vec2::splat(30.0));
    let more = more_button(ui, more_rect, base.id.with("more"), "Song options");
    // A queue row gives the duration's slot to a remove button while the pointer is
    // anywhere left of the ⋯, so the two controls never land on each other.
    let queue = o.queue_remove.filter(|_| ui.rect_contains_pointer(Rect::from_min_max(rect.min, pos2(more_rect.left(), rect.bottom()))));
    if let Some(d) = t.duration.filter(|_| queue.is_none()) {
        ui.painter().text(pos2(right, cy), Align2::RIGHT_CENTER, super::fmt_time(d), theme::regular(13.0), theme::text_dim());
    }
    if let Some(i) = queue {
        let slot = Rect::from_center_size(pos2(right - dur_w / 2.0, cy), Vec2::splat(28.0));
        let xr = named(ui.interact(slot, base.id.with("rm"), Sense::click()), "Remove from queue").on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.painter().circle_filled(slot.center(), 14.0, theme::panel(0.6));
        icon_at(ui, slot.center(), icon::X, 14.0, theme::text());
        if xr.clicked() {
            acts.push(Action::QueueRemove(i));
        }
    }
    egui::Popup::menu(&more).show(|ui| song_menu(ui, t, o.liked, o.remove, o.queue_move, acts));
    // Right-click anywhere on the row opens the same menu.
    egui::Popup::context_menu(&resp).show(|ui| song_menu(ui, t, o.liked, o.remove, o.queue_move, acts));
    resp
}

/// What a song row says under its title: its artists, its plays where they have no
/// column of their own, and in a jam who added it. An album's rows name no artist.
fn track_row_sub(t: &Track, o: &RowOpts) -> Option<String> {
    let plays = t.plays.map(|n| format!("{} plays", crate::backend::compact(n)));
    let sub = match (o.number.is_some(), plays) {
        (true, plays) => plays,
        (false, Some(p)) => Some(format!("{} \u{2022} {p}", t.artist_line())),
        (false, None) => Some(t.artist_line()),
    };
    match (sub, &o.added_by) {
        (Some(s), Some(by)) if !s.is_empty() => Some(format!("{s} \u{2022} Added by {by}")),
        (_, Some(by)) => Some(format!("Added by {by}")),
        (sub, _) => sub,
    }
}

/// The song row of the Material phone: a list row with the song's cover (an album's
/// rows are numbered instead), its duration and its menu. The song playing stands on a
/// tonal container, its title in the primary colour and the bars over its cover.
fn track_row_material(ui: &mut Ui, t: &Track, o: &RowOpts, acts: &mut Vec<Action>) -> Response {
    use super::m3;
    use crate::material::{self, Type, shape};
    let m = material::scheme();
    let (rect, base) = ui.allocate_exact_size(vec2(ui.available_width(), track_row_height()), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return base;
    }
    let cy = rect.center().y;
    // While selecting, a tick box leads the row.
    let left = rect.left() + if o.selected.is_some() { 32.0 } else { 0.0 };
    let lead = if o.number.is_some() { 28.0 } else { m3::row_size(true).1 };
    let duration = t.duration.map(|d| m3::galley(ui, &super::fmt_time(d), Type::BodyMedium, m.on_surface_variant, 80.0));
    let g = m3::row_geom(Rect::from_min_max(pos2(left, rect.top()), rect.max), lead, duration.as_ref().map_or(0.0, |d| d.size().x), true);
    let more_at = g.more.map_or(rect.right_center(), |r| r.center());
    // Picking: the cover plays, the rest of the row picks.
    let hit_left = if o.pick && o.number.is_none() { g.cover.right() + 6.0 } else { rect.left() };
    let hit = Rect::from_min_max(pos2(hit_left, rect.top()), pos2(more_at.x - m3::TOUCH / 2.0, rect.bottom()));
    let resp = named(ui.interact(hit, base.id.with("play"), Sense::click()), &t.title);
    let wash = rect.expand2(vec2(8.0, 0.0));
    if o.current {
        ui.painter().rect_filled(wash, shape::CARD_SMALL, m.surface_container);
    }
    if o.selected == Some(true) {
        ui.painter().rect_filled(wash, shape::CARD_SMALL, m.secondary_container.gamma_multiply(0.7));
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(wash, shape::CARD_SMALL, m.on_surface.gamma_multiply(0.1));
    }
    if let Some(on) = o.selected {
        let r = Rect::from_center_size(pos2(rect.left() + 12.0, cy), Vec2::splat(18.0));
        if on {
            ui.painter().rect_filled(r, 4.0, m.primary);
            icon_at(ui, r.center(), icon::CHECK, 13.0, m.on_primary);
        } else {
            ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(2.0, m.on_surface_variant), egui::StrokeKind::Inside);
        }
    }
    if let Some(n) = o.number {
        if o.current {
            equalizer(ui, Rect::from_center_size(g.cover.center(), Vec2::splat(18.0)), o.time, o.playing, m.primary);
        } else {
            let number = m3::galley(ui, &n.to_string(), Type::BodyMedium, m.on_surface_variant, 40.0);
            ui.painter().galley(g.cover.center() - number.size() / 2.0, number, m.on_surface_variant);
        }
    } else {
        paint_cover(ui, g.cover, &t.thumbs, shape::TILE_SMALL, 1.0);
        if o.pick && named(ui.interact(g.cover, base.id.with("cover"), Sense::click()), "Play").clicked() {
            acts.push(Action::Play(vec![t.clone()], 0));
        }
        if o.current {
            ui.painter().rect_filled(g.cover, shape::TILE_SMALL, theme::shade(0.5));
            equalizer(ui, g.cover.shrink(15.0), o.time, o.playing, m.primary);
        }
    }
    let title_color = if o.current { m.primary } else { m.on_surface };
    let title = m3::galley(ui, &t.title, Type::BodyLarge, title_color, g.text.width());
    match track_row_sub(t, o) {
        Some(sub) => {
            let sub_role = Type::BodyMedium.spec();
            let top = cy - (title.size().y + sub_role.line_height) / 2.0;
            let sub_top = top + title.size().y;
            ui.painter().galley(pos2(g.text.left(), top), title, title_color);
            linked_text(ui, pos2(g.text.left(), sub_top), &sub, &t.artists, sub_role.font(), m.on_surface_variant, g.text.width(), acts);
        }
        None => {
            ui.painter().galley(pos2(g.text.left(), cy - title.size().y / 2.0), title, title_color);
        }
    }
    // A queue row gives the duration's slot to a remove button while a finger is on it.
    let queue = o.queue_remove.filter(|_| ui.rect_contains_pointer(hit));
    match (queue, duration) {
        (Some(i), _) => {
            let at = pos2(g.trailing_right - m3::TOUCH / 2.0 + 8.0, cy);
            if m3::IconButton::new(icon::X, "Remove from queue").show_at(ui, at, base.id.with("rm")).clicked() {
                acts.push(Action::QueueRemove(i));
            }
        }
        (None, Some(d)) => {
            ui.painter().galley(pos2(g.trailing_right - d.size().x, cy - d.size().y / 2.0), d, m.on_surface_variant);
        }
        (None, None) => {}
    }
    let more = m3::IconButton::new(icon::ELLIPSIS_VERTICAL, "Song options").show_at(ui, more_at, base.id.with("more"));
    egui::Popup::menu(&more).show(|ui| song_menu(ui, t, o.liked, o.remove, o.queue_move, acts));
    // A long press anywhere on the row opens the same menu.
    egui::Popup::context_menu(&resp).show(|ui| song_menu(ui, t, o.liked, o.remove, o.queue_move, acts));
    resp
}

/// An 18 px tick box centred on `c`, white like a primary pill when ticked.
fn checkbox(ui: &Ui, c: Pos2, on: bool) {
    let r = Rect::from_center_size(c, Vec2::splat(18.0));
    if on {
        ui.painter().rect_filled(r, 5.0, theme::text());
        icon_at(ui, c, icon::CHECK, 13.0, theme::on_text());
    } else {
        ui.painter().rect_stroke(r, 5.0, egui::Stroke::new(1.5, theme::text_dim()), egui::StrokeKind::Inside);
    }
}

/// Section header with optional strapline/avatar and ‹ › scroll arrows.
/// Returns (scroll left, scroll right) clicks.
pub fn shelf_header(ui: &mut Ui, title: &str, strap: Option<&str>, strap_thumb: Option<&str>, arrows: bool) -> (bool, bool) {
    let mut res = (false, false);
    ui.add_space(34.0);
    ui.horizontal(|ui| {
        if let Some(url) = strap_thumb {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 20.0, theme::glass(0.1));
            paint_remote(ui, url, r, 20.0, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            ui.add_space(4.0);
        }
        ui.vertical(|ui| {
            if let Some(s) = strap {
                ui.label(egui::RichText::new(s.to_uppercase()).font(theme::semibold(11.5)).color(theme::text_dim()));
            }
            ui.label(egui::RichText::new(title).font(theme::bold(21.0)).color(theme::text()));
        });
        if arrows {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                res.1 = icon_button(ui, icon::CHEVRON_RIGHT, 18.0, &format!("Scroll {title} right"), false).clicked();
                res.0 = icon_button(ui, icon::CHEVRON_LEFT, 18.0, &format!("Scroll {title} left"), false).clicked();
            });
        }
    });
    ui.add_space(14.0);
    res
}

/// Aligned starts for complete viewport pages, with a full final page.
fn shelf_page_starts(count: usize, visible: usize) -> Vec<usize> {
    let visible = visible.max(1);
    let last = count.saturating_sub(visible);
    let mut starts = vec![0];
    let mut next = visible;
    while next < last {
        starts.push(next);
        next += visible;
    }
    if last > *starts.last().unwrap() {
        starts.push(last);
    }
    starts
}

fn shelf_page_target(starts: &[usize], step: f32, from: f32, nudge: i32) -> f32 {
    let next = if nudge > 0 {
        starts.iter().copied().find(|&start| start as f32 * step > from + 0.5).unwrap_or(*starts.last().unwrap())
    } else {
        starts.iter().copied().rev().find(|&start| (start as f32 * step) < from - 0.5).unwrap_or(starts[0])
    };
    next as f32 * step
}

/// How a finished horizontal swipe turns the shelf: `1` the next page (finger moved left),
/// `-1` the previous, `0` back to where it was. A swipe long enough or quick enough counts.
pub fn swipe_direction(dx: f32, vx: f32) -> i32 {
    const LONG: f32 = 40.0;
    const QUICK: f32 = 300.0;
    if dx < -LONG || vx < -QUICK {
        1
    } else if dx > LONG || vx > QUICK {
        -1
    } else {
        0
    }
}

/// The page start (in pixels) closest to `offset`.
fn nearest_page(starts: &[usize], step: f32, offset: f32) -> f32 {
    starts.iter().map(|&s| s as f32 * step).min_by(|a, b| (a - offset).abs().total_cmp(&(b - offset).abs())).unwrap_or(0.0)
}

/// Where a shelf lying between two pages should settle; `None` when it is on one.
fn shelf_rest(starts: &[usize], step: f32, offset: f32) -> Option<f32> {
    let page = nearest_page(starts, step, offset);
    ((page - offset).abs() > 0.5).then_some(page)
}

/// A colour for `seed` (0..1 around the colour wheel): the same every time, and spread out
/// so folders named alike still look different.
pub fn hue_of(seed: &str) -> f32 {
    // FNV-1a, whose top bits are spread evenly over 0..1.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    ((h >> 11) as f64 / (1u64 << 53) as f64) as f32
}

/// A rounded square of soft colour for a folder: a diagonal blend from one hue to a deeper
/// neighbour, a glow in the top-left corner and a shade along the bottom. Transparent
/// outside its rounded corners.
pub fn folder_art_image(seed: &str, size: usize) -> egui::ColorImage {
    use egui::ecolor::Hsva;
    let hue = hue_of(seed);
    let top = Color32::from(Hsva::new(hue, 0.50, 0.98, 1.0));
    let bottom = Color32::from(Hsva::new((hue + 0.09).fract(), 0.78, 0.52, 1.0));
    let n = size as f32;
    let radius = n * 0.21;
    let mut pixels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Distance outside the rounded square (negative inside): the edge's softness.
            let (dx, dy) = ((px - n / 2.0).abs() - (n / 2.0 - radius), (py - n / 2.0).abs() - (n / 2.0 - radius));
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - radius;
            let alpha = (0.5 - outside).clamp(0.0, 1.0);
            let t = ((px + py) / (2.0 * n)).clamp(0.0, 1.0);
            let base = theme::lerp_color(top, bottom, t);
            // The glow near the top-left, the shade along the bottom.
            let glow = (1.0 - ((px / n - 0.18).hypot(py / n - 0.12)) * 1.7).clamp(0.0, 1.0).powi(2) * 0.28;
            let shade = ((py / n - 0.62) / 0.38).clamp(0.0, 1.0) * 0.22;
            let lit = theme::lerp_color(theme::lerp_color(base, Color32::WHITE, glow), Color32::BLACK, shade);
            pixels.push(Color32::from_rgba_unmultiplied(lit.r(), lit.g(), lit.b(), (alpha * 255.0).round() as u8));
        }
    }
    egui::ColorImage { size: [size, size], pixels, source_size: egui::vec2(n, n) }
}

/// The art for a folder as a texture, made once per folder name.
pub fn folder_art(ctx: &egui::Context, seed: &str) -> egui::TextureHandle {
    let id = egui::Id::NULL.with(("folder-art", seed));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let t = ctx.load_texture(format!("folder-art-{seed}"), folder_art_image(seed, 192), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}

/// Which way a touch drag is going once it has moved far enough to tell: `Some(true)`
/// horizontal, `Some(false)` vertical, `None` while it is still within the slop.
pub fn drag_axis(delta: Vec2) -> Option<bool> {
    const SLOP: f32 = 10.0;
    (delta.length() > SLOP).then(|| delta.x.abs() > delta.y.abs())
}

/// A shelf follows a finger only when the drag is horizontal; a vertical one belongs to
/// the page. State kept between frames: where the shelf was, and the axis once known.
#[derive(Clone, Copy)]
struct ShelfDrag {
    start_offset: f32,
    axis: Option<bool>,
    /// How far the finger has moved sideways since it went down.
    dx: f32,
}

/// Horizontal scroller that animates between aligned pages of cards or song columns.
pub fn h_scroll<R>(ui: &mut Ui, id: &str, nudge: i32, count: usize, item_width: f32, gap: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    h_scroll_on(ui, crate::platform::phone(), id, nudge, count, item_width, gap, add)
}

/// [`h_scroll`], told whether a finger swipes it (a phone) or a wheel and the arrows do.
#[allow(clippy::too_many_arguments)]
fn h_scroll_on<R>(ui: &mut Ui, phone: bool, id: &str, nudge: i32, count: usize, item_width: f32, gap: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let sid = egui::Id::unique(("hscroll", id));
    let anim_id = sid.with("anim");
    // ScrollArea hashes its id_salt once more. Read that same ID, or left clicks
    // always appear to start from zero.
    let state_id = ui.make_persistent_id(egui::IdSalt::new(sid));
    let current = egui::scroll_area::State::load(ui.ctx(), state_id).map_or(0.0, |s| s.clamped_offset().x);
    let target_id = sid.with("target");
    let viewport_id = sid.with("viewport");
    let viewport = ui.ctx().data(|d| d.get_temp::<f32>(viewport_id)).unwrap_or_else(|| ui.available_width());
    let step = item_width + gap;
    let visible = ((viewport + gap) / step).floor().max(1.0) as usize;
    let starts = shelf_page_starts(count, visible);
    let last_start = *starts.last().unwrap();
    let mut reached_end = false;
    let mut target: Option<f32> = ui.ctx().data(|d| d.get_temp(target_id));
    if nudge != 0 {
        let t = shelf_page_target(&starts, step, target.unwrap_or(current), nudge);
        reached_end = nudge > 0 && count > visible && (t - last_start as f32 * step).abs() < 0.5;
        // Snap the animator to the real position, then glide to the target.
        ui.ctx().animate_value_with_time(anim_id, current, 0.0);
        target = Some(t);
        ui.ctx().data_mut(|d| d.insert_temp(target_id, t));
    }
    let mut area = egui::ScrollArea::horizontal().id_salt(sid).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    let drag_id = sid.with("shelf-drag");
    let held_id = sid.with("shelf-held");
    if phone {
        // The shelf's own drag would take every swipe that starts on it, vertical ones
        // too, and the page (mostly shelves) would hardly scroll: it follows the finger
        // itself below, and only when the swipe is horizontal.
        area = area.scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() });
        if let Some(x) = ui.ctx().data(|d| d.get_temp::<f32>(held_id)) {
            area = area.horizontal_scroll_offset(x);
        }
    }
    if let Some(t) = target {
        let v = ui.ctx().animate_value_with_time(anim_id, t, 0.35 * theme::SPEED);
        area = area.horizontal_scroll_offset(v);
        if (v - t).abs() < 0.5 {
            ui.ctx().data_mut(|d| d.remove::<f32>(target_id));
        }
    }
    // Give the last page enough trailing room to land exactly on its first item.
    let content_width = (last_start as f32 * step + viewport).max(count as f32 * step - gap);
    let out = area.show(ui, |ui| ui.horizontal_top(|ui| {
        ui.set_min_width(content_width);
        add(ui)
    }).inner);
    ui.ctx().data_mut(|d| d.insert_temp(viewport_id, out.inner_rect.width()));
    if phone {
        let pages = Pages { starts: &starts, step, max: (out.content_size.x - out.inner_rect.width()).max(0.0) };
        let touch = Touch { rect: shelf_rect(out.inner_rect, out.content_size, ui.clip_rect()), layer: ui.layer_id() };
        follow_horizontal_swipe(ui.ctx(), touch, out.state.clamped_offset().x, &pages, drag_id, held_id, target_id, anim_id);
        // Swiped to its last page: the end is reached as the arrows reach it.
        reached_end |= at_last_page(count, visible, step, out.state.clamped_offset().x);
    }
    (out.inner, reached_end)
}

/// Is a shelf of `count` items, `visible` to a page, scrolled `offset` to its last page?
/// A shelf that fits on one page has no end to reach.
fn at_last_page(count: usize, visible: usize, step: f32, offset: f32) -> bool {
    count > visible && offset >= (count - visible) as f32 * step - 0.5
}

/// Where a shelf is on the screen. `inner` is the room the scroll area was offered, not
/// what it took: it reaches down over the shelves below (a swipe on one of those moved
/// this one too) and is empty past the first screenful of the page (those never moved).
/// The shelf itself is as tall as its content, and only what `clip` shows of it counts.
fn shelf_rect(inner: Rect, content: Vec2, clip: Rect) -> Rect {
    Rect::from_min_size(inner.min, vec2(inner.width(), content.y)).intersect(clip)
}

/// What a finger must have come down on for a swipe to be a shelf's.
#[derive(Clone, Copy)]
struct Touch {
    rect: Rect,
    /// The page's layer: a touch on something drawn over it (Now Playing, the mini
    /// player, a menu) is not the shelf's.
    layer: egui::LayerId,
}

impl Touch {
    fn began_at(&self, ctx: &egui::Context, origin: Pos2) -> bool {
        self.rect.contains(origin) && ctx.layer_id_at(origin).is_none_or(|top| top == self.layer)
    }
}

/// The shelf's aligned pages, for a swipe to land on.
struct Pages<'a> {
    starts: &'a [usize],
    step: f32,
    max: f32,
}

/// The id the page checks: a shelf is being swiped sideways, so the page itself holds still.
pub fn shelf_swipe_id() -> egui::Id {
    egui::Id::NULL.with("shelf-swiping")
}

/// Move a shelf with a finger that started on it and went horizontally, then turn it one
/// page (as the arrows do) when let go. A vertical swipe is left to the page.
#[allow(clippy::too_many_arguments)]
fn follow_horizontal_swipe(ctx: &egui::Context, touch: Touch, offset: f32, pages: &Pages, drag_id: egui::Id, held_id: egui::Id, target_id: egui::Id, anim_id: egui::Id) {
    let (origin, latest, down, velocity) = ctx.input(|i| (i.pointer.press_origin(), i.pointer.latest_pos(), i.pointer.primary_down(), i.pointer.velocity()));
    let drag: Option<ShelfDrag> = ctx.data(|d| d.get_temp(drag_id));
    match (down, origin.filter(|o| touch.began_at(ctx, *o)), latest) {
        (true, Some(origin), Some(now)) => hold(ctx, drag, offset, now - origin, pages.max, drag_id, held_id),
        _ => {
            if let Some(d) = drag {
                let_go(ctx, drag_id, held_id);
                if d.axis == Some(true) {
                    // From the page it started on, one page the way the finger went (or
                    // back to it); the shelf glides there from where the finger left it.
                    let from = nearest_page(pages.starts, pages.step, d.start_offset);
                    let to = match swipe_direction(d.dx, velocity.x) {
                        0 => from,
                        dir => shelf_page_target(pages.starts, pages.step, from, dir),
                    };
                    ctx.animate_value_with_time(anim_id, offset, 0.0);
                    ctx.data_mut(|data| data.insert_temp(target_id, to));
                    ctx.request_repaint();
                }
            } else if ctx.data(|data| data.get_temp::<f32>(target_id)).is_none()
                && let Some(page) = shelf_rest(pages.starts, pages.step, offset).filter(|p| *p <= pages.max + 0.5)
            {
                // Left between two pages (a position saved before its columns changed
                // width, a glide cut short): it settles on the nearer one. A page the
                // content doesn't reach yet is not a place to glide to.
                ctx.animate_value_with_time(anim_id, offset, 0.0);
                ctx.data_mut(|data| data.insert_temp(target_id, page));
                ctx.request_repaint();
            }
        }
    }
}

/// A finger that came down on a shelf is still down, `delta` from where it landed: once
/// it is going sideways the shelf (scrolled `offset`, at most `max`) goes with it.
fn hold(ctx: &egui::Context, drag: Option<ShelfDrag>, offset: f32, delta: Vec2, max: f32, drag_id: egui::Id, held_id: egui::Id) {
    let mut d = drag.unwrap_or(ShelfDrag { start_offset: offset, axis: None, dx: 0.0 });
    d.dx = delta.x;
    if d.axis.is_none() {
        d.axis = drag_axis(delta);
    }
    if d.axis == Some(true) {
        ctx.data_mut(|data| {
            data.insert_temp(held_id, (d.start_offset - delta.x).clamp(0.0, max));
            data.insert_temp(shelf_swipe_id(), true);
        });
        ctx.request_repaint();
    }
    ctx.data_mut(|data| data.insert_temp(drag_id, d));
}

/// The finger is up: the shelf is its own again, and so is the page.
fn let_go(ctx: &egui::Context, drag_id: egui::Id, held_id: egui::Id) {
    ctx.data_mut(|data| {
        data.remove::<ShelfDrag>(drag_id);
        data.remove::<f32>(held_id);
        data.remove::<bool>(shelf_swipe_id());
    });
}

/// A row wider than the screen that has no pages (chips): on a phone it follows a
/// finger going sideways and stays where it is let go; a finger going up or down is
/// the page's.
pub fn h_strip<R>(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    h_strip_on(ui, crate::platform::phone(), id, add)
}

/// [`h_strip`], told whether a finger swipes it (a phone) or a wheel does.
fn h_strip_on<R>(ui: &mut Ui, phone: bool, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let sid = egui::Id::unique(("hstrip", id));
    let (drag_id, held_id) = (sid.with("strip-drag"), sid.with("strip-held"));
    let mut area = egui::ScrollArea::horizontal().id_salt(sid).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if phone {
        // As for a shelf: its own drag would take the page's vertical swipes too.
        area = area.scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() });
        if let Some(x) = ui.ctx().data(|d| d.get_temp::<f32>(held_id)) {
            area = area.horizontal_scroll_offset(x);
        }
    }
    let out = area.show(ui, |ui| ui.horizontal(|ui| add(ui)).inner);
    if phone {
        let ctx = ui.ctx();
        let touch = Touch { rect: shelf_rect(out.inner_rect, out.content_size, ui.clip_rect()), layer: ui.layer_id() };
        let max = (out.content_size.x - out.inner_rect.width()).max(0.0);
        let (origin, latest, down) = ctx.input(|i| (i.pointer.press_origin(), i.pointer.latest_pos(), i.pointer.primary_down()));
        let drag: Option<ShelfDrag> = ctx.data(|d| d.get_temp(drag_id));
        match (down, origin.filter(|o| touch.began_at(ctx, *o)), latest) {
            (true, Some(origin), Some(now)) => hold(ctx, drag, out.state.clamped_offset().x, now - origin, max, drag_id, held_id),
            _ if drag.is_some() => let_go(ctx, drag_id, held_id),
            _ => {}
        }
    }
    out.inner
}

/// The skeleton's light band sweeping across `r`, for anything still loading.
pub fn shimmer_band(ui: &Ui, r: Rect, radius: f32, time: f64) {
    let phase = ((time * 0.8) % 1.6) as f32 - 0.3;
    let x = r.left() + (r.width() + 400.0) * phase - 200.0;
    let band = Rect::from_min_max(pos2((x - 120.0).max(r.left()), r.top()), pos2((x + 120.0).min(r.right()), r.bottom()));
    if band.width() > 0.0 {
        ui.painter().rect_filled(band, radius, theme::glass(0.04));
    }
}

/// Paint a remote image into `rect`: the skeleton's shimmer while it downloads (not
/// egui's spinner), then a quick fade-in. One already loaded just shows.
pub fn paint_remote(ui: &Ui, url: &str, rect: Rect, radius: f32, uv: Rect, tint: Color32) {
    paint_remote_rounded(ui, url, rect, CornerRadius::same(radius as u8), uv, tint);
}

/// [`paint_remote`] with each corner's own radius.
pub fn paint_remote_rounded(ui: &Ui, url: &str, rect: Rect, corners: CornerRadius, uv: Rect, tint: Color32) {
    const FADE: f64 = 0.22;
    let ctx = ui.ctx();
    let now = ctx.input(|i| i.time);
    let id = egui::Id::unique(("img-fade", url));
    match egui::Image::new(url).load_for_size(ctx, rect.size()) {
        Ok(egui::load::TexturePoll::Ready { texture }) => {
            // Fade only what was seen loading; Some(None) = waiting, Some(Some(t)) = fading since t.
            let a = match ctx.data(|d| d.get_temp::<Option<f64>>(id)) {
                Some(Some(t0)) if now - t0 < FADE => ((now - t0) / FADE) as f32,
                Some(None) => {
                    ctx.data_mut(|d| d.insert_temp(id, Some(now)));
                    0.0
                }
                _ => 1.0,
            };
            if a < 1.0 {
                ctx.request_repaint();
            }
            egui::Image::from_texture(texture).uv(uv).corner_radius(corners).tint(tint.gamma_multiply(theme::ease_out(a))).paint_at(ui, rect);
        }
        Ok(egui::load::TexturePoll::Pending { .. }) => {
            ctx.data_mut(|d| d.insert_temp::<Option<f64>>(id, None));
            shimmer_band(ui, rect, corners.average(), now);
            ctx.request_repaint_after(std::time::Duration::from_millis(66));
        }
        // A failed image keeps its plain placeholder.
        Err(_) => {}
    }
}

/// The newest decoded frame of a motion sleeve, uploaded by the player.
pub struct MotionSprite {
    pub id: egui::TextureId,
    pub size: Vec2,
}

/// Paint a motion frame into `rect`, centre-cropped like the still sleeve it replaces.
pub fn paint_motion(ui: &Ui, rect: Rect, radius: f32, s: &MotionSprite) {
    if !ui.is_rect_visible(rect) || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let tex_aspect = s.size.x / s.size.y;
    let rect_aspect = rect.width() / rect.height();
    let uv = if rect_aspect > tex_aspect {
        Rect::from_center_size(pos2(0.5, 0.5), vec2(1.0, tex_aspect / rect_aspect))
    } else {
        Rect::from_center_size(pos2(0.5, 0.5), vec2(rect_aspect / tex_aspect, 1.0))
    };
    let shape = egui::epaint::RectShape::filled(rect, CornerRadius::same(radius as u8), Color32::WHITE)
        .with_texture(s.id, uv);
    ui.painter().add(shape);
}

/// Song rows still on their way, at the end of a list that's coming in.
pub fn skeleton_rows(ui: &mut Ui, time: f64, rows: usize) {
    for _ in 0..rows {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
        if !ui.is_rect_visible(r) {
            continue;
        }
        let cy = r.center().y;
        for (b, radius) in [
            (Rect::from_min_size(pos2(r.left() + 10.0, cy - 20.0), Vec2::splat(40.0)), theme::thumb_radius()),
            (Rect::from_min_size(pos2(r.left() + 64.0, cy - 13.0), vec2((r.width() * 0.3).min(260.0), 11.0)), 4.0),
            (Rect::from_min_size(pos2(r.left() + 64.0, cy + 4.0), vec2((r.width() * 0.18).min(160.0), 9.0)), 4.0),
        ] {
            ui.painter().rect_filled(b, radius, theme::glass(0.06));
            shimmer_band(ui, b, radius, time);
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(66));
    }
}

/// Shimmering placeholder blocks while a page loads.
pub fn skeleton(ui: &mut Ui, time: f64, rows: usize, card: f32) {
    let shimmer = |ui: &Ui, r: Rect, radius: f32| {
        ui.painter().rect_filled(r, radius, theme::glass(0.06));
        shimmer_band(ui, r, radius, time);
    };
    let top = ui.cursor().top();
    for _ in 0..rows {
        ui.add_space(26.0);
        let (t, _) = ui.allocate_exact_size(vec2(220.0, 26.0), Sense::hover());
        shimmer(ui, t, 6.0);
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            for _ in 0..8 {
                let (r, _) = ui.allocate_exact_size(vec2(card, card + 50.0), Sense::hover());
                shimmer(ui, Rect::from_min_size(r.min, Vec2::splat(card)), 8.0);
                shimmer(ui, Rect::from_min_size(r.min + vec2(0.0, card + 12.0), vec2(card * 0.8, 12.0)), 4.0);
                shimmer(ui, Rect::from_min_size(r.min + vec2(0.0, card + 32.0), vec2(card * 0.5, 10.0)), 4.0);
            }
        });
    }
    // The shimmer needs frames only while it's on screen, and 15 fps is plenty.
    if ui.is_rect_visible(Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top())) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(66));
    }
}

#[cfg(test)]
mod folder_art_tests {
    use super::{folder_art_image, hue_of};

    #[test]
    fn a_folder_always_gets_the_same_colour_and_others_get_other_ones() {
        assert_eq!(hue_of("Japanese"), hue_of("Japanese"));
        assert_ne!(hue_of("Japanese"), hue_of("English"));
        assert!((0.0..1.0).contains(&hue_of("anything")));
    }

    #[test]
    fn the_art_is_a_rounded_tile_with_clear_corners_and_a_solid_middle() {
        let img = folder_art_image("Japanese", 96);
        assert_eq!(img.size, [96, 96]);
        assert_eq!(img.pixels[0].a(), 0, "the corner is cut away");
        assert_eq!(img.pixels[48 * 96 + 48].a(), 255, "the middle is solid");
        assert_eq!(img.pixels[48 * 96].a(), 255, "the straight edges are kept");
    }

    #[test]
    fn two_folders_do_not_look_alike() {
        let a = folder_art_image("Japanese", 64);
        let b = folder_art_image("English", 64);
        assert_ne!(a.pixels[32 * 64 + 32], b.pixels[32 * 64 + 32]);
    }
}

#[cfg(test)]
mod shelf_scroll_tests {
    use super::{drag_axis, h_scroll_on, nearest_page, shelf_page_starts, shelf_page_target, shelf_rest, swipe_direction};
    use egui::{Pos2, Rect, Sense, pos2, vec2};

    #[test]
    fn a_short_slow_swipe_stays_on_the_page() {
        assert_eq!(swipe_direction(-20.0, -100.0), 0);
        assert_eq!(swipe_direction(15.0, 50.0), 0);
    }

    #[test]
    fn a_long_or_quick_swipe_turns_one_page_either_way() {
        assert_eq!(swipe_direction(-90.0, -50.0), 1, "left: the next page");
        assert_eq!(swipe_direction(-15.0, -900.0), 1, "a quick flick left");
        assert_eq!(swipe_direction(120.0, 0.0), -1, "right: the previous page");
        assert_eq!(swipe_direction(10.0, 1500.0), -1, "a quick flick right");
    }

    #[test]
    fn a_shelf_left_between_two_pages_settles_on_the_nearer() {
        let pages = [0, 1, 2, 3];
        // Restored from before an update made its columns wider, or stopped mid-glide.
        assert_eq!(shelf_rest(&pages, 364.0, 223.0), Some(364.0));
        assert_eq!(shelf_rest(&pages, 364.0, 120.0), Some(0.0));
    }

    #[test]
    fn a_shelf_on_a_page_stays_put() {
        let pages = [0, 1, 2, 3];
        assert_eq!(shelf_rest(&pages, 364.0, 0.0), None);
        assert_eq!(shelf_rest(&pages, 364.0, 728.2), None);
    }

    #[test]
    fn the_nearest_page_start_is_found_from_any_offset() {
        let pages = shelf_page_starts(14, 5);
        assert_eq!(nearest_page(&pages, 198.0, 0.0), 0.0);
        assert_eq!(nearest_page(&pages, 198.0, 5.0 * 198.0 - 30.0), 5.0 * 198.0);
        assert_eq!(nearest_page(&pages, 198.0, 9.0 * 198.0 + 400.0), 9.0 * 198.0);
    }

    #[test]
    fn a_small_movement_does_not_decide_the_axis() {
        assert_eq!(drag_axis(vec2(6.0, 5.0)), None);
        assert_eq!(drag_axis(vec2(0.0, 0.0)), None);
    }

    #[test]
    fn a_swipe_mostly_sideways_belongs_to_the_shelf() {
        assert_eq!(drag_axis(vec2(30.0, 8.0)), Some(true));
        assert_eq!(drag_axis(vec2(-24.0, -10.0)), Some(true));
    }

    #[test]
    fn a_swipe_mostly_up_or_down_belongs_to_the_page() {
        assert_eq!(drag_axis(vec2(6.0, -40.0)), Some(false));
        assert_eq!(drag_axis(vec2(-9.0, 25.0)), Some(false));
    }

    /// A phone page of six shelves in a headless egui, 400 x 600: each shelf is 160 tall
    /// under a 40 pt gap, so only the first three fit on the screen before scrolling.
    struct Shelves {
        ctx: egui::Context,
        /// How far down the page is scrolled.
        page_y: f32,
        /// Something drawn over the page (as Now Playing or the mini player is).
        cover: Option<Rect>,
        time: f64,
    }

    impl Shelves {
        fn new(page_y: f32) -> Shelves {
            let mut s = Shelves { ctx: egui::Context::default(), page_y, cover: None, time: 0.0 };
            // Two frames to settle the layout before a finger comes down.
            s.frame(vec![]);
            s.frame(vec![]);
            s
        }

        /// One frame; where each shelf's first tile was drawn (its left edge moves left
        /// as the shelf scrolls).
        fn frame(&mut self, events: Vec<egui::Event>) -> Vec<f32> {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0))), time: Some(self.time), events, ..Default::default() };
            let mut lefts = Vec::new();
            let (page_y, cover) = (self.page_y, self.cover);
            let out = self.ctx.run_ui(input, |ui| {
                egui::ScrollArea::vertical().id_salt("page").auto_shrink(false).vertical_scroll_offset(page_y).show(ui, |ui| {
                    for i in 0..6 {
                        ui.add_space(40.0);
                        h_scroll_on(ui, true, &format!("test-shelf-{i}"), 0, 12, 100.0, 20.0, |ui| {
                            ui.spacing_mut().item_spacing.x = 20.0;
                            lefts.push(ui.cursor().min.x);
                            for _ in 0..12 {
                                ui.allocate_exact_size(vec2(100.0, 160.0), Sense::hover());
                            }
                        });
                    }
                });
                if let Some(rect) = cover {
                    egui::Area::new(egui::Id::unique("test-cover")).order(egui::Order::Middle).fixed_pos(rect.min).show(ui.ctx(), |ui| {
                        ui.allocate_rect(rect, Sense::click_and_drag());
                    });
                }
            });
            out.drop_without_applying_deltas();
            lefts
        }

        /// A finger goes down at `from` and slides to `to` without lifting; how far each
        /// shelf has scrolled by then.
        fn swipe(&mut self, from: Pos2, to: Pos2) -> Vec<f32> {
            let before = self.frame(vec![]);
            self.frame(vec![egui::Event::PointerMoved(from), egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
            self.frame(vec![egui::Event::PointerMoved(to)]);
            // The shelf takes the finger's place on the frame after it moved.
            let after = self.frame(vec![]);
            before.iter().zip(&after).map(|(b, a)| (b - a).round()).collect()
        }
    }

    #[test]
    fn a_sideways_swipe_moves_only_the_shelf_under_the_finger() {
        // The second shelf is on screen from y 240 to 400; the first one is above it.
        let moved = Shelves::new(0.0).swipe(pos2(300.0, 300.0), pos2(220.0, 304.0));
        assert_eq!(moved, [0.0, 80.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_shelf_further_down_the_page_is_swiped_too() {
        // Scrolled 600 down, the fifth shelf (840 to 1000 on the page) is at y 240 to 400.
        let moved = Shelves::new(600.0).swipe(pos2(300.0, 300.0), pos2(220.0, 304.0));
        assert_eq!(moved, [0.0, 0.0, 0.0, 0.0, 80.0, 0.0]);
    }

    #[test]
    fn a_swipe_on_what_covers_the_page_leaves_the_shelf_under_it_alone() {
        let mut page = Shelves::new(0.0);
        page.cover = Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0)));
        let moved = page.swipe(pos2(300.0, 300.0), pos2(220.0, 304.0));
        assert_eq!(moved, [0.0; 6]);
    }

    #[test]
    fn a_swipe_between_two_shelves_moves_neither() {
        // y 220 is in the gap above the second shelf.
        let moved = Shelves::new(0.0).swipe(pos2(300.0, 220.0), pos2(220.0, 224.0));
        assert_eq!(moved, [0.0; 6]);
    }

    /// A phone page with a strip of twelve 100-point chips under a 100-point gap, then
    /// room to scroll: 400 x 600 in a headless egui.
    struct Strip {
        ctx: egui::Context,
        time: f64,
    }

    impl Strip {
        fn new() -> Strip {
            let mut s = Strip { ctx: egui::Context::default(), time: 0.0 };
            s.frame(vec![]);
            s.frame(vec![]);
            s
        }

        /// One frame; where the first chip's left edge and the strip's top were drawn.
        fn frame(&mut self, events: Vec<egui::Event>) -> (f32, f32) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0))), time: Some(self.time), events, ..Default::default() };
            let mut at = (0.0, 0.0);
            let out = self.ctx.run_ui(input, |ui| {
                egui::ScrollArea::vertical().id_salt("page").auto_shrink(false).show(ui, |ui| {
                    ui.add_space(100.0);
                    super::h_strip_on(ui, true, "test-strip", |ui| {
                        at = (ui.cursor().min.x, ui.cursor().min.y);
                        for _ in 0..12 {
                            ui.allocate_exact_size(vec2(100.0, 48.0), Sense::click());
                        }
                    });
                    ui.add_space(900.0);
                });
            });
            out.drop_without_applying_deltas();
            at
        }

        /// A finger goes down at `from`, slides to `to` and lifts; how far the strip
        /// has scrolled sideways once it is up.
        fn swipe(&mut self, from: Pos2, to: Pos2) -> f32 {
            let before = self.frame(vec![]).0;
            self.frame(vec![egui::Event::PointerMoved(from), egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
            self.frame(vec![egui::Event::PointerMoved(to)]);
            self.frame(vec![]);
            self.frame(vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
            self.frame(vec![]);
            (before - self.frame(vec![]).0).round()
        }
    }

    #[test]
    fn a_strip_follows_a_finger_sideways_and_stays_where_it_is_let_go() {
        let mut s = Strip::new();
        assert_eq!(s.swipe(pos2(300.0, 124.0), pos2(180.0, 128.0)), 120.0);
        // And back again, as far as its start.
        assert_eq!(s.swipe(pos2(100.0, 124.0), pos2(390.0, 126.0)), -120.0);
    }

    #[test]
    fn a_strip_leaves_a_finger_going_down_the_page_alone() {
        assert_eq!(Strip::new().swipe(pos2(300.0, 124.0), pos2(304.0, 220.0)), 0.0);
    }

    #[test]
    fn a_swipe_below_a_strip_does_not_move_it() {
        assert_eq!(Strip::new().swipe(pos2(300.0, 300.0), pos2(180.0, 304.0)), 0.0);
    }

    #[test]
    fn arrows_visit_aligned_pages_in_both_directions() {
        let pages = shelf_page_starts(14, 5);
        assert_eq!(pages, [0, 5, 9]);
        let mut target = 0.0;
        for direction in [1, 1, -1] {
            target = shelf_page_target(&pages, 198.0, target, direction);
        }
        assert_eq!(target, 5.0 * 198.0);
        assert_eq!(shelf_page_target(&pages, 198.0, 0.0, -1), 0.0);
    }

    #[test]
    fn a_shelf_swiped_to_its_last_page_has_reached_its_end() {
        // Eleven items, three to a page of 436 points each: the last page starts at the eighth.
        assert!(super::at_last_page(11, 3, 436.0, 8.0 * 436.0));
        assert!(super::at_last_page(11, 3, 436.0, 8.0 * 436.0 - 0.3), "landed a hair short");
        assert!(!super::at_last_page(11, 3, 436.0, 6.0 * 436.0));
        assert!(!super::at_last_page(11, 3, 436.0, 0.0));
    }

    #[test]
    fn a_shelf_that_fits_one_page_has_no_end_to_reach() {
        assert!(!super::at_last_page(3, 3, 436.0, 0.0));
        assert!(!super::at_last_page(2, 3, 436.0, 0.0));
    }

    #[test]
    fn short_final_page_is_filled_without_overscrolling() {
        assert_eq!(shelf_page_starts(4, 3), [0, 1]);
        assert_eq!(shelf_page_starts(3, 3), [0]);
        assert_eq!(shelf_page_starts(1, 3), [0]);
        // The old last page is between new page starts after more picks arrive.
        assert_eq!(shelf_page_target(&shelf_page_starts(11, 3), 436.0, 2.0 * 436.0, 1), 3.0 * 436.0);
    }
}
