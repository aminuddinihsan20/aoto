//! Page bodies. Every page fades and slides in when navigated to.

use crate::lossless::Lossless;
use std::collections::HashSet;
use std::sync::atomic::Ordering;

use egui::{Align2, Color32, Pos2, Rect, RichText, Sense, Ui, Vec2, pos2, vec2};

use super::widgets::{self, RowOpts, card, chip, h_scroll, named, paint_cover, pill, shelf_header, track_row};
use super::settings::Row;
use super::{Action, App, LibTab, Page, Selection, m3};
use crate::backend::compact;
use crate::model::{Card, ShelfKind, Target, Track};
use crate::theme::{self, icon};

const CARD: f32 = 180.0;

/// How many tiles Apple fits across a feed row at this content width. The site
/// measures from its own viewport, which includes the navigation column this page
/// already sits past.
fn feed_columns(width: f32, square: bool) -> usize {
    let viewport = width + 260.0;
    match square {
        true if viewport < 1260.0 => 4,
        true if viewport < 1580.0 => 5,
        true => 6,
        false if viewport < 1260.0 => 2,
        false if viewport < 1580.0 => 3,
        false => 4,
    }
}

/// One tile of an Apple feed row: a share of the content on wide windows, the site's
/// own narrow width below its breakpoint.
fn feed_tile(ui: &Ui, square: bool) -> f32 {
    let width = ui.available_width();
    let min = if square { 144.0 } else { 270.0 };
    if width < 1024.0 {
        return min;
    }
    let cols = feed_columns(width, square);
    ((width - 20.0 * (cols - 1) as f32) / cols as f32).max(min)
}

pub fn page(app: &mut App, ui: &mut Ui, now: f64) {
    let t = theme::ease_out(((now - app.page_changed) / 0.32) as f32);
    if t < 1.0 {
        // The fade-in finishes even when nothing else is drawing frames.
        ui.ctx().request_repaint();
    }
    ui.set_opacity(0.15 + 0.85 * t);
    let page = app.page.clone();
    // Each page (each album, each search) keeps its own scroll position.
    let mut area = egui::ScrollArea::vertical().id_salt(("page", &page)).auto_shrink(false);
    if crate::platform::phone() {
        // A phone's page has no bar to grab: it ran up beside the top row, to the account's picture.
        area = area.scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    }
    if ui.ctx().data(|d| d.get_temp::<bool>(widgets::shelf_swipe_id()).unwrap_or(false)) {
        // A shelf is being swiped sideways: the finger's small up and down must not move the page too.
        area = area.scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() });
    }
    // Dev hook: YTM_SCROLL=<px> pins the page scroll, for screenshots of lower sections.
    if let Some(y) = super::dev_var("YTM_SCROLL").and_then(|v| v.parse::<f32>().ok()) {
        area = area.vertical_scroll_offset(y);
    }
    let out = area.show(ui, |ui| {
        // The panel reaches the window's side so egui's own bar sits flush with it; the
        // content keeps its measured right edge instead of following the panel.
        // (A phone's panel keeps its own side margins, so nothing is held back there.)
        ui.set_max_width((ui.available_width() - if crate::platform::phone() { 0.0 } else { 48.0 }).max(0.0));
        ui.add_space(18.0 * (1.0 - t));
        match &page {
            Page::Home => home(app, ui, now),
            Page::New => new(app, ui, now),
            Page::Search(q) => search(app, ui, q, now),
            Page::Album(id) | Page::Playlist(id) => collection(app, ui, id, now),
            Page::Artist(id) => artist(app, ui, id, now),
            Page::Library(tab) => library(app, ui, *tab, now),
            Page::Downloads => downloads(app, ui, now),
            Page::SettingsCategory(category) if m3::on() => super::settings::category_page(app, ui, *category),
            Page::Settings | Page::SettingsCategory(_) => settings(app, ui),
            Page::Transfer => super::transfer::page(app, ui, now),
            Page::LocalFolder(uri) => local_folder(app, ui, uri, now),
            Page::LocalAlbum(key) => local_album(app, ui, key, now),
            Page::LocalSongs => local_songs(app, ui, now),
            Page::Downloaded(id) => super::downloading::saved_page(app, ui, id, now),
            Page::Equalizer => super::equalizer::page(app, ui),
            Page::Stats => super::stats::page(app, ui),
        }
        // Room to scroll the last rows clear of the player's frosted strip.
        ui.add_space(96.0);
    });
    // Dev hook: YTM_SELECT=<rows> ("0,2") selects those songs of a playlist once it is
    // in, for a screenshot of the selection bar.
    if let Some(rows) = super::dev_var("YTM_SELECT").filter(|_| selectable(app, &page).is_some() && !app.selection.on(&page)) {
        static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
            app.selection.start(&page);
            app.selection.rows = rows.split(',').filter_map(|r| r.trim().parse().ok()).collect();
        }
    }
    selection_bar(app, ui.ctx(), &page, out.inner_rect);
    // Infinite scroll on home: fetch the next shelves when nearing the bottom.
    if page == Page::Home {
        let remaining = out.content_size.y - (out.state.clamped_offset().y + out.inner_rect.height());
        if let Some(h) = &mut app.home {
            // A failed fetch reports nothing (it's a quiet request): allow a retry later.
            if h.loading_more.is_some_and(|t| now - t > 20.0) {
                h.loading_more = None;
            }
            if remaining < 700.0 && h.loading_more.is_none()
                && let Some(token) = h.continuation.clone()
            {
                h.loading_more = Some(now);
                app.backend.request(crate::backend::Req::HomeMore(token));
            }
        }
    }
}

/// What every song row needs from the app, gathered once per frame.
pub(super) struct Rows {
    current: Option<String>,
    playing: bool,
    accent: Color32,
    now: f64,
}

impl Rows {
    pub(super) fn new(app: &App, now: f64) -> Self {
        Self {
            current: app.current_id().map(str::to_owned),
            playing: app.audio.status.playing.load(Ordering::Relaxed),
            accent: app.accent_color(),
            now,
        }
    }

    pub(super) fn opts(&self, liked: &HashSet<String>, t: &Track, number: Option<usize>, show_album: bool) -> RowOpts {
        RowOpts {
            number,
            show_album,
            current: self.current.as_deref() == Some(t.id.as_str()),
            playing: self.playing,
            accent: self.accent,
            time: self.now,
            liked: liked.contains(&t.id),
            added_by: None,
            pick: false,
            selected: None,
            remove: None,
            queue_remove: None,
            queue_move: None,
        }
    }
}

/// A page's data hasn't arrived: shimmering placeholders while it loads, a retry if the
/// request failed.
fn placeholder(app: &mut App, ui: &mut Ui, now: f64, rows: usize, card: f32) {
    if app.load_failed {
        ui.add_space(40.0);
        ui.label(RichText::new("Couldn't load this page").color(theme::text_dim()));
        ui.add_space(12.0);
        if pill(ui, "Try again", None, false).clicked() {
            app.refresh();
        }
    } else {
        widgets::skeleton(ui, now, rows, card);
    }
}

/// What a collection header's round buttons draw: a font glyph, or the three dots.
enum CircleGlyph {
    Char(char),
    More,
}

/// A round glass action on a collection header (shuffle, search, more).
fn circle_action(ui: &mut Ui, glyph: CircleGlyph, name: &str, active: bool) -> egui::Response {
    // Same 38 px height as the Play pill beside it, so the row shares one centre line.
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::click());
    let resp = named(resp, name).on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered() || open, 0.15);
    let press = if resp.is_pointer_button_down_on() { 18.3 } else { 19.0 };
    ui.painter().circle_filled(rect.center(), press, theme::glass(0.10 + 0.09 * h));
    let color = theme::lerp_color(theme::text_dim(), theme::text(), h.max(active as u8 as f32));
    match glyph {
        CircleGlyph::Char(g) => widgets::icon_at(ui, rect.center(), g, 16.0, color),
        CircleGlyph::More => {
            for i in -1..=1 {
                ui.painter().circle_filled(rect.center() + vec2(i as f32 * 5.2, 0.0), 1.8, color);
            }
        }
    }
    resp
}

/// A page's main Play button. Shows Pause while this page's album / playlist / artist
/// is playing, and toggles instead of restarting when it's already loaded.
fn play_pill(ui: &mut Ui, acts: &mut Vec<Action>, target: &Target, tracks: &[Track]) {
    let active = widgets::active_state(ui, target);
    let (label, glyph) = if active == Some(true) { ("Pause", icon::PAUSE) } else { ("Play", icon::PLAY) };
    if pill(ui, label, Some(glyph), true).clicked() {
        if active.is_some() {
            acts.push(Action::TogglePlay);
        } else if !tracks.is_empty() {
            acts.push(Action::Play(tracks.to_vec(), 0));
        }
    }
}

fn card_row(app: &mut App, ui: &mut Ui, id: &str, title: &str, cards: &[Card]) {
    if cards.is_empty() {
        return;
    }
    let (l, r) = shelf_header(ui, title, None, None, true);
    let nudge = r as i32 - l as i32;
    let tile = feed_tile(ui, true);
    h_scroll(ui, id, nudge, cards.len(), tile, 20.0, |ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        for c in cards {
            // Like Apple's shelves, the tile under the pointer animates; one decoder
            // at a time keeps the whole page's motion a few MB.
            let img = Rect::from_min_size(ui.cursor().min, Vec2::splat(tile));
            let sprite = if ui.rect_contains_pointer(img) { card_motion(app, ui, c, tile) } else { None };
            card(ui, c, tile, false, &mut app.actions, sprite.as_ref());
        }
    });
}

/// The motion sleeve of a hovered shelf tile, asked of Apple by the card's own names.
/// Albums only: playlist covers stay still.
fn card_motion(app: &mut App, ui: &Ui, c: &Card, tile: f32) -> Option<widgets::MotionSprite> {
    let Target::Album(_) = &c.target else { return None };
    // The subtitle carries a kind prefix ("EP • …"); the artist link is the plain name.
    let name = c.title.as_str();
    let artist = c.links.first().map(|l| l.name.as_str()).unwrap_or_else(|| c.subtitle.rsplit('•').next().unwrap_or("").trim());
    app.apple_cover(name, artist, None);
    let px = (tile * ui.ctx().pixels_per_point()).round().max(128.0) as u32;
    app.apple_sprite(ui.ctx(), name, artist, None, px)
}

/// A shelf of still tiles, for pages whose results are borrowed while they draw.
fn card_row_plain(ui: &mut Ui, id: &str, title: &str, cards: &[Card], acts: &mut Vec<Action>) {
    if cards.is_empty() {
        return;
    }
    if m3::on() {
        // As Home's shelves are: a carousel, turned by its header's arrow.
        let next = format!("More of {title}");
        let trailing = if cards.len() > 2 { m3::Trailing::Icon(icon::CHEVRON_RIGHT, &next) } else { m3::Trailing::None };
        let nudge = m3::section_header(ui, &m3::SectionHeader { title, overline: None, avatar: None, trailing }) as i32;
        return m3::carousel(ui, id, nudge, cards, &HashSet::new(), acts);
    }
    let (l, r) = shelf_header(ui, title, None, None, true);
    let nudge = r as i32 - l as i32;
    let tile = feed_tile(ui, true);
    h_scroll(ui, id, nudge, cards.len(), tile, 20.0, |ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        for c in cards {
            card(ui, c, tile, false, acts, None);
        }
    });
}

/// How wide a column of the song grid is: a feed tile on a desktop, and on a phone
/// nearly the whole screen, so titles have room and the next column only peeks in.
fn song_column(tile: f32, width: f32, phone: bool) -> f32 {
    if phone { (width - 36.0).max(tile.min(width)) } else { tile }
}

/// Songs laid out in columns of `rows`, scrolling sideways (Quick picks). Clicking a
/// song starts its radio. Returns whether the arrows reached the end.
#[allow(clippy::too_many_arguments)]
fn song_grid(ui: &mut Ui, rows_ctx: &Rows, liked: &HashSet<String>, acts: &mut Vec<Action>, id: &str, title: &str, strap: Option<&str>, tracks: &[Track], rows: usize) -> bool {
    let (l, r) = shelf_header(ui, title, strap, None, true);
    let nudge = r as i32 - l as i32;
    let col_w = song_column(feed_tile(ui, false), ui.available_width(), crate::platform::phone());
    let mut picked = None;
    let (_, reached_end) = h_scroll(ui, id, nudge, tracks.len().div_ceil(rows.max(1)), col_w, 20.0, |ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        for (c, col) in tracks.chunks(rows.max(1)).enumerate() {
            ui.allocate_ui(vec2(col_w, rows as f32 * 60.0), |ui| {
                ui.set_width(col_w);
                ui.vertical(|ui| {
                    for (i, t) in col.iter().enumerate() {
                        let o = rows_ctx.opts(liked, t, None, false);
                        if track_row(ui, t, &o, acts).clicked() {
                            picked = Some(c * rows + i);
                        }
                    }
                });
            });
        }
    });
    if let Some(i) = picked {
        acts.push(Action::Radio(tracks[i].clone()));
    }
    reached_end
}

/// Where a song list lives: the page its selection belongs to, and whether it's your own
/// playlist (so songs can be taken out of it).
struct ListCtx<'a> {
    sel: &'a mut Selection,
    page: &'a Page,
    own: bool,
}

/// A long song list; only the rows on screen are built. Clicking one plays `tracks` from it,
/// or picks it while selecting: Ctrl-click picks one song, Shift-click a run of them.
#[allow(clippy::too_many_arguments)]
fn list(ui: &mut Ui, rows: &Rows, liked: &HashSet<String>, tracks: &[Track], show_album: bool, number: impl Fn(usize, &Track) -> Option<usize>, cx: ListCtx, acts: &mut Vec<Action>) {
    let ListCtx { sel, page, own } = cx;
    let selecting = sel.on(page);
    let mods = ui.input(|i| i.modifiers);
    widgets::visible_rows_of(ui, tracks.len(), widgets::track_row_height(), |ui, i| {
        let t = &tracks[i];
        let o = RowOpts {
            selected: selecting.then(|| sel.rows.contains(&i)),
            remove: own.then_some(i),
            ..rows.opts(liked, t, number(i, t), show_album)
        };
        if track_row(ui, t, &o, acts).clicked() {
            if selecting || mods.command || mods.shift {
                sel.pick(page, i, mods.shift);
            } else {
                acts.push(Action::Play(tracks.to_vec(), i));
            }
        }
    });
}

/// A list page's "Select" button: turns select mode on and off.
fn select_pill(ui: &mut Ui, sel: &mut Selection, page: &Page) {
    let on = sel.on(page);
    if pill(ui, if on { "Done" } else { "Select" }, Some(icon::LIST_CHECKS), false).clicked() {
        if on { sel.clear() } else { sel.start(page) }
    }
}

/// The songs a page lists, and whether they're your own playlist's.
fn selectable<'a>(app: &'a App, page: &Page) -> Option<(&'a [Track], bool)> {
    match page {
        Page::Album(id) | Page::Playlist(id) => {
            app.opened(id).map(|o| &o.c).map(|c| (c.tracks.as_slice(), app.logged_in && c.owned && !c.is_album))
        }
        Page::Library(LibTab::Songs) => app.opened(super::LIKED_MUSIC).map(|o| (o.c.tracks.as_slice(), false)),
        Page::Library(LibTab::History) => app.history.as_deref().map(|h| (h, false)),
        Page::Library(LibTab::Uploads) => app.uploads.as_deref().map(|u| (u, false)),
        _ => None,
    }
}

/// How many rows the Material selection bar needs in `width` for `actions` action
/// buttons beside its count and its two own (select all, done): the actions go on a
/// second row when a finger-sized button each and the count do not fit one.
fn selection_rows(width: f32, actions: usize) -> usize {
    const COUNT: f32 = 20.0 + 96.0;
    if COUNT + (actions + 2) as f32 * m3::TOUCH + 8.0 <= width { 1 } else { 2 }
}

/// While selecting: a bar floating over the bottom of the page with what to do with the picks.
fn selection_bar(app: &mut App, ctx: &egui::Context, page: &Page, area: Rect) {
    if !app.selection.on(page) {
        return;
    }
    let Some((tracks, own)) = selectable(app, page) else { return };
    let n = tracks.len();
    let picked: Vec<Track> = app.selection.rows.iter().filter_map(|&i| tracks.get(i).cloned()).collect();
    let all = !picked.is_empty() && picked.len() == n;
    let all_liked = !picked.is_empty() && picked.iter().all(|t| app.liked.contains(&t.id));
    // Labelled buttons when there's room, icons with tooltips when the window is narrow.
    let wide = area.width() > 820.0;
    let button = |ui: &mut Ui, label: &str, glyph: char, primary: bool| {
        if wide { pill(ui, label, Some(glyph), primary) } else { widgets::icon_button(ui, glyph, 18.0, label, false) }
    };
    let mut acts = Vec::new();
    let mut toggle_all = false;
    let mut done = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if m3::on() {
        // Material: a floating toolbar of icon buttons a finger can hit, the count before
        // them; on a narrow phone the actions take a row of their own under it.
        let m = crate::material::scheme();
        let count = if picked.is_empty() { "Select songs".to_owned() } else { format!("{} selected", picked.len()) };
        let mut actions: Vec<(m3::Icon, &str, u8)> = Vec::new();
        if !picked.is_empty() {
            actions.push((icon::LIST_PLUS.into(), "Add to playlist", 0));
            if own {
                actions.push((icon::LIST_MINUS.into(), "Remove from playlist", 1));
            }
            actions.push((icon::HEART.into(), if all_liked { "Remove like" } else { "Like" }, 2));
            actions.push((icon::LIST_MUSIC.into(), "Add to queue", 3));
        }
        let ends: [(m3::Icon, &str, u8); 2] = [(icon::LIST_CHECKS.into(), if all { "Select none" } else { "Select all" }, 4), (icon::X.into(), "Done selecting", 5)];
        let width = (area.width() - 16.0).min(480.0);
        let rows = selection_rows(width, actions.len());
        let height = 8.0 + rows as f32 * m3::TOUCH + 8.0;
        // Above the mini player, which floats over the page's foot while a song is loaded.
        let lift = if app.current().is_some() { super::phone::MINI_LIFT } else { 0.0 };
        let bar = Rect::from_min_size(pos2(area.center().x - width / 2.0, area.bottom() - 12.0 - lift - height), vec2(width, height));
        egui::Area::new(egui::Id::unique("selection-bar")).order(egui::Order::Foreground).fixed_pos(bar.min).show(ctx, |ui| {
            ui.allocate_exact_size(bar.size(), Sense::click());
            if crate::material::style().outlined() {
                m3::plate(ui, bar, m3::round(crate::material::shape::CARD), m.surface_container_high, m3::Stack::Ink);
            } else {
                widgets::soft_shadow(ui, bar, crate::material::shape::CARD, 16.0, 0.35);
                ui.painter().rect_filled(bar, crate::material::shape::CARD, m.surface_container_high);
            }
            let cy = bar.top() + 8.0 + m3::TOUCH / 2.0;
            let mut tapped = None;
            // The first row from its right end: done, select all, and (when they fit) the actions.
            let first: Vec<_> = if rows == 1 { actions.iter().chain(&ends).collect() } else { ends.iter().collect() };
            let mut x = bar.right() - 8.0 - m3::TOUCH / 2.0;
            for (glyph, name, what) in first.into_iter().rev() {
                let style = if *what == 0 { m3::IconStyle::Tonal } else { m3::IconStyle::Standard };
                if m3::IconButton::new(*glyph, name).style(style).show_at(ui, pos2(x, cy), ui.scope_id().with(("selection", *what))).clicked() {
                    tapped = Some(*what);
                }
                x -= m3::TOUCH;
            }
            let g = m3::galley(ui, &count, crate::material::Type::TitleSmall, m.on_surface, (x + m3::TOUCH / 2.0 - bar.left() - 20.0).max(0.0));
            ui.painter().galley(pos2(bar.left() + 20.0, cy - g.size().y / 2.0), g, m.on_surface);
            if rows == 2 {
                // The actions, spread across the second row.
                let cell = (bar.width() - 16.0) / actions.len().max(1) as f32;
                for (i, (glyph, name, what)) in actions.iter().enumerate() {
                    let at = pos2(bar.left() + 8.0 + cell * (i as f32 + 0.5), cy + m3::TOUCH);
                    let style = if *what == 0 { m3::IconStyle::Tonal } else { m3::IconStyle::Standard };
                    if m3::IconButton::new(*glyph, name).style(style).show_at(ui, at, ui.scope_id().with(("selection", *what))).clicked() {
                        tapped = Some(*what);
                    }
                }
            }
            match tapped {
                Some(0) => acts.push(Action::AddToPlaylist(picked.clone())),
                Some(1) => acts.push(Action::RemoveFromPlaylist(app.selection.rows.iter().copied().collect())),
                Some(2) => acts.push(Action::LikeMany(picked.clone(), !all_liked)),
                Some(3) => acts.extend(picked.iter().cloned().map(Action::Enqueue)),
                Some(4) => toggle_all = true,
                Some(5) => done = true,
                _ => {}
            }
        });
    } else {
    egui::Area::new(egui::Id::unique("selection-bar"))
        .order(egui::Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(area.center().x, area.bottom() - 18.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::menu_fill())
                .corner_radius(14)
                .inner_margin(egui::Margin { left: 20, right: 10, top: 8, bottom: 8 })
                .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: theme::shade(0.45) })
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let count = if picked.is_empty() { "Select songs".to_owned() } else { format!("{} selected", picked.len()) };
                        ui.label(RichText::new(count).font(theme::semibold(14.0)).color(theme::text()));
                        ui.add_space(10.0);
                        if !picked.is_empty() {
                            if button(ui, "Add to playlist", icon::LIST_PLUS, true).clicked() {
                                acts.push(Action::AddToPlaylist(picked.clone()));
                            }
                            if own && button(ui, "Remove from playlist", icon::LIST_MINUS, false).clicked() {
                                acts.push(Action::RemoveFromPlaylist(app.selection.rows.iter().copied().collect()));
                            }
                            if button(ui, if all_liked { "Remove like" } else { "Like" }, icon::HEART, false).clicked() {
                                acts.push(Action::LikeMany(picked.clone(), !all_liked));
                            }
                            if button(ui, "Add to queue", icon::LIST_MUSIC, false).clicked() {
                                acts.extend(picked.iter().cloned().map(Action::Enqueue));
                            }
                        }
                        toggle_all = button(ui, if all { "Select none" } else { "Select all" }, icon::LIST_CHECKS, false).clicked();
                        done |= widgets::icon_button(ui, icon::X, 18.0, "Done selecting", false).clicked();
                    });
                });
        });
    }
    if toggle_all {
        let page = page.clone();
        app.selection.start(&page);
        if !all {
            app.selection.rows = (0..n).collect();
        }
    }
    if done || !acts.is_empty() {
        app.selection.clear();
    }
    app.actions.extend(acts);
}

// ------------------------------------------------------------------ home

/// Phone: a newer version on the releases page, offered above the feed. A tap hands its
/// APK to the browser, which downloads it; opening the download installs it over this one.
fn update_card(app: &mut App, ui: &mut Ui) {
    let offer = match super::dev_var("YTM_FAKE_UPDATE") {
        Some(v) => Some((v.to_owned(), crate::update::RELEASES.to_owned())),
        None => app.updater.as_ref().and_then(|u| match u.state() {
            crate::update::State::Available { version, url } => Some((version, url)),
            _ => None,
        }),
    };
    let Some((version, url)) = offer.filter(|(v, _)| app.update_dismissed.as_deref() != Some(v.as_str())) else { return };
    ui.add_space(12.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::click());
    let resp = named(resp, "Download the update");
    ui.painter().rect_filled(rect, 16.0, theme::glass(0.10));
    let accent = ui.ctx().data(|d| d.get_temp::<Color32>(egui::Id::unique("accent"))).unwrap_or(theme::RED);
    widgets::icon_at(ui, pos2(rect.left() + 28.0, rect.center().y), icon::DOWNLOAD, 20.0, accent);
    let x = rect.left() + 54.0;
    let text_w = rect.right() - 48.0 - x;
    widgets::text_at(ui, pos2(x, rect.center().y - 19.0), &format!("Aoto {version} is available"), theme::semibold(15.0), theme::text(), text_w);
    widgets::text_at(ui, pos2(x, rect.center().y + 3.0), "Tap to download it, then open it to install.", theme::regular(12.5), theme::text_dim(), text_w);
    // Its own, thumb-sized target over the card's right end.
    let close = Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), Vec2::splat(44.0));
    let closed = named(ui.interact(close, resp.id.with("dismiss"), Sense::click()), "Not now");
    widgets::icon_at(ui, close.center(), icon::X, 14.0, theme::text_dim());
    if closed.clicked() {
        app.update_dismissed = Some(version);
    } else if resp.clicked() {
        #[cfg(target_os = "android")]
        crate::platform::open_url(&url);
        #[cfg(not(target_os = "android"))]
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
    ui.add_space(12.0);
}

/// The card the Home feed leads with, large: the album or playlist that opens the
/// first shelf of cards to open with one. `(shelf, card)`, or none when no shelf does
/// (a feed of songs and artists only), and then Home has no hero.
fn hero_pick(shelves: &[crate::model::Shelf]) -> Option<(usize, usize)> {
    shelves.iter().position(|s| matches!(&s.kind, ShelfKind::Cards(cards) if matches!(cards.first().map(|c| &c.target), Some(Target::Album(_) | Target::Playlist(_))))).map(|shelf| (shelf, 0))
}

/// The feed's moods as filter chips, led by "All" (no mood): a tap asks for that feed.
fn home_chips(app: &mut App, ui: &mut Ui) {
    let Some(h) = app.home.as_ref().filter(|h| !h.chips.is_empty()) else { return };
    let all = !h.chips.iter().any(|c| c.selected);
    let mut pick = None;
    m3::chip_row(ui, "home-chips", super::phone::SIDE, |ui| {
        if m3::filter_chip(ui, "All", all).clicked() && !all {
            pick = Some(None);
        }
        for c in &h.chips {
            if m3::filter_chip(ui, &c.title, c.selected).clicked() {
                pick = Some(if c.selected { None } else { c.params.clone() });
            }
        }
    });
    if let Some(params) = pick {
        app.actions.push(Action::HomeChip(params));
    }
}

/// A shelf of songs as tiles, two to a row and two rows to a page, turning sideways
/// (Quick picks). A tap starts the song's radio; "Play all" plays the shelf in order.
/// Returns whether the shelf is at its end.
fn song_tiles(ui: &mut Ui, rows_ctx: &Rows, liked: &HashSet<String>, acts: &mut Vec<Action>, id: &str, title: &str, strap: Option<&str>, tracks: &[Track]) -> bool {
    const ROWS: usize = 2;
    if m3::section_header(ui, &m3::SectionHeader { title, overline: strap, avatar: None, trailing: m3::Trailing::Text("Play all") }) {
        acts.push(Action::Play(tracks.to_vec(), 0));
    }
    let tile = m3::tile_width(ui.available_width());
    let mut picked = None;
    let (_, reached_end) = h_scroll(ui, id, 0, tracks.len().div_ceil(ROWS), tile, m3::TILE_GAP, |ui| {
        ui.spacing_mut().item_spacing = vec2(m3::TILE_GAP, m3::TILE_GAP);
        for (c, col) in tracks.chunks(ROWS).enumerate() {
            ui.vertical(|ui| {
                for (i, t) in col.iter().enumerate() {
                    let o = rows_ctx.opts(liked, t, None, false);
                    if m3::song_tile(ui, tile, t, &o, acts).clicked() {
                        picked = Some(c * ROWS + i);
                    }
                }
            });
        }
    });
    if let Some(i) = picked {
        acts.push(Action::Radio(tracks[i].clone()));
    }
    reached_end
}

/// Home on a phone, in Material 3: the moods as chips, a hero card, the songs as
/// tiles and the shelves as carousels. (The greeting is the frame's top bar.)
fn home_material(app: &mut App, ui: &mut Ui, now: f64) {
    update_card(app, ui);
    home_chips(app, ui);
    // A refresh keeps showing the current feed; the skeleton is only for an empty one.
    if app.home.as_ref().is_none_or(|h| app.home_loading && h.shelves.is_empty()) {
        placeholder(app, ui, now, 3, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let Some(h) = &app.home else { return };
    if h.shelves.is_empty() {
        ui.add_space(40.0);
        ui.label(RichText::new("Nothing here yet").color(theme::text_dim()));
    }
    let hero = hero_pick(&h.shelves);
    if let Some((shelf, card)) = hero
        && let ShelfKind::Cards(cards) = &h.shelves[shelf].kind
    {
        ui.add_space(2.0);
        m3::hero_card(ui, &h.shelves[shelf].title, &cards[card], &mut app.actions);
    }
    let mut more_picks = None;
    for (i, s) in h.shelves.iter().enumerate() {
        let id = format!("home-{i}-{}", s.title);
        match &s.kind {
            ShelfKind::Cards(cards) => {
                // The hero card is not shown a second time in its own shelf.
                let cards = if hero == Some((i, 0)) { &cards[1..] } else { &cards[..] };
                if cards.is_empty() {
                    continue;
                }
                let (overline, avatar) = if s.title == "Listen again" { (None, None) } else { (s.strapline.as_deref(), s.strap_thumb.as_deref()) };
                let next = format!("More of {}", s.title);
                let trailing = if cards.len() > 2 { m3::Trailing::Icon(icon::CHEVRON_RIGHT, &next) } else { m3::Trailing::None };
                let nudge = m3::section_header(ui, &m3::SectionHeader { title: &s.title, overline, avatar, trailing }) as i32;
                m3::carousel(ui, &id, nudge, cards, &app.liked, &mut app.actions);
            }
            ShelfKind::Songs { tracks, .. } => {
                let reached_end = song_tiles(ui, &rows, &app.liked, &mut app.actions, &id, &s.title, s.strapline.as_deref(), tracks);
                if s.title == "Quick picks" && reached_end && !h.picks_loading && !h.picks_expanded {
                    more_picks = tracks.last().map(|t| t.id.clone());
                }
            }
        }
    }
    let loading_more = h.loading_more.is_some();
    if let Some(seed) = more_picks
        && let Some(h) = &mut app.home
    {
        h.picks_loading = true;
        app.backend.request(crate::backend::Req::MoreQuickPicks(seed));
    }
    if loading_more {
        widgets::skeleton(ui, now, 1, CARD);
    }
}

fn home(app: &mut App, ui: &mut Ui, now: f64) {
    if crate::platform::phone() && crate::material::on() {
        return home_material(app, ui, now);
    }
    page_title(ui, "Home");
    if crate::platform::phone() {
        update_card(app, ui);
    }
    // The phone's navigation has no "New" tab: its page opens from here.
    if crate::platform::phone() && pill(ui, "New releases", Some(icon::CHEVRON_RIGHT), false).clicked() {
        app.actions.push(Action::Go(Page::New));
    }
    // A refresh keeps showing the current feed; the skeleton is only for an empty one.
    if app.home.as_ref().is_none_or(|h| app.home_loading && h.shelves.is_empty()) {
        placeholder(app, ui, now, 3, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let Some(h) = &app.home else { return };
    if h.shelves.is_empty() {
        ui.add_space(40.0);
        ui.label(RichText::new("Nothing here yet").color(theme::text_dim()));
    }
    let mut more_picks = None;
    for (i, s) in h.shelves.iter().enumerate() {
        let id = format!("home-{i}-{}", s.title);
        match &s.kind {
            ShelfKind::Cards(cards) => {
                let (strap, thumb) = if s.title == "Listen again" { (None, None) } else { (s.strapline.as_deref(), s.strap_thumb.as_deref()) };
                let (l, r) = shelf_header(ui, &s.title, strap, thumb, true);
                let nudge = r as i32 - l as i32;
                let tile = feed_tile(ui, true);
                let acts = &mut app.actions;
                let liked = &app.liked;
                h_scroll(ui, &id, nudge, cards.len(), tile, 20.0, |ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    for c in cards {
                        let is_liked = matches!(&c.target, Target::Song(t) if liked.contains(&t.id));
                        card(ui, c, tile, is_liked, acts, None);
                    }
                });
            }
            ShelfKind::Songs { tracks, rows: n } => {
                let reached_end = song_grid(ui, &rows, &app.liked, &mut app.actions, &id, &s.title, s.strapline.as_deref(), tracks, *n);
                if s.title == "Quick picks" && reached_end && !h.picks_loading && !h.picks_expanded {
                    more_picks = tracks.last().map(|t| t.id.clone());
                }
            }
        }
    }
    let loading_more = h.loading_more.is_some();
    if let Some(seed) = more_picks
        && let Some(h) = &mut app.home
    {
        h.picks_loading = true;
        app.backend.request(crate::backend::Req::MoreQuickPicks(seed));
    }
    if loading_more {
        widgets::skeleton(ui, now, 1, CARD);
    }
}

// ------------------------------------------------------------------ new

fn new(app: &mut App, ui: &mut Ui, now: f64) {
    page_title(ui, "New");
    if app.explore.is_none() {
        placeholder(app, ui, now, 2, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    // The card rows need the whole app (motion lookups), so take the shelves out first.
    let (albums, charts) = {
        let Some(e) = &app.explore else { return };
        (e.new_albums.clone(), e.charts.clone())
    };
    let heading = if app.logged_in { "New releases for you" } else { "Popular new releases" };
    card_row(app, ui, "ex-new", heading, &albums);
    if let Some(e) = &app.explore
        && !e.top.is_empty()
    {
        song_grid(ui, &rows, &app.liked, &mut app.actions, "ex-top", "Top songs", None, &e.top, 4);
    }
    card_row(app, ui, "ex-charts", "Charts", &charts);
}

/// The title of a tab's own page. On a phone drawn in Material You it stands in the
/// frame's top bar, beside the account, and is not said again here.
fn root_title(ui: &mut Ui, title: &str) {
    if !(crate::platform::phone() && crate::material::on()) {
        page_title(ui, title);
    }
}

pub(super) fn page_title(ui: &mut Ui, title: &str) {
    if crate::material::on() {
        let m = crate::material::scheme();
        ui.add_space(4.0);
        let g = m3::galley(ui, title, crate::material::Type::HeadlineMedium, m.on_surface, ui.available_width());
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
        ui.painter().galley(rect.min, g, m.on_surface);
        return;
    }
    // Left-aligned with the page's content, a little below the top edge.
    ui.add_space(18.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, title, theme::bold(34.0), theme::text());
}

// ------------------------------------------------------------------ search

/// Coming from a suggestion, the sidebar or a link, the search field says what's shown.
/// Only when the route changed: clearing the box leaves the route alone until Enter.
pub(super) fn sync_search(app: &mut App, q: &str) {
    if app.search_synced != q {
        app.search_synced = q.to_owned();
        app.search_text = q.trim().to_owned();
    }
}

/// The kinds of thing a search finds: the filter chips over its results.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchKind {
    /// Everything, in sections: the page as it opens.
    #[default]
    All,
    Songs,
    Artists,
    Albums,
    Playlists,
}

impl SearchKind {
    fn label(self) -> &'static str {
        match self {
            SearchKind::All => "All",
            SearchKind::Songs => "Songs",
            SearchKind::Artists => "Artists",
            SearchKind::Albums => "Albums",
            SearchKind::Playlists => "Playlists",
        }
    }
}

/// The chips a search's results get: "All", then each kind that found something.
fn search_kinds(r: &crate::backend::SearchResults) -> Vec<SearchKind> {
    let mut kinds = vec![SearchKind::All];
    for (kind, found) in [(SearchKind::Songs, !r.tracks.is_empty()), (SearchKind::Artists, !r.artists.is_empty()), (SearchKind::Albums, !r.albums.is_empty()), (SearchKind::Playlists, !r.playlists.is_empty())] {
        if found {
            kinds.push(kind);
        }
    }
    kinds
}

/// The chip that is on for the search `query`: the one chosen for this search, as long
/// as it still has results; otherwise "All".
fn search_kind_shown(chosen: &(String, SearchKind), query: &str, kinds: &[SearchKind]) -> SearchKind {
    if chosen.0 == query && kinds.contains(&chosen.1) { chosen.1 } else { SearchKind::All }
}

/// How many artists, albums or playlists "All" lists before "See all" takes over.
const SEARCH_PREVIEW: usize = 3;

/// An album, playlist or artist as a list row: a tap opens it, its button (or a long
/// press) brings its menu. `mark`: a small mark before its supporting line.
fn card_list_row(ui: &mut Ui, c: &Card, mark: Option<m3::Icon>, tile: Option<m3::Icon>, acts: &mut Vec<Action>) {
    let out = m3::list_row(ui, &m3::ListRow { title: &c.title, subtitle: &c.subtitle, thumbs: &c.thumbs, round: c.round, more: Some("Options"), mark, tile, ..Default::default() });
    if let Some(more) = &out.more {
        egui::Popup::menu(more).show(|ui| widgets::card_menu_contents(ui, c, false, acts));
    }
    egui::Popup::context_menu(&out.row).show(|ui| widgets::card_menu_contents(ui, c, false, acts));
    if out.row.clicked() {
        acts.push(Action::Open(c.target.clone()));
    }
}

/// Where the top result's parts are in its card: the cover, the text beside it, and
/// the play button in its bottom right corner.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopResultGeom {
    cover: Rect,
    text: Rect,
    play: Rect,
}

const TOP_RESULT_H: f32 = 128.0;

fn top_result_geom(rect: Rect) -> TopResultGeom {
    let cover = Rect::from_min_size(rect.min + vec2(16.0, 16.0), Vec2::splat(rect.height() - 32.0));
    let play = Rect::from_min_size(pos2(rect.right() - 16.0 - m3::TOUCH, rect.bottom() - 16.0 - m3::TOUCH), Vec2::splat(m3::TOUCH));
    let left = cover.right() + 16.0;
    TopResultGeom { cover, text: Rect::from_min_max(pos2(left, cover.top()), pos2((play.left() - 8.0).max(left), cover.bottom())), play }
}

/// The top result as a card: its cover, what it is, and a button that plays it. A tap
/// elsewhere opens it (or, choosing a transfer match, picks it).
fn top_result_material(ui: &mut Ui, c: &Card, pick: bool, liked: bool, acts: &mut Vec<Action>) {
    use crate::material::{Type, shape};
    let m = crate::material::scheme();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), TOP_RESULT_H), Sense::click());
    let resp = named(resp, &c.title);
    let g = top_result_geom(rect);
    super::m3::plate(ui, rect, super::m3::round(shape::CARD), m.surface_container_high, super::m3::Stack::Card);
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, shape::CARD, m.on_surface.gamma_multiply(0.08));
    }
    paint_cover(ui, g.cover, &c.thumbs, if c.round { g.cover.width() / 2.0 } else { shape::TILE }, 1.0);
    let over = m3::text(ui, g.text.min, "TOP RESULT", Type::LabelMedium, m.on_surface_variant, g.text.width());
    let title = m3::galley_wrapped(ui, &c.title, Type::TitleLarge, m.on_surface, g.text.width(), 2);
    let title_h = title.size().y;
    ui.painter().galley(pos2(g.text.left(), over.bottom() + 2.0), title, m.on_surface);
    // Beside the button when the title took two lines; under the title otherwise.
    let sub_w = rect.right() - 16.0 - g.text.left() - if title_h > 30.0 { m3::TOUCH + 8.0 } else { 0.0 };
    widgets::linked_text(ui, pos2(g.text.left(), over.bottom() + 4.0 + title_h), &c.subtitle, &c.links, Type::BodyMedium.font(), m.on_surface_variant, sub_w, acts);
    let active = widgets::active_state(ui, &c.target);
    let playing = active == Some(true);
    let name = format!("{} {}", if playing { "Pause" } else { "Play" }, c.title);
    let glyph = if playing { super::glyphs::PAUSE } else { super::glyphs::PLAY };
    let play = m3::IconButton::new(glyph, &name).style(m3::IconStyle::Filled).size(m3::TOUCH).radius(shape::TILE).show_at(ui, g.play.center(), resp.id.with("play"));
    if play.clicked() {
        acts.push(match &c.target {
            _ if active.is_some() => Action::TogglePlay,
            Target::Song(t) => Action::Radio(t.clone()),
            t => Action::PlayCollection(t.clone(), false),
        });
    } else if resp.clicked() {
        acts.push(match &c.target {
            Target::Song(t) if pick => Action::TransferPick(t.clone()),
            t => Action::Open(t.clone()),
        });
    }
    egui::Popup::context_menu(&resp).show(|ui| widgets::card_menu_contents(ui, c, liked, acts));
}

/// What was opened from the last few searches, as chips: a tap opens it again, a long
/// press offers to forget it.
fn recent_chips(ui: &mut Ui, app: &mut App) {
    use crate::material::Type;
    let m = crate::material::scheme();
    if app.recent_searches.is_empty() {
        ui.add_space(12.0);
        ui.label(RichText::new("Search for artists, songs, albums and playlists.").font(Type::BodyLarge.font()).color(m.on_surface_variant));
        return;
    }
    ui.add_space(8.0);
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    m3::text(ui, pos2(head.left(), head.top() + 4.0), "Recent searches", Type::TitleMedium, m.on_surface, head.width());
    // The chips are cloned so a removal can be applied after they are drawn.
    let recents = app.recent_searches.clone();
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(m3::CHIP_GAP, 0.0);
        for (i, r) in recents.iter().enumerate() {
            let resp = m3::assist_chip(ui, &r.title, Some(icon::HISTORY));
            egui::Popup::context_menu(&resp).show(|ui| {
                ui.set_min_width(200.0);
                if ui.button("Remove from Recently Searched").clicked() {
                    remove = Some(i);
                    ui.close();
                }
            });
            if resp.clicked() {
                app.actions.push(match &r.target {
                    Some(t) => Action::Open(t.clone()),
                    // An entry from an earlier version kept only the search.
                    None => Action::Go(Page::Search(r.query.clone())),
                });
            }
        }
    });
    if let Some(i) = remove {
        app.recent_searches.remove(i);
    }
}

/// A section of the results under "All": its heading, with "See all" when the section
/// holds more than is listed. True when that was tapped.
fn results_header(ui: &mut Ui, title: &str, more: bool) -> bool {
    let trailing = if more { m3::Trailing::Text("See all") } else { m3::Trailing::None };
    m3::section_header(ui, &m3::SectionHeader { title, overline: None, avatar: None, trailing })
}

/// Search on the Material phone. The search bar is the frame's top bar; under it, the
/// recent searches, or the results: filter chips, then list rows.
fn search_material(app: &mut App, ui: &mut Ui, q: &str, now: f64) {
    use crate::material::Type;
    let pick = super::transfer::pick_banner(app, ui);
    if q.trim().is_empty() {
        recent_chips(ui, app);
        return;
    }
    if app.search.as_ref().is_none_or(|r| r.query != q) {
        ui.add_space(8.0);
        placeholder(app, ui, now, 2, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let Some(r) = &app.search else { return };
    let acts = &mut app.actions;
    let liked = &app.liked;
    let kinds = search_kinds(r);
    if kinds.len() == 1 {
        ui.add_space(16.0);
        ui.label(RichText::new("No results").font(Type::TitleLarge.font()).color(theme::text()));
        return;
    }
    let shown = search_kind_shown(&app.search_kind, q, &kinds);
    let mut choose = None;
    m3::chip_row(ui, "search-kinds", super::phone::SIDE, |ui| {
        for kind in &kinds {
            if m3::filter_chip(ui, kind.label(), *kind == shown).clicked() {
                choose = Some(*kind);
            }
        }
    });
    let song = |ui: &mut Ui, acts: &mut Vec<Action>, i: usize, t: &Track| {
        // The album beside the artist, as every other song row shows it.
        let o = RowOpts { pick, ..rows.opts(liked, t, None, true) };
        if track_row(ui, t, &o, acts).clicked() {
            acts.push(if pick { Action::TransferPick(t.clone()) } else { Action::Play(r.tracks.clone(), i) });
        }
    };
    let cards = |ui: &mut Ui, acts: &mut Vec<Action>, cards: &[Card]| {
        for c in cards {
            card_list_row(ui, c, None, None, acts);
        }
    };
    match shown {
        SearchKind::All => {
            ui.add_space(4.0);
            // Top result: the first artist when the query names one, otherwise the first song.
            if let Some(top) = r.artists.first().filter(|a| a.title.eq_ignore_ascii_case(q.trim())) {
                top_result_material(ui, top, false, false, acts);
            } else if let Some(t) = r.tracks.first() {
                let c = Card {
                    title: t.title.clone(),
                    subtitle: format!("{} • {}", if t.music_video { "Video" } else { "Song" }, t.artist_line()),
                    thumbs: t.thumbs.clone(),
                    target: Target::Song(t.clone()),
                    round: false,
                    links: t.artists.clone(),
                };
                top_result_material(ui, &c, pick, liked.contains(&t.id), acts);
            }
            if !r.tracks.is_empty() {
                results_header(ui, "Songs", false);
                for (i, t) in r.tracks.iter().take(4).enumerate() {
                    song(ui, acts, i, t);
                }
            }
            for (kind, title, list) in [(SearchKind::Artists, "Artists", &r.artists), (SearchKind::Albums, "Albums", &r.albums)] {
                if !list.is_empty() {
                    if results_header(ui, title, list.len() > SEARCH_PREVIEW) {
                        choose = Some(kind);
                    }
                    cards(ui, acts, &list[..list.len().min(SEARCH_PREVIEW)]);
                }
            }
            if r.tracks.len() > 4 {
                results_header(ui, "More songs", false);
                for (i, t) in r.tracks.iter().enumerate().skip(4) {
                    song(ui, acts, i, t);
                }
            }
            if !r.playlists.is_empty() {
                if results_header(ui, "Playlists", r.playlists.len() > SEARCH_PREVIEW) {
                    choose = Some(SearchKind::Playlists);
                }
                cards(ui, acts, &r.playlists[..r.playlists.len().min(SEARCH_PREVIEW)]);
            }
        }
        SearchKind::Songs => {
            for (i, t) in r.tracks.iter().enumerate() {
                song(ui, acts, i, t);
            }
        }
        SearchKind::Artists => cards(ui, acts, &r.artists),
        SearchKind::Albums => cards(ui, acts, &r.albums),
        SearchKind::Playlists => cards(ui, acts, &r.playlists),
    }
    if let Some(kind) = choose {
        app.search_kind = (q.to_owned(), kind);
    }
}

fn search(app: &mut App, ui: &mut Ui, q: &str, now: f64) {
    if m3::on() {
        return search_material(app, ui, q, now);
    }
    let pick = super::transfer::pick_banner(app, ui);
    root_title(ui, "Search");
    ui.add_space(10.0);
    sync_search(app, q);
    super::shell::search_field(app, ui, ui.available_width().min(560.0), 0.0);
    if q.trim().is_empty() {
        ui.add_space(26.0);
        recent_searches(ui, app);
        return;
    }
    if app.search.as_ref().is_none_or(|r| r.query != q) {
        ui.add_space(14.0);
        placeholder(app, ui, now, 2, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let Some(r) = &app.search else { return };
    let acts = &mut app.actions;
    let liked = &app.liked;
    if r.tracks.is_empty() && r.artists.is_empty() && r.albums.is_empty() && r.playlists.is_empty() {
        ui.add_space(18.0);
        ui.label(RichText::new("No results").font(theme::bold(20.0)).color(theme::text()));
        return;
    }
    ui.add_space(14.0);
    // Top result: the first artist when the query names one, otherwise the first song.
    // Beside the song list on a wide screen; above it on a phone.
    let top_and_songs = |ui: &mut Ui| {
        if let Some(top) = r.artists.first().filter(|a| a.title.eq_ignore_ascii_case(q.trim())) {
            top_result(ui, top, false, false, acts);
        } else if let Some(t) = r.tracks.first() {
            let c = Card {
                title: t.title.clone(),
                subtitle: format!("{} • {}", if t.music_video { "Video" } else { "Song" }, t.artist_line()),
                thumbs: t.thumbs.clone(),
                target: Target::Song(t.clone()),
                round: false,
                links: t.artists.clone(),
            };
            let liked_it = liked.contains(&t.id);
            top_result(ui, &c, pick, liked_it, acts);
        }
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Songs").font(theme::bold(24.0)));
            ui.add_space(8.0);
            for (i, t) in r.tracks.iter().take(4).enumerate() {
                // The album beside the artist, as every other song row shows it.
                let o = RowOpts { pick, ..rows.opts(liked, t, None, true) };
                if track_row(ui, t, &o, acts).clicked() {
                    acts.push(if pick { Action::TransferPick(t.clone()) } else { Action::Play(r.tracks.clone(), i) });
                }
            }
        });
    };
    if crate::platform::phone() {
        ui.vertical(top_and_songs);
    } else {
        ui.horizontal_top(top_and_songs);
    }
    card_row_plain(ui, "s-artists", "Artists", &r.artists, acts);
    card_row_plain(ui, "s-albums", "Albums", &r.albums, acts);
    if r.tracks.len() > 4 {
        ui.add_space(26.0);
        ui.label(RichText::new("More songs").font(theme::bold(24.0)));
        ui.add_space(10.0);
        for (i, t) in r.tracks.iter().enumerate().skip(4) {
            let o = RowOpts { pick, ..rows.opts(liked, t, None, true) };
            if track_row(ui, t, &o, acts).clicked() {
                acts.push(if pick { Action::TransferPick(t.clone()) } else { Action::Play(r.tracks.clone(), i) });
            }
        }
    }
    card_row_plain(ui, "s-pl", "Playlists", &r.playlists, acts);
}

/// Apple's search landing: what was opened from the last few searches, with its artwork.
fn recent_searches(ui: &mut Ui, app: &mut App) {
    if app.recent_searches.is_empty() {
        ui.label(RichText::new("Search for artists, songs, albums and playlists.").font(theme::regular(15.0)).color(theme::text_dim()));
        return;
    }
    ui.label(RichText::new("Recently Searched").font(theme::bold(22.0)).color(theme::text()));
    ui.add_space(12.0);
    let w = ui.available_width().min(620.0);
    // The rows are cloned so a removal can be applied after the list is drawn.
    let recents = app.recent_searches.clone();
    let mut remove = None;
    for (i, r) in recents.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click());
        let resp = named(resp, &r.title).on_hover_cursor(egui::CursorIcon::PointingHand);
        let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.contains_pointer(), 0.12);
        ui.painter().rect_filled(rect, 10.0, theme::glass(0.05 + 0.06 * h));
        let art = Rect::from_min_size(rect.left_top() + vec2(8.0, 8.0), Vec2::splat(48.0));
        let radius = if r.round { 24.0 } else { 6.0 };
        match r.thumb.as_deref().filter(|u| !u.is_empty()) {
            Some(url) => {
                let thumbs = [crate::model::Thumb { url: url.to_owned(), width: 300, height: 300 }];
                paint_cover(ui, art, &thumbs, radius, 1.0);
            }
            None => {
                ui.painter().rect_filled(art, radius, theme::glass(0.1));
            }
        }
        let x = art.right() + 14.0;
        widgets::text_at(ui, pos2(x, rect.center().y - 17.0), &r.title, theme::semibold(15.0), theme::text(), rect.right() - x - 12.0);
        widgets::text_at(ui, pos2(x, rect.center().y + 4.0), &r.subtitle, theme::regular(13.0), theme::text_dim(), rect.right() - x - 12.0);
        egui::Popup::context_menu(&resp).show(|ui| {
            ui.set_min_width(200.0);
            if ui.button("Remove from Recently Searched").clicked() {
                remove = Some(i);
                ui.close();
            }
        });
        if resp.clicked() {
            app.actions.push(match &r.target {
                Some(t) => Action::Open(t.clone()),
                // An entry from an earlier version kept only the search.
                None => Action::Go(Page::Search(r.query.clone())),
            });
        }
        ui.add_space(6.0);
    }
    if let Some(i) = remove {
        app.recent_searches.remove(i);
    }
}

/// `pick`: choosing a transfer match, so clicking a song picks it.
fn top_result(ui: &mut Ui, c: &Card, pick: bool, liked: bool, acts: &mut Vec<Action>) {    ui.vertical(|ui| {
        ui.label(RichText::new("Top result").font(theme::bold(24.0)));
        ui.add_space(8.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 236.0), Sense::click());
        let resp = named(resp, &c.title).on_hover_cursor(egui::CursorIcon::PointingHand);
        let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.contains_pointer(), 0.2);
        ui.painter().rect_filled(rect, 14.0, theme::glass(0.07 + 0.05 * h));
        let img = Rect::from_min_size(rect.min + vec2(20.0, 20.0), Vec2::splat(110.0));
        paint_cover(ui, img, &c.thumbs, if c.round { 55.0 } else { theme::card_radius() }, 1.0);
        let x = rect.left() + 20.0;
        widgets::text_at(ui, pos2(x, img.bottom() + 16.0), &c.title, theme::bold(26.0), theme::text(), rect.width() - 40.0);
        widgets::linked_text(ui, pos2(x, img.bottom() + 52.0), &c.subtitle, &c.links, theme::regular(14.0), theme::text_dim(), rect.width() - 40.0, acts);
        // Floating play button
        let pc = rect.right_bottom() + vec2(-44.0, -44.0 + 6.0 * (1.0 - h));
        let pr = Rect::from_center_size(pc, Vec2::splat(52.0));
        let active = widgets::active_state(ui, &c.target);
        let playing = active == Some(true);
        let label = format!("{} {}", if playing { "Pause" } else { "Play" }, c.title);
        let p = named(ui.interact(pr, resp.id.with("play"), Sense::click()), &label).on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.painter().circle_filled(pc, 26.0, theme::with_alpha(Color32::WHITE, 0.4 + 0.6 * h));
        let nudge = if playing { 0.0 } else { 1.5 };
        widgets::play_pause_glyph(ui, pc + vec2(nudge, 0.0), 10.0, Color32::from_gray(15), playing);
        if p.clicked() {
            acts.push(match &c.target {
                _ if active.is_some() => Action::TogglePlay,
                Target::Song(t) => Action::Radio(t.clone()),
                t => Action::PlayCollection(t.clone(), false),
            });
        } else if resp.clicked() {
            acts.push(match &c.target {
                Target::Song(t) if pick => Action::TransferPick(t.clone()),
                t => Action::Open(t.clone()),
            });
        }
        // Right-click the box for the same menu the tile's ⋮ carries.
        egui::Popup::context_menu(&resp).show(|ui| widgets::card_menu_contents(ui, c, liked, acts));
    });
}

// ------------------------------------------------------------------ album / playlist

/// What an album's or playlist's "More" menu offers.
struct CollectionMenu<'a> {
    tracks: &'a [Track],
    /// The id it is saved to the library by, and whether it is saved; none for a
    /// playlist of the account's own.
    save: Option<(&'a str, bool)>,
    /// Its songs can be selected: signed in, and not one of the account's automatic lists.
    selectable: bool,
    selecting: bool,
    owned: bool,
    target: &'a Target,
}

impl CollectionMenu<'_> {
    /// Draw the menu. `buttons`: saving and sharing are buttons on the page (the
    /// Material phone), and not said again here. Returns whether select mode was
    /// turned on (`Some(true)`) or off.
    fn show(&self, ui: &mut Ui, buttons: bool, acts: &mut Vec<Action>) -> Option<bool> {
        let mut select = None;
        widgets::menu_begin(ui);
        ui.add_space(3.0);
        if let Some(first) = self.tracks.first()
            && widgets::menu_action(ui, Some(icon::RADIO), "Start radio")
        {
            acts.push(Action::Radio(first.clone()));
        }
        if let Some((save_id, saved)) = self.save.filter(|_| !buttons)
            && widgets::menu_action(ui, Some(icon::LIBRARY), if saved { "Remove from library" } else { "Save to library" })
        {
            acts.push(Action::SaveCollection(save_id.to_owned(), !saved));
        }
        if self.selectable && !self.tracks.is_empty() && widgets::menu_action(ui, Some(icon::LIST_CHECKS), if self.selecting { "Done selecting" } else { "Select" }) {
            select = Some(!self.selecting);
        }
        if self.selectable && (self.save.is_some() || self.owned) && !(buttons && !self.owned) {
            ui.separator();
        }
        if self.owned {
            if widgets::menu_action(ui, Some(icon::MUSIC), "Edit playlist") {
                acts.push(Action::EditPlaylist);
            }
            if widgets::menu_action(ui, Some(icon::USERS), "Collaborate") {
                acts.push(Action::Collaborate);
            }
            if widgets::menu_action(ui, Some(icon::TRASH_2), "Delete playlist") {
                acts.push(Action::DeletePlaylist);
            }
        }
        if !buttons {
            if self.selectable && !self.owned && self.save.is_none() {
                ui.separator();
            }
            if widgets::menu_action(ui, Some(icon::LINK), "Share") {
                acts.push(Action::Share(self.target.clone()));
            }
        }
        ui.add_space(3.0);
        select
    }
}

/// How wide the cover at the head of an album's or playlist's page is, on a page
/// `width` wide.
fn cover_side(width: f32) -> f32 {
    (width * 0.56).clamp(160.0, 220.0)
}

/// How long a list plays, in hours and minutes: "3 hr 12 min", "45 min".
fn length_text(secs: u32) -> String {
    let (hours, minutes) = (secs / 3600, secs % 3600 / 60);
    if hours > 0 { format!("{hours} hr {minutes} min") } else { format!("{minutes} min") }
}

/// The one line under a collection's title: who it is by, how many songs and how long
/// it plays. `counted`: the byline says how many songs already (a playlist's does).
fn meta_line(byline: &str, counted: bool, songs: usize, secs: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !byline.is_empty() {
        parts.push(byline.to_owned());
    }
    if secs > 0 {
        if !counted {
            parts.push(count_text(songs, "song"));
        }
        parts.push(length_text(secs));
    }
    parts.join(" \u{2022} ")
}

/// The side of a round action beside the large Play, and that button's height.
const ACTION: f32 = 48.0;
const PLAY_H: f32 = 56.0;

/// Where the actions of an album, playlist or artist are: the large Play, with round
/// buttons before and after it, the row centred.
#[derive(Clone, Debug, PartialEq)]
struct ActionRow {
    before: Vec<Rect>,
    play: Rect,
    after: Vec<Rect>,
}

fn action_row(row: Rect, before: usize, after: usize) -> ActionRow {
    const GAP: f32 = 8.0;
    let rounds = (before + after) as f32 * (ACTION + GAP);
    let play_w = (row.width() - rounds).clamp(96.0, 136.0);
    let mut x = row.center().x - (rounds + play_w) / 2.0;
    let cy = row.center().y;
    let mut round = |x: &mut f32, lead: f32| {
        *x += lead;
        let r = Rect::from_min_size(pos2(*x, cy - ACTION / 2.0), Vec2::splat(ACTION));
        *x += ACTION + GAP - lead;
        r
    };
    let before: Vec<Rect> = (0..before).map(|_| round(&mut x, 0.0)).collect();
    let play = Rect::from_min_size(pos2(x, cy - PLAY_H / 2.0), vec2(play_w, PLAY_H));
    x += play_w;
    let after: Vec<Rect> = (0..after).map(|_| round(&mut x, GAP)).collect();
    ActionRow { before, play, after }
}

fn collection(app: &mut App, ui: &mut Ui, id: &str, now: f64) {
    if app.opened(id).is_none() {
        placeholder(app, ui, now, 1, 232.0);
        return;
    }
    let rows = Rows::new(app, now);
    // The header's motion sleeve, asked for before the release is borrowed below.
    // Albums only: playlist covers stay still.
    let (release, artist) = match app.opened(id) {
        Some(o) => (o.c.title.clone(), o.c.artists.first().map(|a| a.name.clone()).unwrap_or_else(|| o.c.byline())),
        None => (String::new(), String::new()),
    };
    let animated = app.opened(id).is_some_and(|o| o.c.is_album);
    if animated {
        app.apple_cover(&release, &artist, None);
    }
    let px = (232.0 * ui.ctx().pixels_per_point()).round().max(256.0) as u32;
    let sprite = if animated { app.apple_sprite(ui.ctx(), &release, &artist, None, px) } else { None };
    let Some(o) = app.lists.get(id) else { return };
    let (c, more) = (&o.c, app.receiving(id));
    let acts = &mut app.actions;
    let tracks = &c.tracks;
    let is_album = c.is_album;
    let filter_on = app.coll_search.as_ref().is_some_and(|(fid, _)| fid == id);
    let selecting = app.selection.on(&app.page);
    let mut toggle_search = false;
    let mut start_select = false;
    let mut stop_select = false;
    let mut download = false;
    let downloading = app.downloads.queue.collection(id).is_some();
    ui.add_space(18.0);
    let target = if is_album { Target::Album(id.to_owned()) } else { Target::Playlist(id.to_owned()) };
    // Liked Music and Episodes for later are the account's own lists: they can't be
    // saved, removed, edited or deleted.
    let auto = matches!(id, "LM" | "SE");
    let saved = app.library.as_ref().is_some_and(|l| {
        let cards = if is_album { &l.albums } else { &l.playlists };
        cards.iter().any(|card| card.target == target)
    });
    // Saving only applies to someone else's playlist or an album.
    let save_id = if is_album { c.save_id.clone() } else { (!c.owned).then(|| id.to_owned()) };
    let owned = !is_album && c.owned;
    let menu = CollectionMenu { tracks, save: save_id.as_deref().map(|id| (id, saved)), selectable: app.logged_in && !auto, selecting, owned, target: &target };
    // Beside the art on a wide screen; below it, and smaller, on a phone.
    let phone = crate::platform::phone();
    let header = |ui: &mut Ui| {
        let (img, _) = ui.allocate_exact_size(Vec2::splat(if phone { 168.0 } else { 232.0 }), Sense::hover());
        widgets::soft_shadow(ui, img, 10.0, 40.0, 0.5);
        paint_cover(ui, img, &c.thumbs, theme::card_radius() + 2.0, 1.0);
        if let Some(s) = &sprite {
            widgets::paint_motion(ui, img, theme::card_radius() + 2.0, s);
        }
        ui.add_space(if phone { 16.0 } else { 26.0 });
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width().min(760.0));
            ui.add_space(if phone { 0.0 } else { 24.0 });
            ui.label(RichText::new(if is_album { "ALBUM" } else { "PLAYLIST" }).font(theme::semibold(12.0)).color(theme::text_dim()));
            ui.label(RichText::new(&c.title).font(theme::bold(if phone { 26.0 } else { 40.0 })).color(theme::text()));
            ui.add_space(2.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
            widgets::linked_text(ui, r.min, &c.byline(), &c.artists, theme::regular(15.0), theme::text_dim(), r.width(), acts);
            let total: u32 = tracks.iter().filter_map(|t| t.duration).sum();
            if total > 0 {
                ui.label(
                    RichText::new(format!("{} songs • {} min", tracks.len(), total / 60)).font(theme::regular(13.5)).color(theme::text_faint()),
                );
            }
            if let Some(d) = c.description.as_deref().filter(|d| !d.trim().is_empty()) {
                ui.add_space(6.0);
                let g = widgets::galley_wrapped(ui, d.trim(), theme::regular(13.5), theme::text_dim(), ui.available_width(), 2);
                ui.label(g);
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
                // A phone downloads the whole of it: its sheet asks first, and shows one on its way.
                if phone && circle_action(ui, CircleGlyph::Char(icon::DOWNLOAD), if downloading { "Downloading" } else { "Download" }, downloading).clicked() {
                    download = true;
                }
                play_pill(ui, acts, &target, tracks);
                if circle_action(ui, CircleGlyph::Char(icon::SHUFFLE), "Shuffle", false).clicked() && !tracks.is_empty() {
                    let mut t = tracks.clone();
                    super::shuffle(&mut t);
                    acts.push(Action::Play(t, 0));
                }
                if circle_action(ui, CircleGlyph::Char(icon::SEARCH), "Search in this list", filter_on).clicked() {
                    toggle_search = true;
                }
                let more = circle_action(ui, CircleGlyph::More, "More", false);
                egui::Popup::menu(&more).show(|ui| match menu.show(ui, false, acts) {
                    Some(true) => start_select = true,
                    Some(false) => stop_select = true,
                    None => {}
                });
            });
            if toggle_search {
                let closing = app.coll_search.as_ref().is_some_and(|(fid, _)| fid == id);
                app.coll_search = (!closing).then(|| (id.to_owned(), String::new()));
            }
            if start_select {
                app.selection.start(&app.page);
            }
            if stop_select {
                app.selection.clear();
            }

        });
    };
    if m3::on() {
        use crate::material::{Type, Weight, shape};
        let m = crate::material::scheme();
        let w = ui.available_width();
        // The cover, large and centred; its title and one line about it under it.
        let side = cover_side(w);
        ui.add_space(4.0);
        let (band, _) = ui.allocate_exact_size(vec2(w, side), Sense::hover());
        let img = Rect::from_center_size(band.center(), Vec2::splat(side));
        // A lined look stands the cover on a second card instead of a shadow.
        let cover_radius = m3::round(shape::CARD);
        if crate::material::style().outlined() {
            m3::plate(ui, img, cover_radius, crate::material::scheme().surface_container, m3::Stack::Card);
        } else {
            widgets::soft_shadow(ui, img, shape::CARD, 28.0, 0.45);
        }
        paint_cover(ui, img, &c.thumbs, cover_radius, 1.0);
        if let Some(s) = &sprite {
            widgets::paint_motion(ui, img, cover_radius, s);
        }
        ui.add_space(20.0);
        let centred = |ui: &mut Ui, g: std::sync::Arc<egui::Galley>, color: Color32| {
            let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
            ui.painter().galley(pos2(r.center().x, r.top()), g, color);
        };
        let title = m3::galley_centered(ui, &c.title, Type::HeadlineSmall.spec().weight(Weight::Medium), m.on_surface, w - 16.0, 2);
        centred(ui, title, m.on_surface);
        ui.add_space(6.0);
        let total: u32 = tracks.iter().filter_map(|t| t.duration).sum();
        let meta = meta_line(&c.byline(), c.count.is_some(), tracks.len(), total);
        let meta_w = widgets::galley(ui, &meta, Type::BodyMedium.font(), m.on_surface_variant, w).size().x;
        let (r, _) = ui.allocate_exact_size(vec2(w, Type::BodyMedium.spec().line_height), Sense::hover());
        widgets::linked_text(ui, pos2(r.center().x - meta_w / 2.0, r.top()), &meta, &c.artists, Type::BodyMedium.font(), m.on_surface_variant, w, acts);
        if let Some(d) = c.description.as_deref().filter(|d| !d.trim().is_empty()) {
            ui.add_space(6.0);
            let g = m3::galley_centered(ui, d.trim(), Type::BodyMedium, m.on_surface_variant, w - 16.0, 2);
            centred(ui, g, m.on_surface_variant);
        }
        ui.add_space(16.0);
        // The actions: the large Play, with round buttons either side of it.
        let (row, _) = ui.allocate_exact_size(vec2(w, PLAY_H), Sense::hover());
        let slots = action_row(row, 1 + save_id.is_some() as usize, 2);
        let ids = ui.scope_id().with("collection-actions");
        let round = |ui: &Ui, at: Rect, glyph: char, name: &str, style: m3::IconStyle| m3::IconButton::new(glyph, name).style(style).size(ACTION).show_at(ui, at.center(), ids.with(name)).clicked();
        let quiet = m3::IconStyle::Surface;
        // The whole of it is downloaded: its sheet asks first, and shows one on its way.
        if round(ui, slots.before[0], icon::DOWNLOAD, if downloading { "Downloading" } else { "Download" }, if downloading { m3::IconStyle::Tonal } else { quiet }) {
            download = true;
        }
        if let (Some(save_id), Some(at)) = (&save_id, slots.before.get(1)) {
            let (glyph, name) = if saved { (icon::CHECK, "Remove from library") } else { (icon::LIST_PLUS, "Save to library") };
            if round(ui, *at, glyph, name, if saved { m3::IconStyle::Tonal } else { quiet }) {
                acts.push(Action::SaveCollection(save_id.clone(), !saved));
            }
        }
        let active = widgets::active_state(ui, &target);
        let (label, glyph) = if active == Some(true) { ("Pause", super::glyphs::PAUSE) } else { ("Play", super::glyphs::PLAY) };
        if m3::big_button(ui, slots.play, label, glyph, ids.with("play")).clicked() {
            if active.is_some() {
                acts.push(Action::TogglePlay);
            } else if !tracks.is_empty() {
                acts.push(Action::Play(tracks.to_vec(), 0));
            }
        }
        if round(ui, slots.after[0], icon::SHUFFLE, "Shuffle", m3::IconStyle::Tonal) && !tracks.is_empty() {
            let mut t = tracks.clone();
            super::shuffle(&mut t);
            acts.push(Action::Play(t, 0));
        }
        if round(ui, slots.after[1], icon::LINK, "Share", quiet) {
            acts.push(Action::Share(target.clone()));
        }
        // In the top bar's right end: the search in this list, and the rest of what can be done.
        if let Some((first, second)) = super::phone::page_slots(ui.ctx()) {
            let both = first.union(second);
            egui::Area::new(ui.scope_id().with("collection-bar")).order(egui::Order::Middle).fixed_pos(both.min).show(ui.ctx(), |ui| {
                ui.allocate_exact_size(both.size(), Sense::hover());
                let style = if filter_on { m3::IconStyle::Tonal } else { m3::IconStyle::Standard };
                if m3::IconButton::new(icon::SEARCH, "Search in this list").style(style).show_at(ui, first.center(), ui.scope_id().with("search")).clicked() {
                    toggle_search = true;
                }
                let more = m3::IconButton::new(icon::ELLIPSIS_VERTICAL, "More").show_at(ui, second.center(), ui.scope_id().with("more"));
                egui::Popup::menu(&more).show(|ui| match menu.show(ui, true, acts) {
                    Some(true) => start_select = true,
                    Some(false) => stop_select = true,
                    None => {}
                });
            });
        }
        if toggle_search {
            let closing = app.coll_search.as_ref().is_some_and(|(fid, _)| fid == id);
            app.coll_search = (!closing).then(|| (id.to_owned(), String::new()));
        }
        if start_select {
            app.selection.start(&app.page);
        }
        if stop_select {
            app.selection.clear();
        }
    } else if phone {
        ui.vertical(header);
    } else {
        ui.horizontal_top(header);
    }
    let mut dismiss_search = false;
    if let Some((fid, query)) = &mut app.coll_search
        && fid == id
        && m3::on()
    {
        ui.add_space(12.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), m3::SEARCH_BAR_H), Sense::hover());
        let edit = m3::search_bar(ui, rect, query, "Search in this list", 0.0).edit;
        if toggle_search {
            edit.request_focus();
        } else if !edit.has_focus() && query.is_empty() {
            // Left alone empty, the field goes away instead of sitting over the list.
            dismiss_search = true;
        }
        ui.add_space(12.0);
    } else if let Some((fid, query)) = &mut app.coll_search
        && fid == id
    {
        ui.add_space(14.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 40.0), Sense::hover());
        ui.painter().rect_filled(rect, 20.0, theme::glass(0.09));
        widgets::icon_at(ui, pos2(rect.left() + 20.0, rect.center().y), icon::SEARCH, 16.0, theme::text_dim());
        let clearable = !query.is_empty();
        let inner = Rect::from_min_max(pos2(rect.left() + 40.0, rect.top() + 4.0), pos2(rect.right() - if clearable { 40.0 } else { 16.0 }, rect.bottom() - 4.0));
        let edit = ui.put(
            inner,
            egui::TextEdit::singleline(query)
                .hint_text(egui::RichText::new("Search in this list").color(theme::text_faint()))
                .font(theme::regular(14.0))
                .vertical_align(egui::Align::Center)
                .frame(egui::Frame::NONE)
                .desired_width(inner.width()),
        );
        if toggle_search {
            edit.request_focus();
        } else if !edit.has_focus() && query.is_empty() {
            // Left alone empty, the field goes away instead of sitting over the list.
            dismiss_search = true;
        }
        if clearable {
            let x = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(26.0));
            let resp = named(ui.interact(x, ui.scope_id().with("coll-clear"), Sense::click()), "Clear search").on_hover_cursor(egui::CursorIcon::PointingHand);
            widgets::icon_at(ui, x.center(), icon::X, 14.0, theme::text_dim());
            if resp.clicked() {
                query.clear();
            }
        }
        ui.add_space(18.0);
    } else {
        ui.add_space(if m3::on() { 12.0 } else { 26.0 });
    }
    if dismiss_search {
        app.coll_search = None;
    }
    let needle = app.coll_search.as_ref().filter(|(fid, _)| fid == id).map(|(_, q)| q.trim().to_lowercase()).unwrap_or_default();
    let filtered: Vec<Track> = if needle.is_empty() {
        Vec::new()
    } else {
        c.tracks
            .iter()
            .filter(|t| {
                t.title.to_lowercase().contains(&needle)
                    || t.artist_line().to_lowercase().contains(&needle)
                    || t.album.as_ref().is_some_and(|a| a.name.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect()
    };
    let shown: &[Track] = if needle.is_empty() { tracks } else { &filtered };
    let number = |i: usize, t: &Track| is_album.then(|| t.track_nr.map_or(i + 1, |n| n as usize));
    let own = app.logged_in && c.owned && !is_album;
    list(ui, &rows, &app.liked, shown, !is_album, number, ListCtx { sel: &mut app.selection, page: &app.page, own }, acts);
    if more {
        widgets::skeleton_rows(ui, now, 3);
    }
    if download {
        super::downloading::open_sheet(app, id);
    }
}

// ------------------------------------------------------------------ artist

/// Apple's wordmark for the artist, bottom-left at `base`, no wider than `max_w`.
/// False until the image is ready, so the caller can draw the name instead.
fn wordmark(ui: &Ui, base: egui::Pos2, logo: &crate::apple::LogoSrc, max_w: f32) -> bool {
    let aspect = logo.w.max(1) as f32 / logo.h.max(1) as f32;
    let mut size = vec2(72.0 * aspect, 72.0);
    if size.x > max_w {
        size = vec2(max_w, max_w / aspect);
    }
    let url = logo.at_height((size.y * 2.0) as u32);
    let ready = matches!(egui::Image::new(url.as_str()).load_for_size(ui.ctx(), size), Ok(egui::load::TexturePoll::Ready { .. }));
    if ready {
        let rect = Rect::from_min_size(pos2(base.x, base.y - 40.0 - size.y), size);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        widgets::paint_remote(ui, &url, rect, 0.0, uv, Color32::WHITE);
    }
    ready
}

fn artist(app: &mut App, ui: &mut Ui, id: &str, now: f64) {
    if app.artist.as_ref().is_none_or(|a| a.id != id) {
        placeholder(app, ui, now, 2, CARD);
        return;
    }
    let rows = Rows::new(app, now);
    let accent = app.accent_color();
    // Apple's own page for this artist: their display name and the hero clip. Artists
    // Apple has no clip for (Charli xcx) keep the still banner.
    let apple = app.apple_artists.get(id).and_then(|o| o.clone());
    let name = apple.as_ref().map(|x| x.name.clone()).unwrap_or_else(|| app.artist.as_ref().map(|a| a.name.clone()).unwrap_or_default());
    // Apple's search misses once now and then. While the page is open, ask again after
    // the cooldown rather than waiting for the listener to come back to it.
    if apple.is_none() {
        let key = format!("artist:{id}");
        let cooling = app.apple_missed.get(&key).is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(120));
        if !cooling && app.apple_waiting.insert(key) {
            app.backend.request(crate::backend::Req::AppleArtist { id: id.to_owned(), name: name.clone() });
        }
    }
    // Apple's wordmark wins the hero; failing one, the listener's signature service;
    // failing that, the name in type. A lettered artist still has their signature
    // asked for, because it signs the About section below instead of the hero.
    let signature = app.signature_texture(ui.ctx(), &name);
    let Some(a) = &app.artist else { return };
    let acts = &mut app.actions;
    let top = &a.top;
    let material = m3::on();
    let hero_radius = if material { m3::round(crate::material::shape::CARD) } else { 14.0 };
    // Hero banner fading into the page.
    let w = ui.available_width();
    let hero_h = (w * 0.34).clamp(240.0, 380.0);
    let (hero, _) = ui.allocate_exact_size(vec2(w, hero_h), Sense::hover());
    if let Some(t) = a.banner.iter().max_by_key(|t| t.width) {
        let img_ratio = if t.height > 0 { t.width as f32 / t.height as f32 } else { 16.0 / 9.0 };
        // Cover-fit: crop the image's top band to the hero's aspect.
        let vis_h = (img_ratio / (w / hero_h)).min(1.0);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, vis_h));
        ui.painter().rect_filled(hero, hero_radius, theme::glass(0.07));
        widgets::paint_remote(ui, &t.url, hero, hero_radius, uv, Color32::WHITE);
    } else {
        ui.painter().rect_filled(hero, hero_radius, theme::with_alpha(accent, 0.25));
    }
    if material {
        // The darkening keeps to the picture's round corners.
        m3::scrim_bottom(ui, hero, hero_radius, hero.center().y - 40.0, 0.85);
        m3::outline(ui, hero, hero_radius);
    } else {
        super::shell::vgradient(ui.painter(), Rect::from_min_max(pos2(hero.left(), hero.center().y - 40.0), hero.max), Color32::TRANSPARENT, theme::shade(0.85));
    }
    let base = hero.left_bottom() + vec2(if material { 20.0 } else { 28.0 }, -30.0);
    // Apple letters some artists a wordmark image instead of setting their name in type.
    // Until it downloads (or when there's none) the typed name stands in.
    let drawn = apple.as_ref().and_then(|x| x.logo.as_ref()).is_some_and(|logo| wordmark(ui, base, logo, w * 0.62));
    if !drawn {
        match &signature {
            // The artist's own hand, credited above it the way a page signs itself off.
            Some(tex) => {
                let tex_size = tex.size_vec2();
                let aspect = tex_size.x / tex_size.y.max(1.0);
                let mut size = vec2(96.0 * aspect, 96.0);
                let max_w = (w * 0.42).min(340.0);
                if size.x > max_w {
                    size = vec2(max_w, max_w / aspect);
                }
                let rect = Rect::from_min_size(pos2(base.x, base.y - 40.0 - size.y), size);
                ui.painter().text(pos2(base.x, rect.top() - 8.0), Align2::LEFT_BOTTOM, format!("Signed by {name}"), theme::regular(12.0), Color32::from_white_alpha(199));
                let tex = egui::load::SizedTexture::from_handle(tex);
                egui::Image::from_texture(tex).uv(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))).paint_at(ui, rect);
            }
            None if material => {
                let role = crate::material::Type::DisplaySmall.spec().weight(crate::material::Weight::Bold);
                let g = m3::galley(ui, &name, role, Color32::WHITE, w - 40.0);
                ui.painter().galley(base - vec2(0.0, 44.0 + g.size().y), g, Color32::WHITE);
            }
            None => {
                ui.painter().text(base - vec2(0.0, 44.0), Align2::LEFT_BOTTOM, &name, theme::bold(58.0), Color32::WHITE);
            }
        }
    }
    if let Some(s) = a.subscribers {
        ui.painter().text(base - vec2(0.0, 18.0), Align2::LEFT_BOTTOM, format!("{} subscribers", compact(s)), theme::regular(14.0), theme::text_dim());
    }
    ui.add_space(18.0);
    if material {
        // The large Play between the artist's radio and a shuffle.
        let (row, _) = ui.allocate_exact_size(vec2(w, PLAY_H), Sense::hover());
        let slots = action_row(row, top.first().is_some() as usize, 1);
        let ids = ui.scope_id().with("artist-actions");
        if let (Some(first), Some(at)) = (top.first(), slots.before.first())
            && m3::IconButton::new(icon::RADIO, "Radio").style(m3::IconStyle::Surface).size(ACTION).show_at(ui, at.center(), ids.with("radio")).clicked()
        {
            acts.push(Action::Radio(first.clone()));
        }
        let active = widgets::active_state(ui, &Target::Artist(id.to_owned()));
        let (label, glyph) = if active == Some(true) { ("Pause", super::glyphs::PAUSE) } else { ("Play", super::glyphs::PLAY) };
        if m3::big_button(ui, slots.play, label, glyph, ids.with("play")).clicked() {
            if active.is_some() {
                acts.push(Action::TogglePlay);
            } else if !top.is_empty() {
                acts.push(Action::Play(top.to_vec(), 0));
            }
        }
        if m3::IconButton::new(icon::SHUFFLE, "Shuffle").style(m3::IconStyle::Tonal).size(ACTION).show_at(ui, slots.after[0].center(), ids.with("shuffle")).clicked() && !top.is_empty() {
            let mut t = top.clone();
            super::shuffle(&mut t);
            acts.push(Action::Play(t, 0));
        }
    }
    if !material {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            play_pill(ui, acts, &Target::Artist(id.to_owned()), top);
            if pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() && !top.is_empty() {
                let mut t = top.clone();
                super::shuffle(&mut t);
                acts.push(Action::Play(t, 0));
            }
            if let Some(first) = top.first()
                && pill(ui, "Radio", Some(icon::RADIO), false).clicked() {
                    acts.push(Action::Radio(first.clone()));
                }
        });
    }
    if !top.is_empty() && material {
        let trailing = if a.top_playlist.is_some() { m3::Trailing::Text("Show all") } else { m3::Trailing::None };
        if m3::section_header(ui, &m3::SectionHeader { title: "Top songs", overline: None, avatar: None, trailing })
            && let Some(pl) = &a.top_playlist
        {
            acts.push(Action::Open(Target::Playlist(pl.clone())));
        }
    } else if !top.is_empty() {
        ui.add_space(26.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Top songs").font(theme::bold(24.0)).color(theme::text()));
            if let Some(pl) = &a.top_playlist {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill(ui, "Show all", None, false).clicked() {
                        acts.push(Action::Open(Target::Playlist(pl.clone())));
                    }
                });
            }
        });
        ui.add_space(12.0);
    }
    if !top.is_empty() {
        for (i, t) in top.iter().enumerate().take(5) {
            let o = rows.opts(&app.liked, t, None, true);
            if track_row(ui, t, &o, acts).clicked() {
                acts.push(Action::Play(top.clone(), i));
            }
        }
    }
    card_row_plain(ui, "a-albums", "Albums & singles", &a.albums, acts);
    card_row_plain(ui, "a-pl", "Featured on", &a.playlists, acts);
    card_row_plain(ui, "a-sim", "Fans might also like", &a.similar, acts);
    if let Some(d) = a.description.as_deref().filter(|d| !d.trim().is_empty()).filter(|_| material) {
        // The signature of a lettered artist stands under the prose, at the page's width.
        m3::section_header(ui, &m3::SectionHeader { title: "About", overline: None, avatar: None, trailing: m3::Trailing::None });
        let g = m3::galley_wrapped(ui, d.trim(), crate::material::Type::BodyMedium, theme::text_dim(), ui.available_width(), 8);
        ui.label(g);
        if let Some(tex) = drawn.then_some(signature.as_ref()).flatten() {
            ui.add_space(16.0);
            let tex_size = tex.size_vec2();
            let size = vec2(96.0 * tex_size.x / tex_size.y.max(1.0), 96.0);
            let size = if size.x > ui.available_width() { size * (ui.available_width() / size.x) } else { size };
            let (rect, _) = ui.allocate_exact_size(vec2(size.x, size.y + 22.0), Sense::hover());
            ui.painter().text(rect.left_top(), Align2::LEFT_TOP, format!("Signed by {name}"), crate::material::Type::BodySmall.font(), theme::text_dim());
            let img = Rect::from_min_size(pos2(rect.left(), rect.top() + 22.0), size);
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex)).uv(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))).paint_at(ui, img);
        }
    } else if let Some(d) = a.description.as_deref().filter(|d| !d.trim().is_empty()) {
        shelf_header(ui, "About", None, None, false);
        let full = ui.available_width().min(820.0);
        // A lettered artist's signature can't be the hero, so it signs the About
        // section beside the prose the way a page signs itself off.
        let beside = drawn.then_some(signature.as_ref()).flatten();
        if let Some(tex) = beside {
            ui.horizontal_top(|ui| {
                let sig_w = (full * 0.28).clamp(120.0, 210.0);
                let text_w = (full - sig_w - 32.0).max(220.0);
                let g = widgets::galley_wrapped(ui, d.trim(), theme::regular(14.5), theme::text_dim(), text_w, 8);
                ui.label(g);
                ui.add_space(32.0);
                let tex_size = tex.size_vec2();
                let aspect = tex_size.x / tex_size.y.max(1.0);
                let mut size = vec2(sig_w, sig_w / aspect);
                if size.y > 140.0 {
                    size = vec2(140.0 * aspect, 140.0);
                }
                let (rect, _) = ui.allocate_exact_size(vec2(size.x, size.y + 22.0), Sense::hover());
                ui.painter().text(rect.left_top(), Align2::LEFT_TOP, format!("Signed by {name}"), theme::regular(12.0), Color32::from_white_alpha(199));
                let img = Rect::from_min_size(pos2(rect.left(), rect.top() + 22.0), size);
                let tex = egui::load::SizedTexture::from_handle(tex);
                egui::Image::from_texture(tex).uv(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))).paint_at(ui, img);
            });
        } else {
            let g = widgets::galley_wrapped(ui, d.trim(), theme::regular(14.5), theme::text_dim(), full, 8);
            ui.label(g);
        }
    }
}

// ------------------------------------------------------------------ library

/// Dev hook: YTM_FAKE_LIBRARY shows a library made of the home feed's cards, for a
/// screenshot of the page without an account.
fn demo_library(app: &App) -> crate::backend::Library {
    let cards: Vec<&Card> = app.home.iter().flat_map(|h| &h.shelves).filter_map(|s| if let ShelfKind::Cards(cards) = &s.kind { Some(cards) } else { None }).flatten().collect();
    let pick = |want: fn(&Target) -> bool| cards.iter().filter(|c| want(&c.target)).take(8).map(|c| (*c).clone()).collect::<Vec<_>>();
    let liked = Card { title: "Liked Music".into(), subtitle: "Auto playlist".into(), thumbs: vec![], target: Target::Playlist(super::LIKED_MUSIC.into()), round: false, links: vec![] };
    let mut playlists = vec![liked];
    playlists.extend(pick(|t| matches!(t, Target::Playlist(_) | Target::Artist(_))));
    crate::backend::Library { playlists, albums: pick(|t| matches!(t, Target::Album(_))), playlists_failed: false, albums_failed: false }
}

/// How a library row leads and what marks it: Liked Music is the account's own list,
/// shown as a heart on a tonal tile and pinned; what was downloaded whole says so.
fn library_row_marks(c: &Card, downloaded: &HashSet<String>) -> (Option<m3::Icon>, Option<m3::Icon>) {
    let id = match &c.target {
        Target::Playlist(id) | Target::Album(id) => id.as_str(),
        _ => "",
    };
    if id == super::LIKED_MUSIC {
        (Some(icon::HEART.into()), Some(icon::PIN.into()))
    } else if downloaded.contains(id) {
        (None, Some(m3::Icon::Drawn(m3::drawn::saved)))
    } else {
        (None, None)
    }
}

/// The line between the library's chips and its list: how many there are.
fn library_count(ui: &mut Ui, text: &str) {
    let m = crate::material::scheme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let g = m3::galley(ui, text, crate::material::Type::TitleSmall, m.on_surface, rect.width());
    ui.painter().galley(pos2(rect.left(), rect.center().y - g.size().y / 2.0), g, m.on_surface);
}

/// The library on the Material phone: filter chips (History is the top bar's action),
/// then the playlists or albums as list rows, or the songs of the chosen list. A
/// floating button makes a new playlist.
fn library_material(app: &mut App, ui: &mut Ui, tab: LibTab, now: f64) {
    use crate::material::Type;
    let m = crate::material::scheme();
    let demo = super::dev_var("YTM_FAKE_LIBRARY").is_some();
    if !app.logged_in && !demo {
        ui.add_space(8.0);
        ui.label(RichText::new("Sign in to see your playlists, liked songs and history.").font(Type::BodyLarge.font()).color(m.on_surface_variant));
        ui.add_space(12.0);
        if pill(ui, "Sign in with Google", Some(icon::CIRCLE_USER), true).clicked() {
            app.start_sign_in(ui.ctx());
        }
        return;
    }
    if demo && app.library.is_none() && app.home.is_some() {
        app.library = Some(demo_library(app));
    }
    let mut go = None;
    m3::chip_row(ui, "library-tabs", super::phone::SIDE, |ui| {
        for (t, label) in [(LibTab::Playlists, "Playlists"), (LibTab::Songs, "Liked songs"), (LibTab::Albums, "Albums"), (LibTab::Uploads, "Uploads")] {
            if m3::filter_chip(ui, label, tab == t).clicked() {
                go = Some(t);
            }
        }
    });
    if let Some(t) = go {
        // Tabs swap in place (no history entry); the current one refreshes.
        if tab == t {
            app.refresh();
        } else {
            app.page = Page::Library(t);
            app.page_loaded();
        }
    }
    let rows = Rows::new(app, now);
    let plain = |_: usize, _: &Track| None;
    // Play and, as round buttons beside it, shuffle and (for a list that can be picked from) select.
    let play_row = |ui: &mut Ui, songs: &[Track], acts: &mut Vec<Action>, select: Option<(&mut Selection, &Page)>| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if pill(ui, "Play", Some(icon::PLAY), true).clicked() {
                acts.push(Action::Play(songs.to_vec(), 0));
            }
            if m3::IconButton::new(icon::SHUFFLE, "Shuffle").style(m3::IconStyle::Tonal).show(ui).clicked() {
                let mut t = songs.to_vec();
                super::shuffle(&mut t);
                acts.push(Action::Play(t, 0));
            }
            if let Some((sel, page)) = select {
                let on = sel.on(page);
                let style = if on { m3::IconStyle::Tonal } else { m3::IconStyle::Surface };
                if m3::IconButton::new(icon::LIST_CHECKS, if on { "Done" } else { "Select" }).style(style).show(ui).clicked() {
                    if on { sel.clear() } else { sel.start(page) }
                }
            }
        });
        ui.add_space(4.0);
    };
    match tab {
        LibTab::History => {
            library_count(ui, "History");
            if app.history.is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let Some(h) = &app.history else { return };
            list(ui, &rows, &app.liked, h, true, plain, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
        }
        LibTab::Uploads => {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if pill(ui, "Upload music", Some(icon::UPLOAD), true).clicked() {
                    app.pick_uploads(ui.ctx());
                }
                if app.uploads.as_ref().is_some_and(|u| !u.is_empty()) && m3::IconButton::new(icon::SHUFFLE, "Shuffle").style(m3::IconStyle::Tonal).show(ui).clicked() {
                    let mut t = app.uploads.clone().unwrap_or_default();
                    super::shuffle(&mut t);
                    app.actions.push(Action::Play(t, 0));
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("MP3, M4A, FLAC, OGG and WMA.").font(Type::BodyMedium.font()).color(m.on_surface_variant));
            ui.add_space(8.0);
            if app.uploads.is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let Some(up) = &app.uploads else { return };
            if up.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new("No uploads yet").font(Type::BodyLarge.font()).color(m.on_surface_variant));
            }
            list(ui, &rows, &app.liked, up, true, plain, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
        }
        // Liked Music, kept like any playlist.
        LibTab::Songs => {
            if app.opened(super::LIKED_MUSIC).is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let more = app.receiving(super::LIKED_MUSIC);
            let Some(o) = app.lists.get(super::LIKED_MUSIC) else { return };
            let liked = &o.c.tracks;
            if !liked.is_empty() {
                ui.add_space(4.0);
                play_row(ui, liked, &mut app.actions, Some((&mut app.selection, &app.page)));
            }
            list(ui, &rows, &app.liked, liked, true, plain, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
            if more {
                widgets::skeleton_rows(ui, now, 3);
            }
        }
        LibTab::Playlists | LibTab::Albums => {
            if app.library.is_none() {
                placeholder(app, ui, now, 2, CARD);
                return;
            }
            let Some(lib) = &app.library else { return };
            let (cards, one) = if tab == LibTab::Playlists { (&lib.playlists, "playlist") } else { (&lib.albums, "album") };
            library_count(ui, &count_text(cards.len(), one));
            let downloaded: HashSet<String> = crate::downloads::collections().into_iter().map(|c| c.id).collect();
            for c in cards {
                let (tile, mark) = library_row_marks(c, &downloaded);
                card_list_row(ui, c, mark, tile, &mut app.actions);
            }
            if tab == LibTab::Playlists {
                // Over the page's bottom right corner, clear of the mini player.
                let view = Rect::from_x_y_ranges(ui.max_rect().x_range(), ui.clip_rect().y_range());
                let at = m3::fab_rect(view, if app.current().is_some() { super::phone::MINI_LIFT } else { 0.0 });
                let made = egui::Area::new(egui::Id::unique("library-new-playlist")).order(egui::Order::Middle).fixed_pos(at.min).show(ui.ctx(), |ui| {
                    ui.allocate_exact_size(at.size(), Sense::hover());
                    m3::fab(ui, at, m3::Icon::Drawn(m3::drawn::plus), "New playlist", ui.scope_id().with("fab")).clicked()
                });
                if made.inner {
                    app.actions.push(Action::NewPlaylist(Vec::new()));
                }
            }
        }
    }
}

fn library(app: &mut App, ui: &mut Ui, tab: LibTab, now: f64) {
    if m3::on() {
        return library_material(app, ui, tab, now);
    }
    root_title(ui, "Library");
    if !app.logged_in {
        ui.add_space(10.0);
        ui.label(RichText::new("Sign in to see your playlists, liked songs and history.").color(theme::text_dim()));
        ui.add_space(12.0);
        if pill(ui, "Sign in with Google", Some(icon::CIRCLE_USER), true).clicked() {
            app.start_sign_in(ui.ctx());
        }
        return;
    }
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (t, label) in [(LibTab::Playlists, "Playlists"), (LibTab::Songs, "Liked songs"), (LibTab::Albums, "Albums"), (LibTab::History, "History"), (LibTab::Uploads, "Uploads")] {
            if chip(ui, label, tab == t).clicked() {
                // Tabs swap in place (no history entry); the current one refreshes.
                if tab == t {
                    app.refresh();
                } else {
                    app.page = Page::Library(t);
                    app.page_loaded();
                }
            }
        }
    });
    ui.add_space(12.0);
    let grid = |ui: &mut Ui, cards: &[Card], acts: &mut Vec<Action>| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(20.0, 22.0);
            for c in cards {
                card(ui, c, CARD, false, acts, None);
            }
        });
    };
    let rows = Rows::new(app, now);
    let plain = |_: usize, _: &Track| None;
    match tab {
        LibTab::History => {
            if app.history.is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let Some(h) = &app.history else { return };
            list(ui, &rows, &app.liked, h, true, plain, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
        }
        LibTab::Uploads => {
            ui.horizontal(|ui| {
                if pill(ui, "Upload music", Some(icon::UPLOAD), true).clicked() {
                    app.pick_uploads(ui.ctx());
                }
                if app.uploads.as_ref().is_some_and(|u| !u.is_empty()) && pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() {
                    let mut t = app.uploads.clone().unwrap_or_default();
                    super::shuffle(&mut t);
                    app.actions.push(Action::Play(t, 0));
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Or drop audio files anywhere on the window. MP3, M4A, FLAC, OGG and WMA.").font(theme::regular(13.0)).color(theme::text_dim()));
            ui.add_space(12.0);
            if app.uploads.is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let Some(up) = &app.uploads else { return };
            if up.is_empty() {
                ui.add_space(20.0);
                ui.label(RichText::new("No uploads yet").color(theme::text_dim()));
            }
            list(ui, &rows, &app.liked, up, true, plain, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
        }
        // Liked Music, kept like any playlist.
        LibTab::Songs => {
            if app.opened(super::LIKED_MUSIC).is_none() {
                placeholder(app, ui, now, 1, 48.0);
                return;
            }
            let more = app.receiving(super::LIKED_MUSIC);
            let Some(o) = app.lists.get(super::LIKED_MUSIC) else { return };
            let liked = &o.c.tracks;
            if !liked.is_empty() {
                ui.horizontal(|ui| {
                    if pill(ui, "Play", Some(icon::PLAY), true).clicked() {
                        app.actions.push(Action::Play(liked.clone(), 0));
                    }
                    if pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() {
                        let mut t = liked.clone();
                        super::shuffle(&mut t);
                        app.actions.push(Action::Play(t, 0));
                    }
                    select_pill(ui, &mut app.selection, &app.page);
                });
                ui.add_space(10.0);
            }
            list(ui, &rows, &app.liked, liked, true, plain, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
            if more {
                widgets::skeleton_rows(ui, now, 3);
            }
        }
        _ => {
            if app.library.is_none() {
                placeholder(app, ui, now, 2, CARD);
                return;
            }
            let Some(lib) = &app.library else { return };
            match tab {
                LibTab::Playlists => {
                    if pill(ui, "New playlist", None, false).clicked() {
                        app.actions.push(Action::NewPlaylist(Vec::new()));
                    }
                    ui.add_space(14.0);
                    grid(ui, &lib.playlists, &mut app.actions)
                }
                LibTab::Albums => grid(ui, &lib.albums, &mut app.actions),
                LibTab::Songs | LibTab::History | LibTab::Uploads => unreachable!(),
            }
        }
    }
}

// ------------------------------------------------------------------ downloads

fn downloads(app: &mut App, ui: &mut Ui, now: f64) {
    if crate::platform::phone() {
        return downloads_phone(app, ui, now);
    }
    root_title(ui, "Downloads");
    let songs = crate::downloads::list();
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!("{} songs \u{2022} {}", songs.len(), crate::downloads::folder().display()))
            .font(theme::regular(13.0))
            .color(theme::text_dim()),
    );
    ui.add_space(12.0);
    if songs.is_empty() {
        ui.label(RichText::new("No downloads yet").font(theme::semibold(16.0)).color(theme::text()));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Right-click a song and choose Download to keep it on this computer, playable offline.")
                .font(theme::regular(13.0))
                .color(theme::text_dim()),
        );
        ui.add_space(10.0);
        if pill(ui, "Open folder", Some(icon::FOLDER_OPEN), false).clicked() {
            crate::downloads::open_folder();
        }
        return;
    }
    ui.horizontal(|ui| {
        if pill(ui, "Play", Some(icon::PLAY), true).clicked() {
            app.actions.push(Action::Play(songs.clone(), 0));
        }
        if pill(ui, "Shuffle", Some(icon::SHUFFLE), false).clicked() {
            let mut t = songs.clone();
            super::shuffle(&mut t);
            app.actions.push(Action::Play(t, 0));
        }
        if pill(ui, "Open folder", Some(icon::FOLDER_OPEN), false).clicked() {
            crate::downloads::open_folder();
        }
    });
    ui.add_space(12.0);
    let rows = Rows::new(app, now);
    list(ui, &rows, &app.liked, &songs, true, |_, _| None, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
}

/// A group's heading on the phone's pages, with a quiet line under it when it has one.
pub(super) fn phone_section(ui: &mut Ui, title: &str, note: &str) {
    use crate::material::Type;
    let m = crate::material::scheme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    m3::text(ui, pos2(rect.left(), rect.center().y - 12.0), title, Type::TitleMedium.spec().weight(crate::material::Weight::Bold), m.on_surface, rect.width());
    if !note.is_empty() {
        let g = m3::galley_wrapped(ui, note, Type::BodyMedium, m.on_surface_variant, ui.available_width(), 4);
        ui.label(g);
        ui.add_space(8.0);
    }
    ui.add_space(4.0);
}

/// The phone's Downloads: how much room the downloads take and where they go, the
/// Wi-Fi switch, what is on its way, the music folders, then what is downloaded.
fn downloads_phone(app: &mut App, ui: &mut Ui, now: f64) {
    let songs = crate::downloads::list();
    ui.add_space(4.0);
    storage_card(app, ui, songs.len());
    ui.add_space(8.0);
    super::downloading::wifi_row(app, ui);
    if super::downloading::any_on_their_way(app) {
        ui.add_space(8.0);
        phone_section(ui, "Downloading", "");
        super::downloading::on_their_way(app, ui);
    }
    ui.add_space(12.0);
    phone_section(ui, "Music on this phone", "Only the folders you add are read. Ringtones and other sounds stay out.");
    local_tabs(app, ui);
    match app.local_tab {
        LocalTab::Folders => folder_list(app, ui),
        LocalTab::Albums => album_list(app, ui),
        LocalTab::Songs => song_preview(app, ui),
    }
    ui.add_space(20.0);
    phone_section(ui, "Downloaded", "");
    if songs.is_empty() {
        nothing_saved(ui);
        return;
    }
    // The albums and playlists downloaded whole, then every saved song.
    super::downloading::saved_collections(app, ui);
    ui.add_space(4.0);
    play_shuffle(app, ui, &songs);
    ui.add_space(8.0);
    let rows = Rows::new(app, now);
    list(ui, &rows, &app.liked, &songs, true, |_, _| None, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
}

/// Play and Shuffle side by side: the main action filled, the other tonal.
fn play_shuffle(app: &mut App, ui: &mut Ui, songs: &[Track]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (label, glyph, primary) in [("Play", icon::PLAY, true), ("Shuffle", icon::SHUFFLE, false)] {
            if pill(ui, label, Some(glyph), primary).clicked() {
                let mut t = songs.to_vec();
                if !primary {
                    super::shuffle(&mut t);
                }
                app.actions.push(Action::Play(t, 0));
            }
        }
    });
}

/// Where the storage card's parts are: the size the downloads take, the count at its
/// right, the line under them, the bar of the phone's room (when the phone tells it),
/// and where downloads go, with the button that changes it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StorageGeom {
    size: Pos2,
    count_right: Pos2,
    line: Pos2,
    bar: Option<Rect>,
    place: Rect,
    change: Rect,
}

/// How tall the storage card is: with the bar when the phone tells its room (`disk`),
/// and with a line tall enough for a finger when the folder can be changed.
fn storage_height(disk: bool, change: bool) -> f32 {
    20.0 + 44.0 + 4.0 + 20.0 + if disk { 20.0 } else { 0.0 } + 8.0 + if change { m3::TOUCH } else { 24.0 } + if change { 8.0 } else { 16.0 }
}

fn storage_geom(card: Rect, disk: bool, change: bool) -> StorageGeom {
    const PAD: f32 = 24.0;
    let (left, right) = (card.left() + PAD, card.right() - PAD);
    let top = card.top() + 20.0;
    let bar = disk.then(|| Rect::from_min_size(pos2(left, top + 44.0 + 4.0 + 20.0 + 12.0), vec2(right - left, 8.0)));
    let foot = bar.map_or(top + 44.0 + 4.0 + 20.0, |b| b.bottom()) + 8.0;
    let line = if change { m3::TOUCH } else { 24.0 };
    let change = Rect::from_min_max(pos2(right - 72.0 + 12.0, foot), pos2(right + 12.0, foot + m3::TOUCH));
    StorageGeom {
        size: pos2(left, top),
        count_right: pos2(right, top + 22.0),
        line: pos2(left, top + 44.0 + 4.0),
        bar,
        place: Rect::from_min_max(pos2(left, foot), pos2(change.left() - 8.0, foot + line)),
        change,
    }
}

/// What the downloads take up and the room left on the phone where the system tells
/// it, and where downloads go, with a way to change it.
fn storage_card(app: &mut App, ui: &mut Ui, count: usize) {
    use crate::material::{Type, shape};
    let m = crate::material::scheme();
    let used = crate::downloads::total_bytes();
    let disk = crate::downloads::disk_space(&crate::downloads::folder());
    let change = crate::platform::HAS_SAF;
    let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), storage_height(disk.is_some(), change)), Sense::hover());
    let g = storage_geom(card, disk.is_some(), change);
    super::m3::plate(ui, card, super::m3::round(shape::CARD), m.surface_container_high, super::m3::Stack::Card);
    let songs = m3::galley(ui, &count_text(count, "song"), Type::LabelLarge, m.primary, 140.0);
    let size_w = g.count_right.x - songs.size().x - 12.0 - g.size.x;
    m3::text(ui, g.size, &crate::downloads::human_size(used), Type::DisplaySmall, m.on_surface, size_w);
    ui.painter().galley(pos2(g.count_right.x - songs.size().x, g.count_right.y - songs.size().y / 2.0), songs, m.primary);
    let line_w = card.right() - 24.0 - g.line.x;
    let line = match disk {
        Some((free, _)) => {
            let free = format!("{} free on this phone", crate::downloads::human_size(free));
            let whole = format!("used by downloads \u{00b7} {free}");
            // A narrow phone has no room for both halves: the size above says whose it is.
            if m3::galley(ui, &whole, Type::BodyMedium, m.on_surface_variant, f32::INFINITY).size().x <= line_w { whole } else { free }
        }
        None => "used by downloads".to_owned(),
    };
    m3::text(ui, g.line, &line, Type::BodyMedium, m.on_surface_variant, line_w);
    if let (Some(bar), Some((free, total))) = (g.bar, disk) {
        super::downloading::disk_bar(ui, bar, used, free, total);
    }
    let place = match crate::downloads::tree() {
        Some((_, label)) => format!("Saved to {}", crate::local::folder_name(&label)),
        None if crate::platform::HAS_SAF => "Choose where to save".to_owned(),
        None => "Saved on this device".to_owned(),
    };
    let place_g = m3::galley(ui, &place, Type::BodyMedium, m.on_surface, g.place.width());
    ui.painter().galley(pos2(g.place.left(), g.place.center().y - place_g.size().y / 2.0), place_g, m.on_surface);
    if change {
        let resp = named(ui.interact(g.change, ui.scope_id().with("dl-change"), Sense::click()), "Change download folder");
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(g.change.shrink2(vec2(0.0, 4.0)), 20.0, m.primary.gamma_multiply(0.1));
        }
        let label = m3::galley(ui, "Change", Type::LabelLarge, m.primary, g.change.width());
        ui.painter().galley(g.change.center() - label.size() / 2.0, label, m.primary);
        if resp.clicked() {
            app.ask_folder(super::storage::Purpose::Downloads);
        }
    }
}

/// Which of the three views of the music on this phone is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalTab {
    Folders,
    Albums,
    Songs,
}

/// Folder / Album / Song as a row of connected buttons.
fn local_tabs(app: &mut App, ui: &mut Ui) {
    const TABS: [(LocalTab, &str); 3] = [(LocalTab::Folders, "Folder"), (LocalTab::Albums, "Album"), (LocalTab::Songs, "Song")];
    let chosen = TABS.iter().position(|(tab, _)| *tab == app.local_tab).unwrap_or(0);
    if let Some(i) = m3::segmented(ui, &TABS.map(|(_, label)| label), chosen) {
        app.local_tab = TABS[i].0;
    }
    ui.add_space(8.0);
}

/// A row that opens a list of local music: an icon on a tonal tile, a name and a line
/// under it, a chevron.
fn long_card(ui: &mut Ui, glyph: char, title: &str, sub: &str) -> egui::Response {
    m3::list_row(ui, &m3::ListRow { title, subtitle: sub, tile: Some(glyph.into()), chevron: true, ..Default::default() }).row
}

fn count_text(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// The folders as long cards, and a card to add another.
fn folder_list(app: &mut App, ui: &mut Ui) {
    for (i, f) in crate::local::folders().iter().enumerate() {
        let sub = if app.scanning(&f.uri) { "Looking for music\u{2026}".to_owned() } else { count_text(f.songs, "song") };
        if ui.push_id(("folder-card", i), |ui| long_card(ui, icon::FOLDER_OPEN, &f.name, &sub)).inner.clicked() {
            app.actions.push(Action::Go(Page::LocalFolder(f.uri.clone())));
        }
    }
    if crate::platform::HAS_SAF {
        ui.add_space(4.0);
        if pill(ui, "Add a music folder", None, false).clicked() {
            app.ask_folder(super::storage::Purpose::Music);
        }
    }
}

/// Albums across all the folders, as long cards.
fn album_list(app: &mut App, ui: &mut Ui) {
    let albums = crate::local::albums();
    if albums.is_empty() {
        ui.label(RichText::new("No albums yet. Songs whose files name an album are grouped here.").font(crate::material::Type::BodyMedium.font()).color(theme::text_dim()));
        return;
    }
    for (i, a) in albums.iter().enumerate() {
        let sub = if a.artist.is_empty() { count_text(a.songs, "song") } else { format!("{} \u{2022} {}", a.artist, count_text(a.songs, "song")) };
        if ui.push_id(("album-card", i), |ui| long_card(ui, icon::MUSIC, &a.name, &sub)).inner.clicked() {
            app.actions.push(Action::Go(Page::LocalAlbum(a.key.clone())));
        }
    }
}

/// A button to the whole list of songs, and how many there are.
fn song_preview(app: &mut App, ui: &mut Ui) {
    let n = crate::local::all_tracks().len();
    if n == 0 {
        ui.label(RichText::new("No songs yet. Add a folder first.").font(crate::material::Type::BodyMedium.font()).color(theme::text_dim()));
        return;
    }
    if long_card(ui, icon::MUSIC, "All songs", &format!("{} from every folder", count_text(n, "song"))).clicked() {
        app.actions.push(Action::Go(Page::LocalSongs));
    }
}

/// A list of local songs under a heading: Play and Shuffle, then the songs.
pub(super) fn local_song_page(app: &mut App, ui: &mut Ui, title: &str, sub: &str, songs: Vec<Track>, now: f64) {
    use crate::material::Type;
    page_title(ui, title);
    ui.add_space(2.0);
    ui.label(RichText::new(sub).font(Type::BodyMedium.font()).color(theme::text_dim()));
    ui.add_space(12.0);
    if songs.is_empty() {
        ui.label(RichText::new("No music here.").font(Type::BodyLarge.font()).color(theme::text()));
        return;
    }
    play_shuffle(app, ui, &songs);
    ui.add_space(8.0);
    let rows = Rows::new(app, now);
    list(ui, &rows, &app.liked, &songs, true, |_, _| None, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
}

fn local_songs(app: &mut App, ui: &mut Ui, now: f64) {
    let songs = crate::local::all_tracks();
    let sub = format!("{} from every folder", count_text(songs.len(), "song"));
    local_song_page(app, ui, "All songs", &sub, songs, now);
}

fn local_album(app: &mut App, ui: &mut Ui, key: &str, now: f64) {
    let info = crate::local::albums().into_iter().find(|a| a.key == key);
    let songs = crate::local::album_tracks(key);
    let (name, sub) = match &info {
        Some(a) => (a.name.clone(), if a.artist.is_empty() { count_text(a.songs, "song") } else { format!("{} \u{2022} {}", a.artist, count_text(a.songs, "song")) }),
        None => ("Album".to_owned(), String::new()),
    };
    local_song_page(app, ui, &name, &sub, songs, now);
}

/// No downloads yet: a mark and a friendly line, instead of an empty list.
fn nothing_saved(ui: &mut Ui) {
    use crate::material::Type;
    let m = crate::material::scheme();
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        let (art, _) = ui.allocate_exact_size(Vec2::splat(72.0), Sense::hover());
        ui.painter().circle_filled(art.center(), 36.0, m.secondary_container);
        widgets::icon_at(ui, art.center(), icon::DOWNLOAD, 30.0, m.on_secondary_container);
        ui.add_space(12.0);
        ui.label(RichText::new("Nothing saved yet").font(Type::TitleMedium.font()).color(m.on_surface));
        ui.add_space(4.0);
        ui.label(RichText::new("Open a song's menu (the three dots) and choose Download. Saved songs play without internet.").font(Type::BodyMedium.font()).color(m.on_surface_variant));
    });
}

/// One folder of local music: its songs, and ways to keep it up to date.
fn local_folder(app: &mut App, ui: &mut Ui, uri: &str, now: f64) {
    use crate::material::Type;
    let Some(name) = crate::local::name_of(uri) else {
        page_title(ui, "Folder");
        ui.label(RichText::new("This folder isn't added any more.").font(Type::BodyMedium.font()).color(theme::text_dim()));
        return;
    };
    page_title(ui, &name);
    let songs = crate::local::tracks(uri);
    let looking = app.scanning(uri);
    ui.add_space(2.0);
    let sub = if looking { "Looking for music\u{2026}".to_owned() } else { format!("{} song{}", songs.len(), if songs.len() == 1 { "" } else { "s" }) };
    ui.label(RichText::new(sub).font(Type::BodyMedium.font()).color(theme::text_dim()));
    ui.add_space(12.0);
    if !songs.is_empty() {
        play_shuffle(app, ui, &songs);
        ui.add_space(4.0);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if !looking && pill(ui, "Refresh", None, false).clicked() {
            app.scan_folder(uri);
        }
        if pill(ui, "Remove folder", Some(icon::X), false).clicked() {
            crate::local::remove_folder(uri);
            app.toast(format!("{name} removed from Aoto. Its files stay on your phone."));
            app.nav_back();
        }
    });
    ui.add_space(8.0);
    if songs.is_empty() {
        if !looking {
            ui.label(RichText::new("No music found in this folder.").font(Type::BodyLarge.font()).color(theme::text()));
        }
        return;
    }
    let rows = Rows::new(app, now);
    list(ui, &rows, &app.liked, &songs, true, |_, _| None, ListCtx { sel: &mut app.selection, page: &app.page, own: false }, &mut app.actions);
}

// ------------------------------------------------------------------ settings

/// What the box above Settings is looking for: the words typed in it, the group being
/// drawn and whether any of the page was left standing.
#[derive(Clone, Default)]
struct SettingsSearch {
    /// Lowercased words; a row is in when every one of them is in it.
    words: Vec<String>,
    /// The heading of the group whose rows are being drawn.
    section: String,
    /// Draw the group's heading and card at all.
    shown: bool,
    /// Draw every row of the group: nothing is being looked for, or its heading matched.
    all: bool,
    /// Some group survived, so the page isn't empty.
    matched: bool,
}

/// Every row Settings drew while the box was empty — the list the box searches, since a
/// filtered page no longer draws the rows it hides.
#[derive(Clone, Default)]
struct SettingsRows(Vec<(String, String, String)>);

fn search_key() -> egui::Id {
    egui::Id::new("aoto-settings-search")
}

fn rows_key() -> egui::Id {
    egui::Id::new("aoto-settings-rows")
}

/// The heading `section_label` was given on the Material phone, kept for the card that
/// follows: the two are drawn as one, and only when the card holds a row.
#[derive(Clone, Default)]
struct PendingLabel(Option<(String, Option<String>)>);

fn label_key() -> egui::Id {
    egui::Id::new("aoto-settings-label")
}

fn search_of(ui: &Ui) -> SettingsSearch {
    ui.data(|d| d.get_temp::<SettingsSearch>(search_key())).unwrap_or_default()
}

/// The heading of the group whose rows are being drawn.
pub(super) fn settings_section(ui: &Ui) -> String {
    search_of(ui).section
}

fn put_search(ui: &Ui, s: &SettingsSearch) {
    ui.data_mut(|d| d.insert_temp(search_key(), s.clone()));
}

/// Every word has to be in the text, in any order.
pub(super) fn settings_hit(text: &str, words: &[String]) -> bool {
    let text = text.to_lowercase();
    words.iter().all(|w| text.contains(w))
}

/// Whether the group being drawn survives the box at all — for the few notes Settings
/// draws that aren't rows.
fn settings_group_shown(ui: &Ui) -> bool {
    let s = search_of(ui);
    s.words.is_empty() || s.shown
}

/// Called by every row of Settings, `setting_row` and the hand-drawn ones alike: whether
/// to draw it, and what the box has to search through while the page is drawn whole.
pub(super) fn settings_row_shown(ui: &mut Ui, title: &str, desc: &str) -> bool {
    let s = search_of(ui);
    // (The phone's categories are searched as they are drawn, and keep no list.)
    if s.words.is_empty() && m3::on() {
        return true;
    }
    if s.words.is_empty() {
        let section = s.section.clone();
        ui.data_mut(|d| {
            let mut rows = d.get_temp::<SettingsRows>(rows_key()).unwrap_or_default();
            rows.0.push((section, title.to_owned(), desc.to_owned()));
            d.insert_temp(rows_key(), rows);
        });
        return true;
    }
    s.all || settings_hit(&format!("{title} {desc}"), &s.words)
}

fn settings(app: &mut App, ui: &mut Ui) {
    if m3::on() {
        return super::settings::first_page(app, ui);
    }
    let words: Vec<String> = app.settings_search.split_whitespace().map(str::to_lowercase).collect();
    // The page drawn whole is what the box searches; a filtered one leaves that list alone.
    if words.is_empty() {
        ui.data_mut(|d| d.insert_temp(rows_key(), SettingsRows::default()));
    }
    put_search(ui, &SettingsSearch { words, ..Default::default() });
    // One centred column; every section shares its width.
    let avail = ui.available_width();
    let w = avail.min(920.0);
    ui.horizontal(|ui| {
        ui.add_space(((avail - w) / 2.0).max(0.0));
        ui.vertical(|ui| {
            ui.set_width(w);
            settings_body(app, ui);
            let s = search_of(ui);
            if !s.words.is_empty() && !s.matched {
                ui.add_space(14.0);
                ui.label(egui::RichText::new(format!("Nothing in Settings matches “{}”.", app.settings_search.trim())).color(theme::text_dim()));
            }
            ui.add_space(40.0);
        });
    });
    // Only Settings is filtered; the rest of the app draws every row it has.
    put_search(ui, &SettingsSearch::default());
}

/// The phone's Settings look for what is typed among the rows of every category.
/// True when something is typed: then only the rows that say it are drawn.
pub(super) fn begin_settings_search(ui: &Ui, query: &str) -> bool {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let searching = !words.is_empty();
    put_search(ui, &SettingsSearch { words, ..Default::default() });
    searching
}

/// The phone's Settings are drawn: the rest of the app draws every row it has.
pub(super) fn end_settings_search(ui: &Ui) {
    put_search(ui, &SettingsSearch::default());
}

/// The heading of the next group of a searched Settings: a row of it is in when the
/// words are all in the row, or all in this heading.
pub(super) fn search_group(ui: &Ui, title: &str) {
    let mut s = search_of(ui);
    s.shown = true;
    s.all = settings_hit(title, &s.words);
    s.section = title.to_owned();
    put_search(ui, &s);
    ui.data_mut(|d| d.insert_temp(label_key(), PendingLabel(Some((title.to_owned(), None)))));
}

fn settings_body(app: &mut App, ui: &mut Ui) {
    // A section opened from Account takes the column to itself, the way the site's
    // settings drill in.
    if app.jam_open {
        if super::jam::subpage(app, ui) {
            app.jam_open = false;
        }
        ui.add_space(40.0);
        return;
    }
    page_title(ui, "Settings");
    settings_search_bar(ui, &mut app.settings_search);
    settings_account(app, ui);
    settings_playback(app, ui);
    settings_downloads(app, ui);
    super::stats::settings_section(app, ui);
    settings_lyrics(app, ui);
    settings_artists(app, ui);
    settings_appearance(app, ui);
    // A phone has no window to arrange, no proxy of its own to set (its VPN and Wi-Fi
    // settings do that for every app) and no keyboard to bind.
    let desk = !crate::platform::phone();
    if desk {
        settings_window(app, ui);
    }
    settings_connections(app, ui);
    if desk {
        section_label(ui, "Network", None);
        settings_card(ui, |ui| network(app, ui));
    }
    super::transfer::settings_section(app, ui);
    if desk {
        settings_shortcuts(app, ui);
    }
}

/// The box above Settings: what's typed in it leaves only the sections and rows that say
/// it, and the page keeps its shape while it does.
fn settings_search_bar(ui: &mut Ui, query: &mut String) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 40.0), Sense::hover());
    ui.painter().rect_filled(rect, 20.0, theme::glass(0.09));
    widgets::icon_at(ui, pos2(rect.left() + 20.0, rect.center().y), icon::SEARCH, 16.0, theme::text_dim());
    let clearable = !query.is_empty();
    let inner = Rect::from_min_max(pos2(rect.left() + 40.0, rect.top() + 4.0), pos2(rect.right() - if clearable { 40.0 } else { 16.0 }, rect.bottom() - 4.0));
    ui.put(
        inner,
        egui::TextEdit::singleline(query)
            .hint_text(egui::RichText::new("Search settings").color(theme::text_faint()))
            .font(theme::regular(14.0))
            .vertical_align(egui::Align::Center)
            .frame(egui::Frame::NONE)
            .desired_width(inner.width()),
    );
    if clearable {
        let x = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(26.0));
        let resp = ui.interact(x, ui.id().with("settings-search-clear"), Sense::click());
        widgets::icon_at(ui, x.center(), icon::X, 14.0, theme::text_dim());
        if resp.clicked() {
            query.clear();
        }
    }
    ui.add_space(4.0);
}

/// Asked again now and then while Settings shows: a failed ask, or a channel made since.
pub(super) fn ask_channels(app: &mut App, ui: &Ui) {
    let now = ui.input(|i| i.time);
    if app.logged_in && app.channels_asked.is_none_or(|at| now - at > 60.0) {
        app.channels_asked = Some(now);
        app.backend.request(crate::backend::Req::Channels);
    }
}

/// Is there a channel to choose? Also with one, when the one in use isn't it (a brand
/// channel since deleted).
fn has_channels(app: &App) -> bool {
    app.logged_in && app.channels.as_ref().is_some_and(|c| c.list.len() > 1 || !c.list.iter().any(|l| l.page_id == c.current))
}

/// A newer version that can be had: Android's, which the phone downloads itself.
fn update_offer(app: &App) -> Option<(String, String)> {
    match super::dev_var("YTM_FAKE_UPDATE") {
        Some(v) => Some((v.to_owned(), crate::update::RELEASES.to_owned())),
        None => app.updater.as_ref().and_then(|u| match u.state() {
            crate::update::State::Available { version, url } => Some((version, url)),
            _ => None,
        }),
    }
}

/// Open a web page in the browser: a release's APK, which it downloads (opening the
/// download installs it over this one), or a project's page.
fn open_update(ui: &Ui, url: &str) {
    #[cfg(target_os = "android")]
    {
        let _ = ui;
        crate::platform::open_url(url);
    }
    #[cfg(not(target_os = "android"))]
    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
}

/// A value with a button either side to step it down or up (named `names`).
/// Returns the step asked for (-1, 0 or 1).
fn step_buttons(ui: &mut Ui, text: &str, less: bool, more: bool, names: (&str, &str)) -> i32 {
    if m3::on() {
        return m3::stepper(ui, text, less, more, names);
    }
    // The row's control slot fills from the right.
    let mut step = 0;
    if ui.add_enabled(more, egui::Button::new("+").min_size(vec2(28.0, 26.0))).on_hover_text(names.1).clicked() {
        step = 1;
    }
    ui.label(egui::RichText::new(text).font(theme::semibold(13.0)));
    if ui.add_enabled(less, egui::Button::new("−").min_size(vec2(28.0, 26.0))).on_hover_text(names.0).clicked() {
        step = -1;
    }
    step
}

/// One of Settings' rows: its text, its control and what the control does. The desktop's
/// one page and the phone's category pages are both made of these.
pub(super) fn settings_row(app: &mut App, ui: &mut Ui, row: Row) {
    match row {
        Row::Account => {
            let busy = app.signing_in.load(Ordering::Relaxed);
            let (mut sign_in, mut sign_out) = (false, false);
            let shown = if app.logged_in {
                let name = app.account_name.clone().unwrap_or_else(|| "YouTube Music".to_owned());
                let desc = format!("Signed in with Google. Your home feed, library and likes come from this account. {}", crate::platform::KEPT);
                setting_row(ui, &name, Some(&desc), |ui| {
                    sign_out = pill(ui, "Sign out", Some(icon::LOG_OUT), false).clicked();
                })
            } else {
                setting_row(ui, "Not signed in", Some("Sign in to get your home feed, library and likes."), |ui| {
                    if busy {
                        ui.spinner();
                    } else {
                        sign_in = pill(ui, "Sign in with Google", Some(icon::CIRCLE_USER), true).clicked();
                    }
                })
            };
            // The desktop says this under the card.
            if busy && shown && m3::on() {
                ui.label(RichText::new("Finish signing in on the page that opened.").font(crate::material::Type::BodySmall.font()).color(theme::text_dim()));
                ui.add_space(12.0);
            }
            if sign_in {
                app.start_sign_in(ui.ctx());
            }
            if sign_out {
                app.backend.request(crate::backend::Req::Logout);
            }
        }
        Row::Channel => {
            if has_channels(app) {
                setting_row(ui, "Channel", Some("Your Google account and each YouTube channel it manages have their own library, likes, history and recommendations."), |ui| {
                    channel_picker(app, ui);
                });
            }
        }
        // Listen together opens as its own section, under the account it borrows its name from.
        Row::ListenTogether => {
            if open_row(ui, "Listen together", &format!("Play the same music at the same moment, on up to {} devices.", crate::jam::DEFAULT_MEMBERS)) {
                app.jam_open = true;
            }
        }
        Row::ListeningStats => super::stats::stats_row(app, ui),
        Row::ClearHistory => super::stats::clear_row(app, ui),
        Row::PlaybackSpeed if m3::on() => super::speed::settings_buttons(app, ui),
        Row::PlaybackSpeed => {
            setting_row(ui, "Playback speed", Some(super::speed::settings_note(app)), |ui| super::speed::settings_pills(app, ui));
        }
        Row::KeepPitch => {
            setting_row(ui, "Keep the pitch", Some("Voices and instruments stay in tune at any speed. Off, the pitch rises and falls with the speed."), |ui| {
                if toggle(ui, &mut app.keep_pitch, "Keep the pitch").changed() {
                    app.set_speed();
                }
            });
        }
        Row::Gapless => {
            setting_row(ui, "Gapless playback", Some("No silence between the songs of an album: each starts on the last one's final sample. With Automix or Crossfade on, songs blend that way instead."), |ui| {
                if toggle(ui, &mut app.gapless, "Gapless playback").changed() {
                    app.set_gapless();
                }
            });
        }
        Row::Autoplay => {
            setting_row(ui, "Autoplay", Some("Keep playing similar songs when the queue ends."), |ui| {
                toggle(ui, &mut app.autoplay, "Autoplay");
            });
        }
        Row::Normalize => {
            setting_row(ui, "Volume normalization", Some("Turn loud songs down to the same level, like YouTube Music does."), |ui| {
                if toggle(ui, &mut app.normalize, "Volume normalization").changed() {
                    app.audio.set_normalize(app.normalize);
                }
            });
        }
        Row::LogVolume => {
            setting_row(ui, "Logarithmic volume", Some("Space the volume slider out the way ears hear loudness, so quiet levels aren't squeezed into its first few pixels."), |ui| {
                if toggle(ui, &mut app.log_volume, "Logarithmic volume").changed() {
                    app.audio.set_log_volume(app.log_volume);
                }
            });
        }
        Row::PlaySongs => {
            setting_row(ui, "Play songs, not music videos", Some("An official music video plays as its song, the album recording, when YouTube Music has one: no intros, skits or video-only edits. Lists still show what they hold."), |ui| {
                toggle(ui, &mut app.play_songs, "Play songs, not music videos");
            });
        }
        Row::Equalizer => {
            if open_row(ui, "Equalizer", &super::equalizer::settings_note(&app.eq)) {
                app.actions.push(Action::Go(Page::Equalizer));
            }
        }
        Row::SpatialAudio => {
            setting_row(ui, "Spatial audio", Some("Makes stereo songs sound like they surround you, for headphones: a wider stage, a little room around it and the way your head shades each ear. Voices stay in the middle. Not head-tracked, and not Dolby Atmos (that plays as it is when a song has it)."), |ui| {
                if toggle(ui, &mut app.spatial, "Spatial audio").changed() {
                    app.set_crossfade();
                }
            });
        }
        Row::SpatialStrength => {
            let mut pick = None;
            setting_row(ui, "Spatial audio strength", Some("How wide the image is made. Strong is the widest and the easiest to hear; Light is gentler, for listeners who find it too much."), |ui| {
                if m3::on() {
                    let labels = crate::spatial::Level::ALL.map(|l| l.label());
                    let chosen = crate::spatial::Level::ALL.iter().position(|l| *l == app.spatial_level).unwrap_or(0);
                    pick = m3::segmented(ui, &labels, chosen).map(|i| crate::spatial::Level::ALL[i]);
                    return;
                }
                let btn = dropdown_button(ui, app.spatial_level.label(), 160.0, "Spatial audio strength");
                egui::Popup::menu(&btn).show(|ui| {
                    ui.set_width(160.0);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(4.0);
                    for l in crate::spatial::Level::ALL {
                        if option_row(ui, l.label(), l == app.spatial_level).clicked() {
                            pick = Some(l);
                            ui.close();
                        }
                    }
                    ui.add_space(4.0);
                });
            });
            if let Some(l) = pick {
                app.spatial_level = l;
                app.set_crossfade();
            }
        }
        Row::Automix => {
            setting_row(ui, "Automix", Some("Slide from one song into the next the way a DJ would: both hold their level for a moment, then swap in one smooth move, instead of a straight fade. The mix picks its own length, in phrases of 8 or 16 bars (16 on long songs), following the beat it hears."), |ui| {
                if toggle(ui, &mut app.automix, "Automix").changed() {
                    app.set_crossfade();
                }
            });
        }
        // Automix mixes in its own way: the plain fade is for when it is off.
        Row::Crossfade => {
            if !app.automix {
                setting_row(ui, "Crossfade songs", Some("Blend the end of each song into the start of the next."), |ui| {
                    if toggle(ui, &mut app.crossfade, "Crossfade songs").changed() {
                        app.set_crossfade();
                    }
                });
            }
        }
        Row::CrossfadeLength => {
            if !app.automix && app.crossfade {
                // The phone's slider has no room for its value beside it: the row says it.
                let said = format!("{} s", app.crossfade_secs);
                setting_row(ui, "Crossfade length", m3::on().then_some(said.as_str()), |ui| {
                    if seconds_slider(ui, &mut app.crossfade_secs, 1..=super::CROSSFADE_MAX, "Crossfade length").changed() {
                        app.set_crossfade();
                    }
                });
            }
        }
        Row::SkipNonMusic => {
            setting_row(ui, "Skip non-music parts", Some("Jump over talking, skits and sponsor breaks in music videos, as marked by the SponsorBlock community. Album songs don't have any."), |ui| {
                if toggle(ui, &mut app.sponsorblock, "Skip non-music parts").changed() {
                    app.request_segments();
                }
            });
        }
        Row::HiRes => {
            let shown = setting_row(ui, "Hi-Res audio", Some("Addon servers that serve hi-res copies of songs. When one has the song, its copy plays instead of YouTube's, and its quality shows in the player."), |_| {});
            // The desktop's one page keeps the list under its row whatever is searched.
            if shown || !m3::on() {
                ui.add_space(12.0);
                hires_sources_editor(app, ui);
                ui.add_space(10.0);
            }
        }
        Row::QualityWifi => quality_row(app, ui, true),
        Row::QualityMobile => quality_row(app, ui, false),
        Row::Lossless => {
            let desc = "When an addon's lossless copy (FLAC and the like) may play instead of YouTube's. Those files are several times larger, so Wi-Fi only keeps mobile data for YouTube's stream. Off leaves the addons in the list, unused.";
            let mut pick = None;
            setting_row(ui, "Lossless from addons", Some(desc), |ui| {
                if m3::on() {
                    let labels = Lossless::ALL.map(|l| l.label());
                    let chosen = Lossless::ALL.iter().position(|l| *l == app.lossless).unwrap_or(0);
                    pick = m3::segmented(ui, &labels, chosen).map(|i| Lossless::ALL[i]);
                    return;
                }
                let btn = dropdown_button(ui, app.lossless.label(), 160.0, "Lossless from addons");
                egui::Popup::menu(&btn).show(|ui| {
                    ui.set_width(160.0);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(4.0);
                    for l in Lossless::ALL {
                        if option_row(ui, l.label(), l == app.lossless).clicked() {
                            pick = Some(l);
                            ui.close();
                        }
                    }
                    ui.add_space(4.0);
                });
            });
            if let Some(l) = pick {
                app.lossless = l;
                app.backend.set_lossless(l);
            }
        }
        Row::PreferAtmos => {
            setting_row(ui, "Prefer Dolby Atmos", Some("Ask the hi-res sources for a Dolby Atmos copy when they offer one. Falls back to the best stereo copy when the stream's codec can't be decoded."), |ui| {
                if toggle(ui, &mut app.prefer_atmos, "Prefer Dolby Atmos").changed() {
                    app.backend.set_prefer_atmos(app.prefer_atmos);
                }
            });
        }
        Row::DisableDirect => {
            setting_row(ui, "Disable direct source stream", Some("Play YouTube audio immediately and upgrade to hi-res in the background, instead of waiting for the addon to answer first."), |ui| {
                if toggle(ui, &mut app.disable_direct_stream, "Disable direct source stream").changed() {
                    app.backend.set_disable_direct_stream(app.disable_direct_stream);
                }
            });
        }
        Row::LosslessDownloads => {
            setting_row(ui, "Save lossless when an addon has it", Some("When a hi-res addon has a song as FLAC, a download saves that file, with the song's tags and cover, instead of YouTube's audio. FLAC files are 4 to 8 times larger, so mind your storage. Songs no addon has are saved as before."), |ui| {
                if toggle(ui, &mut app.lossless_downloads, "Save lossless when an addon has it").changed() {
                    app.backend.set_lossless_downloads(app.lossless_downloads);
                }
            });
        }
        Row::LyricsInFiles => {
            setting_row(ui, "Save lyrics in the file", Some("A download carries the song's lyrics in its tag: timed ones when a source has them (as LRC, which players that show synced lyrics read), else plain text. The download waits a few seconds at most for them."), |ui| {
                if toggle(ui, &mut app.lyrics_in_files, "Save lyrics in the file").changed() {
                    app.backend.set_lyrics_in_files(app.lyrics_in_files);
                }
            });
        }
        Row::SyncPlaylists => {
            setting_row(ui, "Keep downloaded playlists up to date", Some("Every six hours and when the app opens, a playlist you downloaded whole is checked, and the songs added to it since are downloaded too (on Wi-Fi only, if that is on). A song you took off the phone is not fetched again, and a song taken out of the playlist keeps its file until you remove it."), |ui| {
                toggle(ui, &mut app.sync_playlists, "Keep downloaded playlists up to date");
            });
        }
        Row::DownloadFolder => {
            if crate::platform::HAS_SAF {
                // Android: a folder chosen once; every download is copied into it.
                let desc = match crate::downloads::tree() {
                    Some((_, label)) => format!("Downloads are saved to {}. Change it to save new ones elsewhere.", crate::local::folder_name(&label)),
                    None => "Not chosen yet: you're asked at your first download, once.".to_owned(),
                };
                setting_row(ui, "Download folder", Some(&desc), |ui| {
                    if pill(ui, "Change", Some(icon::FOLDER_OPEN), false).clicked() {
                        app.ask_folder(super::storage::Purpose::Downloads);
                    }
                });
                return;
            }
            let folder = crate::downloads::folder().display().to_string();
            setting_row(ui, "Download folder", Some(&folder), |ui| {
                if crate::platform::HAS_FILE_DIALOG {
                    if pill(ui, "Change", None, false).clicked() {
                        crate::downloads::pick_folder(ui.ctx());
                    }
                    ui.add_space(6.0);
                }
                if pill(ui, "Open", Some(icon::FOLDER_OPEN), false).clicked() {
                    crate::downloads::open_folder();
                }
            });
        }
        Row::Romanization => {
            setting_row(ui, "Romanization", Some("Japanese, Korean, Chinese, Cyrillic and other scripts in Latin letters. Timed with the lyrics when the source has it."), |ui| {
                toggle(ui, &mut app.show_roman, "Romanization");
            });
        }
        Row::Translation => {
            setting_row(ui, "Translation", Some("Translated by Unison, or Google when it can't. Lyrics that ship their own translation use it."), |ui| {
                toggle(ui, &mut app.show_trans, "Translation");
            });
        }
        Row::TranslateInto => {
            setting_row(ui, "Translate into", None, |ui| {
                let current = super::lyric_prefs::LANGUAGES.iter().find(|(c, _)| *c == app.lyric_prefs.target).map_or(app.lyric_prefs.target.as_str(), |(_, n)| n);
                let btn = dropdown_button(ui, current, 200.0, "Translate into");
                let just_opened = btn.clicked();
                egui::Popup::menu(&btn).show(|ui| {
                    ui.set_width(200.0);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        ui.add_space(4.0);
                        for (code, name) in super::lyric_prefs::LANGUAGES {
                            let on = app.lyric_prefs.target == *code;
                            let row = option_row(ui, name, on);
                            if on && just_opened {
                                row.scroll_to_me(Some(egui::Align::Center));
                            }
                            if row.clicked() {
                                app.lyric_prefs.target = (*code).to_owned();
                                ui.close();
                            }
                        }
                        ui.add_space(4.0);
                    });
                });
            });
        }
        Row::TextSize => {
            setting_row(ui, "Text size", Some("Make the lyrics larger or smaller. Works on top of the theme's own size."), |ui| {
                let (min, max, step) = super::lyric_prefs::TEXT_SCALE;
                let v = &mut app.lyric_prefs.text_scale;
                let by = step_buttons(ui, &format!("{:.0}%", *v * 100.0), *v > min + 0.01, *v < max - 0.01, ("Smaller", "Larger"));
                if by != 0 {
                    *v = ((*v + by as f32 * step) * 10.0).round() / 10.0;
                }
            });
        }
        // Offsets: seconds taken off the lyrics clock, as the extension's dock sets them.
        Row::TimingOffset | Row::WordTrim | Row::LineTrim => {
            let (title, desc) = match row {
                Row::TimingOffset => ("Timing offset", "Shift all lyrics later (+) or earlier (−)."),
                Row::WordTrim => ("Word-synced trim", "Extra shift for syllable and word-synced lyrics."),
                _ => ("Line-synced trim", "Extra shift for line-synced lyrics."),
            };
            setting_row(ui, title, Some(desc), |ui| {
                let v = match row {
                    Row::TimingOffset => &mut app.lyric_prefs.global_offset,
                    Row::WordTrim => &mut app.lyric_prefs.word_trim,
                    _ => &mut app.lyric_prefs.line_trim,
                };
                let by = step_buttons(ui, &format!("{:+.1} s", *v).replace("+0.0", "0.0"), true, true, ("Earlier", "Later"));
                if by != 0 {
                    *v = ((*v + by as f32 * 0.1) * 10.0).round() / 10.0;
                }
            });
        }
        // Source order: the first enabled source with lyrics for a song is shown; the
        // lyrics toolbar can pick another for that song.
        Row::LyricsSources => {
            let shown = setting_row(ui, "Lyrics sources", Some("Tried in this order. Switch one off to skip it; the toolbar under the lyrics picks a different one for a single song."), |_| {});
            if shown || !m3::on() {
                lyrics_sources(app, ui);
            }
        }
        Row::Mode => {
            let desc = if cfg!(windows) {
                "Light or dark pages. System follows Windows' app mode. Now playing stays dark, over the album art."
            } else {
                "Light or dark pages. System follows your system's setting. Now playing stays dark, over the album art."
            };
            let mut pick = None;
            setting_row(ui, "Mode", Some(desc), |ui| {
                if m3::on() {
                    let labels = theme::Appearance::ALL.map(|a| a.label());
                    let chosen = theme::Appearance::ALL.iter().position(|a| *a == app.appearance).unwrap_or(0);
                    pick = m3::segmented(ui, &labels, chosen).map(|i| theme::Appearance::ALL[i]);
                    return;
                }
                let btn = dropdown_button(ui, app.appearance.label(), 160.0, "Mode");
                egui::Popup::menu(&btn).show(|ui| {
                    ui.set_width(160.0);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(4.0);
                    for a in theme::Appearance::ALL {
                        if option_row(ui, a.label(), a == app.appearance).clicked() {
                            pick = Some(a);
                            ui.close();
                        }
                    }
                    ui.add_space(4.0);
                });
            });
            if let Some(a) = pick {
                app.appearance = a;
            }
        }
        Row::Look => {
            use crate::material::Look;
            let mut pick = None;
            setting_row(ui, "Theme", Some("Indigo is Aoto's own look. Sticker draws outlined cards with a second card behind them, in soft pastels."), |ui| {
                if m3::on() {
                    let labels = Look::ALL.map(|l| l.label());
                    let chosen = Look::ALL.iter().position(|l| *l == app.look).unwrap_or(0);
                    pick = m3::segmented(ui, &labels, chosen).map(|i| Look::ALL[i]);
                    return;
                }
                let btn = dropdown_button(ui, app.look.label(), 160.0, "Theme");
                egui::Popup::menu(&btn).show(|ui| {
                    ui.set_width(160.0);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.add_space(4.0);
                    for l in Look::ALL {
                        if option_row(ui, l.label(), l == app.look).clicked() {
                            pick = Some(l);
                            ui.close();
                        }
                    }
                    ui.add_space(4.0);
                });
            });
            if let Some(l) = pick {
                app.look = l;
                app.apply_look(ui.ctx());
            }
        }
        Row::AppIcon => {
            let mut pick = None;
            setting_row(ui, "App icon", Some("The icon Aoto wears on the home screen. It changes the next time you close Aoto (swipe it away from your recent apps), so the music is never cut off."), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                for icon in &crate::launcher_icon::ALL {
                    if m3::app_icon_tile(ui, icon, std::ptr::eq(icon, app.launcher_icon)) {
                        pick = Some(icon);
                    }
                }
            });
            if let Some(icon) = pick {
                app.launcher_icon = icon;
            }
        }
        // Sticker's dark is its own near-black: there is nothing for this switch to change.
        Row::PureBlack if app.look == crate::material::Look::Sticker => {}
        Row::PureBlack => {
            setting_row(ui, "Pure black", Some("In dark mode, a true black background instead of dark grey. Saves battery on OLED screens. Cards and sheets stay dark grey so they can be told apart."), |ui| {
                if toggle(ui, &mut app.pure_black, "Pure black").changed() {
                    app.apply_pure_black(ui.ctx());
                }
            });
        }
        Row::AnimatedBackground => {
            setting_row(ui, "Animated background", Some("The album art slowly drifts behind everything."), |ui| {
                toggle(ui, &mut app.anim_bg, "Animated background");
            });
        }
        Row::PulseWithMusic => {
            setting_row(ui, "Pulse with the music", Some("The background swells with the beat."), |ui| {
                toggle(ui, &mut app.reactive_bg, "Pulse with the music");
            });
        }
        Row::AnimatedArtwork => {
            setting_row(ui, "Animated artwork", Some("Apple's moving album covers in Now Playing, the player and the tiles you hover. Costs memory while a song plays, and gives it back when the song ends."), |ui| {
                if toggle(ui, &mut app.animated_artwork, "Animated artwork").changed() && !app.animated_artwork {
                    app.deload_motion();
                }
            });
        }
        Row::StatsForNerds => {
            setting_row(ui, "Stats for nerds", Some("Show the codec, sample rate and bitrate of what's playing at the foot of the cover in Now Playing, whatever the source."), |ui| {
                toggle(ui, &mut app.show_nerd_stats, "Stats for nerds");
            });
        }
        Row::SignatureService => {
            let shown = setting_row(ui, "Signature service", Some("A link to your own signature API. Artists get their handwritten signature, with “Signed by …” above it — beside About when Apple also letters them a wordmark; without a signature, their name is set in type."), |_| {});
            if shown || !m3::on() {
                divider(ui);
                ui.add_space(12.0);
                if super::text_field(ui, "Link", &mut app.signature_api, false, "https://your-signature-service.example").changed() {
                    app.signature_reset();
                }
                ui.add_space(12.0);
            }
        }
        Row::LastFm => {
            let busy = app.lastfm_busy.load(Ordering::Relaxed);
            let desc = match &app.lastfm_user {
                Some(name) => format!("Scrobbling what you play to {name}'s profile."),
                None if busy => "Allow Aoto on the Last.fm page that opened in your browser.".to_owned(),
                None => "Scrobble what you play to your Last.fm profile.".to_owned(),
            };
            let (mut connect, mut disconnect) = (false, false);
            setting_row(ui, "Last.fm", Some(&desc), |ui| {
                if app.lastfm_user.is_some() {
                    disconnect = pill(ui, "Disconnect", None, false).clicked();
                } else if busy {
                    ui.spinner();
                } else {
                    connect = pill(ui, "Connect", None, false).clicked();
                }
            });
            if connect {
                app.lastfm_sign_in();
            }
            if disconnect {
                app.lastfm_sign_out();
            }
        }
        Row::Thanks => {
            setting_row(ui, "Built on BitChord", Some("Aoto comes from BitChord and its Rust rewrite, BitChord PC Official. Thank you to both."), |ui| {
                if pill(ui, "Open", None, false).clicked() {
                    open_update(ui, "https://github.com/kushagrasinghx/BitChord");
                }
            });
        }
        Row::TransferFromSpotify => super::transfer::open_row(app, ui),
        Row::SpotifyAccount => super::transfer::account_row(app, ui),
        Row::Version => {
            setting_row(ui, "Aoto", Some(&format!("Version {}", crate::update::current_version())), |_| {});
        }
        // Where the app can update itself: the check it runs by itself, on demand.
        Row::Updates => {
            if !super::updatable() {
                return;
            }
            use crate::update::State;
            let offer = update_offer(app);
            let state = app.updater.as_ref().map(|u| u.state());
            let checking = matches!(state, Some(State::Checking));
            let desc = match (&offer, &state) {
                (Some((version, _)), _) => format!("Aoto {version} is available. Download it, then open it to install."),
                (None, Some(State::Checking)) => "Looking for a newer version…".to_owned(),
                (None, Some(State::UpToDate)) => "This is the newest version.".to_owned(),
                (None, Some(State::Failed(e))) => format!("Couldn't look for a newer version: {e}"),
                (None, _) => "Aoto looks for a newer version by itself, now and then.".to_owned(),
            };
            let (mut check, mut download) = (false, false);
            setting_row(ui, "Updates", Some(&desc), |ui| {
                if offer.is_some() {
                    download = pill(ui, "Download", Some(icon::DOWNLOAD), true).clicked();
                } else if checking {
                    ui.spinner();
                } else {
                    check = pill(ui, "Check now", None, false).clicked();
                }
            });
            if check && let Some(u) = &app.updater {
                u.check();
            }
            if download && let Some((_, url)) = &offer {
                open_update(ui, url);
            }
        }
    }
}

/// The lyric sources under their row: each can be switched off, and moved up or down.
fn lyrics_sources(app: &mut App, ui: &mut Ui) {
    let material = m3::on();
    let n = app.lyric_prefs.order.len();
    let mut swap: Option<(usize, usize)> = None;
    for i in 0..n {
        let (p, on) = app.lyric_prefs.order[i];
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), if material { 56.0 } else { 34.0 }), Sense::hover());
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let mut enabled = on;
        if toggle(&mut row, &mut enabled, p.name()).changed() {
            app.lyric_prefs.order[i].1 = enabled;
        }
        row.add_space(10.0);
        let kind = match p.sync() {
            crate::lyrics::SyncKind::Syllable => "syllable",
            crate::lyrics::SyncKind::Word => "word",
            crate::lyrics::SyncKind::Line => "line",
            crate::lyrics::SyncKind::Unsynced => "plain",
        };
        if material {
            use crate::material::Type;
            let m = crate::material::scheme();
            let (down, up) = (pos2(rect.right() - m3::TOUCH / 2.0, rect.center().y), pos2(rect.right() - m3::TOUCH * 1.5, rect.center().y));
            let left = row.cursor().left();
            m3::two_lines(ui, left, rect.center().y, up.x - m3::TOUCH / 2.0 - 4.0 - left, p.name(), Type::BodyLarge, if on { m.on_surface } else { m.on_surface_variant }, kind, Type::BodySmall, m.on_surface_variant);
            let id = ui.scope_id().with(("lyric-source", i));
            if i > 0 && m3::IconButton::new(icon::CHEVRON_UP, &format!("Move {} higher", p.name())).show_at(ui, up, id.with("up")).clicked() {
                swap = Some((i, i - 1));
            }
            if i + 1 < n && m3::IconButton::new(icon::CHEVRON_DOWN, &format!("Move {} lower", p.name())).show_at(ui, down, id.with("down")).clicked() {
                swap = Some((i, i + 1));
            }
            continue;
        }
        row.label(egui::RichText::new(p.name()).font(theme::semibold(13.5)).color(if on { theme::text() } else { theme::text_dim() }));
        row.label(egui::RichText::new(kind).font(theme::regular(12.0)).color(theme::text_dim()));
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(i + 1 < n, egui::Button::new("↓").min_size(vec2(28.0, 24.0))).on_hover_text("Lower").clicked() {
                swap = Some((i, i + 1));
            }
            if ui.add_enabled(i > 0, egui::Button::new("↑").min_size(vec2(28.0, 24.0))).on_hover_text("Higher").clicked() {
                swap = Some((i, i - 1));
            }
        });
    }
    if let Some((a, b)) = swap {
        app.lyric_prefs.order.swap(a, b);
    }
    ui.add_space(6.0);
}

/// Settings › Account: signing in and out, and which channel to use.
fn settings_account(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Account", None);
    let busy = app.signing_in.load(Ordering::Relaxed);
    ask_channels(app, ui);
    settings_card(ui, |ui| {
        settings_row(app, ui, Row::Account);
        if has_channels(app) {
            divider(ui);
            settings_row(app, ui, Row::Channel);
        }
        divider(ui);
        settings_row(app, ui, Row::ListenTogether);
        super::stats::account_row(app, ui);
    });
    if busy {
        ui.add_space(6.0);
        ui.label(RichText::new(if crate::platform::phone() { "Finish signing in on the page that opened." } else { "Finish signing in in the window that opened." }).font(theme::regular(12.5)).color(theme::text_dim()));
    }
}

fn channel_picker(app: &mut App, ui: &mut Ui) {
    let Some(crate::backend::Channels { list, current, epoch }) = &app.channels else { return };
    // A brand channel often has the account's name: the handle, or "No channel" (as
    // YouTube's switcher says), tells them apart.
    let label = |c: &crate::ytm::Channel| match (&c.handle, c.has_channel) {
        (Some(h), _) => format!("{} ({h})", c.name),
        (None, false) => format!("{} (No channel)", c.name),
        (None, true) => c.name.clone(),
    };
    // The button is narrower: a handle alone is enough there.
    let shown = match list.iter().find(|c| c.page_id == *current) {
        Some(c) => c.handle.clone().unwrap_or_else(|| label(c)),
        None => "Another channel".to_owned(),
    };
    // A transfer would go on as the new channel.
    if app.transfer.running() {
        ui.label(RichText::new("Not while a transfer runs").font(theme::regular(13.0)).color(theme::text_dim()));
        return;
    }
    let switching = app.switching_to.is_some();
    let btn = dropdown_button(ui, if switching { "Switching…" } else { &shown }, 220.0, "Channel");
    let mut pick = None;
    egui::Popup::menu(&btn).show(|ui| {
        ui.set_width(300.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            ui.add_space(4.0);
            for c in list {
                if option_row(ui, &label(c), c.page_id == *current).clicked() {
                    pick = Some(c.clone());
                    ui.close();
                }
            }
            ui.add_space(4.0);
        });
    });
    if let Some(c) = pick.filter(|c| c.page_id != *current && !switching) {
        app.switching_to = Some(label(&c));
        app.backend.request(crate::backend::Req::SwitchChannel { page_id: c.page_id, epoch: *epoch });
    }
}

/// Settings › Playback.
fn settings_playback(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Playback", None);
    settings_card(ui, |ui| {
        settings_row(app, ui, Row::PlaybackSpeed);
        divider(ui);
        settings_row(app, ui, Row::KeepPitch);
        divider(ui);
        settings_row(app, ui, Row::Autoplay);
        divider(ui);
        settings_row(app, ui, Row::Normalize);
        divider(ui);
        if Row::LogVolume.shown(crate::platform::phone()) {
            settings_row(app, ui, Row::LogVolume);
            divider(ui);
        }
        settings_row(app, ui, Row::PlaySongs);
        divider(ui);
        // On a phone the system routes the sound: speaker, headphones or Bluetooth.
        if !crate::platform::phone() {
            let desc = if cfg!(windows) {
                "Where the music plays. A picked device that isn't connected leaves it to Windows' default until it's back."
            } else {
                "Where the music plays. A picked device that isn't connected leaves it to the system's default until it's back."
            };
            setting_row(ui, "Output device", Some(desc), |ui| {
                output_device_picker(app, ui);
            });
            divider(ui);
        }
        settings_row(app, ui, Row::Equalizer);
        divider(ui);
        settings_row(app, ui, Row::SpatialAudio);
        settings_row(app, ui, Row::SpatialStrength);
        divider(ui);
        settings_row(app, ui, Row::Gapless);
        divider(ui);
        settings_row(app, ui, Row::Automix);
        if !app.automix {
            divider(ui);
            settings_row(app, ui, Row::Crossfade);
            if app.crossfade {
                divider(ui);
                settings_row(app, ui, Row::CrossfadeLength);
            }
        }
        divider(ui);
        settings_row(app, ui, Row::SkipNonMusic);
        divider(ui);
        settings_row(app, ui, Row::QualityWifi);
        settings_row(app, ui, Row::QualityMobile);
        settings_row(app, ui, Row::HiRes);
        settings_row(app, ui, Row::Lossless);
        settings_row(app, ui, Row::PreferAtmos);
        settings_row(app, ui, Row::DisableDirect);
    });
}

/// The hi-res source list: one line per addon, and "+ Add a source" below it that
/// opens a fresh line for the next one. Each settled link is tested against the addon
/// and its verdict shown underneath, the way the old app did.
fn hires_sources_editor(app: &mut App, ui: &mut Ui) {
    let mut edited = false;
    let mut remove = None;
    for i in 0..app.hires_sources.len() {
        let link = app.hires_sources[i].trim().to_owned();
        // Asked once the field is settled: testing on every keystroke would fire a
        // search and a stream at the addon for each character typed.
        let verdict = if link.is_empty() { None } else { app.hires_check(&link).cloned() };
        // Fixed-height row so the × sits at the same height on every line, whatever
        // verdict text appears; the verdict goes below the row, not inside it.
        ui.allocate_ui_with_layout(vec2(ui.available_width(), if m3::on() { m3::TOUCH } else { 42.0 }), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // A finger needs a larger button than a pointer.
            let removed = if m3::on() { m3::IconButton::new(icon::X, "Remove source").show(ui).clicked() } else { widgets::icon_button(ui, icon::X, 14.0, "Remove source", false).clicked() };
            if removed {
                remove = Some(i);
            }
            ui.add_space(8.0);
            if super::text_field(ui, "", &mut app.hires_sources[i], false, "https://your-addon.example").changed() {
                edited = true;
            }
        });
        let text = match &verdict {
            Some(Ok(summary)) => Some((summary.clone(), theme::text_dim())),
            Some(Err(reason)) => Some((reason.clone(), theme::RED)),
            None if !link.is_empty() => Some(("Testing the link…".to_owned(), theme::text_faint())),
            None => None,
        };
        if let Some((text, colour)) = text {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(text).font(theme::regular(11.5)).color(colour));
        }
        ui.add_space(6.0);
    }
    if let Some(i) = remove {
        app.hires_sources.remove(i);
        edited = true;
    }
    if edited {
        app.hires_sources = super::dedupe_hires(app.hires_sources.iter().map(String::as_str));
        app.backend.set_hires_source(&live_sources(app));
    }
    if app.hires_adding {
        let resp = super::text_field(ui, "", &mut app.hires_draft, false, "https://your-addon.example");
        if app.hires_focus {
            resp.request_focus();
            app.hires_focus = false;
        }
        // Enter commits without needing to click away; clicking elsewhere does too.
        let enter = resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if resp.lost_focus() || enter {
            let link = app.hires_draft.trim().to_owned();
            if !link.is_empty() {
                app.hires_sources.push(link);
                app.hires_sources = super::dedupe_hires(app.hires_sources.iter().map(String::as_str));
                app.backend.set_hires_source(&live_sources(app));
            }
            app.hires_draft.clear();
            app.hires_adding = false;
        }
        ui.add_space(6.0);
    } else if pill(ui, "+  Add a source", None, false).clicked() {
        app.hires_adding = true;
        app.hires_focus = true;
        app.hires_draft.clear();
    }
}

/// The quality YouTube's stream is played at, on Wi-Fi or on mobile data.
fn quality_row(app: &mut App, ui: &mut Ui, wifi: bool) {
    use crate::quality::Quality;
    let (title, desc) = if wifi {
        ("Quality on Wi-Fi", "How good YouTube's stream is when you are on Wi-Fi. High is the best your account gets (Opus, about 130 to 160 kbps; with Premium, AAC 256 kbps). Downloads are always the best.")
    } else {
        ("Quality on mobile data", "How good YouTube's stream is on mobile data. Normal is AAC 128 kbps, about 1 MB a minute; Low is about 48 kbps, a third of that. Downloads are always the best.")
    };
    let current = if wifi { app.quality.wifi } else { app.quality.mobile };
    let mut pick = None;
    setting_row(ui, title, Some(desc), |ui| {
        if m3::on() {
            let labels = Quality::ALL.map(|q| q.label());
            let chosen = Quality::ALL.iter().position(|q| *q == current).unwrap_or(0);
            pick = m3::segmented(ui, &labels, chosen).map(|i| Quality::ALL[i]);
            return;
        }
        let btn = dropdown_button(ui, current.label(), 160.0, title);
        egui::Popup::menu(&btn).show(|ui| {
            ui.set_width(160.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.add_space(4.0);
            for q in Quality::ALL {
                if option_row(ui, q.label(), q == current).clicked() {
                    pick = Some(q);
                    ui.close();
                }
            }
            ui.add_space(4.0);
        });
    });
    if let Some(q) = pick {
        if wifi {
            app.quality.wifi = q;
        } else {
            app.quality.mobile = q;
        }
        app.backend.set_quality(app.quality);
    }
}

/// The non-empty links as one newline-separated list, the way the backend reads them.
fn live_sources(app: &App) -> String {
    app.hires_sources.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

/// What Settings › Downloads says above its card.
pub(super) const DOWNLOADS_NOTE: &str = "Download songs from their menu. They're saved at the best quality your account gets, tagged with their cover art, and play from the file, even offline.";

/// Settings › Downloads.
fn settings_downloads(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Downloads", None);
    if settings_group_shown(ui) {
        ui.label(RichText::new(DOWNLOADS_NOTE).font(theme::regular(13.0)).color(theme::text_dim()));
        ui.add_space(12.0);
    }
    settings_card(ui, |ui| {
        settings_row(app, ui, Row::LosslessDownloads);
        divider(ui);
        settings_row(app, ui, Row::LyricsInFiles);
        divider(ui);
        settings_row(app, ui, Row::SyncPlaylists);
        divider(ui);
        settings_row(app, ui, Row::DownloadFolder);
    });
}

/// Settings › Lyrics: the better-lyrics extension's options.
fn settings_lyrics(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Lyrics", None);
    settings_card(ui, |ui| {
        for row in [Row::Romanization, Row::Translation, Row::TranslateInto, Row::TextSize, Row::TimingOffset, Row::WordTrim, Row::LineTrim] {
            settings_row(app, ui, row);
            divider(ui);
        }
        settings_row(app, ui, Row::LyricsSources);
    });
}

/// Settings › Artist pages: the listener's own signature service.
fn settings_artists(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Artist pages", None);
    settings_card(ui, |ui| settings_row(app, ui, Row::SignatureService));
}

/// Settings › Appearance.
fn settings_appearance(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Appearance", None);
    settings_card(ui, |ui| {
        for row in [Row::Mode, Row::AnimatedBackground, Row::PulseWithMusic, Row::AnimatedArtwork] {
            settings_row(app, ui, row);
            divider(ui);
        }
        // A phone draws in its system font, as its other apps do, and has no sidebar to
        // hide playlists from.
        if !crate::platform::phone() {
            let desc = if cfg!(windows) {
                "For the whole app and the lyrics. Any font installed on this PC works, e.g. one made for your language."
            } else {
                "For the whole app and the lyrics. Any font installed on this computer works, e.g. one made for your language."
            };
            setting_row(ui, "Font", Some(desc), |ui| {
                if let Some(name) = font_picker(ui) {
                    theme::set_font_choice(&name);
                    app.chrome_applied = None;
                }
            });
            divider(ui);
            let hidden = app.hidden_playlists.len();
            let desc = match hidden {
                0 => "Nothing hidden. Right-click a playlist in the sidebar to hide it from there.".to_owned(),
                1 => "1 playlist is hidden from the sidebar. Right-click a playlist there to hide or show it.".to_owned(),
                n => format!("{} playlists are hidden from the sidebar. Right-click a playlist there to hide or show it.", thousands(n as u64)),
            };
            setting_row(ui, "Hidden playlists", Some(&desc), |ui| {
                if hidden > 0 && pill(ui, "Show all", None, false).clicked() {
                    app.hidden_playlists.clear();
                }
            });
            divider(ui);
        }
        settings_row(app, ui, Row::StatsForNerds);
    });
}

/// Settings › Window.
fn settings_window(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Window", None);
    settings_card(ui, |ui| {
        if crate::platform::HAS_TRAY {
            setting_row(ui, "Close to tray", Some("Clicking × hides the window and keeps the music playing. Click the tray icon to bring it back, or right-click it to quit."), |ui| {
                toggle(ui, &mut app.close_to_tray, "Close to tray");
            });
            divider(ui);
        }
        setting_row(ui, "Collapse sidebar", Some("Show only icons in the sidebar, for narrow windows. The menu button at its top does the same."), |ui| {
            toggle(ui, &mut app.sidebar_collapsed, "Collapse sidebar");
        });
        divider(ui);
        let api = crate::renderer::api_name();
        let title = format!("Draw with {api}");
        let desc = format!(
            "Use {api} (wgpu) instead of OpenGL. It isn't faster for this app and takes about three times the memory; try it if OpenGL draws wrong on this PC. If {api} won't start, the app goes back to OpenGL."
        );
        setting_row(ui, &title, Some(&desc), |ui| {
            if toggle(ui, &mut app.wgpu_renderer, &title).changed() {
                use crate::renderer::Renderer;
                let r = if app.wgpu_renderer { Renderer::Wgpu } else { Renderer::OpenGl };
                if let Err(e) = crate::renderer::save(app.backend.data_dir(), r) {
                    log::warn!("renderer: couldn't save the setting: {e}");
                }
            }
        });
        if app.wgpu_renderer != (crate::renderer::active() == crate::renderer::Renderer::Wgpu) {
            divider(ui);
            let mut restart = false;
            setting_row(ui, "Restart to apply", Some("The app draws with the new renderer from the next start."), |ui| {
                restart = pill(ui, "Restart now", None, true).clicked();
            });
            if restart {
                app.restart(ui.ctx());
            }
        }
    });
}

/// Settings › Connections.
fn settings_connections(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Connections", None);
    settings_card(ui, |ui| {
        settings_row(app, ui, Row::LastFm);
    });
}

/// A one-line text field in a settings row's control slot.
pub(super) fn settings_field(ui: &mut Ui, text: &mut String, hint: &str, limit: usize) {
    let (rect, _) = ui.allocate_exact_size(vec2(200.0, 36.0), Sense::hover());
    ui.painter().rect_filled(rect, 10.0, theme::glass(0.08));
    let inner = rect.shrink2(vec2(12.0, 4.0));
    ui.put(
        inner,
        egui::TextEdit::singleline(text)
            .hint_text(egui::RichText::new(hint).color(theme::text_faint()))
            .font(theme::regular(14.0))
            .char_limit(limit)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .desired_width(inner.width()),
    );
}

/// Settings › Keyboard shortcuts. The bindings the app answers to, and the way to change
/// them: click a row and press a key. `⌘` is the Mac's command key and `Ctrl` everyone
/// else's, so the same row means the right thing on each machine.
fn settings_shortcuts(app: &mut App, ui: &mut Ui) {
    section_label(ui, "Keyboard shortcuts", None);
    let armed = app.keys_edit;
    // The recorder listens only while a row waits for a key.
    let typed = if armed.is_some() { super::keys::typed(ui.ctx()) } else { super::keys::Typed::Nothing };
    settings_card(ui, |ui| {
        for (i, info) in super::keys::INFOS.iter().enumerate() {
            let caps = app.keys.get(info.cmd).caps();
            if !settings_row_shown(ui, info.label, &caps.join(" ")) {
                continue;
            }
            if i > 0 {
                divider(ui);
            }
            let waiting = armed == Some(info.cmd);
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
            let resp = named(resp, info.label)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if app.keys.is_default(info.cmd) { "Click it, then press the keys" } else { "Click it, then press the keys · right-click for the default" });
            ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, info.label, theme::regular(14.0), theme::text());
            if waiting {
                let accent = app.accent_color();
                ui.painter().rect_stroke(rect.shrink(2.0), 8.0, egui::Stroke::new(1.5, accent), egui::StrokeKind::Inside);
                ui.painter().text(pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER, "Press a key…", theme::regular(13.5), accent);
            } else {
                let mut x = rect.right();
                for (j, k) in caps.iter().enumerate().rev() {
                    x = keycap(ui, pos2(x, rect.center().y), k) - 6.0;
                    if j > 0 {
                        let plus = ui.painter().text(pos2(x, rect.center().y), Align2::RIGHT_CENTER, "+", theme::regular(13.0), theme::text_dim());
                        x = plus.left() - 6.0;
                    }
                }
            }
            if resp.clicked() {
                app.keys_edit = if waiting { None } else { Some(info.cmd) };
                if !waiting {
                    // The keys are for the row now, not for a text box that still has them.
                    if let Some(id) = ui.ctx().memory(|m| m.focused()) {
                        ui.ctx().memory_mut(|m| m.surrender_focus(id));
                    }
                }
            }
            if resp.secondary_clicked() && !app.keys.is_default(info.cmd) {
                app.keys.reset(info.cmd);
            }
        }
    });
    let Some(cmd) = armed else { return };
    match typed {
        super::keys::Typed::Nothing => {}
        super::keys::Typed::Cancel => app.keys_edit = None,
        super::keys::Typed::Combo(combo) => {
            match app.keys.clash(cmd, combo) {
                Some(other) => app.toast(format!("{} already has those keys", other.info().label)),
                None => {
                    app.toast(format!("{} now uses {}", cmd.info().label, combo.caps().join(" + ")));
                    app.keys.set(cmd, combo);
                }
            }
            app.keys_edit = None;
        }
    }
}

/// Settings › Network: the proxy everything goes through, a connection test, and a
/// restart when the setting changed.
fn network(app: &mut App, ui: &mut Ui) {
    use crate::proxy::{self, Setting};
    let same = |a: &Setting, b: &Setting| std::mem::discriminant(a) == std::mem::discriminant(b);
    let mut pick = None;
    setting_row(ui, "Proxy", Some("Send everything Aoto fetches through a proxy: songs, lyrics, sign-in and Listen together."), |ui| {
        let btn = dropdown_button(ui, app.proxy_mode.label(), 220.0, "Proxy");
        egui::Popup::menu(&btn).show(|ui| {
            ui.set_width(220.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.add_space(4.0);
            for s in [Setting::System, Setting::Off, Setting::Custom(String::new())] {
                if option_row(ui, s.label(), same(&s, &app.proxy_mode)).clicked() {
                    pick = Some(s);
                    ui.close();
                }
            }
            ui.add_space(4.0);
        });
    });
    if let Some(s) = pick {
        app.proxy_error = None;
        *app.proxy_test.lock().unwrap() = None;
        app.proxy_mode = s.clone();
        // System and Off take effect as picked; a custom address once it's saved.
        if !matches!(s, Setting::Custom(_)) {
            match proxy::save(app.backend.data_dir(), &s) {
                Ok(()) => app.proxy_saved = s,
                Err(e) => app.proxy_error = Some(format!("{e:#}")),
            }
        }
    }
    match &app.proxy_mode {
        Setting::System => {
            divider(ui);
            let desc = match proxy::system() {
                Some(p) => format!("{SYSTEM} is set to use {}.", proxy::redact(&p)),
                None => format!("{SYSTEM} isn't set to use a proxy, so Aoto connects directly."),
            };
            setting_row(ui, if cfg!(windows) { "Windows proxy" } else { "System proxy" }, Some(&desc), |_| {});
        }
        Setting::Custom(_) => {
            divider(ui);
            ui.add_space(12.0);
            super::text_field(ui, "Address", &mut app.proxy_url, false, "http://127.0.0.1:8080 or socks5://user:pass@host:1080");
            ui.add_space(10.0);
            let custom = Setting::Custom(app.proxy_url.trim().to_owned());
            let saved = app.proxy_saved == custom;
            let mut save = false;
            ui.horizontal(|ui| {
                save = pill(ui, if saved { "Saved" } else { "Save" }, None, !saved).clicked() && !saved;
                ui.add_space(8.0);
                ui.label(RichText::new(if cfg!(windows) { "A password in the address is kept in Windows Credential Manager." } else { "A password in the address is kept in your system's keychain." }).font(theme::regular(12.5)).color(theme::text_dim()));
            });
            if save {
                match proxy::save(app.backend.data_dir(), &custom) {
                    Ok(()) => {
                        app.proxy_saved = custom;
                        app.proxy_error = None;
                    }
                    Err(e) => app.proxy_error = Some(format!("{e:#}")),
                }
            }
            ui.add_space(12.0);
        }
        Setting::Off => {}
    }
    if let Some(e) = &app.proxy_error {
        ui.label(RichText::new(e).font(theme::regular(12.5)).color(theme::RED));
        ui.add_space(8.0);
    }
    divider(ui);
    let result = app.proxy_test.lock().unwrap().clone();
    let desc = match &result {
        Some(Some((_, msg))) => msg.clone(),
        Some(None) => "Checking…".to_owned(),
        None => "Checks that YouTube Music answers through this setting.".to_owned(),
    };
    setting_row(ui, "Test connection", Some(&desc), |ui| {
        if matches!(result, Some(None)) {
            ui.spinner();
        } else if pill(ui, "Test", None, false).clicked() {
            let s = match &app.proxy_mode {
                Setting::Custom(_) => Setting::Custom(app.proxy_url.trim().to_owned()),
                other => other.clone(),
            };
            let (slot, b) = (app.proxy_test.clone(), app.backend.clone());
            *slot.lock().unwrap() = Some(None);
            app.backend.spawn(async move {
                let out = match proxy::test(&s).await {
                    Ok(t) => (true, format!("Working: YouTube Music answered in {} ms.", t.as_millis())),
                    Err(e) => (false, format!("Didn't connect: {e:#}")),
                };
                *slot.lock().unwrap() = Some(Some(out));
                b.wake();
            });
        }
    });
    if app.proxy_saved != *proxy::active() {
        divider(ui);
        let mut restart = false;
        setting_row(ui, "Restart to apply", Some("The new proxy setting is used from the next start."), |ui| {
            restart = pill(ui, "Restart now", None, true).clicked();
        });
        if restart {
            app.restart(ui.ctx());
        }
    }
}

/// Paint `v` into `rect`, from the ICO entry that fits its size in physical pixels.
pub(super) fn paint_app_icon(ui: &Ui, v: &crate::appicon::Variant, rect: Rect) {
    let px = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    let png = v.png(px);
    egui::Image::from_bytes(format!("bytes://app-icon/{}-{}.png", v.id, png.len()), png).paint_at(ui, rect);
}

/// The font setting: the theme's own, a bundled face, or one installed on this PC.
/// Returns the new choice ("" for the theme's).
fn font_picker(ui: &mut Ui) -> Option<String> {
    let current = theme::font_choice();
    let btn = dropdown_button(ui, if current.is_empty() { "Theme's font" } else { &current }, 220.0, "Font");
    let just_opened = btn.clicked();
    let mut picked = None;
    egui::Popup::menu(&btn).show(|ui| {
        ui.set_width(220.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            ui.add_space(4.0);
            let mut row = |ui: &mut Ui, label: &str, value: &str| {
                let on = current.eq_ignore_ascii_case(value);
                let r = option_row(ui, label, on);
                if on && just_opened {
                    r.scroll_to_me(Some(egui::Align::Center));
                }
                if r.clicked() {
                    picked = Some(value.to_owned());
                    ui.close();
                }
            };
            row(ui, "Theme's font", "");
            for name in theme::BUNDLED_FONTS {
                row(ui, name, name);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new(if cfg!(windows) { "Installed on this PC" } else { "Installed on this computer" }).font(theme::semibold(12.0)).color(theme::text_faint()));
            });
            ui.add_space(4.0);
            let ctx = ui.ctx().clone();
            match theme::installed_fonts(move || ctx.request_repaint()) {
                Some(fonts) => {
                    for f in fonts {
                        row(ui, &f.name, &f.name);
                    }
                }
                None => {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(RichText::new("Looking for fonts…").font(theme::regular(13.0)).color(theme::text_dim()));
                    });
                }
            }
            ui.add_space(4.0);
        });
    });
    picked.filter(|p| *p != current)
}

const SYSTEM_DEFAULT: &str = if cfg!(windows) { "Windows default" } else { "System default" };
/// What the settings call the system when they speak of its own setup.
const SYSTEM: &str = if cfg!(windows) { "Windows" } else { "The system" };

fn output_device_picker(app: &mut App, ui: &mut Ui) {
    let current = match &app.output_device {
        None => SYSTEM_DEFAULT.to_owned(),
        Some((id, name)) => match app.output_devices.iter().find(|d| d.id == *id) {
            Some(d) => d.name.clone(),
            None if app.output_devices.is_empty() => name.clone(),
            None => format!("{name} (not connected)"),
        },
    };
    let btn = dropdown_button(ui, &current, 210.0, "Output device");
    // Listed when opened, so a device that just connected shows up.
    if btn.clicked() || app.output_devices.is_empty() {
        app.output_devices = crate::audio::output_devices();
    }
    let mut pick: Option<Option<(String, String)>> = None;
    egui::Popup::menu(&btn).show(|ui| {
        ui.set_width(340.0);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            ui.add_space(4.0);
            if option_row(ui, SYSTEM_DEFAULT, app.output_device.is_none()).clicked() {
                pick = Some(None);
                ui.close();
            }
            for d in &app.output_devices {
                if option_row(ui, &d.name, app.output_device.as_ref().is_some_and(|(id, _)| *id == d.id)).clicked() {
                    pick = Some(Some((d.id.clone(), d.name.clone())));
                    ui.close();
                }
            }
            ui.add_space(4.0);
        });
    });
    if let Some(pick) = pick {
        app.audio.send(crate::audio::AudioCmd::SetDevice(pick.as_ref().map(|(id, _)| id.clone())));
        app.output_device = pick;
    }
}

/// A dropdown's closed state: the chosen value and a chevron on a glass pill.
fn dropdown_button(ui: &mut Ui, value: &str, width: f32, name: &str) -> egui::Response {
    if m3::on() {
        return m3::dropdown(ui, value, width, name);
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    let resp = widgets::named(resp, name).on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered() || open, 0.12);
    ui.painter().rect_filled(rect, 10.0, theme::glass(0.08 + 0.05 * h));
    widgets::text_at(ui, pos2(rect.left() + 14.0, rect.center().y - 9.0), value, theme::regular(14.0), theme::text(), rect.width() - 44.0);
    let glyph = if open { theme::icon::CHEVRON_UP } else { theme::icon::CHEVRON_DOWN };
    widgets::icon_at(ui, pos2(rect.right() - 18.0, rect.center().y), glyph, 15.0, theme::lerp_color(theme::text_dim(), theme::text(), h));
    resp
}

/// A dropdown option: hover glass, and a check on the chosen one.
fn option_row(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    // A finger needs a taller row than a pointer.
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), if m3::on() { m3::TOUCH } else { 34.0 }), Sense::click());
    let resp = widgets::named(resp, label).on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered(), 0.12);
    if h > 0.0 || selected {
        ui.painter().rect_filled(rect, 8.0, theme::glass(0.05 * selected as u8 as f32 + 0.08 * h));
    }
    let font = if selected { theme::semibold(14.0) } else { theme::regular(14.0) };
    widgets::text_at(ui, pos2(rect.left() + 12.0, rect.center().y - 9.0), label, font, theme::text(), rect.width() - 44.0);
    if selected {
        widgets::icon_at(ui, pos2(rect.right() - 16.0, rect.center().y), theme::icon::CHECK, 15.0, theme::text());
    }
    resp
}

/// 12345 -> "12,345".
pub(super) fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Small heading above a settings card, with an optional value on the right.
pub(super) fn section_label(ui: &mut Ui, title: &str, value: Option<&str>) {
    let mut s = search_of(ui);
    if !s.words.is_empty() {
        let on_name = settings_hit(title, &s.words);
        let any_row = ui
            .data(|d| d.get_temp::<SettingsRows>(rows_key()))
            .unwrap_or_default()
            .0
            .iter()
            .any(|(sec, row, desc)| sec == title && (on_name || settings_hit(&format!("{row} {desc}"), &s.words)));
        s.shown = on_name || any_row;
        s.all = on_name;
        s.matched |= s.shown;
    } else {
        s.shown = true;
        s.all = true;
    }
    s.section = title.to_owned();
    put_search(ui, &s);
    if !s.shown {
        return;
    }
    if m3::on() {
        ui.data_mut(|d| d.insert_temp(label_key(), PendingLabel(Some((title.to_owned(), value.map(str::to_owned))))));
        return;
    }
    ui.add_space(30.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 2.0, rect.center().y), Align2::LEFT_CENTER, title, theme::bold(17.0), theme::text());
    if let Some(v) = value {
        ui.painter().text(pos2(rect.right() - 2.0, rect.center().y), Align2::RIGHT_CENTER, v, theme::semibold(13.0), theme::text_dim());
    }
    ui.add_space(10.0);
}

pub(super) fn settings_card(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    if !settings_group_shown(ui) {
        return;
    }
    if m3::on() {
        let label = ui.data_mut(|d| d.remove_temp::<PendingLabel>(label_key())).and_then(|l| l.0);
        let label = label.as_ref().map(|(title, value)| m3::GroupLabel { title, value: value.as_deref() });
        let top = ui.cursor().top();
        m3::card(ui, label, crate::material::scheme().surface_container, body);
        if ui.cursor().top() > top {
            // A group survived the search box, so the page isn't empty.
            let mut s = search_of(ui);
            s.matched = true;
            put_search(ui, &s);
            ui.add_space(12.0);
        }
        return;
    }
    egui::Frame::new()
        .fill(theme::glass(0.06))
        .stroke(egui::Stroke::new(1.0, theme::glass(0.05)))
        .corner_radius(theme::panel_radius())
        .inner_margin(egui::Margin::symmetric(20, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
}

/// Where a phone's settings row puts its control.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    /// Under the text, on a line of its own: a dropdown, a button, a slider.
    Below,
    /// At the right of the text, as phone settings put a switch.
    Beside,
    /// The row is a note and has no control.
    Absent,
}

/// The widest control a phone row keeps beside its text: a switch, a spinner.
const BESIDE_MAX: f32 = 64.0;

/// The slot for a control that came out `width` wide the last time it was drawn; one
/// not drawn yet goes below, where anything fits.
fn phone_slot(width: Option<f32>) -> Slot {
    match width {
        None => Slot::Below,
        Some(w) if w < 1.0 => Slot::Absent,
        Some(w) if w <= BESIDE_MAX => Slot::Beside,
        Some(_) => Slot::Below,
    }
}

/// A settings row: title and description on the left, the control on the right. A phone
/// is too narrow for most controls beside the text: there only a switch stays at the
/// right, and the others go underneath, in a line tall enough for a thumb.
/// Returns whether the row was drawn (the search box hides the ones that don't say
/// what is typed).
pub(super) fn setting_row(ui: &mut Ui, title: &str, desc: Option<&str>, control: impl FnOnce(&mut Ui)) -> bool {
    if !settings_row_shown(ui, title, desc.unwrap_or("")) {
        return false;
    }
    if m3::on() {
        setting_row_material(ui, title, desc, control);
        return true;
    }
    let w = ui.available_width();
    // How wide this row's control was last frame decides where it goes in this one.
    let width_id = ui.make_persistent_id(egui::IdSalt::new(("setting-row-control", title)));
    let slot = crate::platform::phone().then(|| phone_slot(ui.data(|d| d.get_temp::<f32>(width_id))));
    let text_w = match slot {
        None => (w - 230.0).max(200.0),
        Some(Slot::Beside) => w - BESIDE_MAX - 12.0,
        Some(_) => w,
    };
    let title_g = widgets::galley(ui, title, theme::semibold(14.5), theme::text(), text_w);
    // A phone's narrow column needs more lines to say the same thing.
    let lines = if slot.is_some() { 8 } else { 3 };
    let desc_g = desc.map(|d| widgets::galley_wrapped(ui, d, theme::regular(12.5), theme::text_dim(), text_w, lines));
    let text_h = title_g.size().y + desc_g.as_ref().map_or(0.0, |g| g.size().y + 3.0);
    let below = slot == Some(Slot::Below);
    let ctl_h = if below { 48.0 } else { 0.0 };
    let h = match slot {
        Some(Slot::Below) => text_h + 20.0 + ctl_h,
        Some(Slot::Absent) => text_h + 26.0,
        _ => (text_h + 26.0).max(58.0),
    };
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
    let top = if below { rect.top() + 10.0 } else { rect.center().y - text_h / 2.0 };
    ui.painter().galley(pos2(rect.left(), top), title_g.clone(), theme::with_alpha(theme::text(), fade));
    if let Some(g) = desc_g {
        ui.painter().galley(pos2(rect.left(), top + title_g.size().y + 3.0), g, theme::with_alpha(theme::text_dim(), fade));
    }
    let (ctl, layout) = if below {
        (Rect::from_min_max(pos2(rect.left(), rect.bottom() - ctl_h), rect.max), egui::Layout::left_to_right(egui::Align::Center))
    } else {
        (Rect::from_min_max(pos2(rect.left() + text_w + if slot.is_some() { 12.0 } else { 20.0 }, rect.top()), rect.max), egui::Layout::right_to_left(egui::Align::Center))
    };
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(ctl).layout(layout));
    control(&mut cui);
    if let Some(slot) = slot {
        // A control in another slot than its width asks for (the first frame, or a
        // button that became a spinner): this frame is drawn again with it moved.
        let width = cui.min_rect().width();
        ui.data_mut(|d| d.insert_temp(width_id, width));
        if phone_slot(Some(width)) != slot {
            ui.ctx().request_discard("a settings row's control changed slot");
        }
    }
    true
}

/// The room above and below a Material settings row's text.
const ROW_PAD: f32 = 14.0;
/// The line a Material row gives a control that goes under its text.
const ROW_CONTROL: f32 = 56.0;

/// How tall a Material settings row is for text `text_h` tall, with its control in `slot`.
fn material_row_height(text_h: f32, slot: Slot) -> f32 {
    match slot {
        Slot::Below => ROW_PAD + text_h + 4.0 + ROW_CONTROL,
        Slot::Beside | Slot::Absent => (text_h + 2.0 * ROW_PAD).max(56.0),
    }
}

/// A settings row on the Material phone: the title over its description, a switch at
/// their right, any other control on a line of its own under them.
fn setting_row_material(ui: &mut Ui, title: &str, desc: Option<&str>, control: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    let width_id = ui.make_persistent_id(egui::IdSalt::new(("setting-row-control", title)));
    let slot = phone_slot(ui.data(|d| d.get_temp::<f32>(width_id)));
    let text_w = if slot == Slot::Beside { w - BESIDE_MAX - 12.0 } else { w };
    let (title_g, desc_g) = m3::row_text(ui, title, desc, text_w, 8);
    let text_h = title_g.size().y + desc_g.as_ref().map_or(0.0, |g| g.size().y);
    let below = slot == Slot::Below;
    let (rect, _) = ui.allocate_exact_size(vec2(w, material_row_height(text_h, slot)), Sense::hover());
    let fade = if ui.is_enabled() { 1.0 } else { 0.38 };
    let top = if below { rect.top() + ROW_PAD } else { rect.center().y - text_h / 2.0 };
    let title_h = title_g.size().y;
    ui.painter().galley(pos2(rect.left(), top), title_g, theme::with_alpha(theme::text(), fade));
    if let Some(g) = desc_g {
        ui.painter().galley(pos2(rect.left(), top + title_h), g, theme::with_alpha(theme::text_dim(), fade));
    }
    let (ctl, layout) = if below {
        (Rect::from_min_max(pos2(rect.left(), rect.bottom() - ROW_CONTROL), rect.max), egui::Layout::left_to_right(egui::Align::Center))
    } else {
        (Rect::from_min_max(pos2(rect.left() + text_w + 12.0, rect.top()), rect.max), egui::Layout::right_to_left(egui::Align::Center))
    };
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(ctl).layout(layout));
    control(&mut cui);
    let width = cui.min_rect().width();
    ui.data_mut(|d| d.insert_temp(width_id, width));
    if phone_slot(Some(width)) != slot {
        ui.ctx().request_discard("a settings row's control changed slot");
    }
}

/// A settings row that opens a section of its own: the same text as `setting_row`, but
/// the whole row is clickable and a chevron stands where the control would.
pub(super) fn open_row(ui: &mut Ui, title: &str, desc: &str) -> bool {
    if !settings_row_shown(ui, title, desc) {
        return false;
    }
    let w = ui.available_width();
    if m3::on() {
        let (title_g, desc_g) = m3::row_text(ui, title, Some(desc), w - 32.0, 8);
        let text_h = title_g.size().y + desc_g.as_ref().map_or(0.0, |g| g.size().y);
        let (rect, resp) = ui.allocate_exact_size(vec2(w, material_row_height(text_h, Slot::Absent)), Sense::click());
        let resp = named(resp, title);
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect.expand2(vec2(12.0, 0.0)), crate::material::shape::CARD_SMALL, theme::glass(0.1));
        }
        let top = rect.center().y - text_h / 2.0;
        let title_h = title_g.size().y;
        ui.painter().galley(pos2(rect.left(), top), title_g, theme::text());
        if let Some(g) = desc_g {
            ui.painter().galley(pos2(rect.left(), top + title_h), g, theme::text_dim());
        }
        widgets::icon_at(ui, pos2(rect.right() - 6.0, rect.center().y), icon::CHEVRON_RIGHT, 20.0, theme::text_dim());
        return resp.clicked();
    }
    // The chevron needs only a little room; a phone gives the text the rest.
    let text_w = if crate::platform::phone() { w - 32.0 } else { (w - 230.0).max(200.0) };
    let title_g = widgets::galley(ui, title, theme::semibold(14.5), theme::text(), text_w);
    let desc_g = widgets::galley_wrapped(ui, desc, theme::regular(12.5), theme::text_dim(), text_w, 3);
    let text_h = title_g.size().y + desc_g.size().y + 3.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, (text_h + 26.0).max(58.0)), Sense::click());
    let resp = named(resp, title).on_hover_cursor(egui::CursorIcon::PointingHand);
    let top = rect.center().y - text_h / 2.0;
    let title_h = title_g.size().y;
    ui.painter().galley(pos2(rect.left(), top), title_g, theme::text());
    ui.painter().galley(pos2(rect.left(), top + title_h + 3.0), desc_g, theme::text_dim());
    widgets::icon_at(ui, pos2(rect.right() - 8.0, rect.center().y), icon::CHEVRON_RIGHT, 16.0, theme::text_faint());
    resp.clicked()
}

pub(super) fn divider(ui: &mut Ui) {
    // Rows the box kept read as one block, so a filtered group draws no lines between them.
    // A Material card's rows stand apart by their spacing, without lines.
    let s = search_of(ui);
    if (!s.words.is_empty() && !s.all) || m3::on() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::glass(0.07));
}

/// The room a switch takes, for a place measured by hand.
pub(super) fn toggle_size() -> Vec2 {
    if crate::material::on() { super::m3::SWITCH_TARGET } else { vec2(44.0, 26.0) }
}

/// On/off switch tinted with the accent.
pub(super) fn toggle(ui: &mut Ui, on: &mut bool, name: &str) -> egui::Response {
    if crate::material::on() {
        return super::m3::switch(ui, on, name);
    }
    let (rect, mut resp) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), value, name));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = theme::anim_bool(ui.ctx(), resp.id.with("t"), value, 0.16);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered(), 0.12);
    let accent = ui.ctx().data(|d| d.get_temp::<Color32>(egui::Id::unique("accent"))).unwrap_or(theme::RED);
    let fade = if ui.is_enabled() { 1.0 } else { 0.4 };
    let off = theme::glass(0.16 + 0.06 * h);
    ui.painter().rect_filled(rect, 13.0, theme::with_alpha(theme::lerp_color(off, accent, t), fade));
    let knob = pos2(egui::lerp(rect.left() + 13.0..=rect.right() - 13.0, t), rect.center().y);
    ui.painter().circle_filled(knob + vec2(0.0, 1.0), 10.0, theme::shade(0.25 * fade));
    ui.painter().circle_filled(knob, 10.0, theme::with_alpha(Color32::WHITE, fade));
    resp
}

/// Whole seconds on a track like the volume slider's, with the ends labelled and the
/// value beside it: click or drag to set (scrolling over it scrolls the page).
fn seconds_slider(ui: &mut Ui, v: &mut u32, range: std::ops::RangeInclusive<u32>, name: &str) -> egui::Response {
    if m3::on() {
        return m3::slider(ui, v, range, name);
    }
    let (lo, hi) = (*range.start(), *range.end());
    let label = |ui: &mut Ui, text: String, w: f32, font: egui::FontId, color: Color32| {
        let (rect, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
        let g = widgets::galley(ui, &text, font, color, w);
        ui.painter().galley(pos2(rect.right() - g.size().x, rect.center().y - g.size().y / 2.0), g, color);
    };
    label(ui, format!("{v} s"), 34.0, theme::semibold(13.0), theme::text());
    ui.add_space(6.0);
    label(ui, format!("{hi} s"), 26.0, theme::regular(12.0), theme::text_dim());
    let (rect, mut resp) = ui.allocate_exact_size(vec2(170.0, 26.0), Sense::click_and_drag());
    label(ui, format!("{lo} s"), 22.0, theme::regular(12.0), theme::text_dim());
    let track = Rect::from_center_size(rect.center(), vec2(rect.width() - 16.0, 4.0));
    let old = *v;
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        *v = (lo as f32 + t * (hi - lo) as f32).round() as u32;
    }
    if *v != old {
        resp.mark_changed();
    }
    let value = *v;
    resp.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), value as f64, name));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let h = theme::anim_bool(ui.ctx(), resp.id.with("h"), resp.hovered() || resp.dragged(), 0.12);
    let accent = ui.ctx().data(|d| d.get_temp::<Color32>(egui::Id::unique("accent"))).unwrap_or(theme::RED);
    let t = (value - lo) as f32 / (hi - lo) as f32;
    let knob = pos2(egui::lerp(track.left()..=track.right(), t), track.center().y);
    ui.painter().rect_filled(track, 2.0, theme::glass(0.15));
    ui.painter().rect_filled(Rect::from_min_max(track.min, pos2(knob.x, track.max.y)), 2.0, accent);
    ui.painter().circle_filled(knob + vec2(0.0, 1.0), 7.0 + h, theme::shade(0.25));
    ui.painter().circle_filled(knob, 7.0 + h, Color32::WHITE);
    resp
}

/// Draw a key cap whose right edge is at `right`; returns its left edge.
fn keycap(ui: &Ui, right: egui::Pos2, key: &str) -> f32 {
    let g = widgets::galley(ui, key, theme::semibold(12.5), theme::text(), 200.0);
    let size = vec2((g.size().x + 18.0).max(30.0), 26.0);
    let rect = Rect::from_min_size(pos2(right.x - size.x, right.y - size.y / 2.0), size);
    ui.painter().rect_filled(rect, 7.0, theme::glass(0.08));
    ui.painter().rect_stroke(rect, 7.0, egui::Stroke::new(1.0, theme::glass(0.12)), egui::StrokeKind::Inside);
    ui.painter().rect_filled(Rect::from_min_max(pos2(rect.left() + 2.0, rect.bottom() - 2.0), pos2(rect.right() - 2.0, rect.bottom())), 2.0, theme::shade(0.25));
    ui.painter().galley(rect.center() - g.size() / 2.0, g, theme::text());
    rect.left()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selection_bar_takes_a_second_row_when_its_buttons_do_not_fit_one() {
        // The bar is the screen less 8 points each side.
        assert_eq!(selection_rows(344.0, 0), 1, "nothing picked: the count, select all and done");
        assert_eq!(selection_rows(344.0, 3), 2, "a 360-point phone");
        assert_eq!(selection_rows(344.0, 4), 2);
        assert_eq!(selection_rows(396.0, 3), 1, "a 412-point phone holds three actions");
        assert_eq!(selection_rows(396.0, 4), 2, "but not the fourth, of a playlist of your own");
        // Spread across the second row, each action has more than a finger's width.
        assert!((344.0 - 16.0) / 4.0 >= m3::TOUCH);
    }

    #[test]
    fn a_phone_gives_a_song_column_nearly_the_whole_screen() {
        assert_eq!(song_column(270.0, 380.0, true), 344.0);
        // Never wider than a screen narrower than a tile.
        assert_eq!(song_column(270.0, 250.0, true), 250.0);
    }

    fn card(target: Target) -> Card {
        Card { title: "A".into(), subtitle: "B".into(), thumbs: vec![], target, round: false, links: vec![] }
    }

    fn shelf(title: &str, kind: ShelfKind) -> crate::model::Shelf {
        crate::model::Shelf { title: title.into(), strapline: None, strap_thumb: None, kind }
    }

    #[test]
    fn the_hero_is_the_album_or_playlist_that_opens_the_first_shelf_of_cards() {
        let feed = [
            shelf("Quick picks", ShelfKind::Songs { tracks: vec![], rows: 4 }),
            shelf("Trending community playlists", ShelfKind::Cards(vec![card(Target::Playlist("PL1".into())), card(Target::Playlist("PL2".into()))])),
            shelf("New releases", ShelfKind::Cards(vec![card(Target::Album("AL1".into()))])),
        ];
        assert_eq!(hero_pick(&feed), Some((1, 0)));
    }

    #[test]
    fn a_shelf_that_opens_with_an_artist_does_not_give_the_hero() {
        let feed = [
            shelf("Artists for you", ShelfKind::Cards(vec![card(Target::Artist("UC1".into())), card(Target::Album("AL0".into()))])),
            shelf("Mixed for you", ShelfKind::Cards(vec![card(Target::Album("AL1".into()))])),
        ];
        assert_eq!(hero_pick(&feed), Some((1, 0)));
    }

    #[test]
    fn a_feed_with_nothing_to_lead_with_has_no_hero() {
        assert_eq!(hero_pick(&[]), None);
        let feed = [
            shelf("Quick picks", ShelfKind::Songs { tracks: vec![], rows: 4 }),
            shelf("Artists for you", ShelfKind::Cards(vec![card(Target::Artist("UC1".into()))])),
            shelf("Empty", ShelfKind::Cards(vec![])),
        ];
        assert_eq!(hero_pick(&feed), None);
    }

    fn results(tracks: usize, artists: usize, albums: usize, playlists: usize) -> crate::backend::SearchResults {
        let track = |i: usize| Track { id: format!("v{i}"), title: format!("Song {i}"), artists: vec![], album: None, duration: None, thumbs: vec![], track_nr: None, plays: None, music_video: false };
        let cards = |n: usize, target: fn(String) -> Target| (0..n).map(|i| card(target(format!("id{i}")))).collect::<Vec<_>>();
        crate::backend::SearchResults { query: "senja".into(), tracks: (0..tracks).map(track).collect(), artists: cards(artists, Target::Artist), albums: cards(albums, Target::Album), playlists: cards(playlists, Target::Playlist) }
    }

    #[test]
    fn a_searchs_chips_are_all_and_the_kinds_that_found_something() {
        use SearchKind::*;
        assert_eq!(search_kinds(&results(9, 2, 3, 4)), [All, Songs, Artists, Albums, Playlists]);
        assert_eq!(search_kinds(&results(9, 0, 3, 0)), [All, Songs, Albums]);
        assert_eq!(search_kinds(&results(0, 0, 0, 0)), [All], "nothing found: no chips, the page says so");
    }

    #[test]
    fn a_chosen_chip_lasts_for_its_search_and_while_it_has_results() {
        use SearchKind::*;
        let kinds = search_kinds(&results(9, 0, 3, 0));
        assert_eq!(search_kind_shown(&("senja".into(), Albums), "senja", &kinds), Albums);
        assert_eq!(search_kind_shown(&("senja".into(), Albums), "hujan", &kinds), All, "another search starts with everything");
        assert_eq!(search_kind_shown(&("senja".into(), Artists), "senja", &kinds), All, "no artists were found");
        assert_eq!(search_kind_shown(&(String::new(), All), "senja", &kinds), All);
    }

    #[test]
    fn the_top_results_parts_stay_in_its_card_and_apart() {
        for width in [328.0, 344.0, 361.0, 380.0] {
            let rect = Rect::from_min_size(pos2(16.0, 120.0), vec2(width, TOP_RESULT_H));
            let g = top_result_geom(rect);
            for part in [g.cover, g.text, g.play] {
                assert!(rect.contains_rect(part), "{width}: {part:?}");
            }
            assert!(g.cover.right() < g.text.left() && g.text.right() < g.play.left());
            assert!(g.play.width() >= m3::TOUCH && g.play.height() >= m3::TOUCH);
            assert!(g.text.width() >= 110.0, "{width}: {}", g.text.width());
        }
    }

    #[test]
    fn liked_music_leads_with_a_heart_and_a_downloaded_list_is_marked() {
        let downloaded: HashSet<String> = ["PLsaved".to_owned()].into();
        let (tile, mark) = library_row_marks(&card(Target::Playlist("LM".into())), &downloaded);
        assert!(tile.is_some() && mark.is_some(), "the heart on its tile, and the pin");
        let (tile, mark) = library_row_marks(&card(Target::Playlist("PLsaved".into())), &downloaded);
        assert!(tile.is_none() && mark.is_some(), "its own cover, and the downloaded mark");
        let (tile, mark) = library_row_marks(&card(Target::Album("PLsaved".into())), &downloaded);
        assert!(tile.is_none() && mark.is_some());
        for plain in [Target::Playlist("PLother".into()), Target::Artist("UC1".into())] {
            let (tile, mark) = library_row_marks(&card(plain), &downloaded);
            assert!(tile.is_none() && mark.is_none());
        }
    }

    #[test]
    fn the_actions_of_a_collection_fit_its_page_and_a_finger_without_touching() {
        for width in [328.0, 344.0, 361.0, 380.0] {
            for (before, after) in [(2, 2), (1, 2), (1, 1), (0, 0)] {
                let row = Rect::from_min_size(pos2(16.0, 500.0), vec2(width, PLAY_H));
                let a = action_row(row, before, after);
                assert_eq!((a.before.len(), a.after.len()), (before, after));
                let all: Vec<Rect> = a.before.iter().chain([&a.play]).chain(&a.after).copied().collect();
                for r in &all {
                    assert!(row.expand(0.01).contains_rect(*r), "{width} {before}+{after}: {r:?}");
                    assert!(r.width() >= m3::TOUCH && r.height() >= m3::TOUCH);
                }
                for pair in all.windows(2) {
                    assert!(pair[0].right() + 7.99 <= pair[1].left(), "{width}: a gap between {pair:?}");
                }
                assert!(a.play.width() >= 96.0 && a.play.width() <= 136.0, "{width}: {}", a.play.width());
                let (first, last) = (all[0], all[all.len() - 1]);
                assert!(((first.left() - row.left()) - (row.right() - last.right())).abs() < 0.01, "the row is centred");
            }
        }
        // The designs' sizes on a 412-point phone.
        let a = action_row(Rect::from_min_size(pos2(16.0, 0.0), vec2(380.0, PLAY_H)), 2, 2);
        assert_eq!(a.play.size(), vec2(136.0, 56.0));
        assert_eq!(a.before[0].size(), Vec2::splat(48.0));
    }

    #[test]
    fn a_lists_length_is_said_in_hours_and_minutes() {
        assert_eq!(length_text(3 * 3600 + 12 * 60 + 40), "3 hr 12 min");
        assert_eq!(length_text(45 * 60 + 59), "45 min");
        assert_eq!(length_text(3600), "1 hr 0 min");
        assert_eq!(length_text(20), "0 min");
    }

    #[test]
    fn a_collections_one_line_says_who_how_many_and_how_long() {
        assert_eq!(meta_line("Dadang Rusmana \u{2022} 48 songs", true, 48, 3 * 3600 + 12 * 60), "Dadang Rusmana \u{2022} 48 songs \u{2022} 3 hr 12 min");
        assert_eq!(meta_line("Album \u{2022} Arunika \u{2022} 2024", false, 11, 41 * 60), "Album \u{2022} Arunika \u{2022} 2024 \u{2022} 11 songs \u{2022} 41 min");
        assert_eq!(meta_line("Arunika", false, 11, 0), "Arunika", "songs that don't say how long they are");
        assert_eq!(meta_line("", false, 1, 200), "1 song \u{2022} 3 min");
    }

    #[test]
    fn a_collections_cover_is_large_but_leaves_the_page_its_sides() {
        assert_eq!(cover_side(380.0), 212.8);
        assert!(cover_side(328.0) >= 160.0 && cover_side(328.0) < 328.0 * 0.6);
        assert_eq!(cover_side(900.0), 220.0);
    }

    #[test]
    fn the_storage_cards_parts_stay_inside_it_and_apart() {
        for width in [328.0, 344.0, 361.0, 380.0] {
            for (disk, change) in [(true, true), (false, true), (true, false), (false, false)] {
                let card = Rect::from_min_size(pos2(16.0, 92.0), vec2(width, storage_height(disk, change)));
                let g = storage_geom(card, disk, change);
                assert_eq!(g.bar.is_some(), disk);
                assert!(card.contains(g.size) && card.contains(g.count_right) && card.contains(g.line));
                assert!(g.line.y >= g.size.y + 44.0, "under the size's line");
                assert!(card.contains_rect(g.place), "{width} {disk} {change}: {g:?}");
                if change {
                    assert!(card.contains_rect(g.change), "{width} {disk}: {g:?}");
                }
                assert!(g.change.height() >= m3::TOUCH && g.change.width() >= m3::TOUCH);
                assert!(g.place.right() < g.change.left() && g.place.width() >= 150.0, "{width}: {}", g.place.width());
                if let Some(bar) = g.bar {
                    assert!(card.contains_rect(bar));
                    assert!(bar.top() >= g.line.y + 20.0 && bar.bottom() <= g.place.top(), "between the line and the place");
                }
            }
        }
    }

    #[test]
    fn a_desktop_keeps_the_feed_tile_width() {
        assert_eq!(song_column(270.0, 1400.0, false), 270.0);
    }

    #[test]
    fn a_switch_sits_beside_its_text_on_a_phone() {
        assert_eq!(phone_slot(Some(44.0)), Slot::Beside);
    }

    #[test]
    fn a_dropdown_or_a_button_goes_under_the_text() {
        assert_eq!(phone_slot(Some(160.0)), Slot::Below);
        assert_eq!(phone_slot(Some(98.0)), Slot::Below);
    }

    #[test]
    fn a_note_without_a_control_keeps_no_room_for_one() {
        assert_eq!(phone_slot(Some(0.0)), Slot::Absent);
    }

    #[test]
    fn a_control_not_drawn_yet_goes_where_anything_fits() {
        assert_eq!(phone_slot(None), Slot::Below);
    }
}
