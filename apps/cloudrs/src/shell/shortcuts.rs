//! Keyboard shortcuts for playback, likes and pasted links (PLAN §7.4). The
//! keys that text fields also use (Space, the arrows, paste) are bound only
//! outside `SearchField`, so typing and caret movement stay untouched.

use gpui::prelude::*;
use gpui::{App, Context, Div, KeyBinding, Window, actions};
use sc_core::Command;

use super::Shell;
use crate::intent::UiIntent;

actions!(
    shell,
    [
        TogglePlay,
        SeekBack,
        SeekForward,
        PreviousTrack,
        NextTrack,
        ToggleLike,
        PasteLink,
        ToggleMiniPlayer
    ]
);

/// The keys Settings lists: the key as shown (Cmd on macOS) and what it does.
pub(crate) fn help() -> [(&'static str, &'static str); 9] {
    use crate::i18n::shortcuts as t;
    let pick = platform;
    [
        (t::key_play(), t::action_play()),
        (t::key_seek(), t::action_seek()),
        (t::key_skip(), t::action_skip()),
        (pick(t::key_like(), t::key_like_mac()), t::action_like()),
        (
            pick(t::key_search(), t::key_search_mac()),
            t::action_search(),
        ),
        (pick(t::key_paste(), t::key_paste_mac()), t::action_paste()),
        (
            pick(t::key_history(), t::key_history_mac()),
            t::action_history(),
        ),
        (
            pick(t::key_palette(), t::key_palette_mac()),
            t::action_palette(),
        ),
        (pick(t::key_mini(), t::key_mini_mac()), t::action_mini()),
    ]
}

/// The key as written on this platform: Cmd on macOS, Ctrl or Alt elsewhere.
pub(crate) fn platform(pc: &'static str, apple: &'static str) -> &'static str {
    if cfg!(target_os = "macos") { apple } else { pc }
}

/// Key context of the shell when no text field has focus.
const NOT_TYPING: &str = "Shell && !SearchField";

/// Key context of the mini player window.
pub(crate) const MINI_PLAYER: &str = "MiniPlayer";

/// Registers the shortcuts. `secondary` is Cmd on macOS and Ctrl elsewhere.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", TogglePlay, Some(NOT_TYPING)),
        KeyBinding::new("left", SeekBack, Some(NOT_TYPING)),
        KeyBinding::new("right", SeekForward, Some(NOT_TYPING)),
        KeyBinding::new("shift-left", PreviousTrack, Some(NOT_TYPING)),
        KeyBinding::new("shift-right", NextTrack, Some(NOT_TYPING)),
        KeyBinding::new("secondary-l", ToggleLike, None),
        KeyBinding::new("secondary-v", PasteLink, Some(NOT_TYPING)),
        KeyBinding::new("secondary-shift-m", ToggleMiniPlayer, None),
        // The mini player has no text field, so its keys need no guard.
        KeyBinding::new("space", TogglePlay, Some(MINI_PLAYER)),
        KeyBinding::new("shift-left", PreviousTrack, Some(MINI_PLAYER)),
        KeyBinding::new("shift-right", NextTrack, Some(MINI_PLAYER)),
    ]);
}

/// Attaches the shortcut handlers to the root.
pub(super) fn shortcut_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(Shell::on_toggle_play))
        .on_action(cx.listener(Shell::on_seek_back))
        .on_action(cx.listener(Shell::on_seek_forward))
        .on_action(cx.listener(Shell::on_previous))
        .on_action(cx.listener(Shell::on_next))
        .on_action(cx.listener(Shell::on_toggle_like))
        .on_action(cx.listener(Shell::on_paste_link))
        .on_action(cx.listener(Shell::on_toggle_mini_player))
}

impl Shell {
    fn on_toggle_play(&mut self, _: &TogglePlay, _: &mut Window, _: &mut Context<Self>) {
        if self.models.current.is_some() {
            self.send(Command::TogglePlay);
        }
    }

    fn on_seek_back(&mut self, _: &SeekBack, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge(false, cx);
    }

    fn on_seek_forward(&mut self, _: &SeekForward, _: &mut Window, cx: &mut Context<Self>) {
        self.nudge(true, cx);
    }

    fn nudge(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(to) = self.player.read(cx).nudge(forward) {
            self.send(Command::Seek(to));
        }
    }

    fn on_previous(&mut self, _: &PreviousTrack, _: &mut Window, _: &mut Context<Self>) {
        if self.models.current.is_some() {
            self.send(Command::Previous);
        }
    }

    fn on_next(&mut self, _: &NextTrack, _: &mut Window, _: &mut Context<Self>) {
        if self.models.current.is_some() {
            self.send(Command::Next);
        }
    }

    fn on_toggle_like(&mut self, _: &ToggleLike, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_like(cx);
    }

    /// Likes the playing track, or takes the like back.
    pub(super) fn toggle_like(&mut self, cx: &mut Context<Self>) {
        let Some(track) = self.models.current else {
            return;
        };
        let liked = !self.models.liked.contains(&track);
        self.dispatch(UiIntent::Like { track, liked }, cx);
    }

    fn on_paste_link(&mut self, _: &PasteLink, _: &mut Window, cx: &mut Context<Self>) {
        let intent = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| UiIntent::from_link(&text));
        if let Some(intent) = intent {
            self.dispatch(intent, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_has_a_key_and_an_action() {
        for (keys, action) in help() {
            assert!(!keys.is_empty() && !action.is_empty());
        }
    }
}
