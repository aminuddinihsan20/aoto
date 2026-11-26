//! The icons the app can wear on the home screen. Each is an entry of the Android
//! manifest (`.Launcher<Code>`, see `IconLogic.java`); Settings › Appearance picks one.

/// One icon: what it is called, and the colours its picture in Settings is drawn with.
#[derive(Debug, PartialEq)]
pub struct Icon {
    /// What the setting stores and the activity is told; the manifest entry is named after it.
    pub code: &'static str,
    pub name: &'static str,
    /// The ground, the drop and the wave, as 0xRRGGBB.
    pub ground: u32,
    pub drop: u32,
    pub wave: u32,
    /// Drawn the Sticker way: the drop outlined, with a second one behind it.
    pub lined: bool,
}

/// Every icon, the first being the one a fresh install wears. Keep in step with
/// `IconLogic.ICONS` and the manifest's entries.
pub const ALL: [Icon; 5] = [
    Icon { code: "indigo", name: "Indigo", ground: 0x2F3E9E, drop: 0xDEE0FF, wave: 0x2F3E9E, lined: false },
    Icon { code: "sticker", name: "Sticker", ground: 0xFCE7A2, drop: 0xFFFFFF, wave: 0x33314A, lined: true },
    Icon { code: "sky", name: "Sky", ground: 0xBDE3F6, drop: 0xFFFFFF, wave: 0x33314A, lined: true },
    Icon { code: "rose", name: "Rose", ground: 0xF6B8BE, drop: 0xFFFFFF, wave: 0x33314A, lined: true },
    Icon { code: "midnight", name: "Midnight", ground: 0x141414, drop: 0xF5F2EB, wave: 0x141414, lined: false },
];

/// The icon stored as `code`; anything unknown (or nothing) is the first, so a damaged
/// setting can't leave the app without an icon.
pub fn from_code(code: &str) -> &'static Icon {
    ALL.iter().find(|i| i.code == code).unwrap_or(&ALL[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_code_is_the_first_icon() {
        assert_eq!(from_code("sky").name, "Sky");
        assert_eq!(from_code(""), &ALL[0]);
        assert_eq!(from_code("neon"), &ALL[0]);
    }

    #[test]
    fn codes_are_distinct_plain_words_the_manifest_can_name() {
        for (n, icon) in ALL.iter().enumerate() {
            assert!(!icon.code.is_empty() && icon.code.chars().all(|c| c.is_ascii_lowercase()), "{}", icon.code);
            assert!(ALL.iter().skip(n + 1).all(|other| other.code != icon.code), "{} twice", icon.code);
        }
    }

    /// The manifest and the Java side name the same icons, in the same order.
    #[test]
    fn the_android_side_knows_the_same_icons() {
        let java = include_str!("../android/app/src/main/java/io/github/aminuddinihsan20/aoto/IconLogic.java");
        let manifest = include_str!("../android/app/src/main/AndroidManifest.xml");
        let list: Vec<String> = ALL.iter().map(|i| format!("\"{}\"", i.code)).collect();
        assert!(java.contains(&format!("{{{}}}", list.join(", "))), "IconLogic.ICONS differs");
        for icon in &ALL {
            let mut name = icon.code.to_owned();
            name[..1].make_ascii_uppercase();
            assert!(manifest.contains(&format!("android:name=\".Launcher{name}\"")), "no manifest entry for {}", icon.code);
        }
        assert_eq!(manifest.matches("<activity-alias").count(), ALL.len());
        // GameActivity reads the library's name from the entry it was opened through:
        // one without it would not start.
        assert_eq!(manifest.matches("android:name=\"android.app.lib_name\"").count(), ALL.len() + 1);
    }
}
