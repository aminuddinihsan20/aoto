//! The phone frame, in Material 3: a top bar (back or the page's title, and the account),
//! a navigation bar with four destinations, and a mini player above it that opens Now
//! Playing. The pages are drawn inside it (see `platform::phone`).

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::widgets::{self, named};
use super::{Action, App, LibTab, Page, glyphs, m3};
use crate::material::{self, Type, Weight, shape};
use crate::platform::Insets;
use crate::theme::{self, icon};

/// The bottom navigation's tabs. A page opened from one (an album from Library) keeps
/// that tab lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Search,
    Library,
    Downloads,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Home, Tab::Search, Tab::Library, Tab::Downloads];

    /// The tab `page` belongs to; pages that belong to none (album, artist, playlist,
    /// settings, transfer, the equalizer, listening stats) stay with `last`, the tab they
    /// were opened from.
    pub fn of(page: &Page, last: Tab) -> Tab {
        match page {
            Page::Home | Page::New => Tab::Home,
            Page::Search(_) => Tab::Search,
            Page::Library(_) => Tab::Library,
            Page::Downloads | Page::LocalFolder(_) | Page::LocalAlbum(_) | Page::LocalSongs | Page::Downloaded(_) => Tab::Downloads,
            Page::Album(_) | Page::Playlist(_) | Page::Artist(_) | Page::Settings | Page::SettingsCategory(_) | Page::Transfer | Page::Equalizer | Page::Stats => last,
        }
    }

    /// The page a tab opens.
    pub fn page(self) -> Page {
        match self {
            Tab::Home => Page::Home,
            Tab::Search => Page::Search(String::new()),
            Tab::Library => Page::Library(LibTab::Playlists),
            Tab::Downloads => Page::Downloads,
        }
    }

    /// Is `page` a tab's own page (so there's nothing to go back to)?
    pub fn is_root(page: &Page) -> bool {
        matches!(page, Page::Home | Page::Search(_) | Page::Library(_) | Page::Downloads)
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Search => "Search",
            Tab::Library => "Library",
            Tab::Downloads => "Downloads",
        }
    }

    fn glyph(self) -> &'static [u8] {
        match self {
            Tab::Home => glyphs::HOME,
            Tab::Search => glyphs::SEARCH,
            Tab::Library => glyphs::LIBRARY,
            Tab::Downloads => glyphs::DOWNLOAD,
        }
    }
}

/// The navigation bar's height above the gesture area, and the mini player's.
pub const NAV_H: f32 = 72.0;
pub const MINI_H: f32 = 64.0;
/// The top bar: back or the page's title on the left, the account on the right.
pub const TOP_H: f32 = 52.0;
/// The gap between the screen's side and the pages.
pub const SIDE: f32 = 16.0;
/// The room round the mini player: to the screen's sides and to the navigation bar.
const MINI_GAP: f32 = 8.0;
/// How far the mini player reaches up over a page: what floats over the page's foot
/// (a floating button) stands this much higher while a song is loaded.
pub const MINI_LIFT: f32 = MINI_H + MINI_GAP;

/// Draw the whole phone frame and the current page in it.
pub fn frame(app: &mut App, ui: &mut Ui, now: f64, screen: Rect) {
    let ctx = ui.ctx().clone();
    let insets = crate::platform::insets(&ctx);
    app.phone_tab = Tab::of(&app.page, app.phone_tab);

    // While typing the keyboard takes the bottom (its height is in `insets.bottom`): the
    // navigation and the mini player give way to it.
    let typing = ctx.egui_wants_keyboard_input();
    let nav = has_nav(&app.page) && !typing;
    // Always added, only emptied: a panel that comes and goes shifts the automatic ids
    // of the widgets after it, and the text field being typed in would lose its focus.
    egui::Panel::bottom("phone-nav")
        .resizable(false)
        .exact_size(if nav { NAV_H + insets.bottom } else { 0.0 })
        .frame(egui::Frame::new().fill(if material::style().outlined() { material::scheme().surface } else { material::scheme().surface_container }))
        .show_separator_line(false)
        .show(ui, |ui| {
            if nav {
                nav_bar(app, ui, insets);
            }
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().inner_margin(egui::Margin {
            left: (SIDE + insets.left) as i8,
            right: (SIDE + insets.right) as i8,
            top: (insets.top as i8).max(0),
            bottom: if nav { 0 } else { insets.bottom as i8 },
        }))
        .show(ui, |ui| {
            top_row(app, ui);
            super::pages::page(app, ui, now);
        });

    if app.current().is_some() && !typing {
        let nav_top = screen.bottom() - insets.bottom - if nav { NAV_H } else { 0.0 };
        let strip = Rect::from_min_size(
            pos2(screen.left() + MINI_GAP + insets.left, nav_top - MINI_H - MINI_GAP),
            vec2(screen.width() - 2.0 * MINI_GAP - insets.left - insets.right, MINI_H),
        );
        egui::Area::new(ui.scope_id().with("phone-mini"))
            .fixed_pos(strip.min)
            .order(egui::Order::Middle)
            .show(&ctx, |ui| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| mini_player(app, ui, strip));
            });
    }
}

/// Does `page` stand over the navigation bar? Settings take the whole screen, as a
/// phone's settings do: Back leaves them.
fn has_nav(page: &Page) -> bool {
    !matches!(page, Page::Settings | Page::SettingsCategory(_))
}

/// The words a phone greets with at `hour` (0..24) of the day.
pub fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

/// The hour of the day (0..24) at `unix` seconds, `offset` minutes east of UTC.
pub fn hour_of(unix: i64, offset: i32) -> u32 {
    ((unix + i64::from(offset) * 60).rem_euclid(86_400) / 3_600) as u32
}

/// The hour it is here.
fn local_hour() -> u32 {
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    hour_of(unix, crate::ytmapi::utc_offset_minutes())
}

/// What the top bar says on a tab's own page: Home greets, the others are named
/// (Search has its search bar there instead).
fn bar_title(page: &Page, hour: u32) -> Option<&'static str> {
    match page {
        Page::Home => Some(greeting(hour)),
        Page::Library(_) => Some(Tab::Library.label()),
        Page::Downloads => Some(Tab::Downloads.label()),
        _ => None,
    }
}

/// The name beside the back arrow of a page that has no header of its own to say it.
fn page_name(page: &Page) -> Option<&'static str> {
    match page {
        Page::Equalizer => Some("Equalizer"),
        Page::Stats => Some("Listening stats"),
        _ => None,
    }
}

/// What stands at the right end of the top bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ends {
    /// The account's picture and, when `action`, one action of the page before it.
    Account { action: bool },
    /// Two actions the page draws itself (an album's search and its menu).
    Page,
    /// Nothing: Settings, which the account's picture is the way to.
    Nothing,
}

fn bar_ends(page: &Page) -> Ends {
    match page {
        Page::Home | Page::Library(_) | Page::Downloads => Ends::Account { action: true },
        // The equalizer's switch stands there, drawn by its page.
        Page::Album(_) | Page::Playlist(_) | Page::Equalizer => Ends::Page,
        Page::Settings | Page::SettingsCategory(_) | Page::Stats => Ends::Nothing,
        _ => Ends::Account { action: false },
    }
}

/// Where the top bar's parts are: back at the left (a page opened from another), then
/// the title; at the right the end slot (the account's picture), an action before it,
/// and before those the spinner of a page that is loading.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopGeom {
    back: Option<Rect>,
    title: Rect,
    busy: Option<Rect>,
    action: Option<Rect>,
    end: Option<Rect>,
}

fn top_geom(bar: Rect, back: bool, ends: Ends, busy: bool) -> TopGeom {
    let cy = bar.center().y;
    let target = |right: f32| Rect::from_min_max(pos2(right - m3::TOUCH, cy - m3::TOUCH / 2.0), pos2(right, cy + m3::TOUCH / 2.0));
    let end = (ends != Ends::Nothing).then(|| target(bar.right()));
    let action = end.filter(|_| matches!(ends, Ends::Page | Ends::Account { action: true })).map(|e| target(e.left() - 2.0));
    let mut right = action.or(end).map_or(bar.right(), |a| a.left());
    let busy = busy.then(|| Rect::from_center_size(pos2(right - 16.0, cy), Vec2::splat(20.0)));
    right = busy.map_or(right, |b| b.left());
    // The arrow's ink, not its target, lines up with the page's side.
    let back = back.then(|| Rect::from_min_size(pos2(bar.left() - 12.0, cy - m3::TOUCH / 2.0), Vec2::splat(m3::TOUCH)));
    let left = back.map_or(bar.left(), |b| b.right());
    TopGeom { back, title: Rect::from_min_max(pos2(left, bar.top()), pos2((right - 8.0).max(left), bar.bottom())), busy, action, end }
}

fn slots_id() -> egui::Id {
    egui::Id::unique("phone-top-slots")
}

/// The top bar's two right-hand slots on a page that fills them itself (an album, a
/// playlist): the action's, then the end's. Their page draws its buttons there.
pub(super) fn page_slots(ctx: &egui::Context) -> Option<(Rect, Rect)> {
    ctx.data(|d| d.get_temp::<Option<(Rect, Rect)>>(slots_id())).flatten()
}

/// The top bar: back when the page isn't a tab's own, then its title (Search has its
/// search bar there); the page's action and the account on the right.
fn top_row(app: &mut App, ui: &mut Ui) {
    let m = material::scheme();
    let panel = ui.max_rect();
    let bar = Rect::from_min_size(panel.min, vec2(panel.width(), TOP_H));
    let ends = bar_ends(&app.page);
    let g = top_geom(bar, !Tab::is_root(&app.page), ends, app.loading);
    ui.ctx().data_mut(|d| d.insert_temp(slots_id(), g.action.zip(g.end).filter(|_| ends == Ends::Page)));
    if let Some(r) = g.back
        && m3::IconButton::new(icon::CHEVRON_LEFT, "Back").show_at(ui, r.center(), ui.scope_id().with("phone-back")).clicked()
    {
        app.nav_back();
    }
    account(app, ui, &g, ends);
    let page = app.page.clone();
    if let Page::Search(q) = &page {
        // The search bar is the title; the account's picture stands in its right end.
        super::pages::sync_search(app, q);
        let field = Rect::from_center_size(bar.center(), vec2(bar.width(), m3::SEARCH_BAR_H));
        let trailing = bar.right() - g.title.right() - 8.0;
        // (A child of its own: the bar's height is added once, below.)
        let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(field));
        super::shell::search_field(app, &mut bar_ui, field.width(), trailing);
    } else if let Some(title) = super::settings::bar_title(app.jam_open, &page).or(page_name(&page)) {
        let galley = m3::galley(ui, title, Type::TitleLarge, m.on_surface, g.title.width() - 4.0);
        ui.painter().galley(pos2(g.title.left() + 4.0, g.title.center().y - galley.size().y / 2.0), galley, m.on_surface);
    } else if let Some(title) = bar_title(&page, local_hour()) {
        let galley = m3::galley(ui, title, Type::HeadlineMedium, m.on_surface, g.title.width());
        ui.painter().galley(pos2(g.title.left(), g.title.center().y - galley.size().y / 2.0), galley, m.on_surface);
    }
    if let Some(r) = g.busy {
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(r));
        cui.spinner();
    }
    ui.add_space(TOP_H);
}

/// The action a tab's own page has in the top bar, before the account: its icon, its
/// name, and whether it is what the page shows now.
fn bar_action(page: &Page) -> Option<(m3::Icon, &'static str, bool)> {
    match page {
        // The new releases have no tab of their own.
        Page::Home => Some((icon::SPARKLES.into(), "New releases", false)),
        Page::Library(tab) => Some((icon::HISTORY.into(), "History", *tab == LibTab::History)),
        Page::Downloads => Some((m3::Icon::Drawn(m3::drawn::tune), "Download settings", false)),
        _ => None,
    }
}

/// A scalloped disc, Material's "cookie": the shape of an account with no picture.
pub(super) fn cookie(ui: &Ui, center: Pos2, radius: f32, color: Color32) {
    const LOBES: f32 = 12.0;
    let points: Vec<Pos2> = (0..96)
        .map(|i| {
            let a = i as f32 / 96.0 * std::f32::consts::TAU;
            center + radius * (0.93 + 0.07 * (a * LOBES).cos()) * vec2(a.cos(), a.sin())
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    for p in &points {
        mesh.colored_vertex(*p, color);
    }
    let n = points.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    // A hairline in the same colour softens the fan's hard edge.
    ui.painter().add(egui::Shape::closed_line(points, Stroke::new(1.0, color)));
}

/// The right of the top bar: the signed-in person's picture (their initial without
/// one, a figure when signed out), which opens Settings, where the account is; and
/// before it the page's action (`bar_action`).
fn account(app: &mut App, ui: &mut Ui, g: &TopGeom, ends: Ends) {
    // The search field anchors its suggestions where it is drawn; elsewhere there is none.
    app.search_rect = None;
    let Some(avatar) = g.end.filter(|_| matches!(ends, Ends::Account { .. })) else { return };
    let m = material::scheme();
    // Dev hook: YTM_FAKE_ACCOUNT=<name> shows the signed-in look in screenshots.
    let fake = super::dev_var("YTM_FAKE_ACCOUNT");
    let signed_in = app.logged_in || fake.is_some();
    let initial = match (signed_in, fake) {
        (true, Some(name)) => name.chars().next(),
        (true, None) => app.account_name.as_deref().and_then(|n| n.chars().next()),
        _ => None,
    };
    let avatar_url = app.account_avatar.clone();
    let action = g.action.zip(bar_action(&app.page));
    let cluster = action.map_or(avatar, |(a, _)| a.union(avatar));
    // Its own layer, above the pages: a page's header art scrolls up into this corner.
    let (to_account, acted) = egui::Area::new(ui.scope_id().with("account"))
        .order(egui::Order::Middle)
        .fixed_pos(cluster.min)
        .show(ui.ctx(), |ui| {
            ui.allocate_exact_size(cluster.size(), Sense::hover());
            let acted = action.is_some_and(|(r, (glyph, name, on))| {
                let style = if on { m3::IconStyle::Tonal } else { m3::IconStyle::Standard };
                m3::IconButton::new(glyph, name).style(style).show_at(ui, r.center(), ui.scope_id().with("bar-action")).clicked()
            });
            let resp = named(ui.interact(avatar, ui.scope_id().with("account-chip"), Sense::click()), if signed_in { "Account" } else { "Sign in" });
            let pic = Rect::from_center_size(avatar.center(), Vec2::splat(36.0));
            match (&avatar_url, signed_in) {
                (Some(url), true) => {
                    ui.painter().circle_filled(pic.center(), 18.0, m.tertiary_container);
                    widgets::paint_remote(ui, url, pic, 18.0, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                (_, true) => {
                    cookie(ui, pic.center(), 19.0, m.tertiary_container);
                    let letter = initial.unwrap_or('?').to_uppercase().to_string();
                    ui.painter().text(pic.center(), Align2::CENTER_CENTER, letter, Type::TitleMedium.font(), m.on_tertiary_container);
                }
                _ => {
                    cookie(ui, pic.center(), 19.0, m.surface_container_highest);
                    widgets::icon_at(ui, pic.center(), icon::USER, 18.0, m.on_surface_variant);
                }
            }
            if resp.is_pointer_button_down_on() {
                ui.painter().circle_filled(pic.center(), 20.0, m.on_surface.gamma_multiply(0.1));
            }
            (resp.clicked(), acted)
        })
        .inner;
    if to_account {
        app.actions.push(Action::Go(Page::Settings));
    } else if acted {
        match app.page.clone() {
            Page::Home => app.actions.push(Action::Go(Page::New)),
            // The library's views swap in place (no history entry); the one shown refreshes.
            Page::Library(LibTab::History) => app.refresh(),
            Page::Library(_) => {
                app.page = Page::Library(LibTab::History);
                app.page_loaded();
            }
            Page::Downloads => app.actions.push(Action::Go(super::settings::Category::Downloads.page())),
            _ => {}
        }
    }
}

/// One destination of the navigation bar: the cell a finger hits, the pill behind the
/// icon of the active one, and the label's middle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NavCell {
    cell: Rect,
    pill: Rect,
    label: Pos2,
}

/// The navigation bar's destinations, side by side across `row`.
fn nav_cells(row: Rect) -> [NavCell; 4] {
    let w = row.width() / Tab::ALL.len() as f32;
    std::array::from_fn(|i| {
        let cell = Rect::from_min_size(pos2(row.left() + w * i as f32, row.top()), vec2(w, row.height()));
        let pill = Rect::from_min_size(pos2(cell.center().x - 32.0, cell.top() + 12.0), vec2(64.0, 32.0));
        NavCell { cell, pill, label: pos2(cell.center().x, pill.bottom() + 4.0 + 8.0) }
    })
}

/// The accent bar Sticker's navigation stands on: the row, drawn in from the screen's
/// sides and from its own top and bottom, leaving room for the card behind it.
fn nav_plate(row: Rect) -> Rect {
    Rect::from_min_max(pos2(row.left() + 12.0, row.top() + 4.0), pos2(row.right() - 16.0, row.bottom() - 8.0))
}

/// Sticker's destinations: spread across the bar, not the screen, and set closer
/// together top to bottom, so that every label keeps clear of the bar's line.
fn sticker_cells(row: Rect) -> [NavCell; 4] {
    let plate = nav_plate(row);
    let inner = plate.shrink2(vec2(8.0, 0.0));
    let w = inner.width() / Tab::ALL.len() as f32;
    std::array::from_fn(|i| {
        let cell = Rect::from_min_size(pos2(inner.left() + w * i as f32, row.top()), vec2(w, row.height()));
        let pill = Rect::from_min_size(pos2(cell.center().x - 28.0, plate.top() + 6.0), vec2(56.0, 28.0));
        NavCell { cell, pill, label: pos2(cell.center().x, pill.bottom() + 3.0 + 8.0) }
    })
}

fn nav_bar(app: &mut App, ui: &mut Ui, insets: Insets) {
    let m = material::scheme();
    let full = ui.max_rect();
    let row = Rect::from_min_max(pos2(full.left() + insets.left, full.top()), pos2(full.right() - insets.right, full.top() + NAV_H));
    let look = material::style();
    if look.outlined() {
        // Sticker: the destinations stand on one accent bar, itself on a second card.
        m3::plate(ui, nav_plate(row), 14.0, look.accent, m3::Stack::Card);
    }
    let cells = if look.outlined() { sticker_cells(row) } else { nav_cells(row) };
    for (i, (tab, c)) in Tab::ALL.into_iter().zip(cells).enumerate() {
        let resp = named(ui.interact(c.cell, ui.scope_id().with(("phone-tab", i)), Sense::click()), tab.label());
        let on = app.phone_tab == tab;
        if look.outlined() {
            let s = theme::anim_bool(ui.ctx(), ("phone-tab-on", i), on, 0.2);
            if s > 0.0 {
                let pill = Rect::from_center_size(c.pill.center(), vec2(egui::lerp(32.0..=c.pill.width(), s), c.pill.height()));
                ui.painter().rect_filled(pill, 8.0, m.secondary_container.gamma_multiply(s));
                ui.painter().rect_stroke(pill, 8.0, egui::Stroke::new(look.outline, look.line.gamma_multiply(s)), egui::StrokeKind::Inside);
            }
            if resp.is_pointer_button_down_on() {
                ui.painter().rect_filled(c.pill, 8.0, look.on_accent.gamma_multiply(0.1));
            }
            glyphs::draw(ui, c.pill.center(), 22.0, tab.glyph(), look.on_accent);
            let role = if on { Type::LabelMedium.spec().weight(Weight::Bold) } else { Type::LabelMedium.spec() };
            let label = m3::galley(ui, tab.label(), role, look.on_accent, c.cell.width());
            ui.painter().galley(c.label - label.size() / 2.0, label, look.on_accent);
            if resp.clicked() {
                app.actions.push(Action::Go(tab.page()));
            }
            continue;
        }
        // The pill opens out from the icon as its destination becomes the active one.
        let s = theme::anim_bool(ui.ctx(), ("phone-tab-on", i), on, 0.2);
        if s > 0.0 {
            let pill = Rect::from_center_size(c.pill.center(), vec2(egui::lerp(32.0..=c.pill.width(), s), c.pill.height()));
            ui.painter().rect_filled(pill, pill.height() / 2.0, m.secondary_container.gamma_multiply(s));
        }
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(c.pill, c.pill.height() / 2.0, m.on_surface.gamma_multiply(0.1));
        }
        let ink = theme::lerp_color(m.on_surface_variant, m.on_secondary_container, s);
        glyphs::draw(ui, c.pill.center(), 24.0, tab.glyph(), ink);
        let role = if on { Type::LabelMedium.spec().weight(Weight::Bold) } else { Type::LabelMedium.spec() };
        let color = if on { m.on_surface } else { m.on_surface_variant };
        let label = m3::galley(ui, tab.label(), role, color, c.cell.width());
        ui.painter().galley(c.label - label.size() / 2.0, label, color);
        if resp.clicked() {
            app.actions.push(Action::Go(tab.page()));
        }
    }
}

/// Where the mini player's parts are: the cover, the two lines of text, the touch
/// targets of play/pause and next, and the progress line along its bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MiniGeom {
    cover: Rect,
    text: Rect,
    play: Rect,
    next: Rect,
    line: Rect,
}

fn mini_geom(rect: Rect) -> MiniGeom {
    let cy = rect.center().y;
    let cover = Rect::from_min_size(pos2(rect.left() + 8.0, cy - 24.0), Vec2::splat(48.0));
    let next = Rect::from_center_size(pos2(rect.right() - 2.0 - m3::TOUCH / 2.0, cy), Vec2::splat(m3::TOUCH));
    let play = Rect::from_center_size(pos2(next.left() - 6.0 - m3::TOUCH / 2.0, cy), Vec2::splat(m3::TOUCH));
    let left = cover.right() + 12.0;
    MiniGeom {
        cover,
        text: Rect::from_min_max(pos2(left, rect.top()), pos2((play.left() - 4.0).max(left), rect.bottom())),
        play,
        next,
        // Inside the straight part of the bottom edge, clear of the round corners.
        line: Rect::from_min_max(pos2(rect.left() + shape::CARD_SMALL, rect.bottom() - 4.0), pos2(rect.right() - shape::CARD_SMALL, rect.bottom() - 2.0)),
    }
}

/// What the mini player's button shows of the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MiniState {
    /// A song is on its way: nothing is heard yet.
    Loading,
    Playing,
    Paused,
}

/// A song on its way shows as loading whether or not the player counts it as playing.
fn mini_state(playing: bool, buffering: bool) -> MiniState {
    match (buffering, playing) {
        (true, _) => MiniState::Loading,
        (false, true) => MiniState::Playing,
        (false, false) => MiniState::Paused,
    }
}

/// The song playing, with play/pause and next; a tap anywhere else opens Now Playing.
fn mini_player(app: &mut App, ui: &mut Ui, rect: Rect) {
    let Some(current) = app.current().cloned() else { return };
    // An Automix handoff keeps the outgoing song here, as Now Playing does, until the
    // next one is the louder of the two.
    let track = if app.mix_hold().is_some() { app.shown().cloned().unwrap_or(current) } else { current };
    let m = material::scheme();
    let playing = app.playing();
    let g = mini_geom(rect);
    // A lined look stands the bar on its second card, with no soft shadow under it.
    if !material::style().outlined() {
        widgets::soft_shadow(ui, rect, shape::CARD_SMALL, 14.0, 0.3);
    }
    super::m3::plate(ui, rect, super::m3::round(shape::CARD_SMALL), m.secondary_container, super::m3::Stack::Card);

    let open = named(ui.interact(rect, ui.scope_id().with("mini-open"), Sense::click()), "Open now playing");
    if open.clicked() {
        app.np_open = true;
    }
    if open.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, shape::CARD_SMALL, m.on_secondary_container.gamma_multiply(0.08));
    }

    widgets::paint_cover(ui, g.cover, &track.thumbs, shape::TILE_SMALL, 1.0);
    let ink = m.on_secondary_container;
    m3::two_lines(ui, g.text.left(), g.text.center().y, g.text.width(), &track.title, Type::TitleMedium, ink, &track.artist_line(), Type::BodyMedium, ink.gamma_multiply(0.8));

    // Progress: a thin line along the bottom edge, as far as the song has played.
    let super::nowplaying::SeekState { total, pos, .. } = super::nowplaying::seek_state(app, &track);
    if total > 0.0 {
        let frac = (pos / total).clamp(0.0, 1.0);
        let done = Rect::from_min_max(g.line.min, pos2(g.line.left() + g.line.width() * frac, g.line.bottom()));
        ui.painter().rect_filled(done, CornerRadius::same(1), m.primary);
        if playing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    // Play/pause: an arc turns in it while the song is on its way, and its corners are
    // squarer while it plays, as the large one's on Now Playing are.
    let (glyph, name): (m3::Icon, &str) = match mini_state(playing, app.buffering) {
        MiniState::Loading => (m3::Icon::Drawn(m3::drawn::spinner), "Pause"),
        MiniState::Playing => (glyphs::PAUSE.into(), "Pause"),
        MiniState::Paused => (glyphs::PLAY.into(), "Play"),
    };
    let on = theme::anim_bool(ui.ctx(), "mini-play-morph", playing || app.buffering, 0.2);
    let play = m3::IconButton::new(glyph, name).style(m3::IconStyle::Filled).radius(egui::lerp(20.0..=shape::TILE_SMALL, on)).show_at(ui, g.play.center(), ui.scope_id().with(("mini", "play")));
    if play.clicked() {
        app.toggle();
    }
    if m3::IconButton::new(glyphs::NEXT, "Next").show_at(ui, g.next.center(), ui.scope_id().with(("mini", "next"))).clicked() {
        app.next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_pages_belong_to_their_tab() {
        assert_eq!(Tab::of(&Page::Home, Tab::Library), Tab::Home);
        assert_eq!(Tab::of(&Page::New, Tab::Library), Tab::Home);
        assert_eq!(Tab::of(&Page::Search("x".into()), Tab::Home), Tab::Search);
        assert_eq!(Tab::of(&Page::Library(LibTab::Songs), Tab::Home), Tab::Library);
        assert_eq!(Tab::of(&Page::Downloads, Tab::Home), Tab::Downloads);
    }

    #[test]
    fn a_page_opened_from_a_tab_keeps_that_tab() {
        for page in [Page::Album("a".into()), Page::Artist("a".into()), Page::Playlist("p".into()), Page::Settings, Page::Transfer, Page::Equalizer, Page::Stats] {
            assert_eq!(Tab::of(&page, Tab::Library), Tab::Library, "{page:?}");
            assert_eq!(Tab::of(&page, Tab::Search), Tab::Search, "{page:?}");
        }
    }

    #[test]
    fn each_tab_opens_a_page_that_belongs_to_it() {
        for tab in Tab::ALL {
            assert_eq!(Tab::of(&tab.page(), Tab::Home), tab);
            assert!(Tab::is_root(&tab.page()));
        }
    }

    #[test]
    fn a_local_folder_belongs_to_the_downloads_tab_and_has_a_back() {
        assert_eq!(Tab::of(&Page::LocalFolder("content://t/jp".into()), Tab::Home), Tab::Downloads);
        assert_eq!(Tab::of(&Page::Downloaded("PL123".into()), Tab::Home), Tab::Downloads);
        assert!(!Tab::is_root(&Page::LocalFolder("content://t/jp".into())));
    }

    #[test]
    fn only_tab_pages_are_roots() {
        assert!(!Tab::is_root(&Page::Album("a".into())));
        assert!(!Tab::is_root(&Page::Settings));
        assert!(!Tab::is_root(&Page::Equalizer));
        assert!(!Tab::is_root(&Page::Stats));
        assert!(!Tab::is_root(&Page::New));
    }

    /// The rows a phone's frame is laid out in, from a 360-point screen to a 412-point one.
    const SCREENS: [f32; 4] = [360.0, 384.0, 393.0, 412.0];

    fn apart(a: Rect, b: Rect) -> bool {
        let both = a.intersect(b);
        both.width() <= 0.0 || both.height() <= 0.0
    }

    #[test]
    fn the_greeting_follows_the_time_of_day() {
        assert_eq!(greeting(5), "Good morning");
        assert_eq!(greeting(11), "Good morning");
        assert_eq!(greeting(12), "Good afternoon");
        assert_eq!(greeting(17), "Good afternoon");
        assert_eq!(greeting(18), "Good evening");
        assert_eq!(greeting(23), "Good evening");
        assert_eq!(greeting(2), "Good evening", "the small hours are still last evening");
    }

    #[test]
    fn the_hour_is_the_local_one() {
        // 2026-11-20 06:30 UTC.
        let unix = 1_795_156_200;
        assert_eq!(hour_of(unix, 0), 6);
        assert_eq!(hour_of(unix, 7 * 60), 13, "Jakarta");
        assert_eq!(hour_of(unix, -8 * 60), 22, "the evening before, in Los Angeles");
        assert_eq!(hour_of(unix, 5 * 60 + 30), 12, "half-hour zones");
        assert_eq!(hour_of(-3_600, 0), 23, "before 1970 it still is an hour of a day");
    }

    #[test]
    fn home_greets_and_the_other_tabs_are_named_in_the_top_bar() {
        assert_eq!(bar_title(&Page::Home, 9), Some("Good morning"));
        assert_eq!(bar_title(&Page::Library(LibTab::Albums), 9), Some("Library"));
        assert_eq!(bar_title(&Page::Search("x".into()), 9), None, "the search bar stands there instead");
        assert_eq!(bar_title(&Page::Downloads, 9), Some("Downloads"));
        assert_eq!(bar_title(&Page::Settings, 9), None, "a page with a back button names itself below it");
        assert_eq!(bar_title(&Page::Album("a".into()), 9), None);
    }

    #[test]
    fn every_destination_is_a_finger_wide_with_its_pill_and_label_inside() {
        for screen in SCREENS {
            let row = Rect::from_min_size(pos2(0.0, 800.0), vec2(screen, NAV_H));
            let cells = nav_cells(row);
            assert_eq!(cells[0].cell.left(), 0.0);
            assert!((cells[3].cell.right() - screen).abs() < 0.01);
            for c in cells {
                assert!(c.cell.width() >= m3::TOUCH && c.cell.height() >= m3::TOUCH, "{screen}: {c:?}");
                assert!(c.cell.contains_rect(c.pill), "{screen}: {c:?}");
                assert_eq!(c.pill.size(), vec2(64.0, 32.0));
                // The label (a 16-point line) sits under the pill and above the bar's bottom.
                assert!(c.label.y - 8.0 >= c.pill.bottom() && c.label.y + 8.0 <= c.cell.bottom(), "{c:?}");
            }
            for pair in cells.windows(2) {
                assert!(apart(pair[0].cell, pair[1].cell));
            }
        }
    }

    #[test]
    fn the_mini_players_parts_do_not_overlap_and_its_buttons_fit_a_finger() {
        for screen in SCREENS {
            let rect = Rect::from_min_size(pos2(MINI_GAP, 700.0), vec2(screen - 2.0 * MINI_GAP, MINI_H));
            let g = mini_geom(rect);
            for part in [g.cover, g.play, g.next, g.line] {
                assert!(rect.contains_rect(part), "{screen}: {part:?}");
            }
            for target in [g.play, g.next] {
                assert!(target.width() >= m3::TOUCH && target.height() >= m3::TOUCH);
            }
            assert!(apart(g.cover, g.text) && apart(g.text, g.play) && apart(g.play, g.next));
            assert!(g.text.width() >= 150.0, "{screen}: the title keeps {}", g.text.width());
            assert!(g.line.top() >= g.cover.bottom(), "the progress line runs under the cover");
            assert!(g.line.height() <= 2.0);
        }
    }

    #[test]
    fn the_mini_players_button_says_whether_the_song_plays_waits_or_is_on_its_way() {
        assert_eq!(mini_state(true, false), MiniState::Playing);
        assert_eq!(mini_state(false, false), MiniState::Paused);
        assert_eq!(mini_state(false, true), MiniState::Loading);
        assert_eq!(mini_state(true, true), MiniState::Loading, "a song still loading is not heard yet");
    }

    #[test]
    fn the_top_bars_parts_do_not_overlap_and_fit_a_finger() {
        for screen in SCREENS {
            let bar = Rect::from_min_size(pos2(SIDE, 36.0), vec2(screen - 2.0 * SIDE, TOP_H));
            let with_action = Ends::Account { action: true };
            let plain = Ends::Account { action: false };
            for (back, ends, busy) in [(false, with_action, false), (false, with_action, true), (true, plain, true), (false, plain, false), (true, Ends::Page, true), (true, Ends::Nothing, false)] {
                let g = top_geom(bar, back, ends, busy);
                let mut parts = vec![g.title];
                parts.extend(g.end);
                parts.extend(g.back);
                parts.extend(g.action);
                parts.extend(g.busy);
                for (i, a) in parts.iter().enumerate() {
                    for b in &parts[i + 1..] {
                        assert!(apart(*a, *b), "{screen} {back} {ends:?} {busy}: {a:?} and {b:?}");
                    }
                }
                for target in [g.end, g.back, g.action].into_iter().flatten() {
                    assert!(target.width() >= m3::TOUCH && target.height() >= m3::TOUCH);
                    assert!(target.left() >= 0.0 && target.right() <= screen, "on the screen: {target:?}");
                }
                assert_eq!(g.end.is_some(), ends != Ends::Nothing);
                assert_eq!(g.end.map(|e| e.right()), g.end.map(|_| bar.right()));
                assert_eq!(g.action.is_some(), matches!(ends, Ends::Page | Ends::Account { action: true }));
                assert_eq!(g.back.is_some(), back);
                // (A page that fills both slots itself, an album, has no title up here.)
                if ends != Ends::Page {
                    assert!(g.title.width() >= 180.0, "{screen}: the title keeps {}", g.title.width());
                }
            }
        }
    }

    #[test]
    fn a_loading_page_takes_its_spinners_room_from_the_title() {
        let bar = Rect::from_min_size(pos2(SIDE, 36.0), vec2(380.0, TOP_H));
        let ends = Ends::Account { action: true };
        let (idle, busy) = (top_geom(bar, false, ends, false), top_geom(bar, false, ends, true));
        assert!(idle.busy.is_none());
        assert!(busy.title.right() < idle.title.right());
        assert_eq!(busy.end, idle.end);
    }

    #[test]
    fn settings_take_the_whole_screen_and_every_other_page_keeps_the_navigation() {
        assert!(!has_nav(&Page::Settings));
        assert!(!has_nav(&Page::SettingsCategory(crate::ui::settings::Category::Lyrics)));
        for page in [Page::Home, Page::Search(String::new()), Page::Library(LibTab::Playlists), Page::Downloads, Page::Album("a".into()), Page::Stats, Page::Equalizer, Page::Transfer] {
            assert!(has_nav(&page), "{page:?}");
        }
    }

    #[test]
    fn each_page_has_the_right_end_of_the_top_bar_it_needs() {
        assert_eq!(bar_ends(&Page::Home), Ends::Account { action: true });
        assert_eq!(bar_ends(&Page::Library(LibTab::Albums)), Ends::Account { action: true });
        assert_eq!(bar_ends(&Page::Downloads), Ends::Account { action: true });
        assert_eq!(bar_ends(&Page::Search(String::new())), Ends::Account { action: false }, "the picture stands in the search bar's end");
        assert_eq!(bar_ends(&Page::Playlist("p".into())), Ends::Page, "its search and its menu");
        assert_eq!(bar_ends(&Page::Album("a".into())), Ends::Page);
        assert_eq!(bar_ends(&Page::Artist("a".into())), Ends::Account { action: false });
        assert_eq!(bar_ends(&Page::Settings), Ends::Nothing, "the picture is the way to Settings");
        assert_eq!(bar_ends(&Page::Equalizer), Ends::Page, "its switch");
        assert_eq!(bar_ends(&Page::Stats), Ends::Nothing);
        assert_eq!(bar_ends(&Page::SettingsCategory(crate::ui::settings::Category::Sound)), Ends::Nothing);
    }

    #[test]
    fn the_equalizer_and_the_stats_are_named_beside_their_back_arrow() {
        assert_eq!(page_name(&Page::Equalizer), Some("Equalizer"));
        assert_eq!(page_name(&Page::Stats), Some("Listening stats"));
        assert_eq!(page_name(&Page::Album("a".into())), None, "an album's header names it");
        assert_eq!(page_name(&Page::Home), None);
    }

    #[test]
    fn a_tabs_action_is_named_and_lit_when_it_is_what_shows() {
        assert_eq!(bar_action(&Page::Home).map(|a| a.1), Some("New releases"));
        assert_eq!(bar_action(&Page::Library(LibTab::Playlists)).map(|a| (a.1, a.2)), Some(("History", false)));
        assert_eq!(bar_action(&Page::Library(LibTab::History)).map(|a| (a.1, a.2)), Some(("History", true)));
        assert_eq!(bar_action(&Page::Downloads).map(|a| a.1), Some("Download settings"));
        assert!(bar_action(&Page::Search(String::new())).is_none());
    }

    #[test]
    fn a_settings_page_is_named_in_the_top_bar_beside_its_back_arrow() {
        use crate::ui::settings::{Category, bar_title};
        assert_eq!(bar_title(false, &Page::Settings), Some("Settings"));
        assert_eq!(bar_title(false, &Page::SettingsCategory(Category::Sound)), Some("Sound & quality"));
        assert_eq!(bar_title(true, &Page::SettingsCategory(Category::Account)), Some("Listen together"), "the section opened inside it");
        assert_eq!(bar_title(true, &Page::Home), None);
        assert_eq!(bar_title(false, &Page::Stats), None);
    }

    #[test]
    fn stickers_navigation_bar_keeps_every_destination_on_it_and_stays_in_the_row() {
        let row = Rect::from_min_size(pos2(0.0, 800.0), vec2(412.0, NAV_H));
        let plate = nav_plate(row);
        assert!(row.contains_rect(plate.translate(egui::Vec2::splat(4.0))), "the card behind it stays inside the row too");
        for c in sticker_cells(row) {
            assert!(plate.contains_rect(c.pill), "{:?} is off the bar", c.pill);
            // A label is a line of text about 16 high and, at its longest ("Downloads",
            // bold), about 72 wide: all of it keeps at least 4 clear of the bar's line.
            let label = Rect::from_center_size(c.label, vec2(72.0, 16.0));
            assert!(plate.shrink(4.0).contains_rect(label), "{label:?} touches the bar's edge {plate:?}");
        }
    }
}
