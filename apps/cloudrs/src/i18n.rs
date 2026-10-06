//! Interface text. Every string a person reads comes from here, never a
//! literal in a view (`docs/design/i18n.md`).
//!
//! `strings!` and `formats!` require a translation for every [`Language`], so
//! adding a language makes the compiler list every missing string.

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

/// Like `strings!`, for text with named arguments (`"{count} tracks"`), so a
/// language can reorder them.
macro_rules! formats {
    ($( $(#[$meta:meta])* $name:ident($($arg:ident),+) { en: $en:literal $(,)? } )*) => {
        $(
            $(#[$meta])*
            pub fn $name($($arg: impl ::std::fmt::Display),+) -> String {
                match $crate::i18n::current() {
                    $crate::i18n::Language::English => format!($en),
                }
            }
        )*
    };
}

pub mod app {
    strings! {
        window_title { en: "cloudrs" }
        brand_cloud { en: "cloud" }
        brand_rs { en: "rs" }
        switch_to_light { en: "Light theme" }
        switch_to_dark { en: "Dark theme" }
    }
}

pub mod search {
    strings! {
        placeholder { en: "Search tracks or paste a SoundCloud link" }
        hint { en: "Ctrl K" }
        hint_mac { en: "\u{2318}K" }
        results { en: "Search results" }
        empty_title { en: "Search SoundCloud" }
        empty_hint { en: "Type a track or artist, or paste a soundcloud.com link to play it." }
        no_results_hint { en: "Check the spelling or try fewer words." }
        error_title { en: "Could not load the results" }
        error_hint { en: "Check your connection and try again." }
        try_again { en: "Try again" }
        preview_badge { en: "30s preview" }
    }
    formats! {
        no_results_title(query) { en: "No results for \u{201c}{query}\u{201d}" }
        play_track(title, artist) { en: "Play {title} by {artist}" }
    }
}

pub mod player {
    strings! {
        nothing_playing { en: "Nothing playing" }
        nothing_playing_hint { en: "Pick a track from the results." }
        play { en: "Play" }
        pause { en: "Pause" }
        seek { en: "Seek" }
        volume { en: "Volume" }
    }
}

pub mod startup {
    strings! {
        audio_title { en: "No audio output found" }
        audio_hint { en: "Connect speakers or headphones, then try again." }
        network_title { en: "Could not start the network" }
        network_hint { en: "Check your connection and try again." }
        try_again { en: "Try again" }
    }
}

pub mod problem {
    strings! {
        offline { en: "Can\u{2019}t reach SoundCloud. Check your connection." }
        rate_limited { en: "SoundCloud asked us to slow down. Try again in a moment." }
        not_found { en: "That track or link doesn\u{2019}t exist or is private." }
        not_a_track { en: "That link isn\u{2019}t a track." }
        preview_only { en: "Only a 30-second preview is available for this track." }
        cannot_play { en: "This track can\u{2019}t be played here." }
        audio { en: "Something went wrong with the audio. Try again." }
    }
}
