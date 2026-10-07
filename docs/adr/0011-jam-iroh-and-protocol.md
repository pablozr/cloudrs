# ADR 0011: Jam — iroh, the session crate and protocol v1

Status: accepted · 2026-10-07 (options chosen by the maintainer). Builds on
[ADR 0006](./0006-listen-together-p2p.md), which set the direction: sync state, never audio;
peer-to-peer with no cloudrs server; the host is the authority.

## Context

ADR 0006 left the P2P library and the protocol open until a spike. The spike (2026-10-07, both
peers on one Windows 11 PC against n0's public relays) compared iroh, rust-libp2p, WebRTC
(str0m/webrtc-rs), a hosted relay of our own, magic-wormhole and veilid.

Measured:

- **Connection:** 4–9 ms with a full ticket (direct). With a relay-only ticket, 0.6–0.8 s over
  the relay, then upgraded to a direct path within the first pings. A steady relay RTT of
  130–145 ms (US-East).
- **Idle cost:** a host endpoint with no guests used 0.000 s of CPU in 30 s (about 4.7 MB of
  private memory). With one guest, about 0.1 % of one core and about 27 B/s each way.
- **Size:** +8.0 MB on a release binary with the same reqwest/rustls set as cloudrs (+6.5 MB
  without `portmapper`); 69 crates new to cloudrs's `Cargo.lock`.
- **Ticket:** relay-only `EndpointTicket` 122 characters (a full one, with this PC's seven
  addresses, 300).

Not verified: traversal between two real home networks, CGNAT or mobile, hour-long relay-only
sessions, a network change mid-session, macOS and Linux.

Rejected: rust-libp2p (DCUtR about 70 % success and no public relay meant for third-party
apps), WebRTC (needs a signaling server or copying offers both ways, and TURN for symmetric
NAT), our own relay (a cloudrs server, against ADR 0006 §2), magic-wormhole (copyleft,
one-shot transfers through a mailbox server), veilid (heavy for this).

How Spotify Jam behaves, for the UX: the host starts it and shares a link; guests can always
add tracks; playback control by guests is a host toggle; up to 32 people; the Jam ends when the
host leaves.

## Decisions

1. **Library: `iroh` 1.3 and `iroh-tickets` 1.0, behind a Cargo feature `jam`.** The feature is
   on in release builds; a build without it has no Jam and none of iroh's crates. iroh's
   features: no `metrics`, no `fast-apple-datapath`, `tls-aws-lc-rs` (the provider reqwest
   already uses), `portmapper` kept. The workspace MSRV moves to 1.91.
2. **n0's public relays only; no DNS publishing.** The endpoint is built with the relay map and
   without pkarr/DNS discovery, and the link carries a relay-only ticket. Nothing about a
   session is published to `dns.iroh.link`, and no LAN or public address leaks into a chat.
   The relay map is configurable, so a self-hosted `iroh-relay` needs no protocol change.
3. **New crate `sc-session`**, below `sc-core` next to `sc-api` and `sc-audio`, depending on
   neither. It runs on its own thread with a current-thread Tokio runtime, exists only while a
   Jam is active, and talks to `sc-core` over flume (`SessionCommand` in, `SessionEvent` out);
   no async types cross. It owns framing, the ping clock and the endpoint; it knows tracks only
   as ids.
4. **Topology:** a star around the host. One QUIC connection and one bidirectional stream per
   guest.
5. **Protocol v1:** frames are a little-endian `u32` length and a JSON body (`serde_json`,
   already in the workspace), at most 64 KiB. The major version is the ALPN, `cloudrs/jam/1`;
   minor additions are fields with `#[serde(default)]`. Messages: `Hello`, `Welcome`,
   `Ping`/`Pong`, `Queue`, `Prepare`, `Ready`, `CannotPlay`, `Playing`, `Paused`, `Peers`,
   `Request`, `Denied`, `Bye`, `Ended`. Tracks travel as SoundCloud ids: every peer fetches
   metadata and streams with its own client and rights.
6. **Invite link:** `cloudrs:jam/<relay-only ticket>`, pasted into the search field. The host
   uses a fresh key per Jam, so the link is an unguessable capability that dies with the
   session.
7. **Permissions:** adding to the queue is always allowed; one toggle, "guests control
   playback" (play/pause, skip, seek, reorder), off by default. The host can remove a guest.
   At most **16** people per Jam.
8. **Sync:** NTP-style clock offset (eight pings at join, lowest RTT kept, again every 30 s).
   The host sends playback anchors on every change and at 1 Hz. A track starts when every
   guest has it loaded paused at the position (`Prepare` → `Ready`, 5 s timeout), at a shared
   instant 300 ms ahead. A guest more than 300 ms off on two samples corrects with a paused
   seek ahead and a scheduled play, at most every 10 s. A guest who cannot play a track (GO+
   preview, region) is shown to the host and stays silent until the next track.
9. **`sc-audio`:** `Command::Load` gains a way to load paused, so preparing a track does not
   depend on draining `Load` and `Pause` in the same loop.
10. **When it ends:** a guest's pre-Jam queue and session are saved at join and restored,
    paused, when the Jam ends or the guest leaves. While in a Jam the guest's queue mirrors the
    host's, and the guest's own queue and playback commands become requests to the host.
11. **Order:** `sc-session` with a loopback test, then the `sc-audio` change, the host mode
    in `sc-core`, the guest mode, then the UI (Jam panel, copy link, people, cannot-play
    badges, Ctrl K). The two-home traversal test runs before the UI ships.

## Consequences

- Release builds grow about 8 MB and depend on n0's free relays, which n0 labels for
  development and hobby use with no SLA and supports only on the latest iroh. cloudrs follows
  iroh releases; self-hosting `iroh-relay` is the fallback.
- No cloudrs server and no cost; a Jam exists only while the host's app is open.
- Output latency (Bluetooth) is invisible to peers; a manual offset can come later.
- Progressive MP3 sources seek slowly; sync treats them as "may lag".
