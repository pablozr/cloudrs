# ADR 0001: GPUI pinned to a Zed revision, with our own UI kit

Status: accepted · 2026-10-06

## Context

cloudrs needs a native, GPU-rendered UI. GPUI (the framework behind Zed) is the choice. There
are three ways to depend on it:

1. `gpui` on crates.io, frozen at 0.2.2 (October 2025), far behind Zed.
2. Community snapshots (`gpui-pre`, `gpui-unofficial`), used by `gpui-component` 0.7.x.
3. A git dependency on the Zed monorepo, pinned to one revision.

The team already ships xemnas on option 3, pinned to
`244023605536a412ab6b8d5b658466b89fb15401` (includes AccessKit support, zed#56065).

## Decision

- Depend on `gpui` and `gpui_platform` from `https://github.com/zed-industries/zed`, pinned to
  the same `rev` as xemnas.
- Do not use `gpui-component`: it depends on `gpui-pre`, a different crate, and its types do not
  mix with Zed's `gpui`.
- Build our own design system crate, `crates/cloudrs-ui` (tokens, theme, motion, primitives),
  following the structure of xemnas's `ui/` module.
- Only `apps/cloudrs` and `crates/cloudrs-ui` may depend on GPUI. An architecture test will
  enforce it.

## Consequences

- We can reuse what the team learned (and code patterns) from xemnas.
- We write and maintain our own components instead of getting 75+ ready-made ones.
- GPUI upgrades are deliberate: bump the `rev` in a dedicated PR, ideally in step with xemnas.
