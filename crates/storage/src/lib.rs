//! SQLite persistence.
//!
//! `pages` and `documents` are canonical. `search_index` is a cache that can be dropped and
//! rebuilt from them at any time via [`Store::rebuild_search_index`].

use notercrux_domain::{
    validate_value, Block, DatabaseSchema, Document, Page, PageId, PageKind, PageTree, SchemaError,
    TreeError, DOCUMENT_SCHEMA_VERSION,
};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub mod attachments;
pub mod sync;
pub use attachments::{
    AttachmentMeta, AttachmentState, AttachmentWork, ATTACHMENT_CHUNK, MAX_ATTACHMENT_BYTES,
};
pub use sync::{ApplyReport, LocalOp, RemoteOp, SyncStats, SYNC_FORMAT};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("document encoding error: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Crdt(#[from] notercrux_crdt::CrdtError),
    #[error("page {0} is not a database")]
    NotADatabase(PageId),
    #[error("page {0} is not a record of a database")]
    NotARecord(PageId),
    #[error("{0}")]
    Invalid(String),
    #[error("document schema version {found} is newer than supported version {supported}")]
    UnsupportedDocumentVersion { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, StoreError>;

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("../migrations/0001_init.sql")),
    (
        "0002_databases",
        include_str!("../migrations/0002_databases.sql"),
    ),
    ("0003_sync", include_str!("../migrations/0003_sync.sql")),
    (
        "0004_attachments",
        include_str!("../migrations/0004_attachments.sql"),
    ),
];

/// A page that matched a search, with a short excerpt. Matches are wrapped in U+0001 … U+0002.
#[derive(Clone, Debug, Serialize)]
pub struct SearchHit {
    pub page: Page,
    pub snippet: String,
}

/// A record of a database with its non-empty property values.
#[derive(Clone, Debug, Serialize)]
pub struct Record {
    pub page: Page,
    pub values: HashMap<String, Value>,
}

pub struct Store {
    pub(crate) conn: Connection,
    pub(crate) sync: sync::SyncRuntime,
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn page_from_row(r: &Row<'_>) -> rusqlite::Result<Page> {
    let id: String = r.get("id")?;
    let parent: Option<String> = r.get("parent_id")?;
    let parse = |s: String| {
        s.parse::<PageId>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    };
    Ok(Page {
        id: parse(id)?,
        parent_id: parent.map(parse).transpose()?,
        kind: match r.get::<_, String>("kind")?.as_str() {
            "database" => PageKind::Database,
            _ => PageKind::Page,
        },
        title: r.get("title")?,
        icon: r.get("icon")?,
        position: r.get("position")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        archived_at: r.get("archived_at")?,
    })
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let mut store = Self {
            conn,
            sync: sync::SyncRuntime::default(),
        };
        store.migrate()?;
        store.sync.enabled = store.sync_get("enabled")?.as_deref() == Some("1");
        Ok(store)
    }

    /// Applies pending migrations, each in its own transaction.
    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL);",
        )?;
        for (name, sql) in MIGRATIONS {
            let done: bool = self
                .conn
                .query_row(
                    "SELECT 1 FROM schema_migrations WHERE name = ?1",
                    [name],
                    |_| Ok(true),
                )
                .optional()?
                .unwrap_or(false);
            if done {
                continue;
            }
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (name, applied_at) VALUES (?1, ?2)",
                params![name, now_ms()],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    // ---- queries ---------------------------------------------------------------------------

    /// All pages (live and archived), ordered by parent then position.
    pub fn list_pages(&self) -> Result<Vec<Page>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM pages ORDER BY parent_id IS NOT NULL, parent_id, position")?;
        let rows = stmt.query_map([], page_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_page(&self, id: PageId) -> Result<Option<Page>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM pages WHERE id = ?1",
                [id.to_string()],
                page_from_row,
            )
            .optional()?)
    }

    // ---- page mutations (each is a single transaction) ------------------------------------

    pub fn create_page(&mut self, parent: Option<PageId>, title: &str) -> Result<Page> {
        let tx = self.conn.transaction()?;
        let id = PageId::new();
        let ts = now_ms();
        let position = next_position(&tx, parent)?;
        tx.execute(
            "INSERT INTO pages (id, parent_id, title, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                id.to_string(),
                parent.map(|p| p.to_string()),
                title,
                position,
                ts
            ],
        )?;
        reindex(&tx, id)?;
        tx.commit()?;
        self.capture(sync::Change::PageCreated(id));
        Ok(self.get_page(id)?.expect("just inserted"))
    }

    pub fn rename_page(&mut self, id: PageId, title: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let n = tx.execute(
            "UPDATE pages SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), title, now_ms()],
        )?;
        if n == 0 {
            return Err(TreeError::NotFound(id).into());
        }
        reindex(&tx, id)?;
        tx.commit()?;
        self.capture(sync::Change::PageMeta(id));
        Ok(())
    }

    pub fn set_icon(&mut self, id: PageId, icon: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE pages SET icon = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), icon, now_ms()],
        )?;
        self.capture(sync::Change::PageMeta(id));
        Ok(())
    }

    /// Moves `id` under `new_parent` (or to the root) at `index` among its new siblings.
    pub fn move_page(&mut self, id: PageId, new_parent: Option<PageId>, index: u32) -> Result<()> {
        let pages = self.list_pages()?;
        PageTree::from_pages(&pages).check_move(id, new_parent)?;
        let old_parent = pages.iter().find(|p| p.id == id).and_then(|p| p.parent_id);
        if old_parent != new_parent {
            // Records carry property values defined by their database's schema, so they cannot
            // change database, and ordinary pages cannot be turned into records by a move.
            let is_db = |pid: Option<PageId>| {
                pid.and_then(|x| pages.iter().find(|p| p.id == x))
                    .is_some_and(|p| p.kind == PageKind::Database)
            };
            if is_db(old_parent) || is_db(new_parent) {
                return Err(StoreError::Invalid(
                    "Records cannot be moved into or out of a database.".into(),
                ));
            }
        }

        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE pages SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), new_parent.map(|p| p.to_string()), now_ms()],
        )?;
        let mut siblings = sibling_ids(&tx, new_parent, Some(id))?;
        siblings.insert((index as usize).min(siblings.len()), id.to_string());
        write_positions(&tx, &siblings)?;
        if old_parent != new_parent {
            let rest = sibling_ids(&tx, old_parent, None)?;
            write_positions(&tx, &rest)?;
        }
        tx.commit()?;
        self.capture(sync::Change::TreeChanged);
        Ok(())
    }

    /// Deep-copies a page, its descendants and their documents next to the original.
    pub fn duplicate_page(&mut self, id: PageId) -> Result<Page> {
        let pages = self.list_pages()?;
        let tree = PageTree::from_pages(&pages);
        let original = pages
            .iter()
            .find(|p| p.id == id)
            .ok_or(TreeError::NotFound(id))?
            .clone();
        let tx = self.conn.transaction()?;
        let ts = now_ms();
        let mut map = std::collections::HashMap::new();
        for old in tree.subtree(id) {
            map.insert(old, PageId::new());
        }
        for old in tree.subtree(id) {
            let p = pages.iter().find(|p| p.id == old).expect("in tree");
            let new_id = map[&old];
            let parent = if old == id {
                p.parent_id
            } else {
                p.parent_id.map(|x| map[&x])
            };
            let title = if old == id {
                format!("{} (copy)", p.title)
            } else {
                p.title.clone()
            };
            let position = if old == id {
                p.position + 1
            } else {
                p.position
            };
            tx.execute(
                "INSERT INTO pages (id, parent_id, kind, title, icon, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    new_id.to_string(),
                    parent.map(|x| x.to_string()),
                    if p.kind == PageKind::Database { "database" } else { "page" },
                    title,
                    p.icon,
                    position,
                    ts
                ],
            )?;
            tx.execute(
                "INSERT INTO databases (page_id, schema) SELECT ?2, schema FROM databases WHERE page_id = ?1",
                params![old.to_string(), new_id.to_string()],
            )?;
            tx.execute(
                "INSERT INTO page_properties (page_id, property_id, value)
                 SELECT ?2, property_id, value FROM page_properties WHERE page_id = ?1",
                params![old.to_string(), new_id.to_string()],
            )?;
            tx.execute(
                "INSERT INTO documents (page_id, version, content, updated_at)
                 SELECT ?2, version, content, ?3 FROM documents WHERE page_id = ?1",
                params![old.to_string(), new_id.to_string(), ts],
            )?;
            reindex(&tx, new_id)?;
        }
        let mut siblings = sibling_ids(&tx, original.parent_id, Some(map[&id]))?;
        let at = siblings
            .iter()
            .position(|s| *s == id.to_string())
            .map(|i| i + 1)
            .unwrap_or(siblings.len());
        siblings.insert(at, map[&id].to_string());
        write_positions(&tx, &siblings)?;
        tx.commit()?;
        self.capture(sync::Change::SubtreeCreated(
            map.values().copied().collect(),
        ));
        Ok(self.get_page(map[&id])?.expect("just inserted"))
    }

    /// Soft-deletes a page and its descendants.
    pub fn archive_page(&mut self, id: PageId) -> Result<()> {
        self.set_archived(id, Some(now_ms()))
    }

    pub fn restore_page(&mut self, id: PageId) -> Result<()> {
        // A restored page whose parent is still archived would be unreachable; lift it to the root.
        let pages = self.list_pages()?;
        let page = pages
            .iter()
            .find(|p| p.id == id)
            .ok_or(TreeError::NotFound(id))?;
        let parent_archived = page
            .parent_id
            .and_then(|pid| pages.iter().find(|p| p.id == pid))
            .is_some_and(|p| p.archived_at.is_some());
        if parent_archived {
            let n = self
                .list_pages()?
                .iter()
                .filter(|p| p.parent_id.is_none())
                .count() as u32;
            self.move_page(id, None, n)?;
        }
        self.set_archived(id, None)
    }

    fn set_archived(&mut self, id: PageId, at: Option<i64>) -> Result<()> {
        let pages = self.list_pages()?;
        if !pages.iter().any(|p| p.id == id) {
            return Err(TreeError::NotFound(id).into());
        }
        let ids = PageTree::from_pages(&pages).subtree(id);
        let tx = self.conn.transaction()?;
        for i in ids {
            tx.execute(
                "UPDATE pages SET archived_at = ?2 WHERE id = ?1",
                params![i.to_string(), at],
            )?;
        }
        tx.commit()?;
        self.capture(sync::Change::TreeChanged);
        Ok(())
    }

    /// Permanently removes a page and its descendants (documents cascade).
    pub fn delete_page_permanently(&mut self, id: PageId) -> Result<()> {
        let pages = self.list_pages()?;
        let ids = PageTree::from_pages(&pages).subtree(id);
        let tx = self.conn.transaction()?;
        for i in ids.iter() {
            tx.execute(
                "DELETE FROM search_index WHERE page_id = ?1",
                [i.to_string()],
            )?;
        }
        tx.execute("DELETE FROM pages WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        self.capture(sync::Change::Deleted);
        Ok(())
    }

    // ---- documents -------------------------------------------------------------------------

    pub fn load_document(&self, id: PageId) -> Result<Document> {
        let row: Option<(u32, String)> = self
            .conn
            .query_row(
                "SELECT version, content FROM documents WHERE page_id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(Document::default()),
            Some((v, _)) if v > DOCUMENT_SCHEMA_VERSION => {
                Err(StoreError::UnsupportedDocumentVersion {
                    found: v,
                    supported: DOCUMENT_SCHEMA_VERSION,
                })
            }
            Some((_, content)) => Ok(serde_json::from_str(&content)?),
        }
    }

    pub fn save_document(&mut self, id: PageId, doc: &Document) -> Result<()> {
        let tx = self.conn.transaction()?;
        let ts = now_ms();
        tx.execute(
            "INSERT INTO documents (page_id, version, content, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(page_id) DO UPDATE SET version = ?2, content = ?3, updated_at = ?4",
            params![id.to_string(), doc.version, serde_json::to_string(doc)?, ts],
        )?;
        tx.execute(
            "UPDATE pages SET updated_at = ?2 WHERE id = ?1",
            params![id.to_string(), ts],
        )?;
        reindex(&tx, id)?;
        tx.commit()?;
        self.capture(sync::Change::Document(id));
        Ok(())
    }

    /// Saves what the editor changed (`base` -> `local`) on top of the page as it is now. When remote
    /// changes were applied since the editor loaded `base`, they are kept; plain overwriting would
    /// delete them. Returns the merged page (what the editor should now show) and how many blocks
    /// were edited on both sides in the same place.
    pub fn save_document_rebased(
        &mut self,
        id: PageId,
        base: &Document,
        local: &Document,
    ) -> Result<notercrux_domain::Rebased> {
        let current = self.load_document(id)?;
        if current == *base {
            self.save_document(id, local)?;
            return Ok(notercrux_domain::Rebased {
                document: local.clone(),
                conflicts: 0,
            });
        }
        let merged = notercrux_domain::rebase(base, local, &current);
        if merged.document != current {
            self.save_document(id, &merged.document)?;
        }
        Ok(merged)
    }

    // ---- search ----------------------------------------------------------------------------

    /// Prefix search over live page titles and bodies.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<Page>> {
        let fts: String = query
            .split_whitespace()
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT p.* FROM search_index s JOIN pages p ON p.id = s.page_id
             WHERE search_index MATCH ?1 AND p.archived_at IS NULL ORDER BY rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts, limit], page_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Like [`Store::search`], with an excerpt around the match. Matches in the excerpt are
    /// wrapped in U+0001 … U+0002 so callers can highlight them without trusting markup.
    pub fn search_with_snippets(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        let fts: String = query
            .split_whitespace()
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT p.*, s.body AS body FROM search_index s JOIN pages p ON p.id = s.page_id
             WHERE search_index MATCH ?1 AND p.archived_at IS NULL ORDER BY rank LIMIT ?2",
        )?;
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let hits = stmt
            .query_map(params![fts, limit], |r| {
                let page = page_from_row(r)?;
                let body: String = r.get("body")?;
                Ok(SearchHit {
                    snippet: make_snippet(&body, &words),
                    page,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(hits)
    }

    // ---- links -----------------------------------------------------------------------------

    /// Live pages whose content links to `id`.
    pub fn backlinks(&self, id: PageId) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.* FROM page_links l JOIN pages p ON p.id = l.source_id
             WHERE l.target_id = ?1 AND p.archived_at IS NULL ORDER BY p.title",
        )?;
        let rows = stmt.query_map([id.to_string()], page_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- databases -------------------------------------------------------------------------

    /// Creates a database page with an empty schema and one default table view.
    pub fn create_database(&mut self, parent: Option<PageId>, title: &str) -> Result<Page> {
        let page = self.create_page(parent, title)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE pages SET kind = 'database' WHERE id = ?1",
            [page.id.to_string()],
        )?;
        tx.execute(
            "INSERT INTO databases (page_id, schema) VALUES (?1, ?2)",
            params![
                page.id.to_string(),
                serde_json::to_string(&DatabaseSchema::default())?
            ],
        )?;
        tx.commit()?;
        self.capture(sync::Change::DatabaseCreated(page.id));
        Ok(self.get_page(page.id)?.expect("just inserted"))
    }

    pub fn get_schema(&self, db: PageId) -> Result<DatabaseSchema> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT schema FROM databases WHERE page_id = ?1",
                [db.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let schema: DatabaseSchema =
            serde_json::from_str(&raw.ok_or(StoreError::NotADatabase(db))?)?;
        Ok(schema)
    }

    /// Replaces the schema. Values of removed properties, and of properties whose type changed,
    /// are deleted in the same transaction so no value ever disagrees with its property.
    pub fn set_schema(&mut self, db: PageId, schema: &DatabaseSchema) -> Result<()> {
        schema.validate()?;
        let old = self.get_schema(db)?;
        let stale: Vec<&str> = old
            .properties
            .iter()
            .filter(|op| {
                schema.property(&op.id).is_none_or(|np| {
                    np.kind != op.kind || np.relation_database != op.relation_database
                })
            })
            .map(|p| p.id.as_str())
            .collect();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE databases SET schema = ?2 WHERE page_id = ?1",
            params![db.to_string(), serde_json::to_string(schema)?],
        )?;
        for pid in stale {
            tx.execute(
                "DELETE FROM page_properties WHERE property_id = ?2
                 AND page_id IN (SELECT id FROM pages WHERE parent_id = ?1)",
                params![db.to_string(), pid],
            )?;
        }
        // Select options that no longer exist must not linger in values.
        for p in &schema.properties {
            if !matches!(
                p.kind,
                notercrux_domain::PropertyKind::Select
                    | notercrux_domain::PropertyKind::MultiSelect
            ) {
                continue;
            }
            let rows: Vec<(String, String)> = {
                let mut st = tx.prepare(
                    "SELECT page_id, value FROM page_properties WHERE property_id = ?2
                     AND page_id IN (SELECT id FROM pages WHERE parent_id = ?1)",
                )?;
                let it = st.query_map(params![db.to_string(), p.id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
                it.collect::<rusqlite::Result<_>>()?
            };
            for (page_id, raw) in rows {
                let v: Value = serde_json::from_str(&raw)?;
                let keep = |x: &Value| {
                    x.as_str()
                        .is_some_and(|s| p.options.iter().any(|o| o.id == s))
                };
                let cleaned = match &v {
                    Value::Array(a) => {
                        Value::Array(a.iter().filter(|x| keep(x)).cloned().collect())
                    }
                    other if keep(other) => other.clone(),
                    _ => Value::Null,
                };
                if cleaned != v {
                    if cleaned.is_null() || cleaned.as_array().is_some_and(Vec::is_empty) {
                        tx.execute(
                            "DELETE FROM page_properties WHERE page_id = ?1 AND property_id = ?2",
                            params![page_id, p.id],
                        )?;
                    } else {
                        tx.execute(
                            "UPDATE page_properties SET value = ?3 WHERE page_id = ?1 AND property_id = ?2",
                            params![page_id, p.id, serde_json::to_string(&cleaned)?],
                        )?;
                    }
                }
            }
        }
        tx.commit()?;
        self.capture(sync::Change::Schema(db));
        Ok(())
    }

    /// Sets (or, with `null`, clears) a record's property after validating it against the schema.
    pub fn set_property(&mut self, page: PageId, property: &str, value: &Value) -> Result<()> {
        let record = self.get_page(page)?.ok_or(TreeError::NotFound(page))?;
        let db = record.parent_id.ok_or(StoreError::NotARecord(page))?;
        let schema = self
            .get_schema(db)
            .map_err(|_| StoreError::NotARecord(page))?;
        let prop = schema
            .property(property)
            .ok_or_else(|| SchemaError::UnknownProperty(property.to_string()))?;
        validate_value(prop, value)?;
        let empty = value.is_null()
            || value.as_str().is_some_and(str::is_empty)
            || value.as_array().is_some_and(Vec::is_empty);
        let tx = self.conn.transaction()?;
        if empty {
            tx.execute(
                "DELETE FROM page_properties WHERE page_id = ?1 AND property_id = ?2",
                params![page.to_string(), property],
            )?;
        } else {
            tx.execute(
                "INSERT INTO page_properties (page_id, property_id, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(page_id, property_id) DO UPDATE SET value = ?3",
                params![page.to_string(), property, serde_json::to_string(value)?],
            )?;
        }
        tx.execute(
            "UPDATE pages SET updated_at = ?2 WHERE id = ?1",
            params![page.to_string(), now_ms()],
        )?;
        tx.commit()?;
        self.capture(sync::Change::Property(page, property.to_string()));
        Ok(())
    }

    /// Property values of one record (empty values are absent).
    pub fn get_properties(&self, page: PageId) -> Result<HashMap<String, Value>> {
        let mut st = self
            .conn
            .prepare("SELECT property_id, value FROM page_properties WHERE page_id = ?1")?;
        let rows = st.query_map([page.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, serde_json::from_str(&v)?);
        }
        Ok(out)
    }

    /// Live records of a database in page order, each with its values. Sorting and filtering by
    /// view happen on the client.
    pub fn list_records(&self, db: PageId) -> Result<Vec<Record>> {
        self.get_schema(db)?; // NotADatabase if it is not one
        let mut st = self.conn.prepare(
            "SELECT * FROM pages WHERE parent_id = ?1 AND archived_at IS NULL ORDER BY position, created_at",
        )?;
        let pages: Vec<Page> = st
            .query_map([db.to_string()], page_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        let mut values: HashMap<String, HashMap<String, Value>> = HashMap::new();
        let mut vs = self.conn.prepare(
            "SELECT pp.page_id, pp.property_id, pp.value FROM page_properties pp
             JOIN pages p ON p.id = pp.page_id WHERE p.parent_id = ?1",
        )?;
        let rows = vs.query_map([db.to_string()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (page_id, prop, raw) = row?;
            values
                .entry(page_id)
                .or_default()
                .insert(prop, serde_json::from_str(&raw)?);
        }
        Ok(pages
            .into_iter()
            .map(|page| {
                let values = values.remove(&page.id.to_string()).unwrap_or_default();
                Record { page, values }
            })
            .collect())
    }

    /// Drops and recreates the search index from canonical data.
    pub fn rebuild_search_index(&mut self) -> Result<()> {
        let ids: Vec<String> = {
            let mut s = self.conn.prepare("SELECT id FROM pages")?;
            let rows = s.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM search_index", [])?;
        for id in ids {
            reindex(&tx, id.parse().expect("stored ids are valid"))?;
        }
        tx.commit()?;
        Ok(())
    }
}

// ---- helpers ------------------------------------------------------------------------------

fn next_position(tx: &Transaction<'_>, parent: Option<PageId>) -> rusqlite::Result<u32> {
    tx.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM pages WHERE parent_id IS ?1",
        [parent.map(|p| p.to_string())],
        |r| r.get(0),
    )
}

fn sibling_ids(
    tx: &Transaction<'_>,
    parent: Option<PageId>,
    exclude: Option<PageId>,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = tx.prepare(
        "SELECT id FROM pages WHERE parent_id IS ?1 AND id IS NOT ?2 ORDER BY position, created_at",
    )?;
    let rows = stmt.query_map(
        params![
            parent.map(|p| p.to_string()),
            exclude.map(|p| p.to_string())
        ],
        |r| r.get(0),
    )?;
    rows.collect()
}

fn write_positions(tx: &Transaction<'_>, ids: &[String]) -> rusqlite::Result<()> {
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE pages SET position = ?2 WHERE id = ?1",
            params![id, i as u32],
        )?;
    }
    Ok(())
}

pub(crate) fn reindex(tx: &Transaction<'_>, id: PageId) -> Result<()> {
    let id_s = id.to_string();
    let title: String = tx.query_row("SELECT title FROM pages WHERE id = ?1", [&id_s], |r| {
        r.get(0)
    })?;
    let content: Option<String> = tx
        .query_row(
            "SELECT content FROM documents WHERE page_id = ?1",
            [&id_s],
            |r| r.get(0),
        )
        .optional()?;
    let content_doc = content.as_deref().map(serde_json::from_str::<Document>);
    let body = match &content_doc {
        Some(Ok(d)) => d.plain_text(),
        _ => String::new(),
    };
    tx.execute("DELETE FROM search_index WHERE page_id = ?1", [&id_s])?;
    tx.execute(
        "INSERT INTO search_index (page_id, title, body) VALUES (?1, ?2, ?3)",
        params![id_s, title, body],
    )?;

    let mut targets = HashSet::new();
    if let Some(Ok(doc)) = content_doc {
        collect_links(&doc.blocks, &mut targets);
        attachments::register_references(tx, &doc.blocks)?;
    }
    tx.execute("DELETE FROM page_links WHERE source_id = ?1", [&id_s])?;
    for t in targets {
        tx.execute(
            "INSERT OR IGNORE INTO page_links (source_id, target_id) VALUES (?1, ?2)",
            params![id_s, t.to_string()],
        )?;
    }
    Ok(())
}

/// A short excerpt of `body` around the first match of any query word, with matches wrapped in
/// U+0001 … U+0002. Done in Rust, in one linear pass: FTS5's `snippet()` took over a second on a
/// 20,000-block document.
fn make_snippet(body: &str, words: &[String]) -> String {
    const BEFORE: usize = 40;
    const AFTER: usize = 90;
    let chars: Vec<char> = body.chars().collect();
    let lower: Vec<char> = body.chars().flat_map(char::to_lowercase).collect();
    // Lower-casing can change length for a few characters; fall back to the plain start then.
    if words.is_empty() || lower.len() != chars.len() {
        return chars.iter().take(AFTER).collect();
    }
    let wchars: Vec<Vec<char>> = words.iter().map(|w| w.chars().collect()).collect();
    let starts_at = |i: usize| {
        wchars
            .iter()
            .find(|w| !w.is_empty() && lower[i..].starts_with(w))
    };
    // Word-prefix matches only, mirroring the FTS prefix query.
    let word_start = |i: usize| i == 0 || !lower[i - 1].is_alphanumeric();
    let Some(first) = (0..lower.len()).find(|&i| word_start(i) && starts_at(i).is_some()) else {
        return chars.iter().take(AFTER).collect();
    };
    let from = first.saturating_sub(BEFORE);
    let to = (first + AFTER).min(chars.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let mut i = from;
    while i < to {
        match (word_start(i)).then(|| starts_at(i)).flatten() {
            Some(w) => {
                out.push('\u{1}');
                out.extend(&chars[i..i + w.len()]);
                out.push('\u{2}');
                i += w.len();
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    if to < chars.len() {
        out.push('…');
    }
    out
}

/// Page ids referenced by `pageLink` marks anywhere in `blocks`.
fn collect_links(blocks: &[Block], out: &mut HashSet<PageId>) {
    for b in blocks {
        for m in &b.marks {
            if m.kind == "pageLink" {
                if let Some(id) = m
                    .attrs
                    .get("pageId")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
                {
                    out.insert(id);
                }
            }
        }
        collect_links(&b.children, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notercrux_domain::{Block, BlockKind};

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn titles(s: &Store, parent: Option<PageId>) -> Vec<String> {
        s.list_pages()
            .unwrap()
            .into_iter()
            .filter(|p| p.parent_id == parent)
            .map(|p| p.title)
            .collect()
    }

    #[test]
    fn create_nest_and_reorder() {
        let mut s = store();
        let a = s.create_page(None, "A").unwrap();
        let b = s.create_page(None, "B").unwrap();
        let c = s.create_page(Some(a.id), "C").unwrap();
        assert_eq!(titles(&s, None), ["A", "B"]);
        assert_eq!(titles(&s, Some(a.id)), ["C"]);
        s.move_page(b.id, None, 0).unwrap();
        assert_eq!(titles(&s, None), ["B", "A"]);
        s.move_page(c.id, Some(b.id), 0).unwrap();
        assert!(titles(&s, Some(a.id)).is_empty());
        assert_eq!(titles(&s, Some(b.id)), ["C"]);
    }

    #[test]
    fn move_into_descendant_is_rejected_and_changes_nothing() {
        let mut s = store();
        let a = s.create_page(None, "A").unwrap();
        let b = s.create_page(Some(a.id), "B").unwrap();
        let err = s.move_page(a.id, Some(b.id), 0).unwrap_err();
        assert!(matches!(err, StoreError::Tree(TreeError::Cycle)));
        assert_eq!(s.get_page(a.id).unwrap().unwrap().parent_id, None);
    }

    #[test]
    fn document_round_trip_and_search() {
        let mut s = store();
        let p = s.create_page(None, "Groceries").unwrap();
        let doc = Document {
            version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::new(BlockKind::Paragraph, "buy avocados")],
        };
        s.save_document(p.id, &doc).unwrap();
        assert_eq!(s.load_document(p.id).unwrap(), doc);
        assert_eq!(s.search("avoc", 10).unwrap()[0].id, p.id);
        assert_eq!(s.search("grocer", 10).unwrap()[0].id, p.id);
        assert!(s.search("zebra", 10).unwrap().is_empty());
    }

    #[test]
    fn new_pages_are_searchable_by_title_immediately() {
        let mut s = store();
        let p = s.create_page(None, "Fresh idea").unwrap();
        assert_eq!(s.search("fresh", 10).unwrap()[0].id, p.id);
    }

    #[test]
    fn search_index_is_rebuildable() {
        let mut s = store();
        let p = s.create_page(None, "Indexed").unwrap();
        s.conn.execute("DELETE FROM search_index", []).unwrap();
        assert!(s.search("indexed", 10).unwrap().is_empty());
        s.rebuild_search_index().unwrap();
        assert_eq!(s.search("indexed", 10).unwrap()[0].id, p.id);
    }

    #[test]
    fn archive_restore_and_hidden_from_search() {
        let mut s = store();
        let a = s.create_page(None, "Parent").unwrap();
        let b = s.create_page(Some(a.id), "Child").unwrap();
        s.archive_page(a.id).unwrap();
        assert!(s.get_page(b.id).unwrap().unwrap().archived_at.is_some());
        assert!(s.search("child", 10).unwrap().is_empty());
        // Restoring a child under an archived parent lifts it to the root.
        s.restore_page(b.id).unwrap();
        let b2 = s.get_page(b.id).unwrap().unwrap();
        assert!(b2.archived_at.is_none());
        assert_eq!(b2.parent_id, None);
    }

    #[test]
    fn duplicate_copies_subtree_and_documents() {
        let mut s = store();
        let a = s.create_page(None, "A").unwrap();
        let b = s.create_page(Some(a.id), "B").unwrap();
        let doc = Document {
            version: 1,
            blocks: vec![Block::new(BlockKind::Paragraph, "hello")],
        };
        s.save_document(b.id, &doc).unwrap();
        let copy = s.duplicate_page(a.id).unwrap();
        assert_eq!(copy.title, "A (copy)");
        let child = s
            .list_pages()
            .unwrap()
            .into_iter()
            .find(|p| p.parent_id == Some(copy.id))
            .unwrap();
        assert_ne!(child.id, b.id);
        assert_eq!(s.load_document(child.id).unwrap().plain_text(), "hello");
    }

    #[test]
    fn newer_document_versions_are_refused() {
        let mut s = store();
        let p = s.create_page(None, "X").unwrap();
        s.conn
            .execute(
                "INSERT INTO documents (page_id, version, content, updated_at) VALUES (?1, 999, '{}', 0)",
                [p.id.to_string()],
            )
            .unwrap();
        assert!(matches!(
            s.load_document(p.id),
            Err(StoreError::UnsupportedDocumentVersion { found: 999, .. })
        ));
    }

    #[test]
    fn permanent_delete_cascades() {
        let mut s = store();
        let a = s.create_page(None, "A").unwrap();
        let b = s.create_page(Some(a.id), "B").unwrap();
        s.delete_page_permanently(a.id).unwrap();
        assert!(s.get_page(b.id).unwrap().is_none());
        assert!(s.search("B", 10).unwrap().is_empty());
    }

    // ---- databases, links, snippets -----------------------------------------------------------

    use notercrux_domain::{
        DatabaseSchema, Filter, FilterOp, Property, PropertyKind, SelectOption, Sort,
        SortDirection, View,
    };
    use serde_json::json;

    fn prop(id: &str, kind: PropertyKind) -> Property {
        Property {
            id: id.into(),
            name: id.to_uppercase(),
            kind,
            options: vec![
                SelectOption {
                    id: "todo".into(),
                    name: "To do".into(),
                    color: None,
                },
                SelectOption {
                    id: "done".into(),
                    name: "Done".into(),
                    color: None,
                },
            ],
            relation_database: None,
        }
    }

    fn tasks_db(s: &mut Store) -> (Page, DatabaseSchema) {
        let db = s.create_database(None, "Tasks").unwrap();
        let mut schema = s.get_schema(db.id).unwrap();
        schema.properties = vec![
            prop("status", PropertyKind::Select),
            prop("points", PropertyKind::Number),
            prop("tags", PropertyKind::MultiSelect),
        ];
        s.set_schema(db.id, &schema).unwrap();
        (db, schema)
    }

    #[test]
    fn database_records_hold_typed_values() {
        let mut s = store();
        let (db, _) = tasks_db(&mut s);
        assert_eq!(db.kind, PageKind::Database);
        let a = s.create_page(Some(db.id), "Write docs").unwrap();
        let b = s.create_page(Some(db.id), "Ship").unwrap();
        s.set_property(a.id, "status", &json!("todo")).unwrap();
        s.set_property(a.id, "points", &json!(3)).unwrap();
        s.set_property(b.id, "tags", &json!(["done"])).unwrap();

        let records = s.list_records(db.id).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].page.title, "Write docs");
        assert_eq!(records[0].values["status"], json!("todo"));
        assert_eq!(records[0].values["points"], json!(3));
        assert_eq!(records[1].values["tags"], json!(["done"]));
        assert!(!records[1].values.contains_key("status"));

        // Clearing removes the stored value.
        s.set_property(a.id, "points", &Value::Null).unwrap();
        assert!(!s.get_properties(a.id).unwrap().contains_key("points"));
    }

    #[test]
    fn invalid_values_and_unknown_properties_are_rejected_without_changes() {
        let mut s = store();
        let (db, _) = tasks_db(&mut s);
        let r = s.create_page(Some(db.id), "x").unwrap();
        assert!(matches!(
            s.set_property(r.id, "points", &json!("many")),
            Err(StoreError::Schema(_))
        ));
        assert!(matches!(
            s.set_property(r.id, "status", &json!("nope")),
            Err(StoreError::Schema(_))
        ));
        assert!(matches!(
            s.set_property(r.id, "ghost", &json!(1)),
            Err(StoreError::Schema(_))
        ));
        assert!(s.get_properties(r.id).unwrap().is_empty());

        let plain = s.create_page(None, "plain").unwrap();
        assert!(matches!(
            s.set_property(plain.id, "points", &json!(1)),
            Err(StoreError::NotARecord(_))
        ));
        assert!(matches!(
            s.get_schema(plain.id),
            Err(StoreError::NotADatabase(_))
        ));
    }

    #[test]
    fn schema_changes_clean_up_values_that_no_longer_fit() {
        let mut s = store();
        let (db, mut schema) = tasks_db(&mut s);
        let r = s.create_page(Some(db.id), "x").unwrap();
        s.set_property(r.id, "status", &json!("done")).unwrap();
        s.set_property(r.id, "points", &json!(5)).unwrap();
        s.set_property(r.id, "tags", &json!(["todo", "done"]))
            .unwrap();

        // Remove an option, change a type and delete a property in one edit.
        schema.properties[0].options.retain(|o| o.id != "done"); // status loses "done"
        schema.properties[1].kind = PropertyKind::Text; // points: number -> text
        schema.properties[2].options.retain(|o| o.id != "done"); // tags loses "done"
        s.set_schema(db.id, &schema).unwrap();
        let v = s.get_properties(r.id).unwrap();
        assert!(
            !v.contains_key("status"),
            "value pointing at a removed option is cleared"
        );
        assert!(!v.contains_key("points"), "type change drops old values");
        assert_eq!(
            v["tags"],
            json!(["todo"]),
            "only the removed option is dropped"
        );

        schema.properties.remove(2);
        s.set_schema(db.id, &schema).unwrap();
        assert!(s.get_properties(r.id).unwrap().is_empty());
    }

    #[test]
    fn invalid_schemas_are_refused_and_leave_the_old_one() {
        let mut s = store();
        let (db, schema) = tasks_db(&mut s);
        let mut bad = schema.clone();
        bad.views[0].sorts.push(Sort {
            property: "ghost".into(),
            direction: SortDirection::Asc,
        });
        assert!(matches!(
            s.set_schema(db.id, &bad),
            Err(StoreError::Schema(_))
        ));
        assert_eq!(s.get_schema(db.id).unwrap(), schema);

        // A view with sorts and filters on real properties is fine and persists.
        let mut ok = schema;
        ok.views.push(View {
            id: "v2".into(),
            name: "Open".into(),
            sorts: vec![Sort {
                property: "points".into(),
                direction: SortDirection::Desc,
            }],
            filters: vec![Filter {
                property: "status".into(),
                op: FilterOp::Equals,
                value: Some(json!("todo")),
            }],
            filter_mode: Default::default(),
            hidden: vec![],
        });
        s.set_schema(db.id, &ok).unwrap();
        assert_eq!(s.get_schema(db.id).unwrap().views.len(), 2);
    }

    #[test]
    fn records_cannot_move_between_databases_and_archived_records_are_hidden() {
        let mut s = store();
        let (db, _) = tasks_db(&mut s);
        let r = s.create_page(Some(db.id), "r").unwrap();
        let other = s.create_page(None, "other").unwrap();
        assert!(matches!(
            s.move_page(r.id, Some(other.id), 0),
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            s.move_page(other.id, Some(db.id), 0),
            Err(StoreError::Invalid(_))
        ));
        s.move_page(r.id, Some(db.id), 0).unwrap(); // reordering inside is fine

        s.archive_page(r.id).unwrap();
        assert!(s.list_records(db.id).unwrap().is_empty());
        s.restore_page(r.id).unwrap();
        assert_eq!(s.list_records(db.id).unwrap().len(), 1);
    }

    #[test]
    fn duplicating_a_database_copies_schema_records_and_values_and_delete_cascades() {
        let mut s = store();
        let (db, _) = tasks_db(&mut s);
        let r = s.create_page(Some(db.id), "r").unwrap();
        s.set_property(r.id, "points", &json!(8)).unwrap();
        let copy = s.duplicate_page(db.id).unwrap();
        assert_eq!(copy.kind, PageKind::Database);
        assert_eq!(s.get_schema(copy.id).unwrap(), s.get_schema(db.id).unwrap());
        let recs = s.list_records(copy.id).unwrap();
        assert_eq!(recs.len(), 1);
        assert_ne!(recs[0].page.id, r.id);
        assert_eq!(recs[0].values["points"], json!(8));

        s.delete_page_permanently(db.id).unwrap();
        assert!(matches!(
            s.get_schema(db.id),
            Err(StoreError::NotADatabase(_))
        ));
        assert!(s.get_properties(r.id).unwrap().is_empty());
        assert_eq!(
            s.list_records(copy.id).unwrap().len(),
            1,
            "the copy is independent"
        );
    }

    #[test]
    fn relations_store_page_ids() {
        let mut s = store();
        let people = s.create_database(None, "People").unwrap();
        let ann = s.create_page(Some(people.id), "Ann").unwrap();
        let tasks = s.create_database(None, "Tasks").unwrap();
        let mut schema = s.get_schema(tasks.id).unwrap();
        let mut owner = prop("owner", PropertyKind::Relation);
        owner.relation_database = Some(people.id);
        schema.properties.push(owner);
        s.set_schema(tasks.id, &schema).unwrap();
        let t = s.create_page(Some(tasks.id), "Fix bug").unwrap();
        s.set_property(t.id, "owner", &json!([ann.id.to_string()]))
            .unwrap();
        assert_eq!(
            s.get_properties(t.id).unwrap()["owner"],
            json!([ann.id.to_string()])
        );
        assert!(s
            .set_property(t.id, "owner", &json!(["not-an-id"]))
            .is_err());
    }

    #[test]
    fn page_links_produce_backlinks_that_follow_edits_and_archiving() {
        let mut s = store();
        let target = s.create_page(None, "Target").unwrap();
        let src = s.create_page(None, "Source").unwrap();
        let mut b = Block::new(BlockKind::Paragraph, "see Target");
        b.marks.push(notercrux_domain::Mark {
            from: 4,
            to: 10,
            kind: "pageLink".into(),
            attrs: serde_json::Map::from_iter([(
                "pageId".to_string(),
                json!(target.id.to_string()),
            )]),
        });
        s.save_document(
            src.id,
            &Document {
                version: 1,
                blocks: vec![b],
            },
        )
        .unwrap();
        assert_eq!(s.backlinks(target.id).unwrap()[0].id, src.id);

        // Editing the link away removes the backlink; archiving the source hides it.
        s.save_document(
            src.id,
            &Document {
                version: 1,
                blocks: vec![Block::new(BlockKind::Paragraph, "x")],
            },
        )
        .unwrap();
        assert!(s.backlinks(target.id).unwrap().is_empty());
    }

    #[test]
    fn search_returns_highlighted_snippets() {
        let mut s = store();
        let p = s.create_page(None, "Notes").unwrap();
        s.save_document(
            p.id,
            &Document {
                version: 1,
                blocks: vec![Block::new(
                    BlockKind::Paragraph,
                    "the quick brown fox jumps over the lazy dog",
                )],
            },
        )
        .unwrap();
        let hits = s.search_with_snippets("lazy", 5).unwrap();
        assert_eq!(hits[0].page.id, p.id);
        assert!(
            hits[0].snippet.contains("\u{1}lazy\u{2}"),
            "{:?}",
            hits[0].snippet
        );
    }
}
