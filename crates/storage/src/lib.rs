//! SQLite persistence.
//!
//! `pages` and `documents` are canonical. `search_index` is a cache that can be dropped and
//! rebuilt from them at any time via [`Store::rebuild_search_index`].

use notercrux_domain::{Document, Page, PageId, PageTree, TreeError, DOCUMENT_SCHEMA_VERSION};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("document encoding error: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error("document schema version {found} is newer than supported version {supported}")]
    UnsupportedDocumentVersion { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, StoreError>;

const MIGRATIONS: &[(&str, &str)] = &[("0001_init", include_str!("../migrations/0001_init.sql"))];

pub struct Store {
    conn: Connection,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn page_from_row(r: &Row<'_>) -> rusqlite::Result<Page> {
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
        let mut store = Self { conn };
        store.migrate()?;
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
        Ok(())
    }

    pub fn set_icon(&mut self, id: PageId, icon: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE pages SET icon = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), icon, now_ms()],
        )?;
        Ok(())
    }

    /// Moves `id` under `new_parent` (or to the root) at `index` among its new siblings.
    pub fn move_page(&mut self, id: PageId, new_parent: Option<PageId>, index: u32) -> Result<()> {
        let pages = self.list_pages()?;
        PageTree::from_pages(&pages).check_move(id, new_parent)?;
        let old_parent = pages.iter().find(|p| p.id == id).and_then(|p| p.parent_id);

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
                "INSERT INTO pages (id, parent_id, title, icon, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    new_id.to_string(),
                    parent.map(|x| x.to_string()),
                    title,
                    p.icon,
                    position,
                    ts
                ],
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
        Ok(())
    }

    /// Permanently removes a page and its descendants (documents cascade).
    pub fn delete_page_permanently(&mut self, id: PageId) -> Result<()> {
        let pages = self.list_pages()?;
        let ids = PageTree::from_pages(&pages).subtree(id);
        let tx = self.conn.transaction()?;
        for i in ids {
            tx.execute(
                "DELETE FROM search_index WHERE page_id = ?1",
                [i.to_string()],
            )?;
        }
        tx.execute("DELETE FROM pages WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
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
        Ok(())
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

fn reindex(tx: &Transaction<'_>, id: PageId) -> Result<()> {
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
    let body = match content {
        Some(c) => serde_json::from_str::<Document>(&c)
            .map(|d| d.plain_text())
            .unwrap_or_default(),
        None => String::new(),
    };
    tx.execute("DELETE FROM search_index WHERE page_id = ?1", [&id_s])?;
    tx.execute(
        "INSERT INTO search_index (page_id, title, body) VALUES (?1, ?2, ?3)",
        params![id_s, title, body],
    )?;
    Ok(())
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
}
