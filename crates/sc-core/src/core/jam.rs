//! Jam (ADR 0006, ADR 0011): listening together through `sc-session`.
//!
//! **Host.** The queue and playback stay the host's own; every change goes
//! to the guests. A new track starts behind a barrier: everyone prepares it
//! paused at the position (`Prepare` → `Ready`, or 5 s), then the host names a
//! shared instant a moment ahead and everyone presses play at it. Resuming
//! and seeking do the same. Guests' requests become the usual commands.
//!
//! **Guest.** The queue mirrors the host's, and queue or playback commands
//! become requests. The guest plays what the host prepares and follows its
//! anchors with the clock offset the session measures: paused, it schedules
//! play at the right instant; too far behind or ahead, it seeks (paused) a
//! little ahead and schedules play there. Its own queue comes back after.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_api::SoundCloudApi;
use sc_api::models::Track;
use sc_session::{
    EndReason, Ended, PeerId, PeerInfo, Perms, Profile, QueuedTrack, Request, Session,
    SessionCommand, SessionEvent, ToGuest, ToHost, Unplayable,
};
use tokio::task::JoinHandle;

use super::{Core, Input};
use crate::store::Session as SavedSession;
use crate::types::{
    ArtKey, JamPerson, JamRole, JamState, PlayState, Problem, TrackId, TrackSummary, UserId,
};
use crate::{Command, Event};

/// How far ahead the shared start instant is named.
const START_DELAY: Duration = Duration::from_millis(300);
/// How long the host waits for guests to prepare a track.
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// A guest further off than this corrects.
const DRIFT_LIMIT: Duration = Duration::from_millis(300);
/// Readings off by more than the limit before a guest corrects.
const DRIFT_STRIKES: u8 = 2;
const CORRECTION_COOLDOWN: Duration = Duration::from_secs(10);
/// How often a playing host repeats where it is.
const HEARTBEAT: Duration = Duration::from_secs(1);
/// How far ahead a guest seeks to catch up, before it has measured itself.
const DEFAULT_LEAD: Duration = Duration::from_secs(1);
const MAX_LEAD: Duration = Duration::from_secs(4);
/// A schedule this close (or late) plays at once instead of catching up.
const ON_TIME: Duration = Duration::from_millis(20);

pub(super) struct Jam {
    session: Session,
    generation: u64,
    pump: JoinHandle<()>,
    role: Role,
    /// A `Prepare` or a paused seek was sent; the next position means ready.
    awaiting_position: Option<Instant>,
    /// A play scheduled at the shared instant.
    play_at: Option<JoinHandle<()>>,
    play_at_token: u64,
}

enum Role {
    Host(Host),
    Guest(Guest),
}

struct Host {
    link: Option<String>,
    people: Vec<Person>,
    perms: Perms,
    epoch: u64,
    barrier: Option<Barrier>,
    /// Who asked for each track in the queue.
    added_by: HashMap<TrackId, PeerId>,
    last_anchor: Option<Instant>,
    /// A seek or a resume is waiting for the audio to be ready at the new
    /// position; `true` to play once it is.
    resync: Option<bool>,
}

struct Person {
    id: PeerId,
    name: String,
    user_id: Option<u64>,
    avatar_url: Option<String>,
    cannot_play: bool,
}

struct Barrier {
    epoch: u64,
    waiting: HashSet<PeerId>,
    local_ready: bool,
    timed_out: bool,
    pos: Duration,
}

struct Guest {
    host_name: String,
    host_user: Option<u64>,
    connected: bool,
    perms: Perms,
    people: Vec<PeerInfo>,
    me: Option<PeerId>,
    /// Host clock = this session's clock + offset.
    offset_ns: Option<i64>,
    /// The track the host prepared last.
    epoch: u64,
    /// Prepared, paused or playing at a known position.
    ready: bool,
    ready_sent: bool,
    anchor: Option<Anchor>,
    /// How far ahead to seek to catch up: one and a half preparing times.
    lead: Duration,
    strikes: u8,
    last_correction: Option<Instant>,
    /// The guest's own queue and position, restored when the Jam ends.
    saved: Option<SavedSession>,
}

#[derive(Clone, Copy)]
struct Anchor {
    epoch: u64,
    playing: bool,
    pos: Duration,
    at_host_ns: u64,
}

/// What to do with a track fetched for the Jam.
pub(super) enum AfterFetch {
    /// A guest asked for it.
    Enqueue { peer: PeerId, next: bool },
    /// The host prepared it.
    Prepare { epoch: u64, pos: Duration },
}

/// Timers of the Jam, reported back as inputs.
pub(super) enum JamTimer {
    ReadyTimeout { epoch: u64 },
    PlayAt { token: u64 },
}

fn ns(duration: Duration) -> i64 {
    duration.as_nanos() as i64
}

fn from_ns(ns: i64) -> Duration {
    Duration::from_nanos(ns.max(0) as u64)
}

fn ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

impl<A: SoundCloudApi + 'static> Core<A> {
    fn jam_name(&self) -> String {
        self.account
            .as_ref()
            .map_or_else(|| "cloudrs".to_owned(), |a| a.user.username.clone())
    }

    /// Who this person is to the others: name, account and avatar.
    fn jam_profile(&self) -> Profile {
        let user = self.account.as_ref().map(|a| a.user.id);
        Profile {
            name: self.jam_name(),
            user_id: user.map(|u| u.0),
            avatar_url: user.and_then(|u| self.other_art.get(&ArtKey::User(u)).cloned()),
        }
    }

    /// Keeps a Jam member's avatar URL and fetches it into the artwork cache.
    fn remember_avatar(&mut self, user: Option<u64>, url: Option<String>) {
        if let (Some(user), Some(url)) = (user, url) {
            let key = ArtKey::User(UserId(user));
            self.other_art.insert(key, url);
            self.request_artwork(key);
        }
    }

    pub(super) fn is_jam_guest(&self) -> bool {
        matches!(
            self.jam,
            Some(Jam {
                role: Role::Guest(_),
                ..
            })
        )
    }

    /// The session to save: a guest keeps its own, not the Jam's mirror.
    pub(super) fn jam_saved_session(&self) -> Option<SavedSession> {
        match &self.jam {
            Some(Jam {
                role: Role::Guest(guest),
                ..
            }) => guest.saved.clone(),
            _ => None,
        }
    }

    fn jam_send(&self, command: SessionCommand) {
        if let Some(jam) = &self.jam {
            jam.session.send(command);
        }
    }

    fn jam_now_ns(&self) -> u64 {
        self.jam
            .as_ref()
            .map_or(0, |jam| jam.session.clock().now_ns())
    }

    /// Handles the Jam commands, and the commands a Jam changes. Returns
    /// whether `command` was taken.
    pub(super) fn jam_command(&mut self, command: &Command) -> bool {
        match command {
            Command::StartJam => {
                if self.jam.is_none() {
                    self.start_jam(None);
                }
                return true;
            }
            Command::JoinJam(link) => {
                self.leave_jam(None);
                self.start_jam(Some(link.clone()));
                return true;
            }
            Command::LeaveJam => {
                self.leave_jam(None);
                return true;
            }
            Command::SetJamGuestsControl(on) => {
                if let Some(Jam {
                    role: Role::Host(host),
                    ..
                }) = &mut self.jam
                {
                    host.perms.guests_control_playback = *on;
                    let perms = host.perms;
                    self.jam_send(SessionCommand::Broadcast(ToGuest::Perms(perms)));
                    self.emit_jam();
                }
                return true;
            }
            Command::RemoveFromJam(id) => {
                self.jam_send(SessionCommand::Kick(PeerId(*id)));
                return true;
            }
            _ => {}
        }
        match &self.jam {
            Some(Jam {
                role: Role::Guest(guest),
                ..
            }) => {
                let control = guest.perms.guests_control_playback;
                self.guest_command(command, control)
            }
            Some(Jam {
                role: Role::Host(host),
                ..
            }) => {
                let settled = host.barrier.is_none() && self.pending_restore.is_none();
                match command {
                    Command::TogglePlay if settled && self.playback.state == PlayState::Paused => {
                        self.host_resync(self.playback.position, true);
                        true
                    }
                    Command::Seek(at) if settled => {
                        let playing = self.playback.state == PlayState::Playing;
                        self.host_resync(*at, playing);
                        true
                    }
                    _ => false,
                }
            }
            None => false,
        }
    }

    /// A guest's queue and playback commands go to the host as requests.
    fn guest_command(&mut self, command: &Command, control: bool) -> bool {
        let requests = match command {
            Command::TogglePlay => vec![Request::TogglePlay],
            Command::Next => vec![Request::Next],
            Command::Previous => vec![Request::Previous],
            Command::Seek(at) => vec![Request::Seek { ms: ms(*at) }],
            Command::PlayNext(id) => vec![Request::PlayNext { track: id.0 }],
            Command::AddToQueue(id) => vec![Request::AddToQueue { track: id.0 }],
            // Playing from a list: next in the Jam, and now if allowed.
            Command::Play { track, .. } if control => {
                vec![Request::PlayNext { track: track.0 }, Request::Next]
            }
            Command::Play { track, .. } => vec![Request::PlayNext { track: track.0 }],
            Command::RemoveFromQueue(index) => vec![Request::Remove {
                index: *index as u32,
            }],
            Command::MoveInQueue { from, to } => vec![Request::Move {
                from: *from as u32,
                to: *to as u32,
            }],
            // The host's queue and modes: not for guests to change.
            Command::PlayQueueIndex(_) | Command::SetShuffle(_) | Command::SetRepeat(_) => {
                self.emit(Event::Problem(Problem::JamNotAllowed));
                return true;
            }
            _ => return false,
        };
        for request in requests {
            self.jam_send(SessionCommand::Send(ToHost::Request(request)));
        }
        true
    }

    fn start_jam(&mut self, link: Option<String>) {
        self.jam_gen += 1;
        let generation = self.jam_gen;
        let (session, role) = match link {
            None => (
                Session::host(self.jam_network),
                Role::Host(Host {
                    link: None,
                    people: Vec::new(),
                    perms: Perms::default(),
                    epoch: 0,
                    barrier: None,
                    added_by: HashMap::new(),
                    last_anchor: None,
                    resync: None,
                }),
            ),
            Some(link) => {
                let saved = self.session();
                // The Jam's tracks replace whatever played; it comes back after.
                self.to_audio(sc_audio::Command::Stop);
                self.current = None;
                self.pending_restore = None;
                self.queue.set_context(Vec::new(), 0);
                self.emit(Event::Queue(self.queue.snapshot()));
                self.set_state(PlayState::Idle);
                (
                    Session::join(&link, self.jam_profile()),
                    Role::Guest(Guest {
                        host_name: String::new(),
                        host_user: None,
                        connected: false,
                        perms: Perms::default(),
                        people: Vec::new(),
                        me: None,
                        offset_ns: None,
                        epoch: 0,
                        ready: false,
                        ready_sent: false,
                        anchor: None,
                        lead: DEFAULT_LEAD,
                        strikes: 0,
                        last_correction: None,
                        saved: Some(saved),
                    }),
                )
            }
        };
        let events = session.events().clone();
        let inputs = self.inputs.clone();
        let pump = tokio::spawn(async move {
            while let Ok(event) = events.recv_async().await {
                let _ = inputs.send(Input::Jam { generation, event });
            }
        });
        self.jam = Some(Jam {
            session,
            generation,
            pump,
            role,
            awaiting_position: None,
            play_at: None,
            play_at_token: 0,
        });
        if let Some(Jam {
            role: Role::Host(_),
            ..
        }) = &self.jam
        {
            // Whatever plays is already the Jam's; guests catch up as they join.
            self.host_broadcast_queue();
        }
        self.emit_jam();
    }

    /// Ends this side of the Jam, telling the person why when `problem`.
    fn leave_jam(&mut self, problem: Option<Problem>) {
        let Some(mut jam) = self.jam.take() else {
            return;
        };
        jam.pump.abort();
        if let Some(task) = jam.play_at.take() {
            task.abort();
        }
        jam.session.leave();
        match jam.role {
            Role::Guest(guest) => {
                self.to_audio(sc_audio::Command::Stop);
                self.current = None;
                self.queue.set_context(Vec::new(), 0);
                self.set_state(PlayState::Idle);
                match guest.saved {
                    Some(saved) if !saved.tracks.is_empty() => self.restore(saved),
                    _ => self.emit(Event::Queue(self.queue.snapshot())),
                }
            }
            Role::Host(host) => {
                // A track prepared for the Jam but not started yet plays now.
                if host.barrier.is_some() || host.resync == Some(true) {
                    self.to_audio(sc_audio::Command::Play);
                }
            }
        }
        self.emit(Event::Jam(None));
        if let Some(problem) = problem {
            self.emit(Event::Problem(problem));
        }
    }

    fn emit_jam(&self) {
        let Some(jam) = &self.jam else {
            return self.emit(Event::Jam(None));
        };
        let state =
            match &jam.role {
                Role::Host(host) => JamState {
                    role: JamRole::Host,
                    link: host.link.clone(),
                    people: host
                        .people
                        .iter()
                        .map(|p| JamPerson {
                            id: p.id.0,
                            name: p.name.clone(),
                            user: p.user_id.map(UserId),
                            host: false,
                            cannot_play: p.cannot_play,
                        })
                        .collect(),
                    guests_control_playback: host.perms.guests_control_playback,
                    connecting: host.link.is_none(),
                },
                Role::Guest(guest) => {
                    JamState {
                        role: JamRole::Guest {
                            host: guest.host_name.clone(),
                        },
                        link: None,
                        // The host first (id 0: guests are numbered from 1), then the
                        // other guests.
                        people: (!guest.host_name.is_empty())
                            .then(|| JamPerson {
                                id: 0,
                                name: guest.host_name.clone(),
                                user: guest.host_user.map(UserId),
                                host: true,
                                cannot_play: false,
                            })
                            .into_iter()
                            .chain(guest.people.iter().filter(|p| Some(p.id) != guest.me).map(
                                |p| JamPerson {
                                    id: p.id.0,
                                    name: p.name.clone(),
                                    user: p.user_id.map(UserId),
                                    host: false,
                                    cannot_play: p.cannot_play.is_some(),
                                },
                            ))
                            .collect(),
                        guests_control_playback: guest.perms.guests_control_playback,
                        connecting: !guest.connected,
                    }
                }
            };
        self.emit(Event::Jam(Some(state)));
    }

    pub(super) fn jam_event(&mut self, generation: u64, event: SessionEvent) {
        if self
            .jam
            .as_ref()
            .is_none_or(|jam| jam.generation != generation)
        {
            return;
        }
        match event {
            SessionEvent::Started { link } => {
                if let Some(Jam {
                    role: Role::Host(host),
                    ..
                }) = &mut self.jam
                {
                    host.link = Some(link);
                }
                self.emit_jam();
            }
            SessionEvent::PeerJoined { peer, profile } => self.host_peer_joined(peer, profile),
            SessionEvent::PeerLeft { peer } => {
                if let Some(Jam {
                    role: Role::Host(host),
                    ..
                }) = &mut self.jam
                {
                    host.people.retain(|p| p.id != peer);
                    if let Some(barrier) = &mut host.barrier {
                        barrier.waiting.remove(&peer);
                    }
                }
                self.host_broadcast_peers();
                self.host_check_barrier();
                self.emit_jam();
            }
            SessionEvent::FromGuest { peer, message } => self.host_message(peer, message),
            SessionEvent::Connected => {
                if let Some(Jam {
                    role: Role::Guest(guest),
                    ..
                }) = &mut self.jam
                {
                    guest.connected = true;
                }
                self.emit_jam();
            }
            SessionEvent::FromHost(message) => self.guest_message(message),
            SessionEvent::ClockOffset { offset_ns, .. } => {
                if let Some(Jam {
                    role: Role::Guest(guest),
                    ..
                }) = &mut self.jam
                {
                    guest.offset_ns = Some(offset_ns);
                }
                self.guest_follow();
            }
            SessionEvent::Ended(ended) => {
                let problem = match ended {
                    Ended::Left => None,
                    Ended::ByHost(EndReason::HostLeft) | Ended::Lost => Some(Problem::JamEnded),
                    Ended::ByHost(EndReason::Removed) => Some(Problem::JamRemoved),
                    Ended::ByHost(EndReason::Full) => Some(Problem::JamFull),
                    Ended::ByHost(EndReason::Version) => Some(Problem::JamVersion),
                    Ended::BadLink => Some(Problem::JamBadLink),
                    Ended::Unreachable(detail) => {
                        tracing::warn!(%detail, "the Jam could not connect");
                        Some(Problem::JamUnreachable)
                    }
                };
                self.leave_jam(problem);
            }
        }
    }

    pub(super) fn jam_timer(&mut self, generation: u64, timer: JamTimer) {
        let Some(jam) = &mut self.jam else {
            return;
        };
        if jam.generation != generation {
            return;
        }
        match timer {
            JamTimer::ReadyTimeout { epoch } => {
                if let Role::Host(Host {
                    barrier: Some(barrier),
                    ..
                }) = &mut jam.role
                    && barrier.epoch == epoch
                {
                    barrier.timed_out = true;
                    self.host_check_barrier();
                }
            }
            JamTimer::PlayAt { token } => {
                if token == jam.play_at_token {
                    jam.play_at = None;
                    self.to_audio(sc_audio::Command::Play);
                }
            }
        }
    }

    /// Plays at `at_ns` in this session's clock (now if it has passed).
    fn play_at(&mut self, at_ns: i64) {
        let now = self.jam_now_ns() as i64;
        let generation = self.jam_gen;
        let inputs = self.inputs.clone();
        let Some(jam) = &mut self.jam else {
            return;
        };
        if let Some(task) = jam.play_at.take() {
            task.abort();
        }
        jam.play_at_token += 1;
        let token = jam.play_at_token;
        let wait = from_ns(at_ns - now);
        jam.play_at = Some(tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            let _ = inputs.send(Input::JamTimer {
                generation,
                timer: JamTimer::PlayAt { token },
            });
        }));
    }

    fn cancel_play_at(&mut self) {
        if let Some(jam) = &mut self.jam
            && let Some(task) = jam.play_at.take()
        {
            task.abort();
        }
    }

    /// Instead of loading a stream to play at once, a Jam prepares it paused
    /// at the position. Returns whether it did.
    pub(super) fn jam_prepare(&mut self, source: &sc_audio::Source, at: Option<Duration>) -> bool {
        let Some(jam) = &mut self.jam else {
            return false;
        };
        jam.awaiting_position = Some(Instant::now());
        self.to_audio(sc_audio::Command::Prepare {
            source: source.clone(),
            at: at.unwrap_or_default(),
        });
        true
    }

    /// A track starts on this side (`play_summary`). The host gathers everyone.
    pub(super) fn jam_track_starts(&mut self, id: TrackId, at: Option<Duration>) {
        let generation = self.jam_gen;
        let inputs = self.inputs.clone();
        let Some(Jam {
            role: Role::Host(host),
            ..
        }) = &mut self.jam
        else {
            return;
        };
        host.epoch += 1;
        host.resync = None;
        let epoch = host.epoch;
        let pos = at.unwrap_or_default();
        for person in &mut host.people {
            person.cannot_play = false;
        }
        host.barrier = Some(Barrier {
            epoch,
            waiting: host.people.iter().map(|p| p.id).collect(),
            local_ready: false,
            timed_out: false,
            pos,
        });
        self.cancel_play_at();
        self.jam_send(SessionCommand::Broadcast(ToGuest::Prepare {
            epoch,
            track: id.0,
            pos_ms: ms(pos),
        }));
        tokio::spawn(async move {
            tokio::time::sleep(READY_TIMEOUT).await;
            let _ = inputs.send(Input::JamTimer {
                generation,
                timer: JamTimer::ReadyTimeout { epoch },
            });
        });
        self.host_broadcast_peers();
        self.emit_jam();
    }

    /// The audio state changed: a host tells the guests it paused.
    pub(super) fn jam_state_changed(&mut self, state: PlayState) {
        let Some(Jam {
            role: Role::Host(host),
            ..
        }) = &self.jam
        else {
            return;
        };
        if state == PlayState::Paused && host.barrier.is_none() && host.resync.is_none() {
            let paused = ToGuest::Paused {
                epoch: host.epoch,
                pos_ms: ms(self.playback.position),
            };
            self.cancel_play_at();
            self.jam_send(SessionCommand::Broadcast(paused));
        }
    }

    /// A position from the audio. Returns whether the Jam owns the track's
    /// end: a guest never moves on by itself.
    pub(super) fn jam_position(&mut self, position: Duration) {
        let Some(jam) = &mut self.jam else {
            return;
        };
        if let Some(since) = jam.awaiting_position.take() {
            let took = since.elapsed();
            match &mut jam.role {
                Role::Host(host) => {
                    if let Some(barrier) = &mut host.barrier {
                        barrier.local_ready = true;
                        self.host_check_barrier();
                    } else if let Some(play) = host.resync.take() {
                        self.host_announce(position, play);
                    }
                }
                Role::Guest(guest) => {
                    guest.ready = true;
                    guest.lead = (took * 3 / 2).clamp(DEFAULT_LEAD / 2, MAX_LEAD);
                    if !guest.ready_sent {
                        guest.ready_sent = true;
                        let epoch = guest.epoch;
                        self.jam_send(SessionCommand::Send(ToHost::Ready { epoch }));
                    }
                    self.guest_follow();
                }
            }
            return;
        }
        if self.playback.state != PlayState::Playing {
            return;
        }
        match &mut jam.role {
            Role::Host(host) => {
                if host.last_anchor.is_none_or(|at| at.elapsed() >= HEARTBEAT) {
                    host.last_anchor = Some(Instant::now());
                    let anchor = ToGuest::Playing {
                        epoch: host.epoch,
                        pos_ms: ms(position),
                        at_host_ns: jam.session.clock().now_ns(),
                    };
                    self.jam_send(SessionCommand::Broadcast(anchor));
                }
            }
            Role::Guest(_) => self.guest_drift(position),
        }
    }

    /// Whether the audio's track end belongs to the Jam (a guest waits for
    /// the host's next track instead of moving on).
    pub(super) fn jam_owns_track_end(&self) -> bool {
        self.is_jam_guest()
    }

    /// A guest whose track cannot play tells the host and stays silent.
    /// Returns whether the Jam took the failure.
    pub(super) fn jam_play_failed(&mut self, problem: &Problem) -> bool {
        let Some(Jam {
            role: Role::Guest(guest),
            ..
        }) = &self.jam
        else {
            return false;
        };
        let reason = match problem {
            Problem::PreviewOnly => Unplayable::Preview,
            Problem::CannotPlay => Unplayable::Blocked,
            _ => Unplayable::Failed,
        };
        let message = ToHost::CannotPlay {
            epoch: guest.epoch,
            track: self.current.map_or(0, |t| t.0),
            reason,
        };
        self.jam_send(SessionCommand::Send(message));
        true
    }

    // Host

    fn host_peer_joined(&mut self, peer: PeerId, profile: Profile) {
        self.remember_avatar(profile.user_id, profile.avatar_url.clone());
        let me = self.jam_profile();

        let now = self.jam_now_ns();
        let Some(Jam {
            role: Role::Host(host),
            ..
        }) = &mut self.jam
        else {
            return;
        };
        host.people.push(Person {
            id: peer,
            name: profile.name,
            user_id: profile.user_id,
            avatar_url: profile.avatar_url,
            cannot_play: false,
        });
        let welcome = ToGuest::Welcome {
            proto_minor: sc_session::protocol::PROTO_MINOR,
            host: me.name,
            host_user_id: me.user_id,
            host_avatar: me.avatar_url,
            perms: host.perms,
            you: peer,
        };
        let epoch = host.epoch;
        let in_barrier = host.barrier.is_some();
        self.jam_send(SessionCommand::SendTo(peer, welcome));
        self.jam_send(SessionCommand::SendTo(peer, self.host_queue_message()));
        // Joining mid-track: prepare where the track will be, then follow.
        if let Some(track) = self.current
            && !in_barrier
        {
            let playing = self.playback.state == PlayState::Playing;
            let pos = self.playback.position;
            let ahead = if playing { pos + DEFAULT_LEAD * 2 } else { pos };
            self.jam_send(SessionCommand::SendTo(
                peer,
                ToGuest::Prepare {
                    epoch,
                    track: track.0,
                    pos_ms: ms(ahead),
                },
            ));
            let anchor = if playing {
                ToGuest::Playing {
                    epoch,
                    pos_ms: ms(pos),
                    at_host_ns: now,
                }
            } else {
                ToGuest::Paused {
                    epoch,
                    pos_ms: ms(pos),
                }
            };
            self.jam_send(SessionCommand::SendTo(peer, anchor));
        }
        self.host_broadcast_peers();
        self.emit_jam();
    }

    fn host_message(&mut self, peer: PeerId, message: ToHost) {
        match message {
            ToHost::Ready { epoch } => {
                if let Some(Jam {
                    role: Role::Host(host),
                    ..
                }) = &mut self.jam
                    && let Some(barrier) = &mut host.barrier
                    && barrier.epoch == epoch
                {
                    barrier.waiting.remove(&peer);
                }
                self.host_check_barrier();
            }
            ToHost::CannotPlay { epoch, .. } => {
                if let Some(Jam {
                    role: Role::Host(host),
                    ..
                }) = &mut self.jam
                    && host.epoch == epoch
                {
                    if let Some(person) = host.people.iter_mut().find(|p| p.id == peer) {
                        person.cannot_play = true;
                    }
                    if let Some(barrier) = &mut host.barrier {
                        barrier.waiting.remove(&peer);
                    }
                }
                self.host_broadcast_peers();
                self.host_check_barrier();
                self.emit_jam();
            }
            ToHost::Request(request) => self.host_request(peer, request),
            ToHost::Hello { .. } | ToHost::Ping { .. } | ToHost::Bye => {}
        }
    }

    fn host_request(&mut self, peer: PeerId, request: Request) {
        let allowed = match &self.jam {
            Some(Jam {
                role: Role::Host(host),
                ..
            }) => request.always_allowed() || host.perms.guests_control_playback,
            _ => return,
        };
        if !allowed {
            self.jam_send(SessionCommand::SendTo(peer, ToGuest::Denied { request }));
            return;
        }
        let command = match request {
            Request::AddToQueue { track } => return self.host_add(peer, TrackId(track), false),
            Request::PlayNext { track } => return self.host_add(peer, TrackId(track), true),
            Request::TogglePlay => Command::TogglePlay,
            Request::Next => Command::Next,
            Request::Previous => Command::Previous,
            Request::Seek { ms } => Command::Seek(Duration::from_millis(ms)),
            Request::Remove { index } => Command::RemoveFromQueue(index as usize),
            Request::Move { from, to } => Command::MoveInQueue {
                from: from as usize,
                to: to as usize,
            },
        };
        self.command(command);
    }

    /// A guest's track joins the queue; one the host has not seen is fetched.
    fn host_add(&mut self, peer: PeerId, id: TrackId, next: bool) {
        if self.find_summary(id).is_none() {
            let (api, inputs, generation) =
                (Arc::clone(&self.api), self.inputs.clone(), self.jam_gen);
            tokio::spawn(async move {
                let result = api.track(id.0).await.map(Box::new);
                let _ = inputs.send(Input::JamTrack {
                    generation,
                    then: AfterFetch::Enqueue { peer, next },
                    result,
                });
            });
            return;
        }
        if let Some(Jam {
            role: Role::Host(host),
            ..
        }) = &mut self.jam
        {
            host.added_by.insert(id, peer);
        }
        self.enqueue(id, next);
    }

    pub(super) fn jam_track_fetched(
        &mut self,
        generation: u64,
        then: AfterFetch,
        result: sc_api::Result<Box<Track>>,
    ) {
        if generation != self.jam_gen {
            return;
        }
        let track = match result {
            Ok(track) => *track,
            Err(error) => return self.emit(Event::Problem(Problem::from_api(&error))),
        };
        let id = TrackId(track.id);
        self.tracks.insert(id, track);
        match then {
            AfterFetch::Enqueue { peer, next } => self.host_add(peer, id, next),
            AfterFetch::Prepare { epoch, pos } => self.guest_prepare(epoch, id, pos),
        }
    }

    fn host_queue_message(&self) -> ToGuest {
        let snapshot = self.queue.snapshot();
        let added_by = match &self.jam {
            Some(Jam {
                role: Role::Host(host),
                ..
            }) => Some(&host.added_by),
            _ => None,
        };
        ToGuest::Queue {
            tracks: snapshot
                .tracks
                .iter()
                .map(|t| QueuedTrack {
                    track: t.id.0,
                    added_by: added_by.and_then(|by| by.get(&t.id).copied()),
                })
                .collect(),
            current: snapshot.current.map(|i| i as u32),
        }
    }

    /// The queue changed: a host shows the guests.
    pub(super) fn host_broadcast_queue(&self) {
        if let Some(Jam {
            role: Role::Host(_),
            ..
        }) = &self.jam
        {
            self.jam_send(SessionCommand::Broadcast(self.host_queue_message()));
        }
    }

    fn host_broadcast_peers(&self) {
        let Some(Jam {
            role: Role::Host(host),
            ..
        }) = &self.jam
        else {
            return;
        };
        let peers = host
            .people
            .iter()
            .map(|p| PeerInfo {
                id: p.id,
                name: p.name.clone(),
                user_id: p.user_id,
                avatar_url: p.avatar_url.clone(),
                cannot_play: p.cannot_play.then_some(Unplayable::Failed),
            })
            .collect();
        self.jam_send(SessionCommand::Broadcast(ToGuest::Peers { peers }));
    }

    /// Starts everyone once the host and every guest are ready (or waiting
    /// took too long).
    fn host_check_barrier(&mut self) {
        let Some(Jam {
            role: Role::Host(host),
            ..
        }) = &mut self.jam
        else {
            return;
        };
        let Some(barrier) = &host.barrier else {
            return;
        };
        if !barrier.local_ready || !(barrier.waiting.is_empty() || barrier.timed_out) {
            return;
        }
        let pos = barrier.pos;
        host.barrier = None;
        self.host_announce(pos, true);
    }

    /// Tells the guests where the track is from a moment ahead, and plays (or
    /// stays paused) here at that same moment.
    fn host_announce(&mut self, pos: Duration, play: bool) {
        let start = self.jam_now_ns() + START_DELAY.as_nanos() as u64;
        let Some(Jam {
            role: Role::Host(host),
            ..
        }) = &mut self.jam
        else {
            return;
        };
        let epoch = host.epoch;
        host.last_anchor = Some(Instant::now());
        let anchor = if play {
            ToGuest::Playing {
                epoch,
                pos_ms: ms(pos),
                at_host_ns: start,
            }
        } else {
            ToGuest::Paused {
                epoch,
                pos_ms: ms(pos),
            }
        };
        self.jam_send(SessionCommand::Broadcast(anchor));
        if play {
            self.play_at(start as i64);
        }
    }

    /// Resume or seek in a Jam: pause, move, and start everyone together.
    fn host_resync(&mut self, at: Duration, play: bool) {
        let Some(jam) = &mut self.jam else {
            return;
        };
        if let Role::Host(host) = &mut jam.role {
            host.resync = Some(play);
        }
        jam.awaiting_position = Some(Instant::now());
        self.cancel_play_at();
        self.to_audio(sc_audio::Command::Pause);
        self.to_audio(sc_audio::Command::Seek(at));
    }

    // Guest

    fn guest_message(&mut self, message: ToGuest) {
        let Some(Jam {
            role: Role::Guest(guest),
            ..
        }) = &mut self.jam
        else {
            return;
        };
        match message {
            ToGuest::Welcome {
                host,
                host_user_id,
                host_avatar,
                perms,
                you,
                ..
            } => {
                guest.host_name = host;
                guest.host_user = host_user_id;
                guest.perms = perms;
                guest.me = Some(you);
                self.remember_avatar(host_user_id, host_avatar);
                self.emit_jam();
            }
            ToGuest::Queue { tracks, current } => self.guest_queue(tracks, current),
            ToGuest::Prepare {
                epoch,
                track,
                pos_ms,
            } => {
                guest.epoch = epoch;
                guest.ready = false;
                guest.ready_sent = false;
                guest.strikes = 0;
                if guest.anchor.is_some_and(|a| a.epoch != epoch) {
                    guest.anchor = None;
                }
                self.cancel_play_at();
                self.guest_prepare(epoch, TrackId(track), Duration::from_millis(pos_ms));
            }
            ToGuest::Playing {
                epoch,
                pos_ms,
                at_host_ns,
            } => {
                let anchor = Anchor {
                    epoch,
                    playing: true,
                    pos: Duration::from_millis(pos_ms),
                    at_host_ns,
                };
                // A heartbeat while playing only feeds the drift check.
                let heartbeat = guest.anchor.is_some_and(|a| a.playing && a.epoch == epoch)
                    && self.playback.state == PlayState::Playing;
                guest.anchor = Some(anchor);
                if !heartbeat {
                    self.guest_follow();
                }
            }
            ToGuest::Paused { epoch, pos_ms } => {
                guest.anchor = Some(Anchor {
                    epoch,
                    playing: false,
                    pos: Duration::from_millis(pos_ms),
                    at_host_ns: 0,
                });
                self.guest_follow();
            }
            ToGuest::Peers { peers } => {
                let avatars: Vec<_> = peers
                    .iter()
                    .map(|p| (p.user_id, p.avatar_url.clone()))
                    .collect();
                guest.people = peers;
                for (user, url) in avatars {
                    self.remember_avatar(user, url);
                }
                self.emit_jam();
            }
            ToGuest::Perms(perms) => {
                guest.perms = perms;
                self.emit_jam();
            }
            ToGuest::Denied { .. } => self.emit(Event::Problem(Problem::JamNotAllowed)),
            ToGuest::Pong { .. } | ToGuest::Ended { .. } => {}
        }
    }

    /// The host's queue: tracks this guest has not seen are fetched first.
    fn guest_queue(&mut self, tracks: Vec<QueuedTrack>, current: Option<u32>) {
        let ids: Vec<u64> = tracks.iter().map(|t| t.track).collect();
        let missing: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| !self.tracks.contains_key(&TrackId(*id)))
            .collect();
        if missing.is_empty() {
            return self.guest_mirror(&ids, current);
        }
        let (api, inputs, generation) = (Arc::clone(&self.api), self.inputs.clone(), self.jam_gen);
        tokio::spawn(async move {
            let result = api.tracks(&missing).await;
            let _ = inputs.send(Input::JamMirror {
                generation,
                ids,
                current,
                result,
            });
        });
    }

    pub(super) fn jam_mirror_fetched(
        &mut self,
        generation: u64,
        ids: Vec<u64>,
        current: Option<u32>,
        result: sc_api::Result<Vec<Track>>,
    ) {
        if generation != self.jam_gen || !self.is_jam_guest() {
            return;
        }
        match result {
            Ok(tracks) => {
                for track in tracks {
                    self.tracks.insert(TrackId(track.id), track);
                }
            }
            Err(error) => tracing::warn!(%error, "could not fetch the Jam's queue"),
        }
        self.guest_mirror(&ids, current);
    }

    fn guest_mirror(&mut self, ids: &[u64], current: Option<u32>) {
        let summaries: Vec<TrackSummary> = ids
            .iter()
            .filter_map(|id| self.find_summary(TrackId(*id)))
            .collect();
        let start = current.map_or(0, |i| i as usize);
        let art: Vec<TrackId> = summaries.iter().map(|t| t.id).collect();
        self.queue.set_context(summaries, start);
        self.emit(Event::Queue(self.queue.snapshot()));
        // Tracks fetched for the mirror were never on a list here.
        for id in art {
            self.request_artwork(ArtKey::Track(id));
        }
    }

    /// The host prepared a track: load it here, paused at `pos`.
    fn guest_prepare(&mut self, epoch: u64, id: TrackId, pos: Duration) {
        let Some(summary) = self.find_summary(id) else {
            let (api, inputs, generation) =
                (Arc::clone(&self.api), self.inputs.clone(), self.jam_gen);
            tokio::spawn(async move {
                let result = api.track(id.0).await.map(Box::new);
                let _ = inputs.send(Input::JamTrack {
                    generation,
                    then: AfterFetch::Prepare { epoch, pos },
                    result,
                });
            });
            return;
        };
        let index = self.queue.snapshot().tracks.iter().position(|t| t.id == id);
        if let Some(index) = index {
            self.queue.play_index(index);
            self.emit(Event::Queue(self.queue.snapshot()));
        }
        self.play_summary(summary, Some(pos), false);
    }

    /// Follows the host's last anchor once ready and the clocks are known.
    fn guest_follow(&mut self) {
        let now = self.jam_now_ns() as i64;
        let Some(Jam {
            role: Role::Guest(guest),
            ..
        }) = &self.jam
        else {
            return;
        };
        let (Some(anchor), Some(offset)) = (guest.anchor, guest.offset_ns) else {
            return;
        };
        if anchor.epoch != guest.epoch || !guest.ready {
            return;
        }
        let lead = guest.lead;
        let host_now = now + offset;
        let local = self.playback.position;
        if !anchor.playing {
            self.cancel_play_at();
            if self.playback.state == PlayState::Playing {
                self.to_audio(sc_audio::Command::Pause);
            }
            if local.abs_diff(anchor.pos) > DRIFT_LIMIT {
                self.guest_seek_paused(anchor.pos);
            }
            return;
        }
        if self.playback.state == PlayState::Playing {
            return;
        }
        // Paused at `local`: the host's clock reaches that point at `when`.
        let when = anchor.at_host_ns as i64 + ns(local) - ns(anchor.pos);
        if when >= host_now - ns(ON_TIME) {
            self.play_at(when - offset);
        } else {
            let target = anchor.pos + from_ns(host_now + ns(lead) - anchor.at_host_ns as i64);
            self.guest_correct(target);
        }
    }

    /// While playing, compares with where the host is.
    fn guest_drift(&mut self, position: Duration) {
        let now = self.jam_now_ns() as i64;
        let Some(Jam {
            role: Role::Guest(guest),
            ..
        }) = &mut self.jam
        else {
            return;
        };
        let (Some(anchor), Some(offset)) = (guest.anchor, guest.offset_ns) else {
            return;
        };
        if !anchor.playing || anchor.epoch != guest.epoch {
            return;
        }
        let host_now = now + offset;
        let expected = ns(anchor.pos) + host_now - anchor.at_host_ns as i64;
        let error = (ns(position) - expected).unsigned_abs();
        if error <= DRIFT_LIMIT.as_nanos() as u64 {
            guest.strikes = 0;
            return;
        }
        guest.strikes += 1;
        let cooled = guest
            .last_correction
            .is_none_or(|at| at.elapsed() >= CORRECTION_COOLDOWN);
        if guest.strikes >= DRIFT_STRIKES && cooled {
            let target = from_ns(expected + ns(guest.lead));
            self.guest_correct(target);
        }
    }

    /// Pauses, seeks to `target` (a little ahead of the host) and plays when
    /// the host gets there.
    fn guest_correct(&mut self, target: Duration) {
        if let Some(Jam {
            role: Role::Guest(guest),
            ..
        }) = &mut self.jam
        {
            guest.strikes = 0;
            guest.last_correction = Some(Instant::now());
        }
        self.cancel_play_at();
        self.to_audio(sc_audio::Command::Pause);
        self.guest_seek_paused(target);
    }

    fn guest_seek_paused(&mut self, at: Duration) {
        let Some(jam) = &mut self.jam else {
            return;
        };
        if let Role::Guest(guest) = &mut jam.role {
            guest.ready = false;
        }
        jam.awaiting_position = Some(Instant::now());
        self.to_audio(sc_audio::Command::Seek(at));
    }
}
