# ADR 0002: Use SoundCloud's internal api-v2 behind a trait

Status: accepted · 2026-10-06

## Context

The official SoundCloud API needs a manually reviewed app registration that is reported to
require a paid Artist Pro account, and it has no feed or recommendations. The internal
`api-v2.soundcloud.com`, used by the website, is free to call with the public `client_id` found
in the site's JavaScript, and it exposes everything the website does. It is unofficial and can
change without notice.

## Decision

- `sc-api` talks to `api-v2`, extracting and refreshing the `client_id` automatically.
- Signed-in features use the user's own `oauth_token`, stored in the OS keychain.
- All access goes through a `SoundCloudApi` trait, so an official-API backend can be added
  later without touching `sc-core` or the UI.
- The project stays within fair use: no downloads, no DRM circumvention, no GO+ bypass, and a
  clear "unofficial client" notice.

## Consequences

- No cost or approval wait to build and ship.
- Breakage is expected from time to time. Tolerant models, fixtures and a smoke test keep fixes
  quick.
