//! Songs saved to disk: the audio as YouTube serves it (AAC in an .m4a, at the best
//! quality the account gets), tagged with title, artists, album and cover, in the
//! downloads folder. A downloaded song plays from its file, so it works offline.
//!
//! `downloads.json` in the data dir maps video ids to their files; a file that was
//! moved or deleted just stops counting as downloaded. `download_collections.json`
//! beside it keeps the albums and playlists that were downloaded whole, so their saved
//! songs can be shown together.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::model::{Thumb, Track};
use crate::stream::Shared;

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    duration_ms: u64,
    loudness_db: Option<f32>,
    /// A song version: the music video it was downloaded for, and the song, so that
    /// video plays it from this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stands_for: Option<String>,
    /// The song as it was downloaded, for the Downloads page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    song: Option<Track>,
}

/// An album or playlist that was asked for whole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub id: String,
    pub title: String,
    pub is_album: bool,
    pub thumbs: Vec<Thumb>,
    /// Its songs (video ids) in its own order, saved or not.
    pub songs: Vec<String>,
}

/// A kept album or playlist as the Downloads page shows it: the songs of it that are
/// saved, and what they take on disk.
pub struct Saved {
    pub id: String,
    pub title: String,
    pub is_album: bool,
    pub thumbs: Vec<Thumb>,
    pub songs: Vec<Track>,
    pub bytes: u64,
}

/// Keep `kept`, in place of what was kept of it before, newest first.
fn keep(list: &mut Vec<Kept>, kept: Kept) {
    list.retain(|k| k.id != kept.id);
    list.insert(0, kept);
}

/// What is saved of `kept`: `file` gives a saved song and its size. `None` when
/// nothing of it is saved.
fn saved_of(kept: &Kept, file: impl Fn(&str) -> Option<(Track, u64)>) -> Option<Saved> {
    let (songs, sizes): (Vec<Track>, Vec<u64>) = kept.songs.iter().filter_map(|id| file(id)).unzip();
    (!songs.is_empty()).then(|| Saved { id: kept.id.clone(), title: kept.title.clone(), is_album: kept.is_album, thumbs: kept.thumbs.clone(), songs, bytes: sizes.iter().sum() })
}

struct State {
    /// The albums and playlists downloaded whole, newest first.
    kept: Vec<Kept>,
    index: HashMap<String, Entry>,
    /// Where the index is saved (the data dir's downloads.json).
    file: PathBuf,
    folder: PathBuf,
    /// Android: the folder the listener chose (address and name), that every download is
    /// also copied into. The songs themselves stay in the app's own storage to play from.
    tree: Option<(String, String)>,
}

static STATE: LazyLock<RwLock<State>> =
    LazyLock::new(|| RwLock::new(State { kept: Vec::new(), index: HashMap::new(), file: PathBuf::new(), folder: default_folder(), tree: None }));

fn format_tree(uri: &str, label: &str) -> String {
    format!("{uri}\n{label}")
}

fn parse_tree(text: &str) -> Option<(String, String)> {
    let (uri, label) = text.split_once('\n')?;
    (!uri.is_empty()).then(|| (uri.to_owned(), label.trim().to_owned()))
}

fn export_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("m4a") => "audio/mp4",
        Some("flac") => "audio/flac",
        Some("webm") => "audio/webm",
        _ => "application/octet-stream",
    }
}

/// The activity's export request: address, file, name and type, joined by U+001E (the
/// request line itself is joined by U+001F, which would be flattened inside the argument).
fn export_arg(uri: &str, path: &Path, name: &str) -> String {
    [uri, &path.display().to_string(), name, export_mime(path)].join("\u{1e}")
}

/// A size as a phone's storage screen says it: KB, MB, or GB with a decimal.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.0} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

/// How much room the downloaded songs take.
pub fn total_bytes() -> u64 {
    let st = STATE.read().unwrap();
    st.index.values().filter_map(|e| std::fs::metadata(&e.path).ok()).map(|m| m.len()).sum()
}

/// The room on the disk `folder` is (or will be) on, in bytes: what an app may still
/// fill, and the disk's size. `None` where the system has no way to ask.
#[cfg(unix)]
pub fn disk_space(folder: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    // A folder not made yet: the one above it is on the same disk.
    let at = folder.ancestors().find(|p| p.exists())?;
    let path = std::ffi::CString::new(at.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a NUL-terminated string, and `stat` is room for the one
    // `statvfs` the call fills in when it returns 0.
    let stat = unsafe {
        if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) != 0 {
            return None;
        }
        stat.assume_init()
    };
    // The fields' widths differ from system to system.
    #[allow(clippy::unnecessary_cast)]
    let (block, free, all) = (stat.f_frsize as u64, stat.f_bavail as u64, stat.f_blocks as u64);
    Some((free * block, all * block))
}

#[cfg(not(unix))]
pub fn disk_space(_folder: &Path) -> Option<(u64, u64)> {
    None
}

/// An album or playlist is being downloaded whole: remember what it holds (as of now).
pub fn keep_collection(kept: Kept) {
    let mut st = STATE.write().unwrap();
    keep(&mut st.kept, kept);
    if let Ok(json) = serde_json::to_vec(&st.kept) {
        let _ = std::fs::write(st.file.with_file_name("download_collections.json"), json);
    }
}

/// The playlists (not albums) that were downloaded whole, with the songs they had then.
pub fn kept_playlists() -> Vec<Kept> {
    STATE.read().unwrap().kept.iter().filter(|k| !k.is_album).cloned().collect()
}

/// The albums and playlists downloaded whole that have a song saved, newest first.
pub fn collections() -> Vec<Saved> {
    let st = STATE.read().unwrap();
    let file = |id: &str| {
        let entry = st.index.get(id)?;
        let size = std::fs::metadata(&entry.path).ok().filter(|m| m.is_file())?.len();
        Some((entry.song.clone()?, size))
    };
    st.kept.iter().filter_map(|kept| saved_of(kept, file)).collect()
}

/// The folder chosen for downloads (address, name), on Android.
pub fn tree() -> Option<(String, String)> {
    STATE.read().unwrap().tree.clone()
}

pub fn set_tree(uri: &str, label: &str) {
    let mut st = STATE.write().unwrap();
    st.tree = Some((uri.to_owned(), label.to_owned()));
    let file = st.file.with_file_name("download_tree");
    let _ = std::fs::write(file, format_tree(uri, label));
}

/// Copy a downloaded file into the chosen folder (Android); the folder's name on success.
pub fn export(path: &Path) -> Result<String, String> {
    let (uri, label) = tree().ok_or("no download folder chosen")?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or("the file has no name")?;
    let arg = export_arg(&uri, path, name);
    crate::platform::call("export", &arg, std::time::Duration::from_secs(300))?;
    Ok(crate::local::folder_name(&label))
}

/// Music\Aoto.
pub fn default_folder() -> PathBuf {
    // Android: the app's own storage, where it can write and play from; the listener's
    // chosen folder gets a copy of each download (see `export`).
    if cfg!(target_os = "android")
        && let Some(data) = std::env::var_os("YTM_DATA_DIR")
    {
        return PathBuf::from(data).join("downloads");
    }
    let base = directories::UserDirs::new()
        .and_then(|u| u.audio_dir().map(Path::to_path_buf).or_else(|| Some(u.home_dir().join("Music"))))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Aoto")
}

/// Load the index; `folder` is the saved setting (empty for the default).
pub fn init(data_dir: &Path, folder: &str) {
    let file = data_dir.join("downloads.json");
    let index = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut st = STATE.write().unwrap();
    st.index = index;
    st.kept = std::fs::read(file.with_file_name("download_collections.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    st.tree = std::fs::read_to_string(file.with_file_name("download_tree")).ok().and_then(|t| parse_tree(&t));
    st.file = file;
    st.folder = if folder.is_empty() { default_folder() } else { PathBuf::from(folder) };
}

pub fn folder() -> PathBuf {
    STATE.read().unwrap().folder.clone()
}

pub fn set_folder(folder: PathBuf) {
    STATE.write().unwrap().folder = folder;
}

/// Ask for a new folder (the system picker, on its own thread).
#[cfg(not(target_os = "android"))]
pub fn pick_folder(ctx: &egui::Context) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        if let Some(dir) = rfd::FileDialog::new().set_title("Download folder").set_directory(folder()).pick_folder() {
            set_folder(dir);
            ctx.request_repaint();
        }
    });
}

/// Android: no folder picker yet (Settings hides "Change", see `crate::platform`).
#[cfg(target_os = "android")]
pub fn pick_folder(_ctx: &egui::Context) {}

pub fn open_folder() {
    let dir = folder();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

/// Open the song's folder in Explorer with the file selected.
pub fn show_file(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
    }
    // macOS: Finder reveals it.
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    // Linux: the file manager's ShowItems selects it (Nautilus, Dolphin, Nemo, …);
    // without one, just open the folder.
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
    {
        let uri = url::Url::from_file_path(path).map(|u| u.to_string()).unwrap_or_default();
        let shown = std::process::Command::new("dbus-send")
            .args(["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems"])
            .arg(format!("array:string:{uri}"))
            .arg("string:")
            .output()
            .is_ok_and(|o| o.status.success());
        if !shown && let Some(dir) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
    // Android: no file manager to hand it to yet.
    #[cfg(target_os = "android")]
    let _ = path;
}

/// The song's file, if it's downloaded and still there.
pub fn path(video_id: &str) -> Option<PathBuf> {
    STATE.read().unwrap().index.get(video_id).map(|e| e.path.clone()).filter(|p| p.is_file())
}

/// The downloaded songs still on disk, newest first.
pub fn list() -> Vec<Track> {
    let st = STATE.read().unwrap();
    let mut out: Vec<(std::time::SystemTime, Track)> = st
        .index
        .values()
        .filter(|e| e.path.is_file())
        .filter_map(|e| {
            let modified = std::fs::metadata(&e.path).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            e.song.clone().map(|t| (modified, t))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, t)| t).collect()
}

/// Forget `video_id` and delete its file. A file that is already gone still counts as
/// removed; one that can't be deleted (a refused permission) keeps its entry, so the song
/// doesn't vanish from the list while its file stays and takes the room.
fn forget(index: &mut HashMap<String, Entry>, video_id: &str) -> std::io::Result<Option<PathBuf>> {
    let Some(entry) = index.get(video_id) else { return Ok(None) };
    match std::fs::remove_file(&entry.path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(index.remove(video_id).map(|e| e.path))
}

/// Delete a downloaded song's file from the app's storage and drop it from the list.
/// On Android the copy in the folder the listener chose stays: it is theirs.
/// Returns whether the song was removed.
pub fn remove(video_id: &str) -> bool {
    let mut st = STATE.write().unwrap();
    match forget(&mut st.index, video_id) {
        Ok(Some(path)) => {
            log::info!("{video_id}: download removed ({})", path.display());
            if let Ok(json) = serde_json::to_vec(&st.index) {
                let _ = std::fs::write(&st.file, json);
            }
            true
        }
        Ok(None) => false,
        Err(e) => {
            log::warn!("{video_id}: couldn't remove the download: {e}");
            false
        }
    }
}

/// The key a saved copy of `id` has in `index`: `id` itself, or the song that was saved
/// for the music video `id`. Only a file that is still there counts.
fn saved_key_in(index: &HashMap<String, Entry>, id: &str) -> Option<String> {
    if index.get(id).is_some_and(|e| e.path.is_file()) {
        return Some(id.to_owned());
    }
    index.iter().find(|(_, e)| e.stands_for.as_deref() == Some(id) && e.path.is_file()).map(|(k, _)| k.clone())
}

/// The key `id`'s saved copy has, whether `id` is the song or the music video it was saved
/// for (the same song, shown under another id). `None`: not saved.
pub fn saved_key(id: &str) -> Option<String> {
    saved_key_in(&STATE.read().unwrap().index, id)
}

/// Is a copy of this song saved on the phone, under whichever id it is shown?
pub fn is_saved(id: &str) -> bool {
    saved_key(id).is_some()
}

/// The downloaded song version of the music video `video_id`, if its file is still there.
pub fn song_for(video_id: &str) -> Option<Track> {
    let st = STATE.read().unwrap();
    st.index.values().find(|e| e.stands_for.as_deref() == Some(video_id) && e.path.is_file()).and_then(|e| e.song.clone())
}

/// How a saved file is decoded, when it is not the AAC that downloads are by default.
fn playback_format(path: &Path) -> Option<crate::stream::Format> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("flac") => Some(crate::stream::Format::File { hint: "flac", mime: Some("audio/flac".to_owned()) }),
        _ => None,
    }
}

/// The extension a downloaded file gets, from what it is.
fn extension_of(audio: &[u8]) -> &'static str {
    if audio.get(4..8) == Some(b"ftyp") {
        "m4a"
    } else if audio.starts_with(b"fLaC") {
        "flac"
    } else {
        "webm"
    }
}

/// A downloaded song's audio, as a complete stream buffer for playback.
pub fn open(video_id: &str) -> Option<Arc<Shared>> {
    let entry = STATE.read().unwrap().index.get(video_id).cloned()?;
    let data = std::fs::read(&entry.path).ok()?;
    log::info!("{video_id}: playing the downloaded file");
    let format = playback_format(&entry.path).unwrap_or(crate::stream::Format::Aac);
    Some(Shared::from_file(data, entry.duration_ms, entry.loudness_db, format, "Downloaded"))
}

/// Write a downloaded song into the folder and remember it. Returns the file.
/// `stands_for`: the music video `t` is the song version of.
pub fn save(t: &Track, audio: &[u8], duration_ms: u64, loudness_db: Option<f32>, cover: Option<Vec<u8>>, lyrics: Option<&str>, stands_for: Option<String>) -> anyhow::Result<PathBuf> {
    let folder = folder();
    std::fs::create_dir_all(&folder)?;
    let ext = extension_of(audio);
    // A file already there for this song is replaced; another song's isn't.
    let mine = STATE.read().unwrap().index.get(&t.id).map(|e| e.path.clone());
    let stem = file_stem(t);
    let mut path = folder.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() && Some(&path) != mine.as_ref() {
        path = folder.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    let part = path.with_extension(format!("{ext}.part"));
    // YouTube's files say they're DASH segments ("dash" brand); as a file of its own,
    // call it audio, which some players and Explorer go by.
    let mut audio = audio.to_vec();
    if audio.get(8..12) == Some(b"dash") {
        audio[8..12].copy_from_slice(b"M4A ");
    }
    // A FLAC's tags go in its metadata blocks; the sound after them is copied as it is.
    if ext == "flac" {
        match tag_flac(&audio, t, cover.as_deref(), lyrics) {
            Ok(tagged) => audio = tagged,
            // Untagged audio still plays; keep it.
            Err(e) => log::warn!("couldn't tag {}: {e:#}", path.display()),
        }
    }
    std::fs::write(&part, &audio)?;
    if ext == "m4a"
        && let Err(e) = tag(&part, t, cover, lyrics)
    {
        // Untagged audio still plays; keep it.
        log::warn!("couldn't tag {}: {e:#}", path.display());
    }
    std::fs::rename(&part, &path)?;
    let mut st = STATE.write().unwrap();
    // Every entry keeps its song, so the Downloads page can list it later.
    let song = Some(t.clone());
    st.index.insert(t.id.clone(), Entry { path: path.clone(), duration_ms, loudness_db, stands_for, song });
    if let Ok(json) = serde_json::to_vec(&st.index) {
        let _ = std::fs::write(&st.file, json);
    }
    Ok(path)
}

/// "Artists - Title", safe as a Windows file name.
fn file_stem(t: &Track) -> String {
    let artists = t.artist_line();
    let name = if artists.is_empty() { t.title.clone() } else { format!("{artists} - {}", t.title) };
    let clean: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_end_matches('.').trim();
    let clean: String = clean.chars().take(150).collect();
    if clean.is_empty() { t.id.clone() } else { clean }
}

/// The FLAC in `audio` with `t`'s title, artists, album, track number and `cover` in it.
fn tag_flac(audio: &[u8], t: &Track, cover: Option<&[u8]>, lyrics: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let artists: Vec<String> = t.artists.iter().map(|a| a.name.clone()).collect();
    crate::flac_tags::write(audio, &crate::flac_tags::Tags { title: &t.title, artists: &artists, album: t.album.as_ref().map(|a| a.name.as_str()), track_nr: t.track_nr, cover, lyrics })
}

fn tag(path: &Path, t: &Track, cover: Option<Vec<u8>>, lyrics: Option<&str>) -> anyhow::Result<()> {
    let mut tag = mp4ameta::Tag::read_from_path(path)?;
    tag.set_title(&t.title);
    for (i, a) in t.artists.iter().enumerate() {
        if i == 0 {
            tag.set_artist(&a.name);
        } else {
            tag.add_artist(&a.name);
        }
    }
    if let Some(a) = t.artists.first() {
        tag.set_album_artist(&a.name);
    }
    if let Some(album) = &t.album {
        tag.set_album(&album.name);
    }
    if let Some(n) = t.track_nr {
        tag.set_track_number(n);
    }
    let img = cover.and_then(|c| match c.get(..4) {
        Some([0xff, 0xd8, ..]) => Some(mp4ameta::Img::jpeg(c)),
        Some([0x89, b'P', b'N', b'G']) => Some(mp4ameta::Img::png(c)),
        _ => None,
    });
    if let Some(img) = img {
        tag.set_artwork(img);
    }
    if let Some(text) = lyrics {
        tag.set_lyrics(text);
    }
    tag.write_to_path(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Link;

    #[test]
    fn the_chosen_download_folder_is_saved_as_two_lines_and_read_back() {
        let text = format_tree("content://x/tree/primary%3AMusic%2FAoto", "primary:Music/Aoto");
        assert_eq!(parse_tree(&text), Some(("content://x/tree/primary%3AMusic%2FAoto".to_owned(), "primary:Music/Aoto".to_owned())));
        assert_eq!(parse_tree(""), None);
        assert_eq!(parse_tree("only-one-line"), None);
        assert_eq!(parse_tree("\nlabel"), None, "an empty address is no folder");
    }

    fn song(id: &str) -> Track {
        Track { id: id.into(), title: format!("Song {id}"), artists: Vec::new(), album: None, duration: None, thumbs: Vec::new(), track_nr: None, plays: None, music_video: false }
    }

    fn kept(id: &str, songs: &[&str]) -> Kept {
        Kept { id: id.into(), title: format!("List {id}"), is_album: false, thumbs: Vec::new(), songs: songs.iter().map(|s| (*s).to_owned()).collect() }
    }

    #[test]
    fn a_kept_collection_shows_its_saved_songs_in_its_own_order_and_what_they_take() {
        // "b" was never saved (it failed, or the download was cancelled).
        let file = |id: &str| match id {
            "a" => Some((song("a"), 3_000)),
            "c" => Some((song("c"), 4_000)),
            _ => None,
        };
        let saved = saved_of(&kept("p", &["c", "b", "a"]), file).unwrap();
        assert_eq!(saved.songs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["c", "a"]);
        assert_eq!((saved.title.as_str(), saved.bytes), ("List p", 7_000));
    }

    #[test]
    fn a_kept_collection_with_no_song_saved_is_not_shown() {
        assert!(saved_of(&kept("p", &["a", "b"]), |_| None).is_none());
    }

    #[test]
    fn keeping_a_collection_again_brings_it_up_to_date_and_first() {
        let mut list = Vec::new();
        keep(&mut list, kept("p", &["a"]));
        keep(&mut list, kept("r", &["x"]));
        keep(&mut list, kept("p", &["a", "b"]));
        assert_eq!(list.iter().map(|k| k.id.as_str()).collect::<Vec<_>>(), ["p", "r"]);
        assert_eq!(list[0].songs, ["a", "b"]);
    }

    #[test]
    fn the_room_left_is_that_of_the_disk_the_folder_is_on() {
        let dir = std::env::temp_dir();
        let (free, total) = disk_space(&dir).expect("a unix system tells");
        assert!(free > 0 && free <= total, "{free} of {total}");
        // A folder not made yet is on the disk of the folder above it.
        assert!(disk_space(&dir.join("not-made-yet").join("downloads")).is_some());
    }

    #[test]
    fn sizes_read_like_a_phone_says_them() {
        assert_eq!(human_size(0), "0 KB");
        assert_eq!(human_size(1_536), "2 KB");
        assert_eq!(human_size(148 * 1024 * 1024), "148 MB");
        assert_eq!(human_size(1_610_612_736), "1.5 GB");
    }

    #[test]
    fn an_export_request_keeps_its_four_parts_through_the_request_line() {
        let arg = export_arg("content://x/tree/a", Path::new("/data/files/downloads/A - B.m4a"), "A - B.m4a");
        let line = crate::platform::CallRequest { id: 3, kind: "export".into(), arg }.to_line();
        let back = crate::platform::CallRequest::from_line(&line).unwrap();
        let parts: Vec<&str> = back.arg.split('\u{1e}').collect();
        assert_eq!(parts, ["content://x/tree/a", "/data/files/downloads/A - B.m4a", "A - B.m4a", "audio/mp4"]);
    }

    #[test]
    fn an_exported_file_is_named_and_typed_by_its_extension() {
        assert_eq!(export_mime(Path::new("/d/Adele - Hello.m4a")), "audio/mp4");
        assert_eq!(export_mime(Path::new("/d/x.webm")), "audio/webm");
        assert_eq!(export_mime(Path::new("/d/Adele - Hello.flac")), "audio/flac");
        assert_eq!(export_mime(Path::new("/d/x")), "application/octet-stream");
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        let track = |title: &str, artists: Vec<Link>| Track {
            id: "abc".into(),
            title: title.into(),
            artists,
            album: None,
            duration: None,
            thumbs: Vec::new(),
            track_nr: None,
            plays: None,
            music_video: false,
        };
        let t = track("What? / Why: \"Now\"...", vec![Link { name: "AC/DC".into(), id: None }]);
        assert_eq!(file_stem(&t), "AC_DC - What_ _ Why_ _Now_");
        assert_eq!(file_stem(&track("...", Vec::new())), "abc");
    }

    fn entry_at(path: PathBuf) -> Entry {
        Entry { path, duration_ms: 1000, loudness_db: None, stands_for: None, song: None }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aoto-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn removing_a_download_deletes_its_file_and_forgets_it() {
        let dir = temp_dir("remove-one");
        let (a, b) = (dir.join("a.m4a"), dir.join("b.m4a"));
        std::fs::write(&a, b"audio").unwrap();
        std::fs::write(&b, b"audio").unwrap();
        let mut index = HashMap::from([("a".to_owned(), entry_at(a.clone())), ("b".to_owned(), entry_at(b.clone()))]);
        assert_eq!(forget(&mut index, "a").unwrap(), Some(a.clone()));
        assert!(!a.exists());
        assert!(!index.contains_key("a"));
        assert!(b.is_file() && index.contains_key("b"), "another song's file is left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_whose_file_is_already_gone_is_still_forgotten() {
        let dir = temp_dir("remove-gone");
        let mut index = HashMap::from([("a".to_owned(), entry_at(dir.join("moved-away.m4a")))]);
        assert!(forget(&mut index, "a").unwrap().is_some());
        assert!(index.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_a_song_that_was_never_downloaded_changes_nothing() {
        let mut index = HashMap::from([("a".to_owned(), entry_at(PathBuf::from("/nowhere/a.m4a")))]);
        assert_eq!(forget(&mut index, "zzz").unwrap(), None);
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn a_file_that_cannot_be_deleted_keeps_its_entry() {
        let dir = temp_dir("remove-refused");
        // A folder in the file's place: remove_file refuses it.
        let path = dir.join("not-a-file.m4a");
        std::fs::create_dir_all(&path).unwrap();
        let mut index = HashMap::from([("a".to_owned(), entry_at(path.clone()))]);
        assert!(forget(&mut index, "a").is_err());
        assert!(index.contains_key("a"), "still listed: its file is still there");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_song_saved_for_a_music_video_is_found_under_the_videos_id_too() {
        let dir = temp_dir("saved-key");
        let file = dir.join("a.m4a");
        std::fs::write(&file, b"x").unwrap();
        let mut entry = entry_at(file.clone());
        entry.stands_for = Some("videoX".into());
        let index = HashMap::from([("songA".to_owned(), entry), ("songB".to_owned(), entry_at(dir.join("gone.m4a")))]);
        assert_eq!(saved_key_in(&index, "songA").as_deref(), Some("songA"), "by its own id");
        assert_eq!(saved_key_in(&index, "videoX").as_deref(), Some("songA"), "by the video it stands for");
        assert_eq!(saved_key_in(&index, "songB"), None, "its file is gone");
        assert_eq!(saved_key_in(&index, "other"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_song_whose_file_is_gone_is_not_found_by_the_video_either() {
        let dir = temp_dir("saved-key-gone");
        let mut entry = entry_at(dir.join("gone.m4a"));
        entry.stands_for = Some("videoX".into());
        let index = HashMap::from([("songA".to_owned(), entry)]);
        assert_eq!(saved_key_in(&index, "videoX"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_gets_the_extension_of_what_it_is() {
        assert_eq!(extension_of(b"\0\0\0\x20ftypM4A "), "m4a");
        assert_eq!(extension_of(b"fLaC\0\0\0\x22"), "flac");
        assert_eq!(extension_of(b"\x1aE\xdf\xa3 webm"), "webm");
        assert_eq!(extension_of(b""), "webm");
    }

    #[test]
    fn a_saved_flac_is_played_as_flac_and_other_files_as_before() {
        assert!(matches!(playback_format(Path::new("/d/a.flac")), Some(crate::stream::Format::File { hint: "flac", .. })));
        assert!(playback_format(Path::new("/d/a.m4a")).is_none());
        assert!(playback_format(Path::new("/d/a.webm")).is_none());
    }

    #[test]
    fn a_flac_download_gets_the_songs_tags() {
        let mut t = song("x");
        t.title = "Hujan di Kemang".into();
        t.artists = vec![Link { name: "Senja Kala".into(), id: None }];
        t.album = Some(Link { name: "Musim".into(), id: None });
        t.track_nr = Some(4);
        let tagged = tag_flac(&crate::flac_tags::tests::flac(500, &[], &[]), &t, None, Some("[00:01.00]Hujan")).unwrap();
        let (_, at) = read_flac_blocks(&tagged);
        assert!(tagged.len() > at);
        let text = String::from_utf8_lossy(&tagged[..at]).into_owned();
        for want in ["TITLE=Hujan di Kemang", "ARTIST=Senja Kala", "ALBUM=Musim", "TRACKNUMBER=4", "LYRICS=[00:01.00]Hujan"] {
            assert!(text.contains(want), "{want}");
        }
    }

    /// (blocks, where the audio starts) of a FLAC, by the header bytes.
    fn read_flac_blocks(file: &[u8]) -> (usize, usize) {
        let (mut at, mut n) = (4usize, 0usize);
        loop {
            let last = file[at] & 0x80 != 0;
            at += 4 + u32::from_be_bytes([0, file[at + 1], file[at + 2], file[at + 3]]) as usize;
            n += 1;
            if last {
                return (n, at);
            }
        }
    }

    /// A FLAC saved, listed, played back from its file and removed, through the real functions.
    #[test]
    fn a_flac_is_saved_tagged_found_played_and_removed() {
        use rodio::Source;
        let dir = std::env::temp_dir().join(format!("aoto-test-flac-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        init(&dir, dir.join("songs").to_str().unwrap());
        let mut t = song("flac1");
        t.artists = vec![Link { name: "Senja Kala".into(), id: None }];
        let audio = crate::flac_tags::tests::flac(9000, &[], &[]);
        let path = save(&t, &audio, 204, None, None, None, None).unwrap();
        assert_eq!(path.extension().unwrap(), "flac");
        assert_eq!(super::path("flac1"), Some(path.clone()));
        let shared = open("flac1").expect("the saved file opens");
        let decoder = crate::stream::decoder(shared, true).expect("and decodes as a FLAC");
        assert_eq!(decoder.sample_rate().get(), 44_100);
        assert_eq!(decoder.count(), 9000, "every sample of the song is there");
        assert!(remove("flac1"));
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
