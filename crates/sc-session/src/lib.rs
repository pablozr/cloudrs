//! Jam: listening together over peer-to-peer (ADR 0006, ADR 0011).
//!
//! A host starts a session and shares its link; guests join by pasting it.
//! Only session state travels (queue, playback anchors, requests); every
//! person plays the audio from SoundCloud themselves. Connections go direct
//! when the two networks allow it and through n0's public relays otherwise,
//! so a Jam works from anywhere with internet access.
//!
//! The crate knows tracks only as SoundCloud ids and nothing of queues or
//! playback: `sc-core` decides what to send. Each session runs on its own
//! thread with a small Tokio runtime and talks through flume channels, so no
//! async type crosses the boundary. Dropping the [`Session`] ends it.

#[cfg(feature = "jam")]
mod guest;
#[cfg(feature = "jam")]
mod host;
#[cfg(feature = "jam")]
mod net;
pub mod protocol;

use std::time::{Duration, Instant};

pub use protocol::{
    EndReason, PeerId, PeerInfo, Perms, QueuedTrack, Request, ToGuest, ToHost, Unplayable,
};

/// What a Jam link starts with, in the search field or a chat.
pub const LINK_PREFIX: &str = "cloudrs:jam/";
/// Most guests a host lets in (ADR 0011 §7); the host makes 16 people.
pub const MAX_GUESTS: usize = 15;

/// Whether `text` is a Jam link (not whether it can still be joined).
pub fn is_link(text: &str) -> bool {
    text.trim().starts_with(LINK_PREFIX)
}

/// How peers reach each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// n0's public relays, with direct paths found by hole punching. The
    /// link carries only the relay, never an IP address. What the app uses.
    Internet,
    /// No relays: the link carries this machine's addresses. For tests.
    Local,
}

/// The session's monotonic clock, in nanoseconds since it started. Playback
/// anchors are in the host's clock; a guest converts with the offset from
/// [`SessionEvent::ClockOffset`].
#[derive(Debug, Clone, Copy)]
pub struct Clock(Instant);

impl Clock {
    fn new() -> Self {
        Self(Instant::now())
    }

    pub fn now_ns(&self) -> u64 {
        self.0.elapsed().as_nanos() as u64
    }
}

/// What `sc-core` asks of the session.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    /// Host: send to every guest.
    Broadcast(ToGuest),
    /// Host: send to one guest.
    SendTo(PeerId, ToGuest),
    /// Host: remove a guest (it is told `Ended { Removed }`).
    Kick(PeerId),
    /// Guest: send to the host.
    Send(ToHost),
}

/// What the session reports to `sc-core`.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    /// Host: online, with the link to share.
    Started {
        link: String,
    },
    /// Host: a guest said hello. Answer with `Welcome` and the state.
    PeerJoined {
        peer: PeerId,
        name: String,
    },
    PeerLeft {
        peer: PeerId,
    },
    FromGuest {
        peer: PeerId,
        message: ToHost,
    },
    /// Guest: connected to the host (its `Welcome` follows as `FromHost`).
    Connected,
    FromHost(ToGuest),
    /// Guest: host time = this guest's [`Clock`] + `offset_ns`. Sent after
    /// joining and whenever a better sample (lower round trip) arrives.
    ClockOffset {
        offset_ns: i64,
        rtt: Duration,
    },
    /// The session is over and its thread is ending.
    Ended(Ended),
}

/// Why a session ended, as this side sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// `Session::leave` or the handle was dropped.
    Left,
    /// The host said so (it left, removed this guest, the Jam is full, or
    /// it speaks another version).
    ByHost(EndReason),
    /// The other side vanished (closed without a word, network lost).
    Lost,
    /// The link is not a Jam link.
    BadLink,
    /// Could not go online or reach the host.
    Unreachable(String),
}

/// A running Jam, as host or guest.
pub struct Session {
    commands: flume::Sender<SessionCommand>,
    events: flume::Receiver<SessionEvent>,
    clock: Clock,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Session {
    /// Starts hosting. [`SessionEvent::Started`] carries the link.
    pub fn host(network: Network) -> Self {
        #[cfg(feature = "jam")]
        return Self::spawn(move |clock, commands, events, stop| {
            host::run(network, clock, commands, events, stop)
        });
        #[cfg(not(feature = "jam"))]
        {
            let _ = network;
            Self::unavailable()
        }
    }

    /// Joins the Jam behind `link`, introducing this person as `name`.
    pub fn join(link: &str, name: String) -> Self {
        #[cfg(feature = "jam")]
        {
            let link = link.trim().to_owned();
            Self::spawn(move |clock, commands, events, stop| {
                guest::run(link, name, clock, commands, events, stop)
            })
        }
        #[cfg(not(feature = "jam"))]
        {
            let _ = (link, name);
            Self::unavailable()
        }
    }

    /// A build without the `jam` feature: the session ends at once.
    #[cfg(not(feature = "jam"))]
    fn unavailable() -> Self {
        Self::spawn(|_, _, _, _| async { Ended::Unreachable("built without Jam".into()) })
    }

    fn spawn<F, Fut>(run: F) -> Self
    where
        F: FnOnce(
                Clock,
                flume::Receiver<SessionCommand>,
                flume::Sender<SessionEvent>,
                tokio::sync::oneshot::Receiver<()>,
            ) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = Ended>,
    {
        let (commands, command_rx) = flume::unbounded();
        let (event_tx, events) = flume::unbounded();
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let clock = Clock::new();
        let spawned = std::thread::Builder::new()
            .name("cloudrs-jam".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                    .expect("the session runtime starts");
                let ended = runtime.block_on(run(clock, command_rx, event_tx.clone(), stop_rx));
                let _ = event_tx.send(SessionEvent::Ended(ended));
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "the Jam thread could not start");
        }
        Self {
            commands,
            events,
            clock,
            stop: Some(stop),
        }
    }

    /// Sends a command. Returns `false` once the session has ended.
    pub fn send(&self, command: SessionCommand) -> bool {
        self.commands.send(command).is_ok()
    }

    /// Events in order, ending with [`SessionEvent::Ended`].
    pub fn events(&self) -> &flume::Receiver<SessionEvent> {
        &self.events
    }

    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// Ends the session: a host tells every guest, a guest says goodbye.
    pub fn leave(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.leave();
    }
}
