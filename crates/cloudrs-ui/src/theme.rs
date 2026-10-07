//! The theme: the one object screens use to reach tokens.

use gpui::{App, Global, Hsla, SharedString, Styled};

use crate::fonts::FontFamilies;
use crate::tokens::{self, FontRole, Palette, TypeToken};

/// Dark is the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl Global for ThemeMode {}

impl ThemeMode {
    pub fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// Stable id for saved settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// Resolved tokens for the current mode. Cheap to clone; build it once per
/// render with [`Theme::of`].
#[derive(Debug, Clone)]
pub struct Theme {
    pub mode: ThemeMode,
    pub colors: Palette,
    pub fonts: FontFamilies,
}

impl Theme {
    pub fn new(mode: ThemeMode, fonts: FontFamilies) -> Self {
        let colors = match mode {
            ThemeMode::Dark => tokens::dark(),
            ThemeMode::Light => tokens::light(),
        };
        Self {
            mode,
            colors,
            fonts,
        }
    }

    /// The theme for the app's current mode and fonts.
    pub fn of(cx: &App) -> Self {
        let mode = cx.try_global::<ThemeMode>().copied().unwrap_or_default();
        let fonts = cx.try_global::<FontFamilies>().cloned().unwrap_or_default();
        Self::new(mode, fonts)
    }

    pub fn family(&self, role: FontRole) -> SharedString {
        match role {
            FontRole::Display => self.fonts.display.clone(),
            FontRole::Interface => self.fonts.interface.clone(),
            FontRole::Mono => self.fonts.mono.clone(),
        }
    }

    /// Applies a step of the type scale (family, size, weight, line height).
    pub fn text<E: Styled>(&self, element: E, token: TypeToken) -> E {
        element
            .font_family(self.family(token.role))
            .text_size(token.size)
            .line_height(token.line_height)
            .font_weight(token.weight)
    }

    /// `color` adjusted to stay readable as text on this theme's surfaces:
    /// status colors are tuned for dark backgrounds and get darker on light ones.
    pub fn readable(&self, color: Hsla) -> Hsla {
        match self.mode {
            ThemeMode::Dark => color,
            ThemeMode::Light => Hsla {
                l: color.l * 0.62,
                ..color
            },
        }
    }

    /// `color` (an artwork's dominant colour) as the top of the page tint: its
    /// lightness and saturation are clamped for this theme and the opacity is
    /// lower on light surfaces, so it stays quiet in both.
    pub fn tint(&self, color: Hsla) -> Hsla {
        use tokens::tint;
        let (low, high, a) = match self.mode {
            ThemeMode::Dark => (
                tint::MIN_LIGHTNESS_DARK,
                tint::MAX_LIGHTNESS_DARK,
                tint::ALPHA_DARK,
            ),
            ThemeMode::Light => (
                tint::MIN_LIGHTNESS_LIGHT,
                tint::MAX_LIGHTNESS_LIGHT,
                tint::ALPHA_LIGHT,
            ),
        };
        Hsla {
            h: color.h,
            s: color.s.min(tint::MAX_SATURATION),
            l: color.l.clamp(low, high),
            a,
        }
    }

    /// Text color for a step of the scale: muted styles get the muted color.
    pub fn text_color_for(&self, token: TypeToken) -> Hsla {
        if token == tokens::typography::BODY_MUTED {
            self.colors.text_muted
        } else if token == tokens::typography::LABEL || token == tokens::typography::MONO {
            self.colors.text_subtle
        } else {
            self.colors.text
        }
    }
}
