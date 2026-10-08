//! Turns the saved settings into what the window shows (ADR 0017): the theme
//! mode (following the system when asked) and the interface language.

use cloudrs_ui::ThemeMode;
use gpui::{App, WindowAppearance};
use sc_core::{Settings, ThemeChoice};

use crate::i18n;

/// The mode to draw: the choice, or the system's when the choice is `System`.
pub fn theme_mode(choice: ThemeChoice, system: WindowAppearance) -> ThemeMode {
    match choice {
        ThemeChoice::Dark => ThemeMode::Dark,
        ThemeChoice::Light => ThemeMode::Light,
        ThemeChoice::System => match system {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
        },
    }
}

/// Applies the settings that live in globals. Runs before the window opens
/// (the title uses the language) and again whenever the core echoes a change.
pub fn apply(settings: &Settings, cx: &mut App) {
    cx.set_global(theme_mode(settings.theme, cx.window_appearance()));
    i18n::set(settings.language);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_choice_follows_the_operating_system() {
        let cases = [
            (WindowAppearance::Dark, ThemeMode::Dark),
            (WindowAppearance::VibrantDark, ThemeMode::Dark),
            (WindowAppearance::Light, ThemeMode::Light),
            (WindowAppearance::VibrantLight, ThemeMode::Light),
        ];
        for (system, expected) in cases {
            assert_eq!(theme_mode(ThemeChoice::System, system), expected);
        }
    }

    #[test]
    fn an_explicit_choice_ignores_the_operating_system() {
        for system in [WindowAppearance::Dark, WindowAppearance::Light] {
            assert_eq!(theme_mode(ThemeChoice::Dark, system), ThemeMode::Dark);
            assert_eq!(theme_mode(ThemeChoice::Light, system), ThemeMode::Light);
        }
    }
}
