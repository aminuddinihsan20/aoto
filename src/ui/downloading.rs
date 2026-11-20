//! Downloads on the player: the queue (`crate::download_queue`) driven with real
//! fetches, kept between launches, and shown: the sheet an album or playlist page opens
//! to download all of it, and what is on its way on the Downloads page.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Color32, Rect, Sense, Ui, Vec2, pos2, vec2};
use tokio::sync::watch;

use super::sleep::pill;
use super::widgets::{self, named};
use super::{App, glyphs, m3, pages, sheet};
use crate::backend::{Collection, Req};
use crate::download_queue::{Ask, Job, Kind, Outcome, Plan, Queue, STREAK, Song, State, Summary, Update};
use crate::downloads::{Kept, human_size};
use crate::material::{self, Type, Weight, shape};
use crate::model::Track;
use crate::platform::Network;
use crate::theme::{self, icon};

/// The player's downloads: the queue, and the fetches it has under way.
pub struct Downloading {
    pub(super) queue: Queue,
    /// How to stop each fetch under way, by its ticket.
    stops: HashMap<u64, watch::Sender<bool>>,
    /// The queue's clock starts here.
    clock: Instant,
    /// Where the queue is kept between launches.
    file: PathBuf,
    sheet: SheetFor,
}

/// The sheet over an album's or playlist's page.
#[derive(Default)]
struct SheetFor {
    /// Whose it is, and whether it is up.
    id: String,
    open: bool,
    /// What Download would do, counted for this many of its songs (they come in by the page).
    plan: Option<(usize, Plan)>,
}

/// Is this song saved (and its file still there)?
fn downloaded(video_id: &str) -> bool {
    crate::downloads::path(video_id).is_some()
}

/// Keep the queue in `file` for the next launch; an empty one leaves no file.
fn write_queue(file: &Path, queue: &Queue) {
    let jobs = queue.snapshot();
    if jobs.is_empty() {
        let _ = std::fs::remove_file(file);
        return;
    }
    // Written beside it and moved into place: a launch never finds half a file.
    let part = file.with_extension("json.part");
    let written = serde_json::to_vec(&jobs).map_err(std::io::Error::other).and_then(|json| std::fs::write(&part, json)).and_then(|()| std::fs::rename(&part, file));
    if let Err(e) = written {
        log::warn!("couldn't keep the download queue: {e}");
    }
}

/// Take up the queue kept in `file`, less the songs saved since (`have`).
fn read_queue(file: &Path, queue: &mut Queue, have: impl Fn(&str) -> bool) {
    if let Some(jobs) = std::fs::read(file).ok().and_then(|json| serde_json::from_slice(&json).ok()) {
        queue.restore(jobs, have);
    }
}

impl Downloading {
    /// The queue as the last launch left it (after `downloads::init`: songs saved since
    /// are not fetched again). It goes on by itself, unless it was paused.
    pub fn load(data_dir: &Path, wifi_only: bool) -> Downloading {
        let file = data_dir.join("download_queue.json");
        let mut queue = Queue::new(wifi_only, crate::platform::network());
        read_queue(&file, &mut queue, downloaded);
        if !queue.jobs().is_empty() {
            log::info!("downloads: {} left in the queue from last time", count(queue.jobs().len(), "job"));
        }
        // Dev hook: YTM_NETWORK=offline|metered stands in for the phone's connection.
        match super::dev_var("YTM_NETWORK") {
            Some("offline") => crate::platform::set_network(Network::Offline.code()),
            Some("metered") => crate::platform::set_network(Network::Metered.code()),
            _ => {}
        }
        Downloading { queue, stops: HashMap::new(), clock: Instant::now(), file, sheet: SheetFor::default() }
    }
}

/// What the listener asks of the downloads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Command {
    Pause(u64),
    Resume(u64),
    Cancel(u64),
    WifiOnly(bool),
}

impl App {
    /// The queue's turn, drawn or in the background (`pump`): the connection as it is
    /// now, fetches to stop, songs to start, and the queue kept when it changed.
    pub(super) fn tick_downloads(&mut self) {
        let d = &mut self.downloads;
        d.queue.set_network(crate::platform::network());
        for ticket in d.queue.take_stops() {
            if let Some(stop) = d.stops.remove(&ticket) {
                stop.send_replace(true);
            }
        }
        while let Some(fetch) = d.queue.next(d.clock.elapsed().as_secs_f64()) {
            let (stop, told) = watch::channel(false);
            d.stops.insert(fetch.ticket, stop);
            self.backend.request(Req::Download { track: fetch.song.track, for_video: fetch.song.for_video, ticket: fetch.ticket, stop: told });
        }
        if d.queue.take_changed() {
            write_queue(&d.file, &d.queue);
        }
    }

    /// How soon the queue needs another look when nothing else draws a frame: when the
    /// next song may start.
    pub(super) fn downloads_wake(&self) -> Option<Duration> {
        let wait = self.downloads.queue.due_in(self.downloads.clock.elapsed().as_secs_f64())?;
        Some(Duration::from_secs_f64(wait.max(0.02)))
    }

    /// Download one song (its menu's Download).
    pub(super) fn download_song(&mut self, track: Track) {
        // The song version playing for a music video: that video plays the file too.
        let for_video = if self.playing_as().is_some_and(|s| s.id == track.id) { self.current_id().map(str::to_owned) } else { None };
        self.ask_download(Ask { kind: Kind::Song, key: track.id.clone(), title: track.title.clone(), thumbs: track.thumbs.clone(), songs: vec![Song { track, for_video }] });
    }

    /// Queue what is asked for. Android: the folder to save to is chosen once, first.
    fn ask_download(&mut self, ask: Ask) {
        if crate::platform::HAS_SAF && crate::downloads::tree().is_none() {
            self.download_after_folder(ask);
        } else {
            self.queue_download(ask);
        }
    }

    pub(super) fn queue_download(&mut self, ask: Ask) {
        let (kind, title) = (ask.kind, ask.title.clone());
        if kind != Kind::Song {
            // All its songs, also those saved before: Downloads shows them together.
            let mut seen = std::collections::HashSet::new();
            let songs = ask.songs.iter().map(|s| s.track.id.clone()).filter(|id| seen.insert(id.clone())).collect();
            crate::downloads::keep_collection(Kept { id: ask.key.clone(), title: title.clone(), is_album: kind == Kind::Album, thumbs: ask.thumbs.clone(), songs });
        }
        let queue = &mut self.downloads.queue;
        let job = queue.add(ask, downloaded).and_then(|id| queue.jobs().iter().find(|j| j.id == id));
        let text = match job {
            Some(job) => queued_text(kind, &title, job.total, queue.state(job)),
            None if kind == Kind::Song => format!("\u{201c}{title}\u{201d} is downloaded already, or on its way"),
            None => format!("Nothing left to download from \u{201c}{title}\u{201d}"),
        };
        self.toast(text);
        self.tick_downloads();
    }

    pub(super) fn download_command(&mut self, command: Command) {
        let queue = &mut self.downloads.queue;
        match command {
            Command::Pause(id) => queue.pause(id),
            Command::Resume(id) => queue.resume(id),
            Command::Cancel(id) => {
                let title = queue.jobs().iter().find(|j| j.id == id).map(|j| j.title.clone());
                queue.cancel(id);
                if let Some(title) = title {
                    self.toast(format!("Stopped downloading \u{201c}{title}\u{201d}"));
                }
            }
            Command::WifiOnly(on) => queue.set_wifi_only(on),
        }
        self.tick_downloads();
    }

    /// A song was saved (`copied`: Android's copy into the listener's folder).
    pub(super) fn on_downloaded(&mut self, ticket: u64, id: &str, title: &str, bytes: u64, copied: Option<Result<String, String>>) {
        self.downloads.stops.remove(&ticket);
        let outcome = Outcome::Saved { bytes, copied: !matches!(copied, Some(Err(_))) };
        let update = self.downloads.queue.finished(ticket, id, outcome);
        self.download_ended(update, match copied {
            Some(Ok(folder)) => format!("Downloaded \u{201c}{title}\u{201d} to {folder}"),
            Some(Err(e)) => format!("Downloaded \u{201c}{title}\u{201d}, but couldn't copy it to your folder: {e}"),
            None => format!("Downloaded \u{201c}{title}\u{201d}"),
        });
    }

    /// A song wasn't saved: it failed, or its fetch was stopped (then the queue knows).
    pub(super) fn on_download_failed(&mut self, ticket: u64, id: &str, title: &str, error: &str) {
        self.downloads.stops.remove(&ticket);
        let update = self.downloads.queue.finished(ticket, id, Outcome::Failed);
        self.download_ended(update, format!("Couldn't download \u{201c}{title}\u{201d}: {error}"));
    }

    /// Say how a download ended: `song` for one chosen by hand, a collection's count
    /// once its last song is settled.
    fn download_ended(&mut self, update: Option<Update>, song: String) {
        let text = match update {
            Some(Update::Done(s)) if s.kind == Kind::Song => song,
            Some(Update::Done(s)) => summary_text(&s),
            Some(Update::Stalled { title }) => format!("\u{201c}{title}\u{201d} is paused: {STREAK} songs in a row failed. Resume it in Downloads"),
            Some(Update::Going) | None => return,
        };
        log::info!("downloads: {text}");
        self.toast(text);
    }
}

/// "1 song", "3 songs".
fn count(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// What an album or a playlist asks the queue for: all its songs.
fn collection_ask(c: &Collection) -> Ask {
    Ask {
        kind: if c.is_album { Kind::Album } else { Kind::Playlist },
        key: c.id.clone(),
        title: c.title.clone(),
        thumbs: c.thumbs.clone(),
        songs: c.tracks.iter().cloned().map(|track| Song { track, for_video: None }).collect(),
    }
}

/// The songs the sheet leaves out, when it leaves any out: the line under its count.
fn plan_note(plan: &Plan) -> Option<String> {
    let mut left_out = Vec::new();
    if plan.downloaded > 0 {
        left_out.push(format!("{} downloaded already", plan.downloaded));
    }
    if plan.queued > 0 {
        left_out.push(format!("{} in the queue", plan.queued));
    }
    if left_out.is_empty() {
        return None;
    }
    Some(match (plan.fetch, plan.queued) {
        (0, 0) => format!("All {} are downloaded already", count(plan.downloaded, "song")),
        (0, _) => format!("Nothing left to download: {}", left_out.join(", ")),
        (n, _) => format!("{}: {} to download", left_out.join(", "), count(n, "song")),
    })
}

/// The sheet's Download button: how many songs, when it isn't all of them.
fn download_label(plan: &Plan) -> String {
    if plan.downloaded + plan.queued == 0 { "Download".to_owned() } else { format!("Download {}", count(plan.fetch, "song")) }
}

/// What to say when a download is queued: that it started, or what it waits for.
fn queued_text(kind: Kind, title: &str, songs: usize, state: State) -> String {
    let what = match kind {
        Kind::Song => format!("\u{201c}{title}\u{201d}"),
        Kind::Album | Kind::Playlist => format!("{} from \u{201c}{title}\u{201d}", count(songs, "song")),
    };
    match state {
        State::WaitingForWifi => format!("{what} will download on Wi-Fi: \u{201c}Wi-Fi only\u{201d} is on"),
        State::Offline => format!("{what} will download when there is a connection"),
        State::Downloading | State::Queued | State::Paused | State::Stalled => format!("Downloading {what}"),
    }
}

/// How a finished album or playlist went.
fn summary_text(s: &Summary) -> String {
    let mut text = if s.failed == 0 {
        format!("Downloaded \u{201c}{}\u{201d}: {}", s.title, count(s.done, "song"))
    } else if s.done == 0 {
        format!("Couldn't download \u{201c}{}\u{201d}: all {} failed", s.title, count(s.total, "song"))
    } else {
        format!("Downloaded {} of {} from \u{201c}{}\u{201d}: {} failed", s.done, count(s.total, "song"), s.title, s.failed)
    };
    if s.uncopied > 0 {
        text.push_str(&format!(". {} couldn't be copied to your folder", s.uncopied));
    }
    text
}

/// A job's lines on the Downloads page and on its sheet. A collection's first says how
/// far it is; the last says what it is doing when that isn't simply downloading, and
/// its failures.
fn job_lines(job: &Job, state: State) -> Vec<String> {
    let mut lines = Vec::new();
    if job.kind != Kind::Song {
        let mut far = format!("{} of {}", job.done, count(job.total, "song"));
        if job.bytes > 0 {
            far.push_str(&format!(" \u{2022} {}", human_size(job.bytes)));
        }
        lines.push(far);
    }
    let failed = (job.failed > 0).then(|| format!("{} failed", job.failed));
    let doing = match state {
        State::Downloading if job.kind == Kind::Song => Some("Downloading"),
        State::Downloading => None,
        State::Queued => Some("Queued"),
        State::Paused => Some("Paused"),
        State::WaitingForWifi => Some("Waiting for Wi-Fi"),
        State::Offline => Some("No connection"),
        // Its failures are the reason: said with them.
        State::Stalled => {
            lines.push(format!("Paused: {} failed, {STREAK} in a row", job.failed));
            return lines;
        }
    };
    let last: Vec<&str> = doing.into_iter().chain(failed.as_deref()).collect();
    if !last.is_empty() {
        lines.push(last.join(" \u{2022} "));
    }
    lines
}

/// What the sheet shows of an album or a playlist.
struct Showing {
    album: bool,
    title: String,
    /// What Download would fetch, and leave out.
    plan: Plan,
    /// Its songs are still coming in.
    loading: bool,
    /// It is in the queue already.
    job: Option<OnItsWay>,
    wifi_only: bool,
    accent: Color32,
}

struct OnItsWay {
    id: u64,
    /// How far it is and what it is doing (`job_lines`).
    lines: Vec<String>,
    fraction: f32,
    paused: bool,
}

/// What was tapped on the sheet.
#[derive(Debug, PartialEq)]
enum Tap {
    Close,
    Download,
    Command(Command),
}

fn wifi_id() -> egui::Id {
    egui::Id::unique("download-wifi-only")
}

fn left_id() -> egui::Id {
    egui::Id::unique("download-left")
}

fn main_id() -> egui::Id {
    egui::Id::unique("download-main")
}

/// How tall the sheet's two buttons are.
const BUTTON_H: f32 = 56.0;

/// How far a job is, as a line filling up in the accent.
fn progress_bar(ui: &Ui, rect: Rect, fraction: f32, accent: Color32) {
    let radius = rect.height() / 2.0;
    ui.painter().rect_filled(rect, radius, if super::m3::on() { crate::material::scheme().secondary_container } else { theme::glass(0.16) });
    let filled = rect.width() * fraction.clamp(0.0, 1.0);
    if filled > 0.0 {
        // Never thinner than its round ends.
        let fill = Rect::from_min_size(rect.min, vec2(filled.max(rect.height()), rect.height()));
        ui.painter().rect_filled(fill, radius, accent);
    }
}

/// The sheet's main button: Material's round filled one, or the app's filled pill.
fn main_button(ui: &mut Ui, rect: Rect, label: &str, glyph: char, accent: Color32) -> bool {
    if super::m3::on() {
        return super::m3::big_button(ui, rect, label, glyph, main_id()).clicked();
    }
    pill(ui, rect, main_id(), label, Some(glyph), true, false, accent)
}

/// What the sheet holds: what it is about, the Wi-Fi switch, and its two buttons: Cancel
/// and Download, or, for a collection on its way, cancelling and pausing that.
fn contents(ui: &mut Ui, s: &Showing) -> Option<Tap> {
    let mut tap = None;
    let w = ui.available_width();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let what = if s.album { "album" } else { "playlist" };
    let total = s.plan.fetch + s.plan.downloaded + s.plan.queued;
    let (heading, line, notes) = match &s.job {
        Some(job) => (format!("Downloading {what}"), s.title.clone(), job.lines.clone()),
        None if s.loading => (format!("Download {what}"), s.title.clone(), vec!["Its songs are still coming in\u{2026}".to_owned()]),
        None => (format!("Download {what}"), format!("{} \u{2022} {}", count(total, "song"), s.title), plan_note(&s.plan).into_iter().collect()),
    };
    let material = super::m3::on();
    let m = crate::material::scheme();
    if material {
        super::m3::sheet_header(ui, &heading, &line);
    } else {
        let (head, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
        widgets::text_at(ui, pos2(head.left() + 8.0, head.top()), &heading, theme::regular(24.0), theme::text(), w - 16.0);
        widgets::text_at(ui, pos2(head.left() + 8.0, head.top() + 36.0), &line, theme::regular(14.5), theme::text_dim(), w - 16.0);
    }
    for note in &notes {
        let (r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
        if material {
            super::m3::text(ui, pos2(r.left() + 8.0, r.top() + 2.0), note, Type::BodyMedium, m.on_surface_variant, w - 16.0);
        } else {
            widgets::text_at(ui, pos2(r.left() + 8.0, r.top() + 2.0), note, theme::regular(14.5), theme::text_dim(), w - 16.0);
        }
    }
    if let Some(job) = &s.job {
        ui.add_space(10.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, 6.0), Sense::hover());
        progress_bar(ui, r.shrink2(vec2(8.0, 0.0)), job.fraction, s.accent);
    }
    ui.add_space(14.0);

    // Wi-Fi only: the whole row switches it, and so does the switch.
    let (r, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
    let row = named(ui.interact(r, wifi_id(), Sense::click()), "Wi-Fi only");
    let mut on = s.wifi_only;
    if row.clicked() {
        on = !on;
    }
    let text_w = w - 16.0 - 60.0;
    if material {
        super::m3::two_lines(ui, r.left() + 8.0, r.center().y, text_w, "Wi-Fi only", Type::BodyLarge, m.on_surface, "Starts when this phone is on Wi-Fi", Type::BodyMedium, m.on_surface_variant);
    } else {
        widgets::text_at(ui, pos2(r.left() + 8.0, r.top() + 8.0), "Wi-Fi only", theme::regular(17.0), theme::text(), text_w);
        widgets::text_at(ui, pos2(r.left() + 8.0, r.top() + 32.0), "Starts when this phone is on Wi-Fi", theme::regular(13.5), theme::text_dim(), text_w);
    }
    let switch = Rect::from_center_size(pos2(r.right() - 8.0 - pages::toggle_size().x / 2.0, r.center().y), pages::toggle_size());
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(switch));
    pages::toggle(&mut sui, &mut on, "Wi-Fi only");
    if on != s.wifi_only {
        tap = Some(Tap::Command(Command::WifiOnly(on)));
    }
    ui.add_space(12.0);

    let gap = 10.0;
    let (r, _) = ui.allocate_exact_size(vec2(w, BUTTON_H), Sense::hover());
    let left_w = (w - gap) * if s.job.is_some() { 0.5 } else { 0.34 };
    let left = Rect::from_min_size(r.min, vec2(left_w, BUTTON_H));
    let main = Rect::from_min_max(pos2(left.right() + gap, r.top()), r.max);
    match &s.job {
        Some(job) => {
            if pill(ui, left, left_id(), "Cancel download", None, false, !material, s.accent) {
                tap = Some(Tap::Command(Command::Cancel(job.id)));
            }
            let (label, glyph, command) = if job.paused { ("Resume", icon::PLAY, Command::Resume(job.id)) } else { ("Pause", icon::PAUSE, Command::Pause(job.id)) };
            if main_button(ui, main, label, glyph, s.accent) {
                tap = Some(Tap::Command(command));
            }
        }
        None => {
            if pill(ui, left, left_id(), "Cancel", None, false, !material, s.accent) {
                tap = Some(Tap::Close);
            }
            let label = download_label(&s.plan);
            if s.loading || s.plan.fetch == 0 {
                // Nothing to fetch (yet): it is there, and takes no tap.
                named(ui.interact(main, main_id(), Sense::hover()), &label);
                ui.painter().rect_filled(main, BUTTON_H / 2.0, theme::glass(0.09));
                let g = if material { super::m3::galley(ui, &label, Type::TitleMedium, theme::text_faint(), main.width() - 20.0) } else { widgets::galley(ui, &label, theme::semibold(15.0), theme::text_faint(), main.width() - 20.0) };
                ui.painter().galley(pos2(main.center().x - g.size().x / 2.0, main.center().y - g.size().y / 2.0), g, theme::text_faint());
            } else if main_button(ui, main, &label, icon::DOWNLOAD, s.accent) {
                tap = Some(Tap::Download);
            }
        }
    }
    tap
}

/// Bring up the download sheet of the album or playlist `id`.
pub(super) fn open_sheet(app: &mut App, id: &str) {
    app.downloads.sheet = SheetFor { id: id.to_owned(), open: true, plan: None };
}

/// Dev hooks: YTM_DOWNLOAD_SHEET opens the download sheet of the album or playlist on
/// screen (`YTM_PAGE`) once its songs are in; YTM_DOWNLOAD_ALL queues it instead and
/// opens Downloads.
fn dev_hooks(app: &mut App) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let all = super::dev_var("YTM_DOWNLOAD_ALL").is_some();
    if (!all && super::dev_var("YTM_DOWNLOAD_SHEET").is_none()) || DONE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(id) = app.page_list().map(str::to_owned) else { return };
    if app.receiving(&id) || app.opened(&id).is_none_or(|o| o.c.tracks.is_empty()) {
        return;
    }
    DONE.store(true, std::sync::atomic::Ordering::Relaxed);
    if !all {
        open_sheet(app, &id);
    } else if let Some(ask) = app.opened(&id).map(|o| collection_ask(&o.c)) {
        app.queue_download(ask);
        app.go(super::Page::Downloads);
    }
}

/// The sheet, over its album's or playlist's page.
pub(super) fn sheet(app: &mut App, ctx: &egui::Context) {
    dev_hooks(app);
    let id = app.downloads.sheet.id.clone();
    let Some(o) = app.opened(&id) else {
        app.downloads.sheet.open = false;
        return;
    };
    // It goes with its page.
    let mut open = app.downloads.sheet.open && !app.np_open && app.page_list() == Some(id.as_str());
    let queue = &app.downloads.queue;
    let job = queue.collection(&id).map(|job| OnItsWay { id: job.id, lines: job_lines(job, queue.state(job)), fraction: job.fraction(), paused: job.paused });
    // Counted when the sheet comes up, and again as more of the list comes in or once
    // it is no longer on its way.
    let songs = o.c.tracks.len();
    let plan = match (&job, app.downloads.sheet.plan) {
        (Some(_), _) => None,
        (None, Some((counted, plan))) if counted == songs => Some(plan),
        (None, _) => Some(queue.plan(&o.c.tracks, downloaded)),
    };
    let showing = Showing {
        album: o.c.is_album,
        title: o.c.title.clone(),
        plan: plan.unwrap_or(Plan { fetch: 0, downloaded: 0, queued: 0 }),
        loading: app.receiving(&id),
        job,
        wifi_only: queue.wifi_only(),
        accent: app.accent_color(),
    };
    let mut tap = None;
    sheet::show(ctx, "download-collection", &mut open, |ui| tap = contents(ui, &showing));
    app.downloads.sheet.plan = plan.map(|plan| (songs, plan));
    match tap {
        Some(Tap::Close) => open = false,
        Some(Tap::Download) => {
            open = false;
            if let Some(ask) = app.opened(&id).map(|o| collection_ask(&o.c)) {
                app.ask_download(ask);
            }
        }
        Some(Tap::Command(command)) => {
            // Cancelling is what the sheet was up for; pausing and the switch leave it up.
            open &= !matches!(command, Command::Cancel(_));
            app.download_command(command);
        }
        None => {}
    }
    app.downloads.sheet.open = open;
}

/// The Downloads page's switch: downloads wait for Wi-Fi.
pub(super) fn wifi_row(app: &mut App, ui: &mut Ui) {
    const NAME: &str = "Download over Wi-Fi only";
    let m = material::scheme();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let row = named(ui.interact(r, ui.scope_id().with("dl-wifi-only"), Sense::click()), NAME);
    let was = app.downloads.queue.wifi_only();
    let mut on = was;
    if row.clicked() {
        on = !on;
    }
    if row.is_pointer_button_down_on() {
        ui.painter().rect_filled(r.expand2(vec2(8.0, 0.0)), shape::CARD_SMALL, m.on_surface.gamma_multiply(0.08));
    }
    let badge = pos2(r.left() + 20.0, r.center().y);
    ui.painter().circle_filled(badge, 20.0, m.secondary_container);
    m3::drawn::wifi(ui, badge, 20.0, m.on_secondary_container);
    let switch = Rect::from_center_size(pos2(r.right() - pages::toggle_size().x / 2.0, r.center().y), pages::toggle_size());
    let text_l = badge.x + 20.0 + 16.0;
    // On a narrow phone the line under the name takes two.
    let text_w = (switch.left() - 12.0 - text_l).max(40.0);
    let name = m3::galley(ui, NAME, Type::BodyLarge, m.on_surface, text_w);
    let note = m3::galley_wrapped(ui, "Waits for Wi-Fi, saves mobile data", Type::BodyMedium, m.on_surface_variant, text_w, 2);
    let top = r.center().y - (name.size().y + note.size().y) / 2.0;
    let note_top = top + name.size().y;
    ui.painter().galley(pos2(text_l, top), name, m.on_surface);
    ui.painter().galley(pos2(text_l, note_top), note, m.on_surface_variant);
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(switch));
    pages::toggle(&mut sui, &mut on, NAME);
    if on != was {
        app.download_command(Command::WifiOnly(on));
    }
}

/// Is anything on its way? (The Downloads page heads the group only then.)
pub(super) fn any_on_their_way(app: &App) -> bool {
    !app.downloads.queue.jobs().is_empty()
}

/// Where the parts of a card of something on its way are: its cover, its text, the
/// progress under the text (an album's or a playlist's), and the touch targets of
/// its cancel and its pause.
#[derive(Clone, Copy, Debug, PartialEq)]
struct JobGeom {
    cover: Rect,
    text: Rect,
    progress: Rect,
    cancel: Rect,
    pause: Rect,
}

/// How tall a job's card is: its title, its `lines`, and for a collection its progress.
fn job_height(whole: bool, lines: usize) -> f32 {
    let text = 24.0 + 20.0 * lines as f32 + if whole { 22.0 } else { 0.0 };
    (text + 24.0).max(80.0)
}

fn job_geom(card: Rect, whole: bool, lines: usize) -> JobGeom {
    let cy = card.center().y;
    let cover = Rect::from_min_size(pos2(card.left() + 12.0, cy - 28.0), Vec2::splat(56.0));
    let pause = Rect::from_center_size(pos2(card.right() - 12.0 - m3::TOUCH / 2.0, cy), Vec2::splat(m3::TOUCH));
    let cancel = Rect::from_center_size(pos2(pause.left() - m3::TOUCH / 2.0, cy), Vec2::splat(m3::TOUCH));
    let left = cover.right() + 12.0;
    let height = 24.0 + 20.0 * lines as f32 + if whole { 22.0 } else { 0.0 };
    let text = Rect::from_min_size(pos2(left, cy - height / 2.0), vec2((cancel.left() - left).max(40.0), 24.0 + 20.0 * lines as f32));
    JobGeom { cover, text, progress: Rect::from_min_size(pos2(left, text.bottom()), vec2(text.width(), 22.0)), cancel, pause }
}

/// What is on its way, on the Downloads page: a card for each album, playlist or song
/// in the queue, with how far it is, and its cancel and pause.
pub(super) fn on_their_way(app: &mut App, ui: &mut Ui) {
    let m = material::scheme();
    let ink = m.on_primary_container;
    let queue = &app.downloads.queue;
    let time = ui.input(|i| i.time);
    let mut command = None;
    for job in queue.jobs() {
        let state = queue.state(job);
        let lines = job_lines(job, state);
        let whole = job.kind != Kind::Song;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), job_height(whole, lines.len())), Sense::hover());
        let g = job_geom(rect, whole, lines.len());
        ui.painter().rect_filled(rect, shape::CARD, m.primary_container);
        widgets::paint_cover(ui, g.cover, &job.thumbs, shape::TILE, 1.0);
        m3::text(ui, g.text.min, &job.title, Type::TitleMedium.spec().weight(Weight::Bold), ink, g.text.width());
        for (i, line) in lines.iter().enumerate() {
            m3::text(ui, pos2(g.text.left(), g.text.top() + 24.0 + 20.0 * i as f32), line, Type::BodyMedium, ink.gamma_multiply(0.85), g.text.width());
        }
        if whole {
            // The wave runs while songs come in, and lies flat while the job waits.
            let running = state == State::Downloading;
            let wave = theme::anim_bool(ui.ctx(), ("dl-wave", job.id), running, 0.3);
            m3::wavy_progress(ui, g.progress, job.fraction(), -(time as f32) * 4.0, wave);
            if running {
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        }
        let id = ui.scope_id().with(("dl-job", job.id));
        if m3::IconButton::new(icon::X, "Cancel download").show_at(ui, g.cancel.center(), id.with("cancel")).clicked() {
            command = Some(Command::Cancel(job.id));
        }
        let (name, glyph, turn) = if job.paused { ("Resume download", glyphs::PLAY, Command::Resume(job.id)) } else { ("Pause download", glyphs::PAUSE, Command::Pause(job.id)) };
        if m3::IconButton::new(glyph, name).style(m3::IconStyle::Filled).size(m3::TOUCH).radius(shape::TILE).show_at(ui, g.pause.center(), id.with("pause")).clicked() {
            command = Some(turn);
        }
        ui.add_space(8.0);
    }
    if let Some(command) = command {
        app.download_command(command);
    }
}

/// A downloaded album's or playlist's line: what it is, and what is saved of it.
fn saved_line(is_album: bool, songs: usize, bytes: u64) -> String {
    format!("{} \u{2022} {} \u{2022} {}", if is_album { "Album" } else { "Playlist" }, count(songs, "song"), human_size(bytes))
}

/// The albums and playlists downloaded whole, on the Downloads page: a row each, marked
/// as downloaded, which opens its saved songs.
pub(super) fn saved_collections(app: &mut App, ui: &mut Ui) {
    for c in &crate::downloads::collections() {
        let row = m3::ListRow { title: &c.title, subtitle: &saved_line(c.is_album, c.songs.len(), c.bytes), thumbs: &c.thumbs, mark: Some(m3::Icon::Drawn(m3::drawn::saved)), chevron: true, ..Default::default() };
        if ui.push_id(("dl-saved", &c.id), |ui| m3::list_row(ui, &row)).inner.row.clicked() {
            app.actions.push(super::Action::Go(super::Page::Downloaded(c.id.clone())));
        }
    }
}

/// The saved songs of one album or playlist downloaded whole: they play without internet.
pub(super) fn saved_page(app: &mut App, ui: &mut Ui, id: &str, now: f64) {
    match crate::downloads::collections().into_iter().find(|c| c.id == id) {
        Some(c) => pages::local_song_page(app, ui, &c.title, &saved_line(c.is_album, c.songs.len(), c.bytes), c.songs, now),
        None => pages::local_song_page(app, ui, "Downloads", "Nothing of this is saved any more.", Vec::new(), now),
    }
}

/// The disk as the storage bar splits it: the downloads' share and the share of
/// everything else on it; the rest is free.
fn disk_shares(downloads: u64, free: u64, total: u64) -> (f32, f32) {
    if total == 0 {
        return (0.0, 0.0);
    }
    let used = total.saturating_sub(free);
    let downloads = downloads.min(used);
    (downloads as f32 / total as f32, (used - downloads) as f32 / total as f32)
}

/// The storage bar's three parts across `bar`, with a gap between them: the downloads
/// (`mine` of the disk; at least a dot of them, when there are any), everything else
/// (`other`), and the free room.
fn disk_segments(bar: Rect, mine: f32, other: f32) -> [Rect; 3] {
    const GAP: f32 = 4.0;
    let room = bar.width() - 2.0 * GAP;
    let mine_w = if mine > 0.0 { (room * mine).max(bar.height()) } else { 0.0 };
    let other_w = (room * other).clamp(0.0, (room - mine_w - bar.height()).max(0.0));
    let part = |from: f32, width: f32| Rect::from_min_size(pos2(from, bar.top()), vec2(width.max(0.0), bar.height()));
    let other_from = bar.left() + mine_w + if mine_w > 0.0 { GAP } else { 0.0 };
    let free_from = other_from + other_w + if other_w > 0.0 { GAP } else { 0.0 };
    [part(bar.left(), mine_w), part(other_from, other_w), part(free_from, bar.right() - free_from)]
}

/// The room on the phone as a bar across `bar`: the downloads, everything else, and
/// the free room.
pub(super) fn disk_bar(ui: &Ui, bar: Rect, downloads: u64, free: u64, total: u64) {
    let m = material::scheme();
    let (mine, other) = disk_shares(downloads, free, total);
    let [mine, other, free] = disk_segments(bar, if downloads > 0 { mine.max(f32::MIN_POSITIVE) } else { 0.0 }, other);
    for (part, color) in [(mine, m.primary), (other, m.outline), (free, m.surface_container_highest)] {
        if part.width() > 0.0 {
            ui.painter().rect_filled(part, bar.height() / 2.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download_queue::{Ask, Song};

    fn track(id: &str) -> Track {
        Track { id: id.into(), title: format!("Song {id}"), artists: Vec::new(), album: None, duration: None, thumbs: Vec::new(), track_nr: None, plays: None, music_video: false }
    }

    fn plan(fetch: usize, downloaded: usize, queued: usize) -> Plan {
        Plan { fetch, downloaded, queued }
    }

    #[test]
    fn the_sheet_says_how_many_songs_and_which_are_left_out() {
        assert_eq!(plan_note(&plan(48, 0, 0)), None, "nothing to add to \"48 songs\"");
        assert_eq!(plan_note(&plan(36, 12, 0)).as_deref(), Some("12 downloaded already: 36 songs to download"));
        assert_eq!(plan_note(&plan(1, 2, 3)).as_deref(), Some("2 downloaded already, 3 in the queue: 1 song to download"));
        assert_eq!(plan_note(&plan(0, 48, 0)).as_deref(), Some("All 48 songs are downloaded already"));
        assert_eq!(plan_note(&plan(0, 40, 8)).as_deref(), Some("Nothing left to download: 40 downloaded already, 8 in the queue"));
    }

    #[test]
    fn the_download_button_counts_the_songs_when_some_are_left_out() {
        assert_eq!(download_label(&plan(48, 0, 0)), "Download");
        assert_eq!(download_label(&plan(36, 12, 0)), "Download 36 songs");
        assert_eq!(download_label(&plan(1, 0, 4)), "Download 1 song");
    }

    #[test]
    fn a_queued_download_says_that_it_started_or_what_it_waits_for() {
        assert_eq!(queued_text(Kind::Song, "Tabun", 1, State::Queued), "Downloading \u{201c}Tabun\u{201d}");
        assert_eq!(queued_text(Kind::Song, "Tabun", 1, State::WaitingForWifi), "\u{201c}Tabun\u{201d} will download on Wi-Fi: \u{201c}Wi-Fi only\u{201d} is on");
        assert_eq!(queued_text(Kind::Song, "Tabun", 1, State::Offline), "\u{201c}Tabun\u{201d} will download when there is a connection");
        assert_eq!(queued_text(Kind::Playlist, "santai", 31, State::Queued), "Downloading 31 songs from \u{201c}santai\u{201d}");
        assert_eq!(queued_text(Kind::Album, "THE BOOK", 1, State::WaitingForWifi), "1 song from \u{201c}THE BOOK\u{201d} will download on Wi-Fi: \u{201c}Wi-Fi only\u{201d} is on");
        assert_eq!(queued_text(Kind::Playlist, "santai", 31, State::Offline), "31 songs from \u{201c}santai\u{201d} will download when there is a connection");
    }

    #[test]
    fn the_storage_bar_splits_the_disk_into_downloads_everything_else_and_free_room() {
        const GB: u64 = 1 << 30;
        let (downloads, other) = disk_shares(2 * GB, 40 * GB, 64 * GB);
        assert!((downloads - 2.0 / 64.0).abs() < 1e-6 && (other - 22.0 / 64.0).abs() < 1e-6, "{downloads} {other}");
        // Counted a moment apart, the downloads can come out larger than what is in use.
        assert_eq!(disk_shares(10 * GB, 60 * GB, 64 * GB), (4.0 / 64.0, 0.0));
        assert_eq!(disk_shares(GB, 0, 0), (0.0, 0.0), "a disk that says nothing");
    }

    #[test]
    fn the_storage_bars_parts_follow_each_other_with_a_gap_and_fill_it() {
        let bar = Rect::from_min_size(pos2(40.0, 200.0), vec2(280.0, 8.0));
        let [mine, other, free] = disk_segments(bar, 0.05, 0.35);
        assert_eq!(mine.left(), bar.left());
        assert!(mine.right() < other.left() && other.right() < free.left(), "a gap between the parts");
        assert!((free.right() - bar.right()).abs() < 0.01);
        assert!(other.width() > mine.width() * 5.0, "in proportion: {mine:?} {other:?}");
        // A sliver of downloads still shows as a dot, and a full disk keeps a dot of free room.
        let [mine, _, _] = disk_segments(bar, 0.0001, 0.5);
        assert_eq!(mine.width(), bar.height());
        let [_, _, free] = disk_segments(bar, 0.1, 0.9);
        assert!(free.width() >= bar.height() - 0.01, "{free:?}");
        // No downloads: the bar starts with everything else.
        let [mine, other, _] = disk_segments(bar, 0.0, 0.4);
        assert_eq!(mine.width(), 0.0);
        assert_eq!(other.left(), bar.left());
    }

    #[test]
    fn a_card_of_something_on_its_way_keeps_its_parts_apart_and_its_buttons_a_fingers_size() {
        for width in [328.0, 344.0, 361.0, 380.0] {
            for (whole, lines) in [(true, 1), (true, 2), (false, 1)] {
                let card = Rect::from_min_size(pos2(16.0, 300.0), vec2(width, job_height(whole, lines)));
                let g = job_geom(card, whole, lines);
                for part in [g.cover, g.text, g.cancel, g.pause] {
                    assert!(card.contains_rect(part), "{width} {whole} {lines}: {part:?}");
                }
                if whole {
                    assert!(card.contains_rect(g.progress), "{width} {lines}: {:?}", g.progress);
                    assert!(g.progress.top() >= g.text.bottom() && g.progress.height() >= 22.0);
                }
                assert!(g.cover.right() < g.text.left() && g.text.right() <= g.cancel.left() && g.cancel.right() <= g.pause.left());
                for target in [g.cancel, g.pause] {
                    assert!(target.width() >= m3::TOUCH && target.height() >= m3::TOUCH);
                }
                assert!(g.text.width() >= 130.0, "{width}: the title keeps {}", g.text.width());
            }
        }
    }

    #[test]
    fn a_downloaded_collection_says_what_it_is_and_what_it_holds() {
        assert_eq!(saved_line(false, 48, 412 * 1024 * 1024), "Playlist \u{2022} 48 songs \u{2022} 412 MB");
        assert_eq!(saved_line(true, 1, 9 * 1024 * 1024), "Album \u{2022} 1 song \u{2022} 9 MB");
    }

    fn summary(total: usize, done: usize, failed: usize, uncopied: usize) -> Summary {
        Summary { kind: Kind::Playlist, title: "santai".into(), total, done, failed, uncopied }
    }

    #[test]
    fn the_end_of_a_collection_says_what_was_saved_and_what_failed() {
        assert_eq!(summary_text(&summary(31, 31, 0, 0)), "Downloaded \u{201c}santai\u{201d}: 31 songs");
        assert_eq!(summary_text(&summary(1, 1, 0, 0)), "Downloaded \u{201c}santai\u{201d}: 1 song");
        assert_eq!(summary_text(&summary(31, 28, 3, 0)), "Downloaded 28 of 31 songs from \u{201c}santai\u{201d}: 3 failed");
        assert_eq!(summary_text(&summary(31, 0, 31, 0)), "Couldn't download \u{201c}santai\u{201d}: all 31 songs failed");
        assert_eq!(summary_text(&summary(31, 31, 0, 2)), "Downloaded \u{201c}santai\u{201d}: 31 songs. 2 couldn't be copied to your folder");
    }

    /// A queue on `network` with one playlist of four songs: the first saved (2 MB),
    /// the second failed, the third under way.
    fn under_way(network: Network) -> Queue {
        let mut q = Queue::new(true, Network::Unmetered);
        let songs = ["a", "b", "c", "d"].iter().map(|id| Song { track: track(id), for_video: None }).collect();
        q.add(Ask { kind: Kind::Playlist, key: "p".into(), title: "santai".into(), thumbs: Vec::new(), songs }, |_| false).unwrap();
        let (a, b) = (q.next(0.0).unwrap(), q.next(5.0).unwrap());
        q.finished(a.ticket, "a", Outcome::Saved { bytes: 2 * 1024 * 1024, copied: true });
        q.finished(b.ticket, "b", Outcome::Failed);
        q.next(10.0).unwrap();
        q.set_network(network);
        q
    }

    #[test]
    fn a_collection_on_its_way_shows_its_count_its_size_and_its_failures() {
        let q = under_way(Network::Unmetered);
        let job = &q.jobs()[0];
        assert_eq!(job_lines(job, q.state(job)), ["1 of 4 songs \u{2022} 2 MB", "1 failed"]);
    }

    #[test]
    fn a_collection_with_nothing_to_say_but_how_far_it_is_has_one_line() {
        let mut q = Queue::new(true, Network::Unmetered);
        let songs = ["a", "b"].iter().map(|id| Song { track: track(id), for_video: None }).collect();
        q.add(Ask { kind: Kind::Album, key: "p".into(), title: "THE BOOK".into(), thumbs: Vec::new(), songs }, |_| false).unwrap();
        q.next(0.0).unwrap();
        let job = &q.jobs()[0];
        assert_eq!(job_lines(job, q.state(job)), ["0 of 2 songs"]);
    }

    #[test]
    fn a_collection_that_waits_says_what_for() {
        let q = under_way(Network::Metered);
        let job = &q.jobs()[0];
        assert_eq!(job_lines(job, q.state(job))[1], "Waiting for Wi-Fi \u{2022} 1 failed");
        assert_eq!(job_lines(job, State::Offline)[1], "No connection \u{2022} 1 failed");
        assert_eq!(job_lines(job, State::Paused)[1], "Paused \u{2022} 1 failed");
        assert_eq!(job_lines(job, State::Queued)[1], "Queued \u{2022} 1 failed");
        // Paused by its failures: they are the reason, said once.
        assert_eq!(job_lines(job, State::Stalled)[1], "Paused: 1 failed, 5 in a row");
    }

    #[test]
    fn a_single_song_on_its_way_shows_only_what_it_is_doing() {
        let mut q = Queue::new(true, Network::Metered);
        q.add(Ask { kind: Kind::Song, key: "s".into(), title: "Tabun".into(), thumbs: Vec::new(), songs: vec![Song { track: track("s"), for_video: None }] }, |_| false).unwrap();
        let job = &q.jobs()[0];
        assert_eq!(job_lines(job, q.state(job)), ["Waiting for Wi-Fi"]);
        assert_eq!(job_lines(job, State::Downloading), ["Downloading"]);
    }

    #[test]
    fn the_queue_is_written_to_its_file_and_read_back() {
        let dir = std::env::temp_dir().join(format!("aoto-dlq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("download_queue.json");
        let q = under_way(Network::Unmetered);
        write_queue(&file, &q);
        let mut back = Queue::new(true, Network::Unmetered);
        read_queue(&file, &mut back, |_| false);
        // The song under way waits again, before the one that had not started.
        let job = &back.jobs()[0];
        assert_eq!((job.title.as_str(), job.total, job.done, job.failed), ("santai", 4, 1, 1));
        assert_eq!(back.next(0.0).map(|f| f.song.track.id), Some("c".to_owned()));
        // An empty queue leaves no file behind, and no file is an empty queue.
        write_queue(&file, &Queue::new(true, Network::Unmetered));
        assert!(!file.exists());
        let mut none = Queue::new(true, Network::Unmetered);
        read_queue(&file, &mut none, |_| false);
        assert!(none.jobs().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sheet's contents in a headless egui, 380 points wide, with stand-ins for the
    /// app's fonts.
    struct OnScreen {
        ctx: egui::Context,
        showing: Showing,
        time: f64,
        tapped: Vec<Tap>,
    }

    impl OnScreen {
        fn new(plan: Plan, job: Option<OnItsWay>) -> OnScreen {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let plain = fonts.families[&egui::FontFamily::Proportional].clone();
            for family in ["semibold", "bold", "icons"] {
                fonts.families.insert(egui::FontFamily::Name(family.into()), plain.clone());
            }
            ctx.set_fonts(fonts);
            let showing = Showing { album: false, title: "santai".into(), plan, loading: false, job, wifi_only: true, accent: theme::RED };
            let mut s = OnScreen { ctx, showing, time: 0.0, tapped: Vec::new() };
            s.frame(vec![]);
            s.frame(vec![]);
            s
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(380.0, 700.0))), time: Some(self.time), events, ..Default::default() };
            let mut tapped = None;
            let out = self.ctx.run_ui(input, |ui| tapped = contents(ui, &self.showing));
            out.drop_without_applying_deltas();
            self.tapped.extend(tapped);
        }

        fn rect(&self, id: egui::Id) -> Option<Rect> {
            self.ctx.read_response(id).map(|r| r.rect)
        }

        fn tap(&mut self, id: egui::Id) {
            let at = self.rect(id).expect("it is on the sheet").center();
            self.frame(vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
            self.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
            self.frame(vec![]);
        }
    }

    fn on_its_way(paused: bool) -> Option<OnItsWay> {
        Some(OnItsWay { id: 7, lines: vec!["31 of 76 songs".into(), "Waiting for Wi-Fi".into()], fraction: 0.4, paused })
    }

    #[test]
    fn the_sheet_downloads_or_closes() {
        let mut s = OnScreen::new(plan(48, 0, 0), None);
        s.tap(main_id());
        s.tap(left_id());
        assert_eq!(s.tapped, [Tap::Download, Tap::Close]);
    }

    #[test]
    fn the_sheets_switch_turns_wifi_only_off_and_on() {
        let mut s = OnScreen::new(plan(48, 0, 0), None);
        s.tap(wifi_id());
        assert_eq!(s.tapped, [Tap::Command(Command::WifiOnly(false))]);
        s.showing.wifi_only = false;
        s.tap(wifi_id());
        assert_eq!(s.tapped.last(), Some(&Tap::Command(Command::WifiOnly(true))));
    }

    #[test]
    fn with_nothing_left_to_fetch_download_does_nothing() {
        let mut s = OnScreen::new(plan(0, 48, 0), None);
        s.tap(main_id());
        assert!(s.tapped.is_empty());
    }

    #[test]
    fn a_list_still_coming_in_is_not_downloaded_yet() {
        let mut s = OnScreen::new(plan(20, 0, 0), None);
        s.showing.loading = true;
        s.tap(main_id());
        assert!(s.tapped.is_empty());
    }

    #[test]
    fn a_collection_on_its_way_is_paused_resumed_or_cancelled_from_its_sheet() {
        let mut s = OnScreen::new(plan(0, 31, 45), on_its_way(false));
        s.tap(main_id());
        s.tap(left_id());
        assert_eq!(s.tapped, [Tap::Command(Command::Pause(7)), Tap::Command(Command::Cancel(7))]);
        let mut s = OnScreen::new(plan(0, 31, 45), on_its_way(true));
        s.tap(main_id());
        assert_eq!(s.tapped, [Tap::Command(Command::Resume(7))]);
    }

    #[test]
    fn nothing_on_the_sheet_overlaps_and_everything_is_big_enough_to_tap() {
        for job in [None, on_its_way(false)] {
            let s = OnScreen::new(plan(36, 12, 0), job);
            let rects: Vec<Rect> = [wifi_id(), left_id(), main_id()].into_iter().map(|id| s.rect(id).expect("drawn")).collect();
            for (i, a) in rects.iter().enumerate() {
                assert!(a.height() >= 44.0 && a.width() >= 44.0, "{a:?}");
                assert!(a.left() >= 0.0 && a.right() <= 380.0, "{a:?} is on the sheet");
                for b in &rects[i + 1..] {
                    assert!(!a.intersects(*b), "{a:?} and {b:?}");
                }
            }
        }
    }
}
