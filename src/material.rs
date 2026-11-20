//! Material 3 ("Material You") tokens: the colour roles, the type scale and the shape
//! scale. The UI asks for a colour by its role (`scheme().primary`), for text by its
//! role in the type scale (`Type::TitleLarge`) and for a corner by what it rounds
//! (`shape::CARD`); nothing outside this file names a Material colour by its value.

use egui::{Color32, FontId};

// ------------------------------------------------------------------ colour roles

/// One Material 3 colour scheme: every role, by name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scheme {
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub secondary: Color32,
    pub on_secondary: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub tertiary: Color32,
    pub on_tertiary: Color32,
    pub tertiary_container: Color32,
    pub on_tertiary_container: Color32,
    pub surface: Color32,
    pub surface_container_lowest: Color32,
    pub surface_container_low: Color32,
    pub surface_container: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    /// An action that destroys something (delete a playlist), and what is written on it.
    pub error: Color32,
    pub on_error: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Scheme {
    /// The dark scheme, from the seed indigo `#2F3E9E` (藍), Material 3's tonal-spot tones.
    pub const DARK: Scheme = Scheme {
        primary: rgb(0xBFC4EE),
        on_primary: rgb(0x292E50),
        primary_container: rgb(0x3F4468),
        on_primary_container: rgb(0xDFE0FF),
        secondary: rgb(0xC4C5DD),
        on_secondary: rgb(0x2D2F42),
        secondary_container: rgb(0x434559),
        on_secondary_container: rgb(0xE0E1F9),
        tertiary: rgb(0xDDBBE8),
        on_tertiary: rgb(0x40274A),
        tertiary_container: rgb(0x573D62),
        on_tertiary_container: rgb(0xF7D8FF),
        surface: rgb(0x131317),
        surface_container_lowest: rgb(0x0E0E12),
        surface_container_low: rgb(0x1B1B20),
        surface_container: rgb(0x1F1F24),
        surface_container_high: rgb(0x2A2A2E),
        surface_container_highest: rgb(0x343439),
        on_surface: rgb(0xE4E1E7),
        on_surface_variant: rgb(0xC7C5D1),
        outline: rgb(0x90909A),
        outline_variant: rgb(0x46464F),
        error: rgb(0xFFB4AB),
        on_error: rgb(0x690005),
    };

    /// The light scheme, from the same seed.
    pub const LIGHT: Scheme = Scheme {
        primary: rgb(0x545B8B),
        on_primary: rgb(0xFFFFFF),
        primary_container: rgb(0xDFE0FF),
        on_primary_container: rgb(0x101744),
        secondary: rgb(0x5B5D72),
        on_secondary: rgb(0xFFFFFF),
        secondary_container: rgb(0xE0E1F9),
        on_secondary_container: rgb(0x181A2C),
        tertiary: rgb(0x70547B),
        on_tertiary: rgb(0xFFFFFF),
        tertiary_container: rgb(0xF7D8FF),
        on_tertiary_container: rgb(0x291134),
        surface: rgb(0xFBF8FE),
        surface_container_lowest: rgb(0xFFFFFF),
        surface_container_low: rgb(0xF5F2F9),
        surface_container: rgb(0xF0EDF3),
        surface_container_high: rgb(0xEAE7ED),
        surface_container_highest: rgb(0xE4E1E7),
        on_surface: rgb(0x1B1B20),
        on_surface_variant: rgb(0x46464F),
        outline: rgb(0x767680),
        outline_variant: rgb(0xC7C5D1),
        error: rgb(0xBA1A1A),
        on_error: rgb(0xFFFFFF),
    };

    /// What one of the app's older colour names (`theme::text()` and the like) is in
    /// this scheme, so widgets written before the roles come out in them.
    pub fn tone(&self, tone: Tone) -> Color32 {
        match tone {
            Tone::Text => self.on_surface,
            Tone::TextDim => self.on_surface_variant,
            Tone::TextFaint => self.outline,
            Tone::OnText => self.surface,
            Tone::Background => self.surface,
            Tone::Menu => self.surface_container,
            Tone::Floating => self.surface_container_low,
        }
    }

    /// The older "glass" (white at `a` over the page) as a tonal surface: the content
    /// colour at `a`, which over `surface` gives the container ladder (4 % is
    /// surfaceContainerLow, 8 % surfaceContainer, 12 % High, 16 % Highest).
    pub fn glass(&self, a: f32) -> Color32 {
        self.on_surface.gamma_multiply(a.clamp(0.0, 1.0))
    }

    /// The older "panel" (a bar or drawer over the page at `a`): surfaceContainer.
    pub fn panel(&self, a: f32) -> Color32 {
        self.surface_container.gamma_multiply(a.clamp(0.0, 1.0))
    }
}

/// The colour names the app had before the roles (see `Scheme::tone`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Main text.
    Text,
    /// Secondary text.
    TextDim,
    /// Counts, hints, placeholders.
    TextFaint,
    /// Text and icons on a fill of the main text colour.
    OnText,
    /// The window's base colour.
    Background,
    /// Menus and popups.
    Menu,
    /// Dialogs, sheets and cards that float over the page.
    Floating,
}

/// The scheme in use. A light scheme, or one made from the phone's wallpaper, would be
/// chosen here; everything that draws asks this.
pub fn scheme() -> &'static Scheme {
    scheme_for(crate::theme::light())
}

/// The scheme for light pages or dark ones.
pub fn scheme_for(light: bool) -> &'static Scheme {
    if light { &Scheme::LIGHT } else { &Scheme::DARK }
}

/// Is the look being drawn Material You? Then the older colour helpers answer with
/// roles, and the pages draw their Material layouts.
pub fn on() -> bool {
    crate::preset::active().material
}

/// The two ends of the gradient behind Now Playing, top then bottom, for a song whose
/// artwork is mostly `song`: that colour's hue as a dark tone (the designs' is the
/// seed terracotta's, `#5A2A1D`), running down into the surface.
pub fn song_backdrop(song: Color32) -> (Color32, Color32) {
    song_backdrop_for(song, crate::theme::light())
}

/// [`song_backdrop`] for light pages or dark ones. On light pages the top is a pale tint
/// of the song's colour instead of a dark tone of it.
pub fn song_backdrop_for(song: Color32, light: bool) -> (Color32, Color32) {
    if light {
        let [r, g, b] = [song.r(), song.g(), song.b()].map(|c| c as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let pure = if max > min { [r, g, b].map(|c| (c - min) / (max - min)) } else { [1.0; 3] };
        // A seventh of the way from white to the hue: a grey cover leaves it white-grey.
        let strength = if max > 0.0 { ((max - min) / max).min(1.0) * 0.15 } else { 0.0 };
        let top = pure.map(|c| ((1.0 - strength * (1.0 - c)) * 0.97 * 255.0).round() as u8);
        return (Color32::from_rgb(top[0], top[1], top[2]), Scheme::LIGHT.surface);
    }
    let [r, g, b] = [song.r(), song.g(), song.b()].map(|c| c as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    // The colour with its brightest channel at one: its hue at its own saturation.
    let pure = if max > 0.0 { [r / max, g / max, b / max] } else { [1.0; 3] };
    // A grey cover stays grey, and a little darker: there is no colour to carry it.
    let s = if max > 0.0 { ((max - min) / max).min(0.68) } else { 0.0 };
    let value = egui::lerp(0.24..=0.35, s / 0.68);
    let own = if max > min { (max - min) / max } else { 1.0 };
    let tone = pure.map(|c| (1.0 - (1.0 - c) * s / own) * value);
    // Yellows and greens are brighter than a red at the same strength: none is let be
    // brighter than the designs' tone, so the text over it reads the same.
    let k = (0.215 / (0.299 * tone[0] + 0.587 * tone[1] + 0.114 * tone[2])).min(1.0);
    let top = tone.map(|c| (c * k * 255.0).round() as u8);
    (Color32::from_rgb(top[0], top[1], top[2]), Scheme::DARK.surface)
}

// ------------------------------------------------------------------ type scale

/// The three weights the app has faces for. Material's Medium is drawn with the
/// app's middle face (Roboto Medium on Android).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    Bold,
}

/// A role in Material 3's type scale. The whole scale is here; a screen takes the
/// roles it needs.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    DisplaySmall,
    HeadlineLarge,
    HeadlineMedium,
    HeadlineSmall,
    TitleLarge,
    TitleMedium,
    TitleSmall,
    BodyLarge,
    BodyMedium,
    BodySmall,
    LabelLarge,
    LabelMedium,
    LabelSmall,
}

/// How a role is set: its size, the height of its line, the space added between its
/// letters (all in points) and its weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub size: f32,
    pub line_height: f32,
    pub tracking: f32,
    pub weight: Weight,
}

impl Type {
    #[cfg(test)]
    const ALL: [Type; 13] = [
        Type::DisplaySmall,
        Type::HeadlineLarge,
        Type::HeadlineMedium,
        Type::HeadlineSmall,
        Type::TitleLarge,
        Type::TitleMedium,
        Type::TitleSmall,
        Type::BodyLarge,
        Type::BodyMedium,
        Type::BodySmall,
        Type::LabelLarge,
        Type::LabelMedium,
        Type::LabelSmall,
    ];

    pub fn spec(self) -> Spec {
        use Weight::{Medium, Regular};
        let (size, line_height, tracking, weight) = match self {
            Type::DisplaySmall => (36.0, 44.0, 0.0, Regular),
            Type::HeadlineLarge => (32.0, 40.0, 0.0, Regular),
            Type::HeadlineMedium => (28.0, 36.0, 0.0, Regular),
            Type::HeadlineSmall => (24.0, 32.0, 0.0, Regular),
            Type::TitleLarge => (22.0, 28.0, 0.0, Regular),
            Type::TitleMedium => (16.0, 24.0, 0.15, Medium),
            Type::TitleSmall => (14.0, 20.0, 0.1, Medium),
            Type::BodyLarge => (16.0, 24.0, 0.5, Regular),
            Type::BodyMedium => (14.0, 20.0, 0.25, Regular),
            Type::BodySmall => (12.0, 16.0, 0.4, Regular),
            Type::LabelLarge => (14.0, 20.0, 0.1, Medium),
            Type::LabelMedium => (12.0, 16.0, 0.5, Medium),
            Type::LabelSmall => (11.0, 16.0, 0.5, Medium),
        };
        Spec { size, line_height, tracking, weight }
    }

    /// The role's font, for painters that take only a font (its line height and
    /// tracking come with `Spec::format`).
    pub fn font(self) -> FontId {
        self.spec().font()
    }
}

impl Spec {
    /// The same role at another weight (an emphasised title, a selected label).
    pub fn weight(self, weight: Weight) -> Spec {
        Spec { weight, ..self }
    }

    pub fn font(&self) -> FontId {
        match self.weight {
            Weight::Regular => crate::theme::regular(self.size),
            Weight::Medium => crate::theme::semibold(self.size),
            Weight::Bold => crate::theme::bold(self.size),
        }
    }

    /// The role as a text format in `color`: font, line height and tracking.
    pub fn format(&self, color: Color32) -> egui::TextFormat {
        egui::TextFormat { font_id: self.font(), color, extra_letter_spacing: self.tracking, line_height: Some(self.line_height), ..Default::default() }
    }
}

// ------------------------------------------------------------------ shape scale

/// Corner radii, by what they round.
pub mod shape {
    /// Chips.
    pub const CHIP: f32 = 8.0;
    /// Small tiles and the covers in list rows.
    pub const TILE_SMALL: f32 = 12.0;
    /// Tiles.
    pub const TILE: f32 = 16.0;
    /// Small cards, the mini player.
    pub const CARD_SMALL: f32 = 20.0;
    /// Cards and sheets.
    pub const CARD: f32 = 28.0;
    /// The big artwork of Now Playing and a collection's header.
    pub const ARTWORK: f32 = 36.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Color32 {
        crate::preset::hex(s).expect("a #rrggbb colour")
    }

    #[test]
    fn the_dark_scheme_has_the_designs_colour_for_every_role() {
        let s = Scheme::DARK;
        let roles = [
            (s.primary, "#BFC4EE"),
            (s.on_primary, "#292E50"),
            (s.primary_container, "#3F4468"),
            (s.on_primary_container, "#DFE0FF"),
            (s.secondary, "#C4C5DD"),
            (s.on_secondary, "#2D2F42"),
            (s.secondary_container, "#434559"),
            (s.on_secondary_container, "#E0E1F9"),
            (s.tertiary, "#DDBBE8"),
            (s.on_tertiary, "#40274A"),
            (s.tertiary_container, "#573D62"),
            (s.on_tertiary_container, "#F7D8FF"),
            (s.surface, "#131317"),
            (s.surface_container_lowest, "#0E0E12"),
            (s.surface_container_low, "#1B1B20"),
            (s.surface_container, "#1F1F24"),
            (s.surface_container_high, "#2A2A2E"),
            (s.surface_container_highest, "#343439"),
            (s.on_surface, "#E4E1E7"),
            (s.on_surface_variant, "#C7C5D1"),
            (s.outline, "#90909A"),
            (s.outline_variant, "#46464F"),
            (s.error, "#FFB4AB"),
            (s.on_error, "#690005"),
        ];
        for (color, want) in roles {
            assert_eq!(color, hex(want), "{want}");
        }
    }

    #[test]
    fn light_pages_take_the_light_scheme_and_dark_ones_the_dark() {
        assert_eq!(*scheme_for(false), Scheme::DARK);
        assert_eq!(*scheme_for(true), Scheme::LIGHT);
    }

    #[test]
    fn the_light_scheme_reads_dark_on_light_in_every_pair() {
        let s = Scheme::LIGHT;
        let luma = |c: Color32| 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
        assert!(luma(s.surface) > 240.0, "a light page");
        for (on, under, what) in [
            (s.on_surface, s.surface, "text on the page"),
            (s.on_surface_variant, s.surface_container, "dim text on a card"),
            (s.on_primary, s.primary, "on a filled button"),
            (s.on_primary_container, s.primary_container, "on a tonal card"),
            (s.on_secondary_container, s.secondary_container, "on a chosen chip"),
            (s.on_tertiary_container, s.tertiary_container, "on a tertiary tile"),
            (s.on_error, s.error, "on an error"),
        ] {
            assert!((luma(on) - luma(under)).abs() > 110.0, "{what}: {on:?} on {under:?}");
        }
        assert!(luma(s.surface_container_highest) < luma(s.surface_container_low), "containers darken as they rise");
    }

    #[test]
    fn the_older_colour_names_are_roles() {
        let s = Scheme::DARK;
        assert_eq!(s.tone(Tone::Text), s.on_surface);
        assert_eq!(s.tone(Tone::TextDim), s.on_surface_variant);
        assert_eq!(s.tone(Tone::TextFaint), s.outline);
        assert_eq!(s.tone(Tone::OnText), s.surface);
        assert_eq!(s.tone(Tone::Background), s.surface);
        assert_eq!(s.tone(Tone::Menu), s.surface_container);
        assert_eq!(s.tone(Tone::Floating), s.surface_container_low);
    }

    /// How far apart two colours are in brightness, as WCAG counts contrast (1..21).
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let ch = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        };
        let (hi, lo) = (lum(a).max(lum(b)), lum(a).min(lum(b)));
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn a_songs_backdrop_is_its_colour_as_a_dark_tone_over_the_surface() {
        let s = Scheme::DARK;
        // The designs' own: the seed terracotta gives their top colour.
        let (top, bottom) = song_backdrop_for(hex("#B33B15"), false);
        assert_eq!(bottom, s.surface);
        for (got, want) in [(top.r(), 0x5A), (top.g(), 0x2A), (top.b(), 0x1D)] {
            assert!((got as i32 - want).abs() <= 14, "{top:?} is not near #5A2A1D");
        }
        for song in ["#B33B15", "#FFE14D", "#2F6BFF", "#38D07A", "#FFFFFF", "#101010", "#808080", "#FF2D55"] {
            let (top, _) = song_backdrop_for(hex(song), false);
            assert!(contrast(top, s.on_surface) >= 7.0, "{song}: text on {top:?} reads at {}", contrast(top, s.on_surface));
            assert!(contrast(top, s.primary) >= 4.5, "{song}: the primary colour on {top:?}");
        }
        // A cover's hue is kept: a blue song has a blue backdrop, a grey one a grey.
        let (blue, _) = song_backdrop_for(hex("#2F6BFF"), false);
        assert!(blue.b() > blue.r() && blue.b() > blue.g());
        let (grey, _) = song_backdrop_for(hex("#808080"), false);
        assert!(grey.r().abs_diff(grey.g()) <= 1 && grey.g().abs_diff(grey.b()) <= 1);
    }

    #[test]
    fn on_light_pages_a_songs_backdrop_is_a_pale_tint_of_its_colour() {
        let s = Scheme::LIGHT;
        for song in ["#B33B15", "#FFE14D", "#2F6BFF", "#38D07A", "#FFFFFF", "#101010", "#808080", "#FF2D55"] {
            let (top, bottom) = song_backdrop_for(hex(song), true);
            assert_eq!(bottom, s.surface);
            assert!(contrast(top, s.on_surface) >= 7.0, "{song}: text on {top:?} reads at {}", contrast(top, s.on_surface));
            assert!(contrast(top, s.primary) >= 4.5, "{song}: the primary colour on {top:?} reads at {}", contrast(top, s.primary));
        }
        let (blue, _) = song_backdrop_for(hex("#2F6BFF"), true);
        assert!(blue.b() > blue.r() && blue.b() > blue.g(), "a blue song tints it blue: {blue:?}");
        assert!(blue.r() < 235, "and it is a tint, not white: {blue:?}");
        let (grey, _) = song_backdrop_for(hex("#808080"), true);
        assert!(grey.r().abs_diff(grey.g()) <= 1 && grey.g().abs_diff(grey.b()) <= 1);
        assert_eq!(song_backdrop_for(hex("#2F6BFF"), false), {
            let dark = Scheme::DARK;
            (song_backdrop_for(hex("#2F6BFF"), false).0, dark.surface)
        });
    }

    /// `over` painted on `under`, as the screen shows it.
    fn blend(over: Color32, under: Color32) -> [i32; 3] {
        let a = over.a() as f32 / 255.0;
        let mix = |o: u8, u: u8| (o as f32 + u as f32 * (1.0 - a)).round() as i32;
        [mix(over.r(), under.r()), mix(over.g(), under.g()), mix(over.b(), under.b())]
    }

    #[test]
    fn glass_over_the_surface_climbs_the_container_ladder() {
        let s = Scheme::DARK;
        for (a, container) in [(0.04, s.surface_container_low), (0.08, s.surface_container), (0.12, s.surface_container_high), (0.16, s.surface_container_highest)] {
            let got = blend(s.glass(a), s.surface);
            let want = [container.r() as i32, container.g() as i32, container.b() as i32];
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() <= 6, "glass({a}) is {got:?}, the container {want:?}");
            }
        }
        assert_eq!(s.glass(0.0), Color32::TRANSPARENT);
    }

    #[test]
    fn a_panel_is_the_surface_container_at_its_opacity() {
        let s = Scheme::DARK;
        assert_eq!(s.panel(1.0), s.surface_container);
        assert_eq!(s.panel(0.0), Color32::TRANSPARENT);
        assert!(s.panel(0.5).a() > 100 && s.panel(0.5).a() < 150);
    }

    #[test]
    fn the_type_scale_is_material_3s() {
        let spec = |t: Type| {
            let s = t.spec();
            (s.size, s.line_height, s.weight)
        };
        assert_eq!(spec(Type::HeadlineMedium), (28.0, 36.0, Weight::Regular));
        assert_eq!(spec(Type::TitleLarge), (22.0, 28.0, Weight::Regular));
        assert_eq!(spec(Type::TitleMedium), (16.0, 24.0, Weight::Medium));
        assert_eq!(spec(Type::BodyLarge), (16.0, 24.0, Weight::Regular));
        assert_eq!(spec(Type::LabelLarge), (14.0, 20.0, Weight::Medium));
    }

    #[test]
    fn every_role_has_a_line_taller_than_its_letters_and_a_size_that_goes_down_the_scale() {
        for t in Type::ALL {
            let s = t.spec();
            assert!(s.line_height > s.size, "{t:?}");
            assert!((0.0..=0.5).contains(&s.tracking), "{t:?}");
        }
        let sizes: Vec<f32> = [Type::DisplaySmall, Type::HeadlineLarge, Type::HeadlineMedium, Type::HeadlineSmall, Type::TitleLarge, Type::TitleMedium, Type::TitleSmall].map(|t| t.spec().size).to_vec();
        assert!(sizes.windows(2).all(|w| w[0] > w[1]), "{sizes:?}");
    }

    #[test]
    fn a_format_carries_the_roles_size_line_and_tracking() {
        let f = Type::TitleMedium.spec().format(Color32::WHITE);
        assert_eq!(f.font_id.size, 16.0);
        assert_eq!(f.line_height, Some(24.0));
        assert_eq!(f.extra_letter_spacing, 0.15);
        assert_eq!(f.color, Color32::WHITE);
    }

    #[test]
    fn a_role_can_be_set_bolder_and_keeps_its_size() {
        let s = Type::LabelMedium.spec().weight(Weight::Bold);
        assert_eq!((s.size, s.line_height, s.weight), (12.0, 16.0, Weight::Bold));
        assert_ne!(s.font(), Type::LabelMedium.font());
    }

    #[test]
    fn the_shape_scale_grows_from_chips_to_artwork() {
        let scale = [shape::CHIP, shape::TILE_SMALL, shape::TILE, shape::CARD_SMALL, shape::CARD, shape::ARTWORK];
        assert_eq!(scale, [8.0, 12.0, 16.0, 20.0, 28.0, 36.0]);
    }
}
