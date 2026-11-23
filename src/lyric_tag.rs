//! Lyrics as text for a downloaded file's tag. A song with timed lyrics gets them as LRC
//! (`[01:23.45]a line`), which players that show synced lyrics read from the tag; a song
//! with plain lyrics gets the plain text.

use crate::lyrics::{Line, Lyrics, LyricsSet, Sync};

/// The lyrics to embed: the first source (they are in priority order) that is timed, else
/// the first one that has any text.
pub fn choose(set: &LyricsSet) -> Option<&Lyrics> {
    set.sources
        .iter()
        .find(|l| l.sync != Sync::None && has_text(&l.lines))
        .or_else(|| set.sources.iter().find(|l| has_text(&l.lines)))
}

/// The text for the tag: LRC for timed lyrics, plain text otherwise. `None` when there is
/// nothing to write.
pub fn text(lyrics: &Lyrics) -> Option<String> {
    let out = if lyrics.sync == Sync::None { plain(&lyrics.lines) } else { lrc(&lyrics.lines) };
    (!out.trim().is_empty()).then_some(out)
}

fn has_text(lines: &[Line]) -> bool {
    lines.iter().any(|l| !l.text.trim().is_empty())
}

/// One line per lyric line. Word-timed lyrics keep their line's own text and start.
fn lrc(lines: &[Line]) -> String {
    lines
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| format!("[{}]{}", timestamp(l.start), l.text.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn plain(lines: &[Line]) -> String {
    lines.iter().filter(|l| !l.text.trim().is_empty()).map(|l| l.text.trim()).collect::<Vec<_>>().join("\n")
}

/// `mm:ss.xx` for a time in milliseconds (minutes may pass 99; a time before zero is zero).
fn timestamp(ms: i32) -> String {
    let cs = ms.max(0) / 10;
    format!("{:02}:{:02}.{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::Provider;

    fn line(start: i32, text: &str) -> Line {
        Line { start, dur: 2000, text: text.into(), ..Default::default() }
    }

    fn set(sources: Vec<(Provider, Sync, Vec<Line>)>) -> LyricsSet {
        LyricsSet { sources: sources.into_iter().map(|(provider, sync, lines)| Lyrics { lines, sync, provider, language: None, songwriters: Vec::new() }).collect() }
    }

    #[test]
    fn timed_lyrics_become_lrc() {
        let l = Lyrics { lines: vec![line(0, "First"), line(83_450, "Second"), line(3_723_400, "Late")], sync: Sync::Line, provider: Provider::LrclibSynced, language: None, songwriters: vec![] };
        assert_eq!(text(&l).unwrap(), "[00:00.00]First\n[01:23.45]Second\n[62:03.40]Late");
    }

    #[test]
    fn plain_lyrics_stay_plain() {
        let l = Lyrics { lines: vec![line(0, "One"), line(0, "Two")], sync: Sync::None, provider: Provider::LrclibPlain, language: None, songwriters: vec![] };
        assert_eq!(text(&l).unwrap(), "One\nTwo");
    }

    #[test]
    fn word_timed_lyrics_are_written_a_line_at_a_time() {
        let mut first = line(1_000, "Hello world");
        first.parts = vec![crate::lyrics::Part { start: 1_000, dur: 400, text: "Hello ".into(), bg: false }, crate::lyrics::Part { start: 1_400, dur: 400, text: "world".into(), bg: false }];
        let l = Lyrics { lines: vec![first], sync: Sync::Word, provider: Provider::BLyricsRich, language: None, songwriters: vec![] };
        assert_eq!(text(&l).unwrap(), "[00:01.00]Hello world");
    }

    #[test]
    fn instrumental_gaps_and_blank_lines_are_left_out() {
        let mut gap = line(5_000, "");
        gap.instrumental = true;
        let l = Lyrics { lines: vec![line(0, "A"), gap, line(9_000, "   "), line(12_000, "B")], sync: Sync::Line, provider: Provider::LrclibSynced, language: None, songwriters: vec![] };
        assert_eq!(text(&l).unwrap(), "[00:00.00]A\n[00:12.00]B");
    }

    #[test]
    fn lyrics_with_no_text_give_nothing() {
        let l = Lyrics { lines: vec![line(0, ""), line(1000, "  ")], sync: Sync::Line, provider: Provider::LrclibSynced, language: None, songwriters: vec![] };
        assert_eq!(text(&l), None);
    }

    #[test]
    fn a_negative_time_is_zero_and_centiseconds_are_not_rounded_up() {
        assert_eq!(timestamp(-50), "00:00.00");
        assert_eq!(timestamp(1_999), "00:01.99");
        assert_eq!(timestamp(59_999), "00:59.99");
        assert_eq!(timestamp(60_000), "01:00.00");
    }

    #[test]
    fn timed_lyrics_win_over_plain_ones_whatever_their_order() {
        let s = set(vec![(Provider::LrclibPlain, Sync::None, vec![line(0, "plain")]), (Provider::LrclibSynced, Sync::Line, vec![line(1000, "timed")])]);
        assert_eq!(choose(&s).unwrap().provider, Provider::LrclibSynced);
    }

    #[test]
    fn among_timed_lyrics_the_first_in_priority_order_is_taken() {
        let s = set(vec![(Provider::BLyricsRich, Sync::Word, vec![line(0, "a")]), (Provider::LrclibSynced, Sync::Line, vec![line(0, "b")])]);
        assert_eq!(choose(&s).unwrap().provider, Provider::BLyricsRich);
    }

    #[test]
    fn a_source_with_no_text_is_skipped() {
        let s = set(vec![(Provider::BLyricsRich, Sync::Word, vec![line(0, "")]), (Provider::LrclibPlain, Sync::None, vec![line(0, "words")])]);
        assert_eq!(choose(&s).unwrap().provider, Provider::LrclibPlain);
        assert!(choose(&LyricsSet::default()).is_none());
    }
}
