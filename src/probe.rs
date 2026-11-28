//! What a file on the phone holds, read from its own bytes: a name's extension can say
//! anything, and the player must not call a FLAC file "AAC" because saved songs usually are.

/// What the head of an audio file says it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    /// `FLAC`, `ALAC`, `AAC`, `MP3`, `Opus`, `Vorbis`, `WAV`, `AIFF`.
    pub codec: &'static str,
    /// The container, when it is not the codec's own: `MP4`, `Ogg`.
    pub container: Option<&'static str>,
    /// Nothing was thrown away to make the file.
    pub lossless: bool,
    /// As the file states them, where its header is one this reads (FLAC, WAV).
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u8>,
}

impl Probe {
    fn of(codec: &'static str, container: Option<&'static str>, lossless: bool) -> Self {
        Self { codec, container, lossless, sample_rate: None, bit_depth: None, channels: None }
    }

    /// What the player says about a lossless file: where it is from and what it is.
    pub fn info(&self, source: &str, size: u64, duration_ms: u64) -> crate::hires::Info {
        crate::hires::Info {
            source: source.to_owned(),
            host: String::new(),
            codec: self.codec.to_owned(),
            quality: String::new(),
            bitrate_kbps: (duration_ms > 0).then(|| (size * 8 / duration_ms) as u32).filter(|k| *k > 0),
            sample_rate: self.sample_rate,
            bit_depth: self.bit_depth,
            channels: self.channels,
            atmos: false,
            size,
            duration_ms,
        }
    }
}

/// Where the audio starts: past an ID3v2 tag, which some taggers put before any format.
fn past_id3(data: &[u8]) -> &[u8] {
    if data.len() < 10 || &data[..3] != b"ID3" {
        return data;
    }
    // Four 7-bit bytes: the tag's length without its 10-byte header.
    let size = data[6..10].iter().fold(0usize, |n, b| (n << 7) | (*b & 0x7f) as usize);
    data.get(10 + size..).unwrap_or(&[])
}

fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len()).position(|w| w == needle)
}

/// FLAC's STREAMINFO: the first block after the marker.
fn flac(data: &[u8]) -> Probe {
    let mut p = Probe::of("FLAC", None, true);
    // Marker (4), block header (4), then 10 bytes of block and frame sizes.
    if let Some(b) = data.get(18..22).filter(|_| data.get(4).is_some_and(|t| t & 0x7f == 0)) {
        let rate = (b[0] as u32) << 12 | (b[1] as u32) << 4 | (b[2] as u32) >> 4;
        p.sample_rate = (rate > 0).then_some(rate);
        p.channels = Some(((b[2] >> 1) & 0x07) + 1);
        p.bit_depth = Some((((b[2] & 0x01) as u32) << 4 | (b[3] as u32) >> 4) + 1);
    }
    p
}

/// A WAV file's `fmt ` chunk.
fn wav(data: &[u8]) -> Probe {
    let mut p = Probe::of("WAV", None, true);
    let head = &data[..data.len().min(4096)];
    if let Some(f) = find(head, b"fmt ").and_then(|at| head.get(at + 8..at + 24)) {
        let le16 = |i: usize| u16::from_le_bytes([f[i], f[i + 1]]);
        p.channels = u8::try_from(le16(2)).ok().filter(|c| *c > 0);
        p.sample_rate = Some(u32::from_le_bytes([f[4], f[5], f[6], f[7]])).filter(|r| *r > 0);
        p.bit_depth = Some(le16(14) as u32).filter(|b| *b > 0);
    }
    p
}

/// An MP4's audio: the sample description names the codec. The `moov` box may stand at
/// either end of the file, so both ends are looked through.
fn mp4(data: &[u8]) -> Probe {
    const REACH: usize = 1 << 20;
    // After the box's name: version and flags (4), the entry count (4), the first
    // entry's size (4), then its codec.
    let codec_at = |part: &[u8]| find(part, b"stsd").and_then(|at| part.get(at + 16..at + 20)).map(|c| c.to_vec());
    let fourcc = codec_at(&data[..data.len().min(REACH)]).or_else(|| codec_at(&data[data.len().saturating_sub(REACH)..]));
    match fourcc.as_deref() {
        Some(b"alac") => Probe::of("ALAC", Some("MP4"), true),
        Some(b"fLaC") => Probe::of("FLAC", Some("MP4"), true),
        Some(b"Opus") => Probe::of("Opus", Some("MP4"), false),
        _ => Probe::of("AAC", Some("MP4"), false),
    }
}

/// What `data` (a whole file, or its head) is, or `None` for something this doesn't know:
/// the caller then says what it would have said anyway.
pub fn probe(data: &[u8]) -> Option<Probe> {
    let data = past_id3(data);
    let head = &data[..data.len().min(512)];
    if head.starts_with(b"fLaC") {
        return Some(flac(data));
    }
    if head.get(4..8) == Some(b"ftyp") {
        return Some(mp4(data));
    }
    if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
        return Some(wav(data));
    }
    if head.starts_with(b"FORM") && matches!(head.get(8..12), Some(b"AIFF" | b"AIFC")) {
        return Some(Probe::of("AIFF", None, true));
    }
    if head.starts_with(b"OggS") {
        return Some(if find(head, b"OpusHead").is_some() {
            Probe::of("Opus", Some("Ogg"), false)
        } else if find(head, b"\x7fFLAC").is_some() {
            Probe::of("FLAC", Some("Ogg"), true)
        } else {
            Probe::of("Vorbis", Some("Ogg"), false)
        });
    }
    if head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        // Matroska/WebM: YouTube's Opus.
        return Some(Probe::of("Opus", Some("WebM"), false));
    }
    // An MPEG audio frame: eleven sync bits, then a layer that is not "reserved".
    if head.len() >= 2 && head[0] == 0xff && head[1] & 0xe0 == 0xe0 && head[1] & 0x06 != 0 {
        return Some(Probe::of("MP3", None, false));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A FLAC head: the marker, a STREAMINFO block header and the block with these numbers.
    fn flac_head(rate: u32, channels: u8, bits: u32) -> Vec<u8> {
        let mut f = b"fLaC".to_vec();
        f.extend([0x00, 0x00, 0x00, 34]);
        f.extend([0u8; 10]);
        f.push((rate >> 12) as u8);
        f.push((rate >> 4) as u8);
        f.push(((rate & 0x0f) as u8) << 4 | (channels - 1) << 1 | ((bits - 1) >> 4) as u8);
        f.push((((bits - 1) & 0x0f) as u8) << 4);
        f.extend([0u8; 20]);
        f
    }

    #[test]
    fn a_flac_file_is_flac_with_the_numbers_its_header_states() {
        let p = probe(&flac_head(44_100, 2, 16)).unwrap();
        assert_eq!((p.codec, p.lossless), ("FLAC", true));
        assert_eq!((p.sample_rate, p.channels, p.bit_depth), (Some(44_100), Some(2), Some(16)));
        let hi = probe(&flac_head(96_000, 2, 24)).unwrap();
        assert_eq!((hi.sample_rate, hi.bit_depth), (Some(96_000), Some(24)));
        let mono = probe(&flac_head(48_000, 1, 16)).unwrap();
        assert_eq!(mono.channels, Some(1));
    }

    /// The generator the tag writer's tests use makes a real FLAC file: it reads the same.
    #[test]
    fn a_real_flac_file_reads_the_same() {
        let file = crate::flac_tags::tests::flac(4096, &[], &[]);
        let p = probe(&file).unwrap();
        assert_eq!(p.codec, "FLAC");
        assert!(p.sample_rate.is_some_and(|r| r >= 8_000), "{p:?}");
        assert!(p.bit_depth.is_some_and(|b| (8..=32).contains(&b)), "{p:?}");
    }

    #[test]
    fn a_flac_file_behind_an_id3_tag_is_still_flac() {
        let mut f = b"ID3\x04\x00\x00\x00\x00\x00\x05hello".to_vec();
        f.extend(flac_head(44_100, 2, 16));
        assert_eq!(probe(&f).unwrap().codec, "FLAC");
    }

    #[test]
    fn an_mp4_says_which_codec_it_carries() {
        let mp4 = |codec: &[u8; 4]| {
            let mut f = b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00".to_vec();
            f.extend(b"....moov....stsd\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x24");
            f.extend(codec);
            f.extend([0u8; 32]);
            f
        };
        assert_eq!(probe(&mp4(b"mp4a")).unwrap(), Probe::of("AAC", Some("MP4"), false));
        assert_eq!(probe(&mp4(b"alac")).unwrap(), Probe::of("ALAC", Some("MP4"), true));
        // No sample description in reach: said to be what saved songs are.
        assert_eq!(probe(b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00").unwrap().codec, "AAC");
    }

    #[test]
    fn the_sample_description_is_found_at_the_end_of_a_large_file_too() {
        let mut f = b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00".to_vec();
        f.extend(vec![0u8; 3 << 20]);
        f.extend(b"moov....stsd\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x24alac");
        assert_eq!(probe(&f).unwrap().codec, "ALAC");
    }

    #[test]
    fn a_wav_file_states_its_format() {
        let mut f = b"RIFF\x00\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x02\x00".to_vec();
        f.extend(48_000u32.to_le_bytes());
        f.extend([0u8; 6]);
        f.extend(24u16.to_le_bytes());
        let p = probe(&f).unwrap();
        assert_eq!((p.codec, p.lossless, p.channels, p.sample_rate, p.bit_depth), ("WAV", true, Some(2), Some(48_000), Some(24)));
    }

    #[test]
    fn the_lossy_formats_are_named_and_not_called_lossless() {
        assert_eq!(probe(&[0xff, 0xfb, 0x90, 0x00]).unwrap(), Probe::of("MP3", None, false));
        assert_eq!(probe(b"ID3\x03\x00\x00\x00\x00\x00\x00\xff\xfb\x90\x00").unwrap().codec, "MP3");
        assert_eq!(probe(b"OggS\x00\x02................OpusHead").unwrap(), Probe::of("Opus", Some("Ogg"), false));
        assert_eq!(probe(b"OggS\x00\x02................\x01vorbis").unwrap(), Probe::of("Vorbis", Some("Ogg"), false));
        assert_eq!(probe(&[0x1a, 0x45, 0xdf, 0xa3, 0x00]).unwrap().codec, "Opus");
    }

    #[test]
    fn something_unknown_or_cut_short_is_not_guessed() {
        assert_eq!(probe(b""), None);
        assert_eq!(probe(b"hello, world"), None);
        assert_eq!(probe(b"ID3\x04\x00\x00\x7f\x7f\x7f\x7f"), None, "a tag that claims more than the file has");
        // A marker with nothing after it is still FLAC, with no numbers to state.
        assert_eq!(probe(b"fLaC").unwrap(), Probe::of("FLAC", None, true));
    }

    /// `YTM_PROBE_DIR=<folder> cargo test probe::tests::real_files -- --ignored --nocapture`
    /// prints what each file in the folder is found to be, to check against real files.
    #[test]
    #[ignore = "reads files from a folder named in YTM_PROBE_DIR"]
    fn real_files() {
        let dir = std::env::var("YTM_PROBE_DIR").expect("YTM_PROBE_DIR");
        let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        for path in files {
            println!("{:>12}: {:?}", path.file_name().unwrap().to_string_lossy(), probe(&std::fs::read(&path).unwrap()));
        }
    }

    /// `YTM_PROBE_DIR=<folder> cargo test probe::tests::which_files_decode -- --ignored --nocapture`
    /// says, for each file in the folder, whether the decoder opens it as a file from the
    /// phone is opened, and with the hint its own bytes call for.
    #[test]
    #[ignore = "reads files from a folder named in YTM_PROBE_DIR"]
    fn which_files_decode() {
        use rodio::Source;
        let dir = std::env::var("YTM_PROBE_DIR").expect("YTM_PROBE_DIR");
        let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        for path in files {
            let data = std::fs::read(&path).unwrap();
            let try_with = |format: crate::stream::Format| {
                let shared = crate::stream::Shared::from_bytes_with_format(data.clone(), 0, None, format, None);
                match crate::stream::decoder(shared, true) {
                    Ok(d) => format!("ok, {} Hz, {} samples", d.sample_rate().get(), d.count()),
                    Err(e) => format!("FAILS: {e}"),
                }
            };
            let hint = crate::stream::Format::File { hint: "", mime: None };
            println!("{:>10}: as a phone file: {} | with no hint: {}", path.file_name().unwrap().to_string_lossy(), try_with(crate::stream::Format::Aac), try_with(hint));
        }
    }

    /// `YTM_PROBE_DIR=<folder> cargo test probe::tests::lossless_is_lossless -- --ignored --nocapture`
    /// decodes each `<name>.flac` in the folder the way a file from the phone is opened and
    /// compares it, sample for sample, with `<name>.m4a` beside it: the same audio packed by
    /// another lossless codec (ALAC). Two different codecs agreeing on every sample is only
    /// possible if neither loses anything.
    #[test]
    #[ignore = "reads pairs of files from a folder named in YTM_PROBE_DIR"]
    fn lossless_is_lossless() {
        use rodio::Source;
        let dir = std::path::PathBuf::from(std::env::var("YTM_PROBE_DIR").expect("YTM_PROBE_DIR"));
        let play = |path: &std::path::Path| -> (u32, u16, Vec<f32>) {
            let shared = crate::stream::Shared::from_file(std::fs::read(path).unwrap(), 0, None, crate::stream::Format::Aac, "This phone");
            let d = crate::stream::decoder(shared, true).unwrap_or_else(|e| panic!("{path:?} doesn't decode: {e}"));
            (d.sample_rate().get(), d.channels().get(), d.collect())
        };
        let mut pairs = 0;
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let flac = entry.path();
            let wav = flac.with_extension("m4a");
            if flac.extension().is_none_or(|e| e != "flac") || !wav.is_file() {
                continue;
            }
            let (rate, channels, packed) = play(&flac);
            let (wav_rate, wav_channels, plain) = play(&wav);
            let differing = packed.iter().zip(&plain).filter(|(a, b)| a != b).count();
            let loudest = packed.iter().zip(&plain).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            println!("{:>10}: {rate} Hz, {channels} ch, {} samples; differing from the ALAC copy: {differing} (largest difference {loudest})", flac.file_name().unwrap().to_string_lossy(), packed.len());
            assert_eq!((rate, channels, packed.len()), (wav_rate, wav_channels, plain.len()));
            assert_eq!(differing, 0, "a lossless file decodes to exactly the audio it was made from");
            pairs += 1;
        }
        assert!(pairs > 0, "no <name>.flac with a <name>.m4a beside it in {dir:?}");
    }

    #[test]
    fn a_lossless_files_info_carries_what_the_player_shows() {
        let p = probe(&flac_head(44_100, 2, 16)).unwrap();
        let info = p.info("This phone", 49_000_000, 393_000);
        assert!(info.lossless() && !info.hires());
        assert_eq!(info.badge(), "Lossless");
        assert_eq!(info.bitrate_kbps, Some(997), "the file's own bitrate");
        assert_eq!((info.source.as_str(), info.bit_depth), ("This phone", Some(16)));
    }
}
