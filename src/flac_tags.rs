//! Tags for a FLAC file: Vorbis comments (title, artists, album, track number) and a front
//! cover, written into the file's metadata blocks. The audio frames after the metadata are
//! copied byte for byte, so the sound cannot change; only the blocks in front of it do.
//!
//! A FLAC file is `fLaC`, then metadata blocks (a 4-byte header: the top bit marks the last
//! block, 7 bits the type, 24 bits the length), then the audio. STREAMINFO is always the
//! first block.

use anyhow::{bail, ensure};

const STREAMINFO: u8 = 0;
const PADDING: u8 = 1;
const VORBIS_COMMENT: u8 = 4;
const PICTURE: u8 = 6;
/// A block's length is 24 bits.
const MAX_BLOCK: usize = 0xFF_FFFF;
const VENDOR: &str = "Aoto";

pub struct Tags<'a> {
    pub title: &'a str,
    pub artists: &'a [String],
    pub album: Option<&'a str>,
    pub track_nr: Option<u16>,
    /// A JPEG or PNG; anything else is left out.
    pub cover: Option<&'a [u8]>,
    /// Plain text, or LRC for timed lyrics (see `lyric_tag`).
    pub lyrics: Option<&'a str>,
}

/// The file with `tags` written into it.
pub fn write(data: &[u8], tags: &Tags) -> anyhow::Result<Vec<u8>> {
    ensure!(data.get(..4) == Some(b"fLaC"), "not a FLAC file");
    let (blocks, audio_at) = read_blocks(data)?;
    ensure!(blocks.first().is_some_and(|(kind, body)| *kind == STREAMINFO && body.len() >= 34), "the file has no STREAMINFO");

    let picture = tags.cover.and_then(picture_block);
    let mut out: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut old_comments = None;
    for (kind, body) in blocks {
        match kind {
            VORBIS_COMMENT => old_comments = Some(body),
            PADDING => {}
            // Only a new cover replaces the old one.
            PICTURE if picture.is_some() => {}
            _ => out.push((kind, body)),
        }
    }
    out.push((VORBIS_COMMENT, comment_block(old_comments.as_deref(), tags)));
    if let Some(p) = picture {
        out.push((PICTURE, p));
    }

    let mut file = Vec::with_capacity(data.len() + 4096);
    file.extend_from_slice(b"fLaC");
    let last = out.len() - 1;
    for (i, (kind, body)) in out.iter().enumerate() {
        ensure!(body.len() <= MAX_BLOCK, "a metadata block is too long");
        file.push(kind | if i == last { 0x80 } else { 0 });
        file.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        file.extend_from_slice(body);
    }
    file.extend_from_slice(&data[audio_at..]);
    Ok(file)
}

/// The metadata blocks (type, body) and where the audio starts.
fn read_blocks(data: &[u8]) -> anyhow::Result<(Vec<(u8, Vec<u8>)>, usize)> {
    let mut at = 4;
    let mut blocks = Vec::new();
    loop {
        let Some(header) = data.get(at..at + 4) else { bail!("the file ends inside its metadata") };
        let last = header[0] & 0x80 != 0;
        let kind = header[0] & 0x7F;
        let len = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
        let Some(body) = data.get(at + 4..at + 4 + len) else { bail!("a metadata block runs past the end of the file") };
        blocks.push((kind, body.to_vec()));
        at += 4 + len;
        if last {
            return Ok((blocks, at));
        }
    }
}

/// The Vorbis comments: the entries already there that are not being replaced, then ours.
fn comment_block(old: Option<&[u8]>, tags: &Tags) -> Vec<u8> {
    let (vendor, kept) = match old.and_then(parse_comments) {
        Some((vendor, entries)) => (vendor, entries),
        None => (VENDOR.to_owned(), Vec::new()),
    };
    const REPLACED: [&str; 5] = ["TITLE", "ARTIST", "ALBUMARTIST", "ALBUM", "TRACKNUMBER"];
    // Old lyrics are kept unless there are new ones.
    const LYRICS: [&str; 2] = ["LYRICS", "UNSYNCEDLYRICS"];
    let mut entries: Vec<String> = kept
        .into_iter()
        .filter(|e| {
            e.split_once('=').is_none_or(|(k, _)| {
                let k = k.to_ascii_uppercase();
                !REPLACED.contains(&k.as_str()) && !(tags.lyrics.is_some() && LYRICS.contains(&k.as_str()))
            })
        })
        .collect();
    entries.push(format!("TITLE={}", tags.title));
    for artist in tags.artists {
        entries.push(format!("ARTIST={artist}"));
    }
    if let Some(first) = tags.artists.first() {
        entries.push(format!("ALBUMARTIST={first}"));
    }
    if let Some(album) = tags.album {
        entries.push(format!("ALBUM={album}"));
    }
    if let Some(n) = tags.track_nr {
        entries.push(format!("TRACKNUMBER={n}"));
    }
    if let Some(text) = tags.lyrics {
        entries.push(format!("LYRICS={text}"));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor.as_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in &entries {
        out.extend_from_slice(&(e.len() as u32).to_le_bytes());
        out.extend_from_slice(e.as_bytes());
    }
    out
}

/// A VORBIS_COMMENT body as (vendor, "KEY=value" entries).
pub fn parse_comments(body: &[u8]) -> Option<(String, Vec<String>)> {
    let read_u32 = |at: usize| body.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let vendor_len = read_u32(0)?;
    let vendor = String::from_utf8_lossy(body.get(4..4 + vendor_len)?).into_owned();
    let mut at = 4 + vendor_len;
    let count = read_u32(at)?;
    at += 4;
    let mut entries = Vec::new();
    for _ in 0..count {
        let len = read_u32(at)?;
        at += 4;
        entries.push(String::from_utf8_lossy(body.get(at..at + len)?).into_owned());
        at += len;
    }
    Some((vendor, entries))
}

/// A PICTURE block for a front cover; `None` for anything but a JPEG or a PNG.
fn picture_block(cover: &[u8]) -> Option<Vec<u8>> {
    let (mime, width, height) = if cover.starts_with(&[0xFF, 0xD8]) {
        let (w, h) = jpeg_size(cover).unwrap_or((0, 0));
        ("image/jpeg", w, h)
    } else if cover.starts_with(&[0x89, b'P', b'N', b'G']) {
        let w = u32::from_be_bytes(cover.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(cover.get(20..24)?.try_into().ok()?);
        ("image/png", w, h)
    } else {
        return None;
    };
    let mut out = Vec::with_capacity(cover.len() + 64);
    out.extend_from_slice(&3u32.to_be_bytes()); // the front cover
    out.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    out.extend_from_slice(mime.as_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // no description
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&24u32.to_be_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_be_bytes()); // not indexed
    out.extend_from_slice(&(cover.len() as u32).to_be_bytes());
    out.extend_from_slice(cover);
    (out.len() <= MAX_BLOCK).then_some(out)
}

/// A JPEG's size, from its first start-of-frame marker.
fn jpeg_size(jpeg: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 4 <= jpeg.len() {
        if jpeg[at] != 0xFF {
            return None;
        }
        let marker = jpeg[at + 1];
        if marker == 0xFF {
            at += 1;
            continue;
        }
        let len = u16::from_be_bytes([jpeg[at + 2], *jpeg.get(at + 3)?]) as usize;
        // SOF0-SOF15 but not DHT (C4), JPG (C8) or DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([*jpeg.get(at + 5)?, *jpeg.get(at + 6)?]) as u32;
            let w = u16::from_be_bytes([*jpeg.get(at + 7)?, *jpeg.get(at + 8)?]) as u32;
            return Some((w, h));
        }
        at += 2 + len;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn crc(data: &[u8], poly: u32, bits: u32) -> u32 {
        let top = (1u32 << bits) - 1;
        let mut c = 0u32;
        for &byte in data {
            c ^= (byte as u32) << (bits - 8);
            for _ in 0..8 {
                c = if c & (1 << (bits - 1)) != 0 { ((c << 1) ^ poly) & top } else { (c << 1) & top };
            }
        }
        c
    }

    /// A real, playable FLAC: one channel of verbatim 16-bit frames (a rising ramp), with
    /// `comments` as its Vorbis comments and `extra` blocks (type, body) after them.
    pub(crate) fn flac(samples: usize, comments: &[&str], extra: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let block = 4096usize;
        let mut info = Vec::new();
        info.extend_from_slice(&(block as u16).to_be_bytes());
        info.extend_from_slice(&(block as u16).to_be_bytes());
        info.extend_from_slice(&[0; 6]);
        let packed: u64 = (44_100u64 << 44) | (15u64 << 36) | samples as u64; // 44.1 kHz, 1 channel, 16 bits
        info.extend_from_slice(&packed.to_be_bytes());
        info.extend_from_slice(&[0; 16]);
        let mut blocks: Vec<(u8, Vec<u8>)> = vec![(STREAMINFO, info)];
        if !comments.is_empty() {
            let mut body = Vec::new();
            body.extend_from_slice(&5u32.to_le_bytes());
            body.extend_from_slice(b"tests");
            body.extend_from_slice(&(comments.len() as u32).to_le_bytes());
            for c in comments {
                body.extend_from_slice(&(c.len() as u32).to_le_bytes());
                body.extend_from_slice(c.as_bytes());
            }
            blocks.push((VORBIS_COMMENT, body));
        }
        blocks.extend(extra.iter().cloned());
        let mut out = b"fLaC".to_vec();
        for (i, (kind, body)) in blocks.iter().enumerate() {
            out.push(kind | if i == blocks.len() - 1 { 0x80 } else { 0 });
            out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
            out.extend_from_slice(body);
        }
        let (mut done, mut number) = (0usize, 0u8);
        while done < samples {
            let n = (samples - done).min(block);
            let mut frame = vec![0xFF, 0xF8, if n == block { 0xC0 } else { 0x70 }, 0x00, number];
            if n != block {
                frame.extend_from_slice(&((n - 1) as u16).to_be_bytes());
            }
            frame.push(crc(&frame, 0x07, 8) as u8);
            frame.push(0x02); // verbatim subframe
            for i in done..done + n {
                frame.extend_from_slice(&(((i % 2000) as i16 - 1000) * 8).to_be_bytes());
            }
            let c = crc(&frame, 0x8005, 16) as u16;
            frame.extend_from_slice(&c.to_be_bytes());
            out.extend_from_slice(&frame);
            done += n;
            number += 1;
        }
        out
    }

    fn tags<'a>(title: &'a str, artists: &'a [String]) -> Tags<'a> {
        Tags { title, artists, album: None, track_nr: None, cover: None, lyrics: None }
    }

    fn comments_of(file: &[u8]) -> Vec<String> {
        let (blocks, _) = read_blocks(file).unwrap();
        let body = &blocks.iter().find(|(k, _)| *k == VORBIS_COMMENT).expect("a comment block").1;
        parse_comments(body).unwrap().1
    }

    fn audio_of(file: &[u8]) -> &[u8] {
        let (_, at) = read_blocks(file).unwrap();
        &file[at..]
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]; // SOI, a short APP0
        j.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        j.extend_from_slice(&h.to_be_bytes());
        j.extend_from_slice(&w.to_be_bytes());
        j.extend_from_slice(&[0x01, 0x01, 0x11, 0x00, 0xFF, 0xD9]);
        j
    }

    #[test]
    fn the_tags_are_written_and_the_audio_is_not_touched() {
        let file = flac(9000, &[], &[]);
        let artists = vec!["Senja Kala".to_owned()];
        let out = write(&file, &Tags { title: "Hujan di Kemang", artists: &artists, album: Some("Musim"), track_nr: Some(3), cover: None, lyrics: None }).unwrap();
        assert_eq!(audio_of(&out), audio_of(&file), "the audio frames are copied byte for byte");
        let c = comments_of(&out);
        for want in ["TITLE=Hujan di Kemang", "ARTIST=Senja Kala", "ALBUMARTIST=Senja Kala", "ALBUM=Musim", "TRACKNUMBER=3"] {
            assert!(c.contains(&want.to_owned()), "{want} in {c:?}");
        }
    }

    #[test]
    fn streaminfo_stays_first_and_unchanged() {
        let file = flac(9000, &[], &[]);
        let out = write(&file, &tags("T", &[])).unwrap();
        let (before, _) = read_blocks(&file).unwrap();
        let (after, _) = read_blocks(&out).unwrap();
        assert_eq!(before[0], after[0]);
    }

    #[test]
    fn old_title_and_artist_are_replaced_and_other_comments_are_kept() {
        let file = flac(5000, &["TITLE=Old", "artist=Someone Else", "GENRE=Pop", "COMMENT=ripped by me"], &[]);
        let artists = vec!["New Artist".to_owned()];
        let out = write(&file, &tags("New Title", &artists)).unwrap();
        let c = comments_of(&out);
        assert_eq!(c.iter().filter(|e| e.to_ascii_uppercase().starts_with("TITLE=")).count(), 1);
        assert_eq!(c.iter().filter(|e| e.to_ascii_uppercase().starts_with("ARTIST=")).count(), 1);
        assert!(c.contains(&"TITLE=New Title".to_owned()) && c.contains(&"ARTIST=New Artist".to_owned()));
        assert!(c.contains(&"GENRE=Pop".to_owned()) && c.contains(&"COMMENT=ripped by me".to_owned()), "{c:?}");
    }

    #[test]
    fn every_artist_gets_an_entry_and_the_first_is_the_album_artist() {
        let artists = vec!["A".to_owned(), "B".to_owned()];
        let c = comments_of(&write(&flac(100, &[], &[]), &tags("T", &artists)).unwrap());
        assert_eq!(c.iter().filter(|e| e.starts_with("ARTIST=")).collect::<Vec<_>>(), ["ARTIST=A", "ARTIST=B"]);
        assert!(c.contains(&"ALBUMARTIST=A".to_owned()));
    }

    #[test]
    fn a_title_with_unicode_survives() {
        let artists = vec!["藍音".to_owned()];
        let c = comments_of(&write(&flac(100, &[], &[]), &tags("好きだから。", &artists)).unwrap());
        assert!(c.contains(&"TITLE=好きだから。".to_owned()) && c.contains(&"ARTIST=藍音".to_owned()));
    }

    #[test]
    fn exactly_the_last_block_carries_the_last_flag() {
        let out = write(&flac(100, &["GENRE=x"], &[(PADDING, vec![0; 64])]), &Tags { cover: Some(&jpeg(10, 10)), ..tags("T", &[]) }).unwrap();
        let (mut at, mut flags) = (4usize, Vec::new());
        loop {
            let last = out[at] & 0x80 != 0;
            flags.push(last);
            let len = u32::from_be_bytes([0, out[at + 1], out[at + 2], out[at + 3]]) as usize;
            at += 4 + len;
            if last {
                break;
            }
        }
        assert_eq!(flags.iter().filter(|f| **f).count(), 1);
        assert!(*flags.last().unwrap());
        assert_eq!(at, out.len() - audio_of(&out).len());
    }

    #[test]
    fn a_jpeg_cover_becomes_a_front_cover_with_its_size() {
        let cover = jpeg(300, 200);
        let out = write(&flac(100, &[], &[]), &Tags { cover: Some(&cover), ..tags("T", &[]) }).unwrap();
        let (blocks, _) = read_blocks(&out).unwrap();
        let pic = &blocks.iter().find(|(k, _)| *k == PICTURE).expect("a picture block").1;
        assert_eq!(&pic[0..4], &3u32.to_be_bytes(), "front cover");
        assert_eq!(&pic[8..18], b"image/jpeg");
        let after_mime = 8 + 10 + 4; // type, mime length, mime, description length
        assert_eq!(&pic[after_mime..after_mime + 4], &300u32.to_be_bytes());
        assert_eq!(&pic[after_mime + 4..after_mime + 8], &200u32.to_be_bytes());
        assert!(pic.ends_with(&cover));
    }

    #[test]
    fn a_png_cover_is_accepted_and_something_else_is_left_out() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&48u32.to_be_bytes());
        png.extend_from_slice(&[8, 2, 0, 0, 0]);
        let out = write(&flac(100, &[], &[]), &Tags { cover: Some(&png), ..tags("T", &[]) }).unwrap();
        assert!(read_blocks(&out).unwrap().0.iter().any(|(k, _)| *k == PICTURE));
        let webp = b"RIFF....WEBPVP8 ".to_vec();
        let out = write(&flac(100, &[], &[]), &Tags { cover: Some(&webp), ..tags("T", &[]) }).unwrap();
        assert!(!read_blocks(&out).unwrap().0.iter().any(|(k, _)| *k == PICTURE), "not an image Aoto can name");
    }

    #[test]
    fn an_old_cover_stays_unless_there_is_a_new_one() {
        let old = picture_block(&jpeg(5, 5)).unwrap();
        let file = flac(100, &[], &[(PICTURE, old.clone())]);
        let kept = write(&file, &tags("T", &[])).unwrap();
        assert_eq!(read_blocks(&kept).unwrap().0.iter().filter(|(k, _)| *k == PICTURE).count(), 1);
        let replaced = write(&file, &Tags { cover: Some(&jpeg(7, 7)), ..tags("T", &[]) }).unwrap();
        let pics: Vec<_> = read_blocks(&replaced).unwrap().0.into_iter().filter(|(k, _)| *k == PICTURE).collect();
        assert_eq!(pics.len(), 1);
        assert_ne!(pics[0].1, old);
    }

    #[test]
    fn other_blocks_like_a_seek_table_are_kept_and_padding_is_dropped() {
        let file = flac(100, &[], &[(3, vec![1; 18]), (PADDING, vec![0; 100])]);
        let (blocks, _) = read_blocks(&write(&file, &tags("T", &[])).unwrap()).unwrap();
        let kinds: Vec<u8> = blocks.iter().map(|(k, _)| *k).collect();
        assert!(kinds.contains(&3) && !kinds.contains(&PADDING), "{kinds:?}");
    }

    #[test]
    fn something_that_is_not_flac_is_refused() {
        assert!(write(b"ID3 not flac at all", &tags("T", &[])).is_err());
        assert!(write(b"", &tags("T", &[])).is_err());
        assert!(write(b"fLaC", &tags("T", &[])).is_err(), "no blocks");
    }

    #[test]
    fn a_file_cut_off_inside_its_metadata_is_refused() {
        let file = flac(100, &[], &[]);
        assert!(write(&file[..20], &tags("T", &[])).is_err());
    }

    #[test]
    fn a_file_whose_first_block_is_not_streaminfo_is_refused() {
        let mut file = b"fLaC".to_vec();
        file.extend_from_slice(&[0x80 | VORBIS_COMMENT, 0, 0, 8]);
        file.extend_from_slice(&[0; 8]);
        assert!(write(&file, &tags("T", &[])).is_err());
    }

    #[test]
    fn a_tagged_file_still_decodes_to_the_same_song() {
        use rodio::Source;
        let file = flac(9000, &[], &[]);
        let artists = vec!["Senja Kala".to_owned()];
        let tagged = write(&file, &Tags { title: "Hujan", artists: &artists, album: Some("Musim"), track_nr: Some(1), cover: Some(&jpeg(10, 10)), lyrics: Some("[00:01.00]Hujan") }).unwrap();
        let play = |bytes: Vec<u8>| -> (u32, Vec<f32>) {
            let format = crate::stream::Format::File { hint: "flac", mime: Some("audio/flac".to_owned()) };
            let shared = crate::stream::Shared::from_bytes_with_format(bytes, 0, None, format, None);
            let d = crate::stream::decoder(shared, true).expect("the FLAC decodes");
            (d.sample_rate().get(), d.collect())
        };
        let (rate_before, before) = play(file);
        let (rate_after, after) = play(tagged);
        assert_eq!((rate_before, rate_after), (44_100, 44_100));
        assert_eq!(before.len(), 9000);
        assert_eq!(before, after, "the same samples, tags or not");
    }

    #[test]
    fn lyrics_go_in_as_a_lyrics_entry_keeping_line_breaks_and_unicode() {
        let text = "[00:01.00]Hujan di Kemang\n[00:05.50]好きだから。";
        let out = write(&flac(100, &[], &[]), &Tags { lyrics: Some(text), ..tags("T", &[]) }).unwrap();
        assert!(comments_of(&out).contains(&format!("LYRICS={text}")));
    }

    #[test]
    fn new_lyrics_replace_old_ones_and_without_new_ones_the_old_stay() {
        let file = flac(100, &["LYRICS=old words", "UNSYNCEDLYRICS=old plain"], &[]);
        let replaced = comments_of(&write(&file, &Tags { lyrics: Some("new"), ..tags("T", &[]) }).unwrap());
        assert_eq!(replaced.iter().filter(|e| e.to_ascii_uppercase().contains("LYRICS=")).collect::<Vec<_>>(), ["LYRICS=new"]);
        let kept = comments_of(&write(&file, &tags("T", &[])).unwrap());
        assert!(kept.contains(&"LYRICS=old words".to_owned()));
    }
}
