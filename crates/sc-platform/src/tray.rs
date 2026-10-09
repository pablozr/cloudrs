//! The tray icon (ADR 0024): a notification-area icon with a small menu to
//! show the window and control playback. Windows and macOS draw it natively;
//! Linux speaks StatusNotifierItem over D-Bus on a thread of its own (no GTK
//! main loop). It knows nothing of the core: the app maps [`TrayAction`]s to
//! commands and tells it what to show.

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// The name the OS shows as the icon's tooltip. A brand, not interface text.
const NAME: &str = "cloudrs";

const ID_SHOW: &str = "show";
const ID_TOGGLE: &str = "toggle";
const ID_PREVIOUS: &str = "previous";
const ID_NEXT: &str = "next";
const ID_QUIT: &str = "quit";

/// Something the person chose on the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    TogglePlay,
    Previous,
    Next,
    Quit,
}

/// The menu texts, from the app's language.
#[derive(Debug, Clone, Copy)]
pub struct Labels {
    pub show: &'static str,
    pub play: &'static str,
    pub pause: &'static str,
    pub previous: &'static str,
    pub next: &'static str,
    pub quit: &'static str,
}

/// What the icon looks like.
pub enum TrayImage {
    /// 32-bit RGBA pixels, `width * height * 4` bytes.
    Rgba {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
    /// An icon embedded in the executable (Windows only).
    WindowsResource(u16),
}

pub struct Tray {
    _icon: TrayIcon,
    _menu: Menu,
    show: MenuItem,
    toggle: MenuItem,
    previous: MenuItem,
    next: MenuItem,
    quit: MenuItem,
    labels: Labels,
    playing: bool,
}

/// Puts the icon in the notification area. Call on the main thread, once. A
/// receiver of what the person chose comes with it. `None` when the OS refuses
/// (logged), and the app carries on without.
pub fn start(image: TrayImage, labels: Labels) -> Option<(Tray, flume::Receiver<TrayAction>)> {
    let icon = match icon(image) {
        Ok(icon) => icon,
        Err(error) => {
            tracing::warn!(%error, "the tray icon image is not usable");
            return None;
        }
    };
    let (sender, receiver) = flume::unbounded();
    let menu_sender = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(action) = action_for(event.id().as_ref()) {
            menu_sender.send(action).ok();
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            sender.send(TrayAction::Show).ok();
        }
    }));

    let show = MenuItem::with_id(ID_SHOW, labels.show, true, None);
    let toggle = MenuItem::with_id(ID_TOGGLE, labels.play, false, None);
    let previous = MenuItem::with_id(ID_PREVIOUS, labels.previous, false, None);
    let next = MenuItem::with_id(ID_NEXT, labels.next, false, None);
    let quit = MenuItem::with_id(ID_QUIT, labels.quit, true, None);
    let menu = Menu::new();
    let appended = menu.append_items(&[
        &show,
        &PredefinedMenuItem::separator(),
        &toggle,
        &previous,
        &next,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);
    if let Err(error) = appended {
        tracing::warn!(%error, "the tray menu could not be built");
        return None;
    }
    let built = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip(NAME)
        .with_menu(Box::new(menu.clone()))
        // The left click shows the window; the menu is on the right click.
        .with_menu_on_left_click(false)
        .build();
    let tray_icon = match built {
        Ok(tray_icon) => tray_icon,
        Err(error) => {
            tracing::warn!(%error, "the tray icon could not start");
            return None;
        }
    };
    let tray = Tray {
        _icon: tray_icon,
        _menu: menu,
        show,
        toggle,
        previous,
        next,
        quit,
        labels,
        playing: false,
    };
    Some((tray, receiver))
}

fn icon(image: TrayImage) -> Result<Icon, tray_icon::BadIcon> {
    match image {
        TrayImage::Rgba {
            rgba,
            width,
            height,
        } => Icon::from_rgba(rgba, width, height),
        #[cfg(windows)]
        TrayImage::WindowsResource(ordinal) => Icon::from_resource(ordinal, None),
        #[cfg(not(windows))]
        TrayImage::WindowsResource(_) => Err(tray_icon::BadIcon::OsError(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "embedded icons exist only on Windows",
        ))),
    }
}

/// What a menu item id means.
fn action_for(id: &str) -> Option<TrayAction> {
    match id {
        ID_SHOW => Some(TrayAction::Show),
        ID_TOGGLE => Some(TrayAction::TogglePlay),
        ID_PREVIOUS => Some(TrayAction::Previous),
        ID_NEXT => Some(TrayAction::Next),
        ID_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

impl Tray {
    /// The toggle reads "Pause" while playing and "Play" otherwise.
    pub fn set_playing(&mut self, playing: bool) {
        if self.playing != playing {
            self.playing = playing;
            self.toggle.set_text(self.toggle_label());
        }
    }

    /// Playback items work only with a track.
    pub fn set_active(&mut self, has_track: bool) {
        self.toggle.set_enabled(has_track);
        self.previous.set_enabled(has_track);
        self.next.set_enabled(has_track);
    }

    /// Relabels the menu after a language change. On Linux the StatusNotifier
    /// backend follows the items' changes by itself, so nothing is re-applied.
    pub fn set_labels(&mut self, labels: Labels) {
        self.labels = labels;
        self.show.set_text(labels.show);
        self.toggle.set_text(self.toggle_label());
        self.previous.set_text(labels.previous);
        self.next.set_text(labels.next);
        self.quit.set_text(labels.quit);
    }

    fn toggle_label(&self) -> &'static str {
        if self.playing {
            self.labels.pause
        } else {
            self.labels.play
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_id_maps_to_its_action() {
        assert_eq!(action_for("show"), Some(TrayAction::Show));
        assert_eq!(action_for("toggle"), Some(TrayAction::TogglePlay));
        assert_eq!(action_for("previous"), Some(TrayAction::Previous));
        assert_eq!(action_for("next"), Some(TrayAction::Next));
        assert_eq!(action_for("quit"), Some(TrayAction::Quit));
    }

    #[test]
    fn a_separator_or_unknown_id_does_nothing() {
        assert_eq!(action_for(""), None);
        assert_eq!(action_for("separator"), None);
    }
}
