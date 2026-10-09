//! Interface text. Every string a person reads comes from here, never a
//! literal in a view (`docs/design/i18n.md`).
//!
//! `strings!` and `formats!` require a translation for every [`Language`], so
//! adding a language makes the compiler list every missing string.

use std::sync::atomic::{AtomicU8, Ordering};

/// Interface languages. English is the default and, for now, the only one.
/// The enum lives in `sc-core` because it is a saved setting (ADR 0017).
pub use sc_core::Language;

/// Index into `Language::ALL` of the language in use.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Sets the language in use, from the saved settings.
pub fn set(language: Language) {
    let index = Language::ALL
        .iter()
        .position(|l| *l == language)
        .unwrap_or_default();
    CURRENT.store(index as u8, Ordering::Relaxed);
}

/// The language in use.
pub fn current() -> Language {
    Language::ALL
        .get(usize::from(CURRENT.load(Ordering::Relaxed)))
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_current() {
        for language in Language::ALL {
            set(language);
            assert_eq!(current(), language);
        }
    }
}

// Transitional (ADR 0025): `pt_br` is optional until Brazilian Portuguese is a
// `Language`, so its texts can land in small commits. The expansion ignores it
// for now; the commit that adds the language makes it required.
macro_rules! strings {
    ($( $(#[$meta:meta])* $name:ident { en: $en:literal $(, pt_br: $pt:literal)? $(,)? } )*) => {
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
    ($( $(#[$meta:meta])* $name:ident($($arg:ident),+) { en: $en:literal $(, pt_br: $pt:literal)? $(,)? } )*) => {
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
        window_title { en: "cloudrs", pt_br: "cloudrs" }
        brand_cloud { en: "cloud", pt_br: "cloud" }
        brand_rs { en: "rs", pt_br: "rs" }
        try_again { en: "Try again", pt_br: "Tentar de novo" }
        undo { en: "Undo", pt_br: "Desfazer" }
        on { en: "On", pt_br: "Ligado" }
        off { en: "Off", pt_br: "Desligado" }
        switch_to_light { en: "Light theme", pt_br: "Tema claro" }
        switch_to_dark { en: "Dark theme", pt_br: "Tema escuro" }
        minimize { en: "Minimize", pt_br: "Minimizar" }
        maximize { en: "Maximize", pt_br: "Maximizar" }
        close { en: "Close", pt_br: "Fechar" }
    }
}

/// The Settings screen.
pub mod settings {
    strings! {
        title { en: "Settings", pt_br: "Configurações" }
        theme { en: "Theme", pt_br: "Tema" }
        theme_hint { en: "System follows the light or dark mode of your computer.", pt_br: "Sistema segue o modo claro ou escuro do seu computador." }
        theme_system { en: "System", pt_br: "Sistema" }
        theme_dark { en: "Dark", pt_br: "Escuro" }
        theme_light { en: "Light", pt_br: "Claro" }
        output { en: "Output", pt_br: "Saída" }
        output_hint { en: "Where cloudrs plays. System default follows your computer\u{2019}s choice.", pt_br: "Onde o cloudrs toca. O padrão do sistema segue a escolha do seu computador." }
        output_default { en: "System default", pt_br: "Padrão do sistema" }
        output_none { en: "No other output devices found.", pt_br: "Nenhum outro dispositivo de saída encontrado." }
        refresh { en: "Refresh", pt_br: "Atualizar" }
        sound { en: "Sound", pt_br: "Som" }
        sound_hint { en: "How cloudrs shapes what you hear.", pt_br: "Como o cloudrs molda o que você ouve." }
        normalize { en: "Normalize volume", pt_br: "Normalizar volume" }
        normalize_hint { en: "Evens out loud and quiet tracks.", pt_br: "Equilibra faixas altas e baixas." }
        equalizer { en: "Equalizer", pt_br: "Equalizador" }
        eq_off { en: "Off", pt_br: "Desligado" }
        eq_bass { en: "Bass", pt_br: "Graves" }
        eq_treble { en: "Treble", pt_br: "Agudos" }
        eq_vocal { en: "Vocal", pt_br: "Vocal" }
        eq_electronic { en: "Electronic", pt_br: "Eletrônica" }
        volume_boost { en: "Volume boost", pt_br: "Reforço de volume" }
        volume_boost_hint { en: "Lets the volume go up to 200% and lifts quiet tracks. Loud sound can harm your hearing.", pt_br: "Deixa o volume chegar a 200% e realça faixas baixas. Som alto pode prejudicar sua audição." }
        cache { en: "Cache", pt_br: "Cache" }
        cache_hint { en: "Covers kept on this computer so pages open faster. Covers on screen stay.", pt_br: "Capas guardadas neste computador para as páginas abrirem mais rápido. As capas na tela ficam." }
        clear_cache { en: "Clear cache", pt_br: "Limpar cache" }
        cache_cleared { en: "Cache cleared", pt_br: "Cache limpo" }
        shortcuts { en: "Keyboard shortcuts", pt_br: "Atalhos de teclado" }
    }
    formats! {
        cache_size(megabytes) { en: "{megabytes} MB", pt_br: "{megabytes} MB" }
    }
}

/// The keys listed in Settings, and what they do. The `_mac` keys are shown
/// on macOS.
pub mod shortcuts {
    strings! {
        key_play { en: "Space", pt_br: "Espaço" }
        key_seek { en: "\u{2190} \u{2192}", pt_br: "\u{2190} \u{2192}" }
        key_skip { en: "Shift \u{2190} \u{2192}", pt_br: "Shift \u{2190} \u{2192}" }
        key_like { en: "Ctrl L", pt_br: "Ctrl L" }
        key_like_mac { en: "\u{2318}L", pt_br: "\u{2318}L" }
        key_search { en: "Ctrl K  /", pt_br: "Ctrl K  /" }
        key_search_mac { en: "\u{2318}K  /", pt_br: "\u{2318}K  /" }
        key_paste { en: "Ctrl V", pt_br: "Ctrl V" }
        key_paste_mac { en: "\u{2318}V", pt_br: "\u{2318}V" }
        key_history { en: "Alt \u{2190} \u{2192}", pt_br: "Alt \u{2190} \u{2192}" }
        key_history_mac { en: "\u{2318}[ \u{2318}]", pt_br: "\u{2318}[ \u{2318}]" }
        key_palette { en: "Ctrl P", pt_br: "Ctrl P" }
        key_palette_mac { en: "\u{2318}P", pt_br: "\u{2318}P" }
        key_mini { en: "Ctrl Shift M", pt_br: "Ctrl Shift M" }
        key_mini_mac { en: "\u{21e7}\u{2318}M", pt_br: "\u{21e7}\u{2318}M" }
        action_play { en: "Play or pause", pt_br: "Tocar ou pausar" }
        action_seek { en: "Seek 5 seconds", pt_br: "Avançar ou voltar 5 segundos" }
        action_skip { en: "Previous or next track", pt_br: "Faixa anterior ou próxima" }
        action_like { en: "Like the playing track", pt_br: "Curtir a faixa que está tocando" }
        action_search { en: "Search", pt_br: "Buscar" }
        action_paste { en: "Open a copied link", pt_br: "Abrir um link copiado" }
        action_history { en: "Back or forward", pt_br: "Voltar ou avançar" }
        action_palette { en: "Command palette", pt_br: "Paleta de comandos" }
        action_mini { en: "Mini player", pt_br: "Mini player" }
    }
}

/// The command palette (ADR 0018). Screens use the `nav` names.
pub mod palette {
    strings! {
        title { en: "Command palette", pt_br: "Paleta de comandos" }
        placeholder { en: "Go to a screen or run an action", pt_br: "Ir para uma tela ou executar uma ação" }
        no_matches { en: "No matching commands", pt_br: "Nenhum comando encontrado" }
        play_pause { en: "Play or pause", pt_br: "Tocar ou pausar" }
        previous { en: "Previous track", pt_br: "Faixa anterior" }
        next { en: "Next track", pt_br: "Próxima faixa" }
        like { en: "Like the playing track", pt_br: "Curtir a faixa que está tocando" }
        queue { en: "Show or hide the queue", pt_br: "Mostrar ou ocultar a fila" }
        theme_system { en: "Theme: system", pt_br: "Tema: sistema" }
        theme_dark { en: "Theme: dark", pt_br: "Tema: escuro" }
        theme_light { en: "Theme: light", pt_br: "Tema: claro" }
        mini_player { en: "Mini player", pt_br: "Mini player" }
    }
}

/// Updates (ADR 0026), in Settings, the command palette and toasts.
pub mod update {
    strings! {
        title { en: "Updates", pt_br: "Atualizações" }
        hint { en: "cloudrs checks GitHub once a day and installs new versions when you restart.", pt_br: "O cloudrs verifica o GitHub uma vez por dia e instala novas versões quando você reinicia." }
        automatic { en: "Automatic updates", pt_br: "Atualizações automáticas" }
        check_now { en: "Check now", pt_br: "Verificar agora" }
        check_for_updates { en: "Check for updates", pt_br: "Verificar atualizações" }
        never_checked { en: "Not checked yet", pt_br: "Ainda não verificado" }
        checking { en: "Checking for updates\u{2026}", pt_br: "Verificando atualizações\u{2026}" }
        up_to_date { en: "cloudrs is up to date", pt_br: "O cloudrs está atualizado" }
        failed { en: "Couldn\u{2019}t check for updates", pt_br: "Não foi possível verificar atualizações" }
        download { en: "Download", pt_br: "Baixar" }
        restart { en: "Restart to update", pt_br: "Reiniciar para atualizar" }
        unavailable { en: "This build does not update itself.", pt_br: "Esta versão não se atualiza sozinha." }
    }
    formats! {
        version(version) { en: "Version {version}", pt_br: "Versão {version}" }
        checked_at(time) { en: "Up to date \u{b7} checked at {time}", pt_br: "Atualizado \u{b7} verificado às {time}" }
        available(version) { en: "Version {version} is available", pt_br: "A versão {version} está disponível" }
        downloading(version, percent) { en: "Downloading {version}\u{2026} {percent}%", pt_br: "Baixando {version}\u{2026} {percent}%" }
        ready(version) { en: "cloudrs {version} is ready to install", pt_br: "O cloudrs {version} está pronto para instalar" }
    }
}

pub mod search {
    strings! {
        placeholder { en: "Search tracks or paste a SoundCloud link", pt_br: "Busque faixas ou cole um link do SoundCloud" }
        hint { en: "Ctrl K", pt_br: "Ctrl K" }
        hint_mac { en: "\u{2318}K", pt_br: "\u{2318}K" }
        results { en: "Search results", pt_br: "Resultados da busca" }
        empty_title { en: "Search SoundCloud", pt_br: "Buscar no SoundCloud" }
        empty_hint { en: "Type a track or artist, or paste a soundcloud.com link to play it.", pt_br: "Digite uma faixa ou artista, ou cole um link do soundcloud.com para tocá-lo." }
        no_results_hint { en: "Check the spelling or try fewer words.", pt_br: "Confira a ortografia ou use menos palavras." }
        error_title { en: "Could not load the results", pt_br: "Não foi possível carregar os resultados" }
        error_hint { en: "Check your connection and try again.", pt_br: "Verifique sua conexão e tente de novo." }
        preview_badge { en: "30s preview", pt_br: "Prévia de 30s" }
        tab_tracks { en: "Tracks", pt_br: "Faixas" }
        tab_people { en: "People", pt_br: "Pessoas" }
        tab_playlists { en: "Playlists", pt_br: "Playlists" }
        tab_albums { en: "Albums", pt_br: "Álbuns" }
    }
    formats! {
        no_results_title(query) { en: "No results for \u{201c}{query}\u{201d}", pt_br: "Nenhum resultado para \u{201c}{query}\u{201d}" }
        play_track(title, artist) { en: "Play {title} by {artist}", pt_br: "Tocar {title} de {artist}" }
    }
}

/// Joins the parts of a meta line (`Ana \u{b7} 12 tracks`).
pub fn dot_join(parts: &[String]) -> String {
    parts.join(" \u{b7} ")
}

pub mod nav {
    strings! {
        sidebar { en: "Main navigation", pt_br: "Navegação principal" }
        home { en: "Home", pt_br: "Início" }
        search { en: "Search", pt_br: "Buscar" }
        history { en: "History", pt_br: "Histórico" }
        back { en: "Back", pt_br: "Voltar" }
        forward { en: "Forward", pt_br: "Avançar" }
        feed { en: "Feed", pt_br: "Feed" }
        likes { en: "Likes", pt_br: "Curtidas" }
        library { en: "Library", pt_br: "Biblioteca" }
        following { en: "Following", pt_br: "Seguindo" }
        sign_in { en: "Sign in", pt_br: "Entrar" }
        jam { en: "Jam", pt_br: "Jam" }
        settings { en: "Settings", pt_br: "Configurações" }
        your_playlists { en: "YOUR PLAYLISTS", pt_br: "SUAS PLAYLISTS" }
    }
}

/// Shared by every list: the empty and error states of the screens.
pub mod list {
    strings! {
        error_title { en: "Could not load this list", pt_br: "Não foi possível carregar esta lista" }
        error_hint { en: "Check your connection and try again.", pt_br: "Verifique sua conexão e tente de novo." }
        empty_hint { en: "Nothing to show here yet.", pt_br: "Ainda não há nada para mostrar aqui." }
        user_tracks_empty { en: "No tracks yet", pt_br: "Nenhuma faixa ainda" }
        user_playlists_empty { en: "No playlists yet", pt_br: "Nenhuma playlist ainda" }
        user_likes_empty { en: "No likes yet", pt_br: "Nenhuma curtida ainda" }
        playlist_empty { en: "This playlist is empty", pt_br: "Esta playlist está vazia" }
        related_empty { en: "No related tracks", pt_br: "Nenhuma faixa relacionada" }
        history_empty { en: "Nothing played yet", pt_br: "Nada tocado ainda" }
        history_empty_hint { en: "Tracks you play show up here.", pt_br: "As faixas que você tocar aparecem aqui." }
        followings_empty { en: "Not following anyone yet", pt_br: "Você ainda não segue ninguém" }
        feed_empty { en: "Your feed is empty", pt_br: "Seu feed está vazio" }
        feed_empty_hint { en: "Follow people to see what they post and repost.", pt_br: "Siga pessoas para ver o que elas publicam e repostam." }
        library_empty { en: "No playlists or albums yet", pt_br: "Nenhuma playlist ou álbum ainda" }
        trending_empty { en: "Nothing trending here right now", pt_br: "Nada em alta por aqui agora" }
    }
}

/// Shared by the track, profile and playlist pages.
pub mod page {
    strings! {
        error_title { en: "Could not load this page", pt_br: "Não foi possível carregar esta página" }
        error_hint { en: "Check your connection and try again.", pt_br: "Verifique sua conexão e tente de novo." }
        resolving_title { en: "Opening the link", pt_br: "Abrindo o link" }
        resolving_hint { en: "Looking it up on SoundCloud.", pt_br: "Procurando no SoundCloud." }
    }
}

pub mod track {
    strings! {
        related { en: "Related", pt_br: "Relacionadas" }
    }
    formats! {
        plays(count) { en: "{count} plays", pt_br: "{count} reproduções" }
        open_profile(name) { en: "Open the profile of {name}", pt_br: "Abrir o perfil de {name}" }
    }
}

/// The comments of a track page (ADR 0021).
pub mod comments {
    strings! {
        tab { en: "Comments", pt_br: "Comentários" }
        lane { en: "Timed comments", pt_br: "Comentários com marcação de tempo" }
        empty_title { en: "No comments yet", pt_br: "Nenhum comentário ainda" }
        empty_hint { en: "Comments people leave on this track show up here.", pt_br: "Os comentários deixados nesta faixa aparecem aqui." }
        off_title { en: "Comments are turned off", pt_br: "Os comentários estão desativados" }
        off_hint { en: "The artist turned off comments for this track.", pt_br: "O artista desativou os comentários desta faixa." }
        error_title { en: "Could not load the comments", pt_br: "Não foi possível carregar os comentários" }
    }
    formats! {
        more(count) { en: "+{count} more", pt_br: "+{count} mais" }
        row_label(name, time) { en: "Comment by {name} at {time}", pt_br: "Comentário de {name} em {time}" }
        row_label_untimed(name) { en: "Comment by {name}", pt_br: "Comentário de {name}" }
    }
}

pub mod user {
    strings! {
        tab_tracks { en: "Tracks", pt_br: "Faixas" }
        tab_playlists { en: "Playlists", pt_br: "Playlists" }
        tab_likes { en: "Likes", pt_br: "Curtidas" }
    }
    formats! {
        followers(count) { en: "{count} followers", pt_br: "{count} seguidores" }
        following(count) { en: "{count} following", pt_br: "{count} seguindo" }
        open_profile(name) { en: "Open the profile of {name}", pt_br: "Abrir o perfil de {name}" }
    }
}

pub mod playlist {
    strings! {
        album_badge { en: "Album", pt_br: "Álbum" }
        kind_playlist { en: "Playlist", pt_br: "Playlist" }
        play { en: "Play", pt_br: "Tocar" }
    }
    formats! {
        open_playlist(title) { en: "Open {title}", pt_br: "Abrir {title}" }
    }
}

pub mod count {
    strings! {
        one_track { en: "1 track", pt_br: "1 faixa" }
    }
    formats! {
        many_tracks(count) { en: "{count} tracks", pt_br: "{count} faixas" }
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
        mini_player { en: "Mini player", pt_br: "Mini player" }
    }
    formats! {
        volume_percent(percent) { en: "{percent}%" }
    }
}

/// The mini player window (ADR 0023).
pub mod mini {
    strings! {
        window_title { en: "cloudrs mini player", pt_br: "Mini player do cloudrs" }
        open_main { en: "Open cloudrs", pt_br: "Abrir o cloudrs" }
        close { en: "Close the mini player", pt_br: "Fechar o mini player" }
    }
}

/// The tray icon's menu (ADR 0024).
pub mod tray {
    strings! {
        show { en: "Show cloudrs", pt_br: "Mostrar o cloudrs" }
        play { en: "Play", pt_br: "Tocar" }
        pause { en: "Pause", pt_br: "Pausar" }
        previous { en: "Previous track", pt_br: "Faixa anterior" }
        next { en: "Next track", pt_br: "Próxima faixa" }
        quit { en: "Quit cloudrs", pt_br: "Sair do cloudrs" }
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
        cache_not_cleared { en: "Couldn\u{2019}t clear the cache. Try again." }
        offline { en: "Can\u{2019}t reach SoundCloud. Check your connection." }
        rate_limited { en: "SoundCloud asked us to slow down. Try again in a moment." }
        not_found { en: "That track or link doesn\u{2019}t exist or is private." }
        unsupported_link { en: "That link can\u{2019}t be opened." }
        preview_only { en: "Only a 30-second preview is available for this track." }
        cannot_play { en: "This track can\u{2019}t be played here." }
        audio { en: "Something went wrong with the audio. Try again." }
        output_missing { en: "Your chosen audio device isn\u{2019}t available. Using the system default." }
        output_lost { en: "Audio device disconnected. Playback paused on the system default." }
        storage_reset { en: "Your history and saved session were damaged and have been reset." }
        sign_in_failed { en: "SoundCloud didn\u{2019}t accept that sign-in. Try again." }
        session_expired { en: "Your SoundCloud session expired. Sign in again." }
        sign_in_required { en: "Sign in to like tracks and follow people." }
        sign_in_window { en: "The sign-in window could not open. Try signing in with a token." }
        playlist_not_saved { en: "That playlist change couldn\u{2019}t be saved. Try again." }
        jam_unreachable { en: "Couldn\u{2019}t reach the Jam. Check your connection and the link." }
        jam_bad_link { en: "That isn\u{2019}t a Jam link." }
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

/// What Discord shows (ADR 0015), and its on/off switch.
pub mod discord {
    strings! {
        listen_on_soundcloud { en: "Listen on SoundCloud" }
        get_cloudrs { en: "Get cloudrs" }
        playing { en: "Playing on cloudrs" }
        paused { en: "Paused" }
        setting { en: "Show what I play on Discord" }
        setting_hint { en: "Your Discord profile shows the track, its cover and a link to it. Nothing goes through a server of ours." }
    }
    formats! {
        by(artist) { en: "by {artist}" }
        in_jam(artist, people) { en: "by {artist} \u{b7} in a Jam of {people}" }
    }
}
