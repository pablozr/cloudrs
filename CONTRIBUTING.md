# Contributing to cloudrs

Thanks for helping! cloudrs is early, so discussion is as valuable as code.

## Before you start

- For anything larger than a small fix, open an issue first so we can agree on the approach.
- Read [docs/PLAN.md](docs/PLAN.md) for the architecture and
  [docs/design/VISUAL-IDENTITY.md](docs/design/VISUAL-IDENTITY.md) before touching the UI.

## Ground rules

- **English everywhere:** code, comments, docs, commit messages, issues and PRs.
- **Interface text goes through `i18n`**, never as a literal in a screen
  ([docs/design/i18n.md](docs/design/i18n.md)).
- **No raw design values in screens.** Colors, sizes, radii and durations come from
  `cloudrs-ui` tokens.
- **Stay within fair use.** No downloading, DRM circumvention, ad removal or GO+ bypass. Such PRs
  will be closed.

## Commits

Small commits, one topic each, using [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(audio): seek within HLS segments
fix(api): refresh client_id on 401
docs: describe the queue model
```

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI runs the same checks on Linux, macOS and Windows.
