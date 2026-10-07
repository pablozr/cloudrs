# ADR 0012: 0.1.0 beta 1, installers and the release workflow

Status: accepted · 2026-10-07 (options chosen by the maintainer)

## Context

The maintainer wants a first public build, to try Jam between machines and to offer cloudrs
from a website. PLAN M5 named `cargo-dist` or `cargo-packager` for packages.

## Decisions

1. **Version `0.1.0-beta.1`** for the whole workspace. Later betas are `beta.2`, `beta.3`…, then
   `0.1.0`.
2. **`cargo-packager` 0.11.8** builds the installers from `[package.metadata.packager]` in
   `apps/cloudrs/Cargo.toml`. It is a build tool, not a dependency of the app.
   - Windows: an **NSIS** `setup.exe` that installs per user (`installMode = "currentUser"`, no
     administrator rights), with a Start menu shortcut and an uninstaller. `cargo-packager`
     downloads NSIS itself.
   - Linux: `.deb` (depending on WebKitGTK 4.1, GTK 3, ALSA, xkbcommon-x11 and Vulkan) and
     `.AppImage`.
   - macOS: `.dmg`.
3. **All three systems in CI.** `.github/workflows/release.yml` runs on a `v*` tag (or by hand)
   and attaches the installers to the run. Creating a GitHub Release from them stays a manual
   step for the maintainer. Linux and macOS packages are built but not tried by us for this
   beta.
4. **Release builds are GUI apps on Windows** (`windows_subsystem = "windows"` outside debug
   builds), so no console window opens next to cloudrs.
5. **Not code-signed yet.** SmartScreen and Gatekeeper warn on first launch; the README explains
   how to continue. Signing comes with M5 (see ADR 0016).

## Consequences

- One command per system (`cargo packager --release`) makes the installers locally too.
- The Windows installer of beta 1 is about 11 MB (a 32 MB executable).
- Unsigned installers look less trustworthy; this is accepted for a beta.

## Refinements: a branded installer

- The NSIS installer carries the brand: `assets/installer/sidebar.bmp` (164×314, welcome and
  finish pages: the icon over an orange glow, the wordmark, "SoundCloud, native.") and
  `header.bmp` (150×57, the other pages), drawn from the brand colours, the app icon and
  Bricolage Grotesque by `assets/installer/make-images.ps1`; the installer's icon is the
  app's.
- `app-icon-256.png`, `app-icon-512.png` and `app-icon.ico` were cut off at the bottom
  (transparent below about four fifths of the height). They were rendered again from
  `app-icon.svg` with resvg 0.46 (the ICO holds PNGs at 16, 24, 32, 48, 64, 128 and 256 px).
