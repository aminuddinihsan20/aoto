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

    /// The dark scheme on a true black page, for OLED screens. The surfaces are the dark
    /// scheme's moved one step down, so a card still stands out from the page; the
    /// colours of text and accents are the same.
    pub const BLACK: Scheme = Scheme {
        surface: rgb(0x000000),
        surface_container_lowest: rgb(0x000000),
        surface_container_low: rgb(0x0F0F12),
        surface_container: rgb(0x17171B),
        surface_container_high: rgb(0x1F1F24),
        surface_container_highest: rgb(0x2A2A2E),
        ..Scheme::DARK
    };

    /// The Sticker look on light pages: a warm paper page, white cards, ink for text and
    /// lines. The pastel fills are the containers (what is pressed is butter yellow, what is
    /// chosen is sky blue); `primary` is the ink, so that accent text stays readable.
    pub const STICKER_LIGHT: Scheme = Scheme {
        primary: rgb(0x33314A),
        on_primary: rgb(0xFFFCF7),
        primary_container: rgb(0xFCE7A2),
        on_primary_container: rgb(0x33314A),
        secondary: rgb(0x5A5870),
        on_secondary: rgb(0xFFFFFF),
        secondary_container: rgb(0xBDE3F6),
        on_secondary_container: rgb(0x33314A),
        tertiary: rgb(0x7A5560),
        on_tertiary: rgb(0xFFFFFF),
        tertiary_container: rgb(0xF6B8BE),
        on_tertiary_container: rgb(0x33314A),
        surface: rgb(0xFFFCF7),
        surface_container_lowest: rgb(0xFFFFFF),
        surface_container_low: rgb(0xFFFFFF),
        surface_container: rgb(0xFFFFFF),
        surface_container_high: rgb(0xF6F2FA),
        surface_container_highest: rgb(0xEDE8F2),
        on_surface: rgb(0x33314A),
        on_surface_variant: rgb(0x6A6780),
        outline: rgb(0x8B889C),
        outline_variant: rgb(0xD9D4E3),
        error: rgb(0xB3261E),
        on_error: rgb(0xFFFFFF),
    };

    /// The Sticker look on dark pages: a neutral black (no blue in it), cream for text, a
    /// quieter cream for lines (`secondary`), and the pastels a step deeper than on light
    /// pages, where their full brightness would glare, with the same ink on them.
    pub const STICKER_DARK: Scheme = Scheme {
        primary: rgb(0xF5F2EB),
        on_primary: rgb(0x141414),
        primary_container: rgb(0xDFC577),
        on_primary_container: rgb(0x33314A),
        secondary: rgb(0xCFCBC2),
        on_secondary: rgb(0x141414),
        secondary_container: rgb(0x93C1DA),
        on_secondary_container: rgb(0x33314A),
        tertiary: rgb(0xDE9BA3),
        on_tertiary: rgb(0x33314A),
        tertiary_container: rgb(0xDE9BA3),
        on_tertiary_container: rgb(0x33314A),
        surface: rgb(0x141414),
        surface_container_lowest: rgb(0x0E0E0E),
        surface_container_low: rgb(0x1B1B1B),
        surface_container: rgb(0x212121),
        surface_container_high: rgb(0x2A2A2A),
        surface_container_highest: rgb(0x333333),
        on_surface: rgb(0xF5F2EB),
        on_surface_variant: rgb(0xA5A29B),
        outline: rgb(0x7E7B75),
        outline_variant: rgb(0x3A3A3A),
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
    pick(look(), light, PURE_BLACK.load(std::sync::atomic::Ordering::Relaxed))
}

/// Which of the app's looks is drawn (Settings › Appearance › Theme). Both use the
/// Material layouts; they differ in colours and in how a card or a button is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    /// Aoto's own: Material You in indigo.
    #[default]
    Indigo,
    /// Outlined cards with a second card behind them, in soft pastels.
    Sticker,
}

impl Look {
    pub const ALL: [Look; 2] = [Look::Indigo, Look::Sticker];

    pub fn label(self) -> &'static str {
        match self {
            Look::Indigo => "Indigo",
            Look::Sticker => "Sticker",
        }
    }

    /// The stored form: a name, so a later reordering can't change what a saved choice means.
    pub fn code(self) -> &'static str {
        match self {
            Look::Indigo => "indigo",
            Look::Sticker => "sticker",
        }
    }

    /// Anything unknown (or nothing) is Indigo: a damaged setting can't leave the app without a look.
    pub fn from_code(code: &str) -> Look {
        Look::ALL.into_iter().find(|l| l.code() == code.trim()).unwrap_or_default()
    }
}

static LOOK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Draw in `look` from now on. Returns whether it changed, so the caller knows to restyle.
pub fn set_look(look: Look) -> bool {
    let i = Look::ALL.iter().position(|l| *l == look).unwrap_or(0) as u8;
    LOOK.swap(i, std::sync::atomic::Ordering::Relaxed) != i
}

pub fn look() -> Look {
    Look::ALL.get(LOOK.load(std::sync::atomic::Ordering::Relaxed) as usize).copied().unwrap_or_default()
}

/// How the look draws its shapes, beyond its colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// The line around cards, buttons and fields; 0 for none.
    pub outline: f32,
    /// How far the second card behind a card is set off, right and down; 0 for none.
    pub stack: f32,
    /// The colour of those lines.
    pub line: Color32,
    /// The fill of what is pressed (a main button, the song that plays), and what is written on it.
    pub accent: Color32,
    pub on_accent: Color32,
    /// The most a corner is rounded; `None` leaves the shape scale as it is.
    pub corner: Option<f32>,
}

impl Style {
    /// The style of `look` drawn with `scheme`.
    pub fn of(look: Look, scheme: &Scheme) -> Style {
        match look {
            Look::Indigo => Style { outline: 0.0, stack: 0.0, line: scheme.outline_variant, accent: scheme.primary, on_accent: scheme.on_primary, corner: None },
            Look::Sticker => {
                // In the dark the lines are a step quieter than the text (the scheme's
                // secondary), so a page full of outlined cards does not glare; in the
                // light they are the ink itself.
                let dark = scheme.surface.r() < 0x80;
                let line = if dark { scheme.secondary } else { scheme.on_surface };
                Style { outline: 1.5, stack: 4.0, line, accent: scheme.primary_container, on_accent: scheme.on_primary_container, corner: Some(12.0) }
            }
        }
    }

    /// Is this the look with outlines and stacked cards?
    pub fn outlined(&self) -> bool {
        self.outline > 0.0
    }

    /// A corner of the shape scale as this look rounds it.
    pub fn round(&self, corner: f32) -> f32 {
        self.corner.map_or(corner, |most| corner.min(most))
    }
}

/// The style in use (see [`Style`]).
pub fn style() -> Style {
    Style::of(look(), scheme())
}

/// Settings › Appearance › Pure black: dark pages on a true black background.
static PURE_BLACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Draw dark pages on true black (or back on the dark grey). Returns whether it changed,
/// so the caller knows to restyle.
pub fn set_pure_black(on: bool) -> bool {
    PURE_BLACK.swap(on, std::sync::atomic::Ordering::Relaxed) != on
}

/// The scheme for light or dark pages; `black`: dark pages are pure black. Light pages
/// are not affected.
fn pick(look: Look, light: bool, black: bool) -> &'static Scheme {
    match (look, light, black) {
        // Sticker's dark page is black already: Pure black changes nothing there.
        (Look::Sticker, true, _) => &Scheme::STICKER_LIGHT,
        (Look::Sticker, false, _) => &Scheme::STICKER_DARK,
        (Look::Indigo, true, _) => &Scheme::LIGHT,
        (Look::Indigo, false, true) => &Scheme::BLACK,
        (Look::Indigo, false, false) => &Scheme::DARK,
    }
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
    // Sticker keeps its own page behind the player: its colours are the look, not the song's.
    if look() == Look::Sticker {
        let page = scheme_for(light).surface;
        return (page, page);
    }
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
    (Color32::from_rgb(top[0], top[1], top[2]), scheme_for(false).surface)
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
    fn pure_black_changes_only_dark_pages() {
        assert_eq!(*pick(Look::Indigo, false, false), Scheme::DARK);
        assert_eq!(*pick(Look::Indigo, false, true), Scheme::BLACK);
        assert_eq!(*pick(Look::Indigo, true, true), Scheme::LIGHT, "light pages are left as they are");
        assert_eq!(*pick(Look::Indigo, true, false), Scheme::LIGHT);
    }

    #[test]
    fn the_black_scheme_has_a_black_page_and_surfaces_that_still_step_up_from_it() {
        let b = Scheme::BLACK;
        assert_eq!(b.surface, Color32::BLACK);
        let steps = [b.surface_container_lowest, b.surface_container_low, b.surface_container, b.surface_container_high, b.surface_container_highest];
        let luma = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        for pair in steps.windows(2) {
            assert!(luma(pair[0]) <= luma(pair[1]), "{pair:?}");
        }
        assert!(luma(b.surface_container_low) > luma(b.surface), "a card stands out from the page");
    }

    #[test]
    fn the_black_scheme_keeps_the_dark_schemes_text_and_accents() {
        let (b, d) = (Scheme::BLACK, Scheme::DARK);
        assert_eq!((b.primary, b.on_primary, b.on_surface, b.on_surface_variant, b.outline, b.error), (d.primary, d.on_primary, d.on_surface, d.on_surface_variant, d.outline, d.error));
    }

    #[test]
    fn the_look_picks_the_scheme_and_indigo_is_as_it_was() {
        assert_eq!(*pick(Look::Indigo, true, false), Scheme::LIGHT);
        assert_eq!(*pick(Look::Indigo, false, false), Scheme::DARK);
        assert_eq!(*pick(Look::Indigo, false, true), Scheme::BLACK);
        assert_eq!(*pick(Look::Sticker, true, false), Scheme::STICKER_LIGHT);
        assert_eq!(*pick(Look::Sticker, false, false), Scheme::STICKER_DARK);
        assert_eq!(*pick(Look::Sticker, false, true), Scheme::STICKER_DARK, "its dark page is black already");
    }

    #[test]
    fn a_stored_look_comes_back_and_nonsense_is_indigo() {
        for l in Look::ALL {
            assert_eq!(Look::from_code(l.code()), l);
        }
        assert_eq!(Look::from_code(""), Look::Indigo);
        assert_eq!(Look::from_code("neon"), Look::Indigo);
        assert_eq!(Look::ALL.map(Look::label), ["Indigo", "Sticker"]);
    }

    fn ratio(a: Color32, b: Color32) -> f32 {
        let lin = |c: u8| { let v = c as f32 / 255.0; if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } };
        let l = |c: Color32| 0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b());
        let (x, y) = (l(a), l(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_sticker_text_reads_on_what_it_is_written_on() {
        for (s, name) in [(Scheme::STICKER_LIGHT, "light"), (Scheme::STICKER_DARK, "dark")] {
            for (on, under, what) in [
                (s.on_surface, s.surface, "text on the page"),
                (s.on_surface, s.surface_container, "text on a card"),
                (s.on_surface_variant, s.surface, "dim text on the page"),
                (s.on_surface_variant, s.surface_container, "dim text on a card"),
                (s.primary, s.surface, "accent text on the page"),
                (s.on_primary, s.primary, "on a filled button"),
                (s.on_primary_container, s.primary_container, "on butter yellow"),
                (s.on_secondary_container, s.secondary_container, "on sky blue"),
                (s.on_tertiary_container, s.tertiary_container, "on rose"),
                (s.on_error, s.error, "on an error"),
            ] {
                assert!(ratio(on, under) >= 4.5, "{name}, {what}: {:.2}", ratio(on, under));
            }
        }
    }

    #[test]
    fn the_sticker_dark_page_is_a_neutral_black() {
        let s = Scheme::STICKER_DARK;
        for c in [s.surface, s.surface_container_low, s.surface_container, s.surface_container_high, s.surface_container_highest] {
            assert!(c.r() == c.g() && c.g() == c.b(), "{c:?} has a tint");
        }
    }

    #[test]
    fn indigo_draws_no_outlines_and_sticker_draws_them_stacked() {
        let i = Style::of(Look::Indigo, &Scheme::DARK);
        assert!(!i.outlined() && i.stack == 0.0);
        assert_eq!((i.accent, i.on_accent), (Scheme::DARK.primary, Scheme::DARK.on_primary), "Indigo's buttons are as they were");
        assert_eq!(i.round(shape::CARD), shape::CARD, "and so are its corners");
        let s = Style::of(Look::Sticker, &Scheme::STICKER_LIGHT);
        assert!(s.outlined() && s.stack > 0.0);
        assert_eq!(s.accent, Scheme::STICKER_LIGHT.primary_container, "what is pressed is butter yellow");
        assert_eq!(s.line, Scheme::STICKER_LIGHT.on_surface);
        assert!(s.round(shape::CARD) <= 12.0 && s.round(shape::CHIP) == shape::CHIP, "big corners come down, small ones stay");
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
