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
        undo { en: "Undo" }
        switch_to_light { en: "Light theme" }
        switch_to_dark { en: "Dark theme" }
        minimize { en: "Minimize" }
        maximize { en: "Maximize" }
        close { en: "Close" }
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
        home { en: "Home" }
        search { en: "Search" }
        history { en: "History" }
        back { en: "Back" }
        forward { en: "Forward" }
        feed { en: "Feed" }
        likes { en: "Likes" }
        library { en: "Library" }
        following { en: "Following" }
        sign_in { en: "Sign in" }
        jam { en: "Jam" }
        your_playlists { en: "YOUR PLAYLISTS" }
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
        trending_empty { en: "Nothing trending here right now" }
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
        sign_in_window { en: "The sign-in window could not open. Try signing in with a token." }
        playlist_not_saved { en: "That playlist change couldn{2019}t be saved. Try again." }
        jam_unreachable { en: "Couldn{2019}t reach the Jam. Check your connection and the link." }
        jam_bad_link { en: "That isn{2019}t a Jam link." }
        jam_ended { en: "The Jam has ended." }
        jam_removed { en: "The host removed you from the Jam." }
        jam_full { en: "That Jam is full." }
        jam_version { en: "The host uses another version of cloudrs. Update to join." }
        jam_not_allowed { en: "Only the host can do that in this Jam." }
    }
}

/// The account screen (ADR 0010).
pub mod account {
    strings! {
        title { en: "Account" }
        signed_out_title { en: "Sign in to SoundCloud" }
        signed_out_hint { en: "See your feed, likes and library, like tracks and follow people. You sign in on soundcloud.com in a small window; cloudrs only keeps the session in your system\u{2019}s keychain." }
        sign_in { en: "Sign in with SoundCloud" }
        waiting { en: "Finish signing in in the SoundCloud window\u{2026}" }
        other_ways { en: "Other ways to sign in" }
        token_steps { en: "1. Sign in on soundcloud.com in your browser.  2. Open the developer tools (F12) \u{2192} Application (Storage in Firefox) \u{2192} Cookies \u{2192} https://soundcloud.com.  3. Copy the value of the cookie named oauth_token and paste it here." }
        token_placeholder { en: "Paste your oauth_token" }
        token_sign_in { en: "Sign in with token" }
        sign_out { en: "Sign out" }
        signed_in_hint { en: "Signed in to SoundCloud. Signing out removes the session from this computer." }
        unofficial { en: "cloudrs is an unofficial client, not made by SoundCloud." }
    }
}

pub mod social {
    strings! {
        like { en: "Like" }
        unlike { en: "Unlike" }
        follow { en: "Follow" }
        unfollow { en: "Unfollow" }
    }
}

/// The Jam screen: listening together (ADR 0011).
pub mod jam {
    strings! {
        title { en: "Jam" }
        intro_title { en: "Listen together" }
        intro_hint { en: "Start a Jam and share its link: friends hear the same track at the same moment, wherever they are, and can add to the queue. To join one, paste its link in the search field." }
        start { en: "Start a Jam" }
        step_start { en: "Start a Jam" }
        step_start_hint { en: "You host: your queue plays for everyone." }
        step_share { en: "Share the link" }
        step_share_hint { en: "Send it in any chat. It works from any network." }
        step_listen { en: "Listen together" }
        step_listen_hint { en: "Same track, same second. Friends add songs too." }
        going_online { en: "Going online\u{2026}" }
        joining { en: "Joining the Jam\u{2026}" }
        hosting { en: "You\u{2019}re hosting a Jam" }
        share_hint { en: "Share this link. Anyone with cloudrs can join, from any network." }
        copy_link { en: "Copy link" }
        copied { en: "Link copied" }
        people { en: "Listening" }
        nobody_yet { en: "Nobody has joined yet." }
        guests_control { en: "Guests can play, pause and skip" }
        guests_add_only { en: "Guests can only add tracks" }
        end { en: "End Jam" }
        leave { en: "Leave Jam" }
        guest_hint { en: "The host\u{2019}s queue plays here. Tracks you play are added to it." }
        cannot_play { en: "can\u{2019}t play this track" }
        role_host { en: "Host" }
        role_guest { en: "Listening along" }
        you { en: "You" }
        connecting { en: "Connecting\u{2026}" }
        open_jam { en: "Open the Jam" }
        remove { en: "Remove" }
    }
    formats! {
        in_jam(host) { en: "In {host}\u{2019}s Jam" }
        remove_person(name) { en: "Remove {name} from the Jam" }
        listening(count) { en: "{count} listening" }
    }
}

/// Home, where cloudrs opens.
pub mod home {
    strings! {
        welcome { en: "Welcome to cloudrs" }
        good_morning { en: "Good morning" }
        good_afternoon { en: "Good afternoon" }
        good_evening { en: "Good evening" }
        subtitle { en: "Pick up where you left off, or find something new." }
        recently_played { en: "Recently played" }
        your_playlists { en: "Your playlists" }
        from_people_you_follow { en: "New from people you follow" }
        liked_tracks { en: "Liked tracks" }
        artists_you_follow { en: "Artists you follow" }
        trending { en: "Trending on SoundCloud" }
        now_playing { en: "NOW PLAYING" }
        jump_back_in { en: "JUMP BACK IN" }
        trending_now { en: "TRENDING NOW" }
        play { en: "Play" }
        pause { en: "Pause" }
        open_track { en: "Open track" }
        see_all { en: "See all" }
        start_title { en: "Start listening" }
        start_hint { en: "Search for a track or an artist, or paste a SoundCloud link. Sign in to bring your likes, playlists and feed." }
    }
    formats! {
        welcome_back(name) { en: "Welcome back, {name}" }
        good_morning_name(name) { en: "Good morning, {name}" }
        good_afternoon_name(name) { en: "Good afternoon, {name}" }
        good_evening_name(name) { en: "Good evening, {name}" }
        see_all_of(shelf) { en: "See all: {shelf}" }
    }
}

/// The genre pills of Home's trending row.
pub mod genre {
    strings! {
        all { en: "All" }
        electronic { en: "Electronic" }
        house { en: "House" }
        hip_hop { en: "Hip Hop" }
        dubstep { en: "Dubstep" }
        ambient { en: "Ambient" }
        pop { en: "Pop" }
        rock { en: "Rock" }
        indie { en: "Indie" }
        latin { en: "Latin" }
        r_n_b { en: "R&B" }
        trap { en: "Trap" }
    }
}

/// The Library screen.
pub mod library {
    strings! {
        title { en: "Your library" }
        tab_all { en: "All" }
        tab_playlists { en: "Playlists" }
        tab_albums { en: "Albums" }
    }
    formats! {
        playlists(count) { en: "{count} playlists" }
        albums(count) { en: "{count} albums" }
    }
}

/// Your own playlists: the menu, the dialogs and the toasts (ADR 0014).
pub mod playlists {
    strings! {
        add_to_playlist { en: "Add to playlist" }
        new_playlist { en: "New playlist\u{2026}" }
        new_playlist_title { en: "New playlist" }
        name_placeholder { en: "Playlist name" }
        private_hint { en: "Private: only you can see it, until you make it public." }
        public_hint { en: "Public: anyone on SoundCloud can find and play it." }
        description_placeholder { en: "Description (optional)" }
        genre_placeholder { en: "Genre" }
        tags_placeholder { en: "Tags, separated by commas" }
        choose_cover { en: "Choose a cover image" }
        cover_label { en: "COVER" }
        change_cover { en: "Change cover" }
        edit_description { en: "Description" }
        description_title { en: "Playlist description" }
        cover_not_saved { en: "The playlist was saved, but not its cover. Try another image (JPEG or PNG)." }
        create { en: "Create" }
        cancel { en: "Cancel" }
        save { en: "Save" }
        rename { en: "Rename" }
        rename_title { en: "Rename playlist" }
        delete { en: "Delete" }
        delete_hint { en: "It disappears from SoundCloud too. This can\u{2019}t be undone." }
        make_public { en: "Make public" }
        make_private { en: "Make private" }
        public_label { en: "Public" }
        private_label { en: "Private" }
        remove_from { en: "Remove from playlist" }
    }
    formats! {
        add_to(name) { en: "Add to {name}" }
        delete_title(name) { en: "Delete \u{201c}{name}\u{201d}?" }
        created(name) { en: "Created {name}" }
        added(name) { en: "Added to {name}" }
        already_there(name) { en: "Already in {name}" }
        removed(name) { en: "Removed from {name}" }
        renamed(name) { en: "Renamed to {name}" }
        now_public(name) { en: "{name} is now public" }
        now_private(name) { en: "{name} is now private" }
        deleted(name) { en: "Deleted {name}" }
        described(name) { en: "Description of {name} saved" }
        new_cover(name) { en: "New cover for {name}" }
    }
}

/// What Discord shows (ADR 0015), and the Account screen's switch.
pub mod discord {
    strings! {
        listen_on_soundcloud { en: "Listen on SoundCloud" }
        get_cloudrs { en: "Get cloudrs" }
        playing { en: "Playing on cloudrs" }
        paused { en: "Paused" }
        setting { en: "Show what I play on Discord" }
        setting_hint { en: "Your Discord profile shows the track, its cover and a link to it. Nothing goes through a server of ours." }
        on { en: "On" }
        off { en: "Off" }
    }
    formats! {
        by(artist) { en: "by {artist}" }
        in_jam(artist, people) { en: "by {artist} \u{b7} in a Jam of {people}" }
    }
}
