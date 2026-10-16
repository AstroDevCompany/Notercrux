//! Sync support: the outbox, local capture, and transactional application of remote operations.
//!
//! The relational tables stay the queryable local view. When sync is enabled, every local change is
//! also written into CRDT documents (`crdt_docs`); the operations that produced it go to `sync_outbox`.
//! Remote operations are imported into the CRDT documents and the result is **materialized** back into
//! the relational tables, all in one SQLite transaction together with the new sync cursor.
//!
//! Crash safety: a local mutation commits first, then its capture runs in its own transaction. If the
//! app dies in between, `needs_reconcile` (or the next start) diffs the relational state against the
//! CRDT and emits the missing operations, so nothing is lost; the CRDT is only ever *behind*, never
//! ahead of what is stored.

use crate::{now_ms, page_from_row, reindex, Result, Store, StoreError};
use notercrux_crdt::{merge_page_updates, PageCrdt, WorkspaceCrdt};
use notercrux_domain::{
    validate_value, BlockId, DatabaseSchema, Document, Page, PageId, PageKind,
    DOCUMENT_SCHEMA_VERSION,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

/// Version of the operation payload format this build can apply. Operations with a higher number
/// (from a newer app) are held, not dropped, until the app is updated.
pub const SYNC_FORMAT: u32 = 1;

const PAGE_CACHE: usize = 24;
pub const WS_KEY: &str = "ws";

pub fn page_key(id: PageId) -> String {
    format!("page:{id}")
}

pub fn parse_page_key(key: &str) -> Option<PageId> {
    key.strip_prefix("page:")?.parse().ok()
}

/// A local operation waiting to be sent.
#[derive(Clone, Debug)]
pub struct LocalOp {
    pub id: i64,
    pub op_id: String,
    pub doc_key: String,
    pub update: Vec<u8>,
}

/// A decrypted remote operation to apply.
#[derive(Clone, Debug)]
pub struct RemoteOp {
    pub seq: i64,
    pub op_id: String,
    pub doc_key: String,
    pub format: u32,
    pub update: Vec<u8>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub applied: usize,
    /// Operations from a newer app version, kept until this app is updated.
    pub held: usize,
    /// Blocks re-inserted because another device deleted them while this one edited them.
    pub recovered_blocks: usize,
    /// The page tree, titles, schemas or property values changed.
    pub pages_changed: bool,
    /// Pages whose content changed.
    pub documents_changed: Vec<String>,
    pub cursor: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStats {
    pub enabled: bool,
    pub pending_ops: i64,
    pub held_ops: i64,
    pub cursor: i64,
    pub needs_reconcile: bool,
    pub last_error: Option<String>,
}

/// What a local mutation changed; decides which CRDT documents to update.
pub(crate) enum Change {
    PageCreated(PageId),
    PageMeta(PageId),
    DatabaseCreated(PageId),
    TreeChanged,
    SubtreeCreated(Vec<PageId>),
    Deleted,
    Schema(PageId),
    Property(PageId, String),
    Document(PageId),
}

#[derive(Default)]
pub(crate) struct SyncRuntime {
    pub enabled: bool,
    ws: Option<WorkspaceCrdt>,
    pages: HashMap<PageId, PageCrdt>,
    lru: VecDeque<PageId>,
    dirty: HashMap<PageId, HashSet<BlockId>>,
}

impl SyncRuntime {
    fn invalidate(&mut self) {
        self.ws = None;
        self.pages.clear();
        self.lru.clear();
        self.dirty.clear();
    }
}

// ---- loading and saving CRDT documents -------------------------------------------------------

fn read_blob(conn: &Connection, key: &str) -> Result<Option<(Vec<u8>, String)>> {
    Ok(conn
        .query_row(
            "SELECT snapshot, dirty FROM crdt_docs WHERE doc_key = ?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

fn load_ws<'a>(conn: &Connection, rt: &'a mut SyncRuntime) -> Result<&'a WorkspaceCrdt> {
    if rt.ws.is_none() {
        let ws = match read_blob(conn, WS_KEY)? {
            Some((snap, _)) => WorkspaceCrdt::from_snapshot(&snap)?,
            None => WorkspaceCrdt::new(),
        };
        rt.ws = Some(ws);
    }
    Ok(rt.ws.as_ref().expect("just set"))
}

fn load_page<'a>(conn: &Connection, rt: &'a mut SyncRuntime, id: PageId) -> Result<&'a PageCrdt> {
    if !rt.pages.contains_key(&id) {
        let (crdt, dirty) = match read_blob(conn, &page_key(id))? {
            Some((snap, dirty)) => (
                PageCrdt::from_snapshot(&snap)?,
                serde_json::from_str::<Vec<String>>(&dirty)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|s| s.parse().ok())
                    .collect(),
            ),
            None => (PageCrdt::new(), HashSet::new()),
        };
        while rt.pages.len() >= PAGE_CACHE {
            match rt.lru.pop_front() {
                Some(old) => {
                    rt.pages.remove(&old);
                    rt.dirty.remove(&old);
                }
                None => break,
            }
        }
        rt.pages.insert(id, crdt);
        rt.dirty.insert(id, dirty);
    }
    rt.lru.retain(|x| *x != id);
    rt.lru.push_back(id);
    Ok(rt.pages.get(&id).expect("just inserted"))
}

fn save_doc(conn: &Connection, key: &str, snapshot: &[u8], dirty: &HashSet<BlockId>) -> Result<()> {
    let dirty_json =
        serde_json::to_string(&dirty.iter().map(|b| b.to_string()).collect::<Vec<_>>())?;
    conn.execute(
        "INSERT INTO crdt_docs (doc_key, snapshot, dirty, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(doc_key) DO UPDATE SET snapshot = ?2, dirty = ?3, updated_at = ?4",
        params![key, snapshot, dirty_json, now_ms()],
    )?;
    Ok(())
}

fn outbox_push(conn: &Connection, doc_key: &str, update: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_outbox (op_id, doc_key, update_blob, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![uuid_v7(), doc_key, update, now_ms()],
    )?;
    Ok(())
}

fn uuid_v7() -> String {
    notercrux_domain::BlockId::new().to_string()
}

/// Runs a workspace edit and queues the resulting operations.
fn ws_op<R>(
    conn: &Connection,
    rt: &mut SyncRuntime,
    f: impl FnOnce(&WorkspaceCrdt) -> std::result::Result<R, notercrux_crdt::CrdtError>,
) -> Result<R> {
    let ws = load_ws(conn, rt)?;
    let before = ws.version();
    let out = f(ws)?;
    // An unchanged document still exports a small header, so compare versions, not byte lengths.
    if ws.version() == before {
        return Ok(out);
    }
    let update = ws.updates_since(&before)?;
    {
        save_doc(conn, WS_KEY, &ws.snapshot()?, &HashSet::new())?;
        outbox_push(conn, WS_KEY, &update)?;
    }
    Ok(out)
}

pub(crate) fn read_document(conn: &Connection, id: PageId) -> Result<Document> {
    let row: Option<(u32, String)> = conn
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

fn list_pages_conn(conn: &Connection) -> Result<Vec<Page>> {
    let mut stmt =
        conn.prepare("SELECT * FROM pages ORDER BY parent_id IS NOT NULL, parent_id, position")?;
    let rows = stmt.query_map([], page_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn page_conn(conn: &Connection, id: PageId) -> Result<Option<Page>> {
    Ok(conn
        .query_row(
            "SELECT * FROM pages WHERE id = ?1",
            [id.to_string()],
            page_from_row,
        )
        .optional()?)
}

fn schema_conn(conn: &Connection, db: PageId) -> Result<Option<DatabaseSchema>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT schema FROM databases WHERE page_id = ?1",
            [db.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.map(|r| serde_json::from_str(&r)).transpose()?)
}

fn record_values(conn: &Connection, page: PageId) -> Result<HashMap<String, Value>> {
    let mut st =
        conn.prepare("SELECT property_id, value FROM page_properties WHERE page_id = ?1")?;
    let rows = st.query_map([page.to_string()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = HashMap::new();
    for r in rows {
        let (k, v) = r?;
        out.insert(k, serde_json::from_str(&v)?);
    }
    Ok(out)
}

fn capture_document(conn: &Connection, rt: &mut SyncRuntime, id: PageId) -> Result<()> {
    let doc = read_document(conn, id)?;
    let crdt = load_page(conn, rt, id)?;
    let before = crdt.version();
    let mut touched = HashSet::new();
    if !crdt.apply_document_tracked(&doc, &mut touched)? || crdt.version() == before {
        return Ok(());
    }
    let update = crdt.updates_since(&before)?;
    let snapshot = crdt.snapshot()?;
    let dirty = rt.dirty.entry(id).or_default();
    dirty.extend(touched);
    save_doc(conn, &page_key(id), &snapshot, dirty)?;
    outbox_push(conn, &page_key(id), &update)?;
    Ok(())
}

fn capture_record_values(conn: &Connection, rt: &mut SyncRuntime, record: PageId) -> Result<()> {
    let values = record_values(conn, record)?;
    ws_op(conn, rt, |ws| ws.set_record_values(record, &values))
}

fn capture_tree(conn: &Connection, rt: &mut SyncRuntime) -> Result<()> {
    let pages = list_pages_conn(conn)?;
    ws_op(conn, rt, |ws| ws.apply_pages(&pages).map(|_| ()))
}

fn capture_schema(conn: &Connection, rt: &mut SyncRuntime, db: PageId) -> Result<()> {
    if let Some(schema) = schema_conn(conn, db)? {
        ws_op(conn, rt, |ws| ws.set_schema(db, &schema))?;
    }
    Ok(())
}

fn capture(conn: &Connection, rt: &mut SyncRuntime, change: Change) -> Result<()> {
    match change {
        Change::PageCreated(id) | Change::PageMeta(id) => {
            if let Some(p) = page_conn(conn, id)? {
                ws_op(conn, rt, |ws| ws.set_page(&p))?;
            }
        }
        Change::DatabaseCreated(id) => {
            if let Some(p) = page_conn(conn, id)? {
                ws_op(conn, rt, |ws| ws.set_page(&p))?;
            }
            capture_schema(conn, rt, id)?;
        }
        Change::TreeChanged => capture_tree(conn, rt)?,
        Change::SubtreeCreated(ids) => {
            capture_tree(conn, rt)?;
            for id in ids {
                capture_schema(conn, rt, id)?;
                capture_record_values(conn, rt, id)?;
                capture_document(conn, rt, id)?;
            }
        }
        Change::Deleted => {
            // The page's CRDT history is deliberately kept: another device may still be editing the
            // page, and its operations depend on that history. (Compaction of tombstones is future work.)
            capture_tree(conn, rt)?;
        }
        Change::Schema(db) => {
            capture_schema(conn, rt, db)?;
            // The relational layer removes values a schema edit made invalid; mirror that so the CRDT
            // does not resurrect them later.
            let mut st = conn.prepare("SELECT id FROM pages WHERE parent_id = ?1")?;
            let ids: Vec<String> = st
                .query_map([db.to_string()], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in ids.iter().filter_map(|s| s.parse::<PageId>().ok()) {
                capture_record_values(conn, rt, id)?;
            }
        }
        Change::Property(page, prop) => {
            let value = record_values(conn, page)?.remove(&prop);
            ws_op(conn, rt, |ws| ws.set_value(page, &prop, value.as_ref()))?;
        }
        Change::Document(id) => capture_document(conn, rt, id)?,
    }
    Ok(())
}

// ---- materialization: CRDT state -> relational tables ----------------------------------------

#[derive(Default)]
struct WsOutcome {
    changed: bool,
    inserted: Vec<PageId>,
    /// Local pages kept (reparented to the root, archived) because they had unsent edits.
    resurrected: bool,
}

fn kind_str(k: PageKind) -> &'static str {
    match k {
        PageKind::Page => "page",
        PageKind::Database => "database",
    }
}

fn materialize_workspace(
    tx: &Transaction<'_>,
    ws: &WorkspaceCrdt,
    unsent: &HashSet<PageId>,
) -> Result<WsOutcome> {
    let mut out = WsOutcome::default();
    let remote = ws.pages();
    let remote_ids: HashSet<PageId> = remote.iter().map(|p| p.id).collect();
    let existing: HashMap<PageId, Page> = list_pages_conn(tx)?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let now = now_ms();

    // Pages another device deleted, but that hold content this device has not sent yet, are kept
    // (at the root, archived) instead of being destroyed.
    let kept: Vec<PageId> = existing
        .keys()
        .filter(|id| !remote_ids.contains(id) && unsent.contains(id))
        .copied()
        .collect();
    for id in &kept {
        tx.execute(
            "UPDATE pages SET parent_id = NULL, archived_at = COALESCE(archived_at, ?2) WHERE id = ?1",
            params![id.to_string(), now],
        )?;
        out.resurrected = true;
        out.changed = true;
    }
    for id in existing
        .keys()
        .filter(|id| !remote_ids.contains(id) && !kept.contains(id))
    {
        // Cascades to children, documents and property values.
        tx.execute(
            "DELETE FROM search_index WHERE page_id = ?1",
            [id.to_string()],
        )?;
        tx.execute("DELETE FROM pages WHERE id = ?1", [id.to_string()])?;
        out.changed = true;
    }

    for p in &remote {
        match existing.get(&p.id) {
            None => {
                tx.execute(
                    "INSERT INTO pages (id, parent_id, kind, title, icon, position, created_at, updated_at, archived_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        p.id.to_string(),
                        p.parent_id.map(|x| x.to_string()),
                        kind_str(p.kind),
                        p.title,
                        p.icon,
                        p.position,
                        p.created_at,
                        now,
                        p.archived_at
                    ],
                )?;
                reindex(tx, p.id)?;
                out.inserted.push(p.id);
                out.changed = true;
            }
            Some(old) => {
                let same = old.parent_id == p.parent_id
                    && old.kind == p.kind
                    && old.title == p.title
                    && old.icon == p.icon
                    && old.position == p.position
                    && old.archived_at == p.archived_at;
                if !same {
                    tx.execute(
                        "UPDATE pages SET parent_id = ?2, kind = ?3, title = ?4, icon = ?5, position = ?6,
                         archived_at = ?7, updated_at = ?8 WHERE id = ?1",
                        params![
                            p.id.to_string(),
                            p.parent_id.map(|x| x.to_string()),
                            kind_str(p.kind),
                            p.title,
                            p.icon,
                            p.position,
                            p.archived_at,
                            now
                        ],
                    )?;
                    if old.title != p.title {
                        reindex(tx, p.id)?;
                    }
                    out.changed = true;
                }
            }
        }
    }
    Ok(out)
}

fn materialize_schemas_and_values(tx: &Transaction<'_>, ws: &WorkspaceCrdt) -> Result<bool> {
    let mut changed = false;
    let pages = list_pages_conn(tx)?;
    let dbs: HashSet<PageId> = pages
        .iter()
        .filter(|p| p.kind == PageKind::Database)
        .map(|p| p.id)
        .collect();
    let remote_schemas = ws.schemas();

    let mut schemas: HashMap<PageId, DatabaseSchema> = HashMap::new();
    for db in &dbs {
        let existing = schema_conn(tx, *db)?;
        let desired = match remote_schemas.get(db) {
            Some(s) if s.validate().is_ok() => s.clone(),
            _ => existing.clone().unwrap_or_default(),
        };
        let json = serde_json::to_string(&desired)?;
        match &existing {
            None => {
                tx.execute(
                    "INSERT INTO databases (page_id, schema) VALUES (?1, ?2)",
                    params![db.to_string(), json],
                )?;
                changed = true;
            }
            Some(e) if *e != desired => {
                tx.execute(
                    "UPDATE databases SET schema = ?2 WHERE page_id = ?1",
                    params![db.to_string(), json],
                )?;
                changed = true;
            }
            _ => {}
        }
        schemas.insert(*db, desired);
    }
    // A page that stopped being a database no longer has a schema.
    let mut st = tx.prepare("SELECT page_id FROM databases")?;
    let stale: Vec<String> = st
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|s| {
            s.parse::<PageId>()
                .map(|id| !dbs.contains(&id))
                .unwrap_or(true)
        })
        .collect();
    drop(st);
    for s in stale {
        tx.execute("DELETE FROM databases WHERE page_id = ?1", [s])?;
        changed = true;
    }

    // Values: only records of a database, only properties that exist, only values that fit their type.
    // Values that do not fit stay in the CRDT (they are hidden here, not destroyed).
    let remote_values = ws.values();
    let mut desired: HashMap<(String, String), String> = HashMap::new();
    for p in &pages {
        let Some(db) = p.parent_id.filter(|d| dbs.contains(d)) else {
            continue;
        };
        let (Some(schema), Some(vals)) = (schemas.get(&db), remote_values.get(&p.id)) else {
            continue;
        };
        for (prop_id, v) in vals {
            let Some(prop) = schema.property(prop_id) else {
                continue;
            };
            let empty = v.is_null()
                || v.as_str().is_some_and(str::is_empty)
                || v.as_array().is_some_and(Vec::is_empty);
            if !empty && validate_value(prop, v).is_ok() {
                desired.insert(
                    (p.id.to_string(), prop_id.clone()),
                    serde_json::to_string(v)?,
                );
            }
        }
    }
    let mut have: HashMap<(String, String), String> = HashMap::new();
    let mut st = tx.prepare("SELECT page_id, property_id, value FROM page_properties")?;
    for r in st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (a, b, c) = r?;
        have.insert((a, b), c);
    }
    drop(st);
    for (k, _) in have.iter().filter(|(k, _)| !desired.contains_key(*k)) {
        tx.execute(
            "DELETE FROM page_properties WHERE page_id = ?1 AND property_id = ?2",
            params![k.0, k.1],
        )?;
        changed = true;
    }
    for (k, v) in &desired {
        if have.get(k) != Some(v) {
            tx.execute(
                "INSERT INTO page_properties (page_id, property_id, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(page_id, property_id) DO UPDATE SET value = ?3",
                params![k.0, k.1, v],
            )?;
            changed = true;
        }
    }
    Ok(changed)
}

/// Writes a page's merged content into the relational tables. Returns true if it changed.
fn materialize_document(tx: &Transaction<'_>, id: PageId, crdt: &PageCrdt) -> Result<bool> {
    if page_conn(tx, id)?.is_none() {
        return Ok(false); // the page itself has not arrived (or was deleted); content is kept in the CRDT
    }
    let doc = crdt.to_document();
    let have = read_document(tx, id).unwrap_or_default();
    let has_row: bool = tx
        .query_row(
            "SELECT 1 FROM documents WHERE page_id = ?1",
            [id.to_string()],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if doc == have && (has_row || doc.blocks.is_empty()) {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO documents (page_id, version, content, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(page_id) DO UPDATE SET version = ?2, content = ?3, updated_at = ?4",
        params![
            id.to_string(),
            doc.version,
            serde_json::to_string(&doc)?,
            now_ms()
        ],
    )?;
    tx.execute(
        "UPDATE pages SET updated_at = ?2 WHERE id = ?1",
        params![id.to_string(), now_ms()],
    )?;
    reindex(tx, id)?;
    Ok(true)
}

fn unsent_pages(conn: &Connection) -> Result<HashSet<PageId>> {
    let mut st =
        conn.prepare("SELECT DISTINCT doc_key FROM sync_outbox WHERE doc_key LIKE 'page:%'")?;
    let keys: Vec<String> = st
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(keys.iter().filter_map(|k| parse_page_key(k)).collect())
}

// ---- the public surface ---------------------------------------------------------------------

impl Store {
    pub fn sync_enabled(&self) -> bool {
        self.sync.enabled
    }

    pub fn sync_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM sync_state WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn sync_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn sync_cursor(&self) -> Result<i64> {
        Ok(self
            .sync_get("cursor")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }

    pub fn sync_stats(&self) -> Result<SyncStats> {
        let count = |sql: &str| -> Result<i64> { Ok(self.conn.query_row(sql, [], |r| r.get(0))?) };
        Ok(SyncStats {
            enabled: self.sync.enabled,
            pending_ops: count("SELECT COUNT(*) FROM sync_outbox")?,
            held_ops: count("SELECT COUNT(*) FROM sync_held")?,
            cursor: self.sync_cursor()?,
            needs_reconcile: self.sync_get("needs_reconcile")?.as_deref() == Some("1"),
            last_error: self.sync_get("last_error")?,
        })
    }

    /// Turns sync on: builds the CRDT documents from the current relational state (queuing the
    /// operations that describe it). Safe to call again; it only adds what is missing.
    pub fn enable_sync(&mut self) -> Result<()> {
        self.sync_set("enabled", "1")?;
        self.sync.enabled = true;
        self.sync_reconcile()
    }

    /// Turns sync off and forgets all CRDT/outbox state. Pages and content are untouched.
    pub fn disable_sync(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        for t in ["crdt_docs", "sync_outbox", "sync_held", "sync_state"] {
            tx.execute(&format!("DELETE FROM {t}"), [])?;
        }
        tx.commit()?;
        self.sync.enabled = false;
        self.sync.invalidate();
        Ok(())
    }

    /// Deletes every page, document and property, and all sync state. Used when joining an existing
    /// synced workspace and choosing to replace this device's local pages.
    pub fn wipe_workspace(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM search_index", [])?;
        tx.execute("DELETE FROM pages", [])?;
        Store::attachments_wipe(&tx)?;
        for t in ["crdt_docs", "sync_outbox", "sync_held", "sync_state"] {
            tx.execute(&format!("DELETE FROM {t}"), [])?;
        }
        tx.commit()?;
        self.sync.enabled = false;
        self.sync.invalidate();
        Ok(())
    }

    /// Diffs the whole relational state against the CRDT documents and queues operations for any
    /// difference. Used when enabling sync and to repair after an interrupted capture.
    pub fn sync_reconcile(&mut self) -> Result<()> {
        if !self.sync.enabled {
            return Ok(());
        }
        let result = (|| -> Result<()> {
            let tx = self.conn.transaction()?;
            capture_tree(&tx, &mut self.sync)?;
            let pages = list_pages_conn(&tx)?;
            for p in &pages {
                if p.kind == PageKind::Database {
                    capture_schema(&tx, &mut self.sync, p.id)?;
                }
                if p.parent_id.is_some() {
                    capture_record_values(&tx, &mut self.sync, p.id)?;
                }
                capture_document(&tx, &mut self.sync, p.id)?;
            }
            tx.execute("DELETE FROM sync_state WHERE key = 'needs_reconcile'", [])?;
            tx.commit()?;
            Ok(())
        })();
        if result.is_err() {
            self.sync.invalidate();
        }
        result
    }

    /// Records a local change in the CRDT documents. Never fails the caller: the relational change is
    /// already committed, so a failure here only flags that a reconcile is needed.
    pub(crate) fn capture(&mut self, change: Change) {
        if !self.sync.enabled {
            return;
        }
        let result = (|| -> Result<()> {
            let tx = self.conn.transaction()?;
            capture(&tx, &mut self.sync, change)?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(e) = result {
            self.sync.invalidate();
            let _ = self.sync_set("needs_reconcile", "1");
            let _ = self.sync_set("last_error", &format!("capture failed: {e}"));
        }
    }

    /// Operations waiting to be sent, oldest first.
    pub fn sync_pending(&self, limit: u32) -> Result<Vec<LocalOp>> {
        let mut st = self.conn.prepare(
            "SELECT id, op_id, doc_key, update_blob FROM sync_outbox ORDER BY id LIMIT ?1",
        )?;
        let rows = st.query_map([limit], |r| {
            Ok(LocalOp {
                id: r.get(0)?,
                op_id: r.get(1)?,
                doc_key: r.get(2)?,
                update: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The server confirmed these operations. Documents with nothing left to send are no longer
    /// "locally edited".
    pub fn sync_ack(&mut self, op_ids: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        let mut touched = HashSet::new();
        for op in op_ids {
            if let Some(key) = tx
                .query_row(
                    "SELECT doc_key FROM sync_outbox WHERE op_id = ?1",
                    [op],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
            {
                touched.insert(key);
            }
            tx.execute("DELETE FROM sync_outbox WHERE op_id = ?1", [op])?;
        }
        for key in touched {
            let left: i64 = tx.query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE doc_key = ?1",
                [&key],
                |r| r.get(0),
            )?;
            if left == 0 {
                tx.execute(
                    "UPDATE crdt_docs SET dirty = '[]' WHERE doc_key = ?1",
                    [&key],
                )?;
                if let Some(id) = parse_page_key(&key) {
                    self.sync.dirty.remove(&id);
                    // Force the cached copy to reload with its cleared dirty set.
                    self.sync.pages.remove(&id);
                    self.sync.lru.retain(|x| *x != id);
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Applies decrypted remote operations and advances the cursor, atomically.
    pub fn sync_apply_remote(
        &mut self,
        ops: Vec<RemoteOp>,
        new_cursor: i64,
    ) -> Result<ApplyReport> {
        let result = self.apply_remote_inner(ops, Some(new_cursor));
        if result.is_err() {
            self.sync.invalidate();
        }
        result
    }

    /// Applies operations that were held because they came from a newer app, now that this app may
    /// understand them.
    pub fn sync_apply_held(&mut self) -> Result<ApplyReport> {
        let ops: Vec<RemoteOp> = {
            let mut st = self.conn.prepare("SELECT seq, op_id, doc_key, format, update_blob FROM sync_held WHERE format <= ?1 ORDER BY seq")?;
            let rows = st.query_map([SYNC_FORMAT], |r| {
                Ok(RemoteOp {
                    seq: r.get(0)?,
                    op_id: r.get(1)?,
                    doc_key: r.get(2)?,
                    format: r.get(3)?,
                    update: r.get(4)?,
                })
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        if ops.is_empty() {
            return Ok(ApplyReport {
                cursor: self.sync_cursor()?,
                ..Default::default()
            });
        }
        let seqs: Vec<i64> = ops.iter().map(|o| o.seq).collect();
        let result = self.apply_remote_inner(ops, None);
        match result {
            Ok(r) => {
                for s in seqs {
                    self.conn
                        .execute("DELETE FROM sync_held WHERE seq = ?1", [s])?;
                }
                Ok(r)
            }
            Err(e) => {
                self.sync.invalidate();
                Err(e)
            }
        }
    }

    fn apply_remote_inner(
        &mut self,
        ops: Vec<RemoteOp>,
        cursor: Option<i64>,
    ) -> Result<ApplyReport> {
        let mut report = ApplyReport::default();
        let tx = self.conn.transaction()?;
        let rt = &mut self.sync;

        // Operations from a newer app are kept, not applied and not dropped.
        let (supported, newer): (Vec<RemoteOp>, Vec<RemoteOp>) =
            ops.into_iter().partition(|o| o.format <= SYNC_FORMAT);
        for o in &newer {
            tx.execute(
                "INSERT OR REPLACE INTO sync_held (seq, op_id, doc_key, format, update_blob) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![o.seq, o.op_id, o.doc_key, o.format, o.update],
            )?;
        }
        report.held = newer.len();

        let mut ws_updates: Vec<&RemoteOp> = Vec::new();
        let mut page_updates: HashMap<PageId, Vec<&RemoteOp>> = HashMap::new();
        for o in &supported {
            if o.doc_key == WS_KEY {
                ws_updates.push(o);
            } else if let Some(id) = parse_page_key(&o.doc_key) {
                page_updates.entry(id).or_default().push(o);
            }
            // Unknown document kinds are ignored (a future format would have a higher number).
        }
        report.applied = supported.len();

        // 1. Workspace document: import, then rebuild the page tree, schemas and values.
        let mut inserted: Vec<PageId> = Vec::new();
        let mut resurrected = false;
        let ws_touched = !ws_updates.is_empty();
        if ws_touched {
            let unsent = unsent_pages(&tx)?;
            let ws = load_ws(&tx, rt)?;
            for u in &ws_updates {
                ws.import(&u.update)?;
            }
            save_doc(&tx, WS_KEY, &ws.snapshot()?, &HashSet::new())?;
            let outcome = materialize_workspace(&tx, ws, &unsent)?;
            let values_changed = materialize_schemas_and_values(&tx, ws)?;
            report.pages_changed = outcome.changed || values_changed;
            inserted = outcome.inserted;
            resurrected = outcome.resurrected;
        }

        // 2. Page documents: merge without silently losing local edits, then materialize.
        let mut to_materialize: HashSet<PageId> = page_updates.keys().copied().collect();
        to_materialize.extend(inserted.iter().copied());
        for (id, updates) in &page_updates {
            load_page(&tx, rt, *id)?; // also loads the page's "edited locally" set into rt.dirty
            let dirty = rt.dirty.get(id).cloned().unwrap_or_default();
            let blobs: Vec<Vec<u8>> = updates.iter().map(|u| u.update.clone()).collect();
            let crdt = rt.pages.get(id).expect("just loaded");
            let outcome = merge_page_updates(crdt, &dirty, &blobs)?;
            if let Some(local) = outcome.local_update.filter(|u| !u.is_empty()) {
                // The recovery created operations of our own: they must be sent.
                outbox_push(&tx, &page_key(*id), &local)?;
                report.recovered_blocks += outcome.recovered;
            }
            save_doc(&tx, &page_key(*id), &crdt.snapshot()?, &dirty)?;
        }
        for id in &to_materialize {
            if !page_updates.contains_key(id) {
                // A page that arrived after (or without) any content operation in this batch: its
                // content may already be stored from an earlier batch.
                if read_blob(&tx, &page_key(*id))?.is_none() {
                    continue;
                }
            }
            let crdt = load_page(&tx, rt, *id)?;
            if materialize_document(&tx, *id, crdt)? {
                report.documents_changed.push(id.to_string());
            }
        }

        if resurrected {
            // Kept pages must exist in the CRDT again, so the other devices see them.
            capture_tree(&tx, rt)?;
            report.pages_changed = true;
        }
        if let Some(c) = cursor {
            tx.execute(
                "INSERT INTO sync_state (key, value) VALUES ('cursor', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
                [c.to_string()],
            )?;
            report.cursor = c;
        }
        tx.commit()?;
        if report.cursor == 0 {
            report.cursor = self.sync_cursor()?;
        }
        Ok(report)
    }
}
