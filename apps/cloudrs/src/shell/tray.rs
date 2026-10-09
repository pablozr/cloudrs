//! The tray icon (ADR 0024): shows cloudrs and controls playback from the
//! notification area. Follows the core's events like media.rs; playback ticks
//! reach it only when play or pause flips. What the person chooses comes back
//! as core commands.

use gpui::{Context, Task};
use sc_core::{Command, Event, PlayState};
use sc_platform::tray::{Labels, Tray, TrayAction, TrayImage};

use super::Shell;
use crate::i18n::tray as t;

/// The 256 px app icon, scaled down for the tray where there is no resource.
#[cfg_attr(windows, allow(dead_code))]
const ICON_PNG: &[u8] = include_bytes!("../../../../assets/brand/app-icon-256.png");

/// Side of the tray image decoded from [`ICON_PNG`].
#[cfg_attr(windows, allow(dead_code))]
const ICON_SIDE: u32 = 64;

#[derive(Default)]
pub(crate) struct SystemTray {
    tray: Option<Tray>,
    /// Reads what was chosen; dropping it stops it.
    _actions: Option<Task<()>>,
    /// What the menu's toggle says now.
    playing: bool,
}

impl SystemTray {
    /// Puts the icon in the notification area. Call on the main thread.
    pub(crate) fn start(cx: &mut Context<Shell>) -> Self {
        let mut this = Self::default();
        let Some(image) = image() else {
            return this;
        };
        if let Some((mut tray, actions)) = sc_platform::tray::start(image, labels()) {
            tray.set_active(false);
            this.tray = Some(tray);
            this._actions = Some(cx.spawn(async move |shell, cx| {
                while let Ok(action) = actions.recv_async().await {
                    if shell
                        .update(cx, |shell, cx| shell.on_tray_action(action, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        this
    }

    /// Relabels the menu after a language change.
    pub(crate) fn relabel(&mut self) {
        if let Some(tray) = &mut self.tray {
            tray.set_labels(labels());
        }
    }

    fn apply(&mut self, event: &Event) {
        let Some(tray) = &mut self.tray else {
            return;
        };
        match event {
            Event::NowPlaying(_) => tray.set_active(true),
            Event::Playback(playback) => {
                let playing = playback.state == PlayState::Playing;
                if playing != self.playing {
                    self.playing = playing;
                    tray.set_playing(playing);
                }
            }
            _ => {}
        }
    }
}

impl Shell {
    /// Follows what plays, for the menu's items.
    pub(crate) fn tray_event(&mut self, event: &Event) {
        self.tray.apply(event);
    }

    fn on_tray_action(&mut self, action: TrayAction, cx: &mut Context<Self>) {
        match action {
            TrayAction::Show => {
                self.show_main_window(cx);
                cx.activate(true);
            }
            TrayAction::Quit => {
                // Saves the session first; the app quits once the core answers.
                if self.request_shutdown(cx) {
                    cx.quit();
                }
            }
            TrayAction::TogglePlay | TrayAction::Previous | TrayAction::Next => {
                if let Some(command) = command_for(action, self.models.current.is_some()) {
                    self.send(command);
                }
            }
        }
    }
}

/// The command a menu item sends; playback needs a track, like the keys do.
fn command_for(action: TrayAction, has_track: bool) -> Option<Command> {
    if !has_track {
        return None;
    }
    match action {
        TrayAction::TogglePlay => Some(Command::TogglePlay),
        TrayAction::Previous => Some(Command::Previous),
        TrayAction::Next => Some(Command::Next),
        TrayAction::Show | TrayAction::Quit => None,
    }
}

fn labels() -> Labels {
    Labels {
        show: t::show(),
        play: t::play(),
        pause: t::pause(),
        previous: t::previous(),
        next: t::next(),
        quit: t::quit(),
    }
}

/// The icon in the executable on Windows (resource 1, ADR 0012); the app icon
/// decoded elsewhere.
fn image() -> Option<TrayImage> {
    if cfg!(windows) {
        return Some(TrayImage::WindowsResource(1));
    }
    decode_icon()
}

fn decode_icon() -> Option<TrayImage> {
    let decoded = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .inspect_err(|error| tracing::warn!(%error, "the tray icon image is unreadable"))
        .ok()?;
    let scaled = image::imageops::resize(
        &decoded.to_rgba8(),
        ICON_SIDE,
        ICON_SIDE,
        image::imageops::FilterType::Triangle,
    );
    Some(TrayImage::Rgba {
        rgba: scaled.into_raw(),
        width: ICON_SIDE,
        height: ICON_SIDE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_items_need_a_track() {
        for action in [
            TrayAction::TogglePlay,
            TrayAction::Previous,
            TrayAction::Next,
        ] {
            assert_eq!(command_for(action, false), None, "{action:?}");
            assert!(command_for(action, true).is_some(), "{action:?}");
        }
    }

    #[test]
    fn show_and_quit_are_not_commands() {
        for action in [TrayAction::Show, TrayAction::Quit] {
            assert_eq!(command_for(action, true), None, "{action:?}");
        }
    }

    #[test]
    fn the_icon_decodes_to_a_square_of_rgba() {
        let Some(TrayImage::Rgba {
            rgba,
            width,
            height,
        }) = decode_icon()
        else {
            panic!("the icon did not decode");
        };
        assert_eq!((width, height), (ICON_SIDE, ICON_SIDE));
        assert_eq!(rgba.len(), (width * height * 4) as usize);
    }
}
