//! Upgrading a database created by an earlier release must keep every page and document.

use notercrux_domain::{Block, BlockKind, Document, PageKind};
use notercrux_storage::Store;
use rusqlite::{params, Connection};

/// Builds a database exactly as release "Phase 2" would have: only migration 0001 applied, with
/// pages, a nested page, a document and a search-index row.
fn v1_database(path: &std::path::Path) -> (String, String) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(include_str!("../migrations/0001_init.sql"))
        .unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_migrations (name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL);
         INSERT INTO schema_migrations VALUES ('0001_init', 0);",
    )
    .unwrap();
    let (root, child) = (
        "0190a000-0000-7000-8000-000000000001".to_string(),
        "0190a000-0000-7000-8000-000000000002".to_string(),
    );
    for (id, parent, title, pos) in [
        (&root, None, "Old page", 0),
        (&child, Some(&root), "Old child", 0),
    ] {
        conn.execute(
            "INSERT INTO pages (id, parent_id, title, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, 1)",
            params![id, parent, title, pos],
        )
        .unwrap();
    }
    let doc = Document {
        version: 1,
        blocks: vec![Block::new(BlockKind::Paragraph, "legacy content survives")],
    };
    conn.execute(
        "INSERT INTO documents (page_id, version, content, updated_at) VALUES (?1, 1, ?2, 1)",
        params![root, serde_json::to_string(&doc).unwrap()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO search_index (page_id, title, body) VALUES (?1, 'Old page', 'legacy content survives')",
        [&root],
    )
    .unwrap();
    (root, child)
}

#[test]
fn upgrading_a_v1_database_keeps_pages_documents_and_search() {
    let dir = std::env::temp_dir().join(format!("notercrux-mig-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v1.db");
    let _ = std::fs::remove_file(&path);
    let (root, child) = v1_database(&path);

    let mut store = Store::open(&path).unwrap(); // applies 0002
    let pages = store.list_pages().unwrap();
    assert_eq!(pages.len(), 2);
    assert!(
        pages.iter().all(|p| p.kind == PageKind::Page),
        "old pages become ordinary pages"
    );
    let root_id = root.parse().unwrap();
    assert_eq!(
        store.load_document(root_id).unwrap().plain_text(),
        "legacy content survives"
    );
    assert_eq!(store.search("legacy", 5).unwrap()[0].id, root_id);
    assert_eq!(
        pages
            .iter()
            .find(|p| p.id.to_string() == child)
            .unwrap()
            .parent_id,
        Some(root_id)
    );

    // New features work on the upgraded database, and the link cache is built on demand.
    let db = store.create_database(None, "New db").unwrap();
    assert_eq!(db.kind, PageKind::Database);
    store.rebuild_search_index().unwrap();
    assert_eq!(store.search("legacy", 5).unwrap().len(), 1);

    // Reopening is a no-op (migrations are recorded) and loses nothing.
    drop(store);
    let again = Store::open(&path).unwrap();
    assert_eq!(again.list_pages().unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}
