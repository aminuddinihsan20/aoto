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
            Page::Album(_) | Page::Playlist(_) | Page::Artist(_) | Page::Settings | Page::Transfer | Page::Equalizer | Page::Stats => last,
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

/// Draw the whole phone frame and the current page in it.
pub fn frame(app: &mut App, ui: &mut Ui, now: f64, screen: Rect) {
    let ctx = ui.ctx().clone();
    let insets = crate::platform::insets(&ctx);
    app.phone_tab = Tab::of(&app.page, app.phone_tab);

    // While typing the keyboard takes the bottom (its height is in `insets.bottom`): the
    // navigation and the mini player give way to it.
    let typing = ctx.egui_wants_keyboard_input();
    // Always added, only emptied: a panel that comes and goes shifts the automatic ids
    // of the widgets after it, and the text field being typed in would lose its focus.
    egui::Panel::bottom("phone-nav")
        .resizable(false)
        .exact_size(if typing { 0.0 } else { NAV_H + insets.bottom })
        .frame(egui::Frame::new().fill(material::scheme().surface_container))
        .show_separator_line(false)
        .show(ui, |ui| {
            if !typing {
                nav_bar(app, ui, insets);
            }
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().inner_margin(egui::Margin {
            left: (SIDE + insets.left) as i8,
            right: (SIDE + insets.right) as i8,
            top: (insets.top as i8).max(0),
            bottom: if typing { insets.bottom as i8 } else { 0 },
        }))
        .show(ui, |ui| {
            top_row(app, ui);
            super::pages::page(app, ui, now);
        });

    if app.current().is_some() && !typing {
        let nav_top = screen.bottom() - NAV_H - insets.bottom;
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

/// What the top bar says on a tab's own page: Home greets, the others are named.
fn bar_title(page: &Page, hour: u32) -> Option<&'static str> {
    match page {
        Page::Home => Some(greeting(hour)),
        Page::Search(_) => Some(Tab::Search.label()),
        Page::Library(_) => Some(Tab::Library.label()),
        Page::Downloads => Some(Tab::Downloads.label()),
        _ => None,
    }
}

/// Where the top bar's parts are: back at the left (a page opened from another), then
/// the title; at the right the account's picture, an action before it, and before
/// those the spinner of a page that is loading.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopGeom {
    back: Option<Rect>,
    title: Rect,
    busy: Option<Rect>,
    action: Option<Rect>,
    avatar: Rect,
}

fn top_geom(bar: Rect, back: bool, action: bool, busy: bool) -> TopGeom {
    let cy = bar.center().y;
    let target = |right: f32| Rect::from_min_max(pos2(right - m3::TOUCH, cy - m3::TOUCH / 2.0), pos2(right, cy + m3::TOUCH / 2.0));
    let avatar = target(bar.right());
    let action = action.then(|| target(avatar.left() - 2.0));
    let mut right = action.map_or(avatar.left(), |a| a.left());
    let busy = busy.then(|| Rect::from_center_size(pos2(right - 16.0, cy), Vec2::splat(20.0)));
    right = busy.map_or(right, |b| b.left());
    // The arrow's ink, not its target, lines up with the page's side.
    let back = back.then(|| Rect::from_min_size(pos2(bar.left() - 12.0, cy - m3::TOUCH / 2.0), Vec2::splat(m3::TOUCH)));
    let left = back.map_or(bar.left(), |b| b.right());
    TopGeom { back, title: Rect::from_min_max(pos2(left, bar.top()), pos2((right - 8.0).max(left), bar.bottom())), busy, action, avatar }
}

/// The top bar: back when the page isn't a tab's own, else its title; the account on the right.
fn top_row(app: &mut App, ui: &mut Ui) {
    let m = material::scheme();
    let panel = ui.max_rect();
    let bar = Rect::from_min_size(panel.min, vec2(panel.width(), TOP_H));
    let home = app.page == Page::Home;
    let g = top_geom(bar, !Tab::is_root(&app.page), home, app.loading);
    if let Some(r) = g.back
        && m3::IconButton::new(icon::CHEVRON_LEFT, "Back").show_at(ui, r.center(), ui.scope_id().with("phone-back")).clicked()
    {
        app.nav_back();
    }
    if let Some(title) = bar_title(&app.page, local_hour()) {
        let galley = m3::galley(ui, title, Type::HeadlineMedium, m.on_surface, g.title.width());
        ui.painter().galley(pos2(g.title.left(), g.title.center().y - galley.size().y / 2.0), galley, m.on_surface);
    }
    account(app, ui, &g);
    if let Some(r) = g.busy {
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(r));
        cui.spinner();
    }
    ui.add_space(TOP_H);
}

/// A scalloped disc, Material's "cookie": the shape of an account with no picture.
fn cookie(ui: &Ui, center: Pos2, radius: f32, color: Color32) {
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
/// one, a figure when signed out), which opens Settings, where the account is; on
/// Home, before it, the way to the new releases, which have no tab of their own.
fn account(app: &mut App, ui: &mut Ui, g: &TopGeom) {
    // The search field lives on the Search page: nothing here to anchor suggestions to.
    app.search_rect = None;
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
    let (avatar, action) = (g.avatar, g.action);
    let cluster = action.map_or(avatar, |a| a.union(avatar));
    // Its own layer, above the pages: a page's header art scrolls up into this corner.
    let (to_account, to_new) = egui::Area::new(ui.scope_id().with("account"))
        .order(egui::Order::Middle)
        .fixed_pos(cluster.min)
        .show(ui.ctx(), |ui| {
            ui.allocate_exact_size(cluster.size(), Sense::hover());
            let to_new = action.is_some_and(|r| m3::IconButton::new(icon::SPARKLES, "New releases").show_at(ui, r.center(), ui.scope_id().with("new-releases")).clicked());
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
            (resp.clicked(), to_new)
        })
        .inner;
    if to_account {
        app.actions.push(Action::Go(Page::Settings));
    } else if to_new {
        app.actions.push(Action::Go(Page::New));
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

fn nav_bar(app: &mut App, ui: &mut Ui, insets: Insets) {
    let m = material::scheme();
    let full = ui.max_rect();
    let row = Rect::from_min_max(pos2(full.left() + insets.left, full.top()), pos2(full.right() - insets.right, full.top() + NAV_H));
    for (i, (tab, c)) in Tab::ALL.into_iter().zip(nav_cells(row)).enumerate() {
        let resp = named(ui.interact(c.cell, ui.scope_id().with(("phone-tab", i)), Sense::click()), tab.label());
        let on = app.phone_tab == tab;
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

/// The song playing, with play/pause and next; a tap anywhere else opens Now Playing.
fn mini_player(app: &mut App, ui: &mut Ui, rect: Rect) {
    let Some(track) = app.current().cloned() else { return };
    let m = material::scheme();
    let playing = app.playing();
    let g = mini_geom(rect);
    widgets::soft_shadow(ui, rect, shape::CARD_SMALL, 14.0, 0.3);
    ui.painter().rect_filled(rect, shape::CARD_SMALL, m.secondary_container);

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
    let status = app.audio.status.clone();
    let total = status.duration().map(|d| d.as_secs_f32()).or(track.duration.map(|d| d as f32)).unwrap_or(0.0);
    if total > 0.0 {
        let frac = (status.position_smooth().as_secs_f32() / total).clamp(0.0, 1.0);
        let done = Rect::from_min_max(g.line.min, pos2(g.line.left() + g.line.width() * frac, g.line.bottom()));
        ui.painter().rect_filled(done, CornerRadius::same(1), m.primary);
        if playing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    let (glyph, name) = if playing { (glyphs::PAUSE, "Pause") } else { (glyphs::PLAY, "Play") };
    let play = m3::IconButton::new(glyph, name).style(m3::IconStyle::Filled).radius(shape::TILE_SMALL).show_at(ui, g.play.center(), ui.scope_id().with(("mini", "play")));
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
        assert_eq!(bar_title(&Page::Search("x".into()), 9), Some("Search"));
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
    fn the_top_bars_parts_do_not_overlap_and_fit_a_finger() {
        for screen in SCREENS {
            let bar = Rect::from_min_size(pos2(SIDE, 36.0), vec2(screen - 2.0 * SIDE, TOP_H));
            for (back, action, busy) in [(false, true, false), (false, true, true), (true, false, true), (false, false, false)] {
                let g = top_geom(bar, back, action, busy);
                let mut parts = vec![g.title, g.avatar];
                parts.extend(g.back);
                parts.extend(g.action);
                parts.extend(g.busy);
                for (i, a) in parts.iter().enumerate() {
                    for b in &parts[i + 1..] {
                        assert!(apart(*a, *b), "{screen} {back} {action} {busy}: {a:?} and {b:?}");
                    }
                }
                for target in [Some(g.avatar), g.back, g.action].into_iter().flatten() {
                    assert!(target.width() >= m3::TOUCH && target.height() >= m3::TOUCH);
                    assert!(target.left() >= 0.0 && target.right() <= screen, "on the screen: {target:?}");
                }
                assert_eq!(g.avatar.right(), bar.right());
                assert_eq!(g.back.is_some(), back);
                assert!(g.title.width() >= 180.0, "{screen}: the title keeps {}", g.title.width());
            }
        }
    }

    #[test]
    fn a_loading_page_takes_its_spinners_room_from_the_title() {
        let bar = Rect::from_min_size(pos2(SIDE, 36.0), vec2(380.0, TOP_H));
        let (idle, busy) = (top_geom(bar, false, true, false), top_geom(bar, false, true, true));
        assert!(idle.busy.is_none());
        assert!(busy.title.right() < idle.title.right());
        assert_eq!(busy.avatar, idle.avatar);
    }
}
