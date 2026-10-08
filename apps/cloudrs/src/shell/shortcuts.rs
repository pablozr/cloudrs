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
        PasteLink
    ]
);

/// Key context of the shell when no text field has focus.
const NOT_TYPING: &str = "Shell && !SearchField";

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
