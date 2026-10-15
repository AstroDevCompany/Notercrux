-- Databases, record properties and the page-link graph.
ALTER TABLE pages ADD COLUMN kind TEXT NOT NULL DEFAULT 'page';

CREATE TABLE databases (
    page_id TEXT PRIMARY KEY NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    schema  TEXT NOT NULL
);

-- One row per non-empty property value of a record (a child page of a database).
CREATE TABLE page_properties (
    page_id     TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (page_id, property_id)
);

-- Rebuildable cache derived from documents: which pages link to which.
CREATE TABLE page_links (
    source_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    target_id TEXT NOT NULL,
    PRIMARY KEY (source_id, target_id)
);
CREATE INDEX page_links_target ON page_links(target_id);
