# Guidelines for agents and contributors

## Pillars

- **Performance and low cost come first.** Between two equivalent solutions, pick the cheaper
  one in idle CPU, memory, network calls and dependencies.
  - Render only what is on screen (virtual lists).
  - Load the rest on demand.
  - Keep heavy work off the UI thread.
  - Never clone collections inside `render`.
- **Audio never stutters.** Nothing allocates or locks in the cpal callback. Audio does not
  depend on the UI or the network.

## Language

- **Everything in the repository is in English:** code, comments, docs, commit messages.
- Interface text comes from `crate::i18n`, never a literal in a screen
  ([docs/design/i18n.md](docs/design/i18n.md)).

## Workflow

- Commit as soon as each change is done and validated. Small commits, one topic each,
  Conventional Commits style.
- Include the code, tests and docs for a change in the same commit.
- All documentation lives in `docs/`; keep [docs/README.md](docs/README.md) up to date.
  Durable decisions become an ADR in `docs/adr/`.
- Push only to the session's working branch. Pull requests and merges only when asked.
- Record validation limits (a check the environment could not run) in the commit message.

## UI (GPUI)

- Read [docs/design/VISUAL-IDENTITY.md](docs/design/VISUAL-IDENTITY.md) before creating or
  changing a screen. If something does not fit the system, propose a change instead of a local
  variant.
- Only `apps/cloudrs` and `crates/cloudrs-ui` may depend on `gpui`, pinned to the Zed revision
  in [ADR 0001](docs/adr/0001-gpui-pinned-to-zed.md).
- No raw values: colors only in `cloudrs-ui/src/tokens.rs` (in both themes), spacing, radii,
  type and motion through tokens.
- Complete states on every surface: loading (skeleton), empty, recoverable error, confirmation
  (toast).
- Keyboard and accessibility:
  - visible focus;
  - `aria_label` on every control;
  - a tooltip on icon-only controls;
  - every screen reachable from the command palette (`Ctrl K`).

## Layering

- The UI sends commands to `sc-core` and renders its snapshots. It never calls `sc-api` or
  `sc-audio` directly, and holds no business rules.
- `sc-api` and `sc-audio` do not depend on each other.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
