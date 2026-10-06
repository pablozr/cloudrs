//! Interface text. Every string a person reads comes from here, never a
//! literal in a view (`docs/design/i18n.md`).
//!
//! `strings!` requires a translation for every [`Language`], so adding a
//! language makes the compiler list every missing string.

/// Interface languages. English is the default and, for now, the only one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
}

/// The language in use. Becomes a saved setting with the language picker (M4).
pub fn current() -> Language {
    Language::English
}

macro_rules! strings {
    ($( $(#[$meta:meta])* $name:ident { en: $en:literal $(,)? } )*) => {
        $(
            $(#[$meta])*
            pub fn $name() -> &'static str {
                match $crate::i18n::current() {
                    $crate::i18n::Language::English => $en,
                }
            }
        )*
    };
}

pub mod app {
    strings! {
        window_title { en: "cloudrs" }
    }
}

pub mod preview {
    strings! {
        headline { en: "Native SoundCloud, light as Rust." }
        subtitle { en: "Milestone 0 · design system check: tokens, fonts, motion and the player bar." }
        switch_to_light { en: "Light theme" }
        switch_to_dark { en: "Dark theme" }
        filter_all { en: "All" }
        filter_tracks { en: "Tracks" }
        filter_people { en: "People" }
        filter_playlists { en: "Playlists" }
        play_all { en: "Play all" }
        follow { en: "Follow" }
        shuffle { en: "Shuffle" }
        cancel { en: "Cancel" }
        badge_aac { en: "AAC 160k" }
        badge_preview { en: "30s preview" }
        badge_cached { en: "Cached" }
        sample_title { en: "Lights Out (Extended Mix)" }
        sample_artist { en: "Sample track · no audio in this preview" }
    }
}
