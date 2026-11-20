//! A lyric card: a few lines of a song as a picture to save or share. What is on it is
//! decided here (which lines, their text, the colours, the size, the text sizes to
//! try); the picture itself is drawn by Android (`LyricCard.java`), whose text layout
//! shapes every script.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::Color32;

use crate::lyrics::{Line, Lyrics};

/// The most lines a card carries: a quote from a song, not the song.
pub const MAX_LINES: usize = 4;

/// Whether this system can draw the picture (only Android's renderer is written).
pub const AVAILABLE: bool = cfg!(target_os = "android");

/// What a tap on a line did to the choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    Added,
    Removed,
    /// Four are chosen already: nothing changed.
    Full,
}

/// The lines chosen for the card, kept in the song's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    song: String,
    lines: Vec<usize>,
}

impl Selection {
    /// Keep to the song (and lyrics source) on screen: lines chosen in another one are
    /// not this one's. True when the choice was dropped for that.
    pub fn follow(&mut self, song: &str) -> bool {
        if self.song == song {
            return false;
        }
        self.song = song.to_owned();
        let dropped = !self.lines.is_empty();
        self.lines.clear();
        dropped
    }

    /// Choose `line`, or take it back when it is chosen.
    pub fn tap(&mut self, line: usize) -> Tap {
        match self.lines.binary_search(&line) {
            Ok(at) => {
                self.lines.remove(at);
                Tap::Removed
            }
            Err(_) if self.lines.len() >= MAX_LINES => Tap::Full,
            Err(at) => {
                self.lines.insert(at, line);
                Tap::Added
            }
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// The chosen lines, first in the song first.
    pub fn lines(&self) -> &[usize] {
        &self.lines
    }

    pub fn contains(&self, line: usize) -> bool {
        self.lines.contains(&line)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Text as one line: line breaks and runs of spaces become one space, and control
/// characters go.
fn one_line(text: &str) -> String {
    let kept: String = text.chars().filter(|c| !c.is_control() || c.is_whitespace()).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Can this line go on a card? One somebody sings, with words.
pub fn pickable(line: &Line) -> bool {
    !line.instrumental && !one_line(&line.text).is_empty()
}

/// What the card says for the lines `picked`: each as the lyrics view shows it as its
/// main text, which is the line as sung. Romanization and translation are notes under
/// a line there, never the line, so they stay off the card.
pub fn card_lines(lyrics: &Lyrics, picked: &[usize]) -> Vec<String> {
    picked.iter().filter_map(|&i| lyrics.lines.get(i)).filter(|l| pickable(l)).map(|l| one_line(&l.text)).take(MAX_LINES).collect()
}

/// The picture's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    Story,
    #[default]
    Square,
    Wide,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Story, Format::Square, Format::Wide];

    pub fn label(self) -> &'static str {
        match self {
            Format::Story => "Story 9:16",
            Format::Square => "Square",
            Format::Wide => "Wide 16:9",
        }
    }

    /// Width and height of the picture in pixels: 1080 on its short side.
    pub fn size(self) -> (u32, u32) {
        match self {
            Format::Story => (1080, 1920),
            Format::Square => (1080, 1080),
            Format::Wide => (1920, 1080),
        }
    }
}

/// The text sizes (pixels) to try for the card's lines, largest first: Android takes
/// the first at which the lines fit above the song's name, so that four long lines
/// are set smaller rather than cut.
pub fn text_sizes(format: Format) -> Vec<u32> {
    // For 1080 pixels on the short side; each step is about a ninth smaller.
    const STEPS: [u32; 8] = [96, 86, 76, 68, 60, 53, 47, 42];
    let (w, h) = format.size();
    STEPS.iter().map(|s| s * w.min(h) / 1080).collect()
}

/// A card's colours: its background runs from `from` (top left) to `to` (bottom
/// right), and `ink` is its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub from: Color32,
    pub to: Color32,
    pub ink: Color32,
}

/// How a card is coloured: after the song's cover, one of three fixed gradients, or dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    Cover,
    Teal,
    Violet,
    Rose,
    Dark,
}

impl Style {
    pub const ALL: [Style; 5] = [Style::Cover, Style::Teal, Style::Violet, Style::Rose, Style::Dark];

    /// Its name, for the screen reader.
    pub fn name(self) -> &'static str {
        match self {
            Style::Cover => "Cover colours",
            Style::Teal => "Teal",
            Style::Violet => "Violet",
            Style::Rose => "Rose",
            Style::Dark => "Dark",
        }
    }

    /// The style's colours; `cover` is the colour taken from the song's artwork.
    pub fn colors(self, cover: Color32) -> Colors {
        // The fixed ones are the card's own inks, like a paper's colour: they go into
        // the picture, not the app's interface, so they are not the theme's.
        let (from, to) = match self {
            Style::Cover => {
                let from = no_brighter(vivid(cover), LIGHTEST);
                (from, no_brighter(from, luminance(from) * 0.42))
            }
            Style::Teal => (Color32::from_rgb(0x1E, 0x94, 0x88), Color32::from_rgb(0x14, 0x50, 0x5C)),
            Style::Violet => (Color32::from_rgb(0x7A, 0x55, 0xC4), Color32::from_rgb(0x3C, 0x2A, 0x86)),
            Style::Rose => (Color32::from_rgb(0xCF, 0x57, 0x8A), Color32::from_rgb(0x74, 0x2C, 0x5A)),
            Style::Dark => (Color32::from_rgb(0x3A, 0x32, 0x30), Color32::from_rgb(0x1B, 0x15, 0x14)),
        };
        Colors { from, to, ink: Color32::WHITE }
    }
}

/// The brightest a card's background may be for white text on it to read (a little
/// under the 0.30 that gives large text its three to one).
const LIGHTEST: f32 = 0.28;

/// A colour channel as light: sRGB to linear.
fn linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// Linear light back to an sRGB channel.
fn srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let c = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round() as u8
}

/// How bright a colour is to the eye, 0 (black) to 1 (white): WCAG's relative luminance.
fn luminance(c: Color32) -> f32 {
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

/// `c` darkened, its hue kept, until it is no brighter than `max`.
fn no_brighter(c: Color32, max: f32) -> Color32 {
    let l = luminance(c);
    if l <= max {
        return c;
    }
    // Rounding to whole channels may land a hair over: aim a little under.
    let k = max / l * 0.98;
    Color32::from_rgb(srgb(linear(c.r()) * k), srgb(linear(c.g()) * k), srgb(linear(c.b()) * k))
}

/// A pale colour made vivid: darkened as it is, a pastel turns to mud. A grey has no
/// hue to bring out and stays grey.
fn vivid(c: Color32) -> Color32 {
    let (r, g, b) = (c.r() as f32, c.g() as f32, c.b() as f32);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
    if !(0.1..0.6).contains(&saturation) {
        return c;
    }
    // Each channel moves away from the brightest one until the colour is 60% saturated.
    let k = 0.6 / saturation;
    let ch = |v: f32| (max - (max - v) * k).clamp(0.0, 255.0).round() as u8;
    Color32::from_rgb(ch(r), ch(g), ch(b))
}

/// How much brighter the lighter of two colours is (WCAG's contrast ratio, 1 to 21).
#[cfg(test)]
fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The card's description format; Android refuses one it does not know.
pub const VERSION: u32 = 1;

/// Everything Android needs to draw a card.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct Card {
    pub v: u32,
    pub lines: Vec<String>,
    pub title: String,
    pub artist: String,
    /// The cover as a file Android can read; empty when there is none.
    pub cover: String,
    pub width: u32,
    pub height: u32,
    /// Colours as `#RRGGBB`.
    pub from: String,
    pub to: String,
    pub ink: String,
    pub sizes: Vec<u32>,
}

impl Card {
    /// The card for `lines` (at most four are kept) of the song `title` by `artist`.
    pub fn new(lines: &[String], title: &str, artist: &str, cover: Option<&Path>, colors: Colors, format: Format) -> Card {
        let hex = |c: Color32| format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
        let (width, height) = format.size();
        Card {
            v: VERSION,
            lines: lines.iter().map(|l| one_line(l)).filter(|l| !l.is_empty()).take(MAX_LINES).collect(),
            title: one_line(title),
            artist: one_line(artist),
            cover: cover.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            width,
            height,
            from: hex(colors.from),
            to: hex(colors.to),
            ink: hex(colors.ink),
            sizes: text_sizes(format),
        }
    }

    /// The description as Android reads it: JSON, in ASCII alone (anything else is
    /// written as `\uXXXX`), so no conversion on the way can change a letter of it.
    pub fn encode(&self) -> String {
        use std::fmt::Write;
        let json = serde_json::to_string(self).expect("a card is plain data");
        let mut out = String::with_capacity(json.len());
        for c in json.chars() {
            if c.is_ascii() {
                out.push(c);
            } else {
                // Only text holds anything else, and JSON text takes UTF-16 escapes.
                for unit in c.encode_utf16(&mut [0; 2]) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
        out
    }
}

/// The name a saved card gets in the phone's Pictures.
pub fn file_name(title: &str) -> String {
    let plain: String = title.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let short: String = one_line(&plain).chars().take(60).collect();
    let name = short.trim_matches([' ', '.']);
    if name.is_empty() { "BitChord lyrics.png".to_owned() } else { format!("BitChord lyrics - {name}.png") }
}

/// What `save-image` is asked: the picture's file and the name to save it under.
fn save_request(image: &Path, name: &str) -> String {
    format!("{}\u{1e}{name}", image.to_string_lossy())
}

/// Have Android draw `card`; answers the PNG it wrote, in the app's cache.
pub fn render(card: &Card) -> Result<PathBuf, String> {
    crate::platform::call("lyric-card", &card.encode(), Duration::from_secs(30)).map(PathBuf::from)
}

/// Copy the drawn picture into the phone's Pictures; answers where it went. Slow to
/// time out: older Androids ask for the storage permission first.
pub fn save(image: &Path, name: &str) -> Result<String, String> {
    crate::platform::call("save-image", &save_request(image, name), Duration::from_secs(90))
}

/// Hand the drawn picture to Android's share sheet.
pub fn share(image: &Path) -> Result<(), String> {
    crate::platform::call("share-image", &image.to_string_lossy(), Duration::from_secs(15)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::{Provider, Sync};

    fn line(text: &str) -> Line {
        Line { text: text.to_owned(), ..Line::default() }
    }

    fn lyrics(lines: Vec<Line>) -> Lyrics {
        Lyrics { lines, sync: Sync::Line, provider: Provider::LrclibSynced, language: None, songwriters: vec![] }
    }

    fn white() -> Color32 {
        Color32::WHITE
    }

    #[test]
    fn tapping_a_line_chooses_it_and_tapping_it_again_takes_it_back() {
        let mut s = Selection::default();
        assert_eq!(s.tap(3), Tap::Added);
        assert!(s.contains(3));
        assert_eq!(s.tap(3), Tap::Removed);
        assert!(s.is_empty());
    }

    #[test]
    fn the_chosen_lines_stay_in_the_songs_order_whatever_order_they_were_tapped_in() {
        let mut s = Selection::default();
        for l in [9, 2, 14, 5] {
            s.tap(l);
        }
        assert_eq!(s.lines(), [2, 5, 9, 14], "they need not follow one another");
    }

    #[test]
    fn a_fifth_line_is_not_taken_until_one_is_given_up() {
        let mut s = Selection::default();
        for l in [1, 2, 3, 4] {
            assert_eq!(s.tap(l), Tap::Added);
        }
        assert_eq!(s.tap(7), Tap::Full);
        assert_eq!(s.lines(), [1, 2, 3, 4]);
        assert_eq!(s.tap(2), Tap::Removed, "a chosen one can still be taken back");
        assert_eq!(s.tap(7), Tap::Added);
        assert_eq!(s.lines(), [1, 3, 4, 7]);
    }

    #[test]
    fn another_song_starts_with_nothing_chosen() {
        let mut s = Selection::default();
        assert!(!s.follow("song-a/lrclib"), "nothing to drop yet");
        s.tap(4);
        s.tap(6);
        assert!(!s.follow("song-a/lrclib"), "the same song keeps its choice");
        assert_eq!(s.lines(), [4, 6]);
        assert!(s.follow("song-b/lrclib"));
        assert!(s.is_empty());
        // The same song from another source has other lines too.
        s.tap(1);
        assert!(s.follow("song-b/musixmatch"));
        assert!(s.is_empty());
    }

    #[test]
    fn clearing_gives_all_four_places_back() {
        let mut s = Selection::default();
        for l in [1, 2, 3, 4] {
            s.tap(l);
        }
        s.clear();
        assert_eq!(s.tap(9), Tap::Added);
    }

    #[test]
    fn a_break_or_a_blank_line_cannot_be_chosen() {
        assert!(pickable(&line("Biar hujan turun perlahan")));
        assert!(!pickable(&line("   ")));
        assert!(!pickable(&Line { instrumental: true, ..line("♪") }));
    }

    #[test]
    fn the_card_says_the_chosen_lines_in_order() {
        let l = lyrics(vec![line("one"), line("two"), line("three"), line("four")]);
        assert_eq!(card_lines(&l, &[1, 3]), ["two", "four"]);
    }

    #[test]
    fn the_card_carries_the_line_as_sung_with_romanization_and_translation_showing() {
        // The lyrics view puts both under the line; its main text stays the original.
        let sung = Line { romanization: Some("tabun".into()), translations: vec![("en".into(), "probably".into())], ..line("たぶん") };
        let l = lyrics(vec![sung]);
        assert_eq!(card_lines(&l, &[0]), ["たぶん"]);
    }

    #[test]
    fn a_line_that_is_gone_or_not_sung_is_left_off_the_card() {
        let l = lyrics(vec![line("one"), Line { instrumental: true, ..line("") }, line("  ")]);
        assert_eq!(card_lines(&l, &[0, 1, 2, 40]), ["one"]);
    }

    #[test]
    fn a_card_never_says_more_than_four_lines() {
        let l = lyrics((0..8).map(|i| line(&format!("line {i}"))).collect());
        assert_eq!(card_lines(&l, &[0, 1, 2, 3, 4, 5]).len(), MAX_LINES);
        let card = Card::new(&(0..6).map(|i| format!("line {i}")).collect::<Vec<_>>(), "t", "a", None, Style::Dark.colors(white()), Format::Square);
        assert_eq!(card.lines.len(), MAX_LINES);
    }

    #[test]
    fn text_on_the_card_is_one_line_each() {
        assert_eq!(one_line("  Aku pulang\nlewat   jalan\tyang sama \r\n"), "Aku pulang lewat jalan yang sama");
        assert_eq!(one_line("a\u{1f}b\u{0}c"), "abc", "control characters go");
        assert_eq!(one_line("שלום \u{200f}עולם"), "שלום \u{200f}עולם", "direction marks stay");
    }

    #[test]
    fn every_format_is_1080_on_its_short_side() {
        assert_eq!(Format::Story.size(), (1080, 1920));
        assert_eq!(Format::Square.size(), (1080, 1080));
        assert_eq!(Format::Wide.size(), (1920, 1080));
        for f in Format::ALL {
            let (w, h) = f.size();
            assert_eq!(w.min(h), 1080);
        }
    }

    #[test]
    fn the_lines_are_tried_large_first_and_step_down_to_a_size_still_read() {
        for f in Format::ALL {
            let sizes = text_sizes(f);
            assert!(sizes.len() >= 5, "enough steps for four long lines");
            assert!(sizes[0] >= 84, "large and bold to begin with: {sizes:?}");
            assert!(sizes.windows(2).all(|w| w[1] < w[0]), "each step is smaller: {sizes:?}");
            // No step so big a jump that the text shrinks more than it must.
            assert!(sizes.windows(2).all(|w| w[1] * 100 >= w[0] * 80), "{sizes:?}");
            assert!(*sizes.last().unwrap() >= 36 && *sizes.last().unwrap() <= 48, "{sizes:?}");
        }
    }

    #[test]
    fn there_are_four_gradients_and_a_dark_one() {
        assert_eq!(Style::ALL.len(), 5);
        let all: Vec<Colors> = Style::ALL.iter().map(|s| s.colors(Color32::from_rgb(230, 90, 60))).collect();
        for (i, a) in all.iter().enumerate() {
            assert_ne!(a.from, a.to, "a gradient, not a flat colour");
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        let dark = Style::Dark.colors(white());
        assert!(contrast(dark.from, Color32::BLACK) < 2.0 && contrast(dark.to, Color32::BLACK) < 2.0, "{dark:?}");
    }

    #[test]
    fn the_first_style_takes_its_colours_from_the_cover() {
        let red = Style::Cover.colors(Color32::from_rgb(220, 60, 50));
        assert!(red.from.r() > red.from.g() && red.from.r() > red.from.b(), "{red:?}");
        let blue = Style::Cover.colors(Color32::from_rgb(60, 90, 220));
        assert!(blue.from.b() > blue.from.r() && blue.from.b() > blue.from.g(), "{blue:?}");
        // The fixed ones don't care what the cover is.
        assert_eq!(Style::Teal.colors(Color32::from_rgb(220, 60, 50)), Style::Teal.colors(Color32::from_rgb(60, 90, 220)));
    }

    #[test]
    fn the_text_reads_on_every_style_whatever_the_cover() {
        let covers = [white(), Color32::from_rgb(255, 250, 200), Color32::from_rgb(220, 220, 60), Color32::from_rgb(255, 181, 160), Color32::from_rgb(20, 20, 24), Color32::from_rgb(220, 60, 50)];
        for cover in covers {
            for style in Style::ALL {
                let c = style.colors(cover);
                // Large bold text needs three to one; the lightest corner is where it starts.
                assert!(contrast(c.ink, c.from) >= 3.0, "{style:?} on {cover:?}: {c:?}");
                assert!(contrast(c.ink, c.to) >= contrast(c.ink, c.from), "it deepens towards the far corner: {c:?}");
            }
        }
    }

    #[test]
    fn contrast_is_one_for_the_same_colour_and_twenty_one_for_black_on_white() {
        assert!((contrast(white(), white()) - 1.0).abs() < 0.001);
        assert!((contrast(white(), Color32::BLACK) - 21.0).abs() < 0.01);
        assert!((contrast(Color32::BLACK, white()) - 21.0).abs() < 0.01);
    }

    fn sample() -> Card {
        let lines = ["Biar hujan turun perlahan".to_owned(), "Aku pulang lewat jalan yang sama".to_owned()];
        Card::new(&lines, "Hujan di Kemang", "Senja Kala", Some(Path::new("/data/cache/lyric-card/cover")), Style::Teal.colors(white()), Format::Story)
    }

    #[test]
    fn a_card_says_what_to_draw_and_how_big() {
        let card = sample();
        assert_eq!(card.v, VERSION);
        assert_eq!(card.lines, ["Biar hujan turun perlahan", "Aku pulang lewat jalan yang sama"]);
        assert_eq!((card.title.as_str(), card.artist.as_str()), ("Hujan di Kemang", "Senja Kala"));
        assert_eq!(card.cover, "/data/cache/lyric-card/cover");
        assert_eq!((card.width, card.height), (1080, 1920));
        assert_eq!(card.sizes, text_sizes(Format::Story));
        let teal = Style::Teal.colors(white());
        let hex = |c: Color32| format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
        assert_eq!((card.from, card.to, card.ink), (hex(teal.from), hex(teal.to), hex(teal.ink)));
    }

    #[test]
    fn a_card_without_a_cover_says_so_with_an_empty_path() {
        let card = Card::new(&["la".to_owned()], "t", "a", None, Style::Dark.colors(white()), Format::Square);
        assert_eq!(card.cover, "");
    }

    #[test]
    fn the_songs_name_on_the_card_is_one_line_too() {
        let card = Card::new(&["la\nla".to_owned()], "Title\n(Live)", " Artist\u{1f} ", None, Style::Dark.colors(white()), Format::Square);
        assert_eq!((card.lines[0].as_str(), card.title.as_str(), card.artist.as_str()), ("la la", "Title (Live)", "Artist"));
    }

    #[test]
    fn the_description_is_versioned_json_that_reads_back_the_same() {
        let card = sample();
        let text = card.encode();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["v"], 1);
        assert_eq!(json["width"], 1080);
        assert_eq!(json["lines"][1], "Aku pulang lewat jalan yang sama");
        assert_eq!(serde_json::from_str::<Card>(&text).unwrap(), card);
    }

    #[test]
    fn awkward_text_survives_the_description() {
        let awkward = [
            "She said \"don't\" \\ and 'left'",
            "tabs\tand\nnew lines\r\nkept as written",
            "emoji 🎶🫶🏽 and flags 🇮🇩",
            "مرحبا بالعالم (hello) 123",
            "שלום \u{200f}עולם",
            "夜に駆ける — たぶん",
            "ไม่เป็นไร",
            "separators \u{1e} \u{1f} and a null \u{0}",
            "</script> {\"v\":2} \u{2028}",
        ];
        for text in awkward {
            // Straight into the fields: the description must hold whatever it is given.
            let card = Card { lines: vec![text.to_owned(); 2], title: text.to_owned(), artist: text.to_owned(), ..sample() };
            let encoded = card.encode();
            assert!(encoded.is_ascii(), "{encoded}");
            assert!(!encoded.contains(['\n', '\r', '\u{1e}', '\u{1f}']), "one line, clear of the bridge's separators: {encoded}");
            assert_eq!(serde_json::from_str::<Card>(&encoded).unwrap(), card, "{text}");
        }
    }

    #[test]
    fn letters_beyond_the_basic_plane_are_written_as_surrogate_pairs() {
        let card = Card { title: "🎶".to_owned(), ..sample() };
        assert!(card.encode().contains("\\ud83c\\udfb6"), "{}", card.encode());
    }

    #[test]
    fn a_saved_card_is_named_after_the_song() {
        assert_eq!(file_name("Hujan di Kemang"), "BitChord lyrics - Hujan di Kemang.png");
        assert_eq!(file_name("夜に駆ける"), "BitChord lyrics - 夜に駆ける.png");
    }

    #[test]
    fn a_file_name_has_nothing_a_file_system_refuses() {
        assert_eq!(file_name("AC/DC: \"Back\\in*Black?\" <live>|\n\u{1e}"), "BitChord lyrics - AC DC Back in Black live.png");
        assert_eq!(file_name("  ...  "), "BitChord lyrics.png", "a name of dots alone is none");
        assert_eq!(file_name(""), "BitChord lyrics.png");
        let long = file_name(&"あ".repeat(300));
        assert!(long.chars().count() <= 90 && long.ends_with(".png"), "{long}");
    }

    #[test]
    fn saving_names_the_picture_and_what_to_call_it() {
        assert_eq!(save_request(Path::new("/cache/share/lyrics-1f.png"), "BitChord lyrics - Tabun.png"), "/cache/share/lyrics-1f.png\u{1e}BitChord lyrics - Tabun.png");
    }
}
