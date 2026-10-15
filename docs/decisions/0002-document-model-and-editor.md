# ADR 0002 — Structured block documents and Tiptap

**Status:** Accepted (spike validated; see "Limits")

## Context

Content must have stable block identities, typed attributes, nesting, and be mergeable across
devices. Storing HTML or Markdown blobs makes all three hard.

## Decision

- **Canonical model:** `Document { version, blocks[] }` where each `Block` has a UUIDv7 `id`, a
  `type`, `attrs`, plain `text`, inline `marks` (code-point ranges) and nested `children`. The Rust
  definition is in `crates/domain`; the TypeScript mirror is in `packages/document-model`.
- **Editing engine:** Tiptap 3 (ProseMirror). It is an _editing representation_, not the stored
  one. `packages/document-model/src/prosemirror.ts` converts both ways.
- **Stable ids:** a small Tiptap extension (`apps/desktop/src/editor/blockId.ts`) assigns and
  repairs `blockId` attributes on every block node.
- **Fail loudly:** converting a node or mark the model does not know throws
  `UnsupportedContentError`; the UI then refuses to save and says so, rather than dropping content.
- **Versioning:** documents carry `version`; the store refuses to load a newer version than it
  supports.
- **Undo/redo** is ProseMirror history, per open page and per session. It is not persisted.
- **Persistence:** SQLite, with `pages` and `documents` canonical and the FTS5 search index a
  rebuildable cache.

## Limits found by the spike

- The model covers paragraphs, headings, bulleted/numbered/to-do items, quotes, callouts, code and
  dividers. Tables, images, files and page links have no editor nodes yet (`pageLink` exists only as
  a block kind).
- `text` is flat with mark ranges, which maps cleanly onto Loro rich text (ADR 0003).
- Block ids are not yet validated for uniqueness across a whole document on the Rust side.

## Multi-block selection

A "block range" (`editor/blockRange.ts`) is a run of consecutive **top-level** blocks, created by
dragging a rubber band in the page margins (`components/MarqueeSelect.tsx`). It lives in plugin
state, is cleared by any document or selection change, and is drawn as an overlay rather than via
node decorations (which do not reliably reach React node views). Copy, cut, delete and paste are
overridden so whole blocks are handled; leaving the editor selection as the matching text span keeps
focus and accessibility working. Blocks are hit-tested by **vertical extent only**. Limits: list
items cannot be selected individually, and a range cannot be moved or dragged.

## Phase 2 additions

- **Callouts** are a custom Tiptap node (`apps/desktop/src/editor/callout.ts`) mapped to the
  `callout` block kind with an `icon` attribute.
- **Slash menu** uses `@tiptap/suggestion`; **block handle and block moves** are our own
  (`@tiptap/extension-drag-handle` was rejected because it requires collaboration packages).
- **Markdown** export/import is a deliberately small, tested subset parser rather than a
  general-purpose library, so every unsupported construct can be reported. Revisit if fidelity
  requirements grow.
