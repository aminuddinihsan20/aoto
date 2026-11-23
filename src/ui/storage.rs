//! Folders the listener chooses on Android: where downloads are copied to, and where
//! music is read from. Asking Android for a folder (or to list one) takes time and
//! waits on a person, so each runs on a thread of its own and leaves its answer here
//! for the next frame (`App::apply_storage`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::App;
use crate::download_queue::Ask;
use crate::platform::LOGIN_CANCELLED;

/// What a chosen folder is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Every download is copied into it.
    Downloads,
    /// Music is read from it.
    Music,
}

/// A folder's address and name, as the activity reports a choice.
type Choice = Result<(String, String), String>;

/// What the threads leave for the UI.
#[derive(Default)]
pub struct Answers {
    /// A folder was chosen (address, name), or why not.
    folders: Vec<(Purpose, Choice)>,
    /// A folder's music was listed: how many songs, or why not.
    scans: Vec<(String, Result<usize, String>)>,
    /// Folders being listed now.
    scanning: Vec<String>,
}

pub type Shared = Arc<Mutex<Answers>>;

/// "address\u{1f}name" as the activity answers a folder choice.
fn parse_choice(text: &str) -> Choice {
    text.split_once('\u{1f}').filter(|(uri, _)| !uri.is_empty()).map(|(u, l)| (u.to_owned(), l.to_owned())).ok_or_else(|| "no folder".to_owned())
}

impl App {
    /// Open Android's folder chooser; the answer comes back through `apply_storage`.
    pub(super) fn ask_folder(&self, purpose: Purpose) {
        let answers = self.storage.clone();
        std::thread::spawn(move || {
            let result = crate::platform::call("pick-folder", "", Duration::from_secs(10 * 60)).and_then(|t| parse_choice(&t));
            answers.lock().unwrap_or_else(|e| e.into_inner()).folders.push((purpose, result));
            crate::platform::wake_ui();
        });
    }

    /// List a folder's music again.
    pub(super) fn scan_folder(&self, uri: &str) {
        {
            let mut a = self.storage.lock().unwrap_or_else(|e| e.into_inner());
            if a.scanning.iter().any(|u| u == uri) {
                return;
            }
            a.scanning.push(uri.to_owned());
        }
        let (answers, uri) = (self.storage.clone(), uri.to_owned());
        std::thread::spawn(move || {
            let result = crate::local::rescan(&uri);
            let mut a = answers.lock().unwrap_or_else(|e| e.into_inner());
            a.scanning.retain(|u| *u != uri);
            a.scans.push((uri, result));
            crate::platform::wake_ui();
        });
    }

    pub(super) fn scanning(&self, uri: &str) -> bool {
        self.storage.lock().unwrap_or_else(|e| e.into_inner()).scanning.iter().any(|u| u == uri)
    }

    /// A download asked for with no download folder chosen yet: choose it first, once,
    /// then the download goes ahead.
    pub(super) fn download_after_folder(&mut self, ask: Ask) {
        self.pending_download = Some(ask);
        self.toast("Choose where downloads are saved: once, then they go there");
        self.ask_folder(Purpose::Downloads);
    }

    /// Take what the threads found: chosen folders (and the download held for one), and
    /// lists of music.
    pub(super) fn apply_storage(&mut self) {
        let (folders, scans) = {
            let mut a = self.storage.lock().unwrap_or_else(|e| e.into_inner());
            (std::mem::take(&mut a.folders), std::mem::take(&mut a.scans))
        };
        for (purpose, result) in folders {
            match (purpose, result) {
                (Purpose::Downloads, Ok((uri, label))) => {
                    crate::downloads::set_tree(&uri, &label);
                    self.toast(format!("Downloads are saved to {}", crate::local::folder_name(&label)));
                    if let Some(ask) = self.pending_download.take() {
                        self.queue_download(ask);
                    }
                }
                (Purpose::Downloads, Err(e)) => {
                    if self.pending_download.take().is_some() && e != LOGIN_CANCELLED {
                        self.toast(format!("Download cancelled: {e}"));
                    } else if e == LOGIN_CANCELLED {
                        self.toast("Download cancelled: no folder chosen");
                    }
                }
                (Purpose::Music, Ok((uri, label))) => {
                    if crate::local::add_folder(&uri, &label) {
                        self.toast(format!("Looking for music in {}", crate::local::folder_name(&label)));
                        self.scan_folder(&uri);
                    } else {
                        self.toast("That folder is already added");
                    }
                }
                (Purpose::Music, Err(e)) => {
                    if e != LOGIN_CANCELLED {
                        self.toast(format!("Couldn't add the folder: {e}"));
                    }
                }
            }
        }
        for (uri, result) in scans {
            let name = crate::local::name_of(&uri).unwrap_or_default();
            match result {
                Ok(0) => self.toast(format!("No music found in {name}")),
                Ok(n) => self.toast(format!("{name}: {n} song{}", if n == 1 { "" } else { "s" })),
                Err(e) => self.toast(format!("Couldn't read {name}: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_answer_has_an_address_and_a_name() {
        assert_eq!(parse_choice("content://t/jp\u{1f}primary:Music/Japanese"), Ok(("content://t/jp".to_owned(), "primary:Music/Japanese".to_owned())));
        assert!(parse_choice("").is_err());
        assert!(parse_choice("\u{1f}name").is_err(), "an empty address is no folder");
        assert!(parse_choice("no separator").is_err());
    }
}
