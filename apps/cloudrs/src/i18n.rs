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
        try_again { en: "Try again" }
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
        preview_badge { en: "30s preview" }
        tab_tracks { en: "Tracks" }
        tab_people { en: "People" }
        tab_playlists { en: "Playlists" }
        tab_albums { en: "Albums" }
    }
    formats! {
        no_results_title(query) { en: "No results for \u{201c}{query}\u{201d}" }
        play_track(title, artist) { en: "Play {title} by {artist}" }
    }
}

/// Joins the parts of a meta line (`Ana \u{b7} 12 tracks`).
pub fn dot_join(parts: &[String]) -> String {
    parts.join(" \u{b7} ")
}

pub mod nav {
    strings! {
        sidebar { en: "Main navigation" }
        search { en: "Search" }
        history { en: "History" }
        back { en: "Back" }
        forward { en: "Forward" }
    }
}

/// Shared by every list: the empty and error states of the screens.
pub mod list {
    strings! {
        error_title { en: "Could not load this list" }
        error_hint { en: "Check your connection and try again." }
        empty_hint { en: "Nothing to show here yet." }
        user_tracks_empty { en: "No tracks yet" }
        user_playlists_empty { en: "No playlists yet" }
        user_likes_empty { en: "No likes yet" }
        playlist_empty { en: "This playlist is empty" }
        related_empty { en: "No related tracks" }
        history_empty { en: "Nothing played yet" }
        history_empty_hint { en: "Tracks you play show up here." }
        followings_empty { en: "Not following anyone yet" }
        feed_empty { en: "Your feed is empty" }
        feed_empty_hint { en: "Follow people to see what they post and repost." }
        library_empty { en: "No playlists or albums yet" }
    }
}

/// Shared by the track, profile and playlist pages.
pub mod page {
    strings! {
        error_title { en: "Could not load this page" }
        error_hint { en: "Check your connection and try again." }
        resolving_title { en: "Opening the link" }
        resolving_hint { en: "Looking it up on SoundCloud." }
    }
}

pub mod track {
    strings! {
        related { en: "Related" }
    }
    formats! {
        plays(count) { en: "{count} plays" }
        open_profile(name) { en: "Open the profile of {name}" }
    }
}

pub mod user {
    strings! {
        tab_tracks { en: "Tracks" }
        tab_playlists { en: "Playlists" }
        tab_likes { en: "Likes" }
    }
    formats! {
        followers(count) { en: "{count} followers" }
        following(count) { en: "{count} following" }
        open_profile(name) { en: "Open the profile of {name}" }
    }
}

pub mod playlist {
    strings! {
        album_badge { en: "Album" }
        kind_playlist { en: "Playlist" }
        play { en: "Play" }
    }
    formats! {
        open_playlist(title) { en: "Open {title}" }
    }
}

pub mod count {
    strings! {
        one_track { en: "1 track" }
    }
    formats! {
        many_tracks(count) { en: "{count} tracks" }
    }

    /// `1 track`, `12 tracks`.
    pub fn tracks(count: u64) -> String {
        if count == 1 {
            one_track().to_owned()
        } else {
            many_tracks(count)
        }
    }
}

pub mod player {
    strings! {
        nothing_playing { en: "Nothing playing" }
        nothing_playing_hint { en: "Pick a track from the results." }
        play { en: "Play" }
        pause { en: "Pause" }
        previous { en: "Previous" }
        next { en: "Next" }
        shuffle_off { en: "Shuffle: off" }
        shuffle_on { en: "Shuffle: on" }
        repeat_off { en: "Repeat: off" }
        repeat_all { en: "Repeat: all" }
        repeat_one { en: "Repeat: one track" }
        queue { en: "Queue" }
        seek { en: "Seek" }
        volume { en: "Volume" }
    }
}

pub mod queue {
    strings! {
        title { en: "Queue" }
        empty_title { en: "The queue is empty" }
        empty_hint { en: "Play a track, or add one from the results." }
        play_next { en: "Play next" }
        add_to_queue { en: "Add to queue" }
        remove { en: "Remove" }
    }
}

pub mod startup {
    strings! {
        audio_title { en: "No audio output found" }
        audio_hint { en: "Connect speakers or headphones, then try again." }
        network_title { en: "Could not start the network" }
        network_hint { en: "Check your connection and try again." }
    }
}

pub mod problem {
    strings! {
        offline { en: "Can\u{2019}t reach SoundCloud. Check your connection." }
        rate_limited { en: "SoundCloud asked us to slow down. Try again in a moment." }
        not_found { en: "That track or link doesn\u{2019}t exist or is private." }
        unsupported_link { en: "That link can\u{2019}t be opened." }
        preview_only { en: "Only a 30-second preview is available for this track." }
        cannot_play { en: "This track can\u{2019}t be played here." }
        audio { en: "Something went wrong with the audio. Try again." }
        storage_reset { en: "Your history and saved session were damaged and have been reset." }
        sign_in_failed { en: "SoundCloud didn{2019}t accept that sign-in. Try again." }
        session_expired { en: "Your SoundCloud session expired. Sign in again." }
        sign_in_required { en: "Sign in to like tracks and follow people." }
    }
}
