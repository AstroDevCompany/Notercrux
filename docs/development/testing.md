# Testing

## What is automated

| Layer                     | Tests                                                                                                                                                                                                           |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/domain`           | Serialization round trip, unknown block kinds rejected, page-tree cycle detection.                                                                                                                              |
| `crates/storage`          | Nesting/reordering, cycle rejection leaves state unchanged, document round trip, search and index rebuild, archive/restore, duplicate (subtree + documents), newer document versions refused, cascading delete. |
| `crates/crypto`           | Round trip, context binding (AAD), tampering, wrong key, nonce uniqueness, version rejection, recovery-key typo detection, device enrollment, SAS mismatch.                                                     |
| `crates/sync`             | Blueprint sync scenarios listed in [ADR 0003](../decisions/0003-crdt-sync.md).                                                                                                                                  |
| `packages/document-model` | ProseMirror ⇄ Document round trips, stable ids, code-point marks, unsupported content rejected.                                                                                                                 |

Phase 3 added: database schema/value validation (Rust and TypeScript), value cleanup on schema edits,
record move guard, duplicate/delete cascades, relations, backlinks, search excerpts, the
filter/sort engine, navigation history, the keyboard-shortcut registry, page links, palette
highlighting and the database UI (cells, filters, sorts, properties, views). Scale checks live in
`crates/storage/tests/large_workspace.rs` and `packages/document-model/src/scale.test.ts`; see
[performance.md](performance.md).

Run everything with `pnpm test`.

## Not automated yet (manual checklist)

- Native window material on **macOS** (vibrancy follows window focus, light/dark switch).
- Native window material on **Windows** (Mica), and `backdrop-filter` inside WebView2.
- Performance with several overlays visible; large page trees; long documents.
- Editor: paste from other apps, IME input, the block drag handle and sidebar drag-and-drop with a
  real pointer in the native webviews (verified only via synthetic events in a browser preview;
  `dragDropEnabled` is off so Windows WebView2 delivers HTML5 drag events), undo/redo across page
  switches (history is discarded per page).
- Context-menu hover and frosted appearance in the native window (see ADR 0005 addendum).
- Database table with a real pointer: cell editing, popovers, scrolling with thousands of rows.
- `[[` picker and slash menu with an IME.
- Native save/open dialogs for export/import (Rust commands; the browser preview uses a download).
- Keyboard-only walkthrough and screen-reader pass of the sidebar tree, palette and dialogs.
- UI end-to-end tests of the native app.

## Performance baselines

Not measured yet. Establish cold start, page open time, input latency, search latency and memory
before making any performance claims (blueprint §13).
