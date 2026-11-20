//! The sleep timer on the player: what happens to the music when it is up (the timing
//! itself is `crate::sleep`), the chip on Now Playing that shows it, and its sheet.

use std::sync::atomic::Ordering;
use std::time::Duration;

use egui::{Color32, Pos2, Rect, Sense, Ui, pos2, vec2};

use super::widgets::{self, named};
use super::{App, Repeat, pages, sheet};
use crate::audio::AudioCmd;
use crate::sleep::{FADE_SECS, Stop};
use crate::theme::{self, icon};

/// The lengths the sheet offers, in minutes.
const MINUTES: [u32; 6] = [5, 15, 30, 45, 60, 120];

/// The sheet: whether it is up, and what is picked in it until Start.
pub struct Pick {
    pub open: bool,
    pub stop: Stop,
    pub fade: bool,
}

impl Default for Pick {
    fn default() -> Self {
        Self { open: false, stop: Stop::After(30), fade: true }
    }
}

/// What the listener asked the sheet for.
#[derive(Debug, PartialEq)]
enum Ask {
    Start(Stop, bool),
    Cancel,
}

/// What the sheet shows of the player.
struct Showing {
    /// The running timer, and the seconds a timed one has left.
    running: Option<Stop>,
    left: Option<f64>,
    /// Stopping by the song or the queue is on offer (not in a party, where the room
    /// decides what plays next).
    by_song: bool,
    accent: Color32,
}

/// What stops the music when a timed one is up.
#[derive(Debug, PartialEq)]
enum Halt {
    /// Pause, as the listener would.
    Pause,
    /// A song is on its way, or the player waits for more songs: it stays put instead.
    Park,
    /// It is silent already and stays so.
    Nothing,
}

/// `loading`: a song was asked for and hasn't started; `loaded`: the player holds the
/// current song (paused, when not `playing`); `parked`: it waits for Play, not loaded.
fn halt(loading: bool, playing: bool, loaded: bool, parked: bool) -> Halt {
    if loading {
        Halt::Park
    } else if playing {
        Halt::Pause
    } else if loaded || parked {
        Halt::Nothing
    } else {
        Halt::Park
    }
}

/// The time on the clock the timer runs by: the wall clock, so that it keeps going
/// while the phone sleeps with the music paused.
fn clock() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

impl App {
    /// The song playing is the last there is: none follows it, and no playlist is
    /// still coming in behind it.
    fn last_in_queue(&self) -> bool {
        self.queue.at_end() && self.queue.fill.is_none()
    }

    /// Seconds of the song still to play, once the player has it: a song on its way,
    /// or one waiting for Play, has no end to fade towards yet.
    fn song_left(&self) -> Option<f64> {
        if self.buffering || self.resume_at.is_some() {
            return None;
        }
        let total = self.audio.status.duration()?;
        Some(total.saturating_sub(self.audio.status.position()).as_secs_f64())
    }

    /// The timer's turn, drawn or in the background: the fade's volume, and the pause
    /// when a timed one is up. First in `pump`: a Play pressed after the time ran out
    /// unseen (the phone asleep, the music paused) must not be the thing it pauses.
    pub(super) fn tick_sleep(&mut self) {
        let Some(stop) = self.sleep.running() else { return };
        // In a party the room decides what follows a song: only a timed one holds there.
        if self.jam.active() && !matches!(stop, Stop::After(_)) {
            return self.cancel_sleep();
        }
        let now = clock();
        if !self.sleep.due(now) {
            self.audio.set_fade(self.sleep.gain(now, self.song_left(), self.last_in_queue()));
            return;
        }
        let loaded = self.audio.status.playing_gen.load(Ordering::Relaxed) == self.generation;
        let halt = halt(self.buffering, self.playing(), loaded, self.resume_at.is_some());
        log::info!("sleep timer: the time is up ({halt:?})");
        match halt {
            Halt::Pause => self.toggle(),
            Halt::Park => self.park(),
            Halt::Nothing => {}
        }
        // The listener's own volume again, for the next Play.
        self.audio.set_fade(1.0);
    }

    /// The timer stops the music when this song ends: it plays to its last note
    /// rather than handing over to the next.
    pub(super) fn sleep_stops_after_song(&self) -> bool {
        self.sleep.stops_after_song(self.last_in_queue())
    }

    /// The song playing is over (it ran out, or what was left of it is skipped): the
    /// timer may end the music here; otherwise the song repeats or the queue goes on.
    pub(super) fn song_finished(&mut self) {
        if self.sleep.song_ended(self.last_in_queue()) {
            log::info!("sleep timer: stopping with the song");
            self.deload_motion();
            // The next song waits at its start: the same one when it repeats or was the last.
            if self.queue.repeat != Repeat::One && !self.queue.advance() {
                // A playlist still coming in doesn't start playing when its next songs arrive.
                self.queue.started();
            }
            self.park();
        } else if self.queue.repeat == Repeat::One {
            self.play_current();
        } else {
            // The sleeve was this song's: put it down rather than keep its decoder,
            // frame and texture for the rest of the session.
            self.deload_motion();
            self.next();
        }
    }

    /// Stop the player on the current song at its start, without its audio, as a
    /// restored session is: Play starts it. A song still on its way is dropped.
    fn park(&mut self) {
        let Some(track) = self.current().cloned() else { return };
        self.generation += 1;
        self.audio.status.wanted_gen.store(self.generation, Ordering::Relaxed);
        self.audio.send(AudioCmd::Stop);
        self.audio.set_fade(1.0);
        self.pending_mix = None;
        self.mix_from = None;
        self.playing_as = None;
        self.buffering = false;
        self.seek_drag = None;
        self.audio_info = None;
        self.stream_stats = None;
        self.audio.status.duration_ms.store(track.duration.map_or(0, |d| d as u64 * 1000), Ordering::Relaxed);
        self.audio.status.position_ms.store(0, Ordering::Relaxed);
        self.resume_at = Some(Duration::ZERO);
        // It never started here, so playing it is a new play.
        self.resume_counts = true;
        self.show_in_media(&track);
        self.request_art();
        self.refresh_lyrics(track);
    }

    fn start_sleep(&mut self, stop: Stop, fade: bool) {
        log::info!("sleep timer: {stop:?}, fade {fade}");
        self.sleep.start(stop, fade, clock());
    }

    fn cancel_sleep(&mut self) {
        self.sleep.cancel();
        self.audio.set_fade(1.0);
        // An end-of-queue timer kept radio out: the queue may be on its last song.
        self.top_up();
    }

    /// How soon the timer needs another look when nothing else draws a frame: often
    /// through the fade, twice a second while Now Playing shows the time left, and
    /// otherwise when the fade (or the pause) is due.
    pub(super) fn sleep_wake(&self) -> Option<Duration> {
        let left = self.sleep.remaining(clock())?;
        let quiet = if self.sleep.fades() { left - FADE_SECS } else { left };
        let wait = if quiet <= 0.0 { 0.1 } else if self.np_open { quiet.min(0.5) } else { quiet };
        Some(Duration::from_secs_f64(wait.max(0.02)))
    }
}

/// Dev hooks: YTM_SLEEP=<minutes>|song|queue starts a sleep timer (YTM_SLEEP_FADE=0
/// without the fade); YTM_SLEEP_SHEET opens its sheet.
pub(super) fn dev_hooks(app: &mut App) {
    let stop = match super::dev_var("YTM_SLEEP") {
        Some("song") => Some(Stop::SongEnd),
        Some("queue") => Some(Stop::QueueEnd),
        Some(minutes) => minutes.parse().ok().map(Stop::After),
        None => None,
    };
    if let Some(stop) = stop {
        app.start_sleep(stop, super::dev_var("YTM_SLEEP_FADE") != Some("0"));
    }
    if super::dev_var("YTM_SLEEP_SHEET").is_some() {
        open_sheet(app);
    }
}

/// A length on its chip.
fn minutes_label(minutes: u32) -> String {
    match minutes {
        60 => "1 hour".to_owned(),
        m if m > 60 && m % 60 == 0 => format!("{} hours", m / 60),
        m => format!("{m} min"),
    }
}

/// What is left of a timed one, as "24:18" (or "1:04:18"): a second that has begun
/// still shows.
fn countdown(secs: f64) -> String {
    let s = secs.max(0.0).ceil() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The running timer in a few words, for the chip on Now Playing.
fn chip_label(running: Option<Stop>, left: Option<f64>) -> String {
    match running {
        None => "Sleep timer".to_owned(),
        Some(Stop::After(_)) => countdown(left.unwrap_or(0.0)),
        Some(Stop::SongEnd) => "End of song".to_owned(),
        Some(Stop::QueueEnd) => "End of queue".to_owned(),
    }
}

/// The line under the sheet's title.
fn status_line(running: Option<Stop>, left: Option<f64>) -> String {
    match running {
        None => "The music stops when the time is up".to_owned(),
        Some(Stop::After(_)) => format!("The music stops in {}", countdown(left.unwrap_or(0.0))),
        Some(Stop::SongEnd) => "The music stops at the end of this song".to_owned(),
        Some(Stop::QueueEnd) => "The music stops at the end of the queue".to_owned(),
    }
}

fn stop_id(stop: Stop) -> egui::Id {
    egui::Id::unique(("sleep-stop", stop))
}

fn fade_id() -> egui::Id {
    egui::Id::unique("sleep-fade")
}

fn start_id() -> egui::Id {
    egui::Id::unique("sleep-start")
}

fn cancel_id() -> egui::Id {
    egui::Id::unique("sleep-cancel")
}

/// Text and icons on a fill of the accent: dark on a pale one.
fn on_accent(accent: Color32) -> Color32 {
    if accent.r() as u32 + accent.g() as u32 + accent.b() as u32 > 480 { Color32::from_gray(15) } else { Color32::WHITE }
}

/// A crescent moon in the icon font's line (the font has none): Lucide's, on its
/// 24-unit grid.
fn moon(ui: &Ui, center: Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let at = |x: f32, y: f32| center + vec2(x - 12.0, y - 12.0) * k;
    // The inner edge, half a circle from the top tip into the disc and out at the right
    // tip; then the outer edge, three quarters of the disc back round to the top.
    let inner = (0..=16).map(|i| {
        let a = (225.0 - 180.0 * i as f32 / 16.0).to_radians();
        at(16.5 + 6.364 * a.cos(), 7.5 + 6.364 * a.sin())
    });
    let outer = (0..=24).map(|i| {
        let a = (270.0 * i as f32 / 24.0).to_radians();
        at(12.0 + 9.0 * a.cos(), 12.0 + 9.0 * a.sin())
    });
    let stroke = egui::Stroke::new(2.0 * k, color);
    ui.painter().add(egui::Shape::line(inner.collect(), stroke));
    ui.painter().add(egui::Shape::line(outer.collect(), stroke));
    // Round tips, where the two edges meet at a point.
    for tip in [at(12.0, 3.0), at(21.0, 12.0)] {
        ui.painter().circle_filled(tip, k, color);
    }
}

/// How tall the sheet's pills are.
const PILL_H: f32 = 52.0;

/// One of the sheet's pills, filled in the accent while `on`; `outlined`: a line
/// round it instead of a fill until then. True when tapped.
#[allow(clippy::too_many_arguments)]
fn pill(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, glyph: Option<char>, on: bool, outlined: bool, accent: Color32) -> bool {
    let resp = named(ui.interact(rect, id, Sense::click()), label);
    let t = theme::anim_bool(ui.ctx(), id.with("on"), on, 0.15);
    let press = if resp.is_pointer_button_down_on() { 0.08 } else { 0.0 };
    let off = if outlined { theme::glass(press) } else { theme::glass(0.09 + press) };
    ui.painter().rect_filled(rect, rect.height() / 2.0, theme::lerp_color(off, accent, t));
    if outlined && t < 1.0 {
        ui.painter().rect_stroke(rect, rect.height() / 2.0, egui::Stroke::new(1.0, theme::glass(0.28 * (1.0 - t))), egui::StrokeKind::Inside);
    }
    let ink = theme::lerp_color(theme::text(), on_accent(accent), t);
    let icon_w = if glyph.is_some() { 26.0 } else { 0.0 };
    let g = widgets::galley(ui, label, theme::semibold(15.0), ink, rect.width() - 20.0 - icon_w);
    let left = rect.center().x - (icon_w + g.size().x) / 2.0;
    if let Some(glyph) = glyph {
        widgets::icon_at(ui, pos2(left + 9.0, rect.center().y), glyph, 17.0, ink);
    }
    ui.painter().galley(pos2(left + icon_w, rect.center().y - g.size().y / 2.0), g, ink);
    resp.clicked()
}

/// What the sheet holds: the lengths, the two stops that go by the music, the fade
/// switch and the button that starts it (and, while one runs, the one that cancels it).
fn contents(ui: &mut Ui, pick: &mut Pick, s: &Showing) -> Option<Ask> {
    let mut ask = None;
    let w = ui.available_width();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

    let (head, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
    widgets::text_at(ui, pos2(head.left() + 8.0, head.top()), "Sleep timer", theme::regular(24.0), theme::text(), w - 16.0);
    widgets::text_at(ui, pos2(head.left() + 8.0, head.top() + 36.0), &status_line(s.running, s.left), theme::regular(14.5), theme::text_dim(), w - 16.0);
    ui.add_space(16.0);

    let gap = 10.0;
    let third = (w - 2.0 * gap) / 3.0;
    for row in MINUTES.chunks(3) {
        let (r, _) = ui.allocate_exact_size(vec2(w, PILL_H), Sense::hover());
        for (i, &minutes) in row.iter().enumerate() {
            let stop = Stop::After(minutes);
            let cell = Rect::from_min_size(pos2(r.left() + (third + gap) * i as f32, r.top()), vec2(third, PILL_H));
            if pill(ui, cell, stop_id(stop), &minutes_label(minutes), None, pick.stop == stop, false, s.accent) {
                pick.stop = stop;
            }
        }
        ui.add_space(gap);
    }
    if s.by_song {
        let half = (w - gap) / 2.0;
        let (r, _) = ui.allocate_exact_size(vec2(w, PILL_H), Sense::hover());
        for (i, (stop, label, glyph)) in [(Stop::SongEnd, "End of this song", icon::MUSIC), (Stop::QueueEnd, "End of the queue", icon::LIST_MUSIC)].into_iter().enumerate() {
            let cell = Rect::from_min_size(pos2(r.left() + (half + gap) * i as f32, r.top()), vec2(half, PILL_H));
            if pill(ui, cell, stop_id(stop), label, Some(glyph), pick.stop == stop, true, s.accent) {
                pick.stop = stop;
            }
        }
        ui.add_space(gap);
    }
    ui.add_space(6.0);

    // The fade: the whole row switches it, and so does the switch.
    let (r, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
    let row = named(ui.interact(r, fade_id(), Sense::click()), "Fade out");
    if row.clicked() {
        pick.fade = !pick.fade;
    }
    let text_w = w - 16.0 - 60.0;
    widgets::text_at(ui, pos2(r.left() + 8.0, r.top() + 8.0), "Fade out", theme::regular(17.0), theme::text(), text_w);
    let note = format!("Turns the volume down over the last {FADE_SECS:.0} seconds");
    widgets::text_at(ui, pos2(r.left() + 8.0, r.top() + 32.0), &note, theme::regular(13.5), theme::text_dim(), text_w);
    let switch = Rect::from_center_size(pos2(r.right() - 8.0 - 22.0, r.center().y), vec2(44.0, 26.0));
    let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(switch));
    pages::toggle(&mut sui, &mut pick.fade, "Fade out");
    ui.add_space(12.0);

    let (r, _) = ui.allocate_exact_size(vec2(w, 56.0), Sense::hover());
    let label = if s.running.is_some() { "Change timer" } else { "Start timer" };
    let start = named(ui.interact(r, start_id(), Sense::click()), label);
    let ink = on_accent(s.accent);
    let fill = if start.is_pointer_button_down_on() { theme::lerp_color(s.accent, ink, 0.12) } else { s.accent };
    ui.painter().rect_filled(r, 28.0, fill);
    let g = widgets::galley(ui, label, theme::semibold(15.5), ink, w - 80.0);
    let left = r.center().x - (28.0 + g.size().x) / 2.0;
    moon(ui, pos2(left + 9.0, r.center().y), 19.0, ink);
    ui.painter().galley(pos2(left + 28.0, r.center().y - g.size().y / 2.0), g, ink);
    if start.clicked() {
        ask = Some(Ask::Start(pick.stop, pick.fade));
    }

    if s.running.is_some() {
        ui.add_space(gap);
        let (r, _) = ui.allocate_exact_size(vec2(w, PILL_H), Sense::hover());
        if pill(ui, r, cancel_id(), "Cancel timer", None, false, true, s.accent) {
            ask = Some(Ask::Cancel);
        }
    }
    ask
}

/// The chip on Now Playing that opens the sheet, its left end at `at`: its name, or
/// what the running timer waits for (the time left, counting down).
pub(super) fn chip(app: &mut App, ui: &mut Ui, at: Pos2) {
    let running = app.sleep.running();
    let label = chip_label(running, app.sleep.remaining(clock()));
    let ink = if running.is_some() { app.accent_color() } else { theme::text() };
    let g = widgets::galley(ui, &label, theme::semibold(13.5), ink, 170.0);
    let rect = Rect::from_min_size(pos2(at.x, at.y - 18.0), vec2(12.0 + 18.0 + 7.0 + g.size().x + 14.0, 36.0));
    // A finger's height to tap, a little more than what is drawn.
    let resp = named(ui.interact(rect.expand2(vec2(0.0, 6.0)), ui.scope_id().with("np-sleep"), Sense::click()), "Sleep timer");
    let press = if resp.is_pointer_button_down_on() { 0.08 } else { 0.0 };
    ui.painter().rect_filled(rect, 18.0, theme::glass(0.09 + press));
    moon(ui, pos2(rect.left() + 12.0 + 9.0, rect.center().y), 17.0, ink);
    ui.painter().galley(pos2(rect.left() + 12.0 + 18.0 + 7.0, rect.center().y - g.size().y / 2.0), g, ink);
    if resp.clicked() {
        open_sheet(app);
    }
}

/// Bring the sheet up, on the timer that runs when one does (to change or cancel it).
fn open_sheet(app: &mut App) {
    if let Some(stop) = app.sleep.running() {
        app.sleep_sheet.stop = stop;
        app.sleep_sheet.fade = app.sleep.fades();
    }
    app.sleep_sheet.open = true;
}

/// The sheet, over Now Playing.
pub(super) fn sheet(app: &mut App, ctx: &egui::Context) {
    let mut pick = std::mem::take(&mut app.sleep_sheet);
    let by_song = !app.jam.active();
    if !by_song && !matches!(pick.stop, Stop::After(_)) {
        pick.stop = Pick::default().stop;
    }
    let showing = Showing { running: app.sleep.running(), left: app.sleep.remaining(clock()), by_song, accent: app.accent_color() };
    let (mut open, mut ask) = (pick.open, None);
    sheet::show(ctx, "sleep-timer", &mut open, |ui| ask = contents(ui, &mut pick, &showing));
    // Starting or cancelling is what the sheet was up for.
    pick.open = open && ask.is_none();
    app.sleep_sheet = pick;
    match ask {
        Some(Ask::Start(stop, fade)) => app.start_sleep(stop, fade),
        Some(Ask::Cancel) => app.cancel_sleep(),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontFamily;

    #[test]
    fn lengths_read_as_minutes_and_hours() {
        assert_eq!(minutes_label(5), "5 min");
        assert_eq!(minutes_label(45), "45 min");
        assert_eq!(minutes_label(60), "1 hour");
        assert_eq!(minutes_label(120), "2 hours");
    }

    #[test]
    fn what_is_left_counts_down_by_the_second() {
        assert_eq!(countdown(1458.0), "24:18");
        assert_eq!(countdown(1457.2), "24:18");
        assert_eq!(countdown(1800.0), "30:00");
        assert_eq!(countdown(0.4), "0:01");
        assert_eq!(countdown(0.0), "0:00");
        assert_eq!(countdown(59.0), "0:59");
    }

    #[test]
    fn an_hour_or_more_shows_its_hours() {
        assert_eq!(countdown(7200.0), "2:00:00");
        assert_eq!(countdown(3858.0), "1:04:18");
        assert_eq!(countdown(3599.5), "1:00:00");
    }

    #[test]
    fn the_chip_says_what_the_timer_waits_for() {
        assert_eq!(chip_label(None, None), "Sleep timer");
        assert_eq!(chip_label(Some(Stop::After(30)), Some(1458.0)), "24:18");
        assert_eq!(chip_label(Some(Stop::SongEnd), None), "End of song");
        assert_eq!(chip_label(Some(Stop::QueueEnd), None), "End of queue");
    }

    #[test]
    fn the_sheet_says_when_the_music_stops() {
        assert_eq!(status_line(None, None), "The music stops when the time is up");
        assert_eq!(status_line(Some(Stop::After(30)), Some(1458.0)), "The music stops in 24:18");
        assert_eq!(status_line(Some(Stop::SongEnd), None), "The music stops at the end of this song");
        assert_eq!(status_line(Some(Stop::QueueEnd), None), "The music stops at the end of the queue");
    }

    #[test]
    fn music_that_plays_is_paused_when_the_time_is_up() {
        assert_eq!(halt(false, true, true, false), Halt::Pause);
    }

    #[test]
    fn a_song_on_its_way_does_not_start_after_the_time_is_up() {
        assert_eq!(halt(true, false, false, false), Halt::Park);
        // The last song can still read as playing for a moment.
        assert_eq!(halt(true, true, false, false), Halt::Park);
    }

    #[test]
    fn a_player_waiting_for_more_songs_does_not_go_on_after_the_time_is_up() {
        assert_eq!(halt(false, false, false, false), Halt::Park);
    }

    #[test]
    fn music_paused_by_hand_stays_where_it_is() {
        assert_eq!(halt(false, false, true, false), Halt::Nothing);
        assert_eq!(halt(false, false, false, true), Halt::Nothing);
    }

    /// The sheet's contents in a headless egui, 380 points wide, with stand-ins for the
    /// app's fonts.
    struct Sheet {
        ctx: egui::Context,
        pick: Pick,
        showing: Showing,
        time: f64,
        asked: Vec<Ask>,
    }

    impl Sheet {
        fn new(running: Option<Stop>, by_song: bool) -> Sheet {
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let plain = fonts.families[&FontFamily::Proportional].clone();
            for family in ["semibold", "bold", "icons"] {
                fonts.families.insert(FontFamily::Name(family.into()), plain.clone());
            }
            ctx.set_fonts(fonts);
            let showing = Showing { running, left: running.map(|_| 1458.0), by_song, accent: theme::RED };
            let mut s = Sheet { ctx, pick: Pick::default(), showing, time: 0.0, asked: Vec::new() };
            s.frame(vec![]);
            s.frame(vec![]);
            s
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(380.0, 700.0))), time: Some(self.time), events, ..Default::default() };
            let mut asked = None;
            let out = self.ctx.run_ui(input, |ui| asked = contents(ui, &mut self.pick, &self.showing));
            out.drop_without_applying_deltas();
            self.asked.extend(asked);
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

    #[test]
    fn it_opens_on_thirty_minutes_with_the_fade_on() {
        let mut s = Sheet::new(None, true);
        s.tap(start_id());
        assert_eq!(s.asked, [Ask::Start(Stop::After(30), true)]);
    }

    #[test]
    fn every_length_and_both_ends_can_be_picked() {
        let stops = MINUTES.map(Stop::After).into_iter().chain([Stop::SongEnd, Stop::QueueEnd]);
        for stop in stops {
            let mut s = Sheet::new(None, true);
            s.tap(stop_id(stop));
            assert_eq!(s.pick.stop, stop);
            assert!(s.asked.is_empty(), "picking alone starts nothing");
            s.tap(start_id());
            assert_eq!(s.asked, [Ask::Start(stop, true)]);
        }
    }

    #[test]
    fn the_fade_can_be_switched_off_and_on() {
        let mut s = Sheet::new(None, true);
        s.tap(fade_id());
        assert!(!s.pick.fade);
        s.tap(start_id());
        assert_eq!(s.asked, [Ask::Start(Stop::After(30), false)]);
        s.tap(fade_id());
        assert!(s.pick.fade);
    }

    #[test]
    fn nothing_overlaps_and_everything_is_big_enough_to_tap() {
        let s = Sheet::new(Some(Stop::After(30)), true);
        let ids = MINUTES.map(Stop::After).into_iter().chain([Stop::SongEnd, Stop::QueueEnd]).map(stop_id).chain([fade_id(), start_id(), cancel_id()]);
        let rects: Vec<Rect> = ids.map(|id| s.rect(id).expect("drawn")).collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.height() >= 44.0 && a.width() >= 44.0, "{a:?}");
            assert!(a.left() >= 0.0 && a.right() <= 380.0, "{a:?} is on the sheet");
            for b in &rects[i + 1..] {
                assert!(!a.intersects(*b), "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn with_no_timer_running_there_is_nothing_to_cancel() {
        let s = Sheet::new(None, true);
        assert!(s.rect(cancel_id()).is_none());
    }

    #[test]
    fn a_running_timer_can_be_cancelled() {
        let mut s = Sheet::new(Some(Stop::After(30)), true);
        s.tap(cancel_id());
        assert_eq!(s.asked, [Ask::Cancel]);
    }

    #[test]
    fn a_running_timer_can_be_changed() {
        let mut s = Sheet::new(Some(Stop::After(30)), true);
        s.tap(stop_id(Stop::SongEnd));
        s.tap(start_id());
        assert_eq!(s.asked, [Ask::Start(Stop::SongEnd, true)]);
    }

    #[test]
    fn in_a_party_only_the_lengths_are_offered() {
        let s = Sheet::new(None, false);
        assert!(s.rect(stop_id(Stop::SongEnd)).is_none());
        assert!(s.rect(stop_id(Stop::QueueEnd)).is_none());
        assert!(s.rect(stop_id(Stop::After(5))).is_some());
    }
}
