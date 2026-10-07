//! The host: accepts guests, answers their clock pings, and passes the rest
//! between them and `sc-core`.

use std::collections::HashMap;
use std::time::Duration;

use iroh::endpoint::{Connection, Incoming};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use crate::net;
use crate::protocol::{EndReason, PeerId, ToGuest, ToHost};
use crate::{Clock, Ended, MAX_GUESTS, Network, SessionCommand, SessionEvent};

/// How long a new connection may take to say hello.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// How long goodbyes may take to reach the guests when the host leaves.
const GOODBYE_TIMEOUT: Duration = Duration::from_secs(2);

/// From the guests' tasks to the host loop.
enum FromPeer {
    Hello {
        peer: PeerId,
        profile: crate::Profile,
        writer: mpsc::UnboundedSender<ToGuest>,
    },
    Gone {
        peer: PeerId,
    },
}

pub async fn run(
    network: Network,
    clock: Clock,
    commands: flume::Receiver<SessionCommand>,
    events: flume::Sender<SessionEvent>,
    mut stop: oneshot::Receiver<()>,
) -> Ended {
    let endpoint = match net::endpoint(network, true).await {
        Ok(endpoint) => endpoint,
        Err(error) => return Ended::Unreachable(error),
    };
    if network == Network::Internet
        && tokio::time::timeout(net::ONLINE_TIMEOUT, endpoint.online())
            .await
            .is_err()
    {
        endpoint.close().await;
        return Ended::Unreachable("no relay could be reached".into());
    }
    let _ = events.send(SessionEvent::Started {
        link: net::link(&endpoint, network),
    });

    let (from_peers, mut from_peers_rx) = mpsc::unbounded_channel();
    let mut tasks = JoinSet::new();
    let mut peers: HashMap<PeerId, mpsc::UnboundedSender<ToGuest>> = HashMap::new();
    let mut next_peer = 1;
    let ended = loop {
        tokio::select! {
            _ = &mut stop => break Ended::Left,
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break Ended::Lost };
                let peer = PeerId(next_peer);
                next_peer += 1;
                tasks.spawn(serve(peer, incoming, clock, from_peers.clone(), events.clone()));
            }
            command = commands.recv_async() => match command {
                Ok(SessionCommand::Broadcast(message)) => {
                    for writer in peers.values() {
                        let _ = writer.send(message.clone());
                    }
                }
                Ok(SessionCommand::SendTo(peer, message)) => {
                    if let Some(writer) = peers.get(&peer) {
                        let _ = writer.send(message);
                    }
                }
                Ok(SessionCommand::Kick(peer)) => {
                    if let Some(writer) = peers.remove(&peer) {
                        let _ = writer.send(ToGuest::Ended { reason: EndReason::Removed });
                        let _ = events.send(SessionEvent::PeerLeft { peer });
                    }
                }
                Ok(SessionCommand::Send(_)) => {}
                // The core dropped its side: same as leaving.
                Err(_) => break Ended::Left,
            },
            Some(message) = from_peers_rx.recv() => match message {
                FromPeer::Hello { peer, profile, writer } => {
                    if peers.len() >= MAX_GUESTS {
                        let _ = writer.send(ToGuest::Ended { reason: EndReason::Full });
                        continue;
                    }
                    peers.insert(peer, writer);
                    let _ = events.send(SessionEvent::PeerJoined { peer, profile });
                }
                FromPeer::Gone { peer } => {
                    if peers.remove(&peer).is_some() {
                        let _ = events.send(SessionEvent::PeerLeft { peer });
                    }
                }
            },
        }
    };

    for writer in peers.values() {
        let _ = writer.send(ToGuest::Ended {
            reason: EndReason::HostLeft,
        });
    }
    peers.clear();
    let _ = tokio::time::timeout(GOODBYE_TIMEOUT, tasks.join_all()).await;
    endpoint.close().await;
    ended
}

/// One guest's connection: the hello, then its frames both ways until either
/// side stops. Pings are answered here, so the clock never waits on the core.
async fn serve(
    peer: PeerId,
    incoming: Incoming,
    clock: Clock,
    host: mpsc::UnboundedSender<FromPeer>,
    events: flume::Sender<SessionEvent>,
) {
    let Ok(Ok(connection)) = tokio::time::timeout(HELLO_TIMEOUT, incoming).await else {
        return;
    };
    let Ok(Ok((mut send, mut recv))) =
        tokio::time::timeout(HELLO_TIMEOUT, connection.accept_bi()).await
    else {
        return;
    };
    let profile = match tokio::time::timeout(HELLO_TIMEOUT, net::read::<ToHost>(&mut recv)).await {
        Ok(Ok(ToHost::Hello {
            name,
            user_id,
            avatar_url,
            ..
        })) => crate::Profile {
            name,
            user_id,
            avatar_url,
        },
        _ => return,
    };
    let (writer, mut outgoing) = mpsc::unbounded_channel();
    let pong = writer.clone();
    if host
        .send(FromPeer::Hello {
            peer,
            profile,
            writer,
        })
        .is_err()
    {
        return;
    }

    let write = async {
        while let Some(message) = outgoing.recv().await {
            let last = matches!(message, ToGuest::Ended { .. });
            if net::write(&mut send, &message).await.is_err() {
                return;
            }
            if last {
                // The guest closes once it reads the goodbye.
                let _ = send.finish();
                let _ = tokio::time::timeout(GOODBYE_TIMEOUT, connection.closed()).await;
                return;
            }
        }
    };
    let read = async {
        loop {
            match net::read::<ToHost>(&mut recv).await {
                Ok(ToHost::Ping { t0 }) => {
                    let _ = pong.send(ToGuest::Pong {
                        t0,
                        th: clock.now_ns(),
                    });
                }
                Ok(ToHost::Bye) | Err(_) => return,
                // A second hello is ignored.
                Ok(ToHost::Hello { .. }) => {}
                Ok(message) => {
                    let _ = events.send(SessionEvent::FromGuest { peer, message });
                }
            }
        }
    };
    tokio::select! {
        () = write => {}
        () = read => {}
    }
    let _ = host.send(FromPeer::Gone { peer });
    close(&connection);
}

fn close(connection: &Connection) {
    connection.close(0u32.into(), b"bye");
}
