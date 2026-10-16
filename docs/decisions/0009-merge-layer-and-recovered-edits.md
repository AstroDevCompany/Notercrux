# ADR 0009: CRDT merge layer, recovered edits and rebased saves

**Status:** Accepted for implementation (Phase 4). Supersedes the open questions in ADR 0003.

## Decisions

- **The relational tables stay the app's working copy.** Pages, documents and properties in SQLite
  are what the UI reads and what search indexes. Loro documents are the merge layer beside them:
  one `ws` document (page tree, database schemas, property values) and one document per page
  (`page:<id>`). A local change is written to SQLite first and then captured into the CRDT in its
  own transaction; the resulting update goes to the outbox. If the app dies between the two, a
  startup reconcile diffs SQLite against the CRDT and emits what is missing, so the CRDT is only ever
  behind SQLite, never ahead.
- **Remote operations are applied in one SQLite transaction** together with the new cursor:
  import into the CRDT, rebuild the affected rows, advance the cursor. A crash leaves either all of
  it or none. Imports are idempotent.
- **Page tree.** A Loro movable tree with fractional order; concurrent moves that would form a cycle
  converge identically on every device. Deleting a page remotely while this device has unsent
  content keeps the page (archived) rather than discarding the content.
- **Databases.** Schema entries and property values are per-key last-writer-wins; order uses movable
  lists. Values are validated against the merged schema while rebuilding; an invalid value is
  hidden, not destroyed. References to deleted properties are removed by a repair pass.
- **No silent loss on delete versus edit.** If a merge deletes a block that this device edited and
  has not yet had confirmed, or deletes a block whose text another device changed, the text is
  restored into a yellow callout (icon ♻️) at the end of the page, once, and the person is told.
- **Editor saves are rebased.** The editor remembers the document it last matched (`base`). A save
  sends `base` and the editor's copy; the backend does a three-way merge of `base → editor` onto the
  page as it is now. Block-level changes merge by block id; two edits to different parts of one
  plain-text paragraph merge by text; edits to the same part keep the local version and put the other
  one in a callout. This is what lets a remote change arrive while the person is typing. Without it
  a stale editor would overwrite the remote edit.
- **Attachments** are content-addressed by the SHA-256 of the plaintext, stored in 1 MiB chunks in
  SQLite, referenced from `image` and `file` blocks, uploaded as encrypted chunks (each sealed with
  its blob id, index, chunk count and epoch as associated data), resumed chunk by chunk, and verified
  against the content hash after download.

## Consequences

- Page documents keep tombstones, so a page document grows with edit history. Compaction is not
  built. Measured costs are in `docs/development/performance.md`.
- The rebase merges at block granularity, so it cannot split two simultaneous edits inside one
  formatted paragraph; it keeps both versions instead.
- Attachments are never deleted automatically (history and archived pages may reference them).
- A newer app can add block kinds; an older app reads an unknown kind as a paragraph, so
  upgrading all devices is advisable. Unknown operation formats are held, not applied.
