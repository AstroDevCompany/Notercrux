//! Two (or three) real stores exchanging operations directly, without encryption or a server.
//! The network and crypto layers have their own tests; this checks that what the *data* layer does
//! with operations is correct: capture, merge, materialize, recovery, and crash repair.

use notercrux_domain::{
    Block, BlockKind, DatabaseSchema, Document, Page, PageId, PageKind, Property, PropertyKind,
    SelectOption,
};
use notercrux_storage::{ApplyReport, RemoteOp, Store, SYNC_FORMAT};
use serde_json::json;

/// A tiny stand-in for the server: one global, ordered operation log. Each device uploads its queued
/// operations to it and downloads what others uploaded since its own cursor.
struct Net {
    log: Vec<(String, RemoteOp)>,
    cursors: std::collections::HashMap<String, i64>,
    seq: i64,
}

impl Net {
    fn new() -> Self {
        Self {
            log: Vec::new(),
            cursors: Default::default(),
            seq: 0,
        }
    }

    /// Upload `store`'s queued operations (the server acks them, so the outbox empties).
    fn push(&mut self, name: &str, store: &mut Store) {
        let ops = store.sync_pending(10_000).unwrap();
        let ids: Vec<String> = ops.iter().map(|o| o.op_id.clone()).collect();
        for o in ops {
            self.seq += 1;
            self.log.push((
                name.to_string(),
                RemoteOp {
                    seq: self.seq,
                    op_id: o.op_id,
                    doc_key: o.doc_key,
                    format: SYNC_FORMAT,
                    update: o.update,
                },
            ));
        }
        store.sync_ack(&ids).unwrap();
    }

    /// Download what other devices uploaded since this device's cursor.
    fn pull(&mut self, name: &str, store: &mut Store) -> ApplyReport {
        let cursor = *self.cursors.get(name).unwrap_or(&0);
        let ops: Vec<RemoteOp> = self
            .log
            .iter()
            .filter(|(from, o)| from != name && o.seq > cursor)
            .map(|(_, o)| o.clone())
            .collect();
        let report = store.sync_apply_remote(ops, self.seq).unwrap();
        self.cursors.insert(name.to_string(), self.seq);
        report
    }

    fn sync(&mut self, name: &str, store: &mut Store) -> ApplyReport {
        self.push(name, store);
        self.pull(name, store)
    }

    fn settle(&mut self, a: &mut Store, b: &mut Store) {
        for _ in 0..3 {
            self.sync("a", a);
            self.sync("b", b);
        }
    }
}

fn synced() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.enable_sync().unwrap();
    s
}

fn para(text: &str) -> Block {
    Block::new(BlockKind::Paragraph, text)
}

fn doc(blocks: Vec<Block>) -> Document {
    Document { version: 1, blocks }
}

fn titles(s: &Store) -> Vec<String> {
    let mut v: Vec<String> = s
        .list_pages()
        .unwrap()
        .into_iter()
        .filter(|p| p.archived_at.is_none())
        .map(|p| p.title)
        .collect();
    v.sort();
    v
}

fn texts(s: &Store, id: PageId) -> Vec<String> {
    s.load_document(id)
        .unwrap()
        .blocks
        .iter()
        .map(|b| b.text.clone())
        .collect()
}

fn same_state(a: &Store, b: &Store) {
    let norm = |s: &Store| {
        let mut pages = s.list_pages().unwrap();
        pages.sort_by_key(|p| p.id);
        pages
            .into_iter()
            .map(|p| {
                (
                    p.id,
                    p.parent_id,
                    p.kind,
                    p.title,
                    p.icon,
                    p.position,
                    p.archived_at,
                    s.load_document(p.id).unwrap(),
                    {
                        let mut v: Vec<_> = s.get_properties(p.id).unwrap().into_iter().collect();
                        v.sort_by(|x, y| x.0.cmp(&y.0));
                        v
                    },
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(norm(a), norm(b));
}

// ---- basics --------------------------------------------------------------------------------

#[test]
fn a_new_device_receives_the_whole_workspace() {
    let mut a = synced();
    let mut b = synced();
    let mut net = Net::new();
    let home = a.create_page(None, "Home").unwrap();
    let child = a.create_page(Some(home.id), "Notes").unwrap();
    a.set_icon(home.id, Some("🏠")).unwrap();
    a.save_document(child.id, &doc(vec![para("hello from A")]))
        .unwrap();

    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert_eq!(titles(&b), ["Home", "Notes"]);
    assert_eq!(texts(&b, child.id), ["hello from A"]);
    assert_eq!(
        b.get_page(home.id).unwrap().unwrap().icon.as_deref(),
        Some("🏠")
    );
    assert_eq!(
        b.search("hello", 5).unwrap()[0].id,
        child.id,
        "the search index follows synced content"
    );
}

#[test]
fn enabling_sync_on_a_workspace_with_existing_data_uploads_all_of_it() {
    let mut a = Store::open_in_memory().unwrap();
    let p = a.create_page(None, "Existing").unwrap();
    let c = a.create_page(Some(p.id), "Nested").unwrap();
    a.save_document(c.id, &doc(vec![para("old content")]))
        .unwrap();
    let db = a.create_database(None, "Tasks").unwrap();
    let mut schema = a.get_schema(db.id).unwrap();
    schema.properties.push(Property {
        id: "n".into(),
        name: "N".into(),
        kind: PropertyKind::Number,
        options: vec![],
        relation_database: None,
    });
    a.set_schema(db.id, &schema).unwrap();
    let r = a.create_page(Some(db.id), "Row").unwrap();
    a.set_property(r.id, "n", &json!(7)).unwrap();
    assert!(
        a.sync_pending(10).unwrap().is_empty(),
        "nothing is captured before sync is enabled"
    );

    a.enable_sync().unwrap();
    let mut b = synced();
    Net::new().settle(&mut a, &mut b);
    same_state(&a, &b);
    assert_eq!(b.get_properties(r.id).unwrap()["n"], json!(7));
    assert_eq!(b.get_schema(db.id).unwrap().properties.len(), 1);
    assert_eq!(b.list_records(db.id).unwrap().len(), 1);
}

#[test]
fn local_changes_queue_operations_and_acks_clear_them() {
    let mut a = synced();
    assert!(a.sync_pending(10).unwrap().is_empty());
    let p = a.create_page(None, "x").unwrap();
    a.rename_page(p.id, "y").unwrap();
    let ops = a.sync_pending(100).unwrap();
    assert!(!ops.is_empty());
    assert!(ops.iter().all(|o| o.doc_key == "ws"));
    assert_eq!(a.sync_stats().unwrap().pending_ops as usize, ops.len());
    let ids: Vec<String> = ops.iter().map(|o| o.op_id.clone()).collect();
    a.sync_ack(&ids).unwrap();
    assert!(a.sync_pending(10).unwrap().is_empty());
    a.save_document(p.id, &doc(vec![para("c")])).unwrap();
    assert_eq!(
        a.sync_pending(10).unwrap()[0].doc_key,
        format!("page:{}", p.id)
    );
}

// ---- the blueprint's multi-device scenarios ------------------------------------------------

#[test]
fn two_devices_edit_different_parts_of_the_same_page_offline() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let p = a.create_page(None, "Page").unwrap();
    a.save_document(p.id, &doc(vec![para("one"), para("two"), para("three")]))
        .unwrap();
    net.settle(&mut a, &mut b);

    // Offline on both.
    let mut da = a.load_document(p.id).unwrap();
    da.blocks[0].text = "one (A)".into();
    a.save_document(p.id, &da).unwrap();
    let mut db = b.load_document(p.id).unwrap();
    db.blocks[2].text = "three (B)".into();
    db.blocks.push(para("added on B"));
    b.save_document(p.id, &db).unwrap();

    net.settle(&mut a, &mut b);
    assert_eq!(
        texts(&a, p.id),
        ["one (A)", "two", "three (B)", "added on B"]
    );
    same_state(&a, &b);
}

#[test]
fn two_devices_type_into_the_same_text_at_once() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let p = a.create_page(None, "P").unwrap();
    a.save_document(p.id, &doc(vec![para("hello world")]))
        .unwrap();
    net.settle(&mut a, &mut b);
    let mut da = a.load_document(p.id).unwrap();
    da.blocks[0].text = "Oh, hello world!".into();
    a.save_document(p.id, &da).unwrap();
    let mut db = b.load_document(p.id).unwrap();
    db.blocks[0].text = "hello, world (waves)".into();
    b.save_document(p.id, &db).unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    let merged = texts(&a, p.id)[0].clone();
    for frag in ["Oh, ", "!", ",", " (waves)", "hello", "world"] {
        assert!(merged.contains(frag), "lost {frag:?} in {merged:?}");
    }
}

#[test]
fn one_device_deletes_a_block_while_another_edits_it_and_the_edit_is_recovered() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let p = a.create_page(None, "P").unwrap();
    a.save_document(p.id, &doc(vec![para("keep"), para("draft")]))
        .unwrap();
    net.settle(&mut a, &mut b);

    let mut da = a.load_document(p.id).unwrap();
    da.blocks.retain(|x| x.text != "draft");
    a.save_document(p.id, &da).unwrap();
    let mut db = b.load_document(p.id).unwrap();
    db.blocks[1].text = "draft plus an important edit".into();
    b.save_document(p.id, &db).unwrap();

    // B's edit is confirmed by the server and reaches A, which had already deleted the block: A (the
    // deleter) recovers it. Then A's delete (and the recovery) reach B, which recovers nothing again.
    net.push("b", &mut b);
    let on_a = net.pull("a", &mut a);
    assert_eq!(on_a.recovered_blocks, 1, "B edited a block that A deleted");
    net.push("a", &mut a);
    let on_b = net.pull("b", &mut b);
    assert_eq!(
        on_b.recovered_blocks, 0,
        "the text must not be recovered twice"
    );
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    let all = {
        fn flat(bs: &[Block], out: &mut Vec<String>) {
            for b in bs {
                out.push(b.text.clone());
                flat(&b.children, out);
            }
        }
        let mut v = Vec::new();
        flat(&a.load_document(p.id).unwrap().blocks, &mut v);
        v.join(" | ")
    };
    assert!(
        all.contains("draft plus an important edit"),
        "the edit must not be lost: {all}"
    );
    assert!(all.contains("keep"));
    let last = a.load_document(p.id).unwrap().blocks.pop().unwrap();
    assert_eq!(
        last.kind,
        BlockKind::Callout,
        "recovered edits are collected in a callout"
    );
}

#[test]
fn a_page_moves_on_one_device_while_its_content_is_edited_on_another() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let target = a.create_page(None, "Target").unwrap();
    let p = a.create_page(None, "Moving").unwrap();
    a.save_document(p.id, &doc(vec![para("body")])).unwrap();
    net.settle(&mut a, &mut b);

    a.move_page(p.id, Some(target.id), 0).unwrap();
    let mut db = b.load_document(p.id).unwrap();
    db.blocks[0].text = "body edited on B".into();
    b.save_document(p.id, &db).unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert_eq!(
        a.get_page(p.id).unwrap().unwrap().parent_id,
        Some(target.id)
    );
    assert_eq!(texts(&a, p.id), ["body edited on B"]);
}

#[test]
fn a_device_offline_for_a_long_time_catches_up() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let p = a.create_page(None, "Log").unwrap();
    net.settle(&mut a, &mut b);
    // A makes many changes while B is away; B makes one.
    let mut d = Document::default();
    for i in 0..60 {
        d.blocks.push(para(&format!("entry {i}")));
        a.save_document(p.id, &d).unwrap();
        if i % 10 == 0 {
            a.create_page(Some(p.id), &format!("sub {i}")).unwrap();
        }
    }
    b.create_page(None, "B's page").unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert_eq!(texts(&b, p.id).len(), 60);
    assert!(titles(&b).contains(&"B's page".to_string()));
}

#[test]
fn replaying_and_reordering_deliveries_changes_nothing() {
    let (mut a, mut b) = (synced(), synced());
    let p = a.create_page(None, "P").unwrap();
    a.save_document(p.id, &doc(vec![para("v1")])).unwrap();
    let mut d = a.load_document(p.id).unwrap();
    d.blocks[0].text = "v2".into();
    a.save_document(p.id, &d).unwrap();
    let ops = a.sync_pending(100).unwrap();
    let mk = |o: &notercrux_storage::LocalOp, seq: i64| RemoteOp {
        seq,
        op_id: o.op_id.clone(),
        doc_key: o.doc_key.clone(),
        format: SYNC_FORMAT,
        update: o.update.clone(),
    };

    // Deliver the batch reversed, then again in order, then a duplicate: same end state.
    let rev: Vec<RemoteOp> = ops
        .iter()
        .rev()
        .enumerate()
        .map(|(i, o)| mk(o, i as i64 + 1))
        .collect();
    b.sync_apply_remote(rev, 10).unwrap();
    let fwd: Vec<RemoteOp> = ops
        .iter()
        .enumerate()
        .map(|(i, o)| mk(o, 20 + i as i64))
        .collect();
    b.sync_apply_remote(fwd.clone(), 30).unwrap();
    b.sync_apply_remote(fwd, 40).unwrap();
    same_state(&a, &b);
    assert_eq!(b.sync_cursor().unwrap(), 40);
    assert!(
        b.sync_pending(10).unwrap().is_empty(),
        "applying remote operations must not queue echoes"
    );
}

// ---- structure, archive, delete ------------------------------------------------------------

#[test]
fn rename_archive_restore_duplicate_and_delete_propagate() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let p = a.create_page(None, "Parent").unwrap();
    let c = a.create_page(Some(p.id), "Child").unwrap();
    a.save_document(c.id, &doc(vec![para("child text")]))
        .unwrap();
    net.settle(&mut a, &mut b);

    a.rename_page(p.id, "Parent (renamed)").unwrap();
    a.archive_page(p.id).unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert!(b.get_page(c.id).unwrap().unwrap().archived_at.is_some());

    a.restore_page(p.id).unwrap();
    let copy = a.duplicate_page(p.id).unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert!(titles(&b).contains(&copy.title));
    let copied_child = b
        .list_pages()
        .unwrap()
        .into_iter()
        .find(|x| x.parent_id == Some(copy.id))
        .unwrap();
    assert_eq!(
        texts(&b, copied_child.id),
        ["child text"],
        "a duplicated page arrives with its content"
    );

    a.archive_page(copy.id).unwrap();
    a.delete_page_permanently(copy.id).unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert!(b.get_page(copy.id).unwrap().is_none());
}

#[test]
fn a_page_deleted_on_one_device_but_edited_on_another_is_kept_not_destroyed() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let p = a.create_page(None, "Doomed").unwrap();
    a.save_document(p.id, &doc(vec![para("original")])).unwrap();
    net.settle(&mut a, &mut b);

    a.archive_page(p.id).unwrap();
    a.delete_page_permanently(p.id).unwrap();
    let mut d = b.load_document(p.id).unwrap();
    d.blocks[0].text = "original, with B's precious new notes".into();
    b.save_document(p.id, &d).unwrap();

    net.push("a", &mut a);
    net.pull("b", &mut b); // the deletion reaches B, which has unsent edits
    let kept = b
        .get_page(p.id)
        .unwrap()
        .expect("B's page must survive the remote deletion");
    assert!(
        kept.archived_at.is_some() && kept.parent_id.is_none(),
        "kept at the root, archived"
    );
    assert_eq!(texts(&b, p.id), ["original, with B's precious new notes"]);
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert!(
        a.get_page(p.id).unwrap().is_some(),
        "the kept page is shared again"
    );
}

#[test]
fn page_ordering_and_reparenting_converge_across_three_devices() {
    let (mut a, mut b, mut c, mut net) = (synced(), synced(), synced(), Net::new());
    let x = a.create_page(None, "X").unwrap();
    let y = a.create_page(None, "Y").unwrap();
    let z = a.create_page(None, "Z").unwrap();
    net.sync("a", &mut a);
    net.sync("b", &mut b);
    net.sync("c", &mut c);
    // Concurrent restructuring from three devices.
    a.move_page(x.id, Some(y.id), 0).unwrap();
    b.move_page(y.id, Some(z.id), 0).unwrap();
    c.rename_page(z.id, "Z renamed").unwrap();
    for _ in 0..3 {
        net.sync("a", &mut a);
        net.sync("b", &mut b);
        net.sync("c", &mut c);
    }
    same_state(&a, &b);
    same_state(&a, &c);
    assert_eq!(a.list_pages().unwrap().len(), 3);
}

// ---- databases -----------------------------------------------------------------------------

fn tasks_db(s: &mut Store) -> (Page, Page) {
    let db = s.create_database(None, "Tasks").unwrap();
    let mut schema = s.get_schema(db.id).unwrap();
    schema.properties = vec![
        Property {
            id: "status".into(),
            name: "Status".into(),
            kind: PropertyKind::Select,
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
        },
        Property {
            id: "points".into(),
            name: "Points".into(),
            kind: PropertyKind::Number,
            options: vec![],
            relation_database: None,
        },
    ];
    s.set_schema(db.id, &schema).unwrap();
    let rec = s.create_page(Some(db.id), "Write docs").unwrap();
    (db, rec)
}

#[test]
fn databases_schema_records_and_values_sync_and_concurrent_cell_edits_merge() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let (db, rec) = tasks_db(&mut a);
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    assert_eq!(b.get_schema(db.id).unwrap().properties.len(), 2);
    assert_eq!(b.get_page(db.id).unwrap().unwrap().kind, PageKind::Database);

    // Different cells of the same record, first-ever values, edited at once on two devices.
    a.set_property(rec.id, "status", &json!("todo")).unwrap();
    b.set_property(rec.id, "points", &json!(5)).unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    let v = a.get_properties(rec.id).unwrap();
    assert_eq!(
        (v["status"].clone(), v["points"].clone()),
        (json!("todo"), json!(5)),
        "neither first edit may be lost"
    );
}

#[test]
fn a_schema_change_on_one_device_and_a_value_edit_on_another_never_corrupt_each_other() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let (db, rec) = tasks_db(&mut a);
    a.set_property(rec.id, "points", &json!(3)).unwrap();
    net.settle(&mut a, &mut b);

    // A turns "points" into text (which clears old values); B concurrently edits points as a number.
    let mut schema = a.get_schema(db.id).unwrap();
    schema.properties[1].kind = PropertyKind::Text;
    a.set_schema(db.id, &schema).unwrap();
    b.set_property(rec.id, "points", &json!(99)).unwrap();
    net.settle(&mut a, &mut b);

    same_state(&a, &b);
    // The merged schema says text; a number no longer fits, so it is hidden, not stored as a bad value.
    assert_eq!(
        a.get_schema(db.id).unwrap().properties[1].kind,
        PropertyKind::Text
    );
    assert!(!a.get_properties(rec.id).unwrap().contains_key("points"));
    assert!(a.get_schema(db.id).unwrap().validate().is_ok());
    // And ordinary use afterwards works on both.
    a.set_property(rec.id, "points", &json!("high")).unwrap();
    net.settle(&mut a, &mut b);
    assert_eq!(b.get_properties(rec.id).unwrap()["points"], json!("high"));
}

#[test]
fn deleting_a_property_on_one_device_cleans_views_edited_on_another() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let (db, _rec) = tasks_db(&mut a);
    net.settle(&mut a, &mut b);
    let mut sa = a.get_schema(db.id).unwrap();
    sa.properties.retain(|p| p.id != "points");
    a.set_schema(db.id, &sa).unwrap();
    let mut sb = b.get_schema(db.id).unwrap();
    sb.views[0].sorts.push(notercrux_domain::Sort {
        property: "points".into(),
        direction: notercrux_domain::SortDirection::Asc,
    });
    b.set_schema(db.id, &sb).unwrap();
    net.settle(&mut a, &mut b);
    same_state(&a, &b);
    let merged: DatabaseSchema = a.get_schema(db.id).unwrap();
    assert!(merged.validate().is_ok());
    assert!(merged.views[0].sorts.is_empty());
}

// ---- robustness ----------------------------------------------------------------------------

#[test]
fn an_interrupted_capture_is_repaired_by_reconcile() {
    let (mut a, mut b, mut net) = (synced(), synced(), Net::new());
    let p = a.create_page(None, "Before").unwrap();
    net.settle(&mut a, &mut b);

    // Simulate a crash between the relational commit and the capture: change the tables directly.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    drop(conn);
    // (a real crash cannot be injected into an in-memory store, so flag the same state explicitly:
    // a mutation whose capture failed leaves `needs_reconcile` set and the CRDT behind.)
    a.rename_page(p.id, "After").unwrap();
    // Throw away the queued operations to model "the capture never happened".
    let lost: Vec<String> = a
        .sync_pending(100)
        .unwrap()
        .into_iter()
        .map(|o| o.op_id)
        .collect();
    a.sync_ack(&lost).unwrap();
    assert_eq!(b.get_page(p.id).unwrap().unwrap().title, "Before");

    // Reconcile only diffs what the CRDT does not yet say. Since the CRDT already holds "After"
    // (only its *sending* was lost), the operations to resend come from a full resync instead.
    a.sync_set("needs_reconcile", "1").unwrap();
    a.sync_reconcile().unwrap();
    assert!(!a.sync_stats().unwrap().needs_reconcile);
}

#[test]
fn reconcile_emits_operations_for_relational_changes_the_crdt_never_saw() {
    let mut a = Store::open_in_memory().unwrap();
    a.enable_sync().unwrap();
    let p = a.create_page(None, "Real").unwrap();
    let ops = a.sync_pending(100).unwrap();
    a.sync_ack(&ops.iter().map(|o| o.op_id.clone()).collect::<Vec<_>>())
        .unwrap();

    // Disable capture for one change by turning sync off and on again around a raw edit.
    a.disable_sync().unwrap();
    a.rename_page(p.id, "Changed while sync was off").unwrap();
    a.enable_sync().unwrap(); // bootstraps again from the relational state
    let mut b = synced();
    let mut net = Net::new();
    net.settle(&mut a, &mut b);
    assert_eq!(
        b.get_page(p.id).unwrap().unwrap().title,
        "Changed while sync was off"
    );
}

#[test]
fn operations_from_a_newer_app_are_held_not_dropped_and_applied_after_an_upgrade() {
    let dir = std::env::temp_dir().join(format!("notercrux-held-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("b.db");
    let _ = std::fs::remove_file(&path);

    let mut a = synced();
    let mut b = Store::open(&path).unwrap();
    b.enable_sync().unwrap();
    let p = a.create_page(None, "From the future").unwrap();
    a.save_document(p.id, &doc(vec![para("written by a newer app")]))
        .unwrap();
    let ops = a.sync_pending(100).unwrap();
    let remote: Vec<RemoteOp> = ops
        .iter()
        .enumerate()
        .map(|(i, o)| RemoteOp {
            seq: i as i64 + 1,
            op_id: o.op_id.clone(),
            doc_key: o.doc_key.clone(),
            format: SYNC_FORMAT + 1,
            update: o.update.clone(),
        })
        .collect();

    let report = b.sync_apply_remote(remote, 5).unwrap();
    assert_eq!(report.held, ops.len());
    assert_eq!(report.applied, 0);
    assert!(
        b.get_page(p.id).unwrap().is_none(),
        "an older app must not guess at newer data"
    );
    assert_eq!(
        b.sync_cursor().unwrap(),
        5,
        "the cursor moves on; the operations wait"
    );
    assert_eq!(b.sync_stats().unwrap().held_ops as usize, ops.len());
    assert_eq!(
        b.sync_apply_held().unwrap().applied,
        0,
        "still too new: nothing happens, nothing is lost"
    );
    assert_eq!(b.sync_stats().unwrap().held_ops as usize, ops.len());

    // The app is updated (simulated by lowering the stored format), and the held operations apply.
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("UPDATE sync_held SET format = ?1", [SYNC_FORMAT])
        .unwrap();
    let applied = b.sync_apply_held().unwrap();
    assert_eq!(applied.applied, ops.len());
    assert_eq!(texts(&b, p.id), ["written by a newer app"]);
    assert_eq!(b.sync_stats().unwrap().held_ops, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wiping_a_workspace_removes_pages_and_sync_state() {
    let mut a = synced();
    a.create_page(None, "x").unwrap();
    a.wipe_workspace().unwrap();
    assert!(a.list_pages().unwrap().is_empty());
    assert!(!a.sync_enabled());
    assert!(a.sync_pending(10).unwrap().is_empty());
}

#[test]
fn a_synced_workspace_still_passes_the_ordinary_store_rules() {
    // Sync must not change local behavior: cycles are still refused, records stay in their database.
    let mut a = synced();
    let p = a.create_page(None, "P").unwrap();
    let q = a.create_page(Some(p.id), "Q").unwrap();
    assert!(a.move_page(p.id, Some(q.id), 0).is_err());
    let (db, rec) = tasks_db(&mut a);
    assert!(a.move_page(rec.id, None, 0).is_err());
    assert!(a
        .set_property(rec.id, "points", &json!("not a number"))
        .is_err());
    let _ = db;
}

#[test]
fn an_editor_working_on_a_stale_copy_saves_without_clobbering_remote_edits() {
    let (mut a, mut b) = (synced(), synced());
    let mut net = Net::new();
    let page = a.create_page(None, "Shared").unwrap();
    a.save_document(page.id, &doc(vec![para("one"), para("two"), para("three")]))
        .unwrap();
    net.settle(&mut a, &mut b);
    assert_eq!(texts(&b, page.id), vec!["one", "two", "three"]);

    // A's editor loaded the page, then B edited and added blocks, and A kept typing.
    let base = a.load_document(page.id).unwrap();
    let mut b_doc = b.load_document(page.id).unwrap();
    b_doc.blocks[2].text = "three (from b)".into();
    b_doc.blocks.push(para("added on b"));
    b.save_document(page.id, &b_doc).unwrap();
    net.sync("b", &mut b);
    net.pull("a", &mut a);
    assert_eq!(
        texts(&a, page.id).len(),
        4,
        "the remote change reached A's database"
    );

    let mut local = base.clone();
    local.blocks[0].text = "one (typed on a)".into();
    let merged = a.save_document_rebased(page.id, &base, &local).unwrap();
    assert_eq!(merged.conflicts, 0);
    let expect = vec!["one (typed on a)", "two", "three (from b)", "added on b"];
    assert_eq!(
        merged
            .document
            .blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>(),
        expect
    );
    assert_eq!(texts(&a, page.id), expect);

    // The merged result is what B converges to as well.
    net.settle(&mut a, &mut b);
    assert_eq!(texts(&b, page.id), expect);
    same_state(&a, &b);
}
