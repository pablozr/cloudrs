# ADR 0015: Who's in the Jam, Discord Rich Presence, and richer new playlists

Status: accepted · 2026-10-07 (options chosen by the maintainer)

## Context

Three requests before the beta:
- creating a playlist should be more than a name;
- a Jam should show, at a glance and nicely, who is in it;
- cloudrs should show what plays on Discord, and look good there.

## Decisions

1. **New playlists take:**
   - a name;
   - a description;
   - public or private;
   - a genre and tags;
   - a cover of the person's own (an image picked from disk), or SoundCloud's default, the
     first track's.

   On their own playlist the owner can change the description and cover later too.
2. **The cover upload** follows soundcloud.com's web app: `PUT /playlists/{urn}/artwork` with
   `{"image_data": "<base64 of the file>"}` (JSON, no multipart), after the playlist exists.
   The description, genre and tags go in the playlist body (`description`, `genre`,
   `tag_list`). None of it is verified live; the maintainer tries it.
3. **Jam presence: a pill in the title bar.** While in a Jam it shows stacked avatars of
   everyone (the host first, with a crown) and how many listen; a click opens the Jam screen,
   which shows a card per person: avatar, name, host or guest, and whether they cannot play
   the current track.
4. **Avatars in the Jam protocol.** `Hello` and `PeerInfo` gain an optional `avatar_url`
   (`#[serde(default)]`, so older builds still connect), and `Welcome` the host's. Without an
   account the UI draws the initial on a coloured circle. The images download through the
   core's artwork cache like every other cover.
5. **Discord Rich Presence with `discord-rich-presence` 1.x** (MIT, Discord's local IPC, no
   network of ours), in `sc-platform::discord`, **on by default**, with a toggle on the
   Account screen. It shows:
   - *Listening to* (activity type Listening) the track's title, and the artist below;
   - the track's cover as the large image, the cloudrs logo as the small one (with the play
     state as its tooltip);
   - start and end times, so Discord draws the progress bar;
   - buttons "Listen on SoundCloud" (the track's page) and "Get cloudrs";
   - in a Jam: "In a Jam" with the party size (people, and at most 16).

   Paused, the presence keeps the track without the bar; with nothing playing it is cleared.
   If Discord is not running, cloudrs retries quietly every 30 s. The Discord application
   (named "cloudrs") is registered by the maintainer; its id is public and lives in the code.

## Consequences

- One new dependency (`discord-rich-presence`), used only by `sc-platform`.
- `TrackSummary` gains the cover URL and the track's page, for Discord.
- What someone plays shows on their Discord profile by default; the Account toggle and
  Discord's own activity settings turn it off.
