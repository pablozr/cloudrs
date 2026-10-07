//! The iroh side: endpoints, links and frames on a QUIC stream.

use std::time::Duration;

use iroh::endpoint::{RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode};
use iroh_tickets::endpoint::EndpointTicket;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::protocol::{self, ALPN};
use crate::{LINK_PREFIX, Network};

/// How long going online (reaching a relay) may take.
pub const ONLINE_TIMEOUT: Duration = Duration::from_secs(15);

/// A fresh endpoint, so every Jam has a new key and its link dies with it.
/// Relays without publishing to any DNS (ADR 0011 §2).
pub async fn endpoint(network: Network, accept: bool) -> Result<Endpoint, String> {
    let relay = match network {
        Network::Internet => RelayMode::Default,
        Network::Local => RelayMode::Disabled,
    };
    let mut builder = Endpoint::builder(presets::Minimal).relay_mode(relay);
    if accept {
        builder = builder.alpns(vec![ALPN.to_vec()]);
    }
    builder.bind().await.map_err(|e| e.to_string())
}

/// The link to this endpoint. On the internet it names only the relay: no
/// address of the host's network ends up in a chat.
pub fn link(endpoint: &Endpoint, network: Network) -> String {
    let addr = endpoint.addr();
    let addr = match network {
        Network::Internet => {
            EndpointAddr::from_parts(addr.id, addr.addrs.iter().filter(|a| a.is_relay()).cloned())
        }
        Network::Local => addr,
    };
    format!("{LINK_PREFIX}{}", EndpointTicket::new(addr))
}

/// Where a link points, or `None` when it is not a Jam link.
pub fn parse_link(link: &str) -> Option<EndpointAddr> {
    let ticket: EndpointTicket = link.strip_prefix(LINK_PREFIX)?.parse().ok()?;
    Some(ticket.endpoint_addr().clone())
}

pub async fn write<T: Serialize>(send: &mut SendStream, message: &T) -> Result<(), String> {
    send.write_all(&protocol::encode(message))
        .await
        .map_err(|e| e.to_string())
}

/// The next message, `Err` when the stream ends or a frame is malformed.
pub async fn read<T: DeserializeOwned>(recv: &mut RecvStream) -> Result<T, String> {
    let mut header = [0u8; 4];
    recv.read_exact(&mut header)
        .await
        .map_err(|e| e.to_string())?;
    let len = protocol::frame_len(header).ok_or("frame too large")?;
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}
