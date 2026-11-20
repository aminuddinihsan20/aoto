//! Settings on the phone: a first page of categories, each opening a page of its own
//! rows, and a box on the first page that looks through the rows of all of them. What
//! each row is and does is `pages::settings_row`; where it lives is decided here.
//! (The desktop keeps its one page, `pages::settings`.)

use egui::{Sense, Ui, pos2, vec2};

use super::widgets::{self, named};
use super::{Action, App, Page, m3, pages};
use crate::material::{self, Type, Weight, shape};
use crate::theme::icon;

/// A category of the phone's Settings: a row of the first page, and the page it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Account,
    Playback,
    Sound,
    Downloads,
    Lyrics,
    Appearance,
    Stats,
    Connections,
    Transfer,
    About,
}

impl Category {
    pub const ALL: [Category; 10] = [
        Category::Account,
        Category::Playback,
        Category::Sound,
        Category::Downloads,
        Category::Lyrics,
        Category::Appearance,
        Category::Stats,
        Category::Connections,
        Category::Transfer,
        Category::About,
    ];

    /// The first page's cards under the account's: the categories each one groups.
    pub const CARDS: [&'static [Category]; 3] = [
        &[Category::Playback, Category::Sound, Category::Downloads, Category::Lyrics],
        &[Category::Appearance, Category::Stats],
        &[Category::Connections, Category::Transfer, Category::About],
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Account => "Your account",
            Category::Playback => "Playback",
            Category::Sound => "Sound & quality",
            Category::Downloads => "Downloads",
            Category::Lyrics => "Lyrics",
            Category::Appearance => "Appearance",
            Category::Stats => "Listening stats",
            Category::Connections => "Connections",
            Category::Transfer => "Bring music from Spotify",
            Category::About => "About",
        }
    }

    fn icon(self) -> m3::Icon {
        match self {
            Category::Account => icon::USER.into(),
            Category::Playback => super::glyphs::PLAY.into(),
            Category::Sound => m3::Icon::Drawn(m3::drawn::tune),
            Category::Downloads => icon::DOWNLOAD.into(),
            Category::Lyrics => super::glyphs::LYRICS.into(),
            Category::Appearance => icon::SPARKLES.into(),
            Category::Stats => m3::Icon::Drawn(m3::drawn::bars),
            Category::Connections => icon::LINK.into(),
            Category::Transfer => m3::Icon::Drawn(m3::drawn::swap),
            Category::About => m3::Icon::Drawn(m3::drawn::info),
        }
    }

    /// Its rows, in the order its page shows them.
    pub fn rows(self) -> impl Iterator<Item = Row> {
        Row::ALL.into_iter().filter(move |r| r.place().0 == self)
    }

    /// The page a tap on its row opens: its own, but for the stats, which have theirs.
    pub fn page(self) -> Page {
        match self {
            Category::Stats => Page::Stats,
            other => Page::SettingsCategory(other),
        }
    }
}

/// Every setting the phone has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    Account,
    Channel,
    ListenTogether,
    ClearHistory,
    PlaybackSpeed,
    KeepPitch,
    Autoplay,
    Gapless,
    Automix,
    Crossfade,
    CrossfadeLength,
    SkipNonMusic,
    Equalizer,
    SpatialAudio,
    Normalize,
    LogVolume,
    HiRes,
    PreferAtmos,
    DisableDirect,
    PlaySongs,
    DownloadFolder,
    Romanization,
    Translation,
    TranslateInto,
    TextSize,
    TimingOffset,
    WordTrim,
    LineTrim,
    LyricsSources,
    Mode,
    AnimatedBackground,
    PulseWithMusic,
    AnimatedArtwork,
    StatsForNerds,
    SignatureService,
    ListeningStats,
    LastFm,
    TransferFromSpotify,
    SpotifyAccount,
    Version,
    Updates,
}

impl Row {
    pub const ALL: [Row; 41] = [
        Row::Account,
        Row::Channel,
        Row::ListenTogether,
        Row::ClearHistory,
        Row::PlaybackSpeed,
        Row::KeepPitch,
        Row::Autoplay,
        Row::Gapless,
        Row::Automix,
        Row::Crossfade,
        Row::CrossfadeLength,
        Row::SkipNonMusic,
        Row::Equalizer,
        Row::SpatialAudio,
        Row::Normalize,
        Row::LogVolume,
        Row::HiRes,
        Row::PreferAtmos,
        Row::DisableDirect,
        Row::PlaySongs,
        Row::DownloadFolder,
        Row::Romanization,
        Row::Translation,
        Row::TranslateInto,
        Row::TextSize,
        Row::TimingOffset,
        Row::WordTrim,
        Row::LineTrim,
        Row::LyricsSources,
        Row::Mode,
        Row::AnimatedBackground,
        Row::PulseWithMusic,
        Row::AnimatedArtwork,
        Row::StatsForNerds,
        Row::SignatureService,
        Row::ListeningStats,
        Row::LastFm,
        Row::TransferFromSpotify,
        Row::SpotifyAccount,
        Row::Version,
        Row::Updates,
    ];

    /// Where the row lives: its category, and the group of that category's page it is
    /// in ("" on a page that is one group).
    pub fn place(self) -> (Category, &'static str) {
        use Category as C;
        match self {
            Row::Account | Row::Channel | Row::ListenTogether => (C::Account, "Account"),
            Row::ClearHistory => (C::Account, "Listening history"),
            Row::PlaybackSpeed | Row::KeepPitch => (C::Playback, super::speed::SETTINGS_GROUP),
            Row::Autoplay | Row::Gapless | Row::Automix | Row::Crossfade | Row::CrossfadeLength | Row::SkipNonMusic => (C::Playback, "Between songs"),
            Row::Equalizer | Row::SpatialAudio | Row::Normalize | Row::LogVolume => (C::Sound, "Sound"),
            Row::HiRes | Row::PreferAtmos | Row::DisableDirect | Row::PlaySongs => (C::Sound, "Quality"),
            Row::DownloadFolder => (C::Downloads, ""),
            Row::Romanization | Row::Translation | Row::TranslateInto | Row::TextSize => (C::Lyrics, "Text"),
            Row::TimingOffset | Row::WordTrim | Row::LineTrim => (C::Lyrics, "Timing"),
            Row::LyricsSources => (C::Lyrics, "Sources"),
            Row::Mode => (C::Appearance, "Theme"),
            Row::AnimatedBackground | Row::PulseWithMusic | Row::AnimatedArtwork => (C::Appearance, "Motion"),
            Row::StatsForNerds => (C::Appearance, "More"),
            Row::SignatureService => (C::Appearance, "Artist pages"),
            Row::ListeningStats => (C::Stats, ""),
            Row::LastFm => (C::Connections, ""),
            Row::TransferFromSpotify | Row::SpotifyAccount => (C::Transfer, ""),
            Row::Version | Row::Updates => (C::About, ""),
        }
    }
}

/// A category's rows as its page groups them: each group's label and its rows.
pub fn groups(category: Category) -> Vec<(&'static str, Vec<Row>)> {
    let mut groups: Vec<(&'static str, Vec<Row>)> = Vec::new();
    for row in category.rows() {
        let label = row.place().1;
        match groups.last_mut() {
            Some((last, rows)) if *last == label => rows.push(row),
            _ => groups.push((label, vec![row])),
        }
    }
    groups
}

/// What the first page says under a category's name.
fn summary(app: &App, category: Category) -> String {
    match category {
        Category::Account => {
            let who = match (&app.account_name, app.logged_in) {
                (Some(name), true) => name.as_str(),
                (None, true) => "Signed in",
                _ => "Not signed in",
            };
            format!("{who} \u{00b7} Listen together, listening history")
        }
        Category::Playback => "Autoplay, Automix, crossfade".to_owned(),
        Category::Sound => "Equalizer, spatial audio, Hi-Res".to_owned(),
        Category::Downloads => "Where downloads are saved".to_owned(),
        Category::Lyrics => "Romanization, translation, text size".to_owned(),
        Category::Appearance => "Theme, motion".to_owned(),
        Category::Stats => "Your time, top songs and artists".to_owned(),
        Category::Connections => "Last.fm".to_owned(),
        Category::Transfer => "Playlists and liked songs".to_owned(),
        Category::About => format!("Version {}", crate::update::current_version()),
    }
}

/// What the top bar names the Settings page being shown.
pub fn bar_title(listen_together: bool, page: &Page) -> Option<&'static str> {
    match page {
        Page::Settings | Page::SettingsCategory(_) if listen_together => Some("Listen together"),
        Page::Settings => Some("Settings"),
        Page::SettingsCategory(c) => Some(c.title()),
        _ => None,
    }
}

/// Listen together takes the page to itself while it is open. True when it did.
fn listen_together(app: &mut App, ui: &mut Ui) -> bool {
    if !app.jam_open {
        return false;
    }
    if super::jam::subpage(app, ui) {
        app.jam_open = false;
    }
    true
}

/// The account's card at the head of the first page.
fn account_card(app: &mut App, ui: &mut Ui) {
    let m = material::scheme();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 76.0), Sense::click());
    let resp = named(resp, Category::Account.title());
    ui.painter().rect_filled(rect, shape::CARD, m.primary_container);
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, shape::CARD, m.on_primary_container.gamma_multiply(0.08));
    }
    let face = pos2(rect.left() + 18.0 + 22.0, rect.center().y);
    super::phone::cookie(ui, face, 23.0, m.primary);
    widgets::icon_at(ui, face, icon::USER, 20.0, m.on_primary);
    let left = face.x + 22.0 + 18.0;
    let ink = m.on_primary_container;
    m3::two_lines(ui, left, rect.center().y, rect.right() - 44.0 - left, Category::Account.title(), Type::TitleMedium.spec().weight(Weight::Bold), ink, &summary(app, Category::Account), Type::BodySmall, ink.gamma_multiply(0.85));
    widgets::icon_at(ui, pos2(rect.right() - 26.0, rect.center().y), icon::CHEVRON_RIGHT, 20.0, ink);
    if resp.clicked() {
        app.actions.push(Action::Go(Category::Account.page()));
    }
    ui.add_space(12.0);
}

/// The first page: the search box, then the account and the categories, or, once
/// something is typed, the rows of every category that say it.
pub fn first_page(app: &mut App, ui: &mut Ui) {
    if listen_together(app, ui) {
        return;
    }
    pages::ask_channels(app, ui);
    let m = material::scheme();
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), m3::SEARCH_BAR_H), Sense::hover());
    m3::search_bar(ui, bar, &mut app.settings_search, "Search settings", 0.0);
    ui.add_space(12.0);
    if pages::begin_settings_search(ui, &app.settings_search) {
        let top = ui.cursor().top();
        for category in Category::ALL {
            pages::search_group(ui, category.title());
            pages::settings_card(ui, |ui| {
                for row in category.rows() {
                    pages::settings_row(app, ui, row);
                }
            });
        }
        if ui.cursor().top() <= top {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(format!("Nothing in Settings matches \u{201c}{}\u{201d}.", app.settings_search.trim())).font(Type::BodyMedium.font()).color(m.on_surface_variant));
        }
    } else {
        account_card(app, ui);
        for card in Category::CARDS {
            m3::card(ui, None, m.surface_container, |ui| {
                for &category in card {
                    if m3::nav_row(ui, category.icon(), category.title(), &summary(app, category)).clicked() {
                        app.actions.push(Action::Go(category.page()));
                    }
                }
            });
            ui.add_space(12.0);
        }
    }
    pages::end_settings_search(ui);
}

/// A category's page: its rows, in their groups.
pub fn category_page(app: &mut App, ui: &mut Ui, category: Category) {
    if listen_together(app, ui) {
        return;
    }
    if category == Category::Account {
        pages::ask_channels(app, ui);
    }
    // Nothing is being looked for here, whatever the first page's box holds.
    pages::begin_settings_search(ui, "");
    if category == Category::Downloads {
        let m = material::scheme();
        let note = m3::galley_wrapped(ui, pages::DOWNLOADS_NOTE, Type::BodyMedium, m.on_surface_variant, ui.available_width() - 16.0, 6);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), note.size().y + 12.0), Sense::hover());
        ui.painter().galley(pos2(rect.left() + 8.0, rect.top()), note, m.on_surface_variant);
    }
    for (label, rows) in groups(category) {
        if !label.is_empty() {
            // The speed's group says the speed that plays beside its name.
            let value = (label == super::speed::SETTINGS_GROUP).then(|| super::speed::settings_value(app));
            pages::section_label(ui, label, value.as_deref());
        }
        pages::settings_card(ui, |ui| {
            for row in rows {
                pages::settings_row(app, ui, row);
            }
        });
    }
    pages::end_settings_search(ui);
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_setting_is_in_exactly_one_category() {
        let listed: HashSet<Row> = Row::ALL.into_iter().collect();
        assert_eq!(listed.len(), Row::ALL.len(), "no row is listed twice");
        let mut seen = HashSet::new();
        for category in Category::ALL {
            for row in category.rows() {
                assert!(seen.insert(row), "{row:?} is in two categories");
            }
        }
        assert_eq!(seen, listed, "every row is on some category's page");
    }

    #[test]
    fn the_settings_live_where_the_designs_put_them() {
        use Category as C;
        let rows = |c: Category| c.rows().collect::<Vec<_>>();
        assert_eq!(rows(C::Account), [Row::Account, Row::Channel, Row::ListenTogether, Row::ClearHistory]);
        assert_eq!(rows(C::Playback), [Row::PlaybackSpeed, Row::KeepPitch, Row::Autoplay, Row::Gapless, Row::Automix, Row::Crossfade, Row::CrossfadeLength, Row::SkipNonMusic]);
        assert_eq!(rows(C::Sound), [Row::Equalizer, Row::SpatialAudio, Row::Normalize, Row::LogVolume, Row::HiRes, Row::PreferAtmos, Row::DisableDirect, Row::PlaySongs]);
        assert_eq!(rows(C::Downloads), [Row::DownloadFolder]);
        assert_eq!(rows(C::Lyrics), [Row::Romanization, Row::Translation, Row::TranslateInto, Row::TextSize, Row::TimingOffset, Row::WordTrim, Row::LineTrim, Row::LyricsSources]);
        assert_eq!(rows(C::Appearance), [Row::Mode, Row::AnimatedBackground, Row::PulseWithMusic, Row::AnimatedArtwork, Row::StatsForNerds, Row::SignatureService]);
        assert_eq!(rows(C::Stats), [Row::ListeningStats]);
        assert_eq!(rows(C::Connections), [Row::LastFm]);
        assert_eq!(rows(C::Transfer), [Row::TransferFromSpotify, Row::SpotifyAccount]);
        assert_eq!(rows(C::About), [Row::Version, Row::Updates]);
    }

    #[test]
    fn the_first_page_lists_every_category_once_under_the_account() {
        let mut seen = vec![Category::Account];
        seen.extend(Category::CARDS.iter().flat_map(|card| card.iter().copied()));
        assert_eq!(seen, Category::ALL);
        let titles: HashSet<&str> = Category::ALL.iter().map(|c| c.title()).collect();
        assert_eq!(titles.len(), Category::ALL.len(), "no two categories share a name");
    }

    #[test]
    fn a_categorys_page_keeps_its_rows_in_their_groups() {
        assert_eq!(groups(Category::Sound), [("Sound", vec![Row::Equalizer, Row::SpatialAudio, Row::Normalize, Row::LogVolume]), ("Quality", vec![Row::HiRes, Row::PreferAtmos, Row::DisableDirect, Row::PlaySongs])]);
        assert_eq!(groups(Category::Playback), [("Playback speed", vec![Row::PlaybackSpeed, Row::KeepPitch]), ("Between songs", vec![Row::Autoplay, Row::Gapless, Row::Automix, Row::Crossfade, Row::CrossfadeLength, Row::SkipNonMusic])], "the speed and its pitch switch are a card of their own");
        assert_eq!(groups(Category::Downloads).len(), 1, "a page of one group");
        assert_eq!(groups(Category::Appearance).iter().map(|g| g.0).collect::<Vec<_>>(), ["Theme", "Motion", "More", "Artist pages"]);
        for category in Category::ALL {
            let grouped: Vec<Row> = groups(category).into_iter().flat_map(|g| g.1).collect();
            assert_eq!(grouped, category.rows().collect::<Vec<_>>(), "{category:?}: grouping loses or moves nothing");
        }
    }

    #[test]
    fn every_category_opens_its_own_page_but_the_stats_open_theirs() {
        assert_eq!(Category::Stats.page(), Page::Stats);
        for category in Category::ALL.into_iter().filter(|c| *c != Category::Stats) {
            assert_eq!(category.page(), Page::SettingsCategory(category));
        }
    }

    /// The phone's history: where it is, and the pages Back returns to.
    struct History {
        page: Page,
        back: Vec<Page>,
        forward: Vec<Page>,
    }

    impl History {
        fn at(page: Page) -> History {
            History { page, back: Vec::new(), forward: Vec::new() }
        }

        fn go(&mut self, to: Page) {
            super::super::visit(&mut self.page, &mut self.back, &mut self.forward, to);
        }

        /// Android's back button. False when it has nothing to go back to.
        fn back(&mut self) -> bool {
            super::super::step_back(&mut self.page, &mut self.back, &mut self.forward)
        }
    }

    #[test]
    fn back_goes_from_a_category_to_the_first_page_and_then_out_of_settings() {
        let mut h = History::at(Page::Home);
        h.go(Page::Settings);
        h.go(Category::Sound.page());
        assert_eq!(h.page, Page::SettingsCategory(Category::Sound));
        assert!(h.back());
        assert_eq!(h.page, Page::Settings);
        assert!(h.back());
        assert_eq!(h.page, Page::Home);
        assert!(!h.back(), "nothing left: the app goes to the background");
    }

    #[test]
    fn back_from_a_page_a_category_opened_returns_to_that_category() {
        let mut h = History::at(Page::Library(crate::ui::LibTab::Playlists));
        h.go(Page::Settings);
        h.go(Category::Sound.page());
        h.go(Page::Equalizer);
        assert!(h.back());
        assert_eq!(h.page, Page::SettingsCategory(Category::Sound));
        // The stats have a page of their own, opened from the first page.
        let mut h = History::at(Page::Settings);
        h.go(Category::Stats.page());
        assert!(h.back());
        assert_eq!(h.page, Page::Settings);
    }

    #[test]
    fn going_to_the_page_being_shown_leaves_the_history_alone() {
        let mut h = History::at(Page::Home);
        h.go(Page::Settings);
        h.go(Page::Settings);
        assert_eq!(h.back, [Page::Home]);
    }

    #[test]
    fn what_back_closes_first() {
        use super::super::BackStep;
        let step = super::super::back_step;
        assert_eq!(step(true, true, true), BackStep::NowPlaying, "the player comes down before anything else");
        assert_eq!(step(false, true, true), BackStep::ListenTogether, "then the section opened inside Settings");
        assert_eq!(step(false, true, false), BackStep::Page, "the desktop's Back is its history's");
        assert_eq!(step(false, false, true), BackStep::Page);
    }

    // -------------------------------------------------------------- search

    fn words(typed: &str) -> Vec<String> {
        typed.split_whitespace().map(str::to_lowercase).collect()
    }

    #[test]
    fn the_box_finds_a_row_by_any_of_its_words_in_any_order() {
        let row = "Spatial audio Makes stereo songs sound like they surround you, for headphones";
        assert!(pages::settings_hit(row, &words("spatial")));
        assert!(pages::settings_hit(row, &words("HEADPHONES stereo")));
        assert!(!pages::settings_hit(row, &words("spatial lyrics")), "every word has to be there");
        assert!(pages::settings_hit(row, &words("")), "nothing typed hides nothing");
    }

    /// Draw a searched Settings made of `rows` (category, title, description) in a
    /// headless egui the size of a phone, and return the titles that were drawn.
    fn searched(typed: &str, rows: &[(&str, &str, &str)]) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut drawn = Vec::new();
        for _ in 0..2 {
            drawn.clear();
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, 800.0))), ..Default::default() };
            let run = ctx.run_ui(input, |ui| {
                assert!(pages::begin_settings_search(ui, typed));
                let mut last = "";
                for (category, title, desc) in rows {
                    if *category != last {
                        pages::search_group(ui, category);
                        last = category;
                    }
                    if pages::settings_row_shown(ui, title, desc) {
                        drawn.push((*title).to_owned());
                    }
                }
                pages::end_settings_search(ui);
            });
            run.drop_without_applying_deltas();
        }
        drawn
    }

    const SOME: [(&str, &str, &str); 5] = [
        ("Playback", "Autoplay", "Keep playing similar songs when the queue ends."),
        ("Sound & quality", "Spatial audio", "Makes stereo songs sound like they surround you."),
        ("Sound & quality", "Hi-Res audio", "Addon servers that serve hi-res copies of songs."),
        ("Lyrics", "Romanization", "Japanese, Korean, Chinese in Latin letters."),
        ("Lyrics", "Translation", "Translated by Unison, or Google when it can't."),
    ];

    #[test]
    fn typing_shows_the_matching_rows_of_every_category() {
        assert_eq!(searched("songs", &SOME), ["Autoplay", "Spatial audio", "Hi-Res audio"], "rows of two categories");
        assert_eq!(searched("google", &SOME), ["Translation"]);
        assert!(searched("wallpaper", &SOME).is_empty());
    }

    #[test]
    fn a_categorys_name_brings_all_its_rows() {
        assert_eq!(searched("lyrics", &SOME), ["Romanization", "Translation"]);
        assert_eq!(searched("quality", &SOME), ["Spatial audio", "Hi-Res audio"]);
        // Not mixed: one word in the name and one in a row is neither.
        assert!(searched("lyrics autoplay", &SOME).is_empty());
    }

    #[test]
    fn after_the_search_the_rest_of_the_app_draws_every_row() {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, 800.0))), ..Default::default() };
        let run = ctx.run_ui(input, |ui| {
            pages::begin_settings_search(ui, "nothing says this");
            assert!(!pages::settings_row_shown(ui, "Autoplay", ""));
            pages::end_settings_search(ui);
            assert!(pages::settings_row_shown(ui, "Autoplay", ""));
        });
        run.drop_without_applying_deltas();
    }
}
