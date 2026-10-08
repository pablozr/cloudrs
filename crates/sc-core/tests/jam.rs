#![cfg(feature = "jam")]
//! A Jam between two cores in this process (ADR 0011): a real session over
//! loopback, fake SoundCloud and fake audio engines.

mod common;

use std::time::{Duration, Instant};

use common::*;
use sc_core::{ArtKey, Command, Event, JamRole, JamState, Problem, TrackId};

const WAIT: Duration = Duration::from_secs(15);

/// The next audio command matching `pick`, skipping the others.
fn audio(h: &Harness, pick: impl Fn(&sc_audio::Command) -> bool) -> sc_audio::Command {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let command = h
            .audio_commands
            .recv_timeout(left)
            .expect("expected audio command did not arrive");
        if pick(&command) {
            return command;
        }
    }
}

/// Waits for `Prepare` and answers like the engine: paused, buffered at the position.
fn prepared(h: &Harness) -> Duration {
    let at = match audio(h, |c| matches!(c, sc_audio::Command::Prepare { .. })) {
        sc_audio::Command::Prepare { at, .. } => at,
        _ => unreachable!(),
    };
    send_audio(h, sc_audio::Event::State(sc_audio::PlaybackState::Paused));
    send_audio(h, sc_audio::Event::Position(at));
    at
}

fn send_audio(h: &Harness, event: sc_audio::Event) {
    h.audio_events.send(event).unwrap();
}

/// Waits for `Play` and answers that it plays. Returns when it came.
fn plays(h: &Harness) -> Instant {
    audio(h, |c| matches!(c, sc_audio::Command::Play));
    let at = Instant::now();
    send_audio(h, sc_audio::Event::State(sc_audio::PlaybackState::Playing));
    at
}

fn jam(h: &Harness, pick: impl Fn(&JamState) -> bool) -> JamState {
    h.wait(|e| match e {
        Event::Jam(Some(state)) if pick(&state) => Some(state),
        _ => None,
    })
}

/// A host and a guest in the same Jam.
fn pair(name: &str) -> (Harness, Harness) {
    let host = Harness::new(&format!("{name}-host"));
    let guest = Harness::new(&format!("{name}-guest"));
    host.core.send(Command::StartJam);
    let link = jam(&host, |s| s.link.is_some()).link.unwrap();
    assert!(sc_core::is_jam_link(&link));
    guest.core.send(Command::JoinJam(link));
    jam(
        &guest,
        |s| matches!(&s.role, JamRole::Guest { host } if !host.is_empty()),
    );
    jam(&host, |s| s.people.len() == 1);
    (host, guest)
}

#[test]
fn host_and_guest_start_a_track_together() {
    let (host, guest) = pair("together");
    host.search();
    host.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    // Both prepare the track paused at its start, then play at the same instant.
    assert_eq!(prepared(&host), Duration::ZERO);
    assert_eq!(prepared(&guest), Duration::ZERO);
    let host_at = plays(&host);
    let guest_at = plays(&guest);
    let apart = host_at.max(guest_at) - host_at.min(guest_at);
    assert!(apart < Duration::from_millis(100), "{apart:?} apart");
    // The guest's queue mirrors the host's.
    let queue = guest.wait(|e| match e {
        Event::Queue(queue) if !queue.tracks.is_empty() => Some(queue),
        _ => None,
    });
    assert_eq!(queue.tracks[0].id, TrackId(1));
}

#[test]
fn a_late_guest_catches_up_at_the_right_moment() {
    let host = Harness::new("late-host");
    host.core.send(Command::StartJam);
    let link = jam(&host, |s| s.link.is_some()).link.unwrap();
    host.search();
    host.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    prepared(&host);
    plays(&host);
    send_audio(&host, sc_audio::Event::Position(Duration::from_secs(30)));

    let guest = Harness::new("late-guest");
    guest.core.send(Command::JoinJam(link));
    // Prepared ahead of the host, then started once the host reaches it.
    let at = prepared(&guest);
    assert!(at > Duration::from_secs(30), "{at:?}");
    let asked = Instant::now();
    plays(&guest);
    assert!(
        asked.elapsed() > Duration::from_millis(500),
        "played too early"
    );
}

#[test]
fn guests_add_tracks_and_need_permission_for_the_rest() {
    let (host, guest) = pair("requests");
    guest.core.send(Command::AddToQueue(TrackId(7)));
    // The host had never seen track 7: it fetches it and, with nothing
    // playing, starts it for everyone.
    prepared(&host);
    assert!(host.api.calls().iter().any(|c| c == "stream 7"));

    guest.core.send(Command::Next);
    let problem = guest.wait(|e| match e {
        Event::Problem(problem) => Some(problem),
        _ => None,
    });
    assert_eq!(problem, Problem::JamNotAllowed);

    host.core.send(Command::SetJamGuestsControl(true));
    jam(&guest, |s| s.guests_control_playback);
}

#[test]
fn when_the_host_leaves_the_guest_is_told() {
    let (host, guest) = pair("host-leaves");
    host.core.send(Command::LeaveJam);
    host.wait(|e| matches!(e, Event::Jam(None)).then_some(()));
    guest.wait(|e| matches!(e, Event::Jam(None)).then_some(()));
    guest.wait(|e| matches!(e, Event::Problem(Problem::JamEnded)).then_some(()));
}

#[test]
fn a_guest_who_leaves_is_gone_for_the_host() {
    let (host, guest) = pair("guest-leaves");
    guest.core.send(Command::LeaveJam);
    guest.wait(|e| matches!(e, Event::Jam(None)).then_some(()));
    jam(&host, |s| s.people.is_empty());
}

#[test]
fn a_bad_link_ends_the_join() {
    let guest = Harness::new("bad-link");
    guest.core.send(Command::JoinJam("cloudrs:jam/nope".into()));
    guest.wait(|e| matches!(e, Event::Problem(Problem::JamBadLink)).then_some(()));
}

#[test]
fn a_guest_that_drifts_seeks_back_in_step() {
    let (host, guest) = pair("drift");
    host.search();
    host.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    prepared(&host);
    prepared(&guest);
    plays(&host);
    plays(&guest);
    // Ten seconds off, twice: the guest pauses and seeks to where the host
    // will be a moment later.
    for _ in 0..2 {
        send_audio(&guest, sc_audio::Event::Position(Duration::from_secs(10)));
    }
    audio(&guest, |c| matches!(c, sc_audio::Command::Pause));
    send_audio(
        &guest,
        sc_audio::Event::State(sc_audio::PlaybackState::Paused),
    );
    let at = match audio(&guest, |c| matches!(c, sc_audio::Command::Seek(_))) {
        sc_audio::Command::Seek(at) => at,
        _ => unreachable!(),
    };
    assert!(at < Duration::from_secs(5), "{at:?}");
    // Once there, it plays again on its own.
    send_audio(&guest, sc_audio::Event::Position(at));
    plays(&guest);
}

#[test]
fn a_guest_sees_the_host_first() {
    let (_host, guest) = pair("people");
    let state = jam(&guest, |s| !s.people.is_empty());
    assert!(state.people[0].host, "{:?}", state.people);
    assert_eq!(state.people[0].id, 0);
}

#[test]
fn a_guest_gets_the_covers_of_the_whole_queue() {
    let (host, guest) = pair("covers");
    host.search();
    host.core.send(Command::Play {
        list: TRACKS,
        track: TrackId(1),
    });
    // The guest never saw track 2: it comes with the host's queue, cover too.
    guest.wait(|e| match e {
        Event::Artwork {
            key: ArtKey::Track(TrackId(2)),
            ..
        } => Some(()),
        _ => None,
    });
}
