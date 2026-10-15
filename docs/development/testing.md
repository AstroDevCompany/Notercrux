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

Block editing tests (`editor/blockOps.test.ts`, `components/menus.test.tsx`) cover block lookup,
turn-into for every kind, duplicate/delete/move, property edits, Esc/Enter/⌘D keyboard paths, the
context menu's keyboard and submenu behaviour, the block menu and the editor menu items.

Multi-block selection: `editor/blockRange.test.ts` covers the range model (index/position mapping,
vertical hit-testing, lists as one block), state clearing, Backspace/Delete/Esc/Enter/arrows/⌘D,
typing, whole-block copy/cut/paste and turn-into; `components/marquee.test.tsx` covers the drag
(start rules, upward drags, threshold, scrollbar, plain clicks) with stubbed layout.

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
- Native save/open dialogs for export/import (Rust commands; the browser preview uses a download).
- Keyboard-only walkthrough and screen-reader pass of the sidebar tree, palette and dialogs.
- UI end-to-end tests of the native app.

## Performance baselines

Not measured yet. Establish cold start, page open time, input latency, search latency and memory
before making any performance claims (blueprint §13).
