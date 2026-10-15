# Contributing to Notercrux

Thanks for helping. Notercrux is pre-alpha; the most valuable contributions right now are bug
reports, focused fixes, tests, and design feedback on the [ADRs](docs/decisions/README.md).

## Setup

See [docs/development](docs/development/README.md). In short: `pnpm install`, then `pnpm dev`.

## Conventions

- **Small, verifiable changes.** One concern per pull request.
- **Tests with behaviour.** A feature is not done because it compiles. Editor, persistence and sync
  changes need tests of observable behaviour, including failure paths (data-loss bugs live there).
- **Rules live in Rust.** Page-tree, storage, crypto and sync rules belong in `crates/`; the Tauri
  command layer and the UI stay thin. `packages/document-model` must stay in step with
  `crates/domain`.
- **Never drop content silently.** Unknown nodes, newer document versions and failed saves must
  surface an error.
- **No custom cryptography.** Use established libraries and standard constructions; justify new
  dependencies (licence, maintenance, platform support, security history).
- **Docs are part of the change.** Update docs and ADRs in the same PR, and never document
  something as available that is not implemented.
- **UI:** check light and dark, keyboard-only use, reduced transparency and reduced motion.

## Before opening a PR

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
```

By contributing you agree your work is licensed under AGPL-3.0-only.
