//! When an addon's lossless copy of a song may play. A FLAC file is several times the
//! size of YouTube's stream, so the listener can keep it to Wi-Fi, or turn the addons
//! off for the moment without removing them. The default is the way it always was:
//! whenever an addon has the song.

use crate::platform::Network;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lossless {
    /// Wi-Fi and mobile data alike (how the app behaved before the setting).
    #[default]
    Always,
    /// Only on an unmetered connection; on mobile data YouTube plays.
    WifiOnly,
    /// Never: YouTube plays, the addons stay in the list.
    Off,
}

impl Lossless {
    pub const ALL: [Lossless; 3] = [Lossless::Always, Lossless::WifiOnly, Lossless::Off];

    pub fn label(self) -> &'static str {
        match self {
            Lossless::Always => "Always",
            Lossless::WifiOnly => "Wi-Fi only",
            Lossless::Off => "Off",
        }
    }

    /// The stored form. A name rather than a number, so a later reordering can't change
    /// what a saved choice means.
    pub fn code(self) -> &'static str {
        match self {
            Lossless::Always => "always",
            Lossless::WifiOnly => "wifi",
            Lossless::Off => "off",
        }
    }

    /// What was stored; anything unknown (or nothing) is the default, so a damaged or
    /// newer settings file can't turn the addons off by accident.
    pub fn from_code(code: &str) -> Lossless {
        Lossless::ALL.into_iter().find(|l| l.code() == code.trim()).unwrap_or_default()
    }

    /// May an addon's copy play on this connection?
    pub fn allows(self, network: Network) -> bool {
        match self {
            Lossless::Always => true,
            Lossless::WifiOnly => network == Network::Unmetered,
            Lossless::Off => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_always_until_the_listener_says_otherwise() {
        assert_eq!(Lossless::default(), Lossless::Always);
        for network in [Network::Offline, Network::Metered, Network::Unmetered] {
            assert!(Lossless::default().allows(network));
        }
    }

    #[test]
    fn wifi_only_keeps_lossless_off_mobile_data() {
        assert!(Lossless::WifiOnly.allows(Network::Unmetered));
        assert!(!Lossless::WifiOnly.allows(Network::Metered));
        assert!(!Lossless::WifiOnly.allows(Network::Offline));
    }

    #[test]
    fn off_never_asks_the_addons() {
        for network in [Network::Offline, Network::Metered, Network::Unmetered] {
            assert!(!Lossless::Off.allows(network));
        }
    }

    #[test]
    fn a_stored_choice_comes_back_as_it_was() {
        for l in Lossless::ALL {
            assert_eq!(Lossless::from_code(l.code()), l);
        }
    }

    #[test]
    fn nothing_or_nonsense_stored_means_always() {
        assert_eq!(Lossless::from_code(""), Lossless::Always);
        assert_eq!(Lossless::from_code("2"), Lossless::Always);
        assert_eq!(Lossless::from_code("WIFI"), Lossless::Always, "the codes are lower case");
    }

    #[test]
    fn the_labels_are_distinct_and_are_what_the_setting_shows() {
        let labels = Lossless::ALL.map(Lossless::label);
        assert_eq!(labels, ["Always", "Wi-Fi only", "Off"]);
    }
}
