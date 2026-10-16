//! Scale checks. The default test is a smaller correctness run for CI; the `#[ignore]`d benchmark
//! prints timings for a large workspace:
//!
//! ```text
//! cargo test -p notercrux-storage --release --test large_workspace -- --ignored --nocapture
//! ```

use notercrux_domain::{Block, BlockKind, Document, PageId};
use notercrux_storage::Store;
use std::time::Instant;

fn build_tree(store: &mut Store, fanout: usize, depth: usize) -> Vec<PageId> {
    fn grow(
        store: &mut Store,
        parent: Option<PageId>,
        fanout: usize,
        depth: usize,
        n: &mut usize,
        out: &mut Vec<PageId>,
    ) {
        for _ in 0..fanout {
            *n += 1;
            let p = store
                .create_page(parent, &format!("Page {n} about topic {}", *n % 97))
                .unwrap();
            out.push(p.id);
            if depth > 1 {
                grow(store, Some(p.id), fanout, depth - 1, n, out);
            }
        }
    }
    let mut out = Vec::new();
    let mut n = 0;
    grow(store, None, fanout, depth, &mut n, &mut out);
    out
}

fn long_doc(blocks: usize) -> Document {
    Document {
        version: 1,
        blocks: (0..blocks)
            .map(|i| {
                let kind = match i % 5 {
                    0 => BlockKind::Heading,
                    3 => BlockKind::BulletedListItem,
                    _ => BlockKind::Paragraph,
                };
                let mut b = Block::new(
                    kind,
                    format!(
                        "Block {i}: the quick brown fox jumps over the lazy dog, again and again."
                    ),
                );
                if kind == BlockKind::Heading {
                    b.attrs.insert("level".into(), 2.into());
                }
                b
            })
            .collect(),
    }
}

#[test]
fn medium_workspace_stays_correct() {
    let mut s = Store::open_in_memory().unwrap();
    let ids = build_tree(&mut s, 6, 4); // 6 + 36 + 216 + 1296 = 1554 pages
    assert_eq!(ids.len(), 1554);
    assert_eq!(s.list_pages().unwrap().len(), 1554);

    let doc = long_doc(2_000);
    s.save_document(ids[10], &doc).unwrap();
    assert_eq!(s.load_document(ids[10]).unwrap(), doc);
    assert_eq!(s.search("lazy", 5).unwrap()[0].id, ids[10]);

    // Reorder and nest within the big tree; positions stay dense and unique per parent.
    s.move_page(ids[1000], Some(ids[0]), 0).unwrap();
    let pages = s.list_pages().unwrap();
    let kids: Vec<_> = pages
        .iter()
        .filter(|p| p.parent_id == Some(ids[0]))
        .collect();
    assert_eq!(kids[0].id, ids[1000]);
    let mut positions: Vec<_> = kids.iter().map(|p| p.position).collect();
    positions.sort_unstable();
    assert_eq!(positions, (0..kids.len() as u32).collect::<Vec<_>>());
}

#[test]
#[ignore = "benchmark: run explicitly with --release --nocapture"]
fn benchmark_large_workspace() {
    let mut s = Store::open_in_memory().unwrap();

    let t = Instant::now();
    let ids = build_tree(&mut s, 10, 4); // 10 + 100 + 1000 + 10000 = 11,110 pages
    println!("create {} pages:          {:?}", ids.len(), t.elapsed());

    let t = Instant::now();
    let pages = s.list_pages().unwrap();
    println!("list_pages ({}):       {:?}", pages.len(), t.elapsed());

    let t = Instant::now();
    for (i, id) in ids.iter().take(500).enumerate() {
        s.save_document(*id, &long_doc(40 + i % 20)).unwrap();
    }
    println!("save 500 docs (~50 blocks): {:?}", t.elapsed());

    let big = long_doc(20_000);
    let t = Instant::now();
    s.save_document(ids[0], &big).unwrap();
    println!("save 20,000-block doc:    {:?}", t.elapsed());
    let t = Instant::now();
    let loaded = s.load_document(ids[0]).unwrap();
    println!(
        "load 20,000-block doc:    {:?} ({} blocks)",
        t.elapsed(),
        loaded.block_count()
    );

    let t = Instant::now();
    let hits = s.search_with_snippets("topic 42", 20).unwrap();
    println!("search (title, {} hits):  {:?}", hits.len(), t.elapsed());
    let t = Instant::now();
    let plain = s.search("lazy", 20).unwrap();
    println!(
        "search (body, no snippet, {} hits): {:?}",
        plain.len(),
        t.elapsed()
    );
    let t = Instant::now();
    let hits = s.search_with_snippets("lazy", 20).unwrap();
    println!("search (body, {} hits):   {:?}", hits.len(), t.elapsed());

    let t = Instant::now();
    s.move_page(ids[5000], Some(ids[1]), 0).unwrap();
    println!("move page in big tree:    {:?}", t.elapsed());

    let t = Instant::now();
    s.rebuild_search_index().unwrap();
    println!("rebuild search index:     {:?}", t.elapsed());
}

#[test]
#[ignore = "benchmark: run explicitly with --release --nocapture"]
fn benchmark_sync_capture_cost() {
    use notercrux_domain::{Block, BlockKind, Document};
    let mut s = Store::open_in_memory().unwrap();
    s.enable_sync().unwrap();

    let t = Instant::now();
    let ids = build_tree(&mut s, 7, 4); // 7 + 49 + 343 + 2401 = 2,800 pages
    println!(
        "create {} pages with sync on:      {:?} ({:?}/page)",
        ids.len(),
        t.elapsed(),
        t.elapsed() / ids.len() as u32
    );

    let t = Instant::now();
    s.rename_page(ids[100], "renamed").unwrap();
    println!("rename a page in a 2,800-page tree: {:?}", t.elapsed());
    let t = Instant::now();
    s.move_page(ids[1500], Some(ids[3]), 0).unwrap();
    println!("move a page in a 2,800-page tree:   {:?}", t.elapsed());

    // Autosave of a long document: the edit that happens every few hundred milliseconds of typing.
    for blocks in [100usize, 1_000, 5_000] {
        let mut doc = Document {
            version: 1,
            blocks: (0..blocks)
                .map(|i| {
                    Block::new(
                        BlockKind::Paragraph,
                        format!("Block {i}: the quick brown fox jumps over the lazy dog."),
                    )
                })
                .collect(),
        };
        s.save_document(ids[0], &doc).unwrap(); // first save creates the content
        let t = Instant::now();
        let n = 20;
        for k in 0..n {
            doc.blocks[blocks / 2].text.push_str(&format!(" edit{k}"));
            s.save_document(ids[0], &doc).unwrap();
        }
        println!(
            "autosave of a {blocks}-block page: {:?} per save",
            t.elapsed() / n
        );
    }
    let stats = s.sync_stats().unwrap();
    println!("pending ops: {}", stats.pending_ops);
}
