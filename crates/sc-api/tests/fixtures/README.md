# Fixtures

- `search_tracks.json`: hand-written in the shape of `api-v2` (field names and nesting as
  returned by soundcloud.com in 2026). IDs, tokens and URLs are fake.
- `mixed_selections.json`, `chart_selections.json`, `system_playlist.json`: real responses
  captured signed out on 2026-10-07 (public curated playlists), trimmed to two items per list
  and without `media` or `track_authorization`.
- `track_comments.json`: hand-written in the 2026 `api-v2` shape, because it could not be
  captured live. IDs and URLs are fake.
