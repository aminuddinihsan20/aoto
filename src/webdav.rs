//! Music on a WebDAV server (a NAS, Nextcloud, a computer sharing a folder): the server
//! is listed with PROPFIND, its audio files found below a chosen folder, and a song plays
//! by streaming its file with Range requests, so it starts at once and seeks.
//!
//! What is kept (`webdav.json`): the servers and the files found on them. The password is
//! in the secret store, never in that file. A song's title and artist come from where the
//! file is and what it is called, because reading tags would mean fetching every file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::{Link, Track};

/// How deep a scan goes below the folder it starts in, and how many songs it keeps.
const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 20_000;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    /// The folder music is read from, as an address (https://nas.local/dav/Music/).
    pub url: String,
    pub user: String,
}

/// An audio file found on a server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    /// Where it is on the server, decoded (`/dav/Music/Artist/Album/01 Song.flac`).
    pub path: String,
    pub size: u64,
}

/// One answer of a PROPFIND.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

// ------------------------------------------------------------------ reading an answer

/// `%41` → `A`. A bad escape stays as it is.
fn decode(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The path of an `href`, which a server writes as a path or as a whole address.
fn href_path(href: &str) -> String {
    let h = href.trim();
    let path = match h.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("/", |i| &rest[i..]),
        None => h,
    };
    decode(path.split(['?', '#']).next().unwrap_or(""))
}

/// The entries of a PROPFIND answer (a `multistatus`), without the folder asked about.
/// Servers differ in the prefix they give the DAV namespace (`D:`, `d:`, none), so names
/// are matched without it.
pub fn parse_multistatus(xml: &str, asked: &str) -> Vec<Entry> {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return Vec::new() };
    let asked = asked.trim_end_matches('/');
    let mut out = Vec::new();
    for resp in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "response") {
        let Some(href) = resp.descendants().find(|n| n.is_element() && n.tag_name().name() == "href").and_then(|n| n.text()) else { continue };
        let path = href_path(href);
        if path.trim_end_matches('/') == asked {
            continue;
        }
        // Only the properties of a 200 answer count; a prop the server could not give is a 404 one.
        let ok = |n: &roxmltree::Node| {
            n.ancestors().find(|a| a.tag_name().name() == "propstat").and_then(|p| p.descendants().find(|d| d.is_element() && d.tag_name().name() == "status")).and_then(|s| s.text()).is_none_or(|t| t.contains(" 200"))
        };
        let is_dir = resp.descendants().any(|n| n.is_element() && n.tag_name().name() == "collection" && ok(&n)) || path.ends_with('/');
        let size = resp.descendants().find(|n| n.is_element() && n.tag_name().name() == "getcontentlength" && ok(n)).and_then(|n| n.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_owned();
        if name.is_empty() {
            continue;
        }
        out.push(Entry { path, name, is_dir, size });
    }
    out
}

// ------------------------------------------------------------------ what a file is

/// The decoder hint for an audio file, from its name.
pub fn audio_hint(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "flac" => "flac",
        "mp3" => "mp3",
        "m4a" | "mp4" | "aac" | "alac" => "m4a",
        "ogg" | "oga" => "ogg",
        "wav" => "wav",
        _ => return None,
    })
}

/// The mime type for a hint.
pub fn mime_of(hint: &str) -> &'static str {
    match hint {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "audio/mp4",
    }
}

/// (title, artist, album) of a file from its path below `root` (the folder the scan began
/// in). A leading track number is dropped. `Artist - Title` names give both; otherwise the
/// folders say: `Artist/Album/Title`, or `Artist/Title` with one folder.
pub fn meta_from_path(path: &str, root: &str) -> (String, String, String) {
    let below = path.strip_prefix(root.trim_end_matches('/')).unwrap_or(path);
    let mut parts: Vec<&str> = below.split('/').filter(|p| !p.is_empty()).collect();
    let file = parts.pop().unwrap_or("");
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let stem = strip_track_number(stem);
    if let Some((artist, title)) = stem.split_once(" - ") {
        return (title.trim().to_owned(), artist.trim().to_owned(), parts.last().copied().unwrap_or("").to_owned());
    }
    match parts.as_slice() {
        [] => (stem.to_owned(), String::new(), String::new()),
        [artist] => (stem.to_owned(), (*artist).to_owned(), String::new()),
        [.., artist, album] => (stem.to_owned(), (*artist).to_owned(), (*album).to_owned()),
    }
}

/// `01 Song`, `01. Song`, `1-Song`, `101_Song` → `Song`. A name that is only a number stays.
fn strip_track_number(stem: &str) -> &str {
    let digits = stem.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return stem;
    }
    let rest = stem[digits..].trim_start_matches(['.', '-', '_', ' ']);
    if rest.is_empty() || rest.len() == stem.len() - digits { stem } else { rest }
}

/// The id a file's song has in queues: the same file on the same server, the same id.
pub fn song_id(server_id: &str, path: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in server_id.bytes().chain(std::iter::once(0)).chain(path.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("dav:{h:016x}")
}

/// The id a server has: stable for the same address and user, so adding it again replaces it.
pub fn server_id(url: &str, user: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.trim().trim_end_matches('/').bytes().chain(std::iter::once(0)).chain(user.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub fn is_dav(id: &str) -> bool {
    id.starts_with("dav:")
}

fn track_of(server_id: &str, root: &str, f: &File) -> Track {
    let (title, artist, album) = meta_from_path(&f.path, root);
    Track {
        id: song_id(server_id, &f.path),
        title,
        artists: if artist.is_empty() { Vec::new() } else { vec![Link { name: artist, id: None }] },
        album: (!album.is_empty()).then(|| Link { name: album, id: None }),
        duration: None,
        thumbs: Vec::new(),
        track_nr: None,
        plays: None,
        music_video: false,
    }
}

// ------------------------------------------------------------------ addresses

/// The address of `path` on `server`, with the sign-in in it (reqwest sends it as Basic
/// authentication and keeps it out of the request line). Each part of the path is encoded.
pub fn file_url(server: &Server, password: &str, path: &str) -> anyhow::Result<String> {
    let mut url = url::Url::parse(&server.url)?;
    anyhow::ensure!(matches!(url.scheme(), "http" | "https") && url.host_str().is_some(), "that is not an http(s) address");
    {
        let mut segments = url.path_segments_mut().map_err(|_| anyhow::anyhow!("that address can't hold a path"))?;
        segments.clear();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            segments.push(part);
        }
    }
    url.set_query(None);
    url.set_fragment(None);
    if !server.user.is_empty() {
        let _ = url.set_username(&server.user);
        let _ = url.set_password(Some(password));
    }
    Ok(url.into())
}

/// The path of the folder a server's address names.
pub fn root_path(server: &Server) -> anyhow::Result<String> {
    let url = url::Url::parse(&server.url)?;
    Ok(decode(url.path()))
}

// ------------------------------------------------------------------ talking to a server

/// The entries directly in `dir` (a path on the server).
pub async fn list(http: &reqwest::Client, server: &Server, password: &str, dir: &str) -> anyhow::Result<Vec<Entry>> {
    let mut url = file_url(server, password, dir)?;
    // A folder's address ends with a slash.
    if !url.contains('?') && !url.ends_with('/') {
        url.push('/');
    }
    let method = reqwest::Method::from_bytes(b"PROPFIND")?;
    let mut req = http.request(method, &url).header("Depth", "1").header("Content-Type", "application/xml").timeout(TIMEOUT).body(
        r#"<?xml version="1.0" encoding="utf-8"?><propfind xmlns="DAV:"><prop><resourcetype/><getcontentlength/></prop></propfind>"#,
    );
    if !server.user.is_empty() {
        req = req.basic_auth(&server.user, Some(password));
    }
    let resp = req.send().await.map_err(|e| anyhow::anyhow!("couldn't reach the server ({})", e.without_url()))?;
    let status = resp.status();
    anyhow::ensure!(status != reqwest::StatusCode::UNAUTHORIZED, "the user name or password was refused");
    anyhow::ensure!(status != reqwest::StatusCode::NOT_FOUND, "that folder is not on the server");
    anyhow::ensure!(status.as_u16() == 207 || status.is_success(), "the server answered {status}");
    let body = resp.text().await?;
    let entries = parse_multistatus(&body, dir);
    anyhow::ensure!(!(entries.is_empty() && !body.contains("multistatus")), "that address answered, but not as a WebDAV server");
    Ok(entries)
}

/// Every audio file below the server's folder.
pub async fn scan(http: &reqwest::Client, server: &Server, password: &str) -> anyhow::Result<Vec<File>> {
    let root = root_path(server)?;
    let mut files = Vec::new();
    let mut todo = vec![(root, 0usize)];
    let mut seen = HashSet::new();
    while let Some((dir, depth)) = todo.pop() {
        if !seen.insert(dir.trim_end_matches('/').to_owned()) {
            continue;
        }
        let entries = match list(http, server, password, &dir).await {
            Ok(e) => e,
            // The first folder failing is the answer; one below it failing is skipped.
            Err(e) if depth == 0 => return Err(e),
            Err(e) => {
                log::info!("webdav: skipping a folder ({e})");
                continue;
            }
        };
        for e in entries {
            if e.name.starts_with('.') {
                continue;
            }
            if e.is_dir {
                if depth < MAX_DEPTH {
                    todo.push((e.path, depth + 1));
                }
            } else if audio_hint(&e.name).is_some() {
                files.push(File { path: e.path, size: e.size });
                if files.len() >= MAX_FILES {
                    return Ok(files);
                }
            }
        }
    }
    files.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(files)
}

// ------------------------------------------------------------------ what is kept

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub servers: Vec<Stored>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub server: Server,
    pub files: Vec<File>,
}

struct State {
    lib: Library,
    file: PathBuf,
    data_dir: PathBuf,
}

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| RwLock::new(State { lib: Library::default(), file: PathBuf::new(), data_dir: PathBuf::new() }));

fn secret_name(server_id: &str) -> String {
    format!("webdav-{server_id}")
}

pub fn init(data_dir: &Path) {
    let file = data_dir.join("webdav.json");
    let lib = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    *STATE.write().unwrap_or_else(|e| e.into_inner()) = State { lib, file, data_dir: data_dir.to_path_buf() };
}

fn save(st: &State) {
    if st.file.as_os_str().is_empty() {
        return;
    }
    if let Ok(json) = serde_json::to_vec(&st.lib) {
        let _ = std::fs::write(&st.file, json);
    }
}

pub fn servers() -> Vec<(Server, usize)> {
    STATE.read().unwrap_or_else(|e| e.into_inner()).lib.servers.iter().map(|s| (s.server.clone(), s.files.len())).collect()
}

/// Remember a server and its password (a server with the same id is replaced).
pub fn add_server(server: Server, password: &str) -> anyhow::Result<()> {
    let mut st = STATE.write().unwrap_or_else(|e| e.into_inner());
    if !server.user.is_empty() {
        crate::vault::set(&secret_name(&server.id), &st.data_dir, password.as_bytes())?;
    }
    let files = st.lib.servers.iter().find(|s| s.server.id == server.id).map(|s| s.files.clone()).unwrap_or_default();
    st.lib.servers.retain(|s| s.server.id != server.id);
    st.lib.servers.push(Stored { server, files });
    save(&st);
    Ok(())
}

pub fn remove_server(id: &str) {
    let mut st = STATE.write().unwrap_or_else(|e| e.into_inner());
    st.lib.servers.retain(|s| s.server.id != id);
    crate::vault::delete(&secret_name(id), &st.data_dir);
    save(&st);
}

pub fn set_files(id: &str, files: Vec<File>) {
    let mut st = STATE.write().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = st.lib.servers.iter_mut().find(|s| s.server.id == id) {
        s.files = files;
    }
    save(&st);
}

/// A server with its password, for talking to it.
pub fn server_with_password(id: &str) -> Option<(Server, String)> {
    let st = STATE.read().unwrap_or_else(|e| e.into_inner());
    let server = st.lib.servers.iter().find(|s| s.server.id == id)?.server.clone();
    let password = crate::vault::get(&secret_name(id), &st.data_dir).and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
    Some((server, password))
}

/// A server's songs, by name.
pub fn tracks(id: &str) -> Vec<Track> {
    let st = STATE.read().unwrap_or_else(|e| e.into_inner());
    st.lib.servers.iter().find(|s| s.server.id == id).map(|s| {
        let root = root_path(&s.server).unwrap_or_default();
        s.files.iter().map(|f| track_of(id, &root, f)).collect()
    }).unwrap_or_default()
}

/// Where to stream a song from, when `id` is one of a server's: the address (with the
/// sign-in in it), the size and the decoder hint.
pub struct Remote {
    pub url: String,
    pub size: u64,
    pub hint: &'static str,
}

pub fn remote(id: &str) -> Option<Remote> {
    if !is_dav(id) {
        return None;
    }
    let st = STATE.read().unwrap_or_else(|e| e.into_inner());
    for s in &st.lib.servers {
        if let Some(f) = s.files.iter().find(|f| song_id(&s.server.id, &f.path) == id) {
            let password = crate::vault::get(&secret_name(&s.server.id), &st.data_dir).and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
            let url = file_url(&s.server, &password, &f.path).ok()?;
            return Some(Remote { url, size: f.size, hint: audio_hint(&f.path)? });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const APACHE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
 <D:response><D:href>/dav/Music/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
 <D:response><D:href>/dav/Music/Senja%20Kala/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
 <D:response><D:href>/dav/Music/Lagu%20Lama.flac</D:href><D:propstat><D:prop><D:resourcetype/><D:getcontentlength>12345</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>"#;

    const NEXTCLOUD: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:oc="http://owncloud.org/ns">
 <d:response><d:href>/remote.php/dav/files/ani/Music/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/files/ani/Music/Pulang.mp3</d:href><d:propstat><d:prop><d:resourcetype/><d:getcontentlength>4096</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat><d:propstat><d:prop><d:foo/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>
</d:multistatus>"#;

    const NO_PREFIX: &str = r#"<multistatus xmlns="DAV:"><response><href>http://nas.local:5005/music/a%26b/</href><propstat><prop><resourcetype><collection/></resourcetype></prop><status>HTTP/1.1 200 OK</status></propstat></response><response><href>http://nas.local:5005/music/a%26b/Song.ogg</href><propstat><prop><getcontentlength>77</getcontentlength></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;

    #[test]
    fn an_apache_answer_lists_a_folder_and_a_file_without_the_folder_asked() {
        let e = parse_multistatus(APACHE, "/dav/Music/");
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].name.as_str(), e[0].is_dir, e[0].path.as_str()), ("Senja Kala", true, "/dav/Music/Senja Kala/"));
        assert_eq!((e[1].name.as_str(), e[1].is_dir, e[1].size), ("Lagu Lama.flac", false, 12345));
    }

    #[test]
    fn another_prefix_and_a_prop_the_server_could_not_give_are_read_the_same() {
        let e = parse_multistatus(NEXTCLOUD, "/remote.php/dav/files/ani/Music");
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].name.as_str(), e[0].size, e[0].is_dir), ("Pulang.mp3", 4096, false));
    }

    #[test]
    fn no_prefix_and_whole_addresses_in_the_hrefs() {
        let e = parse_multistatus(NO_PREFIX, "/music/a&b/");
        assert_eq!(e.len(), 1, "the folder asked about is left out even though its href is a whole address");
        assert_eq!((e[0].name.as_str(), e[0].path.as_str(), e[0].size), ("Song.ogg", "/music/a&b/Song.ogg", 77));
    }

    #[test]
    fn something_that_is_not_xml_gives_nothing() {
        assert!(parse_multistatus("<html>not dav</html>", "/").is_empty());
        assert!(parse_multistatus("", "/").is_empty());
        assert!(parse_multistatus("<<<", "/").is_empty());
    }

    #[test]
    fn percent_escapes_decode_and_a_bad_one_stays() {
        assert_eq!(decode("a%20b%2Fc%C3%A9"), "a b/cé");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn audio_files_are_known_by_their_extension() {
        assert_eq!(audio_hint("a.FLAC"), Some("flac"));
        assert_eq!(audio_hint("a.mp3"), Some("mp3"));
        assert_eq!(audio_hint("a.m4a"), Some("m4a"));
        assert_eq!(audio_hint("a.ogg"), Some("ogg"));
        assert_eq!(audio_hint("cover.jpg"), None);
        assert_eq!(audio_hint("noextension"), None);
        assert_eq!(audio_hint("playlist.m3u"), None);
    }

    #[test]
    fn artist_dash_title_names_give_both() {
        assert_eq!(meta_from_path("/music/Hits/Senja Kala - Hujan di Kemang.mp3", "/music"), ("Hujan di Kemang".into(), "Senja Kala".into(), "Hits".into()));
        assert_eq!(meta_from_path("/music/Senja Kala - Pulang.mp3", "/music/"), ("Pulang".into(), "Senja Kala".into(), String::new()), "no folder, no album");
    }

    #[test]
    fn artist_album_title_folders_give_all_three_and_a_track_number_goes() {
        assert_eq!(meta_from_path("/music/Senja Kala/Musim/03 - Lampu Kota.flac", "/music"), ("Lampu Kota".into(), "Senja Kala".into(), "Musim".into()));
        assert_eq!(meta_from_path("/music/Senja Kala/Musim/04. Kopi Sore.mp3", "/music"), ("Kopi Sore".into(), "Senja Kala".into(), "Musim".into()));
        assert_eq!(meta_from_path("/music/A/B/101_Song.ogg", "/music").0, "Song");
        assert_eq!(meta_from_path("/music/Genre/Senja Kala/Musim/Lagu.flac", "/music"), ("Lagu".into(), "Senja Kala".into(), "Musim".into()), "the two folders nearest the file");
    }

    #[test]
    fn a_title_that_is_only_a_number_or_starts_with_one_that_is_part_of_it_stays() {
        assert_eq!(strip_track_number("1999"), "1999", "a year is not a track number");
        assert_eq!(strip_track_number("7"), "7");
        assert_eq!(strip_track_number("2 Become 1"), "Become 1", "a track number then a title");
        assert_eq!(strip_track_number("Song"), "Song");
    }

    #[test]
    fn a_file_on_the_top_level_has_no_artist_or_album() {
        assert_eq!(meta_from_path("/music/Pulang.flac", "/music"), ("Pulang".into(), String::new(), String::new()));
        assert_eq!(meta_from_path("/m/Rona Jingga/Pulang.flac", "/m"), ("Pulang".into(), "Rona Jingga".into(), String::new()), "one folder is the artist");
    }

    #[test]
    fn a_server_id_is_the_same_for_the_same_address_whatever_the_trailing_slash() {
        assert_eq!(server_id("https://nas/dav/", "ani"), server_id("https://nas/dav", "ani"));
        assert_eq!(server_id(" https://nas/dav ", "ani"), server_id("https://nas/dav", "ani"));
        assert_ne!(server_id("https://nas/dav", "ani"), server_id("https://nas/dav", "budi"));
        assert_ne!(server_id("https://nas/dav", "ani"), server_id("https://nas/other", "ani"));
    }

    #[test]
    fn a_song_id_is_stable_and_differs_by_server_and_path() {
        assert_eq!(song_id("s1", "/a.flac"), song_id("s1", "/a.flac"));
        assert_ne!(song_id("s1", "/a.flac"), song_id("s2", "/a.flac"));
        assert_ne!(song_id("s1", "/a.flac"), song_id("s1", "/b.flac"));
        assert!(is_dav(&song_id("s1", "/a")) && !is_dav("local:00") && !is_dav("dQw4w9WgXcQ"));
    }

    fn server(url: &str, user: &str) -> Server {
        Server { id: "s1".into(), name: "NAS".into(), url: url.into(), user: user.into() }
    }

    #[test]
    fn a_file_address_encodes_each_part_and_carries_the_sign_in() {
        let u = file_url(&server("https://nas.local:5006/dav/Music/", "ani"), "p@ss:w/rd", "/dav/Music/Senja Kala/Lagu #1 & ?.flac").unwrap();
        assert!(u.starts_with("https://ani:"), "{u}");
        assert!(u.contains("@nas.local:5006/dav/Music/Senja%20Kala/Lagu%20%231%20&%20%3F.flac"), "{u}");
        let parsed = url::Url::parse(&u).unwrap();
        assert_eq!(parsed.password(), Some("p%40ss%3Aw%2Frd"), "the password is escaped inside the address");
        assert_eq!(parsed.path(), "/dav/Music/Senja%20Kala/Lagu%20%231%20&%20%3F.flac");
    }

    #[test]
    fn without_a_user_the_address_has_no_sign_in() {
        let u = file_url(&server("http://192.168.1.20:8080/", ""), "ignored", "/a/b.mp3").unwrap();
        assert_eq!(u, "http://192.168.1.20:8080/a/b.mp3");
    }

    #[test]
    fn only_http_and_https_addresses_are_taken() {
        assert!(file_url(&server("ftp://nas/", ""), "", "/a.mp3").is_err());
        assert!(file_url(&server("file:///etc/", ""), "", "/passwd").is_err());
        assert!(file_url(&server("not an address", ""), "", "/a").is_err());
    }

    #[test]
    fn the_scan_starts_in_the_folder_the_address_names() {
        assert_eq!(root_path(&server("https://nas/dav/My%20Music/", "")).unwrap(), "/dav/My Music/");
    }

    /// A real WebDAV server end to end with Aoto's own pieces: the listing, the scan, a wrong
    /// password, and a song streamed with Range requests and decoded. Run the test server
    /// (tools/webdav_test_server.py) over a folder of FLAC files, then:
    /// YTM_WEBDAV_TEST="http://127.0.0.1:8801/music/|ani|secret|<rate>|<channels>|<seconds>" cargo test --lib live_webdav -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_webdav_lists_scans_and_streams() {
        use rodio::Source;
        let Ok(spec) = std::env::var("YTM_WEBDAV_TEST") else { return };
        let f: Vec<&str> = spec.split('|').collect();
        let (url, user, pass) = (f[0], f[1], f[2]);
        let (rate, channels, secs) = (f[3].parse::<u32>().unwrap(), f[4].parse::<u16>().unwrap(), f[5].parse::<f64>().unwrap());
        let http = reqwest::Client::new();
        let server = Server { id: "live".into(), name: "test".into(), url: url.into(), user: user.into() };

        let root = list(&http, &server, pass, &root_path(&server).unwrap()).await.expect("the listing");
        println!("root: {:?}", root.iter().map(|e| (e.name.as_str(), e.is_dir)).collect::<Vec<_>>());
        assert!(!root.is_empty());

        let wrong = list(&http, &server, "wrong", &root_path(&server).unwrap()).await;
        assert!(wrong.unwrap_err().to_string().contains("refused"), "a wrong password says so");
        let missing = list(&http, &server, pass, "/music/no/such/folder/").await;
        let missing = missing.unwrap_err().to_string();
        assert!(missing.contains("not on the server"), "{missing}");

        let files = scan(&http, &server, pass).await.expect("the scan");
        println!("scan: {} file(s): {:?}", files.len(), files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>());
        assert!(!files.is_empty());
        let file = files.iter().find(|f| f.path.contains("Hujan")).expect("the song in the folders below");
        let hint = audio_hint(&file.path).expect("an audio file");
        let address = file_url(&server, pass, &file.path).unwrap();
        let shared = crate::stream::Shared::new(file.size, 0, None, crate::stream::Format::File { hint, mime: Some(mime_of(hint).to_owned()) }, None, false);
        crate::stream::download(http.clone(), address, "Aoto".into(), shared.clone(), None, crate::stream::Ranging::Header).await;
        assert!(shared.error().is_none(), "{:?}", shared.error());
        assert!(shared.is_complete());
        let decoder = crate::stream::decoder(shared.clone(), true).expect("Aoto's decoder opens it");
        assert_eq!((decoder.sample_rate().get(), decoder.channels().get()), (rate, channels));
        let n = decoder.count();
        let seconds = n as f64 / (rate as f64 * channels as f64);
        println!("streamed and decoded {seconds:.3} s");
        assert!((seconds - secs).abs() < 0.05, "{seconds} vs {secs}");
        let mut again = crate::stream::decoder(shared, true).unwrap();
        again.try_seek(Duration::from_secs_f64(secs / 2.0)).expect("it seeks");
        let left = again.count() as f64 / (rate as f64 * channels as f64);
        assert!((left - secs / 2.0).abs() < 0.2, "{left} s left after seeking to the middle");
    }
}
