# ADR 0001: Tauri 2 + React desktop shell

**Status:** Accepted

## Context

The desktop app needs a rich-text editor, native window materials (vibrancy on macOS, Mica on
Windows), SQLite, and a path to sharing Rust code with an iOS client. Memory and start-up time
matter more than any single feature.

## Options

- **Electron**: mature, but ships a full Chromium per app and has no first-class Rust core.
- **Tauri 2 + React**: system webview, Rust backend, built-in `windowEffects`, small binaries.
- **Fully native (SwiftUI/WinUI)**: best fidelity, but two desktop codebases and no mature
  ProseMirror-class editor.

## Decision

Tauri 2 with a React/TypeScript front end. Business rules (page tree, document model, storage,
crypto, sync) live in Rust crates under `crates/`; the Tauri command layer
(`apps/desktop/src-tauri/src/commands.rs`) is deliberately thin. The same crates are what an iOS
client would bind to later.

Tauri 3 is currently alpha; we pin to 2.x.

## Consequences

- Rendering uses the platform webview (WKWebView / WebView2), so CSS and editor behaviour must be
  verified on both macOS and Windows, not assumed from a Chromium preview.
- `macOSPrivateApi` is required for a transparent window. This API is rejected by the Mac App
  Store; direct distribution is assumed. Revisit if App Store distribution becomes a goal.
- The UI can run in a plain browser against a localStorage mock (`pnpm web`) for fast iteration.
  The mock is a convenience, not a behavioural reference. The Rust storage crate is.
