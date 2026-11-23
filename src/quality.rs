//! The quality of the stream YouTube plays, chosen separately for Wi-Fi and for mobile
//! data (Settings › Sound & quality). It only decides which of YouTube's audio formats is
//! asked for; downloads always take the best, and addons (see `lossless`) have their own
//! rule. Both default to High, which is how the app always played.

use crate::platform::Network;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    /// HE-AAC, about 48 kbps: the least data.
    Low,
    /// AAC, 128 kbps.
    Normal,
    /// The best the account gets: Opus (about 160 kbps), or AAC 256 kbps with Premium.
    #[default]
    High,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Low, Quality::Normal, Quality::High];

    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Normal => "Normal",
            Quality::High => "High",
        }
    }

    /// The stored form: a name, so a later reordering can't change what a saved choice means.
    pub fn code(self) -> &'static str {
        match self {
            Quality::Low => "low",
            Quality::Normal => "normal",
            Quality::High => "high",
        }
    }

    /// Anything unknown (or nothing) is High, so a damaged settings file can't lower the sound.
    pub fn from_code(code: &str) -> Quality {
        Quality::ALL.into_iter().find(|q| q.code() == code.trim()).unwrap_or_default()
    }

    /// YouTube's audio formats (itags) to take, best match first, for a song played at this
    /// quality. `opus`: the caller can play Opus (playback can; a saved song is AAC).
    /// 251 is Opus (about 160 kbps), 140 is AAC at 128 kbps, 139 is HE-AAC at about 48.
    pub fn itags(self, opus: bool) -> &'static [u64] {
        match (self, opus) {
            (Quality::Low, _) => &[139, 140],
            (Quality::Normal, _) => &[140, 139],
            (Quality::High, true) => &[251, 140, 139],
            (Quality::High, false) => &[140, 139],
        }
    }

    /// Does this quality take the Premium account's 256 kbps stream when there is one?
    pub fn takes_premium_stream(self) -> bool {
        self == Quality::High
    }
}

/// The two choices, one for each kind of connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ceilings {
    pub wifi: Quality,
    pub mobile: Quality,
}

impl Ceilings {
    /// The quality for the connection the phone has right now. Offline there is nothing to
    /// stream; a saved song plays from its file, so the Wi-Fi choice stands in.
    pub fn for_network(self, network: Network) -> Quality {
        match network {
            Network::Metered => self.mobile,
            Network::Unmetered | Network::Offline => self.wifi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_high_everywhere_until_the_listener_says_otherwise() {
        assert_eq!(Quality::default(), Quality::High);
        assert_eq!(Ceilings::default(), Ceilings { wifi: Quality::High, mobile: Quality::High });
    }

    #[test]
    fn high_is_the_order_the_app_always_asked_in() {
        assert_eq!(Quality::High.itags(true), [251, 140, 139], "playback: Opus, then AAC");
        assert_eq!(Quality::High.itags(false), [140, 139], "a saved song is AAC");
    }

    #[test]
    fn normal_is_aac_128_and_low_is_the_smallest_stream() {
        assert_eq!(Quality::Normal.itags(true), [140, 139]);
        assert_eq!(Quality::Low.itags(true), [139, 140]);
    }

    #[test]
    fn a_lower_quality_never_takes_opus() {
        for q in [Quality::Low, Quality::Normal] {
            assert!(!q.itags(true).contains(&251), "{q:?}");
        }
    }

    #[test]
    fn every_quality_has_a_fallback_so_a_song_without_its_first_choice_still_plays() {
        for q in Quality::ALL {
            for opus in [true, false] {
                assert!(q.itags(opus).len() >= 2, "{q:?} opus={opus}");
            }
        }
    }

    #[test]
    fn only_high_takes_the_premium_stream() {
        assert!(Quality::High.takes_premium_stream());
        assert!(!Quality::Normal.takes_premium_stream());
        assert!(!Quality::Low.takes_premium_stream());
    }

    #[test]
    fn mobile_data_and_wifi_have_their_own_choice() {
        let c = Ceilings { wifi: Quality::High, mobile: Quality::Low };
        assert_eq!(c.for_network(Network::Unmetered), Quality::High);
        assert_eq!(c.for_network(Network::Metered), Quality::Low);
        assert_eq!(c.for_network(Network::Offline), Quality::High, "offline: the Wi-Fi choice");
    }

    #[test]
    fn a_stored_choice_comes_back_as_it_was_and_nonsense_is_high() {
        for q in Quality::ALL {
            assert_eq!(Quality::from_code(q.code()), q);
        }
        assert_eq!(Quality::from_code(""), Quality::High);
        assert_eq!(Quality::from_code("ultra"), Quality::High);
        assert_eq!(Quality::ALL.map(Quality::label), ["Low", "Normal", "High"]);
    }
}
