//! The phone frame: a top row (back and account), bottom navigation with four tabs, and
//! a mini player above it that opens Now Playing. The pages are the desktop's; only what
//! surrounds them differs (see `platform::phone`).

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use super::widgets::{self, named, text_at};
use super::{Action, App, LibTab, Page, glyphs};
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
    /// settings, transfer, the equalizer) stay with `last`, the tab they were opened from.
    pub fn of(page: &Page, last: Tab) -> Tab {
        match page {
            Page::Home | Page::New => Tab::Home,
            Page::Search(_) => Tab::Search,
            Page::Library(_) => Tab::Library,
            Page::Downloads | Page::LocalFolder(_) | Page::LocalAlbum(_) | Page::LocalSongs | Page::Downloaded(_) => Tab::Downloads,
            Page::Album(_) | Page::Playlist(_) | Page::Artist(_) | Page::Settings | Page::Transfer | Page::Equalizer => last,
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

/// The bottom navigation's height above the gesture area, and the mini player's.
pub const NAV_H: f32 = 64.0;
pub const MINI_H: f32 = 64.0;
/// The top row: the back button on the left, the account on the right.
pub const TOP_H: f32 = 48.0;
/// The gap between the screen's side and the pages.
pub const SIDE: f32 = 16.0;

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
        .frame(egui::Frame::new().fill(theme::panel(0.94)))
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
            pos2(screen.left() + 8.0 + insets.left, nav_top - MINI_H - 6.0),
            vec2(screen.width() - 16.0 - insets.left - insets.right, MINI_H),
        );
        egui::Area::new(ui.scope_id().with("phone-mini"))
            .fixed_pos(strip.min)
            .order(egui::Order::Middle)
            .show(&ctx, |ui| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| mini_player(app, ui, strip));
            });
    }
}

/// The top row: back when the page isn't a tab's own, the account on the right.
fn top_row(app: &mut App, ui: &mut Ui) {
    let panel = ui.max_rect();
    if !Tab::is_root(&app.page) {
        let r = Rect::from_min_size(pos2(panel.left() - 8.0, panel.top()), Vec2::splat(TOP_H));
        let resp = named(ui.interact(r, ui.scope_id().with("phone-back"), Sense::click()), "Back");
        widgets::icon_at(ui, r.center(), icon::CHEVRON_LEFT, 26.0, theme::text());
        if resp.clicked() {
            app.nav_back();
        }
    }
    account_chip(app, ui);
    ui.add_space(TOP_H - 2.0);
}

/// The signed-in person's picture and name (or "Sign in") in a frosted pill at the top
/// right; it opens Settings, where the account is.
fn account_chip(app: &mut App, ui: &mut Ui) {
    // The search field lives on the Search page: nothing here to anchor suggestions to.
    app.search_rect = None;
    let panel = ui.max_rect();
    // Dev hook: YTM_FAKE_ACCOUNT=<name> shows the signed-in look in screenshots.
    let fake = super::dev_var("YTM_FAKE_ACCOUNT");
    let signed_in = app.logged_in || fake.is_some();
    let label = match (signed_in, fake) {
        (true, Some(name)) => name.to_owned(),
        (true, None) => app.account_name.clone().unwrap_or_else(|| "Account".to_owned()),
        _ => "Sign in".to_owned(),
    };
    let galley = widgets::galley(ui, &label, theme::semibold(14.5), theme::text(), 150.0);
    let (avatar, pad) = (34.0, 4.0);
    let text_w = galley.size().x.min(150.0);
    let w = pad + avatar + 8.0 + text_w + 14.0;
    let h = avatar + 2.0 * pad;
    let chip = Rect::from_min_size(pos2(panel.right() - w, panel.top() + 4.0), vec2(w, h));
    let accent = app.accent_color();
    let loading = app.loading;
    let name_font = theme::semibold(14.5);
    let avatar_url = app.account_avatar.clone();
    let initial = label.chars().next().unwrap_or('?');
    // Its own layer, above the pages: a page's header art scrolls up into this corner.
    let clicked = egui::Area::new(ui.scope_id().with("account"))
        .order(egui::Order::Middle)
        .fixed_pos(chip.min)
        .show(ui.ctx(), |ui| {
            let (r, _) = ui.allocate_exact_size(chip.size(), Sense::hover());
            let resp = named(ui.interact(r, ui.scope_id().with("account-chip"), Sense::click()), if signed_in { "Account" } else { "Sign in" });
            ui.painter().rect_filled(r, h / 2.0, theme::glass(if resp.is_pointer_button_down_on() { 0.22 } else { 0.13 }));
            ui.painter().rect_stroke(r, h / 2.0, Stroke::new(1.0, theme::glass(0.12)), egui::StrokeKind::Inside);
            let pic = Rect::from_center_size(pos2(r.left() + pad + avatar / 2.0, r.center().y), Vec2::splat(avatar));
            match (&avatar_url, signed_in) {
                (Some(url), true) => {
                    ui.painter().circle_filled(pic.center(), avatar / 2.0, theme::with_alpha(accent, 0.5));
                    widgets::paint_remote(ui, url, pic, avatar / 2.0, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                (_, true) => {
                    ui.painter().circle_filled(pic.center(), avatar / 2.0, accent);
                    ui.painter().text(pic.center(), Align2::CENTER_CENTER, initial.to_uppercase(), theme::bold(16.0), Color32::WHITE);
                }
                _ => {
                    ui.painter().circle_filled(pic.center(), avatar / 2.0, theme::glass(0.16));
                    widgets::icon_at(ui, pic.center(), icon::USER, 18.0, theme::text_dim());
                }
            }
            widgets::text_at(ui, pos2(pic.right() + 8.0, r.center().y - galley.size().y / 2.0), &label, name_font.clone(), theme::text(), text_w + 2.0);
            resp.clicked()
        })
        .inner;
    if clicked {
        app.actions.push(Action::Go(Page::Settings));
    }
    if loading {
        // A page is loading: the spinner sits left of the pill.
        let spin = Rect::from_min_size(pos2(chip.left() - 30.0, chip.center().y - 10.0), Vec2::splat(20.0));
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(spin));
        cui.spinner();
    }
}

fn nav_bar(app: &mut App, ui: &mut Ui, insets: Insets) {
    let full = ui.max_rect();
    ui.painter().hline(full.x_range(), full.top(), Stroke::new(1.0, theme::glass(0.1)));
    let row = Rect::from_min_max(pos2(full.left() + insets.left, full.top()), pos2(full.right() - insets.right, full.top() + NAV_H));
    let w = row.width() / Tab::ALL.len() as f32;
    let accent = app.accent_color();
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let cell = Rect::from_min_size(pos2(row.left() + w * i as f32, row.top()), vec2(w, NAV_H));
        let resp = named(ui.interact(cell, ui.scope_id().with(("phone-tab", i)), Sense::click()), tab.label());
        let on = app.phone_tab == tab;
        // Apple's tab bar: the selected tab in the accent, the rest in secondary grey.
        let tint = if on { accent } else { Color32::from_rgb(142, 142, 147) };
        let tint = if resp.is_pointer_button_down_on() { theme::lerp_color(tint, Color32::WHITE, 0.25) } else { tint };
        glyphs::draw(ui, pos2(cell.center().x, cell.top() + 22.0), 25.0, tab.glyph(), tint);
        ui.painter().text(pos2(cell.center().x, cell.top() + 46.0), Align2::CENTER_CENTER, tab.label(), theme::semibold(10.5), tint);
        if resp.clicked() {
            app.actions.push(Action::Go(tab.page()));
        }
    }
}

/// The song playing, with play/pause and next; a tap anywhere else opens Now Playing.
fn mini_player(app: &mut App, ui: &mut Ui, rect: Rect) {
    let Some(track) = app.current().cloned() else { return };
    let playing = app.playing();
    let painter = ui.painter();
    widgets::soft_shadow(ui, rect, 16.0, 16.0, 0.4);
    painter.rect_filled(rect, CornerRadius::same(16), theme::panel(0.92));
    painter.rect_stroke(rect, CornerRadius::same(16), Stroke::new(1.0, theme::glass(0.14)), egui::StrokeKind::Inside);

    let open = named(ui.interact(rect, ui.scope_id().with("mini-open"), Sense::click()), "Open now playing");
    if open.clicked() {
        app.np_open = true;
    }

    let cover = Rect::from_center_size(pos2(rect.left() + 12.0 + 22.0, rect.center().y - 1.0), Vec2::splat(44.0));
    widgets::paint_cover(ui, cover, &track.thumbs, 8.0, 1.0);
    let btns = 2.0 * 48.0;
    let text_l = cover.right() + 12.0;
    let text_w = (rect.right() - btns - 4.0 - text_l).max(20.0);
    text_at(ui, pos2(text_l, rect.center().y - 19.0), &track.title, theme::semibold(15.0), theme::text(), text_w);
    text_at(ui, pos2(text_l, rect.center().y + 2.0), &track.artist_line(), theme::regular(13.0), theme::text_dim(), text_w);

    let tap = |ui: &mut Ui, x: f32, key: &str, name: &str| {
        let r = Rect::from_center_size(pos2(x, rect.center().y - 1.0), Vec2::splat(48.0));
        named(ui.interact(r, ui.scope_id().with(("mini", key)), Sense::click()), name)
    };
    let r = tap(ui, rect.right() - 4.0 - 72.0, "play", if playing { "Pause" } else { "Play" });
    glyphs::draw(ui, r.rect.center(), 30.0, if playing { glyphs::PAUSE } else { glyphs::PLAY }, theme::text());
    if r.clicked() {
        app.toggle();
    }
    let r = tap(ui, rect.right() - 4.0 - 24.0, "next", "Next");
    glyphs::draw(ui, r.rect.center(), 28.0, glyphs::NEXT, theme::text());
    if r.clicked() {
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
        for page in [Page::Album("a".into()), Page::Artist("a".into()), Page::Playlist("p".into()), Page::Settings, Page::Transfer, Page::Equalizer] {
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
        assert!(!Tab::is_root(&Page::New));
    }
}
