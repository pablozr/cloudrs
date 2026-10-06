# ADR 0006: Listen together over peer-to-peer

Status: accepted direction · 2026-10-06 (the library is chosen after a spike)

## Context

People want to listen with friends the way Spotify Jam works: the host creates a link,
friends open it and join, everyone hears the same track at the same moment, and guests can add
to a shared queue. Spotify keeps those sessions on its own servers. cloudrs has no servers and
should stay free to run.

## Decisions

1. **Sync state, never audio.** Each person plays the track from SoundCloud in their own
   cloudrs. Only the session state travels: track, position, play/pause, queue. Relaying audio
   between people would be redistribution and is out of scope for good (see `PLAN.md` §11,
   DMCA risk).
2. **Peer-to-peer, no cloudrs server.** The host's app is the session. The invite link carries
   everything needed to reach the host (its address and fallback routes), so no service of ours
   stores sessions. Public relays of the chosen library may be used when a direct connection
   cannot be made; they only forward encrypted traffic.
3. **The host is the authority.** The host's `sc-core` owns the queue and playback state.
   Guests send requests (add a track, skip, pause if allowed); the host applies them and
   broadcasts the new state. When the host leaves, the session ends.
4. **Remote commands use the existing contract.** A request from a guest becomes the same
   `Command` the UI sends, so `sc-core` does not care where a "play" came from.
5. **New layer.** Networking lives in a new crate (working name `sc-session`) below `sc-core`,
   next to `sc-api` and `sc-audio`, and does not depend on either.
6. **Joining.** Guests paste the invite link into the search field, as they already do with
   SoundCloud links. A `cloudrs://` link that opens the app goes with OS integration (M4).
7. **Sync.** Estimate each guest's clock offset to the host, start playback only when everyone
   has buffered, and correct drift with a seek when it passes about 300 ms.
8. **Different rights per person.** A track may be full for the host and a GO+ preview, or
   blocked by region, for a guest. The session shows who cannot play it instead of failing.

## Open (needs the maintainer's approval)

- **The P2P library.** Candidate: `iroh` (QUIC with hole punching, public relays as a fallback,
  dial by node id). A spike must confirm NAT traversal between two home networks, binary size
  and idle cost before it becomes a dependency.
- The session protocol (message format, versioning) and the guest permissions the host can
  toggle.

## Consequences

- No hosting cost, and no cloudrs service to keep up.
- A session exists only while the host's app is open.
- Depends on the M2 queue; planned after M2.
