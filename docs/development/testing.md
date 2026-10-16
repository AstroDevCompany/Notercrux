# Testing

## What is automated

| Layer                     | Tests                                                                                                                                                                                                           |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/domain`           | Serialization round trip, unknown block kinds rejected, page-tree cycle detection.                                                                                                                              |
| `crates/storage`          | Nesting/reordering, cycle rejection leaves state unchanged, document round trip, search and index rebuild, archive/restore, duplicate (subtree + documents), newer document versions refused, cascading delete. |
| `crates/crypto`           | Round trip, context binding (AAD), tampering, wrong key, nonce uniqueness, version rejection, recovery-key typo detection, device enrollment, SAS mismatch.                                                     |
| `crates/crdt`             | Page and workspace documents: round trips, minimal operations, moves, marks, concurrent edits, delete-versus-edit recovery, out-of-order delivery, randomized convergence (12 + 10 seeds).                      |
| `crates/protocol`         | Wire types, base64 JSON, operation envelope.                                                                                                                                                                    |
| `apps/sync-server`        | See below.                                                                                                                                                                                                      |
| `crates/sync`             | See below.                                                                                                                                                                                                      |
| `packages/document-model` | ProseMirror ⇄ Document round trips, stable ids, code-point marks, unsupported content rejected.                                                                                                                 |

Phase 3 added: database schema/value validation (Rust and TypeScript), value cleanup on schema edits,
record move guard, duplicate/delete cascades, relations, backlinks, search excerpts, the
filter/sort engine, navigation history, the keyboard-shortcut registry, page links, palette
highlighting and the database UI (cells, filters, sorts, properties, views). Scale checks live in
`crates/storage/tests/large_workspace.rs` and `packages/document-model/src/scale.test.ts`; see
[performance.md](performance.md).

Block editing tests (`editor/blockOps.test.ts`, `components/menus.test.tsx`) cover block lookup,
turn-into for every kind, duplicate/delete/move, property edits, Esc/Enter/⌘D keyboard paths, the
context menu's keyboard and submenu behaviour, the block menu and the editor menu items.

Multi-block selection: `editor/blockRange.test.ts` covers the range model (index/position mapping,
vertical hit-testing, lists as one block), state clearing, Backspace/Delete/Esc/Enter/arrows/⌘D,
typing, whole-block copy/cut/paste and turn-into; `components/marquee.test.tsx` covers the drag
(start rules, upward drags, threshold, scrollbar, plain clicks) with stubbed layout.

### Phase 4: sync, encryption, server

- `crates/storage/tests/sync.rs`: two or three real stores exchanging operations through a simulated
  hub (capture, merge, materialize, held operations from newer apps, crash repair, the rebased save).
- `apps/sync-server/tests/api.rs`: signature, replay, clock skew, protocol version, rate limit, quotas,
  idempotent push, long polling, concurrent pushes, enrollment, revocation and key wraps, recovery,
  chunked attachments with resume, metrics, deletion. `tests/postgres.rs`: restart persistence,
  replay protection across instances, one total order across two instances, TLS to PostgreSQL.
- `crates/sync/tests`: a real server on a loopback port and a fault-injecting proxy that can drop a
  request, drop a response, take the server down or rewrite responses like a hostile server.
  `scenarios.rs` covers the blueprint cases (parts edited offline, same text, delete versus edit,
  move versus edit, long offline, app closed between upload and acknowledgement, server unavailable,
  device removed with unsynced changes, newer versus older app) and a three-device random run.
  `security.rs`: ciphertext-only storage scan, damaged and re-addressed operations, forged key
  update, enrollment tampering, expired and reused invites, recovery (also after rotation), quota
  errors. `attachments.rs`: resume of interrupted uploads and downloads, damaged and reordered
  chunks, quota, rotation.
- Desktop UI: setup wizard and recovery key confirmation, adding and removing devices, join flow,
  removed-device banner, image and file blocks, applying remote changes to an open editor.

PostgreSQL tests run when `NOTERCRUX_TEST_DATABASE_URL` is set (an empty database; each test uses
unique workspace ids, so the database can be reused). The TLS test also needs
`NOTERCRUX_TEST_DATABASE_TLS_URL` and `NOTERCRUX_TEST_DATABASE_CA`:

```bash
NOTERCRUX_TEST_DATABASE_URL=postgres://user@127.0.0.1:5432/ncx_test cargo test -p notercrux-sync-server -p notercrux-sync
```

Without the variable the same tests run against the in-memory repository.

### Phase 5: iOS

- `crates/mobile/tests/core.rs`: a phone-side core joins a desktop-side engine through a real server,
  both edit offline, and both end up with both sets of changes, through the same JSON API Swift uses.
- `apps/ios` `NotercruxTests` (run in the simulator, against the real Rust core): marks follow text
  edits (including Unicode scalars), splitting and merging blocks, Backspace and `/` behaviour,
  desktop-shaped JSON round trips with unknown attributes preserved, rebased saves, async calls on the
  core's runtime. See [apps/ios/README.md](../../apps/ios/README.md).
- Manual, on one physical iPhone (iPhone 15 Pro Max class): joining a stand-in desktop with an invite
  code, receiving pages, editing on both sides, creating a page on the phone. Not exercised: iPad
  layout, rotation, Dynamic Type, VoiceOver, background and low-connectivity behaviour, the Keychain
  across reinstalls, screenshots of every state.

Run everything with `pnpm test`.

## Not automated yet (manual checklist)

- Native window material on **macOS** (vibrancy follows window focus, light/dark switch).
- Native window material on **Windows** (Mica), and `backdrop-filter` inside WebView2.
- Performance with several overlays visible; large page trees; long documents.
- Editor: paste from other apps, IME input, the block drag handle and sidebar drag-and-drop with a
  real pointer in the native webviews (verified only via synthetic events in a browser preview;
  `dragDropEnabled` is off so Windows WebView2 delivers HTML5 drag events), undo/redo across page
  switches (history is discarded per page).
- Margin drag-selection with a real pointer in the native windows. Edge auto-scroll was exercised in a
  browser preview with a substituted animation-frame clock (the preview pane is hidden, which pauses
  `requestAnimationFrame`).
- Frosted appearance of the palette, slash menu and context menus, and context-menu hover, in the
  native macOS/Windows windows (see ADR 0005 addendum; checked only in a browser preview).
- Right-click Cut/Copy/Paste against the real system clipboard (including rich paste from other
  apps) and the block drag handle with a real pointer.
- Database table with a real pointer: cell editing, popovers, scrolling with thousands of rows.
- `[[` picker and slash menu with an IME.
- Sync in the native app: the Keychain / Credential Manager / Secret Service paths of the secure
  storage, the file picker for image and file blocks, paste and drop of files, saving an attachment
  with the native dialog, and the sync status badge while the window is in the background.
- Sync against a server behind a real reverse proxy with HTTPS (the tests use plain HTTP on loopback).
- Native save/open dialogs for export/import (Rust commands; the browser preview uses a download).
- Keyboard-only walkthrough and screen-reader pass of the sidebar tree, palette and dialogs.
- UI end-to-end tests of the native app.

## Performance baselines

Not measured yet. Establish cold start, page open time, input latency, search latency and memory
before making any performance claims (blueprint §13).
