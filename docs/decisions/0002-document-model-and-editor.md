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

- The model covers paragraphs, headings, bulleted/numbered/to-do items, quotes, code and dividers.
  Callouts, tables, images, files and page links are defined as kinds (or planned) but have no editor
  nodes yet.
- `text` is flat with mark ranges, which maps cleanly onto Loro rich text (ADR 0003).
- Block ids are not yet validated for uniqueness across a whole document on the Rust side.
