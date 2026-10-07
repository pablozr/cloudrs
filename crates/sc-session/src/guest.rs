//! The guest: dials the host from the link, keeps the clock offset, and
//! passes messages between the host and `sc-core`.

use std::time::Duration;

use iroh::endpoint::RecvStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::net;
use crate::protocol::{ALPN, PROTO_MINOR, ToGuest, ToHost};
use crate::{Clock, Ended, Network, SessionCommand, SessionEvent};

/// Pings sent right after joining, this far apart.
const FIRST_PINGS: u32 = 8;
const FIRST_PING_EVERY: Duration = Duration::from_millis(100);
/// Then one ping this often, to follow the clocks' drift.
const PING_EVERY: Duration = Duration::from_secs(30);
/// A best sample older than this is replaced by the next one, even if slower.
const SAMPLE_MAX_AGE: Duration = Duration::from_secs(300);
/// How long a goodbye may take to leave.
const BYE_TIMEOUT: Duration = Duration::from_secs(1);

pub async fn run(
    link: String,
    name: String,
    clock: Clock,
    commands: flume::Receiver<SessionCommand>,
    events: flume::Sender<SessionEvent>,
    mut stop: oneshot::Receiver<()>,
) -> Ended {
    let Some(addr) = net::parse_link(&link) else {
        return Ended::BadLink;
    };
    // A link naming a relay is an internet Jam; one with only addresses is local.
    let network = if addr.addrs.iter().any(|a| a.is_relay()) {
        Network::Internet
    } else {
        Network::Local
    };
    let endpoint = match net::endpoint(network, false).await {
        Ok(endpoint) => endpoint,
        Err(error) => return Ended::Unreachable(error),
    };
    let connection =
        match tokio::time::timeout(net::ONLINE_TIMEOUT, endpoint.connect(addr, ALPN)).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => {
                endpoint.close().await;
                return Ended::Unreachable(error.to_string());
            }
            Err(_) => {
                endpoint.close().await;
                return Ended::Unreachable("the host did not answer".into());
            }
        };
    let (mut send, recv) = match connection.open_bi().await {
        Ok(streams) => streams,
        Err(error) => return Ended::Unreachable(error.to_string()),
    };
    let hello = ToHost::Hello {
        proto_minor: PROTO_MINOR,
        name,
    };
    if net::write(&mut send, &hello).await.is_err() {
        return Ended::Lost;
    }
    let _ = events.send(SessionEvent::Connected);

    // Reading is not cancel-safe, so it runs on its own and feeds a channel.
    let (incoming_tx, mut incoming) = mpsc::unbounded_channel();
    let reader = tokio::spawn(read_all(recv, incoming_tx));

    let mut pings_sent = 0;
    let mut next_ping = Instant::now();
    let mut best: Option<Sample> = None;
    let ended = loop {
        tokio::select! {
            _ = &mut stop => {
                let _ = net::write(&mut send, &ToHost::Bye).await;
                let _ = send.finish();
                let _ = tokio::time::timeout(BYE_TIMEOUT, send.stopped()).await;
                break Ended::Left;
            }
            command = commands.recv_async() => match command {
                Ok(SessionCommand::Send(message)) => {
                    if net::write(&mut send, &message).await.is_err() {
                        break Ended::Lost;
                    }
                }
                Ok(_) => {}
                Err(_) => break Ended::Left,
            },
            () = tokio::time::sleep_until(next_ping) => {
                let ping = ToHost::Ping { t0: clock.now_ns() };
                if net::write(&mut send, &ping).await.is_err() {
                    break Ended::Lost;
                }
                pings_sent += 1;
                next_ping += if pings_sent < FIRST_PINGS { FIRST_PING_EVERY } else { PING_EVERY };
            }
            message = incoming.recv() => match message {
                Some(ToGuest::Pong { t0, th }) => {
                    let sample = Sample::new(t0, th, clock.now_ns());
                    if best.as_ref().is_none_or(|b| sample.better_than(b)) {
                        let _ = events.send(SessionEvent::ClockOffset {
                            offset_ns: sample.offset_ns,
                            rtt: sample.rtt,
                        });
                        best = Some(sample);
                    }
                }
                Some(ToGuest::Ended { reason }) => break Ended::ByHost(reason),
                Some(message) => {
                    let _ = events.send(SessionEvent::FromHost(message));
                }
                None => break Ended::Lost,
            },
        }
    };
    reader.abort();
    connection.close(0u32.into(), b"bye");
    endpoint.close().await;
    ended
}

async fn read_all(mut recv: RecvStream, out: mpsc::UnboundedSender<ToGuest>) {
    while let Ok(message) = net::read::<ToGuest>(&mut recv).await {
        if out.send(message).is_err() {
            return;
        }
    }
}

/// One NTP-style clock sample.
struct Sample {
    offset_ns: i64,
    rtt: Duration,
    taken: Instant,
}

impl Sample {
    /// `t0` sent and `t3` received in the guest's clock, `th` the host's
    /// clock when it answered. The host's clock reads `guest + offset`.
    fn new(t0: u64, th: u64, t3: u64) -> Self {
        let rtt = t3.saturating_sub(t0);
        let midpoint = t0 + rtt / 2;
        Self {
            offset_ns: th as i64 - midpoint as i64,
            rtt: Duration::from_nanos(rtt),
            taken: Instant::now(),
        }
    }

    /// A faster round trip is more exact; an old best gives way anyway.
    fn better_than(&self, best: &Sample) -> bool {
        self.rtt <= best.rtt || best.taken.elapsed() > SAMPLE_MAX_AGE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offset_is_measured_from_the_midpoint() {
        // Sent at 1000, answered at host time 5_000_100, back at 1200.
        let sample = Sample::new(1_000, 5_000_100, 1_200);
        assert_eq!(sample.rtt, Duration::from_nanos(200));
        assert_eq!(sample.offset_ns, 5_000_100 - 1_100);
        // A host clock behind the guest's gives a negative offset.
        assert_eq!(Sample::new(10_000, 2_000, 10_400).offset_ns, 2_000 - 10_200);
    }

    #[test]
    fn faster_samples_win() {
        let slow = Sample::new(0, 100, 1_000);
        let fast = Sample::new(0, 100, 200);
        assert!(fast.better_than(&slow));
        assert!(!slow.better_than(&fast));
    }
}
