-- Attachments (images and files). The id is the SHA-256 of the plaintext, so identical files are stored
-- once, and a downloaded file can be verified end to end whatever the server returned.
--
-- state: 'local'   data present, not yet confirmed on the server (or sync is off)
--        'synced'  data present and on the server
--        'missing' referenced by a block but the data has not arrived yet (download pending)
--        'failed'  sync gave up (see `error`); `attachment_retry_failed` puts it back in the queue
CREATE TABLE attachments (
    id         TEXT PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    state      TEXT NOT NULL,
    error      TEXT,
    created_at INTEGER NOT NULL
);

-- Plaintext in fixed-size chunks (the last one may be shorter). Chunking lets a transfer resume.
CREATE TABLE attachment_chunks (
    id   TEXT NOT NULL,
    idx  INTEGER NOT NULL,
    data BLOB NOT NULL,
    PRIMARY KEY (id, idx)
);
