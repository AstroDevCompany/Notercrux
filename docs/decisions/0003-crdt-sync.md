# ADR 0003 — Loro CRDT documents over an opaque relay

**Status:** Proposed (spike: `crates/sync`)

## Context

Naive last-write-wins silently destroys edits. The blueprint requires merging for block ordering,
deletion, movement and concurrent text edits, with no silent data loss.

## Options

- **Yjs / `yrs`** — very mature; tree moves are not first-class.
- **Automerge** — mature; larger documents and slower for long text.
- **Loro** — CRDT tree (with move), rich text, Rust-native with a small API surface.

## Spike

`crates/sync` models a page as a Loro **tree** of blocks (movable, nestable) whose text is a Loro
text container. Two simulated devices sync through an in-memory **relay** that stores only
encrypted blobs, de-duplicated by content hash.

Covered by tests (`crates/sync/src/tests.rs`):

| Blueprint scenario                             | Result                                               |
| ---------------------------------------------- | ---------------------------------------------------- |
| Different parts of a page edited offline       | Both survive                                         |
| Same text edited simultaneously                | Character-level merge; all fragments kept            |
| Block deleted on one device, edited on another | Block hidden on both; edit retained in the tombstone |
| Block moved while another device edits it      | Move and edit both applied                           |
| Concurrent moves forming a cycle               | Converges identically; both blocks remain            |
| Long offline period                            | Catches up                                           |
| Relay outage mid-sync                          | Retry is safe; nothing duplicated                    |
| App closes during sync                         | Re-push de-duplicated; import idempotent             |
| Device revoked with unsynced changes           | Rejected by relay; local data intact                 |
| Relay and wrong-key devices                    | See ciphertext only; cross-page replay rejected      |

## Not yet validated

- Page _tree_ moves versus content edits across **separate** documents (cross-document consistency).
- Database/property consistency rules, schema evolution, newer/older protocol versions.
- Attachment transfer, resumable uploads, backoff, and real network transport.
- Size growth and compaction of CRDT history on large, long-lived documents.
- How a deleted-but-edited block is **surfaced** to the user (the data is kept; the UX is undecided).

## Consequences

Until these are resolved this ADR stays _proposed_, and the desktop app does **not** yet use
`crates/sync`: it persists plain structured documents. Adopting the CRDT will change how
`storage` persists documents (CRDT snapshot + updates rather than JSON) and must come with a
migration.
