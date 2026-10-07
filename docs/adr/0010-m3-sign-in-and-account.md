# ADR 0010: M3 — sign-in and the user's account

Status: accepted · 2026-10-07 (all options chosen by the maintainer)

## Context

M3 adds the person's account: sign-in, Likes, Library, Feed, Following, like/unlike and
follow/unfollow. `api-v2` has no OAuth flow for third-party apps (ADR 0002): it accepts the
`oauth_token` cookie of soundcloud.com as `Authorization: OAuth <token>`. A browser keeps that
cookie to itself, so "sign in on the web and come back signed in" cannot go through the
person's default browser.

## Decisions

1. **Sign in with a webview window.** "Sign in" starts the same executable as a child process,
   `cloudrs --sign-in`. It opens a small native window with `https://soundcloud.com/signin`
   (`wry` on a `tao` event loop), private (no data kept on disk). It reads the `oauth_token`
   cookie when a page loads and once a second; when the cookie appears it prints the token on
   stdout and exits. Closing the window exits with no output (cancelled). A separate process
   keeps a second event loop out of the GPUI process and costs memory only while signing in.
2. **Paste a token as the fallback.** The account screen keeps "Other ways to sign in": a field
   for the `oauth_token` with a step-by-step guide, for "Sign in with Google" (Google refuses
   embedded webviews) or a Linux machine without WebKitGTK.
3. **`sc-platform` starts now**, with `keychain` (`keyring`) and `sign_in` (the webview window).
   It knows nothing of SoundCloud's API or the core. The app is the composition root: it reads
   the token from the keychain at start and passes it in `CoreConfig`, saves it when the core
   says `SignedIn`, and deletes it on `SignedOut`. `sc-core` stays free of OS code.
4. **The token swaps at runtime.** `SoundCloudApi::set_oauth_token(Option<String>)`; `ScClient`
   keeps it behind a `std::sync::RwLock` read once per request.
5. **Core contract.**
   - `CoreConfig { oauth_token: Option<String> }`. A token from the keychain is checked with
     `/me` at start.
   - `Command::SignIn(String)` checks the token with `/me`; `Command::SignOut` forgets it.
   - `Event::SignedIn(Account)` with `Account { user: UserSummary, token: String }`,
     `Event::SignedOut`, and `Problem::SignInFailed` / `Problem::SessionExpired` (a 401 on a
     signed-in call signs out).
   - Lists: `ListId::Feed` (`/stream`, tracks only for now), `ListId::Library`
     (`/me/library/all`, playlists and albums), `ListId::Following` (`/me/followings`).
     The person's likes are `ListId::UserLikes(me)`.
   - `Command::Like { track, liked }` and `Command::Follow { user, following }` update
     optimistically and answer `Event::Liked { track, liked }` / `Event::Followed { user,
     following }`; a failure reverts with a `Problem`. After sign-in the core loads the liked
     track ids and followed user ids and sends them as `Event::LikedIds` and
     `Event::FollowedIds`, so rows and pages can show the state.
6. **UI.** An account item at the bottom of the sidebar ("Sign in", or the avatar and name)
   opens the account screen: the sign-in button, the fallback, and "Sign out" once signed in.
   Likes, Library, Feed and Following join the sidebar when signed in. A heart on track rows,
   the track page and the player bar; a follow button on profiles. Every screen is reachable
   from `Ctrl K`.
7. **Order:** this ADR, then `sc-api`, `sc-core` with tests, `sc-platform`, and the app.

## Consequences

- New dependencies: `keyring` (OS keychain), `wry` and `tao` (sign-in window). On Linux the app
  needs WebKitGTK and GTK 3 (`libwebkit2gtk-4.1-dev libgtk-3-dev` to build); CI installs them.
  Windows 11 ships WebView2 and macOS ships WebKit.
- The token never touches the disk outside the keychain and is never logged.
- Embedded webviews can be refused by some identity providers; the paste fallback covers them.
