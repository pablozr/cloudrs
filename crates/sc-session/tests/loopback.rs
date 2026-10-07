#![cfg(feature = "jam")]
//! A host and guests on this machine, without relays or internet.

use std::time::Duration;

use sc_session::{
    EndReason, Ended, Network, PeerId, Perms, Profile, QueuedTrack, Request, Session,
    SessionCommand, SessionEvent, ToGuest, ToHost,
};

const WAIT: Duration = Duration::from_secs(20);

fn next<T>(session: &Session, mut pick: impl FnMut(SessionEvent) -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let event = session
            .events()
            .recv_timeout(left)
            .expect("expected session event did not arrive");
        if let Some(found) = pick(event) {
            return found;
        }
    }
}

fn host() -> (Session, String) {
    let host = Session::host(Network::Local);
    let link = next(&host, |e| match e {
        SessionEvent::Started { link } => Some(link),
        _ => None,
    });
    assert!(sc_session::is_link(&link), "{link}");
    (host, link)
}

/// Joins and waits until the host has the guest.
fn join(host: &Session, link: &str, name: &str) -> (Session, PeerId) {
    let guest = Session::join(
        link,
        Profile {
            name: name.into(),
            user_id: Some(9),
            avatar_url: Some("https://i1.sndcdn.com/avatars-9-large.jpg".into()),
        },
    );
    next(&guest, |e| {
        matches!(e, SessionEvent::Connected).then_some(())
    });
    let peer = next(host, |e| match e {
        SessionEvent::PeerJoined { peer, profile } => {
            assert_eq!(
                profile.user_id,
                Some(9),
                "the avatar travels with the hello"
            );
            assert_eq!(profile.name, name);
            Some(peer)
        }
        _ => None,
    });
    (guest, peer)
}

#[test]
fn a_guest_joins_talks_and_is_told_when_the_host_leaves() {
    let (mut host, link) = host();
    let (guest, peer) = join(&host, &link, "Ana");

    let welcome = ToGuest::Welcome {
        proto_minor: 0,
        host: "Bo".into(),
        host_user_id: None,
        host_avatar: None,
        perms: Perms::default(),
        you: peer,
    };
    host.send(SessionCommand::SendTo(peer, welcome.clone()));
    let got = next(&guest, |e| match e {
        SessionEvent::FromHost(message) => Some(message),
        _ => None,
    });
    assert_eq!(got, welcome);

    // The clock offset arrives without the core doing anything.
    let rtt = next(&guest, |e| match e {
        SessionEvent::ClockOffset { rtt, .. } => Some(rtt),
        _ => None,
    });
    assert!(rtt < Duration::from_secs(1), "{rtt:?}");

    guest.send(SessionCommand::Send(ToHost::Request(Request::AddToQueue {
        track: 42,
    })));
    let (from, message) = next(&host, |e| match e {
        SessionEvent::FromGuest { peer, message } => Some((peer, message)),
        _ => None,
    });
    assert_eq!(from, peer);
    assert_eq!(message, ToHost::Request(Request::AddToQueue { track: 42 }));

    let queue = ToGuest::Queue {
        tracks: vec![QueuedTrack {
            track: 42,
            added_by: Some(peer),
        }],
        current: Some(0),
    };
    host.send(SessionCommand::Broadcast(queue.clone()));
    let got = next(&guest, |e| match e {
        SessionEvent::FromHost(message @ ToGuest::Queue { .. }) => Some(message),
        _ => None,
    });
    assert_eq!(got, queue);

    host.leave();
    let ended = next(&guest, |e| match e {
        SessionEvent::Ended(ended) => Some(ended),
        _ => None,
    });
    assert_eq!(ended, Ended::ByHost(EndReason::HostLeft));
    let ended = next(&host, |e| match e {
        SessionEvent::Ended(ended) => Some(ended),
        _ => None,
    });
    assert_eq!(ended, Ended::Left);
}

#[test]
fn a_guest_who_leaves_is_gone_for_the_host() {
    let (host, link) = host();
    let (mut guest, peer) = join(&host, &link, "Ana");
    guest.leave();
    let left = next(&host, |e| match e {
        SessionEvent::PeerLeft { peer } => Some(peer),
        _ => None,
    });
    assert_eq!(left, peer);
    let ended = next(&guest, |e| match e {
        SessionEvent::Ended(ended) => Some(ended),
        _ => None,
    });
    assert_eq!(ended, Ended::Left);
}

#[test]
fn the_host_can_remove_a_guest() {
    let (host, link) = host();
    let (guest, peer) = join(&host, &link, "Ana");
    host.send(SessionCommand::Kick(peer));
    let ended = next(&guest, |e| match e {
        SessionEvent::Ended(ended) => Some(ended),
        _ => None,
    });
    assert_eq!(ended, Ended::ByHost(EndReason::Removed));
}

#[test]
fn a_link_that_is_not_a_jam_ends_at_once() {
    let guest = Session::join("cloudrs:jam/not-a-ticket", Profile::default());
    let ended = next(&guest, |e| match e {
        SessionEvent::Ended(ended) => Some(ended),
        _ => None,
    });
    assert_eq!(ended, Ended::BadLink);
    assert!(!sc_session::is_link("https://soundcloud.com/a"));
    assert!(
        !sc_session::is_link("cloudrs:jam/endpointab"),
        "half a link"
    );
}

/// Through n0's public relays, as the app runs. Needs internet, so it does
/// not run in CI: `cargo test -p sc-session -- --ignored`.
#[test]
#[ignore = "needs internet access to n0's relays"]
fn over_the_internet_relays() {
    let host = Session::host(Network::Internet);
    let link = next(&host, |e| match e {
        SessionEvent::Started { link } => Some(link),
        SessionEvent::Ended(ended) => panic!("host ended: {ended:?}"),
        _ => None,
    });
    // Only the relay travels in the link, never an address of this network.
    assert!(link.len() < 200, "{} chars: {link}", link.len());
    let (guest, peer) = join(&host, &link, "Ana");
    host.send(SessionCommand::SendTo(
        peer,
        ToGuest::Perms(Perms {
            guests_control_playback: true,
        }),
    ));
    let got = next(&guest, |e| match e {
        SessionEvent::FromHost(message) => Some(message),
        _ => None,
    });
    assert!(matches!(got, ToGuest::Perms(_)));
}
