-- Canonical data: pages + documents. Everything else is a rebuildable cache.
CREATE TABLE pages (
    id          TEXT PRIMARY KEY NOT NULL,
    parent_id   TEXT REFERENCES pages(id) ON DELETE CASCADE,
    title       TEXT NOT NULL DEFAULT '',
    icon        TEXT,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    archived_at INTEGER
);
CREATE INDEX pages_parent ON pages(parent_id, position);

CREATE TABLE documents (
    page_id     TEXT PRIMARY KEY NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    content     TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Rebuildable local search index (see Store::rebuild_search_index).
CREATE VIRTUAL TABLE search_index USING fts5(page_id UNINDEXED, title, body);
