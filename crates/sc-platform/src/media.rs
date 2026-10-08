//! The OS media controls (ADR 0019): SMTC on Windows, MPRIS on Linux, Now
//! Playing on macOS. The OS shows what plays and sends back the media keys
//! and its own buttons as `MediaKey`s. It knows nothing of the core: the app
//! maps keys to commands and tells it what to show.

use std::ffi::c_void;
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection,
};

/// The name the OS shows, and the D-Bus name on Linux
/// (`org.mpris.MediaPlayer2.cloudrs`). A brand, not interface text.
const NAME: &str = "cloudrs";

/// Something the person asked of the player from outside the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaKey {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    /// A fast-forward or rewind button; the step is ours to choose.
    Seek {
        forward: bool,
    },
    SeekBy {
        forward: bool,
        by: Duration,
    },
    SetPosition(Duration),
    /// 0 to 1.
    SetVolume(f64),
}

/// What the OS shows about the track.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub duration: Duration,
}

/// Where playback is, for the OS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Playing { at: Duration },
    Paused { at: Duration },
    Stopped,
}

pub struct MediaControls {
    inner: souvlaki::MediaControls,
    /// A failure was already logged as a warning.
    warned: bool,
}

/// Registers with the OS. `hwnd` is the main window's handle, required on
/// Windows and ignored elsewhere; call this on the main thread. `None` when
/// the OS refuses (logged), and the app carries on without.
pub fn start(hwnd: Option<*mut c_void>) -> Option<(MediaControls, flume::Receiver<MediaKey>)> {
    #[cfg(windows)]
    if hwnd.is_none() {
        tracing::warn!("the OS media controls need the window handle");
        return None;
    }
    let config = PlatformConfig {
        display_name: NAME,
        dbus_name: NAME,
        hwnd,
    };
    let mut inner = match souvlaki::MediaControls::new(config) {
        Ok(inner) => inner,
        Err(error) => {
            tracing::warn!(%error, "the OS media controls could not start");
            return None;
        }
    };
    let (tx, rx) = flume::unbounded();
    let attached = inner.attach(move |event| {
        if let Some(key) = key_from(event) {
            let _ = tx.send(key);
        }
    });
    if let Err(error) = attached {
        tracing::warn!(%error, "the OS media controls could not start");
        return None;
    }
    Some((
        MediaControls {
            inner,
            warned: false,
        },
        rx,
    ))
}

impl MediaControls {
    pub fn show(&mut self, metadata: &Metadata) {
        let result = self.inner.set_metadata(MediaMetadata {
            title: Some(&metadata.title),
            album: None,
            artist: Some(&metadata.artist),
            cover_url: metadata.cover_url.as_deref(),
            duration: (!metadata.duration.is_zero()).then_some(metadata.duration),
        });
        self.report(result);
    }

    pub fn set_state(&mut self, state: State) {
        let playback = match state {
            State::Playing { at } => MediaPlayback::Playing {
                progress: Some(MediaPosition(at)),
            },
            State::Paused { at } => MediaPlayback::Paused {
                progress: Some(MediaPosition(at)),
            },
            State::Stopped => MediaPlayback::Stopped,
        };
        let result = self.inner.set_playback(playback);
        self.report(result);
    }

    /// 0 to 1. Only MPRIS has a volume; elsewhere this does nothing.
    pub fn set_volume(&mut self, volume: f32) {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let result = self.inner.set_volume(f64::from(volume));
            self.report(result);
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let _ = volume;
    }

    /// The first failure is a warning; the rest would repeat it.
    fn report(&mut self, result: Result<(), souvlaki::Error>) {
        let Err(error) = result else { return };
        if self.warned {
            tracing::debug!(%error, "the OS media controls did not answer");
        } else {
            self.warned = true;
            tracing::warn!(%error, "the OS media controls stopped answering");
        }
    }
}

fn key_from(event: MediaControlEvent) -> Option<MediaKey> {
    let forward = |direction: SeekDirection| direction == SeekDirection::Forward;
    Some(match event {
        MediaControlEvent::Play => MediaKey::Play,
        MediaControlEvent::Pause => MediaKey::Pause,
        MediaControlEvent::Toggle => MediaKey::Toggle,
        MediaControlEvent::Stop => MediaKey::Stop,
        MediaControlEvent::Next => MediaKey::Next,
        MediaControlEvent::Previous => MediaKey::Previous,
        MediaControlEvent::Seek(direction) => MediaKey::Seek {
            forward: forward(direction),
        },
        MediaControlEvent::SeekBy(direction, by) => MediaKey::SeekBy {
            forward: forward(direction),
            by,
        },
        MediaControlEvent::SetPosition(MediaPosition(at)) => MediaKey::SetPosition(at),
        MediaControlEvent::SetVolume(volume) => MediaKey::SetVolume(volume),
        MediaControlEvent::OpenUri(_) | MediaControlEvent::Raise | MediaControlEvent::Quit => {
            return None;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_become_keys() {
        use MediaControlEvent as E;
        assert_eq!(key_from(E::Play), Some(MediaKey::Play));
        assert_eq!(key_from(E::Pause), Some(MediaKey::Pause));
        assert_eq!(key_from(E::Toggle), Some(MediaKey::Toggle));
        assert_eq!(key_from(E::Stop), Some(MediaKey::Stop));
        assert_eq!(key_from(E::Next), Some(MediaKey::Next));
        assert_eq!(key_from(E::Previous), Some(MediaKey::Previous));
    }

    #[test]
    fn seeks_keep_their_direction() {
        use MediaControlEvent as E;
        let by = Duration::from_secs(10);
        assert_eq!(
            key_from(E::Seek(SeekDirection::Forward)),
            Some(MediaKey::Seek { forward: true })
        );
        assert_eq!(
            key_from(E::Seek(SeekDirection::Backward)),
            Some(MediaKey::Seek { forward: false })
        );
        assert_eq!(
            key_from(E::SeekBy(SeekDirection::Forward, by)),
            Some(MediaKey::SeekBy { forward: true, by })
        );
        assert_eq!(
            key_from(E::SeekBy(SeekDirection::Backward, by)),
            Some(MediaKey::SeekBy { forward: false, by })
        );
    }

    #[test]
    fn position_and_volume_carry_their_value() {
        use MediaControlEvent as E;
        let at = Duration::from_secs(42);
        assert_eq!(
            key_from(E::SetPosition(MediaPosition(at))),
            Some(MediaKey::SetPosition(at))
        );
        assert_eq!(key_from(E::SetVolume(0.5)), Some(MediaKey::SetVolume(0.5)));
    }

    #[test]
    fn what_the_player_cannot_do_is_dropped() {
        use MediaControlEvent as E;
        assert_eq!(key_from(E::OpenUri("file:///a.mp3".into())), None);
        assert_eq!(key_from(E::Raise), None);
        assert_eq!(key_from(E::Quit), None);
    }
}
