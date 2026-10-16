-- Sync support (opt-in). CRDT documents are the merge layer; pages/documents/properties stay the
-- queryable local view and are rebuilt from the CRDT state when remote changes arrive.

-- One CRDT snapshot per document: 'ws' (page tree, schemas, values) or 'page:<page id>' (content).
-- `dirty` lists block ids this device edited since the server last confirmed its operations.
CREATE TABLE crdt_docs (
    doc_key    TEXT PRIMARY KEY NOT NULL,
    snapshot   BLOB NOT NULL,
    dirty      TEXT NOT NULL DEFAULT '[]',
    updated_at INTEGER NOT NULL
);

-- Local operations waiting to be sent. `op_id` is stable across retries so the server can de-duplicate.
CREATE TABLE sync_outbox (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    op_id       TEXT NOT NULL UNIQUE,
    doc_key     TEXT NOT NULL,
    update_blob BLOB NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX sync_outbox_doc ON sync_outbox(doc_key);

-- Remote operations written by a newer app version than this one understands. Kept (never dropped)
-- until the app is upgraded.
CREATE TABLE sync_held (
    seq         INTEGER PRIMARY KEY NOT NULL,
    op_id       TEXT NOT NULL,
    doc_key     TEXT NOT NULL,
    format      INTEGER NOT NULL,
    update_blob BLOB NOT NULL
);

-- Small key/value state: enabled, cursor, workspace id, needs_reconcile, ...
CREATE TABLE sync_state (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
