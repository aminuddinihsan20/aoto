//! Direct InnerTube `/player` call using the visionOS client.
//!
//! As of Oct 2026 this client returns plain (unciphered) stream URLs that need neither
//! signature deobfuscation nor a PO token, and googlevideo serves every byte range for
//! them; the web clients' streams need a PO token or get cut off after 1 MiB, so this
//! is the path for everyone except signed-in Premium accounts (see `webmusic.rs`).
//! Mirrors yt-dlp's `visionos` client definition; update together if YouTube changes it.

use serde_json::{Value, json};

pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

pub struct Stream {
    pub url: String,
    pub size: u64,
    pub duration_ms: u64,
    /// dB above YouTube's -14 LUFS loudness target (positive = louder).
    pub loudness_db: Option<f32>,
    /// Opus in WebM; otherwise AAC in MP4.
    pub opus: bool,
}

/// A format's loudness relative to YouTube's target, falling back to the track-level
/// figure in `playerConfig.audioConfig`.
pub fn loudness(resp: &Value, format: &Value) -> Option<f32> {
    format["loudnessDb"].as_f64().map(|v| v as f32).or_else(|| {
        let a = &resp["playerConfig"]["audioConfig"];
        let track = a["trackAbsoluteLoudnessLkfs"].as_f64().or_else(|| a["perceptualLoudnessDb"].as_f64())?;
        Some((track - a["loudnessTargetLkfs"].as_f64().unwrap_or(-14.0)) as f32)
    })
}

/// The Android VR client's queue for an album's playlist (`OLAK5uy_…`). Unlike the web
/// clients, which list a track's music video when it has one, it lists the album's songs.
/// Needs no sign-in or PO token. Mirrors Morphe's "Play albums songs" patch
/// (github.com/MorpheApp/morphe-patches, `PlaylistRequest.java`); update together.
pub async fn album_queue(http: &reqwest::Client, playlist_id: &str) -> anyhow::Result<Value> {
    const VERSION: &str = "1.73.21";
    let body = json!({
        "context": {"client": {
            "clientName": "ANDROID_VR",
            "clientVersion": VERSION,
            "deviceMake": "Pico",
            "deviceModel": "A8110",
            "osName": "Android",
            "osVersion": "10",
            "androidSdkVersion": 29,
            // English, so the playlist's title says "Album - …".
            "hl": "en",
            "gl": "US",
        }},
        "playlistId": playlist_id,
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    Ok(http
        .post("https://youtubei.googleapis.com/youtubei/v1/next?prettyPrint=false")
        .header("User-Agent", format!("com.google.android.apps.youtube.vr.pico/{VERSION} (Linux; U; Android 10; A8110 Build/QKQ1) gzip"))
        .header("X-YouTube-Client-Name", "28")
        .header("X-YouTube-Client-Version", VERSION)
        .timeout(std::time::Duration::from_secs(8))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// `opus`: Opus (itag 251, about 130-160 kbps) when the song has it, else AAC.
pub async fn audio_stream(http: &reqwest::Client, video_id: &str, visitor_data: &str, opus: bool) -> anyhow::Result<Stream> {
    audio_stream_at(http, video_id, visitor_data, opus, crate::quality::Quality::High).await
}

/// The first of the song's formats that `quality` asks for (itag), with the format. A format
/// with no address or size (one YouTube only hands out signed) is passed over for the next.
fn pick_format<'a>(formats: &'a [Value], quality: crate::quality::Quality, opus: bool) -> Option<(u64, &'a Value)> {
    let usable = |f: &Value| f["url"].as_str().is_some() && f["contentLength"].as_str().and_then(|n| n.parse::<u64>().ok()).is_some();
    quality.itags(opus).iter().find_map(|itag| Some((*itag, formats.iter().find(|f| f["itag"] == *itag && usable(f))?)))
}

/// [`audio_stream`] at a chosen quality: High is what it always took; Normal and Low take
/// the smaller AAC streams (and fall back to the other one when the song has not got it).
pub async fn audio_stream_at(http: &reqwest::Client, video_id: &str, visitor_data: &str, opus: bool, quality: crate::quality::Quality) -> anyhow::Result<Stream> {
    let body = json!({
        "context": {"client": {
            "clientName": "VISIONOS",
            "clientVersion": "1.02",
            "deviceMake": "Apple",
            "deviceModel": "RealityDevice17,1",
            "userAgent": UA,
            "osName": "visionOS",
            "osVersion": "26.5.23O471",
            "hl": "en",
            "gl": "US",
            "visitorData": visitor_data,
        }},
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    let resp: Value = http
        .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
        .header("User-Agent", UA)
        .header("X-YouTube-Client-Name", "101")
        .header("X-YouTube-Client-Version", "1.02")
        .header("X-Goog-Visitor-Id", visitor_data)
        .header("Origin", "https://www.youtube.com")
        .timeout(std::time::Duration::from_secs(8))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let status = &resp["playabilityStatus"];
    if status["status"] != "OK" {
        anyhow::bail!(
            "{}: {}",
            status["status"].as_str().unwrap_or("UNPLAYABLE"),
            status["reason"].as_str().unwrap_or("no reason given")
        );
    }
    // Opus in WebM (251) when asked for; AAC-LC in MP4 (140), HE-AAC (139) as fallbacks.
    let formats = resp["streamingData"]["adaptiveFormats"].as_array().map(Vec::as_slice).unwrap_or_default();
    let (itag, f) = pick_format(formats, quality, opus).ok_or_else(|| anyhow::anyhow!("no audio stream in visionOS player response"))?;
    log::info!("visionOS: {video_id} itag {itag} ({} kbps, {quality:?})", f["averageBitrate"].as_u64().or(f["bitrate"].as_u64()).unwrap_or(0) / 1000);
    Ok(Stream {
        url: f["url"].as_str().ok_or_else(|| anyhow::anyhow!("no audio stream in visionOS player response"))?.to_owned(),
        size: f["contentLength"].as_str().and_then(|n| n.parse().ok()).ok_or_else(|| anyhow::anyhow!("no audio stream in visionOS player response"))?,
        duration_ms: f["approxDurationMs"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0),
        loudness_db: loudness(&resp, f),
        opus: itag == 251,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formats() -> Vec<Value> {
        [251u64, 140, 139].iter().map(|i| json!({"itag": i, "url": format!("https://x/{i}"), "contentLength": "1000"})).collect()
    }

    #[test]
    fn high_takes_opus_for_playback_and_aac_for_a_download() {
        use crate::quality::Quality;
        let f = formats();
        assert_eq!(pick_format(&f, Quality::High, true).unwrap().0, 251);
        assert_eq!(pick_format(&f, Quality::High, false).unwrap().0, 140);
    }

    #[test]
    fn normal_takes_aac_128_and_low_takes_he_aac() {
        use crate::quality::Quality;
        let f = formats();
        assert_eq!(pick_format(&f, Quality::Normal, true).unwrap().0, 140);
        assert_eq!(pick_format(&f, Quality::Low, true).unwrap().0, 139);
    }

    #[test]
    fn a_song_without_the_first_choice_falls_back_to_the_next() {
        use crate::quality::Quality;
        let only_140: Vec<Value> = formats().into_iter().filter(|f| f["itag"] == 140).collect();
        assert_eq!(pick_format(&only_140, Quality::Low, true).unwrap().0, 140, "no 139: 140 plays");
        assert_eq!(pick_format(&only_140, Quality::High, true).unwrap().0, 140, "no 251: 140 plays");
        let none: Vec<Value> = Vec::new();
        assert!(pick_format(&none, Quality::High, true).is_none());
    }

    #[test]
    fn a_format_with_no_address_or_size_is_passed_over() {
        use crate::quality::Quality;
        let f = vec![json!({"itag": 251, "signatureCipher": "s=..."}), json!({"itag": 140, "url": "https://x/140", "contentLength": "5"})];
        assert_eq!(pick_format(&f, Quality::High, true).unwrap().0, 140, "251 has no url: as before, 140 plays");
        let no_size = vec![json!({"itag": 140, "url": "https://x/140"})];
        assert!(pick_format(&no_size, Quality::Normal, true).is_none());
    }

    /// A real song at each quality: which stream comes back, its size, and that it
    /// downloads and decodes. `cargo test --lib live_quality_levels -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_quality_levels() {
        use crate::quality::Quality;
        use rodio::Source;
        let api = crate::ytmapi::Api::new(&std::env::temp_dir().join("ytm-rs-quality-test"));
        let vd = api.visitor_data(false).await.unwrap();
        let http = reqwest::Client::new();
        let mut sizes = Vec::new();
        for quality in Quality::ALL {
            let s = audio_stream_at(&http, "zuO2fClon98", &vd, true, quality).await.unwrap();
            println!("{quality:?}: opus={} size={} bytes, {} ms", s.opus, s.size, s.duration_ms);
            let shared = crate::stream::Shared::new(s.size, s.duration_ms, s.loudness_db, crate::stream::Format::from_opus(s.opus), None, false);
            crate::stream::download(http.clone(), s.url.clone(), UA.to_owned(), shared.clone(), None, crate::stream::Ranging::Query).await;
            assert!(shared.error().is_none(), "{:?}", shared.error());
            let decoder = crate::stream::decoder(shared, true).unwrap();
            let rate = decoder.sample_rate().get();
            let n = decoder.take(rate as usize * 2 * 5).count();
            println!("  decoded {n} samples at {rate} Hz");
            assert!(n > 0);
            sizes.push(s.size);
        }
        assert!(sizes[0] < sizes[1], "Low is smaller than Normal: {sizes:?}");
    }
}
