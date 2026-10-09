# ADR 0026: In-app updates

Status: accepted · 2026-10-08 (options chosen by the maintainer)

ADRs 0023 to 0025 are reserved for the mini player, the tray icon and the pt-BR translation,
which are not done yet. This ADR takes 0026 so those numbers keep their place.

## Context

Beta 1 shipped as installers on GitHub Releases (ADR 0012) and signed with SignPath (ADR 0016).
A person who installed it has no way to learn about, or get, the next build except by visiting
the page. cloudrs should update itself, quietly, without a server of its own and without
weakening the pillars: no idle cost, no surprises, and nothing runs that was not signed by us.

## Decisions

1. **D1: our own thin updater in `sc-platform::update`**, with one new crate, `minisign-verify`
   0.3 (pure Rust, no dependencies of its own that the lock lacks). The rest reuses what the
   workspace already has: `reqwest` (same features as `sc-audio`), `serde`, `serde_json`,
   `semver`, `base64` and `url`. `cargo-packager-updater` was rejected (see Alternatives).
2. **D2: layering.** The code lives in `sc-platform::update`, which knows nothing of the core.
   The app shell drives it, like Discord and the media controls (ADR 0015, ADR 0019).
   `sc-core` only stores the `auto_update` setting (schema v6, on by default) and does nothing
   with it.
3. **D3: when it runs.** A new version is downloaded and verified in the background; then a
   "ready" toast offers Restart. Restart closes the core, saves the session, installs and
   relaunches. A plain quit installs silently and does not relaunch. Failures of automatic
   checks are silent (a log line); a manual check from Settings or the command palette says what
   happened.
4. **D4: one channel.** The manifest is
   `https://github.com/pablozr/cloudrs/releases/latest/download/latest.json`. CI creates the
   GitHub Release as a **draft**; the maintainer publishes it. Betas are published as normal
   releases, not as pre-releases, because GitHub's "latest" ignores pre-releases.

### The manifest

A static `latest.json`, compatible with the format `cargo-packager-updater` reads:

```json
{
  "version": "0.1.0-beta.2",
  "notes": "…",
  "pub_date": "2026-10-09T12:00:00Z",
  "platforms": {
    "windows-x86_64": { "url": "https://…/cloudrs_0.1.0-beta.2_x64-setup.exe", "signature": "<base64 of the .sig>", "format": "nsis" },
    "linux-x86_64":   { "url": "https://…/cloudrs_0.1.0-beta.2_x86_64.AppImage", "signature": "…", "format": "appimage" },
    "macos-aarch64":  { "url": "https://…/cloudrs_0.1.0-beta.2_aarch64.app.tar.gz", "signature": "…", "format": "app" }
  }
}
```

- `version` may carry a leading `v`; it is parsed as SemVer. Only a version **strictly greater**
  than the running one counts, so a manifest can never downgrade an install, and
  `0.1.0-beta.10 > 0.1.0-beta.9` and `0.1.0 > 0.1.0-beta.9` hold.
- Platform keys are `windows-x86_64`, `linux-x86_64`, `macos-aarch64` and `macos-x86_64`. A
  platform missing from the manifest (Intel macOS, for now) only shows "available" with a link.
- The manifest is read with a 30 s timeout and a 64 KB cap.

### Security rules

- **The signature is checked before anything runs**, with the public key compiled into the app
  (`PUBLIC_KEY` in `sc-platform/src/update.rs`). The package is streamed to disk while a
  minisign verifier consumes it (the signatures are prehashed, so the whole file is never
  held in memory). `Ready` is never sent for an unverified file, and a failed check deletes the
  partial file.
- **The trusted comment is bound to the file.** cargo-packager writes `file:<name>` in the
  trusted comment. It must equal the last segment of the download URL and the name must contain
  `_<version>_`. This stops a valid signature of an old build from being replayed as the new one
  (a rollback with a genuine signature).
- **The installer is verified again from disk** right before it runs (NSIS, macOS), since the
  file sat in a user-writable folder between download and install.
- **URLs must be `https`**; plain `http` is accepted only for a loopback host, which is the seam
  the tests use.
- **A size limit of 300 MB** and a 30 minute timeout on the download.
- **An empty `PUBLIC_KEY` turns the updater off**, like an empty `APP_ID` turns Discord off.
  Debug builds never update.

### Installing

| Platform | How it is detected | Install |
|---|---|---|
| Windows | `uninstall.exe` next to the executable (the NSIS installer writes it) | run the installer: `/P /R /NS` to relaunch, `/S /NS` to stay closed. In both modes it ends a running `cloudrs.exe`. No PowerShell, and the app does not `exit` by itself |
| Linux AppImage | `APPIMAGE` is set, the file exists and its folder is writable | the verified file is swapped over the AppImage, with its permissions, right after the download (a rename in the same folder); relaunch runs the new file |
| macOS | the executable lives in `X.app/Contents/MacOS` and the folder holding `X.app` is writable | unpack the `.app.tar.gz` next to the bundle, rename the old one to `.old`, move the new one in, delete `.old`; the rename is undone on failure |
| Anything else (`.deb`, a read-only folder) | none of the above | only a notice with a link to the release page |

Detection runs on the update thread, never at startup.

### Privacy and schedule

- First check 10 s after start, then when 24 h have passed since the last one (the shell looks
  once an hour, so a laptop that slept catches up). A clock that moved back counts as due.
- One GET to GitHub for a small file. The only thing it carries is cloudrs's version in the
  User-Agent. Nothing is sent about the person or their listening.
- Off with one switch in Settings › Updates; "Check for updates" in the palette checks on demand.

## Consequences

- One new crate, `minisign-verify`, in `sc-platform`.
- **The first real update is beta.2 to beta.3.** Beta 1 has no updater and must be updated by
  hand once. The code in beta.2 needs the real public key in `PUBLIC_KEY` and a published
  release with a `latest.json` before it can be tried end to end.
- The update key is separate from code signing. **Losing it means installed copies refuse every
  future update**; keep it and its password in a password manager.
- The minisign signature is made after Authenticode signing on Windows, since SignPath changes
  the file.
- Limits:
  - Windows: a SmartScreen reputation warning can still appear until the signed installers
    build reputation (ADR 0016);
  - macOS: the app is not notarized, so Gatekeeper still applies to the first install, and
    only Apple Silicon has an update package (Intel gets the link);
  - `.deb`: only notifies, since replacing a system package needs the package manager.
- Real install paths (NSIS, AppImage swap outside the tests, macOS bundle swap) are only
  exercised by an end-to-end update after a release exists.

## Alternatives

- **`cargo-packager-updater` 0.2.3.** Same manifest format and key tooling, but it calls
  `std::process::exit(0)` right after starting the Windows installer (the app could not save its
  session first), does `set_var("SSL_CERT_FILE")` on Linux (process-wide, unsound with other
  threads running) and adds 21 crates to the lock, against one for our own code.
- **Velopack.** A complete system with delta updates, but it replaces the installers and the
  packaging chain from ADR 0012 and ADR 0016 and needs its own CLI and runtime.
- **Notify only** (a link to the release page). Cheapest, but it leaves the person to download
  and run the installer by hand. It remains the behavior of the `.deb`, of platforms with no
  package and of builds where the key is empty.

## Releasing

1. `cargo packager signer generate --path "$HOME/.cloudrs/update.key"`. Keep the key and its
   password in a password manager: losing the key means installed copies refuse future updates.
2. `gh secret set UPDATE_SIGNING_KEY --repo pablozr/cloudrs < "$HOME/.cloudrs/update.key"` and
   `gh secret set UPDATE_SIGNING_KEY_PASSWORD --repo pablozr/cloudrs`.
3. Put the contents of `update.key.pub` into `PUBLIC_KEY`
   (`crates/sc-platform/src/update.rs`) and commit.
4. Move CHANGELOG Unreleased to `## 0.1.0-beta.N · <date>`, bump `version` in `Cargo.toml`, run
   `cargo check`, and commit.
5. `git tag v0.1.0-beta.N && git push origin v0.1.0-beta.N`. CI builds, signs, writes
   `latest.json` and creates a draft release.
6. Review the draft, then `gh release edit v0.1.0-beta.N --draft=false --latest`.
7. End-to-end test: build an older version locally with `cargo packager --release --formats
   nsis`, install it, then Ctrl P, "Check for updates", Restart.
