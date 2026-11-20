//! Listening stats: the player's side of the listening history (it tells `crate::stats`
//! what is audible at every look), the page that shows a week, a month or a year of
//! it, and its rows in Settings.

use std::sync::LazyLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use egui::{Align2, Color32, Rect, Sense, Ui, pos2, vec2};

use super::widgets::{self, named};
use super::{Action, App, Page, m3, notes, pages, sleep};
use crate::material::{self, Type, Weight, shape};
use crate::model::{Thumb, Track};
use crate::stats::{Clock, Cover, Date, Day, Pace, Period, Playing, Song, Summary, TopSong};
use crate::theme::{self, icon};

/// The page: the period it shows, and whether Settings is asking before it clears.
pub struct View {
    period: Period,
    /// The page's numbers, with what they were added up from (the history's revision,
    /// the day and the period): added up again only when one of those changes.
    shown: Option<((u64, Day, Period), Summary)>,
    clearing: bool,
}

impl Default for View {
    fn default() -> Self {
        Self { period: Period::Month, shown: None, clearing: false }
    }
}

/// Dev hook: YTM_STATS=week|year opens the page on that period.
pub(super) fn dev_hooks(app: &mut App) {
    match super::dev_var("YTM_STATS") {
        Some("week") => app.stats.period = Period::Week,
        Some("year") => app.stats.period = Period::Year,
        _ => {}
    }
}

/// The clock the history's seconds are counted on starts with the app.
static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);

fn clock() -> Clock {
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    Clock { steady: STARTED.elapsed().as_secs_f64(), unix, east: crate::ytmapi::utc_offset_minutes() }
}

/// Today, where the listener is.
pub(super) fn today() -> Day {
    let now = clock();
    crate::stats::local_day(now.unix, now.east)
}

/// A song as the history keeps it: its names, and the one picture a row needs.
fn song_of(track: &Track) -> Song {
    Song {
        id: track.id.clone(),
        title: track.title.clone(),
        artists: track.artists.iter().map(|a| a.name.clone()).collect(),
        album: track.album.as_ref().map(|a| a.name.clone()),
        cover: crate::model::pick_thumb(&track.thumbs, 120).map(|t| Cover { url: t.url.clone(), width: t.width, height: t.height }),
    }
}

/// The history's picture of a song, as the app's cover painter takes one.
fn thumbs_of(song: &Song) -> Vec<Thumb> {
    song.cover.iter().map(|c| Thumb { url: c.url.clone(), width: c.width, height: c.height }).collect()
}

impl App {
    /// Tell the listening history what is audible right now. Part of `pump`, so it
    /// goes on while the app is away.
    pub(super) fn tick_stats(&mut self) {
        // Taken out while it is told, so the song can be read from the player meanwhile.
        let mut log = std::mem::take(&mut self.listening);
        let status = &self.audio.status;
        // (the stream, the song, seconds into it, its length)
        let heard = if let Some(hold) = self.mix_hold() {
            // An Automix handoff: the outgoing song is the one heard while it is still
            // the louder, though the player has moved on to the next stream.
            status.outgoing_playing().then(|| (self.generation.wrapping_sub(1), &hold.track, self.mix_hold_pos(hold) as f64, Some(hold.total as f64)))
        } else if self.playing() && !self.buffering {
            let length = |t: &Track| status.duration().map(|d| d.as_secs_f64()).or(t.duration.map(f64::from));
            self.playing_track().map(|t| (self.generation, t, status.position().as_secs_f64(), length(t)))
        } else {
            None
        };
        match heard {
            Some((key, track, position, length)) => log.tick(clock(), Some(Playing { key, id: &track.id, position, length }), || song_of(track)),
            None => log.tick(clock(), None, || unreachable!("nothing plays")),
        }
        self.listening = log;
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// A length of listening: "42 hr 18 min", "18 min", and seconds under a minute.
fn time_text(secs: u64) -> String {
    match (secs / 3600, secs / 60 % 60) {
        (0, 0) if secs > 0 => format!("{secs} sec"),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{} hr", pages::thousands(hours)),
        (hours, minutes) => format!("{} hr {minutes} min", pages::thousands(hours)),
    }
}

/// "1 play", "386 plays".
fn plays_text(plays: u64) -> String {
    format!("{} play{}", pages::thousands(plays), if plays == 1 { "" } else { "s" })
}

/// What the period is called on its card.
fn period_name(period: Period, first: Date) -> String {
    match period {
        Period::Week => "This week".to_owned(),
        Period::Month => MONTHS[first.month as usize - 1].to_owned(),
        Period::Year => first.year.to_string(),
    }
}

/// What the period before is called in a comparison.
fn before_name(period: Period, before: Date) -> String {
    match period {
        Period::Week => "last week".to_owned(),
        Period::Month | Period::Year => period_name(period, before),
    }
}

/// The comparison in words. It is of the days that are over with as many of the
/// period before, which is what "ahead" and "behind" say.
fn pace_text(period: Period, before: Date, pace: Pace) -> String {
    let than = before_name(period, before);
    match pace.percent() {
        0 => format!("Level with {than}"),
        // Next to nothing was heard then: the percent would say little.
        more if more > 999 => format!("Far ahead of {than}"),
        more if more > 0 => format!("{more}% ahead of {than}"),
        less => format!("{}% behind {than}", -less),
    }
}

/// The line under the time: the comparison when there is one, and the plays.
fn sub_line(s: &Summary) -> String {
    if s.heard == 0 {
        let this = match s.period {
            Period::Week => "week",
            Period::Month => "month",
            Period::Year => "year",
        };
        return format!("Nothing played this {this} yet");
    }
    let plays = plays_text(s.plays as u64);
    match s.pace {
        Some(pace) => format!("{} \u{b7} {plays}", pace_text(s.period, s.before, pace)),
        None => plays,
    }
}

/// How much of the period's time went to its top album.
fn share_text(part: u64, whole: u64) -> String {
    match (part * 100 + whole / 2) / whole.max(1) {
        0 => "Under 1% of your time".to_owned(),
        percent => format!("{percent}% of your time"),
    }
}

/// What stands under a bar: the day's letter in a week, the month's in a year, and in
/// a month the date of every seventh day.
fn bar_label(period: Period, bar: usize) -> Option<String> {
    match period {
        Period::Week => Some(["M", "T", "W", "T", "F", "S", "S"][bar].to_owned()),
        Period::Month => bar.is_multiple_of(7).then(|| (bar + 1).to_string()),
        Period::Year => Some(MONTHS[bar][..1].to_owned()),
    }
}

/// How tall a bar is in a chart `full` tall whose tallest stands for `most` seconds;
/// never less than `least`, so a day with nothing still shows as a dot.
fn bar_height(secs: u64, most: u64, full: f32, least: f32) -> f32 {
    (full * secs as f32 / most.max(1) as f32).max(least)
}

/// "3 March 2026".
fn date_text(date: Date) -> String {
    format!("{} {} {}", date.day, MONTHS[date.month as usize - 1], date.year)
}

/// What Settings says is kept, on a device called `place` ("this phone").
fn history_note(plays: u64, since: Option<Date>, place: &str) -> String {
    let kept = format!("Counted on {place} for Listening stats and kept only here");
    match since {
        Some(day) => format!("{kept}: {} since {}.", plays_text(plays), date_text(day)),
        None => format!("{kept}. Nothing yet."),
    }
}

fn place() -> &'static str {
    if crate::platform::phone() { "this phone" } else { "this computer" }
}

fn period_id(period: Period) -> egui::Id {
    egui::Id::unique(("stats-period", period))
}

fn hero_id() -> egui::Id {
    egui::Id::unique("stats-hero")
}

/// The "Top artist" card (0) and the "Top album" card (1).
fn card_id(card: usize) -> egui::Id {
    egui::Id::unique(("stats-card", card))
}

fn song_id(rank: usize) -> egui::Id {
    egui::Id::unique(("stats-song", rank))
}

fn empty_id() -> egui::Id {
    egui::Id::unique("stats-empty")
}

fn clear_id() -> egui::Id {
    egui::Id::unique("stats-clear")
}

fn keep_id() -> egui::Id {
    egui::Id::unique("stats-keep")
}

/// Painted text has no name for a screen reader: this gives a part of the page one.
fn described(ui: &Ui, rect: Rect, id: egui::Id, text: &str) {
    ui.interact(rect, id, Sense::hover()).widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}

/// How tall the period pills are, and the card with the time and its bars.
const PILL_H: f32 = 44.0;
const HERO_H: f32 = 236.0;

/// Week, Month and Year as three pills, the one showing filled.
fn switch(ui: &mut Ui, period: &mut Period, accent: Color32) {
    let (w, gap) = (ui.available_width(), if m3::on() { 4.0 } else { 8.0 });
    let third = (w - 2.0 * gap) / 3.0;
    let (row, _) = ui.allocate_exact_size(vec2(w, PILL_H), Sense::hover());
    for (i, (p, label)) in [(Period::Week, "Week"), (Period::Month, "Month"), (Period::Year, "Year")].into_iter().enumerate() {
        let cell = Rect::from_min_size(pos2(row.left() + (third + gap) * i as f32, row.top()), vec2(third, PILL_H));
        if m3::on() {
            // Material: three connected buttons, the period showing a filled pill.
            let resp = named(ui.interact(cell, period_id(p), Sense::click()), label);
            m3::segment(ui, cell, label, &m3::Segment { on: *period == p, first: i == 0, last: i == 2, pressed: resp.is_pointer_button_down_on(), rest: material::scheme().surface_container_high, fade: 1.0 });
            if resp.clicked() {
                *period = p;
            }
        } else if sleep::pill(ui, cell, period_id(p), label, None, *period == p, false, accent) {
            *period = p;
        }
    }
}

/// The period's time in a card tinted with the accent: its name, the time, how it
/// compares, and a bar for each day (each month of a year).
fn hero(ui: &mut Ui, s: &Summary, accent: Color32) {
    let w = ui.available_width();
    let (card, _) = ui.allocate_exact_size(vec2(w, HERO_H), Sense::hover());
    let (left, text_w) = (card.left() + 24.0, w - 48.0);
    let (name, time, sub) = (period_name(s.period, s.first), time_text(s.heard), sub_line(s));
    described(ui, card, hero_id(), &format!("{name}: {time}. {sub}"));
    if m3::on() {
        // Material: a tonal card in the primary container, its text in the colour for it.
        let m = material::scheme();
        ui.painter().rect_filled(card, shape::CARD, m.primary_container);
        let ink = m.on_primary_container;
        m3::text(ui, pos2(left, card.top() + 22.0), &name.to_uppercase(), Type::LabelLarge, ink.gamma_multiply(0.8), text_w);
        m3::text(ui, pos2(left, card.top() + 40.0), &time, Type::DisplaySmall.spec().weight(Weight::Bold), ink, text_w);
        m3::text(ui, pos2(left, card.top() + 90.0), &sub, Type::BodyMedium, ink.gamma_multiply(0.8), text_w);
        bars(ui, Rect::from_min_max(pos2(left, card.top() + 124.0), pos2(card.right() - 24.0, card.bottom() - 18.0)), s, accent);
        return;
    }
    ui.painter().rect_filled(card, theme::panel_radius(), theme::with_alpha(accent, 0.30));
    widgets::text_at(ui, pos2(left, card.top() + 22.0), &name.to_uppercase(), theme::semibold(12.0), theme::text_dim(), text_w);
    widgets::text_at(ui, pos2(left, card.top() + 40.0), &time, theme::bold(36.0), theme::text(), text_w);
    widgets::text_at(ui, pos2(left, card.top() + 90.0), &sub, theme::regular(14.0), theme::text_dim(), text_w);
    bars(ui, Rect::from_min_max(pos2(left, card.top() + 124.0), pos2(card.right() - 24.0, card.bottom() - 18.0)), s, accent);
}

/// The period's bars across `area`, today's (this month's) in the accent, with what
/// each stands for underneath.
fn bars(ui: &Ui, area: Rect, s: &Summary, accent: Color32) {
    let chart = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - 18.0));
    let slot = area.width() / s.bars.len() as f32;
    let bar_w = (slot * 0.62).min(14.0);
    let most = s.bars.iter().copied().max().unwrap_or(0);
    for (i, &secs) in s.bars.iter().enumerate() {
        let x = area.left() + slot * (i as f32 + 0.5);
        let color = if m3::on() {
            // On the primary container: today in the primary colour, the rest in the
            // container's text colour, fainter where nothing was heard or is yet to come.
            let m = material::scheme();
            match (i == s.today, secs > 0) {
                (true, _) => m.primary,
                (false, true) => m.on_primary_container.gamma_multiply(0.3),
                (false, false) => m.on_primary_container.gamma_multiply(if i > s.today { 0.08 } else { 0.14 }),
            }
        } else if i == s.today {
            theme::lerp_color(accent, theme::text(), 0.3)
        } else if secs > 0 {
            theme::with_alpha(theme::text(), 0.30)
        } else {
            // Nothing heard; fainter still for a day that hasn't come.
            theme::with_alpha(theme::text(), if i > s.today { 0.08 } else { 0.14 })
        };
        let top = chart.bottom() - bar_height(secs, most, chart.height(), bar_w);
        ui.painter().rect_filled(Rect::from_x_y_ranges(x - bar_w / 2.0..=x + bar_w / 2.0, top..=chart.bottom()), bar_w / 2.0, color);
        if let Some(label) = bar_label(s.period, i) {
            let ink = if m3::on() { material::scheme().on_primary_container.gamma_multiply(0.8) } else { theme::text_dim() };
            ui.painter().text(pos2(x, area.bottom()), Align2::CENTER_BOTTOM, label, theme::regular(10.5), ink);
        }
    }
}

/// The top artist and the top album side by side; one of them alone takes the row.
fn top_cards(ui: &mut Ui, s: &Summary) {
    let artist = s.top_artist.as_ref().map(|t| (0, "Top artist", t.name.as_str(), time_text(t.heard)));
    let album = s.top_album.as_ref().map(|t| (1, "Top album", t.name.as_str(), share_text(t.heard, s.heard)));
    let cards: Vec<_> = artist.into_iter().chain(album).collect();
    if cards.is_empty() {
        return;
    }
    let (w, gap) = (ui.available_width(), 10.0);
    let each = (w - gap * (cards.len() - 1) as f32) / cards.len() as f32;
    let (row, _) = ui.allocate_exact_size(vec2(w, 108.0), Sense::hover());
    for (i, (card, label, name, note)) in cards.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(row.left() + (each + gap) * i as f32, row.top()), vec2(each, row.height()));
        described(ui, r, card_id(card), &format!("{label}: {name}, {note}"));
        let (left, text_w) = (r.left() + 18.0, each - 36.0);
        if m3::on() {
            // Material: a card with one of its shapes in a corner, the text over it.
            let m = material::scheme();
            ui.painter().rect_filled(r, shape::CARD, m.surface_container);
            let corner = pos2(r.right() - 38.0, r.bottom() - 34.0);
            if card == 0 {
                super::phone::cookie(ui, corner, 26.0, m.tertiary_container);
            } else {
                ui.painter().rect_filled(Rect::from_center_size(corner, vec2(48.0, 48.0)), shape::TILE, m.secondary_container);
            }
            m3::text(ui, pos2(left, r.top() + 18.0), &label.to_uppercase(), Type::LabelMedium, m.on_surface_variant, text_w);
            m3::text(ui, pos2(left, r.top() + 38.0), name, Type::TitleLarge.spec().weight(Weight::Medium), m.on_surface, text_w);
            m3::text(ui, pos2(left, r.top() + 70.0), &note, Type::BodyMedium, m.on_surface_variant, text_w);
            continue;
        }
        ui.painter().rect_filled(r, theme::panel_radius(), theme::glass(0.07));
        widgets::text_at(ui, pos2(left, r.top() + 18.0), &label.to_uppercase(), theme::semibold(11.5), theme::text_dim(), text_w);
        widgets::text_at(ui, pos2(left, r.top() + 38.0), name, theme::semibold(20.0), theme::text(), text_w);
        widgets::text_at(ui, pos2(left, r.top() + 70.0), &note, theme::regular(13.5), theme::text_dim(), text_w);
    }
    ui.add_space(22.0);
}

/// The most played songs, each with its place, its cover and how often it played.
fn top_songs(ui: &mut Ui, songs: &[TopSong], accent: Color32) {
    if songs.is_empty() {
        return;
    }
    let w = ui.available_width();
    let (head, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    let material = m3::on();
    if material {
        m3::text(ui, pos2(head.left() + 2.0, head.center().y - 12.0), "Top songs", Type::TitleMedium.spec().weight(Weight::Bold), material::scheme().on_surface, w);
    } else {
        ui.painter().text(pos2(head.left() + 2.0, head.center().y), Align2::LEFT_CENTER, "Top songs", theme::bold(20.0), theme::text());
    }
    ui.add_space(6.0);
    for (i, top) in songs.iter().enumerate() {
        let (row, _) = ui.allocate_exact_size(vec2(w, 64.0), Sense::hover());
        let (artists, plays) = (top.song.artists.join(", "), plays_text(top.plays as u64));
        described(ui, row, song_id(i), &format!("{}. {}, {artists}, {plays}", i + 1, top.song.title));
        if material {
            // Material: the place in the primary colour, then a list row's cover and lines.
            let m = material::scheme();
            let rank = m3::galley(ui, &(i + 1).to_string(), Type::HeadlineSmall, m.primary, 30.0);
            ui.painter().galley(pos2(row.left() + 14.0 - rank.size().x / 2.0, row.center().y - rank.size().y / 2.0), rank, m.primary);
            let cover = Rect::from_center_size(pos2(row.left() + 34.0 + 24.0, row.center().y), vec2(48.0, 48.0));
            widgets::paint_cover(ui, cover, &thumbs_of(&top.song), shape::TILE_SMALL, 1.0);
            let plays_g = m3::galley(ui, &plays, Type::LabelLarge, m.on_surface_variant, 120.0);
            let left = cover.right() + 16.0;
            let text_w = (row.right() - plays_g.size().x - 14.0 - left).max(20.0);
            ui.painter().galley(pos2(row.right() - 2.0 - plays_g.size().x, row.center().y - plays_g.size().y / 2.0), plays_g, m.on_surface_variant);
            if artists.is_empty() {
                // A song that names nobody has its title to itself, in the middle.
                let g = m3::galley(ui, &top.song.title, Type::BodyLarge, m.on_surface, text_w);
                ui.painter().galley(pos2(left, row.center().y - g.size().y / 2.0), g, m.on_surface);
            } else {
                m3::two_lines(ui, left, row.center().y, text_w, &top.song.title, Type::BodyLarge, m.on_surface, &artists, Type::BodyMedium, m.on_surface_variant);
            }
            continue;
        }
        ui.painter().text(pos2(row.left() + 14.0, row.center().y), Align2::CENTER_CENTER, (i + 1).to_string(), theme::semibold(20.0), accent);
        let cover = Rect::from_center_size(pos2(row.left() + 34.0 + 24.0, row.center().y), vec2(48.0, 48.0));
        widgets::paint_cover(ui, cover, &thumbs_of(&top.song), theme::thumb_radius(), 1.0);
        let plays_w = ui.painter().text(pos2(row.right() - 2.0, row.center().y), Align2::RIGHT_CENTER, plays, theme::regular(13.5), theme::text_dim()).width();
        let left = cover.right() + 14.0;
        let text_w = (row.right() - plays_w - 14.0 - left).max(20.0);
        // A song that names nobody has its title to itself, in the middle.
        let title_y = if artists.is_empty() { row.center().y - 10.0 } else { row.center().y - 20.0 };
        widgets::text_at(ui, pos2(left, title_y), &top.song.title, theme::regular(16.0), theme::text(), text_w);
        widgets::text_at(ui, pos2(left, row.center().y + 2.0), &artists, theme::regular(13.5), theme::text_dim(), text_w);
    }
}

/// Nothing was ever heard here: no numbers, and how to get some.
fn nothing_yet(ui: &mut Ui, accent: Color32) {
    const TITLE: &str = "No stats yet";
    const NOTE: &str = "Play something and your stats appear here";
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 220.0), Sense::hover());
    described(ui, r, empty_id(), &format!("{TITLE}. {NOTE}"));
    let disc = pos2(r.center().x, r.top() + 76.0);
    if m3::on() {
        let m = material::scheme();
        ui.painter().circle_filled(disc, 44.0, m.primary_container);
        widgets::icon_at(ui, disc, icon::HEADPHONES, 38.0, m.on_primary_container);
        let title = m3::galley(ui, TITLE, Type::TitleLarge, m.on_surface, w - 32.0);
        ui.painter().galley(pos2(r.center().x - title.size().x / 2.0, r.top() + 138.0), title, m.on_surface);
        let note = m3::galley_centered(ui, NOTE, Type::BodyMedium, m.on_surface_variant, w - 32.0, 2);
        ui.painter().galley(pos2(r.center().x, r.top() + 170.0), note, m.on_surface_variant);
        return;
    }
    ui.painter().circle_filled(disc, 44.0, theme::with_alpha(accent, 0.22));
    widgets::icon_at(ui, disc, icon::HEADPHONES, 38.0, theme::text());
    ui.painter().text(pos2(r.center().x, r.top() + 152.0), Align2::CENTER_CENTER, TITLE, theme::bold(20.0), theme::text());
    let note = widgets::galley_wrapped(ui, NOTE, theme::regular(14.0), theme::text_dim(), w - 32.0, 2);
    ui.painter().galley(pos2(r.center().x - note.size().x / 2.0, r.top() + 170.0), note, theme::text_dim());
}

/// The page: `summary` is the period's numbers, None while nothing was ever heard.
fn contents(ui: &mut Ui, period: &mut Period, summary: Option<&Summary>, accent: Color32) {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    // The Material phone names the page in its top bar.
    if m3::on() {
        ui.add_space(8.0);
    } else {
        pages::page_title(ui, "Listening stats");
        ui.add_space(14.0);
    }
    let Some(s) = summary else { return nothing_yet(ui, accent) };
    switch(ui, period, accent);
    ui.add_space(16.0);
    hero(ui, s, accent);
    ui.add_space(12.0);
    top_cards(ui, s);
    top_songs(ui, &s.top_songs, accent);
}

/// The page, in the app.
pub(super) fn page(app: &mut App, ui: &mut Ui) {
    let key = (app.listening.revision(), today(), app.stats.period);
    if app.stats.shown.as_ref().is_none_or(|(of, _)| *of != key) {
        app.stats.shown = Some((key, app.listening.history().summary(key.2, key.1)));
    }
    let summary = app.stats.shown.as_ref().map(|(_, s)| s).filter(|_| !app.listening.history().is_empty());
    let (accent, mut period) = (app.accent_color(), app.stats.period);
    // One column, as on a phone, also where there is room for more.
    let w = ui.available_width().min(520.0);
    ui.scope(|ui| {
        ui.set_max_width(w);
        contents(ui, &mut period, summary, accent);
    });
    app.stats.period = period;
}

/// The way to the page, a row of Settings › Account's card.
pub(super) fn account_row(app: &mut App, ui: &mut Ui) {
    pages::divider(ui);
    stats_row(app, ui);
}

/// The row that opens the page.
pub(super) fn stats_row(app: &mut App, ui: &mut Ui) {
    if pages::open_row(ui, "Listening stats", &format!("How long you listened this week, this month and this year, with your top artist and songs. Counted on {}.", place())) {
        app.actions.push(Action::Go(Page::Stats));
    }
}

/// Settings › Listening history: what is kept, and the way to clear it.
pub(super) fn settings_section(app: &mut App, ui: &mut Ui) {
    pages::section_label(ui, "Listening history", None);
    pages::settings_card(ui, |ui| clear_row(app, ui));
}

/// The row that clears the history, and the question it asks first.
pub(super) fn clear_row(app: &mut App, ui: &mut Ui) {
    let history = app.listening.history();
    let (empty, note) = (history.is_empty(), history_note(history.plays(), history.since().map(Date::of), place()));
    pages::setting_row(ui, "Clear listening history", Some(&note), |ui| {
        if !empty && widgets::pill(ui, "Clear", Some(icon::TRASH_2), false).clicked() {
            app.stats.clearing = true;
        }
    });
    if app.stats.clearing {
        match confirm_clear(ui.ctx(), &note) {
            Some(true) => {
                app.listening.clear();
                app.stats.clearing = false;
                app.toast("Listening history cleared");
            }
            Some(false) => app.stats.clearing = false,
            None => {}
        }
    }
}

/// "Clear listening history?" as the app's modal card: Some(true) clear, Some(false)
/// keep, None still open.
fn confirm_clear(ctx: &egui::Context, kept: &str) -> Option<bool> {
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::unique("confirm-clear-stats")).frame(notes::surface()).backdrop_color(theme::shade(0.55)).show(ctx, |ui| {
        ui.set_width(400.0_f32.min(ctx.content_rect().width() - 48.0));
        egui::Frame::new().inner_margin(egui::Margin { left: 26, right: 26, top: 24, bottom: 22 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let body = format!("{kept} Listening stats starts again from nothing. This can't be undone.");
            if m3::on() {
                m3::dialog_title(ui, "Clear listening history?");
                ui.add_space(16.0);
                m3::dialog_text(ui, &body);
                ui.add_space(24.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if m3::button(ui, "Clear", None, m3::ButtonStyle::Danger).clicked() {
                        answer = Some(true);
                    }
                    if m3::button(ui, "Cancel", None, m3::ButtonStyle::Text).clicked() {
                        answer = Some(false);
                    }
                });
                return;
            }
            ui.label(egui::RichText::new("Clear listening history?").font(theme::bold(22.0)).color(theme::text()));
            ui.add_space(10.0);
            ui.add(egui::Label::new(egui::RichText::new(body).font(theme::regular(14.0)).color(theme::text_dim())).wrap());
            ui.add_space(22.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                // The one that can't be taken back, in red.
                for (id, label, fill) in [(clear_id(), "Clear", Some(theme::RED)), (keep_id(), "Cancel", None)] {
                    let ink = if fill.is_some() { Color32::WHITE } else { theme::text() };
                    let g = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(14.0), ink);
                    let (rect, _) = ui.allocate_exact_size(vec2(g.size().x + 40.0, 44.0), Sense::hover());
                    let resp = named(ui.interact(rect, id, Sense::click()), label);
                    let press = resp.is_pointer_button_down_on();
                    let fill = match fill {
                        Some(red) => theme::lerp_color(red, Color32::BLACK, if press { 0.12 } else { 0.0 }),
                        None => theme::glass(if press { 0.18 } else { 0.10 }),
                    };
                    ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
                    ui.painter().galley(rect.center() - g.size() / 2.0, g, ink);
                    if resp.clicked() {
                        answer = Some(id == clear_id());
                    }
                }
            });
        });
    });
    if answer.is_none() && (modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
        answer = Some(false);
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Link;
    use crate::stats::Top;
    use egui::{FontFamily, Pos2};

    fn date(year: i32, month: u32, day: u32) -> Date {
        Date { year, month, day }
    }

    #[test]
    fn a_time_reads_in_hours_and_minutes() {
        assert_eq!(time_text(42 * 3600 + 18 * 60 + 31), "42 hr 18 min");
        assert_eq!(time_text(6 * 3600 + 40 * 60), "6 hr 40 min");
        assert_eq!(time_text(3600), "1 hr");
        assert_eq!(time_text(3599), "59 min");
        assert_eq!(time_text(60), "1 min");
        assert_eq!(time_text(1204 * 3600 + 60), "1,204 hr 1 min");
    }

    #[test]
    fn under_a_minute_it_reads_in_seconds() {
        assert_eq!(time_text(59), "59 sec");
        assert_eq!(time_text(1), "1 sec");
        assert_eq!(time_text(0), "0 min");
    }

    #[test]
    fn plays_are_counted_in_words() {
        assert_eq!(plays_text(0), "0 plays");
        assert_eq!(plays_text(1), "1 play");
        assert_eq!(plays_text(47), "47 plays");
        assert_eq!(plays_text(12_345), "12,345 plays");
    }

    #[test]
    fn a_period_is_called_by_its_name() {
        assert_eq!(period_name(Period::Week, date(2026, 11, 16)), "This week");
        assert_eq!(period_name(Period::Month, date(2026, 11, 1)), "November");
        assert_eq!(period_name(Period::Year, date(2026, 1, 1)), "2026");
        assert_eq!(before_name(Period::Week, date(2026, 11, 9)), "last week");
        assert_eq!(before_name(Period::Month, date(2026, 10, 1)), "October");
        assert_eq!(before_name(Period::Month, date(2025, 12, 1)), "December");
        assert_eq!(before_name(Period::Year, date(2025, 1, 1)), "2025");
    }

    #[test]
    fn the_comparison_says_ahead_of_or_behind_the_period_before() {
        let october = date(2026, 10, 1);
        assert_eq!(pace_text(Period::Month, october, Pace { now: 6720, then: 6000 }), "12% ahead of October");
        assert_eq!(pace_text(Period::Month, october, Pace { now: 4500, then: 6000 }), "25% behind October");
        assert_eq!(pace_text(Period::Month, october, Pace { now: 6010, then: 6000 }), "Level with October");
        assert_eq!(pace_text(Period::Week, date(2026, 11, 9), Pace { now: 12_000, then: 6000 }), "100% ahead of last week");
        assert_eq!(pace_text(Period::Year, date(2025, 1, 1), Pace { now: 0, then: 6000 }), "100% behind 2025");
        // A period before with next to nothing in it: no five-figure percents.
        assert_eq!(pace_text(Period::Month, october, Pace { now: 66_000, then: 6000 }), "Far ahead of October");
        assert_eq!(pace_text(Period::Month, october, Pace { now: 65_940, then: 6000 }), "999% ahead of October");
    }

    fn a_summary() -> Summary {
        let song = |id: &str, title: &str, artist: &str| Song { id: id.to_owned(), title: title.to_owned(), artists: vec![artist.to_owned()], album: None, cover: None };
        let mut bars = vec![0; 30];
        for (i, bar) in bars.iter_mut().enumerate().take(20) {
            *bar = 3000 + (i as u64 * 7919) % 9000;
        }
        Summary {
            period: Period::Month,
            first: date(2026, 11, 1),
            before: date(2026, 10, 1),
            heard: 42 * 3600 + 18 * 60,
            plays: 386,
            bars,
            today: 19,
            pace: Some(Pace { now: 6720, then: 6000 }),
            top_artist: Some(Top { name: "Senja Kala".to_owned(), heard: 6 * 3600 + 40 * 60 }),
            top_album: Some(Top { name: "Layang-Layang".to_owned(), heard: 16 * 3600 }),
            top_songs: vec![
                TopSong { song: song("1", "Hujan di Kemang", "Senja Kala"), plays: 47, heard: 9000 },
                TopSong { song: song("2", "Midnight Angkot", "Rona Jingga"), plays: 39, heard: 8000 },
                TopSong { song: song("3", "Layang-Layang", "Arunika"), plays: 31, heard: 7000 },
            ],
        }
    }

    #[test]
    fn under_the_time_stand_the_comparison_and_the_plays() {
        let mut s = a_summary();
        assert_eq!(sub_line(&s), "12% ahead of October \u{b7} 386 plays");
        s.pace = None;
        assert_eq!(sub_line(&s), "386 plays");
    }

    #[test]
    fn a_period_with_nothing_in_it_says_so() {
        let mut s = a_summary();
        (s.heard, s.plays, s.pace) = (0, 0, Some(Pace { now: 0, then: 6000 }));
        assert_eq!(sub_line(&s), "Nothing played this month yet");
        assert_eq!(sub_line(&Summary { period: Period::Week, ..s.clone() }), "Nothing played this week yet");
        assert_eq!(sub_line(&Summary { period: Period::Year, ..s }), "Nothing played this year yet");
    }

    #[test]
    fn the_top_album_says_its_share_of_the_time() {
        assert_eq!(share_text(38, 100), "38% of your time");
        assert_eq!(share_text(16 * 3600, 42 * 3600 + 18 * 60), "38% of your time");
        assert_eq!(share_text(500, 500), "100% of your time");
        assert_eq!(share_text(1, 1000), "Under 1% of your time");
    }

    #[test]
    fn bars_are_marked_by_day_date_or_month() {
        let week: Vec<_> = (0..7).map(|i| bar_label(Period::Week, i).unwrap()).collect();
        assert_eq!(week, ["M", "T", "W", "T", "F", "S", "S"]);
        let year: Vec<_> = (0..12).map(|i| bar_label(Period::Year, i).unwrap()).collect();
        assert_eq!(year, ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"]);
        let month: Vec<_> = (0..31).filter_map(|i| bar_label(Period::Month, i)).collect();
        assert_eq!(month, ["1", "8", "15", "22", "29"]);
    }

    #[test]
    fn a_bar_is_as_tall_as_its_share_of_the_tallest() {
        assert_eq!(bar_height(9000, 9000, 80.0, 6.0), 80.0);
        assert_eq!(bar_height(4500, 9000, 80.0, 6.0), 40.0);
        // A little, and nothing, still show.
        assert_eq!(bar_height(90, 9000, 80.0, 6.0), 6.0);
        assert_eq!(bar_height(0, 9000, 80.0, 6.0), 6.0);
        assert_eq!(bar_height(0, 0, 80.0, 6.0), 6.0);
    }

    #[test]
    fn settings_says_what_is_kept_and_since_when() {
        assert_eq!(date_text(date(2026, 3, 3)), "3 March 2026");
        assert_eq!(
            history_note(412, Some(date(2026, 3, 3)), "this phone"),
            "Counted on this phone for Listening stats and kept only here: 412 plays since 3 March 2026."
        );
        assert_eq!(
            history_note(0, Some(date(2026, 11, 20)), "this phone"),
            "Counted on this phone for Listening stats and kept only here: 0 plays since 20 November 2026."
        );
        assert_eq!(history_note(0, None, "this computer"), "Counted on this computer for Listening stats and kept only here. Nothing yet.");
    }

    fn thumb(width: u32) -> Thumb {
        Thumb { url: format!("https://lh3.googleusercontent.com/x=w{width}-h{width}-l90-rj"), width, height: width }
    }

    #[test]
    fn a_song_is_kept_with_its_names_and_one_picture() {
        let track = Track {
            id: "abc".to_owned(),
            title: "Hujan di Kemang".to_owned(),
            artists: vec![Link { name: "Senja Kala".to_owned(), id: Some("UC1".to_owned()) }, Link { name: "Rona Jingga".to_owned(), id: None }],
            album: Some(Link { name: "Layang-Layang".to_owned(), id: Some("MPRE1".to_owned()) }),
            duration: Some(200),
            thumbs: vec![thumb(60), thumb(226), thumb(544)],
            track_nr: None,
            plays: None,
            music_video: false,
        };
        let song = song_of(&track);
        assert_eq!((song.id.as_str(), song.title.as_str()), ("abc", "Hujan di Kemang"));
        assert_eq!(song.artists, ["Senja Kala", "Rona Jingga"]);
        assert_eq!(song.album.as_deref(), Some("Layang-Layang"));
        // The smallest that fills a row; the image host sizes it on request.
        assert_eq!(song.cover, Some(Cover { url: thumb(226).url, width: 226, height: 226 }));
        assert_eq!(thumbs_of(&song), [thumb(226)]);
        // A file on the phone has neither album link nor picture.
        let local = song_of(&Track { album: None, thumbs: Vec::new(), ..track });
        assert_eq!((&local.album, &local.cover), (&None, &None));
        assert!(thumbs_of(&local).is_empty());
    }

    /// The page's contents in a headless egui, 380 points wide, with stand-ins for the
    /// app's fonts.
    struct Shown {
        ctx: egui::Context,
        period: Period,
        summary: Option<Summary>,
        time: f64,
    }

    fn headless() -> egui::Context {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        let plain = fonts.families[&FontFamily::Proportional].clone();
        for family in ["semibold", "bold", "icons"] {
            fonts.families.insert(FontFamily::Name(family.into()), plain.clone());
        }
        ctx.set_fonts(fonts);
        ctx
    }

    fn screen(time: f64, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(380.0, 900.0))), time: Some(time), events, ..Default::default() }
    }

    fn press(at: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    impl Shown {
        fn new(summary: Option<Summary>) -> Shown {
            let mut s = Shown { ctx: headless(), period: Period::Month, summary, time: 0.0 };
            s.frame(vec![]);
            s.frame(vec![]);
            s
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let out = self.ctx.run_ui(screen(self.time, events), |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| contents(ui, &mut self.period, self.summary.as_ref(), theme::RED));
            });
            out.drop_without_applying_deltas();
        }

        fn rect(&self, id: egui::Id) -> Option<Rect> {
            self.ctx.read_response(id).map(|r| r.rect)
        }

        fn tap(&mut self, id: egui::Id) {
            let at = self.rect(id).expect("it is on the page").center();
            self.frame(vec![egui::Event::PointerMoved(at), press(at, true)]);
            self.frame(vec![press(at, false)]);
            self.frame(vec![]);
        }
    }

    #[test]
    fn the_page_opens_on_the_month() {
        assert_eq!(View::default().period, Period::Month);
    }

    #[test]
    fn each_period_can_be_picked() {
        let mut s = Shown::new(Some(a_summary()));
        for period in [Period::Week, Period::Year, Period::Month] {
            s.tap(period_id(period));
            assert_eq!(s.period, period);
        }
    }

    #[test]
    fn the_periods_stand_side_by_side_and_are_big_enough_to_tap() {
        let s = Shown::new(Some(a_summary()));
        let rects = [Period::Week, Period::Month, Period::Year].map(|p| s.rect(period_id(p)).expect("drawn"));
        for (i, r) in rects.iter().enumerate() {
            assert!(r.height() >= 44.0 && r.width() >= 44.0, "{r:?}");
            assert!(r.left() >= 0.0 && r.right() <= 380.5, "{r:?}");
            assert!(i == 0 || rects[i - 1].right() < r.left(), "{rects:?}");
        }
    }

    #[test]
    fn a_period_shows_its_time_its_tops_and_its_songs() {
        let s = Shown::new(Some(a_summary()));
        let hero = s.rect(hero_id()).expect("the time's card");
        let (artist, album) = (s.rect(card_id(0)).expect("top artist"), s.rect(card_id(1)).expect("top album"));
        // The two cards share a row under the time.
        assert!(artist.top() >= hero.bottom() && artist.top() == album.top() && artist.right() < album.left());
        let songs: Vec<Rect> = (0..3).map(|i| s.rect(song_id(i)).expect("a top song")).collect();
        assert!(songs[0].top() >= artist.bottom() && songs.windows(2).all(|w| w[0].bottom() <= w[1].top()));
        assert!(s.rect(song_id(3)).is_none());
        assert!(s.rect(empty_id()).is_none());
    }

    #[test]
    fn the_album_card_is_left_out_when_no_top_album_is_known() {
        let mut summary = a_summary();
        summary.top_album = None;
        let s = Shown::new(Some(summary));
        assert!(s.rect(card_id(1)).is_none());
        // The artist's takes the row.
        let (artist, hero) = (s.rect(card_id(0)).expect("top artist"), s.rect(hero_id()).unwrap());
        assert_eq!((artist.left(), artist.right()), (hero.left(), hero.right()));
    }

    #[test]
    fn a_period_with_nothing_in_it_shows_its_empty_days_and_no_tops() {
        let empty = Summary { heard: 0, plays: 0, bars: vec![0; 30], pace: None, top_artist: None, top_album: None, top_songs: Vec::new(), ..a_summary() };
        let s = Shown::new(Some(empty));
        assert!(s.rect(hero_id()).is_some());
        assert!(s.rect(card_id(0)).is_none() && s.rect(card_id(1)).is_none() && s.rect(song_id(0)).is_none());
        assert!(s.rect(empty_id()).is_none());
    }

    #[test]
    fn with_nothing_ever_heard_the_page_says_so_and_shows_no_numbers() {
        let s = Shown::new(None);
        assert!(s.rect(empty_id()).is_some());
        assert!(s.rect(hero_id()).is_none());
        assert!(s.rect(period_id(Period::Month)).is_none());
        assert!(s.rect(card_id(0)).is_none() && s.rect(song_id(0)).is_none());
    }

    /// The question in a headless egui: what was answered after tapping `id`.
    fn answer_after_tapping(id: egui::Id) -> Option<bool> {
        let ctx = headless();
        let mut time = 0.0;
        let mut frame = |events: Vec<egui::Event>| {
            time += 1.0 / 60.0;
            let mut answer = None;
            let out = ctx.run_ui(screen(time, events), |ui| answer = confirm_clear(ui.ctx(), "Kept: 412 plays."));
            out.drop_without_applying_deltas();
            answer
        };
        // The card finds its place over its first frames, and waits for an answer.
        for _ in 0..4 {
            assert_eq!(frame(vec![]), None);
        }
        let at = ctx.read_response(id).expect("its button").rect.center();
        assert_eq!(frame(vec![egui::Event::PointerMoved(at), press(at, true)]), None);
        frame(vec![press(at, false)])
    }

    #[test]
    fn clearing_asks_first() {
        assert_eq!(answer_after_tapping(clear_id()), Some(true));
        assert_eq!(answer_after_tapping(keep_id()), Some(false));
    }

    #[test]
    fn the_questions_buttons_are_big_enough_to_tap_and_apart() {
        let ctx = headless();
        for i in 0..4 {
            let out = ctx.run_ui(screen(i as f64 / 60.0, vec![]), |ui| {
                confirm_clear(ui.ctx(), "Kept: 412 plays.");
            });
            out.drop_without_applying_deltas();
        }
        let (clear, keep) = (ctx.read_response(clear_id()).unwrap().rect, ctx.read_response(keep_id()).unwrap().rect);
        assert!(clear.height() >= 44.0 && keep.height() >= 44.0 && clear.width() >= 44.0 && keep.width() >= 44.0);
        assert!(!clear.intersects(keep) && keep.right() < clear.left());
    }
}
