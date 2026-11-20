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
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Scheme {
    /// The dark scheme of the designs, from the seed terracotta `#B33B15`.
    pub const DARK: Scheme = Scheme {
        primary: rgb(0xFFB5A0),
        on_primary: rgb(0x561F0F),
        primary_container: rgb(0x723523),
        on_primary_container: rgb(0xFFDBD1),
        secondary: rgb(0xE7BDB2),
        on_secondary: rgb(0x442A22),
        secondary_container: rgb(0x5D4037),
        on_secondary_container: rgb(0xFFDBD1),
        tertiary: rgb(0xD8C58D),
        on_tertiary: rgb(0x3B2F05),
        tertiary_container: rgb(0x534619),
        on_tertiary_container: rgb(0xF5E1A7),
        surface: rgb(0x1A110F),
        surface_container_lowest: rgb(0x140C0A),
        surface_container_low: rgb(0x231917),
        surface_container: rgb(0x271D1B),
        surface_container_high: rgb(0x322825),
        surface_container_highest: rgb(0x3D3230),
        on_surface: rgb(0xF1DFDA),
        on_surface_variant: rgb(0xD8C2BC),
        outline: rgb(0xA08C87),
        outline_variant: rgb(0x53433F),
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
    &Scheme::DARK
}

/// Is the look being drawn Material You? Then the older colour helpers answer with
/// roles, and the pages draw their Material layouts.
pub fn on() -> bool {
    crate::preset::active().material
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
    #[allow(dead_code)]
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
            (s.primary, "#FFB5A0"),
            (s.on_primary, "#561F0F"),
            (s.primary_container, "#723523"),
            (s.on_primary_container, "#FFDBD1"),
            (s.secondary, "#E7BDB2"),
            (s.on_secondary, "#442A22"),
            (s.secondary_container, "#5D4037"),
            (s.on_secondary_container, "#FFDBD1"),
            (s.tertiary, "#D8C58D"),
            (s.on_tertiary, "#3B2F05"),
            (s.tertiary_container, "#534619"),
            (s.on_tertiary_container, "#F5E1A7"),
            (s.surface, "#1A110F"),
            (s.surface_container_lowest, "#140C0A"),
            (s.surface_container_low, "#231917"),
            (s.surface_container, "#271D1B"),
            (s.surface_container_high, "#322825"),
            (s.surface_container_highest, "#3D3230"),
            (s.on_surface, "#F1DFDA"),
            (s.on_surface_variant, "#D8C2BC"),
            (s.outline, "#A08C87"),
            (s.outline_variant, "#53433F"),
        ];
        for (color, want) in roles {
            assert_eq!(color, hex(want), "{want}");
        }
    }

    #[test]
    fn the_scheme_in_use_is_the_dark_one() {
        assert_eq!(*scheme(), Scheme::DARK);
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
