//! Jam protocol v1 (ADR 0011 §5): the messages and their framing.
//!
//! A frame is a little-endian `u32` length followed by a JSON body, at most
//! [`MAX_FRAME`] bytes. The major version is the ALPN; later minor versions
//! only add fields with `#[serde(default)]`, so older peers keep reading.

use serde::{Deserialize, Serialize};

/// Protocol name and major version, negotiated by QUIC.
pub const ALPN: &[u8] = b"cloudrs/jam/1";
/// Minor version this build speaks.
pub const PROTO_MINOR: u16 = 0;
/// A larger frame closes the connection.
pub const MAX_FRAME: usize = 64 * 1024;

/// A guest, as the host numbers them (1, 2, 3...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub u32);

/// What a guest sends to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToHost {
    /// First frame of a connection.
    Hello {
        #[serde(default)]
        proto_minor: u16,
        /// Shown to the others (the SoundCloud username).
        #[serde(default)]
        name: String,
    },
    /// Clock sample: the guest's time when sent, in its session clock.
    Ping {
        t0: u64,
    },
    /// Loaded the prepared track, paused at the position.
    Ready {
        epoch: u64,
    },
    /// Cannot play the prepared track with this guest's account.
    CannotPlay {
        epoch: u64,
        track: u64,
        reason: Unplayable,
    },
    Request(Request),
    /// Leaving.
    Bye,
}

/// What a guest asks the host to do. Adding is always allowed; the rest
/// needs the "guests control playback" permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Request {
    AddToQueue { track: u64 },
    PlayNext { track: u64 },
    TogglePlay,
    Next,
    Previous,
    Seek { ms: u64 },
    Remove { index: u32 },
    Move { from: u32, to: u32 },
}

impl Request {
    /// Whether any guest may ask this, even without the playback permission.
    pub fn always_allowed(&self) -> bool {
        matches!(self, Self::AddToQueue { .. } | Self::PlayNext { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unplayable {
    /// Only a GO+ preview for this account.
    Preview,
    /// Not available in this person's region.
    Blocked,
    Failed,
}

/// What the host sends to a guest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToGuest {
    /// Answer to `Hello`.
    Welcome {
        #[serde(default)]
        proto_minor: u16,
        host: String,
        perms: Perms,
        you: PeerId,
    },
    /// Answer to `Ping`: the guest's `t0` and the host's time.
    Pong {
        t0: u64,
        th: u64,
    },
    /// The whole queue, replacing the guest's mirror.
    Queue {
        tracks: Vec<QueuedTrack>,
        current: Option<u32>,
    },
    /// Load this track paused at `pos_ms` and answer `Ready`.
    Prepare {
        epoch: u64,
        track: u64,
        pos_ms: u64,
    },
    /// Playing: the track was at `pos_ms` at the host's time `at_host_ns`.
    Playing {
        epoch: u64,
        pos_ms: u64,
        at_host_ns: u64,
    },
    Paused {
        epoch: u64,
        pos_ms: u64,
    },
    /// Who is in the Jam.
    Peers {
        peers: Vec<PeerInfo>,
    },
    Perms(Perms),
    /// A request the guest is not allowed to make.
    Denied {
        request: Request,
    },
    /// The Jam is over for this guest.
    Ended {
        reason: EndReason,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Perms {
    /// Play/pause, skip, seek and reorder, not only adding.
    #[serde(default)]
    pub guests_control_playback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedTrack {
    pub track: u64,
    /// Who added it; `None` for the host.
    #[serde(default)]
    pub added_by: Option<PeerId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: PeerId,
    pub name: String,
    /// Cannot play the current track (preview, region).
    #[serde(default)]
    pub cannot_play: Option<Unplayable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndReason {
    /// The host ended the Jam or closed cloudrs.
    HostLeft,
    /// The host removed this guest.
    Removed,
    /// The Jam already has the most people it allows.
    Full,
    /// The guest's cloudrs speaks another version.
    Version,
}

/// Encodes a frame: length, then JSON.
pub fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    let body = serde_json::to_vec(message).expect("protocol messages always serialize");
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    frame
}

/// The body length a frame header announces, if it is allowed.
pub fn frame_len(header: [u8; 4]) -> Option<usize> {
    let len = u32::from_le_bytes(header) as usize;
    (len <= MAX_FRAME).then_some(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(message: T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let frame = encode(&message);
        let len = frame_len(frame[..4].try_into().unwrap()).unwrap();
        assert_eq!(len, frame.len() - 4);
        let back: T = serde_json::from_slice(&frame[4..]).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn messages_survive_a_frame() {
        roundtrip(ToHost::Hello {
            proto_minor: PROTO_MINOR,
            name: "Ana".into(),
        });
        roundtrip(ToHost::Request(Request::Move { from: 1, to: 3 }));
        roundtrip(ToGuest::Queue {
            tracks: vec![QueuedTrack {
                track: 7,
                added_by: Some(PeerId(2)),
            }],
            current: Some(0),
        });
        roundtrip(ToGuest::Ended {
            reason: EndReason::Full,
        });
    }

    #[test]
    fn missing_fields_from_older_peers_default() {
        let hello: ToHost = serde_json::from_str(r#"{"type":"Hello"}"#).unwrap();
        assert_eq!(
            hello,
            ToHost::Hello {
                proto_minor: 0,
                name: String::new()
            }
        );
        let welcome: ToGuest = serde_json::from_str(
            r#"{"type":"Welcome","host":"Bo","perms":{},"you":3,"later_field":true}"#,
        )
        .unwrap();
        assert!(matches!(welcome, ToGuest::Welcome { you: PeerId(3), .. }));
    }

    #[test]
    fn oversized_frames_are_refused() {
        assert_eq!(frame_len((MAX_FRAME as u32).to_le_bytes()), Some(MAX_FRAME));
        assert_eq!(frame_len((MAX_FRAME as u32 + 1).to_le_bytes()), None);
    }

    #[test]
    fn only_adding_is_always_allowed() {
        assert!(Request::AddToQueue { track: 1 }.always_allowed());
        assert!(Request::PlayNext { track: 1 }.always_allowed());
        assert!(!Request::TogglePlay.always_allowed());
        assert!(!Request::Seek { ms: 0 }.always_allowed());
    }
}
