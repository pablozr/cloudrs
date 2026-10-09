//! The mini player window, seen from the shell (ADR 0023): opening and closing
//! it, forwarding it the player events and turning what it asks into commands.

use gpui::{Context, Window};
use sc_core::{Command, Event};

use super::Shell;
use super::shortcuts::ToggleMiniPlayer;
use crate::mini_player::{self, MiniAction};

impl Shell {
    pub(super) fn on_toggle_mini_player(
        &mut self,
        _: &ToggleMiniPlayer,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_mini_player(cx);
    }

    /// Opens the mini player, or closes it when it is open.
    pub(crate) fn toggle_mini_player(&mut self, cx: &mut Context<Self>) {
        if self.mini.is_some() {
            self.close_mini(cx);
        } else {
            self.open_mini(cx);
        }
        self.sync_mini_button(cx);
    }

    fn open_mini(&mut self, cx: &mut Context<Self>) {
        let state = self.player.read(cx).state().clone();
        let Some(handle) = mini_player::open(state, cx) else {
            return;
        };
        // Its root entity emits the actions; keep the subscription with it.
        self.mini_sub = handle.update(cx, |_, _, cx| cx.entity()).ok().map(|mini| {
            cx.subscribe(&mini, |this, _, action, cx| {
                this.on_mini_action(*action, cx)
            })
        });
        self.mini = Some(handle);
    }

    /// Closes the window if it is open and forgets it.
    pub(super) fn close_mini(&mut self, cx: &mut Context<Self>) {
        self.mini_sub = None;
        if let Some(handle) = self.mini.take() {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }

    fn sync_mini_button(&mut self, cx: &mut Context<Self>) {
        let open = self.mini.is_some();
        self.player
            .update(cx, |bar, cx| bar.set_mini_open(open, cx));
    }

    fn on_mini_action(&mut self, action: MiniAction, cx: &mut Context<Self>) {
        match action {
            MiniAction::TogglePlay => self.send_playback(Command::TogglePlay),
            MiniAction::Previous => self.send_playback(Command::Previous),
            MiniAction::Next => self.send_playback(Command::Next),
            MiniAction::ShowMain => self.show_main_window(cx),
            // The window is already going away.
            MiniAction::Closed => self.forget_mini(cx),
        }
    }

    /// Drops a mini window that is gone, and turns its button off.
    fn forget_mini(&mut self, cx: &mut Context<Self>) {
        self.mini = None;
        self.mini_sub = None;
        self.sync_mini_button(cx);
    }

    /// Writes the window title again, after the language changed.
    pub(super) fn relabel_mini(&self, cx: &mut Context<Self>) {
        if let Some(handle) = &self.mini {
            handle
                .update(cx, |_, window, _| {
                    window.set_window_title(crate::i18n::mini::window_title());
                })
                .ok();
        }
    }

    /// Brings the main window to the front.
    pub(crate) fn show_main_window(&mut self, cx: &mut Context<Self>) {
        self.main_window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    }

    /// Gives the mini player what the player bar just got, except the
    /// waveform, which it does not draw. A window that is gone is forgotten.
    pub(crate) fn mini_event(&mut self, event: &Event, cx: &mut Context<Self>) {
        let Some(handle) = &self.mini else {
            return;
        };
        if matches!(event, Event::Waveform { .. }) {
            return;
        }
        let artwork = &self.models.art.tracks;
        let updated = handle.update(cx, |mini, _, cx| mini.apply(event, artwork, cx));
        if updated.is_err() {
            self.forget_mini(cx);
        }
    }
}
