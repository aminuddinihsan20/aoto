//! Parallel, seek-aware HTTP audio buffer.
//!
//! The file is split into fixed blocks fetched by a few concurrent range requests.
//! Workers always claim the first missing block at or after the one a reader is
//! waiting on, so playback start and seeks are served first and the rest fills in
//! around them. Reads of a missing block block until it arrives (or the download fails).
//!
//! Several readers may share one buffer (the audio engine opens a second, seekable
//! decoder when the user seeks); the download stops once the last reader is dropped.

use std::future::Future;
use std::io::{self, Read, Seek, SeekFrom};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use bytes::Bytes;

const BLOCK: u64 = 256 * 1024;
const WORKERS: usize = 4;
/// Tries per block before the download gives up, waiting a little longer each time
/// (about half a minute in all): long enough to ride out a dropped connection.
const RETRIES: u32 = 6;
/// A response that delivers nothing for this long is dead; a slow one that keeps
/// delivering is fine, however long the block takes.
const STALL: Duration = Duration::from_secs(20);
/// googlevideo occasionally starts rejecting a URL partway through (HTTP 403);
/// a freshly resolved URL for the same video resumes the download.
const MAX_REFRESHES: u32 = 2;
/// A prefetch only fetches this much (~16 s of audio) until playback actually opens it:
/// enough to start at once, without holding a whole extra song in memory.
const HEAD_BLOCKS: usize = 2;
/// Files larger than this (about 20 minutes at 256 kbps: long mixes, whole albums in
/// one video) aren't kept whole. Each reader keeps a window around where it reads,
/// `AHEAD` blocks forward (about 5 minutes at 256 kbps) and `BEHIND` back; the rest is dropped and
/// fetched again if a seek goes back to it.
const WINDOWED_FROM: u64 = 40 * 1024 * 1024;
const AHEAD: usize = 40;
const BEHIND: usize = 8;

/// Resolves a fresh stream URL for the same video.
pub type Refresh = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send>> + Send + Sync>;

/// The container and codec a stream carries.
#[derive(Clone, Debug, PartialEq)]
pub enum Format {
    /// AAC in MP4: YouTube's streams, and saved songs.
    Aac,
    /// Opus in WebM: YouTube without Premium, and addons that serve it.
    Opus,
    /// A file rodio opens with a hint and MIME type: FLAC, ALAC, WAV, MP3, Vorbis.
    File { hint: &'static str, mime: Option<String> },
    /// Interleaved signed 16-bit little-endian PCM (decoded in-process, e.g. Atmos).
    Pcm { rate: u32, channels: u16 },
}

impl Format {
    pub fn from_opus(opus: bool) -> Self {
        if opus { Self::Opus } else { Self::Aac }
    }
}

/// How a source takes byte ranges: YouTube's URLs carry them in the query string,
/// addon servers expect the standard `Range` header.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ranging {
    Query,
    Header,
}

struct Inner {
    blocks: Vec<Option<Bytes>>,
    in_flight: Vec<bool>,
    /// Block a reader most recently needed; workers prioritize from here.
    want: usize,
    error: Option<String>,
    readers: usize,
    /// Set once a reader existed and all were dropped.
    cancelled: bool,
    /// Blocks the workers may fetch; lifted to all of them when a reader opens.
    limit: usize,
    /// Each open reader's id and the block it last read (windowed files keep only
    /// what's near these).
    wants: Vec<(u64, usize)>,
    windowed: bool,
    /// Blocks fetched at least once, and how many: the download counts as finished
    /// once each was, even if a windowed file has dropped some since.
    ever: Vec<bool>,
    fetched: usize,
}

impl Inner {
    /// Whether block `i` belongs in memory now.
    fn in_window(&self, i: usize) -> bool {
        !self.windowed || self.wants.is_empty() || self.wants.iter().any(|&(_, w)| i + BEHIND >= w && i < w + AHEAD)
    }
}

enum Claim {
    Block(usize),
    /// Everything allowed so far is fetched or in flight; wait for the limit to lift.
    Wait,
    Done,
}

pub struct Shared {
    inner: Mutex<Inner>,
    ready: Condvar,
    pub total: u64,
    /// Track length reported by the stream metadata (0 if unknown).
    pub duration_ms: u64,
    /// YouTube's loudness for this stream, in dB above its -14 LUFS target.
    pub loudness_db: Option<f32>,
    pub tempo: Arc<std::sync::OnceLock<Option<f32>>>,
    /// The container and codec this buffer holds.
    pub format: Format,
    /// What the player can say about this stream, when its source reported it (addons).
    pub info: Option<crate::hires::Info>,
    /// Flips to true once the download task has stopped (complete, failed or cancelled).
    done: tokio::sync::watch::Sender<bool>,
    /// Wakes waiting workers: `limit` lifted, a reader moved or left, or cancelled.
    changed: tokio::sync::Notify,
}

impl Shared {
    /// A buffer for `total` bytes. `head_only` (a prefetch) fetches just the start until
    /// a reader opens it.
    pub fn new(total: u64, duration_ms: u64, loudness_db: Option<f32>, format: Format, info: Option<crate::hires::Info>, head_only: bool) -> Arc<Self> {
        let n = total.div_ceil(BLOCK) as usize;
        let limit = if head_only { HEAD_BLOCKS.min(n) } else { n };
        Arc::new(Self {
            inner: Mutex::new(Inner {
                blocks: vec![None; n],
                in_flight: vec![false; n],
                want: 0,
                error: None,
                readers: 0,
                cancelled: false,
                limit,
                wants: Vec::new(),
                windowed: total > WINDOWED_FROM,
                ever: vec![false; n],
                fetched: 0,
            }),
            ready: Condvar::new(),
            total,
            duration_ms,
            loudness_db,
            tempo: Arc::new(std::sync::OnceLock::new()),
            format,
            info,
            done: tokio::sync::watch::Sender::new(false),
            changed: tokio::sync::Notify::new(),
        })
    }

    /// A buffer that already holds the whole file (a downloaded song).
    pub fn from_bytes(data: Vec<u8>, duration_ms: u64, loudness_db: Option<f32>) -> Arc<Self> {
        // Saved songs are AAC (downloads take that stream).
        Self::from_bytes_with_format(data, duration_ms, loudness_db, Format::Aac, None)
    }

    /// A buffer that's already whole, in whatever container it was decoded to.
    pub fn from_bytes_with_format(data: Vec<u8>, duration_ms: u64, loudness_db: Option<f32>, format: Format, info: Option<crate::hires::Info>) -> Arc<Self> {
        let total = data.len() as u64;
        let data = Bytes::from(data);
        let blocks: Vec<Option<Bytes>> =
            (0..total.div_ceil(BLOCK)).map(|i| Some(data.slice(i as usize * BLOCK as usize..((i + 1) * BLOCK).min(total) as usize))).collect();
        let n = blocks.len();
        let shared = Self::new(total, duration_ms, loudness_db, format, info, false);
        {
            let mut inner = shared.inner.lock().unwrap();
            inner.blocks = blocks;
            inner.windowed = false;
            inner.ever = vec![true; n];
            inner.fetched = n;
        }
        shared.done.send_replace(true);
        shared
    }

    /// The whole file, once every block has arrived (a download nobody's reading).
    pub fn bytes(&self) -> Option<Vec<u8>> {
        let inner = self.inner.lock().unwrap();
        let mut out = Vec::with_capacity(self.total as usize);
        for b in &inner.blocks {
            out.extend_from_slice(b.as_ref()?);
        }
        Some(out)
    }

    /// The file's first block, when it has arrived: where an MP4 keeps its header.
    fn head(&self) -> Option<Bytes> {
        self.inner.lock().unwrap().blocks.first().cloned().flatten()
    }

    /// Every block is in memory: readers never wait and the buffer can be replayed.
    pub fn is_complete(&self) -> bool {
        self.inner.lock().unwrap().blocks.iter().all(Option::is_some)
    }

    /// Wait until the download stops, however it ends.
    pub async fn finished(&self) {
        let _ = self.done.subscribe().wait_for(|d| *d).await;
    }

    /// Why the download stopped early, if it failed.
    pub fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }

    /// Stop downloading (e.g. a prefetch that was superseded).
    pub fn cancel(&self) {
        self.inner.lock().unwrap().cancelled = true;
        self.ready.notify_all();
        self.changed.notify_waiters();
    }

    fn claim(&self) -> Claim {
        let mut inner = self.inner.lock().unwrap();
        if inner.cancelled || inner.error.is_some() {
            return Claim::Done;
        }
        let n = inner.blocks.len();
        let start = inner.want.min(n.saturating_sub(1));
        let mut missing = false;
        for i in (start..n).chain(0..start) {
            if inner.blocks[i].is_some() || inner.in_flight[i] {
                continue;
            }
            missing = true;
            if i < inner.limit && inner.in_window(i) {
                inner.in_flight[i] = true;
                return Claim::Block(i);
            }
        }
        // A windowed file may need dropped blocks again while it's being read.
        if missing || (inner.windowed && inner.readers > 0) { Claim::Wait } else { Claim::Done }
    }

    fn put(&self, idx: usize, data: Bytes) {
        let mut inner = self.inner.lock().unwrap();
        inner.blocks[idx] = Some(data);
        inner.in_flight[idx] = false;
        if !std::mem::replace(&mut inner.ever[idx], true) {
            inner.fetched += 1;
            if inner.fetched == inner.blocks.len() {
                self.done.send_replace(true);
            }
        }
        if inner.windowed && !inner.wants.is_empty() {
            for i in 0..inner.blocks.len() {
                if inner.blocks[i].is_some() && !inner.in_window(i) {
                    inner.blocks[i] = None;
                }
            }
        }
        self.ready.notify_all();
    }

    fn fail(&self, err: String) {
        log::error!("download failed: {err}");
        FAIL_FROM.store(usize::MAX, std::sync::atomic::Ordering::Relaxed);
        self.inner.lock().unwrap().error = Some(err);
        self.ready.notify_all();
    }

    fn range(&self, idx: usize) -> (u64, u64) {
        let start = idx as u64 * BLOCK;
        (start, (start + BLOCK).min(self.total) - 1)
    }
}

/// The current URL plus how many times it has been replaced.
struct UrlSlot {
    url: tokio::sync::Mutex<(String, u32)>,
    refresh: Option<Refresh>,
}

impl UrlSlot {
    /// Replace the URL if `seen` is still current; returns false when out of refreshes.
    async fn renew(&self, seen: u32) -> bool {
        let mut slot = self.url.lock().await;
        if slot.1 != seen {
            return true; // another worker already refreshed it
        }
        let Some(refresh) = self.refresh.as_ref().filter(|_| slot.1 < MAX_REFRESHES) else { return false };
        match refresh().await {
            Ok(url) => {
                log::info!("stream URL refreshed after 403");
                *slot = (url, slot.1 + 1);
                true
            }
            Err(e) => {
                log::warn!("stream URL refresh failed: {e:#}");
                false
            }
        }
    }
}

/// Download into `shared` with parallel range requests; returns when complete,
/// failed, or every reader was dropped.
pub async fn download(http: reqwest::Client, url: String, ua: String, shared: Arc<Shared>, refresh: Option<Refresh>, ranging: Ranging) {
    // Addon servers may ignore ranges and answer 200 with the whole file: then one
    // sequential download fills the buffer, rather than every worker pulling the file.
    if ranging == Ranging::Header && !ranges_supported(&http, &url, &ua).await {
        download_whole(&http, &url, &ua, &shared).await;
        return;
    }
    let slot = Arc::new(UrlSlot { url: tokio::sync::Mutex::new((url, 0)), refresh });
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..WORKERS {
        let (http, ua, shared, slot) = (http.clone(), ua.clone(), shared.clone(), slot.clone());
        set.spawn(async move {
            loop {
                // Registered before claiming, so a change in between isn't missed.
                let changed = shared.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                let idx = match shared.claim() {
                    Claim::Block(idx) => idx,
                    Claim::Wait => {
                        changed.await;
                        continue;
                    }
                    Claim::Done => break,
                };
                let (a, b) = shared.range(idx);
                let mut attempt = 0;
                loop {
                    let (url, version) = slot.url.lock().await.clone();
                    // A refreshed URL is tied to the address that resolved it, which is the
                    // current shared client's (a proxy may have switched exits since).
                    let client = if version > 0 { crate::proxy::http() } else { http.clone() };
                    let res = if idx >= FAIL_FROM.load(std::sync::atomic::Ordering::Relaxed) {
                        Err(anyhow::anyhow!("simulated network failure"))
                    } else {
                        match ranging {
                            Ranging::Query => fetch(&client, &format!("{url}&range={a}-{b}"), &ua, b - a + 1).await,
                            Ranging::Header => fetch_range(&client, &url, &ua, a, b, shared.total).await,
                        }
                    };
                    match res {
                        Ok(data) => {
                            shared.put(idx, data);
                            break;
                        }
                        Err(e) if is_forbidden(&e) && slot.renew(version).await => {}
                        Err(e) if attempt < RETRIES && !is_forbidden(&e) => {
                            attempt += 1;
                            log::warn!("range {a}-{b}: {e:#} (retry {attempt})");
                            tokio::time::sleep(Duration::from_millis(400 * (attempt * attempt) as u64)).await;
                        }
                        Err(e) => {
                            shared.fail(format!("range {a}-{b}: {e:#}"));
                            return;
                        }
                    }
                }
            }
        });
    }
    while set.join_next().await.is_some() {}
    shared.done.send_replace(true);
}

fn is_forbidden(e: &anyhow::Error) -> bool {
    e.downcast_ref::<reqwest::Error>()
        .and_then(reqwest::Error::status)
        .is_some_and(|s| s == reqwest::StatusCode::FORBIDDEN)
}

/// Dev hook: YTM_STREAM_FAIL=<block> makes the first download fail from that block on,
/// as if the connection died mid-song; later downloads work.
static FAIL_FROM: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

pub fn init_dev_hooks() {
    if let Some(b) = std::env::var("YTM_STREAM_FAIL").ok().and_then(|v| v.parse().ok()) {
        FAIL_FROM.store(b, std::sync::atomic::Ordering::Relaxed);
    }
}

async fn fetch(http: &reqwest::Client, url: &str, ua: &str, expect: u64) -> anyhow::Result<Bytes> {
    let stalled = || anyhow::anyhow!("no data for {} s", STALL.as_secs());
    // Errors leave the URL out: it carries the client's IP address and signatures (logs).
    let mut resp = tokio::time::timeout(STALL, http.get(url).header("User-Agent", ua).send())
        .await
        .map_err(|_| stalled())?
        .and_then(reqwest::Response::error_for_status)
        .map_err(reqwest::Error::without_url)?;
    let mut buf = bytes::BytesMut::with_capacity(expect as usize);
    while let Some(chunk) = tokio::time::timeout(STALL, resp.chunk()).await.map_err(|_| stalled())?.map_err(reqwest::Error::without_url)? {
        buf.extend_from_slice(&chunk);
    }
    anyhow::ensure!(buf.len() as u64 == expect, "short read: {} of {expect} bytes", buf.len());
    Ok(buf.freeze())
}

/// A block from a server that takes standard `Range` headers. A 200 (the server ignored
/// the range) still yields this block out of the whole body.
async fn fetch_range(http: &reqwest::Client, url: &str, ua: &str, a: u64, b: u64, total: u64) -> anyhow::Result<Bytes> {
    let stalled = || anyhow::anyhow!("no data for {} s", STALL.as_secs());
    let mut resp = tokio::time::timeout(
        STALL,
        http.get(url)
            .header("User-Agent", ua)
            .header("Range", format!("bytes={a}-{b}"))
            .header("Accept-Encoding", "identity")
            .send(),
    )
    .await
    .map_err(|_| stalled())?
    .and_then(reqwest::Response::error_for_status)
    .map_err(reqwest::Error::without_url)?;
    let partial = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let expect = if partial { b - a + 1 } else { total };
    let mut buf = bytes::BytesMut::with_capacity(expect as usize);
    while let Some(chunk) = tokio::time::timeout(STALL, resp.chunk()).await.map_err(|_| stalled())?.map_err(reqwest::Error::without_url)? {
        buf.extend_from_slice(&chunk);
    }
    anyhow::ensure!(buf.len() as u64 == expect, "short read: {} of {expect} bytes", buf.len());
    if partial {
        return Ok(buf.freeze());
    }
    Ok(buf.freeze().slice(a as usize..=b as usize))
}

/// Whether the server honours a `Range` header with a 206. A server that answers 200
/// gets one sequential download instead. A probe that can't connect leaves the workers
/// to try ranges themselves (a 200 there is still handled).
async fn ranges_supported(http: &reqwest::Client, url: &str, ua: &str) -> bool {
    match tokio::time::timeout(STALL, http.get(url).header("User-Agent", ua).header("Range", "bytes=0-0").header("Accept-Encoding", "identity").send()).await {
        Ok(Ok(r)) => r.status() == reqwest::StatusCode::PARTIAL_CONTENT,
        _ => true,
    }
}

/// One sequential download for a server that won't serve ranges, filling `shared` block
/// by block as bytes arrive so playback can start before the file is complete.
async fn download_whole(http: &reqwest::Client, url: &str, ua: &str, shared: &Arc<Shared>) {
    let stalled = || anyhow::anyhow!("no data for {} s", STALL.as_secs());
    let result = async {
        let mut resp = tokio::time::timeout(
            STALL,
            http.get(url).header("User-Agent", ua).header("Accept-Encoding", "identity").send(),
        )
        .await
        .map_err(|_| stalled())?
        .and_then(reqwest::Response::error_for_status)
        .map_err(reqwest::Error::without_url)?;
        let block = BLOCK as usize;
        let blocks = shared.inner.lock().unwrap().blocks.len();
        let mut buf = bytes::BytesMut::with_capacity(block);
        let (mut idx, mut received) = (0usize, 0u64);
        while let Some(chunk) = tokio::time::timeout(STALL, resp.chunk()).await.map_err(|_| stalled())?.map_err(reqwest::Error::without_url)? {
            received += chunk.len() as u64;
            buf.extend_from_slice(&chunk);
            while buf.len() >= block && idx < blocks {
                shared.put(idx, buf.split_to(block).freeze());
                idx += 1;
            }
            if shared.inner.lock().unwrap().cancelled {
                return anyhow::Ok(());
            }
        }
        anyhow::ensure!(received == shared.total, "short read: {received} of {} bytes", shared.total);
        if idx < blocks {
            shared.put(idx, buf.split().freeze());
        }
        anyhow::Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            shared.done.send_replace(true);
        }
        Err(e) => {
            if !shared.inner.lock().unwrap().cancelled {
                shared.fail(format!("stream: {e:#}"));
            }
        }
    }
}

pub struct StreamReader {
    shared: Arc<Shared>,
    pos: u64,
    id: u64,
    /// The block this reader last read.
    block: usize,
    /// Opened ahead of the song's own play (see `decoder_ahead`): dropping it leaves
    /// the download running for that play.
    ahead: bool,
}

impl StreamReader {
    pub fn new(shared: Arc<Shared>) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        {
            let mut inner = shared.inner.lock().unwrap();
            inner.readers += 1;
            inner.limit = inner.blocks.len();
            inner.wants.push((id, 0));
        }
        shared.changed.notify_waiters();
        Self { shared, pos: 0, id, block: 0, ahead: false }
    }

    fn ahead(shared: Arc<Shared>) -> Self {
        let mut reader = Self::new(shared);
        reader.ahead = true;
        reader
    }
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.readers -= 1;
        inner.wants.retain(|&(id, _)| id != self.id);
        if inner.readers == 0 && !self.ahead {
            // Stops the workers once their current request finishes.
            inner.cancelled = true;
        }
        self.shared.changed.notify_waiters();
    }
}

impl Read for StreamReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.pos >= self.shared.total {
            return Ok(0);
        }
        let idx = (self.pos / BLOCK) as usize;
        let mut inner = self.shared.inner.lock().unwrap();
        inner.want = idx;
        if idx != self.block {
            self.block = idx;
            if let Some(w) = inner.wants.iter_mut().find(|(id, _)| *id == self.id) {
                w.1 = idx;
            }
            if inner.windowed {
                self.shared.changed.notify_waiters();
            }
        }
        while inner.blocks[idx].is_none() {
            if let Some(e) = &inner.error {
                return Err(io::Error::other(e.clone()));
            }
            if inner.cancelled {
                return Err(io::Error::other("download cancelled"));
            }
            inner = self.shared.ready.wait(inner).unwrap();
        }
        let block = inner.blocks[idx].as_ref().unwrap();
        let off = (self.pos - idx as u64 * BLOCK) as usize;
        let n = out.len().min(block.len() - off);
        out[..n].copy_from_slice(&block[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let total = self.shared.total as i64;
        let new = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(off) => total + off,
            SeekFrom::Current(off) => self.pos as i64 + off,
        };
        if new < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = new as u64;
        Ok(self.pos)
    }
}

/// Open a decoder over `shared`. Non-seekable AAC decoders start after the first
/// fragment; seekable ones index every fragment first (i.e. effectively wait for the
/// download). Opus ones are always seekable: WebM's index is at the start. Raw PCM
/// (decoded in-process, e.g. Dolby Atmos) plays straight from the buffer.
pub fn decoder(shared: Arc<Shared>, seekable: bool) -> anyhow::Result<TrackDecoder> {
    open(shared, seekable, StreamReader::new)
}

/// Open a decoder over a song that isn't playing yet, from its start, for it to follow
/// the playing one without a gap. Unlike `decoder`'s, dropping it unplayed leaves the
/// download running: the song's own play still wants the buffer.
pub fn decoder_ahead(shared: Arc<Shared>) -> anyhow::Result<TrackDecoder> {
    open(shared, false, StreamReader::ahead)
}

fn open(shared: Arc<Shared>, seekable: bool, reader: fn(Arc<Shared>) -> StreamReader) -> anyhow::Result<TrackDecoder> {
    let size = shared.total;
    match shared.format {
        Format::Opus => {
            let duration_ms = shared.duration_ms;
            Ok(TrackDecoder::Opus(Box::new(crate::opus::OpusSource::new(reader(shared), size, duration_ms)?)))
        }
        Format::Pcm { rate, channels } => Ok(TrackDecoder::Pcm(PcmSource::new(reader(shared), rate, channels))),
        Format::Aac => {
            let builder = rodio::Decoder::builder()
                .with_data(reader(shared.clone()))
                .with_byte_len(size)
                .with_seekable(seekable)
                .with_mime_type("audio/mp4")
                .with_hint("m4a");
            let decoder = builder.build()?;
            // With gapless playback on, what the file's edit list says isn't the song
            // (the encoder's priming and padding) is left out: the decoder doesn't.
            let rate = rodio::Source::sample_rate(&decoder).get();
            let edit = HONOUR_EDITS.load(std::sync::atomic::Ordering::Relaxed).then(|| shared.head()).flatten().and_then(|head| edit_list(&head)).and_then(|e| e.in_frames(rate, shared.duration_ms));
            Ok(TrackDecoder::Rodio(Edited::new(decoder, edit)))
        }
        Format::File { ref hint, ref mime } => {
            let hint = *hint;
            let mime = mime.clone();
            let builder = rodio::Decoder::builder()
                .with_data(reader(shared))
                .with_byte_len(size)
                .with_seekable(seekable)
                .with_hint(hint);
            let builder = match mime.as_deref() {
                Some(mime) => builder.with_mime_type(mime),
                None => builder,
            };
            Ok(TrackDecoder::Rodio(Edited::new(builder.build()?, None)))
        }
    }
}

/// Whether AAC decoders play only what the MP4's edit list says is the song. Set with
/// gapless playback: off, a song plays as it always did, priming and padding included.
static HONOUR_EDITS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn honour_edits(on: bool) {
    HONOUR_EDITS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// An MP4 audio track's edit list, when it is the usual single edit: where in the
/// track the song starts (the samples before are the encoder's priming) and how long it
/// plays (what the last frame holds beyond that is padding), each in its own time scale.
#[derive(Debug, PartialEq)]
struct EditList {
    media_time: u64,
    media_scale: u32,
    duration: u64,
    movie_scale: u32,
}

/// What to play of a decoder's output: `skip` frames in, for `play` frames (to the end
/// when the file doesn't say).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Edit {
    skip: u64,
    play: Option<u64>,
}

impl EditList {
    /// In frames at the decoder's `rate`. Nothing when the priming is longer than any
    /// encoder's; and the length only when it is the one the stream was stated to have
    /// (`duration_ms`, 0 when unknown), so that a wrong header can't cut a song short.
    fn in_frames(&self, rate: u32, duration_ms: u64) -> Option<Edit> {
        if self.media_scale == 0 || self.movie_scale == 0 {
            return None;
        }
        let skip = self.media_time * rate as u64 / self.media_scale as u64;
        let play = self.duration * rate as u64 / self.movie_scale as u64;
        let stated = duration_ms * rate as u64 / 1000;
        let trusted = play > 0 && duration_ms > 0 && play.abs_diff(stated) <= rate as u64 / 4;
        (skip <= 8192).then_some(Edit { skip, play: trusted.then_some(play) })
    }
}

/// The edit list in an MP4's header (`head`: the start of the file, which holds it in
/// YouTube's streams and in saved songs).
fn edit_list(head: &[u8]) -> Option<EditList> {
    /// The boxes in `data`: (type, body).
    fn boxes(data: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
        let mut at = 0;
        std::iter::from_fn(move || {
            let size = u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize;
            let item = (data.get(at + 4..at + 8)?, data.get(at + 8..at.checked_add(size.max(8))?)?);
            at += size.max(8);
            Some(item)
        })
    }
    fn find<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
        boxes(data).find(|(k, _)| k == kind).map(|(_, body)| body)
    }
    fn be32(b: &[u8], at: usize) -> Option<u64> {
        Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as u64)
    }
    fn be64(b: &[u8], at: usize) -> Option<u64> {
        Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
    }
    /// The time scale of an `mvhd` or `mdhd`: after the two dates, 4 or 8 bytes each.
    fn scale(header: &[u8]) -> Option<u32> {
        be32(header, if *header.first()? == 1 { 20 } else { 12 }).map(|s| s as u32)
    }
    let moov = find(head, b"moov")?;
    let trak = find(moov, b"trak")?;
    let elst = find(find(trak, b"edts")?, b"elst")?;
    // One edit, and not an empty one (a media time of -1 delays the track instead).
    if be32(elst, 4)? != 1 {
        return None;
    }
    let (duration, media_time) = if elst[0] == 1 { (be64(elst, 8)?, be64(elst, 16)?) } else { (be32(elst, 8)?, be32(elst, 12)?) };
    if media_time > u32::MAX as u64 / 2 {
        return None;
    }
    Some(EditList { media_time, media_scale: scale(find(find(trak, b"mdia")?, b"mdhd")?)?, duration, movie_scale: scale(find(moov, b"mvhd")?)? })
}

/// A decoder playing only what an `Edit` says is the song. Positions are the song's:
/// a seek to 0 lands on its first sample, past the priming.
pub struct Edited<D> {
    inner: D,
    edit: Option<Edit>,
    /// Samples still to drop before the song starts, and how many of it are left to play.
    skip: u64,
    left: Option<u64>,
}

impl<D: rodio::Source> Edited<D> {
    fn new(inner: D, edit: Option<Edit>) -> Self {
        let channels = inner.channels().get() as u64;
        Self { skip: edit.map_or(0, |e| e.skip * channels), left: edit.and_then(|e| e.play).map(|p| p * channels), inner, edit }
    }
}

impl<D: rodio::Source> Iterator for Edited<D> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        while self.skip > 0 {
            self.inner.next()?;
            self.skip -= 1;
        }
        if let Some(left) = &mut self.left {
            *left = left.checked_sub(1)?;
        }
        self.inner.next()
    }
}

impl<D: rodio::Source> rodio::Source for Edited<D> {
    fn current_span_len(&self) -> Option<usize> {
        if self.left == Some(0) { Some(0) } else { self.inner.current_span_len() }
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let Some(edit) = self.edit else { return self.inner.try_seek(pos) };
        let (rate, channels) = (self.inner.sample_rate().get() as u64, self.inner.channels().get() as u64);
        let frame = (pos.as_secs_f64() * rate as f64).round() as u64;
        self.inner.try_seek(Duration::from_secs_f64((frame + edit.skip) as f64 / rate as f64))?;
        self.skip = 0;
        self.left = edit.play.map(|p| p.saturating_sub(frame) * channels);
        Ok(())
    }
}

/// What the decoder is being fed, for stats for nerds: the codec, the rate and channels
/// it decodes to, and the bitrate. The bit depth is the one a hi-res source states, as
/// rodio's decoders don't report theirs.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub codec: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate_kbps: Option<u32>,
}

impl Stats {
    /// `shared` as `decoder` reads it.
    pub fn of(shared: &Shared, decoder: &TrackDecoder) -> Self {
        let rate = rodio::Source::sample_rate(decoder).get();
        let channels = rodio::Source::channels(decoder).get();
        Self::from_parts(&shared.format, shared.info.as_ref(), shared.total, shared.duration_ms, rate, channels)
    }

    fn from_parts(format: &Format, info: Option<&crate::hires::Info>, size: u64, duration_ms: u64, sample_rate: u32, channels: u16) -> Self {
        let codec = match info.map(|i| i.codec.trim()).filter(|c| !c.is_empty()) {
            Some(c) => codec_label(c),
            None => match format {
                Format::Aac => "AAC".to_owned(),
                Format::Opus => "Opus".to_owned(),
                Format::File { hint, .. } => codec_label(hint),
                Format::Pcm { .. } => "PCM".to_owned(),
            },
        };
        let bit_depth = info.and_then(|i| i.bit_depth);
        let measured = (duration_ms > 0 && size != crate::hires::UNKNOWN_SIZE).then(|| (size * 8 / duration_ms) as u32);
        let bitrate_kbps = match bit_depth {
            Some(bits) if info.is_some_and(|i| i.lossless()) => Some(sample_rate * bits * channels as u32 / 1000),
            _ => info.and_then(|i| i.bitrate_kbps).or(measured),
        };
        Self { codec, bit_depth, sample_rate, channels, bitrate_kbps: bitrate_kbps.filter(|k| *k > 0) }
    }
}

fn codec_label(codec: &str) -> String {
    match codec.to_ascii_lowercase().as_str() {
        "opus" => "Opus".to_owned(),
        "vorbis" => "Vorbis".to_owned(),
        _ => codec.to_ascii_uppercase(),
    }
}

/// A song's decoder: Opus (our own), in-process PCM (decoded Atmos), or a file
/// rodio decodes (AAC/MP4 included).
pub enum TrackDecoder {
    Rodio(Edited<rodio::Decoder<StreamReader>>),
    /// Boxed: its decoder state is about 11 KB.
    Opus(Box<crate::opus::OpusSource>),
    Pcm(PcmSource),
}

impl Iterator for TrackDecoder {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        match self {
            Self::Rodio(d) => d.next(),
            Self::Opus(d) => d.next(),
            Self::Pcm(d) => d.next(),
        }
    }
}

impl rodio::Source for TrackDecoder {
    fn current_span_len(&self) -> Option<usize> {
        match self {
            Self::Rodio(d) => d.current_span_len(),
            Self::Opus(d) => d.current_span_len(),
            Self::Pcm(d) => d.current_span_len(),
        }
    }
    fn channels(&self) -> rodio::ChannelCount {
        match self {
            Self::Rodio(d) => d.channels(),
            Self::Opus(d) => d.channels(),
            Self::Pcm(d) => d.channels(),
        }
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        match self {
            Self::Rodio(d) => d.sample_rate(),
            Self::Opus(d) => d.sample_rate(),
            Self::Pcm(d) => d.sample_rate(),
        }
    }
    fn total_duration(&self) -> Option<Duration> {
        match self {
            Self::Rodio(d) => d.total_duration(),
            Self::Opus(d) => d.total_duration(),
            Self::Pcm(d) => d.total_duration(),
        }
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        match self {
            Self::Rodio(d) => d.try_seek(pos),
            Self::Opus(d) => d.try_seek(pos),
            Self::Pcm(d) => d.try_seek(pos),
        }
    }
}

/// Interleaved signed 16-bit little-endian PCM read from the stream buffer.
pub struct PcmSource {
    reader: StreamReader,
    rate: u32,
    channels: u16,
    duration: Option<Duration>,
}

impl PcmSource {
    fn new(reader: StreamReader, rate: u32, channels: u16) -> Self {
        let duration_ms = reader.shared.duration_ms;
        Self { reader, rate, channels, duration: (duration_ms > 0).then(|| Duration::from_millis(duration_ms)) }
    }
}

impl Iterator for PcmSource {
    type Item = rodio::Sample;
    fn next(&mut self) -> Option<rodio::Sample> {
        let mut buf = [0u8; 2];
        if self.reader.read_exact(&mut buf).is_err() {
            return None;
        }
        Some(i16::from_le_bytes(buf) as f32 / 32768.0)
    }
}

impl rodio::Source for PcmSource {
    /// The samples left in the file, frame-aligned and `Some(0)` once it's read to the
    /// end. Never `None`: rodio's `UniformSourceIterator` bootstraps its
    /// `SampleRateConverter` with `Take { n: span }` and only re-reads the source's rate
    /// and channel count when that `Take` runs out, so an unknown span freezes the
    /// `from` rate for the whole player — the 44.1 kHz song after this one is then
    /// resampled as if it were 48 kHz and plays about 9 % fast.
    fn current_span_len(&self) -> Option<usize> {
        let left = self.reader.shared.total.saturating_sub(self.reader.pos) as usize / 2;
        let ch = self.channels.max(1) as usize;
        Some(left / ch * ch)
    }
    fn channels(&self) -> rodio::ChannelCount { std::num::NonZeroU16::new(self.channels).unwrap_or(std::num::NonZeroU16::MIN) }
    fn sample_rate(&self) -> rodio::SampleRate { std::num::NonZeroU32::new(self.rate).unwrap_or(std::num::NonZeroU32::MIN) }
    fn total_duration(&self) -> Option<Duration> { self.duration }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let frame = (pos.as_secs_f64() * self.rate as f64) as u64;
        let byte = frame * self.channels as u64 * 2;
        self.reader.seek(SeekFrom::Start(byte)).map_err(|e| rodio::source::SeekError::Other(std::sync::Arc::new(e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// The song queued after a PCM one is resampled at its own rate. `cargo test pcm_then_44k_rate`
    #[test]
    fn pcm_then_44k_rate() {
        let nz16 = |n| std::num::NonZeroU16::new(n).unwrap();
        let nz32 = |n| std::num::NonZeroU32::new(n).unwrap();
        // A second of 48 kHz PCM stereo, ±8000 so it can't be mistaken for the ones below.
        let mut pcm = Vec::with_capacity(48_000 * 2 * 2);
        for _ in 0..48_000 {
            pcm.extend_from_slice(&8_000i16.to_le_bytes());
            pcm.extend_from_slice(&(-8_000i16).to_le_bytes());
        }
        let shared = super::Shared::from_bytes_with_format(pcm, 1_000, None, super::Format::Pcm { rate: 48_000, channels: 2 }, None);
        let (tx, rx) = rodio::queue::queue(false);
        tx.append(super::decoder(shared, true).unwrap());
        // One second of 44.1 kHz, all ones.
        tx.append(rodio::buffer::SamplesBuffer::new(nz16(2), nz32(44_100), vec![1.0f32; 44_100 * 2]));
        let out: Vec<f32> = rodio::source::UniformSourceIterator::new(rx, nz16(2), nz32(48_000)).collect();
        let second = out.iter().skip(48_000 * 2).filter(|&&x| (x - 1.0).abs() < 1e-3).count() / 2;
        assert!((second as i64 - 48_000).abs() < 200, "the 44.1 kHz song came out as {second} frames, not 48000");
    }

    /// An MP4 box: its size, its type, its body.
    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        [&(body.len() as u32 + 8).to_be_bytes()[..], kind, body].concat()
    }

    /// The header of an AAC stream as YouTube writes it: `priming` samples before the
    /// song, which plays for `length` (both in a 44.1 kHz time scale).
    fn mp4_head(priming: u32, length: u32) -> Vec<u8> {
        let be = |v: u32| v.to_be_bytes();
        // Version and flags, two dates, the time scale, the duration.
        let header = [&[0u8; 12][..], &be(44_100), &be(length)].concat();
        let elst = [&[0u8; 4][..], &be(1), &be(length), &be(priming), &be(0x0001_0000)].concat();
        let trak = [mp4_box(b"tkhd", &[0; 84]), mp4_box(b"edts", &mp4_box(b"elst", &elst)), mp4_box(b"mdia", &mp4_box(b"mdhd", &header))].concat();
        [mp4_box(b"ftyp", b"dash"), mp4_box(b"moov", &[mp4_box(b"mvhd", &header), mp4_box(b"trak", &trak)].concat()), mp4_box(b"sidx", &[0; 40])].concat()
    }

    #[test]
    fn an_mp4s_edit_list_says_how_much_of_the_track_is_not_the_song() {
        // The numbers of a real stream: 1600 samples of priming, 10,405,248 of song.
        let list = super::edit_list(&mp4_head(1600, 10_405_248)).unwrap();
        assert_eq!(list, super::EditList { media_time: 1600, media_scale: 44_100, duration: 10_405_248, movie_scale: 44_100 });
        assert_eq!(list.in_frames(44_100, 235_946), Some(super::Edit { skip: 1600, play: Some(10_405_248) }));
        // A length that isn't the one the stream was stated to have cuts nothing off the end.
        assert_eq!(list.in_frames(44_100, 300_000), Some(super::Edit { skip: 1600, play: None }));
        assert_eq!(list.in_frames(44_100, 0), Some(super::Edit { skip: 1600, play: None }));
        // More priming than any encoder's is a header not to be trusted.
        assert_eq!(super::edit_list(&mp4_head(90_000, 10_405_248)).unwrap().in_frames(44_100, 235_946), None);
        // No header, a cut-off one, another kind of file: nothing is left out.
        assert_eq!(super::edit_list(&[]), None);
        assert_eq!(super::edit_list(&mp4_head(1600, 10_405_248)[..60]), None);
        assert_eq!(super::edit_list(b"OggS\0\x02 not an mp4 at all, whatever it says"), None);
    }

    #[test]
    fn a_decoder_plays_the_song_without_its_priming_and_padding_and_seeks_within_it() {
        use rodio::Source;
        // Mono at 1 kHz, each sample its own number: 5 of priming, 20 of song, 7 of padding.
        let track = || rodio::buffer::SamplesBuffer::new(std::num::NonZeroU16::new(1).unwrap(), std::num::NonZeroU32::new(1000).unwrap(), (0..32).map(|i| i as f32).collect::<Vec<_>>());
        let edit = Some(super::Edit { skip: 5, play: Some(20) });
        let song: Vec<f32> = (5..25).map(|i| i as f32).collect();
        assert_eq!(super::Edited::new(track(), edit).collect::<Vec<_>>(), song);
        // Positions are the song's own: its 8th millisecond is the track's 13th sample.
        let mut d = super::Edited::new(track(), edit);
        d.try_seek(std::time::Duration::from_millis(8)).unwrap();
        assert_eq!(d.by_ref().collect::<Vec<_>>(), song[8..]);
        assert_eq!(d.current_span_len(), Some(0));
        // Without an edit the track plays as it is.
        assert_eq!(super::Edited::new(track(), None).count(), 32);
        assert_eq!(super::Edited::new(track(), Some(super::Edit { skip: 5, play: None })).count(), 27);
    }

    /// What a YouTube stream's decoder hands out around the song itself: the encoder's
    /// priming before it and its padding after, which stand between two songs played
    /// back to back however closely the player joins them.
    /// `cargo test --release -- --ignored encoder_padding_live --nocapture`
    #[tokio::test]
    #[ignore]
    async fn encoder_padding_live() {
        let api = crate::ytmapi::Api::new(&std::env::temp_dir().join("ytm-rs-opus-test"));
        let vd = api.visitor_data(false).await.unwrap();
        let http = reqwest::Client::new();
        for id in ["zuO2fClon98", "N48TFWsBA-k"] {
            for opus in [true, false] {
                let s = crate::innertube::audio_stream(&http, id, &vd, opus).await.unwrap();
                let shared = super::Shared::new(s.size, s.duration_ms, s.loudness_db, super::Format::from_opus(s.opus), None, false);
                super::download(http.clone(), s.url, crate::innertube::UA.to_owned(), shared.clone(), None, super::Ranging::Query).await;
                let d = super::decoder(shared.clone(), false).unwrap();
                let (ch, rate) = (rodio::Source::channels(&d).get() as usize, rodio::Source::sample_rate(&d).get() as f64);
                let all: Vec<f32> = d.collect();
                let frames = all.len() / ch;
                // Digital silence, and anything under -80 dB, at each end.
                let quiet = |limit: f32, it: &mut dyn Iterator<Item = &[f32]>| it.take_while(|f| f.iter().all(|x| x.abs() <= limit)).count();
                let lead0 = quiet(0.0, &mut all.chunks(ch));
                let lead80 = quiet(1e-4, &mut all.chunks(ch));
                let tail0 = quiet(0.0, &mut all.chunks(ch).rev());
                let tail80 = quiet(1e-4, &mut all.chunks(ch).rev());
                println!(
                    "{id} {}: {frames} frames at {rate} Hz = {:.4} s decoded, {:.4} s stated ({:+.1} ms); leading zeros {lead0} frames ({:.1} ms), under -80 dB {lead80} ({:.1} ms); trailing zeros {tail0} ({:.1} ms), under -80 dB {tail80} ({:.1} ms)",
                    if opus { "opus" } else { "aac " },
                    frames as f64 / rate,
                    s.duration_ms as f64 / 1000.0,
                    frames as f64 / rate * 1000.0 - s.duration_ms as f64,
                    lead0 as f64 / rate * 1000.0,
                    lead80 as f64 / rate * 1000.0,
                    tail0 as f64 / rate * 1000.0,
                    tail80 as f64 / rate * 1000.0,
                );
                if !opus {
                    let list = super::edit_list(&shared.bytes().unwrap());
                    println!("    {list:?}");
                    // Honouring it, exactly the song is left: its stated length, to the sample.
                    super::honour_edits(true);
                    let edited = super::decoder(shared.clone(), false).unwrap().count() / ch;
                    super::honour_edits(false);
                    println!("    with the edit list honoured: {edited} frames = {:.4} s", edited as f64 / rate);
                    assert_eq!(edited as u64, list.unwrap().duration);
                }
            }
        }
    }
}
