//! Listening stats: what was listened to on this device, kept on this device and sent
//! nowhere. A play is the song, when, and the seconds of it that were heard. The player
//! tells `Tracker` what plays as it goes (nothing here reads a clock), `History` adds it
//! up by day, week, month and year, and `Log` keeps it in a file.
//!
//! Days are the listener's own: a moment is kept as UTC with the zone it was heard in,
//! and falls on the day it was there. A song heard across midnight is heard on both
//! days, and is a play on the day enough of it had been heard.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A song is played once this many seconds of it were heard, or half of one shorter
/// than twice this.
const PLAY_SECS: f64 = 30.0;
/// A song that is back within this many seconds of its start, from further in, is
/// played again.
const START_SECS: f64 = 3.0;
/// A position standing still for longer than this is a stream that stalled: nothing
/// is heard.
const STALL_SECS: f64 = 1.5;
/// Looks further apart than this missed something (the app was held up): no more was
/// heard between them than the song moved on.
const MISSED_SECS: f64 = 2.0;
/// How many seconds of a song still playing are heard between two reports of it.
const REPORT_SECS: u32 = 10;
/// Plays older than this many days are kept as their day's totals for each song.
const RECENT_DAYS: Day = 92;
/// How many songs the top list has.
const TOP_SONGS: usize = 5;

/// A day on the listener's calendar, counted from 1 January 1970.
pub type Day = i32;

/// The day a moment falls on where it is heard: `unix` seconds, in a zone `east`
/// minutes ahead of UTC.
pub fn local_day(unix: i64, east: i32) -> Day {
    (unix + east as i64 * 60).div_euclid(86_400) as Day
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// The date of a day, by the calendar's rules worked from 1 March (so a leap day
    /// comes last in its year).
    pub fn of(day: Day) -> Date {
        let z = day as i64 + 719_468;
        let (era, in_era) = (z.div_euclid(146_097), z.rem_euclid(146_097));
        let year_of_era = (in_era - in_era / 1460 + in_era / 36_524 - in_era / 146_096) / 365;
        let in_year = in_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let from_march = (5 * in_year + 2) / 153;
        let month = if from_march < 10 { from_march + 3 } else { from_march - 9 };
        Date {
            year: (year_of_era + era * 400 + i64::from(month <= 2)) as i32,
            month: month as u32,
            day: (in_year - (153 * from_march + 2) / 5 + 1) as u32,
        }
    }

    pub fn number(self) -> Day {
        let year = self.year as i64 - i64::from(self.month <= 2);
        let (era, year_of_era) = (year.div_euclid(400), year.rem_euclid(400));
        let from_march = (self.month as i64 + 9) % 12;
        let in_year = (153 * from_march + 2) / 5 + self.day as i64 - 1;
        let in_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + in_year;
        (era * 146_097 + in_era - 719_468) as Day
    }
}

/// The day of the week, Monday being 0.
fn weekday(day: Day) -> Day {
    (day + 3).rem_euclid(7)
}

/// What the page adds up: the week (Monday to Sunday), the month or the year.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Period {
    Week,
    Month,
    Year,
}

/// Days from `first` to `last`, both included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    first: Day,
    last: Day,
}

impl Period {
    /// The one `day` is in.
    fn around(self, day: Day) -> Span {
        let date = Date::of(day);
        let (first, next) = match self {
            Period::Week => {
                let monday = day - weekday(day);
                return Span { first: monday, last: monday + 6 };
            }
            Period::Month if date.month == 12 => (Date { day: 1, ..date }, Date { year: date.year + 1, month: 1, day: 1 }),
            Period::Month => (Date { day: 1, ..date }, Date { month: date.month + 1, day: 1, ..date }),
            Period::Year => (Date { month: 1, day: 1, ..date }, Date { year: date.year + 1, month: 1, day: 1 }),
        };
        Span { first: first.number(), last: next.number() - 1 }
    }

    /// The one before the one `day` is in.
    fn before(self, day: Day) -> Span {
        self.around(self.around(day).first - 1)
    }

    /// How many bars it is drawn with: a day each, or a month each for a year.
    fn bars(self, span: Span) -> usize {
        match self {
            Period::Year => 12,
            Period::Week | Period::Month => (span.last - span.first + 1) as usize,
        }
    }

    /// The bar `day` is in.
    fn bar(self, span: Span, day: Day) -> usize {
        match self {
            Period::Year => Date::of(day).month as usize - 1,
            Period::Week | Period::Month => (day - span.first) as usize,
        }
    }
}

/// A song's picture, as the app keeps one: where it is and its shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Cover {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub cover: Option<Cover>,
}

/// A stretch of listening to one song on one day.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Play {
    /// Which of `History::songs`.
    song: u32,
    /// When it began, in unix seconds, and how many minutes ahead of UTC the zone was.
    at: i64,
    east: i32,
    /// Seconds heard.
    heard: u32,
    /// The song became a play during it.
    counted: bool,
}

impl Play {
    fn day(&self) -> Day {
        local_day(self.at, self.east)
    }
}

/// What was heard of one song on one day.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tally {
    song: u32,
    heard: u32,
    plays: u32,
}

/// Everything listened to: the latest plays one by one, older days as their totals.
#[derive(Debug, Default, PartialEq)]
pub struct History {
    songs: Vec<Song>,
    by_id: HashMap<String, u32>,
    days: BTreeMap<Day, Vec<Tally>>,
    /// Oldest first; the last is the one a song still playing adds to.
    plays: Vec<Play>,
}

/// What a report changed, for the file.
struct Taken {
    /// The song, when the history hadn't met it.
    song: Option<u32>,
    play: Play,
    /// A new stretch, not more of the last one.
    first: bool,
}

/// An artist or an album, and the seconds heard of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Top {
    pub name: String,
    pub heard: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TopSong {
    pub song: Song,
    pub plays: u32,
    pub heard: u64,
}

/// The days of a period that are over, against as many at the start of the one before:
/// the seconds heard in each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    pub now: u64,
    pub then: u64,
}

impl Pace {
    /// How much more was heard now, in percent of then; negative for less.
    pub fn percent(self) -> i64 {
        ((self.now as f64 - self.then as f64) / self.then as f64 * 100.0).round() as i64
    }
}

/// A period as the page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    /// The period's first day, and the first day of the one before it.
    pub first: Date,
    pub before: Date,
    /// Seconds heard, and how many plays that made.
    pub heard: u64,
    pub plays: u32,
    /// Seconds heard on each day (in each month, for a year), and which of them is today.
    pub bars: Vec<u64>,
    pub today: usize,
    /// None when the period before has nothing fair to compare with.
    pub pace: Option<Pace>,
    pub top_artist: Option<Top>,
    /// None also when too few of the plays say which album they are from.
    pub top_album: Option<Top>,
    pub top_songs: Vec<TopSong>,
}

impl History {
    pub fn is_empty(&self) -> bool {
        self.days.is_empty() && self.plays.is_empty()
    }

    /// The first day anything was heard on.
    pub fn since(&self) -> Option<Day> {
        let folded = self.days.keys().next().copied();
        folded.into_iter().chain(self.plays.iter().map(Play::day)).min()
    }

    /// How many plays there are in all.
    pub fn plays(&self) -> u64 {
        let folded: u64 = self.days.values().flatten().map(|t| t.plays as u64).sum();
        folded + self.plays.iter().filter(|p| p.counted).count() as u64
    }

    /// What was heard of each song on each of the days of `span`: the day, the song, its
    /// seconds and its plays.
    fn each(&self, span: Span, mut f: impl FnMut(Day, u32, u32, u32)) {
        if span.first > span.last {
            return;
        }
        for (day, tallies) in self.days.range(span.first..=span.last) {
            for t in tallies {
                f(*day, t.song, t.heard, t.plays);
            }
        }
        for p in &self.plays {
            let day = p.day();
            if span.first <= day && day <= span.last {
                f(day, p.song, p.heard, p.counted as u32);
            }
        }
    }

    /// The seconds heard on the days of `span`.
    fn heard_in(&self, span: Span) -> u64 {
        let mut heard = 0;
        self.each(span, |_, _, secs, _| heard += secs as u64);
        heard
    }

    /// Take a report from the tracker in: a new stretch, or more of the last one.
    fn take(&mut self, report: Report) -> Option<Taken> {
        let Some(song) = report.song else {
            let last = self.plays.last_mut()?;
            (last.heard, last.counted) = (report.heard, report.counted);
            return Some(Taken { song: None, play: *last, first: false });
        };
        let known = self.by_id.get(&song.id).copied();
        let index = known.unwrap_or(self.songs.len() as u32);
        if known.is_none() {
            self.by_id.insert(song.id.clone(), index);
            self.songs.push(song);
        }
        let play = Play { song: index, at: report.at, east: report.east, heard: report.heard, counted: report.counted };
        self.plays.push(play);
        Some(Taken { song: known.is_none().then_some(index), play, first: true })
    }

    /// Plays from before the last `RECENT_DAYS` become their day's totals. True when
    /// any did.
    fn fold(&mut self, today: Day) -> bool {
        let (old, recent): (Vec<Play>, Vec<Play>) = self.plays.iter().partition(|p| p.day() < today - RECENT_DAYS);
        for p in &old {
            let day = self.days.entry(p.day()).or_default();
            match day.iter_mut().find(|t| t.song == p.song) {
                Some(t) => {
                    t.heard += p.heard;
                    t.plays += p.counted as u32;
                }
                None => day.push(Tally { song: p.song, heard: p.heard, plays: p.counted as u32 }),
            }
        }
        self.plays = recent;
        !old.is_empty()
    }

    /// The period `today` is in.
    pub fn summary(&self, period: Period, today: Day) -> Summary {
        let (span, before) = (period.around(today), period.before(today));
        let mut bars = vec![0u64; period.bars(span)];
        // Each song's seconds and plays in the period.
        let mut songs: HashMap<u32, (u64, u32)> = HashMap::new();
        self.each(span, |day, song, heard, plays| {
            bars[period.bar(span, day)] += heard as u64;
            let s = songs.entry(song).or_default();
            s.0 += heard as u64;
            s.1 += plays;
        });
        let plays: u32 = songs.values().map(|s| s.1).sum();

        // An artist's time is that of the songs they are named first on.
        let (mut artists, mut albums): (HashMap<&str, (u64, u32)>, HashMap<&str, (u64, u32)>) = Default::default();
        let mut with_album = 0;
        for (&index, &(heard, plays)) in &songs {
            let song = &self.songs[index as usize];
            let add = |to: &mut HashMap<_, (u64, u32)>, name| {
                let t = to.entry(name).or_default();
                t.0 += heard;
                t.1 += plays;
            };
            if let Some(artist) = song.artists.first() {
                add(&mut artists, artist.as_str());
            }
            if let Some(album) = &song.album {
                add(&mut albums, album.as_str());
                with_album += plays;
            }
        }
        // The longest heard; then the most played; then by name, so it never flips.
        let top = |of: HashMap<&str, (u64, u32)>| {
            let best = of.into_iter().filter(|(_, (heard, _))| *heard > 0).max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)));
            best.map(|(name, (heard, _))| Top { name: name.to_owned(), heard })
        };
        let top_album = if plays > 0 && with_album * 2 >= plays { top(albums) } else { None };

        let mut played: Vec<(&Song, u32, u64)> = songs.iter().filter(|(_, s)| s.1 > 0).map(|(i, s)| (&self.songs[*i as usize], s.1, s.0)).collect();
        played.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then_with(|| a.0.title.cmp(&b.0.title)).then_with(|| a.0.id.cmp(&b.0.id)));
        let top_songs = played.into_iter().take(TOP_SONGS).map(|(song, plays, heard)| TopSong { song: song.clone(), plays, heard }).collect();

        Summary {
            first: Date::of(span.first),
            before: Date::of(before.first),
            heard: bars.iter().sum(),
            plays,
            today: period.bar(span, today),
            bars,
            pace: self.pace(span, before, today),
            top_artist: top(artists),
            top_album,
            top_songs,
        }
    }

    /// How the days of `span` that are over compare with as many at the start of
    /// `before`. Today isn't over, so it would count against the period; a period
    /// from before the history began, or with nothing heard in those days, says nothing.
    fn pace(&self, span: Span, before: Span, today: Day) -> Option<Pace> {
        let over = today - span.first;
        if over < 1 || self.since()? > before.first {
            return None;
        }
        let then = self.heard_in(Span { first: before.first, last: (before.first + over - 1).min(before.last) });
        (then > 0).then(|| Pace { now: self.heard_in(Span { first: span.first, last: today - 1 }), then })
    }
}

/// The time, as the player reads it for the tracker.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Seconds on a clock that only runs forward, and not while the device sleeps.
    pub steady: f64,
    /// The time of day: unix seconds, and how many minutes ahead of UTC the zone is.
    pub unix: i64,
    pub east: i32,
}

/// What is audible right now.
#[derive(Clone, Copy, Debug)]
pub struct Playing<'a> {
    /// Changes whenever the player starts a stream, the same song's again included.
    pub key: u64,
    pub id: &'a str,
    /// Seconds into the song, and how long it is when that is known.
    pub position: f64,
    pub length: Option<f64>,
}

/// A stretch of listening as it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// The song, the first time its stretch is reported; later ones bring that up to date.
    pub song: Option<Song>,
    pub at: i64,
    pub east: i32,
    pub heard: u32,
    pub counted: bool,
}

/// Follows the player and works out what was heard: told at every look what is
/// audible, it reports each stretch of listening as it grows and when it ends.
#[derive(Default)]
pub struct Tracker {
    play: Option<Live>,
}

/// The play going on (or paused).
struct Live {
    key: u64,
    song: Song,
    /// Seconds heard of it on all its days, and whether that made it a play.
    heard: f64,
    counted: bool,
    position: f64,
    /// When the position last moved, and when it was last seen playing (None while it
    /// isn't).
    moved: f64,
    seen: Option<f64>,
    stretch: Option<Stretch>,
}

/// The day's part of a play.
struct Stretch {
    at: i64,
    east: i32,
    day: Day,
    heard: f64,
    counted: bool,
    /// What was reported of it: its whole seconds, and whether it counted.
    told: Option<(u32, bool)>,
}

/// How many seconds of a song of this length have to be heard for a play.
fn play_secs(length: Option<f64>) -> f64 {
    match length {
        Some(length) if length < 2.0 * PLAY_SECS => length / 2.0,
        _ => PLAY_SECS,
    }
}

impl Stretch {
    /// The stretch as it stands, when that is news: every `REPORT_SECS` of it, the
    /// moment it counts, and whatever is new when `all` is asked for (it stopped).
    fn report(&mut self, song: &Song, all: bool) -> Option<Report> {
        let secs = self.heard as u32;
        let (told_secs, told_counted) = self.told.unwrap_or((0, false));
        if self.counted == told_counted && secs < told_secs + REPORT_SECS && !(all && secs > told_secs) {
            return None;
        }
        let song = self.told.is_none().then(|| song.clone());
        self.told = Some((secs, self.counted));
        Some(Report { song, at: self.at, east: self.east, heard: secs, counted: self.counted })
    }
}

impl Live {
    /// The player shows this song from nearer its start than it was: it plays again,
    /// when that is a new stream or its very beginning (a seek back a little is not).
    fn started_over(&self, p: &Playing) -> bool {
        p.position + START_SECS < self.position && (p.key != self.key || p.position < START_SECS)
    }

    /// The seconds heard since the last look, with the song now at `position`.
    fn listened(&mut self, now: f64, position: f64) -> f64 {
        let moved = position - self.position;
        let heard = match self.seen {
            // It wasn't playing at the last look.
            None => {
                self.moved = now;
                0.0
            }
            Some(seen) => {
                let gap = (now - seen).max(0.0);
                if gap > MISSED_SECS {
                    moved.clamp(0.0, gap)
                } else if moved != 0.0 || now - self.moved <= STALL_SECS {
                    gap
                } else {
                    0.0
                }
            }
        };
        if moved != 0.0 {
            self.position = position;
            self.moved = now;
        }
        self.seen = Some(now);
        heard
    }
}

impl Tracker {
    /// A look at the player: `playing` is what is audible (None while paused, loading or
    /// stopped), `song` says what that is when it's new.
    pub fn tick(&mut self, clock: Clock, playing: Option<Playing>, song: impl FnOnce() -> Song) -> Option<Report> {
        let Some(p) = playing else {
            let live = self.play.as_mut()?;
            live.seen = None;
            return live.stretch.as_mut()?.report(&live.song, true);
        };
        if self.play.as_ref().is_none_or(|l| l.song.id != p.id || l.started_over(&p)) {
            let last = self.play.take().and_then(|mut l| l.stretch.as_mut()?.report(&l.song, true));
            self.play = Some(Live { key: p.key, song: song(), heard: 0.0, counted: false, position: p.position, moved: clock.steady, seen: Some(clock.steady), stretch: None });
            return last;
        }
        let live = self.play.as_mut()?;
        live.key = p.key;
        let heard = live.listened(clock.steady, p.position);
        let mut ended = None;
        if heard > 0.0 {
            let day = local_day(clock.unix, clock.east);
            if live.stretch.as_ref().is_some_and(|s| s.day != day) {
                // Midnight: the day that is over keeps what was heard on it.
                ended = live.stretch.take().and_then(|mut s| s.report(&live.song, true));
            }
            let stretch = live.stretch.get_or_insert(Stretch { at: clock.unix, east: clock.east, day, heard: 0.0, counted: false, told: None });
            stretch.heard += heard;
            live.heard += heard;
            if !live.counted && live.heard >= play_secs(p.length) {
                live.counted = true;
                stretch.counted = true;
            }
        }
        ended.or_else(|| live.stretch.as_mut()?.report(&live.song, false))
    }
}

/// What a file holds.
#[derive(Debug, PartialEq)]
enum Stored {
    /// The history that could be read, and how many bytes of the file that was.
    Read(History, usize),
    /// Something else, or a later version's: nothing in it is understood.
    Foreign,
}

/// The file's first bytes: its kind and version.
const HEADER: [u8; 5] = *b"BCLH\x01";

/// The kinds of record.
const SONG: u8 = 1;
const PLAY: u8 = 2;
const DAY: u8 = 3;

/// A number in as few bytes as it needs: seven bits a byte, the eighth saying more follow.
fn put_number(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push(n as u8 | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

/// One that can be negative: small either way stays short.
fn put_signed(out: &mut Vec<u8>, n: i64) {
    put_number(out, ((n << 1) ^ (n >> 63)) as u64);
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    put_number(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// A record's check (FNV-1a), which a cut or damaged one fails.
fn check(kind: u8, body: &[u8]) -> [u8; 4] {
    let mut hash = 0x811c_9dc5u32;
    for byte in std::iter::once(&kind).chain(body) {
        hash = (hash ^ *byte as u32).wrapping_mul(0x0100_0193);
    }
    hash.to_le_bytes()
}

/// A record: its kind, its body's length, the body, the check.
fn record(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    put_number(&mut out, body.len() as u64);
    out.extend_from_slice(body);
    out.extend_from_slice(&check(kind, body));
    out
}

fn song_record(song: &Song) -> Vec<u8> {
    let mut body = Vec::new();
    put_text(&mut body, &song.id);
    put_text(&mut body, &song.title);
    put_number(&mut body, song.artists.len() as u64);
    for artist in &song.artists {
        put_text(&mut body, artist);
    }
    body.push(song.album.is_some() as u8);
    if let Some(album) = &song.album {
        put_text(&mut body, album);
    }
    body.push(song.cover.is_some() as u8);
    if let Some(cover) = &song.cover {
        put_text(&mut body, &cover.url);
        put_number(&mut body, cover.width as u64);
        put_number(&mut body, cover.height as u64);
    }
    record(SONG, &body)
}

fn play_record(play: &Play) -> Vec<u8> {
    let mut body = Vec::new();
    put_number(&mut body, play.song as u64);
    put_signed(&mut body, play.at);
    put_signed(&mut body, play.east as i64);
    put_number(&mut body, play.heard as u64);
    body.push(play.counted as u8);
    record(PLAY, &body)
}

fn day_record(day: Day, tallies: &[Tally]) -> Vec<u8> {
    let mut body = Vec::new();
    put_signed(&mut body, day as i64);
    put_number(&mut body, tallies.len() as u64);
    for t in tallies {
        put_number(&mut body, t.song as u64);
        put_number(&mut body, t.heard as u64);
        put_number(&mut body, t.plays as u64);
    }
    record(DAY, &body)
}

/// Reads what the `put_` functions wrote; None where the bytes run out or make no sense.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(n)?;
        self.bytes = rest;
        Some(taken)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn number(&mut self) -> Option<u64> {
        let mut n = 0;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            n |= ((byte & 0x7f) as u64) << shift;
            if byte < 0x80 {
                return Some(n);
            }
        }
        None
    }

    fn small(&mut self) -> Option<u32> {
        self.number()?.try_into().ok()
    }

    fn signed(&mut self) -> Option<i64> {
        let n = self.number()?;
        Some((n >> 1) as i64 ^ -((n & 1) as i64))
    }

    fn flag(&mut self) -> Option<bool> {
        match self.byte()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn text(&mut self) -> Option<String> {
        let len = self.number()?.try_into().ok()?;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }
}

impl History {
    /// The whole history as a file.
    fn encode(&self) -> Vec<u8> {
        let mut out = HEADER.to_vec();
        for song in &self.songs {
            out.extend(song_record(song));
        }
        for (day, tallies) in &self.days {
            out.extend(day_record(*day, tallies));
        }
        for play in &self.plays {
            out.extend(play_record(play));
        }
        out
    }

    /// Take in the record `bytes` begins with: how long it was, or None when it is
    /// cut short, damaged or makes no sense here (then nothing of it is taken).
    fn read_record(&mut self, bytes: &[u8]) -> Option<usize> {
        let mut file = Reader { bytes };
        let kind = file.byte()?;
        let len = file.number()?.try_into().ok()?;
        let mut r = Reader { bytes: file.take(len)? };
        if file.take(4)? != check(kind, r.bytes) {
            return None;
        }
        let known = |song: u32| ((song as usize) < self.songs.len()).then_some(song);
        match kind {
            SONG => {
                let (id, title) = (r.text()?, r.text()?);
                let artists = (0..r.number()?).map(|_| r.text()).collect::<Option<Vec<_>>>()?;
                let album = if r.flag()? { Some(r.text()?) } else { None };
                let cover = if r.flag()? { Some(Cover { url: r.text()?, width: r.small()?, height: r.small()? }) } else { None };
                if !r.bytes.is_empty() || self.by_id.contains_key(&id) {
                    return None;
                }
                self.by_id.insert(id.clone(), self.songs.len() as u32);
                self.songs.push(Song { id, title, artists, album, cover });
            }
            PLAY => {
                let play = Play { song: known(r.small()?)?, at: r.signed()?, east: r.signed()?.try_into().ok()?, heard: r.small()?, counted: r.flag()? };
                if !r.bytes.is_empty() {
                    return None;
                }
                self.plays.push(play);
            }
            DAY => {
                let day = r.signed()?.try_into().ok()?;
                let tallies = (0..r.number()?).map(|_| Some(Tally { song: known(r.small()?)?, heard: r.small()?, plays: r.small()? })).collect::<Option<Vec<_>>>()?;
                if !r.bytes.is_empty() || self.days.contains_key(&day) {
                    return None;
                }
                self.days.insert(day, tallies);
            }
            _ => return None,
        }
        Some(bytes.len() - file.bytes.len())
    }
}

/// Read a file: every record up to the first that is cut short or damaged.
fn decode(bytes: &[u8]) -> Stored {
    // One that never got its first bytes whole has nothing in it yet.
    if bytes.len() < HEADER.len() && HEADER.starts_with(bytes) {
        return Stored::Read(History::default(), 0);
    }
    if !bytes.starts_with(&HEADER) {
        return Stored::Foreign;
    }
    let mut history = History::default();
    let mut good = HEADER.len();
    while let Some(len) = history.read_record(&bytes[good..]) {
        good += len;
    }
    Stored::Read(history, good)
}

const FILE: &str = "listening.bin";

/// The history and its file. The file only grows at its end: a song's first report
/// adds a record, and later ones write over that record until the next begins, so a
/// kill leaves every earlier play whole. Reading stops at a record that is cut short.
pub struct Log {
    history: History,
    tracker: Tracker,
    path: PathBuf,
    /// The file, while it can be written.
    file: Option<File>,
    /// How long the file is, and where in it the stretch being heard is.
    len: u64,
    live: u64,
    revision: u64,
}

impl Log {
    /// The history kept in `dir`, its plays from long before `today` folded into
    /// their days.
    pub fn open(dir: &Path, today: Day) -> Log {
        let path = dir.join(FILE);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let (mut history, good) = match decode(&bytes) {
            Stored::Read(history, good) => (history, good),
            Stored::Foreign => {
                // Not ours to throw away: it stays beside the new one.
                log::warn!("listening history: {} isn't a file this version reads; kept as listening.old", path.display());
                let _ = std::fs::rename(&path, path.with_extension("old"));
                (History::default(), 0)
            }
        };
        if good < bytes.len() {
            log::warn!("listening history: the file's last {} bytes are cut short or damaged; dropped", bytes.len() - good);
        }
        let folded = history.fold(today);
        let mut log = Log { history, tracker: Tracker::default(), path, file: None, len: 0, live: 0, revision: 0 };
        let opened = if folded || good < HEADER.len() { log.rewrite() } else { log.reopen(good as u64) };
        if let Err(e) = opened {
            log::warn!("listening history: can't be written, so it lasts until the app closes: {e}");
        }
        log
    }

    /// Write the whole history anew, in a file beside the old one that takes its place
    /// only once it is whole and on the disk.
    fn rewrite(&mut self) -> std::io::Result<()> {
        self.file = None;
        let bytes = self.history.encode();
        let part = self.path.with_extension("part");
        let mut file = File::create(&part)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&part, &self.path)?;
        self.reopen(bytes.len() as u64)
    }

    /// Open the file to write on after its first `len` bytes, the ones understood.
    fn reopen(&mut self, len: u64) -> std::io::Result<()> {
        let file = std::fs::OpenOptions::new().write(true).open(&self.path)?;
        file.set_len(len)?;
        self.file = Some(file);
        self.len = len;
        Ok(())
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Goes up whenever the history changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A look at the player (see `Tracker::tick`); what it reports is kept.
    pub fn tick(&mut self, clock: Clock, playing: Option<Playing>, song: impl FnOnce() -> Song) {
        let Some(report) = self.tracker.tick(clock, playing, song) else { return };
        let Some(taken) = self.history.take(report) else { return };
        self.revision += 1;
        let Some(file) = &mut self.file else { return };
        // A new stretch goes at the end, after its song when that is new too; more of
        // the last one is written over it.
        let mut bytes = Vec::new();
        let at = if taken.first {
            if let Some(song) = taken.song {
                bytes = song_record(&self.history.songs[song as usize]);
            }
            self.live = self.len + bytes.len() as u64;
            self.len
        } else {
            self.live
        };
        bytes.extend(play_record(&taken.play));
        match file.seek(SeekFrom::Start(at)).and_then(|_| file.write_all(&bytes)) {
            Ok(()) => self.len = at + bytes.len() as u64,
            Err(e) => {
                log::warn!("listening history: writing failed, so the rest lasts until the app closes: {e}");
                self.file = None;
            }
        }
    }

    /// Forget everything, the file's contents too.
    pub fn clear(&mut self) {
        self.history = History::default();
        self.tracker = Tracker::default();
        self.revision += 1;
        if let Err(e) = self.rewrite() {
            log::warn!("listening history: the cleared file couldn't be written: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: &str) -> Song {
        Song { id: id.to_owned(), title: format!("Song {id}"), artists: vec![format!("Artist {id}")], album: None, cover: None }
    }

    fn day(year: i32, month: u32, day: u32) -> Day {
        Date { year, month, day }.number()
    }

    /// `hour`:`minute` UTC on a day.
    fn moment(day: Day, hour: i64, minute: i64) -> i64 {
        day as i64 * 86_400 + hour * 3600 + minute * 60
    }

    const ALWAYS: Span = Span { first: Day::MIN, last: Day::MAX };

    impl History {
        /// The seconds heard and the plays of a song on the days of `span`.
        fn of(&self, song: &Song, span: Span) -> (u64, u32) {
            let (mut heard, mut plays) = (0, 0);
            self.each(span, |_, s, h, p| {
                if self.songs[s as usize].id == song.id {
                    heard += h as u64;
                    plays += p;
                }
            });
            (heard, plays)
        }

        /// A stretch of `secs` seconds of a song at noon UTC on `day`.
        fn add(&mut self, song: &Song, day: Day, secs: u32, counted: bool) {
            self.take(Report { song: Some(song.clone()), at: moment(day, 12, 0), east: 0, heard: secs, counted });
        }

        /// A song played `times` times on `day`, `secs` seconds each.
        fn played(&mut self, song: &Song, day: Day, times: u32, secs: u32) {
            for _ in 0..times {
                self.add(song, day, secs, true);
            }
        }
    }

    /// Someone listening: a player looked at four times a second, and the history
    /// that makes.
    struct Listener {
        tracker: Tracker,
        history: History,
        steady: f64,
        unix: f64,
        east: i32,
    }

    impl Listener {
        fn starting(unix: i64, east: i32) -> Listener {
            Listener { tracker: Tracker::default(), history: History::default(), steady: 500.0, unix: unix as f64, east }
        }

        fn new() -> Listener {
            Listener::starting(moment(day(2026, 11, 20), 9, 0), 0)
        }

        fn look(&mut self, playing: Option<(Playing, &Song)>) {
            let clock = Clock { steady: self.steady, unix: self.unix.floor() as i64, east: self.east };
            let report = match playing {
                Some((p, song)) => self.tracker.tick(clock, Some(p), || song.clone()),
                None => self.tracker.tick(clock, None, || unreachable!("nothing plays")),
            };
            if let Some(r) = report {
                self.history.take(r);
            }
        }

        fn pass(&mut self, secs: f64) {
            self.steady += secs;
            self.unix += secs;
        }

        /// `secs` seconds of a song from `from` seconds in, at `speed` times its pace.
        fn play_at(&mut self, key: u64, song: &Song, from: f64, secs: f64, length: Option<f64>, speed: f64) {
            let looks = (secs * 4.0) as usize;
            for i in 0..=looks {
                self.look(Some((Playing { key, id: &song.id, position: from + i as f64 * 0.25 * speed, length }, song)));
                if i < looks {
                    self.pass(0.25);
                }
            }
        }

        /// The same of a song 200 seconds long, at its own pace.
        fn play(&mut self, key: u64, song: &Song, from: f64, secs: f64) {
            self.play_at(key, song, from, secs, Some(200.0), 1.0);
        }

        /// Nothing plays for `secs` seconds: paused, or stopped.
        fn rest(&mut self, secs: f64) {
            for _ in 0..(secs * 4.0) as usize {
                self.look(None);
                self.pass(0.25);
            }
            self.look(None);
        }

        fn heard(&self, song: &Song) -> u64 {
            self.history.of(song, ALWAYS).0
        }

        fn plays(&self, song: &Song) -> u32 {
            self.history.of(song, ALWAYS).1
        }
    }

    // ---------------------------------------------------------------- days

    #[test]
    fn days_are_counted_from_the_first_of_january_1970() {
        assert_eq!(Date::of(0), Date { year: 1970, month: 1, day: 1 });
        assert_eq!(day(2000, 1, 1), 10_957);
        assert_eq!(day(2024, 2, 29), 19_782);
        assert_eq!(day(2024, 3, 1), 19_783);
        assert_eq!(day(2026, 11, 20), 20_777);
        assert_eq!(Date::of(20_777), Date { year: 2026, month: 11, day: 20 });
        assert_eq!(Date::of(-1), Date { year: 1969, month: 12, day: 31 });
    }

    #[test]
    fn every_day_has_one_date_and_they_follow_each_other() {
        let mut last = Date::of(day(1999, 12, 31) - 1);
        for n in day(1999, 12, 31)..day(2032, 1, 2) {
            let d = Date::of(n);
            assert_eq!(d.number(), n, "{d:?}");
            let next_day = d.day == last.day + 1 && d.month == last.month && d.year == last.year;
            let next_month = d.day == 1 && (d.month == last.month + 1 && d.year == last.year || d.month == 1 && last.month == 12 && d.year == last.year + 1);
            assert!(next_day || next_month, "{last:?} then {d:?}");
            last = d;
        }
    }

    #[test]
    fn the_week_starts_on_monday() {
        // 20 November 2026 is a Friday.
        assert_eq!(weekday(day(2026, 11, 20)), 4);
        assert_eq!(weekday(day(2026, 11, 16)), 0);
        assert_eq!(weekday(day(2026, 11, 22)), 6);
        assert_eq!(weekday(day(1969, 12, 29)), 0);
    }

    #[test]
    fn a_moment_falls_on_the_day_it_is_where_the_listener_is() {
        let evening = moment(day(2026, 11, 20), 18, 30);
        assert_eq!(local_day(evening, 0), day(2026, 11, 20));
        // Half past one at night in Jakarta, already the 21st.
        assert_eq!(local_day(evening, 7 * 60), day(2026, 11, 21));
        let night = moment(day(2026, 11, 20), 3, 0);
        // Ten in the evening of the 19th in New York.
        assert_eq!(local_day(night, -5 * 60), day(2026, 11, 19));
        assert_eq!(local_day(-1, 0), -1);
    }

    #[test]
    fn a_week_runs_from_monday_to_sunday() {
        let week = Period::Week.around(day(2026, 11, 20));
        assert_eq!(week, Span { first: day(2026, 11, 16), last: day(2026, 11, 22) });
        assert_eq!(Period::Week.around(day(2026, 11, 16)), week);
        assert_eq!(Period::Week.around(day(2026, 11, 22)), week);
        assert_eq!(Period::Week.before(day(2026, 11, 20)), Span { first: day(2026, 11, 9), last: day(2026, 11, 15) });
    }

    #[test]
    fn a_month_is_the_calendars() {
        assert_eq!(Period::Month.around(day(2026, 11, 20)), Span { first: day(2026, 11, 1), last: day(2026, 11, 30) });
        assert_eq!(Period::Month.around(day(2024, 2, 10)), Span { first: day(2024, 2, 1), last: day(2024, 2, 29) });
        assert_eq!(Period::Month.around(day(2026, 12, 31)), Span { first: day(2026, 12, 1), last: day(2026, 12, 31) });
        assert_eq!(Period::Month.before(day(2026, 11, 20)), Span { first: day(2026, 10, 1), last: day(2026, 10, 31) });
        // January's is the December of the year before.
        assert_eq!(Period::Month.before(day(2027, 1, 5)), Span { first: day(2026, 12, 1), last: day(2026, 12, 31) });
    }

    #[test]
    fn a_year_is_the_calendars() {
        assert_eq!(Period::Year.around(day(2026, 11, 20)), Span { first: day(2026, 1, 1), last: day(2026, 12, 31) });
        assert_eq!(Period::Year.before(day(2026, 11, 20)), Span { first: day(2025, 1, 1), last: day(2025, 12, 31) });
    }

    // ---------------------------------------------------------------- what is heard

    #[test]
    fn a_song_heard_for_thirty_seconds_is_a_play() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 29.75);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (29, 0));
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 30.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (30, 1));
    }

    #[test]
    fn a_song_shorter_than_a_minute_is_a_play_at_half_its_length() {
        let a = song("a");
        let mut l = Listener::new();
        l.play_at(1, &a, 0.0, 19.75, Some(40.0), 1.0);
        l.rest(1.0);
        assert_eq!(l.plays(&a), 0);
        let mut l = Listener::new();
        l.play_at(1, &a, 0.0, 20.0, Some(40.0), 1.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (20, 1));
        // A minute and more goes by the thirty seconds.
        let mut l = Listener::new();
        l.play_at(1, &a, 0.0, 29.75, Some(60.0), 1.0);
        l.rest(1.0);
        assert_eq!(l.plays(&a), 0);
    }

    #[test]
    fn a_song_whose_length_is_not_known_is_a_play_at_thirty_seconds() {
        let a = song("a");
        let mut l = Listener::new();
        l.play_at(1, &a, 0.0, 29.75, None, 1.0);
        l.rest(1.0);
        assert_eq!(l.plays(&a), 0);
        l.play_at(1, &a, 29.75, 0.25, None, 1.0);
        l.rest(1.0);
        assert_eq!(l.plays(&a), 1);
    }

    #[test]
    fn the_seconds_of_a_song_that_was_skipped_are_still_listened_to() {
        let (a, b) = (song("a"), song("b"));
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 12.0);
        l.play(2, &b, 0.0, 45.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (12, 0));
        assert_eq!((l.heard(&b), l.plays(&b)), (45, 1));
        let today = local_day(l.unix as i64, 0);
        let s = l.history.summary(Period::Week, today);
        assert_eq!((s.heard, s.plays), (57, 1));
    }

    #[test]
    fn a_song_that_never_played_a_second_leaves_nothing() {
        let (a, b) = (song("a"), song("b"));
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 0.75);
        l.play(2, &b, 0.0, 5.0);
        l.rest(1.0);
        assert_eq!(l.history.plays.len(), 1);
        assert_eq!(l.history.songs, [b]);
    }

    #[test]
    fn a_pause_is_not_heard() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 10.0);
        l.rest(600.0);
        l.play(1, &a, 10.0, 10.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (20, 0));
        // And it is one play, however long the pause: ten more seconds make it one.
        l.play(1, &a, 20.0, 10.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (30, 1));
        assert_eq!(l.history.plays.len(), 1);
    }

    #[test]
    fn what_a_seek_jumps_over_is_not_heard() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 10.0);
        l.play(1, &a, 150.0, 10.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (20, 0));
    }

    #[test]
    fn a_part_gone_back_to_is_heard_again() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 40.0);
        l.play(1, &a, 20.0, 20.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (60, 1));
    }

    #[test]
    fn at_another_speed_it_is_the_time_on_the_clock_that_is_heard() {
        let a = song("a");
        let mut l = Listener::new();
        l.play_at(1, &a, 0.0, 20.0, Some(200.0), 2.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (20, 0));
        let mut l = Listener::new();
        l.play_at(1, &a, 0.0, 40.0, Some(200.0), 0.5);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (40, 1));
    }

    #[test]
    fn a_stream_that_stalls_is_not_heard() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 10.0);
        // It says it plays, and stands still for a minute.
        l.play_at(1, &a, 10.0, 60.0, Some(200.0), 0.0);
        l.play(1, &a, 10.0, 5.0);
        l.rest(1.0);
        assert!((15..=17).contains(&l.heard(&a)), "{}", l.heard(&a));
        assert_eq!(l.plays(&a), 0);
    }

    #[test]
    fn time_nobody_looked_counts_only_as_far_as_the_song_moved_on() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 10.0);
        // Paused and started again while the app was held up: a minute later the song
        // is five seconds on.
        l.pass(60.0);
        l.play(1, &a, 15.0, 5.0);
        l.rest(1.0);
        assert_eq!(l.heard(&a), 20);
        // Held up while it played on: all of it was heard.
        l.play(1, &a, 20.0, 1.0);
        l.pass(8.0);
        l.play(1, &a, 29.0, 1.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (30, 1));
    }

    #[test]
    fn a_song_started_over_is_another_play() {
        let a = song("a");
        let mut l = Listener::new();
        // On repeat: the player starts its stream again.
        l.play(1, &a, 0.0, 200.0);
        l.play(2, &a, 0.0, 200.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (400, 2));
        // Back to its start by hand.
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 40.0);
        l.play(1, &a, 0.0, 40.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (80, 2));
        // Blending into itself (one song on repeat, with Automix): its stream starts
        // again a few seconds in.
        let mut l = Listener::new();
        l.play(1, &a, 150.0, 40.0);
        l.play(2, &a, 6.0, 40.0);
        l.rest(1.0);
        assert_eq!(l.plays(&a), 2);
    }

    #[test]
    fn a_stream_opened_again_where_it_broke_off_is_the_same_play() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 20.0);
        l.rest(3.0);
        l.play(2, &a, 20.0, 20.0);
        l.rest(1.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (40, 1));
        assert_eq!(l.history.plays.len(), 1);
    }

    #[test]
    fn a_song_still_playing_is_in_the_history_as_far_as_it_was_last_reported() {
        let a = song("a");
        let mut l = Listener::new();
        l.play(1, &a, 0.0, 9.75);
        assert!(l.history.is_empty());
        l.play(1, &a, 9.75, 15.25);
        assert_eq!((l.heard(&a), l.plays(&a)), (20, 0));
        // The moment it becomes a play, that is in.
        l.play(1, &a, 25.0, 5.0);
        assert_eq!((l.heard(&a), l.plays(&a)), (30, 1));
        // And a pause brings it up to date.
        l.play(1, &a, 30.0, 7.0);
        l.rest(0.25);
        assert_eq!(l.heard(&a), 37);
        assert_eq!(l.history.plays.len(), 1);
    }

    #[test]
    fn a_song_heard_across_midnight_is_heard_on_both_days() {
        let a = song("a");
        // A minute to midnight on the 20th in Jakarta (UTC+7).
        let (d20, d21) = (day(2026, 11, 20), day(2026, 11, 21));
        let mut l = Listener::starting(moment(d20, 16, 59), 7 * 60);
        l.play(1, &a, 0.0, 120.0);
        l.rest(1.0);
        let (before, after) = (l.history.of(&a, Span { first: d20, last: d20 }), l.history.of(&a, Span { first: d21, last: d21 }));
        assert!((59..=60).contains(&before.0) && (60..=61).contains(&after.0), "{before:?} {after:?}");
        assert!((119..=120).contains(&(before.0 + after.0)));
        // One play, on the day its thirtieth second was heard.
        assert_eq!((before.1, after.1), (1, 0));

        // Ten seconds before midnight: it becomes a play on the 21st.
        let mut l = Listener::starting(moment(d20, 16, 59) + 50, 7 * 60);
        l.play(1, &a, 0.0, 60.0);
        l.rest(1.0);
        let (before, after) = (l.history.of(&a, Span { first: d20, last: d20 }), l.history.of(&a, Span { first: d21, last: d21 }));
        assert!((9..=10).contains(&before.0) && (50..=51).contains(&after.0), "{before:?} {after:?}");
        assert_eq!((before.1, after.1), (0, 1));
    }

    #[test]
    fn a_song_heard_into_a_new_month_is_heard_in_both() {
        let a = song("a");
        // A minute to midnight on 30 November, five hours behind UTC.
        let mut l = Listener::starting(moment(day(2026, 12, 1), 4, 59), -5 * 60);
        l.play(1, &a, 0.0, 120.0);
        l.rest(1.0);
        let year = l.history.summary(Period::Year, day(2026, 12, 1));
        assert!((59..=60).contains(&year.bars[10]) && (60..=61).contains(&year.bars[11]), "{:?}", year.bars);
        let december = l.history.summary(Period::Month, day(2026, 12, 1));
        assert!((60..=61).contains(&december.heard));
        assert_eq!(december.bars[0], december.heard);
        // The play is November's.
        assert_eq!(december.plays, 0);
        assert_eq!(l.history.summary(Period::Month, day(2026, 11, 30)).plays, 1);
    }

    // ---------------------------------------------------------------- adding up

    #[test]
    fn an_empty_history_has_nothing_to_show() {
        let h = History::default();
        assert!(h.is_empty());
        assert_eq!(h.since(), None);
        assert_eq!(h.plays(), 0);
        let s = h.summary(Period::Month, day(2026, 11, 20));
        assert_eq!((s.heard, s.plays), (0, 0));
        assert_eq!(s.bars, vec![0; 30]);
        assert_eq!((s.pace, s.top_artist, s.top_album), (None, None, None));
        assert!(s.top_songs.is_empty());
    }

    #[test]
    fn a_week_has_a_bar_for_each_day() {
        let (a, b) = (song("a"), song("b"));
        let mut h = History::default();
        h.played(&a, day(2026, 11, 16), 2, 100);
        h.add(&b, day(2026, 11, 18), 20, false);
        h.played(&a, day(2026, 11, 20), 1, 60);
        // The Sunday before and the Monday after are other weeks'.
        h.played(&a, day(2026, 11, 15), 1, 999);
        h.played(&a, day(2026, 11, 23), 1, 999);
        let s = h.summary(Period::Week, day(2026, 11, 20));
        assert_eq!(s.bars, [200, 0, 20, 0, 60, 0, 0]);
        assert_eq!(s.today, 4);
        assert_eq!((s.heard, s.plays), (280, 3));
        assert_eq!(s.first, Date { year: 2026, month: 11, day: 16 });
        assert_eq!(s.before, Date { year: 2026, month: 11, day: 9 });
    }

    #[test]
    fn a_month_has_a_bar_for_each_day() {
        let a = song("a");
        let mut h = History::default();
        h.played(&a, day(2026, 11, 1), 1, 50);
        h.played(&a, day(2026, 11, 20), 3, 40);
        h.played(&a, day(2026, 11, 30), 1, 70);
        h.played(&a, day(2026, 10, 31), 1, 999);
        h.played(&a, day(2026, 12, 1), 1, 999);
        let s = h.summary(Period::Month, day(2026, 11, 20));
        assert_eq!(s.bars.len(), 30);
        assert_eq!((s.bars[0], s.bars[19], s.bars[29]), (50, 120, 70));
        assert_eq!(s.today, 19);
        assert_eq!((s.heard, s.plays), (240, 5));
        assert_eq!(h.summary(Period::Month, day(2024, 2, 10)).bars.len(), 29);
    }

    #[test]
    fn a_year_has_a_bar_for_each_month() {
        let a = song("a");
        let mut h = History::default();
        h.played(&a, day(2026, 1, 1), 1, 50);
        h.played(&a, day(2026, 11, 3), 1, 40);
        h.played(&a, day(2026, 11, 20), 1, 40);
        h.played(&a, day(2026, 12, 31), 1, 70);
        h.played(&a, day(2025, 12, 31), 1, 999);
        h.played(&a, day(2027, 1, 1), 1, 999);
        let s = h.summary(Period::Year, day(2026, 11, 20));
        assert_eq!(s.bars, [50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 80, 70]);
        assert_eq!(s.today, 10);
        assert_eq!((s.heard, s.plays), (200, 4));
        assert_eq!(s.before, Date { year: 2025, month: 1, day: 1 });
    }

    #[test]
    fn a_play_belongs_to_the_listeners_day_not_utcs() {
        let a = song("a");
        let mut h = History::default();
        // 18:30 UTC on 30 November is half past one on 1 December in Jakarta.
        h.take(Report { song: Some(a.clone()), at: moment(day(2026, 11, 30), 18, 30), east: 7 * 60, heard: 100, counted: true });
        assert_eq!(h.since(), Some(day(2026, 12, 1)));
        assert_eq!(h.summary(Period::Month, day(2026, 11, 30)).heard, 0);
        let december = h.summary(Period::Month, day(2026, 12, 1));
        assert_eq!((december.heard, december.bars[0]), (100, 100));
        // It was a Tuesday there: the week that began on 30 November.
        assert_eq!(h.summary(Period::Week, day(2026, 12, 1)).bars, [0, 100, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn the_days_that_are_over_are_compared_with_as_many_of_the_period_before() {
        let a = song("a");
        let mut h = History::default();
        h.add(&a, day(2026, 9, 28), 10, false);
        // October: 6000 seconds by the 19th, far more after it.
        h.add(&a, day(2026, 10, 1), 2000, true);
        h.add(&a, day(2026, 10, 19), 4000, true);
        h.add(&a, day(2026, 10, 20), 30_000, true);
        h.add(&a, day(2026, 10, 31), 30_000, true);
        // November: 6720 by the 19th, and a lot today, the 20th.
        h.add(&a, day(2026, 11, 2), 6000, true);
        h.add(&a, day(2026, 11, 19), 720, true);
        h.add(&a, day(2026, 11, 20), 9000, true);
        let s = h.summary(Period::Month, day(2026, 11, 20));
        assert_eq!(s.pace, Some(Pace { now: 6720, then: 6000 }));
        assert_eq!(s.pace.unwrap().percent(), 12);
        assert_eq!(s.heard, 15_720);
        // The week: Monday to Thursday of each.
        h.add(&a, day(2026, 11, 9), 500, true);
        h.add(&a, day(2026, 11, 12), 500, true);
        h.add(&a, day(2026, 11, 13), 7000, true);
        h.add(&a, day(2026, 11, 17), 250, true);
        assert_eq!(h.summary(Period::Week, day(2026, 11, 20)).pace, Some(Pace { now: 970, then: 1000 }));
    }

    #[test]
    fn a_year_is_compared_by_its_days_too() {
        let a = song("a");
        let mut h = History::default();
        h.add(&a, day(2024, 12, 31), 10, false);
        h.add(&a, day(2025, 3, 1), 1000, true);
        h.add(&a, day(2025, 11, 19), 1000, true);
        h.add(&a, day(2025, 11, 20), 5000, true);
        h.add(&a, day(2026, 6, 1), 3000, true);
        h.add(&a, day(2026, 11, 20), 800, true);
        assert_eq!(h.summary(Period::Year, day(2026, 11, 20)).pace, Some(Pace { now: 3000, then: 2000 }));
    }

    #[test]
    fn a_period_that_has_only_begun_is_not_compared() {
        let a = song("a");
        let mut h = History::default();
        h.add(&a, day(2026, 9, 1), 1000, true);
        h.add(&a, day(2026, 10, 1), 1000, true);
        h.add(&a, day(2026, 11, 1), 50, true);
        h.add(&a, day(2026, 11, 2), 50, true);
        // The first day isn't over.
        assert_eq!(h.summary(Period::Month, day(2026, 11, 1)).pace, None);
        assert_eq!(h.summary(Period::Week, day(2026, 11, 2)).pace, None);
        assert_eq!(h.summary(Period::Year, day(2027, 1, 1)).pace, None);
        assert_eq!(h.summary(Period::Month, day(2026, 11, 2)).pace, Some(Pace { now: 50, then: 1000 }));
    }

    #[test]
    fn nothing_is_compared_with_a_time_before_the_history_began() {
        let a = song("a");
        let mut h = History::default();
        // The first song ever was on 10 October: that October is no whole month.
        h.add(&a, day(2026, 10, 10), 5000, true);
        h.add(&a, day(2026, 11, 13), 5000, true);
        h.add(&a, day(2026, 11, 18), 5000, true);
        assert_eq!(h.summary(Period::Month, day(2026, 11, 20)).pace, None);
        assert_eq!(h.summary(Period::Year, day(2026, 11, 20)).pace, None);
        // Last week is all there, with nothing in its first four days.
        assert_eq!(h.summary(Period::Week, day(2026, 11, 20)).pace, None);
        h.add(&a, day(2026, 11, 9), 2500, true);
        assert_eq!(h.summary(Period::Week, day(2026, 11, 20)).pace, Some(Pace { now: 5000, then: 2500 }));
        // In December, November is whole.
        h.add(&a, day(2026, 12, 3), 100, true);
        assert_eq!(h.summary(Period::Month, day(2026, 12, 15)).pace, Some(Pace { now: 100, then: 7500 }));
        // A history that begins in the period itself has no period before.
        let mut h = History::default();
        h.add(&a, day(2026, 11, 3), 5000, true);
        assert_eq!(h.summary(Period::Month, day(2026, 11, 20)).pace, None);
    }

    #[test]
    fn a_long_month_is_compared_with_all_of_a_shorter_one() {
        let a = song("a");
        let mut h = History::default();
        h.add(&a, day(2027, 1, 31), 10, false);
        h.add(&a, day(2027, 2, 1), 1000, true);
        h.add(&a, day(2027, 2, 28), 1000, true);
        h.add(&a, day(2027, 3, 1), 500, true);
        h.add(&a, day(2027, 3, 30), 500, true);
        // 30 days of March are over, and February has 28.
        assert_eq!(h.summary(Period::Month, day(2027, 3, 31)).pace, Some(Pace { now: 1000, then: 2000 }));
    }

    #[test]
    fn more_and_less_are_percents_of_the_period_before() {
        assert_eq!(Pace { now: 6720, then: 6000 }.percent(), 12);
        assert_eq!(Pace { now: 4500, then: 6000 }.percent(), -25);
        assert_eq!(Pace { now: 0, then: 6000 }.percent(), -100);
        assert_eq!(Pace { now: 6000, then: 6000 }.percent(), 0);
        assert_eq!(Pace { now: 6020, then: 6000 }.percent(), 0);
        assert_eq!(Pace { now: 18_000, then: 6000 }.percent(), 200);
    }

    #[test]
    fn the_top_songs_are_the_most_played_of_the_period() {
        let songs: Vec<Song> = ["a", "b", "c", "d", "e", "f", "g"].map(song).into();
        let mut h = History::default();
        let today = day(2026, 11, 20);
        for (i, s) in songs.iter().enumerate() {
            h.played(s, today - i as Day, i as u32 + 1, 100);
        }
        // Heard a lot, never long enough to be a play.
        let skipped = song("skipped");
        for _ in 0..50 {
            h.add(&skipped, today, 25, false);
        }
        // Played most of all, last month.
        h.played(&song("october"), day(2026, 10, 20), 40, 100);
        let s = h.summary(Period::Month, today);
        let top: Vec<(&str, u32, u64)> = s.top_songs.iter().map(|t| (t.song.id.as_str(), t.plays, t.heard)).collect();
        assert_eq!(top, [("g", 7, 700), ("f", 6, 600), ("e", 5, 500), ("d", 4, 400), ("c", 3, 300)]);
        assert_eq!(s.top_songs[0].song, songs[6]);
        assert_eq!(h.summary(Period::Year, today).top_songs[0].song.id, "october");
    }

    #[test]
    fn songs_played_as_often_go_by_the_time_heard_then_by_title() {
        let mut h = History::default();
        let today = day(2026, 11, 20);
        let titled = |id: &str, title: &str| Song { title: title.to_owned(), ..song(id) };
        let (long, zebra, apple, apple2) = (titled("1", "Long"), titled("2", "Zebra"), titled("3", "Apple"), titled("4", "Apple"));
        h.played(&zebra, today, 2, 100);
        h.played(&apple2, today, 2, 100);
        h.played(&long, today, 2, 150);
        h.played(&apple, today, 2, 100);
        // The same however they were met.
        let ids = |h: &History| h.summary(Period::Week, today).top_songs.iter().map(|t| t.song.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&h), ["1", "3", "4", "2"]);
        let mut other = History::default();
        for s in [&apple, &long, &apple2, &zebra] {
            other.played(s, today, 2, if s.id == "1" { 150 } else { 100 });
        }
        assert_eq!(ids(&other), ["1", "3", "4", "2"]);
    }

    #[test]
    fn the_top_artist_is_the_one_heard_longest() {
        let by = |id: &str, artists: &[&str]| Song { artists: artists.iter().map(|a| (*a).to_owned()).collect(), ..song(id) };
        let mut h = History::default();
        let today = day(2026, 11, 20);
        h.played(&by("1", &["Senja Kala"]), today, 1, 300);
        h.played(&by("2", &["Senja Kala", "Rona Jingga"]), today - 1, 1, 200);
        // More plays, less time.
        h.played(&by("3", &["Arunika"]), today, 4, 100);
        // Skipped songs are heard too.
        h.add(&by("4", &["Arunika"]), today, 20, false);
        // A song that names nobody.
        h.played(&by("5", &[]), today, 1, 900);
        let s = h.summary(Period::Week, today);
        assert_eq!(s.top_artist, Some(Top { name: "Senja Kala".to_owned(), heard: 500 }));
        h.add(&by("4", &["Arunika"]), today, 81, false);
        assert_eq!(h.summary(Period::Week, today).top_artist, Some(Top { name: "Arunika".to_owned(), heard: 501 }));
    }

    #[test]
    fn artists_heard_as_long_go_by_their_plays_then_by_name() {
        let by = |id: &str, artist: &str| Song { artists: vec![artist.to_owned()], ..song(id) };
        let today = day(2026, 11, 20);
        let mut h = History::default();
        h.played(&by("1", "Zed"), today, 1, 200);
        h.played(&by("2", "Abe"), today, 1, 200);
        h.played(&by("3", "Mia"), today, 1, 200);
        assert_eq!(h.summary(Period::Week, today).top_artist.unwrap().name, "Abe");
        let mut h = History::default();
        h.played(&by("1", "Abe"), today, 1, 200);
        h.played(&by("2", "Zed"), today, 2, 100);
        assert_eq!(h.summary(Period::Week, today).top_artist.unwrap().name, "Zed");
    }

    #[test]
    fn the_top_album_needs_half_the_plays_to_say_their_album() {
        let on = |id: &str, album: Option<&str>| Song { album: album.map(str::to_owned), ..song(id) };
        let today = day(2026, 11, 20);
        let mut h = History::default();
        h.played(&on("1", Some("Layang-Layang")), today, 2, 200);
        h.played(&on("2", Some("Layang-Layang")), today, 1, 100);
        h.played(&on("3", Some("Senja")), today, 1, 450);
        h.played(&on("4", None), today, 4, 100);
        let s = h.summary(Period::Week, today);
        assert_eq!(s.top_album, Some(Top { name: "Layang-Layang".to_owned(), heard: 500 }));
        // One more play without an album: now most don't say.
        h.played(&on("4", None), today, 1, 100);
        assert_eq!(h.summary(Period::Week, today).top_album, None);
        // Nothing that was a play: no album either.
        let mut h = History::default();
        h.add(&on("1", Some("Layang-Layang")), today, 20, false);
        assert_eq!(h.summary(Period::Week, today).top_album, None);
    }

    // ---------------------------------------------------------------- keeping it

    /// A history with every kind of thing in it.
    fn a_history() -> History {
        let mut h = History::default();
        let plain = song("dQw4w9WgXcQ");
        let full = Song {
            id: "local:/storage/emulated/0/Music/Hujan di Kemang.flac".to_owned(),
            title: "Hujan di Kemang \u{2014} \u{96e8}".to_owned(),
            artists: vec!["Senja Kala".to_owned(), "Rona Jingga".to_owned()],
            album: Some("Layang-Layang".to_owned()),
            cover: Some(Cover { url: "https://lh3.googleusercontent.com/abc=w120-h120-l90-rj".to_owned(), width: 120, height: 90 }),
        };
        let today = day(2026, 11, 20);
        h.played(&plain, today - 300, 3, 180);
        h.add(&full, today - 300, 12, false);
        h.played(&full, today - 200, 1, 240);
        h.take(Report { song: Some(full.clone()), at: moment(today - 2, 23, 10), east: -(9 * 60 + 30), heard: 200, counted: true });
        h.take(Report { song: Some(plain.clone()), at: moment(today, 1, 0), east: 7 * 60, heard: 100_000, counted: false });
        h.fold(today);
        assert!(!h.days.is_empty() && !h.plays.is_empty());
        h
    }

    #[test]
    fn a_history_comes_back_from_its_file_as_it_was() {
        let h = a_history();
        let bytes = h.encode();
        assert_eq!(decode(&bytes), Stored::Read(a_history(), bytes.len()));
        let empty = History::default().encode();
        assert_eq!(empty, HEADER);
        assert_eq!(decode(&empty), Stored::Read(History::default(), HEADER.len()));
    }

    #[test]
    fn a_file_cut_short_keeps_every_record_that_is_whole() {
        let h = a_history();
        let bytes = h.encode();
        let mut last = 0;
        for cut in 0..bytes.len() {
            let Stored::Read(part, good) = decode(&bytes[..cut]) else { panic!("cut at {cut}") };
            assert!(good <= cut && good >= last, "cut at {cut}");
            last = good;
            // What is there is the beginning of what was written.
            assert_eq!(part.songs, h.songs[..part.songs.len()]);
            assert_eq!(part.plays, h.plays[..part.plays.len()]);
            assert!(part.days.iter().all(|(day, tallies)| h.days.get(day) == Some(tallies)));
            // Nothing is read of one whose first bytes aren't whole.
            assert_eq!(if good == 0 { Vec::new() } else { part.encode() }, bytes[..good]);
        }
        assert!(last > HEADER.len());
    }

    #[test]
    fn a_damaged_record_ends_the_reading_before_it() {
        let h = a_history();
        let bytes = h.encode();
        for at in HEADER.len()..bytes.len() {
            let mut bad = bytes.clone();
            bad[at] ^= 0x40;
            let Stored::Read(part, good) = decode(&bad) else { panic!("damaged at {at}") };
            assert!(good <= at, "damaged at {at}: read {good}");
            assert_eq!(part.encode(), bytes[..good]);
        }
    }

    #[test]
    fn a_file_of_another_kind_or_a_later_version_is_not_read() {
        assert_eq!(decode(b"{\"plays\":[]}"), Stored::Foreign);
        assert_eq!(decode(b"BCLH\x02rest"), Stored::Foreign);
        assert_eq!(decode(b"BCLX"), Stored::Foreign);
        // A file that never got its first bytes whole is an empty one.
        assert_eq!(decode(b""), Stored::Read(History::default(), 0));
        assert_eq!(decode(b"BCL"), Stored::Read(History::default(), 0));
    }

    #[test]
    fn plays_from_long_ago_become_their_days_totals() {
        let (a, b) = (song("a"), song("b"));
        let today = day(2026, 11, 20);
        let mut h = History::default();
        h.played(&a, today - 200, 3, 100);
        h.add(&a, today - 200, 15, false);
        h.played(&b, today - 200, 1, 50);
        h.played(&a, today - 93, 1, 60);
        h.played(&a, today - 92, 1, 70);
        h.played(&b, today, 2, 80);
        assert!(h.fold(today));
        assert_eq!(h.days.len(), 2);
        assert_eq!(h.days[&(today - 200)], [Tally { song: 0, heard: 315, plays: 3 }, Tally { song: 1, heard: 50, plays: 1 }]);
        assert_eq!(h.days[&(today - 93)], [Tally { song: 0, heard: 60, plays: 1 }]);
        assert_eq!(h.plays.len(), 3);
        assert_eq!(h.plays(), 8);
        assert_eq!(h.since(), Some(today - 200));
        // Nothing more to fold, until time has gone on.
        assert!(!h.fold(today));
        assert!(h.fold(today + 1));
        assert_eq!(h.days[&(today - 92)], [Tally { song: 0, heard: 70, plays: 1 }]);
        // A day folded into twice adds up.
        h.played(&b, today - 200, 1, 5);
        h.played(&song("c"), today - 200, 1, 7);
        assert!(h.fold(today + 1));
        assert_eq!(h.days[&(today - 200)], [Tally { song: 0, heard: 315, plays: 3 }, Tally { song: 1, heard: 55, plays: 2 }, Tally { song: 2, heard: 7, plays: 1 }]);
    }

    #[test]
    fn every_number_is_the_same_after_folding() {
        let today = day(2026, 11, 20);
        let mut h = History::default();
        // Two years of listening, a few songs a day, some of them skipped.
        let songs: Vec<Song> = (0..9).map(|i| Song { album: (i % 3 != 0).then(|| format!("Album {}", i % 2)), artists: vec![format!("Artist {}", i % 4)], ..song(&i.to_string()) }).collect();
        for back in 0..730 {
            for k in 0..3 {
                let s = &songs[((back * 7 + k * 5) % 9) as usize];
                h.take(Report { song: Some(s.clone()), at: moment(today - back, 20 + k as i64, 30), east: 7 * 60, heard: 20 + ((back * 13 + k * 31) % 200) as u32, counted: (back + k) % 4 != 0 });
            }
        }
        let periods = [Period::Week, Period::Month, Period::Year];
        let before: Vec<Summary> = periods.iter().map(|p| h.summary(*p, today)).collect();
        let (since, plays) = (h.since(), h.plays());
        assert!(before.iter().all(|s| s.pace.is_some() && s.top_album.is_some() && s.top_songs.len() == 5));
        assert!(h.fold(today));
        assert!(h.plays.len() < 300 && h.days.len() > 600);
        let after: Vec<Summary> = periods.iter().map(|p| h.summary(*p, today)).collect();
        assert_eq!(before, after);
        assert_eq!((h.since(), h.plays()), (since, plays));
        // Years of it stay small: a day's songs take a few bytes each.
        let size = h.encode().len();
        assert!(size < 25_000, "{size} bytes");
    }

    /// A folder of its own for a test's file.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitchord-stats-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `secs` seconds of a song from its start, looked at four times a second, on a
    /// clock that reads `*steady`.
    fn listen(log: &mut Log, steady: &mut f64, key: u64, song: &Song, secs: f64) {
        for i in 0..=(secs * 4.0) as usize {
            let position = i as f64 * 0.25;
            let clock = Clock { steady: *steady + position, unix: moment(day(2026, 11, 20), 9, 0) + (*steady + position) as i64, east: 0 };
            log.tick(clock, Some(Playing { key, id: &song.id, position, length: Some(200.0) }), || song.clone());
        }
        *steady += secs;
    }

    #[test]
    fn what_was_heard_is_there_when_the_app_starts_again() {
        let dir = folder("restart");
        let today = day(2026, 11, 20);
        let (a, b) = (song("a"), song("b"));
        let mut steady = 0.0;
        let mut log = Log::open(&dir, today);
        assert!(log.history().is_empty());
        listen(&mut log, &mut steady, 1, &a, 45.0);
        listen(&mut log, &mut steady, 2, &b, 12.0);
        listen(&mut log, &mut steady, 3, &a, 31.0);
        // Killed while the third still plays: no goodbye.
        drop(log);
        let log = Log::open(&dir, today);
        let h = log.history();
        // All but the second it was in the middle of.
        assert_eq!(h.of(&a, ALWAYS), (45 + 30, 2));
        assert_eq!(h.of(&b, ALWAYS), (12, 0));
        assert_eq!(h.songs, [a, b]);
        assert_eq!(h.plays.len(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_kill_loses_no_more_than_the_last_seconds_of_the_song_playing() {
        let dir = folder("kill");
        let today = day(2026, 11, 20);
        let (a, b) = (song("a"), song("b"));
        let mut steady = 0.0;
        let mut log = Log::open(&dir, today);
        listen(&mut log, &mut steady, 1, &a, 40.0);
        listen(&mut log, &mut steady, 2, &b, 27.0);
        drop(log);
        let h = Log::open(&dir, today);
        assert_eq!(h.history().of(&a, ALWAYS), (40, 1));
        assert_eq!(h.history().of(&b, ALWAYS), (20, 0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_file_holds_each_play_once_however_long_it_played() {
        let dir = folder("once");
        let today = day(2026, 11, 20);
        let a = song("a");
        let mut steady = 0.0;
        let mut log = Log::open(&dir, today);
        listen(&mut log, &mut steady, 1, &a, 190.0);
        let written = std::fs::read(dir.join(FILE)).unwrap();
        assert_eq!(written, log.history().encode());
        assert_eq!(log.history().plays.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_with_a_torn_end_is_read_up_to_it_and_written_on() {
        let dir = folder("torn");
        let today = day(2026, 11, 20);
        let (a, b) = (song("a"), song("b"));
        let mut steady = 0.0;
        let mut log = Log::open(&dir, today);
        listen(&mut log, &mut steady, 1, &a, 40.0);
        listen(&mut log, &mut steady, 2, &b, 35.0);
        drop(log);
        // The last record lost its end, as a write cut off by the power going would.
        let path = dir.join(FILE);
        let whole = std::fs::read(&path).unwrap();
        std::fs::write(&path, &whole[..whole.len() - 3]).unwrap();
        let mut log = Log::open(&dir, today);
        assert_eq!(log.history().of(&a, ALWAYS), (40, 1));
        assert_eq!(log.history().of(&b, ALWAYS), (0, 0));
        listen(&mut log, &mut steady, 3, &b, 33.0);
        drop(log);
        let log = Log::open(&dir, today);
        assert_eq!(log.history().of(&a, ALWAYS), (40, 1));
        assert_eq!(log.history().of(&b, ALWAYS), (30, 1));
        // Nothing of the torn record is left in the file.
        let mended = std::fs::read(&path).unwrap();
        assert!(matches!(decode(&mended), Stored::Read(h, good) if h == *log.history() && good == mended.len()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn old_plays_are_folded_in_the_file_when_the_app_starts() {
        let dir = folder("fold");
        let today = day(2026, 11, 20);
        let a = song("a");
        let mut steady = 0.0;
        let mut log = Log::open(&dir, today);
        listen(&mut log, &mut steady, 1, &a, 40.0);
        listen(&mut log, &mut steady, 2, &a, 50.0);
        drop(log);
        // Half a year on.
        let later = today + 180;
        let mut log = Log::open(&dir, later);
        assert!(log.history().plays.is_empty());
        assert_eq!(log.history().days[&today], [Tally { song: 0, heard: 90, plays: 2 }]);
        assert_eq!(std::fs::read(dir.join(FILE)).unwrap(), log.history().encode());
        assert!(!dir.join("listening.part").exists());
        // And it is written on from there.
        listen(&mut log, &mut steady, 3, &a, 30.0);
        drop(log);
        assert_eq!(Log::open(&dir, later).history().of(&a, ALWAYS), (120, 3));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clearing_forgets_everything_and_starts_again() {
        let dir = folder("clear");
        let today = day(2026, 11, 20);
        let a = song("a");
        let mut steady = 0.0;
        let mut log = Log::open(&dir, today);
        listen(&mut log, &mut steady, 1, &a, 40.0);
        let before = log.revision();
        log.clear();
        assert!(log.history().is_empty());
        assert!(log.revision() > before);
        assert_eq!(std::fs::read(dir.join(FILE)).unwrap(), HEADER);
        // The song still playing is heard from here on.
        for i in 0..=48 {
            let position = 40.0 + i as f64 * 0.25;
            log.tick(Clock { steady: position, unix: moment(today, 9, 1), east: 0 }, Some(Playing { key: 1, id: &a.id, position, length: Some(200.0) }), || a.clone());
        }
        assert_eq!(log.history().of(&a, ALWAYS), (10, 0));
        drop(log);
        assert_eq!(Log::open(&dir, today).history().of(&a, ALWAYS), (10, 0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_it_cannot_read_is_put_aside_not_thrown_away() {
        let dir = folder("foreign");
        let today = day(2026, 11, 20);
        std::fs::write(dir.join(FILE), b"BCLH\x07a later version's history").unwrap();
        let a = song("a");
        let mut steady = 0.0;
        let mut log = Log::open(&dir, today);
        assert!(log.history().is_empty());
        listen(&mut log, &mut steady, 1, &a, 40.0);
        drop(log);
        assert_eq!(std::fs::read(dir.join("listening.old")).unwrap(), b"BCLH\x07a later version's history");
        assert_eq!(Log::open(&dir, today).history().of(&a, ALWAYS), (40, 1));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_that_cannot_be_written_still_counts_until_the_app_closes() {
        let dir = folder("gone").join("not-there");
        let a = song("a");
        let mut steady = 0.0;
        let mut log = Log::open(&dir, day(2026, 11, 20));
        listen(&mut log, &mut steady, 1, &a, 40.0);
        assert_eq!(log.history().of(&a, ALWAYS), (40, 1));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
