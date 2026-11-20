//! Android Auto: what a car's screen browses and plays. The car talks to the Java side
//! (`AutoBrowserService`), which has no songs of its own: the app publishes here what
//! there is to browse (the queue, the downloads, the library's playlists, quick picks),
//! as lines the service reads, and takes from here what the car asked to play, by a tap
//! or by voice. Nothing is published unless a car is connected.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::model::{Card, Target, Track};

const FIELD: char = '\u{1f}';

/// The tree's nodes, by the names the service asks for them with.
pub const ROOT: &str = "root";
pub const QUEUE: &str = "queue";
pub const DOWNLOADS: &str = "downloads";
pub const PLAYLISTS: &str = "playlists";
pub const PICKS: &str = "picks";

/// The liked songs, as a playlist.
const LIKED: &str = "LM";

/// How much of a list a car is given: the queue from the song playing on, and the
/// newest downloads. A car's screen is no place to scroll through a thousand rows.
pub const QUEUE_SHOWN: usize = 100;
pub const DOWNLOADS_SHOWN: usize = 300;

/// What a tap on a row in the car asks to play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaId {
    /// The queue's song at this place (and which song that was when the car was told).
    Queue(usize, String),
    /// A downloaded song, and the downloads after it.
    Download(String),
    /// A playlist of the library, or the liked songs, from its start.
    Playlist(String),
    /// One of home's quick picks, and the picks after it.
    Pick(String),
}

impl MediaId {
    pub fn encode(&self) -> String {
        match self {
            MediaId::Queue(at, song) => format!("queue/{at}/{song}"),
            MediaId::Download(song) => format!("download/{song}"),
            MediaId::Playlist(id) => format!("playlist/{id}"),
            MediaId::Pick(song) => format!("pick/{song}"),
        }
    }

    pub fn parse(id: &str) -> Option<MediaId> {
        let (kind, rest) = id.split_once('/').filter(|(_, rest)| !rest.is_empty())?;
        Some(match kind {
            "queue" => {
                let (at, song) = rest.split_once('/').filter(|(_, song)| !song.is_empty())?;
                MediaId::Queue(at.parse().ok()?, song.to_owned())
            }
            "download" => MediaId::Download(rest.to_owned()),
            "playlist" => MediaId::Playlist(rest.to_owned()),
            "pick" => MediaId::Pick(rest.to_owned()),
            _ => return None,
        })
    }
}

/// A row in the car: something to play, or a list to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// What the car sends back: a [`MediaId`] to play, or the name of a node to open.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub playable: bool,
}

/// What the app has to show a car.
#[derive(Clone, Copy, Default)]
pub struct Source<'a> {
    pub queue: &'a [Track],
    /// The queue's song that plays (or is loaded).
    pub index: Option<usize>,
    /// The downloaded songs, newest first.
    pub downloads: &'a [Track],
    /// The library's playlists.
    pub playlists: &'a [Card],
    pub signed_in: bool,
    /// Home's quick picks.
    pub picks: &'a [Track],
}

/// The rows under `node`. Only what is really there: a list with nothing in it is not
/// offered at the root. `None`: no such node.
pub fn children(node: &str, s: &Source) -> Option<Vec<Item>> {
    let song = |id: MediaId, t: &Track| Item { id: id.encode(), title: t.title.clone(), subtitle: t.artist_line(), playable: true };
    // Up next is the song that plays and what follows it.
    let from = s.index.filter(|&i| i < s.queue.len());
    let upcoming = from.map_or(&[][..], |i| &s.queue[i..]);
    Some(match node {
        ROOT => {
            let list = |id: &str, title: &str, n: usize, one: &str| (n > 0).then(|| Item { id: id.to_owned(), title: title.to_owned(), subtitle: counted(n, one), playable: false });
            [
                list(QUEUE, "Up next", upcoming.len(), "song"),
                list(DOWNLOADS, "Downloads", s.downloads.len(), "song"),
                list(PLAYLISTS, "Playlists", playlists(s).len(), "playlist"),
                list(PICKS, "Quick picks", s.picks.len(), "song"),
            ]
            .into_iter()
            .flatten()
            .collect()
        }
        QUEUE => upcoming.iter().take(QUEUE_SHOWN).enumerate().map(|(k, t)| song(MediaId::Queue(from.unwrap_or(0) + k, t.id.clone()), t)).collect(),
        DOWNLOADS => s.downloads.iter().take(DOWNLOADS_SHOWN).map(|t| song(MediaId::Download(t.id.clone()), t)).collect(),
        PLAYLISTS => playlists(s),
        PICKS => s.picks.iter().map(|t| song(MediaId::Pick(t.id.clone()), t)).collect(),
        _ => return None,
    })
}

/// "1 song", "12 songs".
fn counted(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

/// The playlists a tap plays whole: the liked songs first (an account always has
/// them), then the library's.
fn playlists(s: &Source) -> Vec<Item> {
    let listed = |id: &str| s.playlists.iter().any(|c| matches!(&c.target, Target::Playlist(p) if p == id));
    let liked = (s.signed_in && !listed(LIKED)).then(|| Item { id: MediaId::Playlist(LIKED.to_owned()).encode(), title: "Liked songs".to_owned(), subtitle: String::new(), playable: true });
    let rest = s.playlists.iter().filter_map(|c| match &c.target {
        Target::Playlist(id) => Some(Item { id: MediaId::Playlist(id.clone()).encode(), title: c.title.clone(), subtitle: c.subtitle.clone(), playable: true }),
        _ => None,
    });
    liked.into_iter().chain(rest).collect()
}

/// Rows as the service reads them: one per line, its id, title, subtitle and whether
/// it plays ("p") or opens ("b"), joined by U+001F.
pub fn encode(items: &[Item]) -> String {
    let clean = |t: &str| t.replace([FIELD, '\n', '\r'], " ");
    let row = |i: &Item| [clean(&i.id), clean(&i.title), clean(&i.subtitle), if i.playable { "p" } else { "b" }.to_owned()].join(&FIELD.to_string());
    items.iter().map(row).collect::<Vec<_>>().join("\n")
}

/// Where in `queue` the car's tap on place `at` (then holding `song`) lands now: that
/// place if the song is still there, else the nearest place that holds it. `None`:
/// the song left the queue.
pub fn queue_place(queue: &[Track], at: usize, song: &str) -> Option<usize> {
    if queue.get(at).is_some_and(|t| t.id == song) {
        return Some(at);
    }
    (0..queue.len()).filter(|&i| queue[i].id == song).min_by_key(|i| i.abs_diff(at))
}

#[derive(Default)]
struct Published {
    /// Each node's rows, and how many times they have changed.
    nodes: BTreeMap<&'static str, (u64, String)>,
}

static WANTED: AtomicBool = AtomicBool::new(false);
static PUBLISHED: Mutex<Option<Published>> = Mutex::new(None);
static PLAYS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn with_published<R>(f: impl FnOnce(&mut Published) -> R) -> R {
    f(PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Published::default))
}

/// The service: a car connected, or the last one left. With none, what was published
/// is forgotten (the next car starts from a fresh tree).
pub fn set_wanted(on: bool) {
    WANTED.store(on, Ordering::SeqCst);
    if !on {
        with_published(|p| p.nodes.clear());
    }
    crate::platform::wake_ui();
}

/// Is a car connected? Only then is the tree kept up to date.
pub fn wanted() -> bool {
    WANTED.load(Ordering::SeqCst)
}

/// The app: this is what there is to browse now. A node whose rows changed counts on.
pub fn publish(s: &Source) {
    with_published(|p| {
        for node in [ROOT, QUEUE, DOWNLOADS, PLAYLISTS, PICKS] {
            let rows = encode(&children(node, s).unwrap_or_default());
            match p.nodes.get_mut(node) {
                Some((_, same)) if *same == rows => {}
                Some((count, old)) => {
                    *count += 1;
                    *old = rows;
                }
                None => {
                    p.nodes.insert(node, (1, rows));
                }
            }
        }
    });
}

/// The service polls this: every node with its count, "node␟count" a line. Empty until
/// the app has published: the service waits for it.
pub fn versions() -> String {
    with_published(|p| p.nodes.iter().map(|(node, (count, _))| format!("{node}{FIELD}{count}")).collect::<Vec<_>>().join("\n"))
}

/// The service: the rows of `node` as last published (see [`encode`]).
pub fn rows(node: &str) -> String {
    with_published(|p| p.nodes.get(node).map(|(_, rows)| rows.clone()).unwrap_or_default())
}

/// The service: the car asked to play this.
pub fn push_play(id: &str) {
    PLAYS.lock().unwrap_or_else(|e| e.into_inner()).push(id.to_owned());
    crate::platform::wake_ui();
}

/// What a voice command with no words in it ("play music") starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anything {
    /// The song that is loaded, from where it is.
    Resume,
    /// The downloads: they need no connection.
    Downloads,
    /// Home's quick picks.
    Picks,
    Nothing,
}

/// What to play when asked for anything: what was playing, else what is on the phone,
/// else what home suggests.
pub fn anything(loaded: bool, downloads: usize, picks: usize) -> Anything {
    if loaded {
        Anything::Resume
    } else if downloads > 0 {
        Anything::Downloads
    } else if picks > 0 {
        Anything::Picks
    } else {
        Anything::Nothing
    }
}

static SEARCHES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The service: the listener asked by voice for `query` (empty: for anything).
pub fn push_search(query: &str) {
    SEARCHES.lock().unwrap_or_else(|e| e.into_inner()).push(query.to_owned());
    crate::platform::wake_ui();
}

/// What was asked for by voice since the last call, in order.
pub fn take_searches() -> Vec<String> {
    std::mem::take(&mut *SEARCHES.lock().unwrap_or_else(|e| e.into_inner()))
}

/// What the car asked to play since the last call, in order; what can't be read is dropped.
pub fn take_plays() -> Vec<MediaId> {
    std::mem::take(&mut *PLAYS.lock().unwrap_or_else(|e| e.into_inner())).iter().filter_map(|id| MediaId::parse(id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Link;

    /// The tests that publish share the published tree.
    static TREE: Mutex<()> = Mutex::new(());

    fn track(id: &str, title: &str, artist: &str) -> Track {
        Track { id: id.into(), title: title.into(), artists: vec![Link { name: artist.into(), id: None }], album: None, duration: None, thumbs: vec![], track_nr: None, plays: None, music_video: false }
    }

    fn songs(n: usize) -> Vec<Track> {
        (0..n).map(|i| track(&format!("v{i}"), &format!("Song {i}"), "Artist")).collect()
    }

    fn playlist(id: &str, title: &str) -> Card {
        Card { title: title.into(), subtitle: "Playlist \u{2022} 12 songs".into(), thumbs: vec![], target: Target::Playlist(id.into()), round: false, links: vec![] }
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn a_media_id_survives_the_trip_to_the_car_and_back() {
        for id in [MediaId::Queue(3, "dQw4w9WgXcQ".into()), MediaId::Download("local:00ff".into()), MediaId::Playlist("PL_a-b".into()), MediaId::Playlist("LM".into()), MediaId::Pick("a/b".into())] {
            assert_eq!(MediaId::parse(&id.encode()), Some(id.clone()), "{}", id.encode());
        }
        assert_eq!(MediaId::Queue(3, "abc".into()).encode(), "queue/3/abc", "AutoLogic.java reads the place out of it");
    }

    #[test]
    fn what_is_not_a_media_id_is_refused() {
        for id in ["", "queue", "queue/x/abc", "queue/3", "queue/3/", "download/", "radio/abc", "root", "downloads"] {
            assert_eq!(MediaId::parse(id), None, "{id:?}");
        }
    }

    #[test]
    fn the_root_offers_only_the_lists_that_have_something() {
        let (queue, downloads, picks) = (songs(3), songs(2), songs(4));
        let playlists = [playlist("PL1", "Road trip")];
        let full = Source { queue: &queue, index: Some(0), downloads: &downloads, playlists: &playlists, signed_in: true, picks: &picks };
        let root = children(ROOT, &full).unwrap();
        assert_eq!(ids(&root), [QUEUE, DOWNLOADS, PLAYLISTS, PICKS]);
        assert!(root.iter().all(|i| !i.playable), "they open, they don't play");
        assert_eq!(root[0].title, "Up next");
        assert_eq!(root[1].title, "Downloads");
        assert_eq!(root[1].subtitle, "2 songs");

        assert_eq!(children(ROOT, &Source::default()).unwrap(), [], "nothing to show is an empty tree, not empty lists");
        let only_downloads = Source { downloads: &downloads[..1], ..Source::default() };
        let root = children(ROOT, &only_downloads).unwrap();
        assert_eq!(ids(&root), [DOWNLOADS]);
        assert_eq!(root[0].subtitle, "1 song");
    }

    #[test]
    fn a_queue_with_no_song_loaded_is_not_up_next() {
        let queue = songs(3);
        let s = Source { queue: &queue, index: None, ..Source::default() };
        assert_eq!(children(ROOT, &s).unwrap(), []);
        assert_eq!(children(QUEUE, &s).unwrap(), []);
    }

    #[test]
    fn up_next_runs_from_the_song_playing() {
        let queue = songs(5);
        let s = Source { queue: &queue, index: Some(2), ..Source::default() };
        let rows = children(QUEUE, &s).unwrap();
        assert_eq!(ids(&rows), ["queue/2/v2", "queue/3/v3", "queue/4/v4"]);
        assert!(rows.iter().all(|r| r.playable));
        assert_eq!((rows[0].title.as_str(), rows[0].subtitle.as_str()), ("Song 2", "Artist"));
    }

    #[test]
    fn a_long_queue_is_cut_for_the_car() {
        let queue = songs(1000);
        let s = Source { queue: &queue, index: Some(400), ..Source::default() };
        let rows = children(QUEUE, &s).unwrap();
        assert_eq!(rows.len(), QUEUE_SHOWN);
        assert_eq!(rows[0].id, "queue/400/v400");
        let root = children(ROOT, &s).unwrap();
        assert_eq!(root[0].subtitle, "600 songs", "the root still says how many are to come");
    }

    #[test]
    fn downloads_play_and_the_newest_come_first_as_given() {
        let downloads = songs(3);
        let s = Source { downloads: &downloads, ..Source::default() };
        let rows = children(DOWNLOADS, &s).unwrap();
        assert_eq!(ids(&rows), ["download/v0", "download/v1", "download/v2"]);
        assert!(rows.iter().all(|r| r.playable));
        let many = songs(DOWNLOADS_SHOWN + 50);
        assert_eq!(children(DOWNLOADS, &Source { downloads: &many, ..Source::default() }).unwrap().len(), DOWNLOADS_SHOWN);
    }

    #[test]
    fn playlists_start_with_the_liked_songs_when_signed_in() {
        let playlists = [playlist("PL1", "Road trip"), playlist("PL2", "Focus")];
        let s = Source { playlists: &playlists, signed_in: true, ..Source::default() };
        let rows = children(PLAYLISTS, &s).unwrap();
        assert_eq!(ids(&rows), ["playlist/LM", "playlist/PL1", "playlist/PL2"]);
        assert_eq!(rows[0].title, "Liked songs");
        assert!(rows.iter().all(|r| r.playable), "a tap plays the playlist");
        assert_eq!(rows[1].subtitle, "Playlist \u{2022} 12 songs");
    }

    #[test]
    fn the_liked_songs_are_not_listed_twice() {
        let playlists = [playlist("PL1", "Road trip"), playlist("LM", "Liked Music")];
        let s = Source { playlists: &playlists, signed_in: true, ..Source::default() };
        assert_eq!(ids(&children(PLAYLISTS, &s).unwrap()), ["playlist/PL1", "playlist/LM"]);
    }

    #[test]
    fn signed_out_there_are_no_liked_songs_and_only_playlists_are_listed() {
        let album = Card { target: Target::Album("MPRE1".into()), ..playlist("x", "An album") };
        let cards = [album, playlist("PL1", "Road trip")];
        let s = Source { playlists: &cards, signed_in: false, ..Source::default() };
        assert_eq!(ids(&children(PLAYLISTS, &s).unwrap()), ["playlist/PL1"]);
        assert_eq!(children(ROOT, &Source::default()).unwrap(), []);
    }

    #[test]
    fn quick_picks_play() {
        let picks = songs(2);
        let s = Source { picks: &picks, ..Source::default() };
        assert_eq!(ids(&children(PICKS, &s).unwrap()), ["pick/v0", "pick/v1"]);
        assert_eq!(children(ROOT, &s).unwrap()[0].title, "Quick picks");
    }

    #[test]
    fn a_node_that_does_not_exist_has_no_rows() {
        assert_eq!(children("albums", &Source::default()), None);
        assert_eq!(children("queue/0/v0", &Source::default()), None, "a song is not a list");
    }

    #[test]
    fn rows_cross_to_the_service_one_a_line() {
        let items = [
            Item { id: "queue/0/v0".into(), title: "Tabun".into(), subtitle: "YOASOBI".into(), playable: true },
            Item { id: QUEUE.into(), title: "Up next".into(), subtitle: String::new(), playable: false },
        ];
        assert_eq!(encode(&items), "queue/0/v0\u{1f}Tabun\u{1f}YOASOBI\u{1f}p\nqueue\u{1f}Up next\u{1f}\u{1f}b");
        assert_eq!(encode(&[]), "");
    }

    #[test]
    fn a_title_cannot_break_a_row() {
        let items = [Item { id: "pick/v0".into(), title: "a\u{1f}b\nc".into(), subtitle: "d\re".into(), playable: true }];
        let line = encode(&items);
        assert_eq!(line.lines().count(), 1);
        assert_eq!(line.split('\u{1f}').count(), 4);
    }

    #[test]
    fn a_tap_on_a_queue_row_finds_its_song_where_it_is_now() {
        let queue = songs(5);
        assert_eq!(queue_place(&queue, 3, "v3"), Some(3));
        // The queue moved on since the car was told: the song is elsewhere.
        assert_eq!(queue_place(&queue, 1, "v3"), Some(3));
        assert_eq!(queue_place(&queue, 9, "v4"), Some(4));
        assert_eq!(queue_place(&queue, 2, "gone"), None);
        // The same song twice: the place nearest the one tapped.
        let twice = [queue[0].clone(), queue[1].clone(), queue[0].clone(), queue[2].clone(), queue[0].clone()];
        assert_eq!(queue_place(&twice, 3, "v0"), Some(2));
        assert_eq!(queue_place(&twice, 4, "v0"), Some(4));
    }

    #[test]
    fn nothing_is_published_until_the_app_does_and_then_every_node_is() {
        let _one = TREE.lock().unwrap_or_else(|e| e.into_inner());
        set_wanted(false);
        assert_eq!(versions(), "", "the service waits for the app");
        set_wanted(true);
        assert!(wanted());
        assert_eq!(versions(), "");
        publish(&Source::default());
        let nodes: Vec<String> = versions().lines().map(|l| l.split('\u{1f}').next().unwrap().to_owned()).collect();
        assert_eq!(nodes, [DOWNLOADS, PICKS, PLAYLISTS, QUEUE, ROOT], "every node, so the service knows the tree is there");
        set_wanted(false);
    }

    #[test]
    fn only_a_node_whose_rows_changed_counts_on() {
        let _one = TREE.lock().unwrap_or_else(|e| e.into_inner());
        set_wanted(false);
        set_wanted(true);
        let count = |node: &str| -> u64 {
            versions().lines().find_map(|l| l.strip_prefix(&format!("{node}\u{1f}")).map(|v| v.parse().unwrap())).unwrap()
        };
        let queue = songs(3);
        let s = Source { queue: &queue, index: Some(0), ..Source::default() };
        publish(&s);
        let (q, d, r) = (count(QUEUE), count(DOWNLOADS), count(ROOT));
        publish(&s);
        assert_eq!((count(QUEUE), count(DOWNLOADS), count(ROOT)), (q, d, r), "the same again changes nothing");
        // The next song plays: up next is a row shorter, and the root says so.
        publish(&Source { index: Some(1), ..s });
        assert_eq!(count(QUEUE), q + 1);
        assert_eq!(count(ROOT), r + 1);
        assert_eq!(count(DOWNLOADS), d);
        assert_eq!(rows(QUEUE).lines().count(), 2);
        assert!(rows(QUEUE).starts_with("queue/1/v1\u{1f}"));
        assert_eq!(rows("nowhere"), "");
        set_wanted(false);
    }

    #[test]
    fn when_the_car_leaves_the_tree_is_forgotten() {
        let _one = TREE.lock().unwrap_or_else(|e| e.into_inner());
        set_wanted(true);
        publish(&Source::default());
        assert_ne!(versions(), "");
        set_wanted(false);
        assert!(!wanted());
        assert_eq!(versions(), "");
    }

    #[test]
    fn asked_for_anything_the_loaded_song_goes_on() {
        assert_eq!(anything(true, 12, 8), Anything::Resume);
        assert_eq!(anything(true, 0, 0), Anything::Resume);
    }

    #[test]
    fn with_nothing_loaded_anything_is_the_downloads_then_the_quick_picks() {
        assert_eq!(anything(false, 12, 8), Anything::Downloads, "what needs no connection first");
        assert_eq!(anything(false, 0, 8), Anything::Picks);
        assert_eq!(anything(false, 0, 0), Anything::Nothing);
    }

    #[test]
    fn what_is_asked_for_by_voice_is_taken_once_in_order() {
        assert!(take_searches().is_empty());
        push_search("yoasobi tabun");
        push_search("");
        assert_eq!(take_searches(), ["yoasobi tabun", ""]);
        assert!(take_searches().is_empty());
    }

    #[test]
    fn what_the_car_asks_to_play_is_taken_once_in_order() {
        assert!(take_plays().is_empty());
        push_play("download/v1");
        push_play("nonsense");
        push_play("queue/2/v2");
        assert_eq!(take_plays(), [MediaId::Download("v1".into()), MediaId::Queue(2, "v2".into())]);
        assert!(take_plays().is_empty());
    }
}
