//! Music from folders the listener chose (Android: Storage Access Framework folders).
//!
//! Only chosen folders are looked at, never the whole phone, so ringtones and system
//! sounds don't turn up. Each folder is a collection of its own (say "Japanese" and
//! "English"), listed on its own page. Files are read by the activity (a `content://`
//! address is not a path), so a song is copied to the app's cache when it is played.
//!
//! `local.json` in the data dir keeps the folders and what was found in them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::{Link, Track};
use crate::platform::LocalFile;
use crate::stream::Shared;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    /// The folder's `content://` address.
    pub uri: String,
    /// What Android calls it, like `primary:Music/Japanese`.
    pub label: String,
    pub files: Vec<File>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
}

/// A folder as the pages list it.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderInfo {
    pub uri: String,
    pub name: String,
    pub songs: usize,
}

#[derive(Default)]
struct Library {
    folders: Vec<Folder>,
    /// Where it is saved (the data dir's local.json); empty before `init`.
    file: PathBuf,
}

static LIBRARY: LazyLock<RwLock<Library>> = LazyLock::new(|| RwLock::new(Library::default()));

/// "Japanese" from `primary:Music/Japanese`: the last part of the folder's label.
pub fn folder_name(label: &str) -> String {
    let after_volume = label.rsplit_once(':').map_or(label, |(_, path)| path);
    let name = after_volume.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if name.is_empty() { label.to_owned() } else { name.to_owned() }
}

/// The id a file's song has in queues and downloads: the same file always the same id.
pub fn song_id(uri: &str) -> String {
    // FNV-1a: stable across runs and platforms, unlike the std hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in uri.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("local:{h:016x}")
}

pub fn is_local(id: &str) -> bool {
    id.starts_with("local:")
}

fn track_of(f: &File) -> Track {
    Track {
        id: song_id(&f.uri),
        title: f.title.clone(),
        artists: if f.artist.is_empty() { Vec::new() } else { vec![Link { name: f.artist.clone(), id: None }] },
        album: (!f.album.is_empty()).then(|| Link { name: f.album.clone(), id: None }),
        duration: (f.duration_ms > 0).then_some((f.duration_ms / 1000) as u32),
        thumbs: Vec::new(),
        track_nr: None,
        plays: None,
        music_video: false,
    }
}

/// An album across all the folders.
#[derive(Clone, Debug, PartialEq)]
pub struct AlbumInfo {
    /// Stands for the album in addresses (see [`album_key`]).
    pub key: String,
    pub name: String,
    pub artist: String,
    pub songs: usize,
    /// The id of one of its songs, to take a name-matched look from.
    pub first_song: String,
}

/// An album is its name and its artist, ignoring case.
pub fn album_key(album: &str, artist: &str) -> String {
    format!("{}\u{1f}{}", album.trim().to_lowercase(), artist.trim().to_lowercase())
}

fn sort_key(f: &File) -> (String, String, String) {
    (f.artist.to_lowercase(), f.album.to_lowercase(), f.title.to_lowercase())
}

impl Library {
    fn add(&mut self, uri: &str, label: &str) -> bool {
        if self.folders.iter().any(|f| f.uri == uri) {
            return false;
        }
        self.folders.push(Folder { uri: uri.to_owned(), label: label.to_owned(), files: Vec::new() });
        true
    }

    fn remove(&mut self, uri: &str) {
        self.folders.retain(|f| f.uri != uri);
    }

    fn set_files(&mut self, uri: &str, found: &[LocalFile]) {
        let Some(folder) = self.folders.iter_mut().find(|f| f.uri == uri) else { return };
        folder.files = found
            .iter()
            .map(|f| File { uri: f.uri.clone(), title: f.title.clone(), artist: f.artist.clone(), album: f.album.clone(), duration_ms: f.duration_ms })
            .collect();
        // By artist, album, then title: a folder reads like a shelf.
        folder.files.sort_by_cached_key(sort_key);
    }

    /// Every folder's songs as one list, by artist, album and title.
    fn all_tracks(&self) -> Vec<Track> {
        let mut files: Vec<&File> = self.folders.iter().flat_map(|f| f.files.iter()).collect();
        files.sort_by_cached_key(|f| sort_key(f));
        files.into_iter().map(track_of).collect()
    }

    /// The albums across all folders: songs with the same album name and artist are one
    /// album, wherever they are kept. Songs without an album belong to none.
    fn albums(&self) -> Vec<AlbumInfo> {
        let mut found: Vec<AlbumInfo> = Vec::new();
        for f in self.folders.iter().flat_map(|f| f.files.iter()).filter(|f| !f.album.is_empty()) {
            let key = album_key(&f.album, &f.artist);
            match found.iter_mut().find(|a| a.key == key) {
                Some(a) => a.songs += 1,
                None => found.push(AlbumInfo { key, name: f.album.clone(), artist: f.artist.clone(), songs: 1, first_song: song_id(&f.uri) }),
            }
        }
        found.sort_by_cached_key(|a| (a.artist.to_lowercase(), a.name.to_lowercase()));
        found
    }

    /// One album's songs, in track order as the files sort.
    fn album_tracks(&self, key: &str) -> Vec<Track> {
        let mut files: Vec<&File> = self.folders.iter().flat_map(|f| f.files.iter()).filter(|f| !f.album.is_empty() && album_key(&f.album, &f.artist) == key).collect();
        files.sort_by_cached_key(|f| f.title.to_lowercase());
        files.into_iter().map(track_of).collect()
    }

    fn save(&self) {
        if self.file.as_os_str().is_empty() {
            return;
        }
        if let Ok(json) = serde_json::to_vec(&self.folders) {
            let _ = std::fs::write(&self.file, json);
        }
    }
}

/// Load the folders saved last time.
pub fn init(data_dir: &Path) {
    let file = data_dir.join("local.json");
    let folders = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut lib = LIBRARY.write().unwrap_or_else(|e| e.into_inner());
    lib.folders = folders;
    lib.file = file;
}

pub fn folders() -> Vec<FolderInfo> {
    let lib = LIBRARY.read().unwrap_or_else(|e| e.into_inner());
    lib.folders.iter().map(|f| FolderInfo { uri: f.uri.clone(), name: folder_name(&f.label), songs: f.files.len() }).collect()
}

/// The songs of one folder, in its order.
pub fn tracks(folder_uri: &str) -> Vec<Track> {
    let lib = LIBRARY.read().unwrap_or_else(|e| e.into_inner());
    lib.folders.iter().find(|f| f.uri == folder_uri).map(|f| f.files.iter().map(track_of).collect()).unwrap_or_default()
}

/// All the songs of every added folder, as one list.
pub fn all_tracks() -> Vec<Track> {
    LIBRARY.read().unwrap_or_else(|e| e.into_inner()).all_tracks()
}

pub fn albums() -> Vec<AlbumInfo> {
    LIBRARY.read().unwrap_or_else(|e| e.into_inner()).albums()
}

pub fn album_tracks(key: &str) -> Vec<Track> {
    LIBRARY.read().unwrap_or_else(|e| e.into_inner()).album_tracks(key)
}

/// The name of a folder, for headings.
pub fn name_of(folder_uri: &str) -> Option<String> {
    let lib = LIBRARY.read().unwrap_or_else(|e| e.into_inner());
    lib.folders.iter().find(|f| f.uri == folder_uri).map(|f| folder_name(&f.label))
}

/// Remember a chosen folder; false when it already was one.
pub fn add_folder(uri: &str, label: &str) -> bool {
    let mut lib = LIBRARY.write().unwrap_or_else(|e| e.into_inner());
    let added = lib.add(uri, label);
    lib.save();
    added
}

pub fn remove_folder(uri: &str) {
    let mut lib = LIBRARY.write().unwrap_or_else(|e| e.into_inner());
    lib.remove(uri);
    lib.save();
}

/// Look in the folder again (the activity lists it): blocks while it does, so run it on a
/// thread of its own. Answers how many songs it has now.
pub fn rescan(folder_uri: &str) -> Result<usize, String> {
    let text = crate::platform::call("scan", folder_uri, Duration::from_secs(10 * 60))?;
    let found = LocalFile::parse_list(&text);
    let mut lib = LIBRARY.write().unwrap_or_else(|e| e.into_inner());
    lib.set_files(folder_uri, &found);
    lib.save();
    Ok(found.len())
}

/// A local song's audio, as a complete buffer for playback, or `None` when `id` isn't one.
pub fn open(id: &str) -> Option<Arc<Shared>> {
    if !is_local(id) {
        return None;
    }
    let (uri, duration_ms) = {
        let lib = LIBRARY.read().unwrap_or_else(|e| e.into_inner());
        let file = lib.folders.iter().flat_map(|f| f.files.iter()).find(|f| song_id(&f.uri) == id)?;
        (file.uri.clone(), file.duration_ms)
    };
    let path = match crate::platform::call("fetch", &uri, Duration::from_secs(120)) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("{id}: couldn't read the file: {e}");
            return None;
        }
    };
    let data = std::fs::read(path).ok()?;
    log::info!("{id}: playing a local file");
    // Whatever the file is (FLAC, MP3, …), it says so itself; it is decoded as before.
    Some(Shared::from_file(data, duration_ms, None, crate::stream::Format::Aac, "This phone"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(uri: &str, title: &str, artist: &str) -> LocalFile {
        LocalFile { uri: uri.into(), title: title.into(), artist: artist.into(), album: String::new(), duration_ms: 200_000, size: 1 }
    }

    fn lib_with_two_folders() -> Library {
        let mut lib = Library::default();
        lib.add("content://t/jp", "primary:Music/Japanese");
        lib.add("content://t/en", "primary:Music/English");
        let song = |uri: &str, title: &str, artist: &str, album: &str| LocalFile { uri: uri.into(), title: title.into(), artist: artist.into(), album: album.into(), duration_ms: 1000, size: 1 };
        lib.set_files("content://t/jp", &[song("jp1", "Tabun", "YOASOBI", "THE BOOK"), song("jp2", "Yoru ni Kakeru", "YOASOBI", "THE BOOK"), song("jp3", "Single", "Aimer", "")]);
        lib.set_files("content://t/en", &[song("en1", "Hello", "Adele", "25"), song("en2", "Easy On Me", "Adele", "30"), song("en3", "Other", "X", "THE BOOK")]);
        lib
    }

    #[test]
    fn every_folders_songs_are_listed_together() {
        let lib = lib_with_two_folders();
        let titles: Vec<String> = lib.all_tracks().into_iter().map(|t| t.title).collect();
        assert_eq!(titles.len(), 6);
        assert_eq!(titles, ["Hello", "Easy On Me", "Single", "Other", "Tabun", "Yoru ni Kakeru"], "Adele (25, 30), Aimer, X, YOASOBI");
    }

    #[test]
    fn albums_gather_songs_across_folders_by_name_and_artist() {
        let lib = lib_with_two_folders();
        let albums = lib.albums();
        let names: Vec<(&str, &str, usize)> = albums.iter().map(|a| (a.name.as_str(), a.artist.as_str(), a.songs)).collect();
        assert!(names.contains(&("THE BOOK", "YOASOBI", 2)));
        assert!(names.contains(&("25", "Adele", 1)));
        // The same title by another artist is another album.
        assert!(names.contains(&("THE BOOK", "X", 1)));
        assert!(!names.iter().any(|(n, _, _)| n.is_empty()), "songs without an album are not an album");
    }

    #[test]
    fn an_albums_songs_come_back_by_its_key() {
        let lib = lib_with_two_folders();
        let key = album_key("THE BOOK", "YOASOBI");
        let titles: Vec<String> = lib.album_tracks(&key).into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Tabun", "Yoru ni Kakeru"]);
        assert!(lib.album_tracks("no such album").is_empty());
    }

    #[test]
    fn a_folder_is_named_by_the_last_part_of_its_label() {
        assert_eq!(folder_name("primary:Music/Japanese"), "Japanese");
        assert_eq!(folder_name("primary:Music/Musik Inggris/"), "Musik Inggris");
        assert_eq!(folder_name("1234-ABCD:Lagu"), "Lagu");
        assert_eq!(folder_name("primary:"), "primary:");
        assert_eq!(folder_name("Downloads"), "Downloads");
    }

    #[test]
    fn a_files_song_id_is_stable_and_marked_local() {
        let a = song_id("content://x/tree/primary%3AMusic/document/primary%3AMusic%2Fa.mp3");
        assert_eq!(a, song_id("content://x/tree/primary%3AMusic/document/primary%3AMusic%2Fa.mp3"));
        assert_ne!(a, song_id("content://x/tree/primary%3AMusic/document/primary%3AMusic%2Fb.mp3"));
        assert!(is_local(&a) && !is_local("dQw4w9WgXcQ"));
    }

    #[test]
    fn the_same_folder_is_added_once_and_removed_cleanly() {
        let mut lib = Library::default();
        assert!(lib.add("content://t/jp", "primary:Music/Japanese"));
        assert!(lib.add("content://t/en", "primary:Music/English"));
        assert!(!lib.add("content://t/jp", "primary:Music/Japanese"), "a folder chosen twice is one folder");
        lib.remove("content://t/jp");
        assert_eq!(lib.folders.iter().map(|f| f.uri.as_str()).collect::<Vec<_>>(), ["content://t/en"]);
    }

    #[test]
    fn each_folder_keeps_its_own_songs() {
        let mut lib = Library::default();
        lib.add("content://t/jp", "primary:Music/Japanese");
        lib.add("content://t/en", "primary:Music/English");
        lib.set_files("content://t/jp", &[found("content://t/jp/1", "Tabun", "YOASOBI")]);
        lib.set_files("content://t/en", &[found("content://t/en/1", "Hello", "Adele"), found("content://t/en/2", "Easy On Me", "Adele")]);
        let counts: Vec<usize> = lib.folders.iter().map(|f| f.files.len()).collect();
        assert_eq!(counts, [1, 2]);
        lib.set_files("content://nowhere", &[found("x", "y", "z")]);
        assert_eq!(lib.folders.len(), 2, "files for a folder that isn't there go nowhere");
    }

    #[test]
    fn a_folder_reads_by_artist_then_title() {
        let mut lib = Library::default();
        lib.add("t", "primary:A");
        lib.set_files("t", &[found("3", "Zebra", "Beck"), found("1", "Mad", "adele"), found("2", "Easy", "Adele")]);
        let titles: Vec<&str> = lib.folders[0].files.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(titles, ["Easy", "Mad", "Zebra"]);
    }

    #[test]
    fn a_file_becomes_a_song_with_what_its_tags_say() {
        let f = File { uri: "content://t/1".into(), title: "Tabun".into(), artist: "YOASOBI".into(), album: "THE BOOK".into(), duration_ms: 261_500 };
        let t = track_of(&f);
        assert_eq!((t.title.as_str(), t.artist_line().as_str(), t.duration), ("Tabun", "YOASOBI", Some(261)));
        assert_eq!(t.album.map(|a| a.name), Some("THE BOOK".to_owned()));
        let bare = track_of(&File { uri: "u".into(), title: "x".into(), artist: String::new(), album: String::new(), duration_ms: 0 });
        assert!(bare.artists.is_empty() && bare.album.is_none() && bare.duration.is_none());
    }

    #[test]
    fn the_library_is_saved_and_loaded_again() {
        let dir = std::env::temp_dir().join(format!("aoto-local-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut lib = Library { folders: Vec::new(), file: dir.join("local.json") };
        lib.add("content://t/jp", "primary:Music/Japanese");
        lib.set_files("content://t/jp", &[found("content://t/jp/1", "Tabun", "YOASOBI")]);
        lib.save();
        let back: Vec<Folder> = serde_json::from_slice(&std::fs::read(dir.join("local.json")).unwrap()).unwrap();
        assert_eq!(back, lib.folders);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
