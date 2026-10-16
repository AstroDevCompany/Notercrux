use super::*;
use notercrux_domain::{Block, BlockId, BlockKind, Document, Mark};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

fn blk(kind: BlockKind, text: &str) -> Block {
    Block::new(kind, text)
}

fn para(text: &str) -> Block {
    blk(BlockKind::Paragraph, text)
}

fn mark(from: u32, to: u32, kind: &str) -> Mark {
    Mark {
        from,
        to,
        kind: kind.into(),
        attrs: Map::new(),
    }
}

fn doc(blocks: Vec<Block>) -> Document {
    Document { version: 1, blocks }
}

/// A replica that starts from `from`'s state.
fn fork(from: &PageCrdt) -> PageCrdt {
    PageCrdt::from_snapshot(&from.snapshot().unwrap()).unwrap()
}

/// Sends everything `a` knows to `b` (and vice versa) as updates.
fn sync(a: &PageCrdt, b: &PageCrdt) {
    let to_b = a.updates_since(&b.version()).unwrap();
    let to_a = b.updates_since(&a.version()).unwrap();
    b.import(&to_b).unwrap();
    a.import(&to_a).unwrap();
}

fn texts(c: &PageCrdt) -> Vec<String> {
    fn walk(bs: &[Block], out: &mut Vec<String>) {
        for b in bs {
            out.push(b.text.clone());
            walk(&b.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&c.to_document().blocks, &mut out);
    out
}

fn find(d: &Document, text: &str) -> Block {
    fn walk(bs: &[Block], text: &str) -> Option<Block> {
        for b in bs {
            if b.text == text {
                return Some(b.clone());
            }
            if let Some(f) = walk(&b.children, text) {
                return Some(f);
            }
        }
        None
    }
    walk(&d.blocks, text).unwrap_or_else(|| panic!("no block with text {text:?}"))
}

// ---- round trips ---------------------------------------------------------------------------

#[test]
fn a_document_survives_a_round_trip_through_the_crdt() {
    let mut h = blk(BlockKind::Heading, "Title 😀");
    h.attrs.insert("level".into(), json!(2));
    let mut text = para("bold and link and plain");
    text.marks = vec![
        mark(0, 4, "bold"),
        Mark {
            from: 9,
            to: 13,
            kind: "link".into(),
            attrs: json!({"href": "https://x.y"}).as_object().unwrap().clone(),
        },
    ];
    let mut parent = blk(BlockKind::BulletedListItem, "parent");
    parent.children = vec![
        blk(BlockKind::Todo, "child"),
        blk(BlockKind::BulletedListItem, "child two"),
    ];
    let mut code = blk(BlockKind::Code, "fn main() {}\nlet x = 1;");
    code.attrs.insert("language".into(), json!("rust"));
    let d = doc(vec![h, text, parent, code, blk(BlockKind::Divider, "")]);

    let c = PageCrdt::new();
    assert!(c.apply_document(&d).unwrap());
    assert_eq!(c.to_document(), d);
    // A second apply of the same content changes nothing.
    let v = c.version();
    assert!(!c.apply_document(&d).unwrap());
    assert!(c.updates_since(&v).unwrap().len() < 16 || c.version() == v);

    // And it survives a snapshot.
    assert_eq!(
        PageCrdt::from_snapshot(&c.snapshot().unwrap())
            .unwrap()
            .to_document(),
        d
    );
}

#[test]
fn edits_are_small_operations_not_rewrites() {
    let c = PageCrdt::new();
    let blocks: Vec<Block> = (0..200)
        .map(|i| {
            para(&format!(
                "Block {i} with some text to make the document sizeable"
            ))
        })
        .collect();
    let mut d = doc(blocks);
    c.apply_document(&d).unwrap();
    let full = c.snapshot().unwrap().len();

    let v = c.version();
    d.blocks[120].text.insert_str(5, "XYZ"); // one small edit in the middle of the document
    c.apply_document(&d).unwrap();
    let delta = c.updates_since(&v).unwrap();
    assert_eq!(c.to_document(), d);
    assert!(
        delta.len() * 20 < full,
        "a 3-character edit produced {} bytes against a {} byte document",
        delta.len(),
        full
    );
}

#[test]
fn block_moves_keep_identity_and_become_moves() {
    let c = PageCrdt::new();
    let (a, b, cc) = (para("a"), para("b"), para("c"));
    let ids: Vec<BlockId> = vec![a.id, b.id, cc.id];
    c.apply_document(&doc(vec![a.clone(), b.clone(), cc.clone()]))
        .unwrap();
    // Move c to the front and nest a under b.
    let mut b2 = b.clone();
    b2.children = vec![a.clone()];
    c.apply_document(&doc(vec![cc.clone(), b2])).unwrap();
    let d = c.to_document();
    assert_eq!(
        d.blocks.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
        ["c", "b"]
    );
    assert_eq!(d.blocks[1].children[0].text, "a");
    assert_eq!(d.blocks[0].id, ids[2]);
    assert_eq!(
        d.blocks[1].children[0].id, ids[0],
        "the nested block is the same block, not a copy"
    );
}

#[test]
fn formatting_changes_are_applied_and_removed() {
    let c = PageCrdt::new();
    let mut p = para("hello world");
    p.marks = vec![mark(0, 5, "bold")];
    c.apply_document(&doc(vec![p.clone()])).unwrap();
    p.marks = vec![mark(0, 5, "bold"), mark(6, 11, "italic")];
    c.apply_document(&doc(vec![p.clone()])).unwrap();
    assert_eq!(c.to_document().blocks[0].marks, p.marks);
    p.marks = vec![mark(6, 11, "italic")];
    c.apply_document(&doc(vec![p.clone()])).unwrap();
    assert_eq!(c.to_document().blocks[0].marks, p.marks);
    // Text edits and formatting in one step.
    p.text = "hello brave world".into();
    p.marks = vec![mark(12, 17, "italic")];
    c.apply_document(&doc(vec![p.clone()])).unwrap();
    assert_eq!(c.to_document().blocks[0], p);
}

// ---- concurrency ---------------------------------------------------------------------------

#[test]
fn different_blocks_edited_on_two_replicas_both_survive() {
    let a = PageCrdt::new();
    a.apply_document(&doc(vec![para("one"), para("two"), para("three")]))
        .unwrap();
    let b = fork(&a);

    let mut da = a.to_document();
    da.blocks[0].text = "one (edited on A)".into();
    a.apply_document(&da).unwrap();
    let mut db = b.to_document();
    db.blocks[2].text = "three (edited on B)".into();
    db.blocks.push(para("new on B"));
    b.apply_document(&db).unwrap();

    sync(&a, &b);
    let expect = [
        "one (edited on A)",
        "two",
        "three (edited on B)",
        "new on B",
    ];
    assert_eq!(texts(&a), expect);
    assert_eq!(a.to_document(), b.to_document());
}

#[test]
fn simultaneous_edits_to_the_same_text_merge_without_loss() {
    let a = PageCrdt::new();
    a.apply_document(&doc(vec![para("hello world")])).unwrap();
    let b = fork(&a);
    let mut da = a.to_document();
    da.blocks[0].text = "Oh, hello world!".into();
    a.apply_document(&da).unwrap();
    let mut db = b.to_document();
    db.blocks[0].text = "hello, world (waves)".into();
    b.apply_document(&db).unwrap();
    sync(&a, &b);
    let merged = texts(&a)[0].clone();
    assert_eq!(merged, texts(&b)[0]);
    for frag in ["Oh, ", "!", ",", " (waves)", "hello", "world"] {
        assert!(merged.contains(frag), "lost {frag:?} in {merged:?}");
    }
}

#[test]
fn formatting_follows_text_when_another_replica_types_before_it() {
    let a = PageCrdt::new();
    let mut p = para("make this bold");
    p.marks = vec![mark(10, 14, "bold")];
    a.apply_document(&doc(vec![p])).unwrap();
    let b = fork(&a);
    let mut da = a.to_document();
    da.blocks[0].text = format!("PREFIX {}", da.blocks[0].text);
    for m in &mut da.blocks[0].marks {
        // The editor reports formatting at its new offsets after typing before it.
        m.from += 7;
        m.to += 7;
    }
    a.apply_document(&da).unwrap();
    // B, meanwhile, makes "this" italic. (Formatting exactly at the position another replica types at is
    // ambiguous by nature, so the test keeps them apart.)
    let mut db = b.to_document();
    db.blocks[0].marks.push(mark(5, 9, "italic"));
    b.apply_document(&db).unwrap();
    sync(&a, &b);
    let d = a.to_document();
    assert_eq!(d, b.to_document());
    assert_eq!(d.blocks[0].text, "PREFIX make this bold");
    let bold = d.blocks[0].marks.iter().find(|m| m.kind == "bold").unwrap();
    assert_eq!(
        &d.blocks[0]
            .text
            .chars()
            .skip(bold.from as usize)
            .take((bold.to - bold.from) as usize)
            .collect::<String>(),
        "bold"
    );
    let italic = d.blocks[0]
        .marks
        .iter()
        .find(|m| m.kind == "italic")
        .unwrap();
    assert_eq!(
        &d.blocks[0]
            .text
            .chars()
            .skip(italic.from as usize)
            .take((italic.to - italic.from) as usize)
            .collect::<String>(),
        "this"
    );
}

#[test]
fn moving_a_block_while_another_replica_edits_it() {
    let a = PageCrdt::new();
    let first = para("first");
    let parent = para("parent");
    a.apply_document(&doc(vec![first.clone(), parent.clone()]))
        .unwrap();
    let b = fork(&a);
    // A nests "first" under "parent"; B edits "first".
    let mut p2 = parent.clone();
    p2.children = vec![first.clone()];
    a.apply_document(&doc(vec![p2])).unwrap();
    let mut db = b.to_document();
    db.blocks[0].text = "first edited".into();
    b.apply_document(&db).unwrap();
    sync(&a, &b);
    let d = a.to_document();
    assert_eq!(d, b.to_document());
    assert_eq!(d.blocks.len(), 1);
    assert_eq!(d.blocks[0].children[0].text, "first edited");
}

#[test]
fn concurrent_moves_that_would_form_a_cycle_converge_without_losing_blocks() {
    let a = PageCrdt::new();
    let (x, y) = (para("x"), para("y"));
    a.apply_document(&doc(vec![x.clone(), y.clone()])).unwrap();
    let b = fork(&a);
    let mut y1 = y.clone();
    y1.children = vec![x.clone()];
    a.apply_document(&doc(vec![y1])).unwrap(); // x under y
    let mut x1 = x.clone();
    x1.children = vec![y.clone()];
    b.apply_document(&doc(vec![x1])).unwrap(); // y under x
    sync(&a, &b);
    assert_eq!(a.to_document(), b.to_document());
    let mut t = texts(&a);
    t.sort();
    assert_eq!(t, ["x", "y"]);
}

#[test]
fn delete_versus_edit_is_recovered_not_lost() {
    let a = PageCrdt::new();
    a.apply_document(&doc(vec![para("keep"), para("draft")]))
        .unwrap();
    let b = fork(&a);

    // A deletes "draft"; B edits it.
    let mut da = a.to_document();
    da.blocks.retain(|x| x.text != "draft");
    a.apply_document(&da).unwrap();
    let before_b = b.version();
    let mut db = b.to_document();
    db.blocks[1].text = "draft plus important edit".into();
    let mut dirty = HashSet::new();
    b.apply_document_tracked(&db, &mut dirty).unwrap();
    assert_eq!(dirty.len(), 1);
    let a_updates = a.updates_since(&before_b).unwrap();

    // B (who edited) merges A's delete: the edited block must come back.
    let outcome = merge_page_updates(&b, &dirty, &[a_updates]).unwrap();
    assert_eq!(outcome.recovered, 1);
    let recovered = find(&outcome.document, "draft plus important edit");
    assert_eq!(recovered.kind, BlockKind::Paragraph);
    let callout = outcome.document.blocks.last().unwrap();
    assert_eq!(callout.kind, BlockKind::Callout);
    assert_eq!(callout.attrs["icon"], json!(RECOVERED_ICON));

    // The recovery syncs back, so A sees the text too and everyone converges.
    sync(&a, &b);
    assert_eq!(a.to_document(), b.to_document());
    assert!(texts(&a).contains(&"draft plus important edit".to_string()));
    assert!(texts(&a).contains(&"keep".to_string()));
}

#[test]
fn deletes_without_a_concurrent_local_edit_are_plain_deletes() {
    let a = PageCrdt::new();
    a.apply_document(&doc(vec![para("keep"), para("remove me")]))
        .unwrap();
    let b = fork(&a);
    let mut da = a.to_document();
    da.blocks.retain(|x| x.text != "remove me");
    a.apply_document(&da).unwrap();
    let outcome = merge_page_updates(
        &b,
        &HashSet::new(),
        &[a.updates_since(&b.version()).unwrap()],
    )
    .unwrap();
    assert_eq!(outcome.recovered, 0);
    assert_eq!(texts(&b), ["keep"]);
}

#[test]
fn a_remote_edit_already_merged_does_not_count_as_a_local_edit() {
    // B receives A's edit of "shared", then A deletes it. B never touched it, so nothing is "recovered".
    let a = PageCrdt::new();
    a.apply_document(&doc(vec![para("keep"), para("shared")]))
        .unwrap();
    let b = fork(&a);
    let mut da = a.to_document();
    da.blocks[1].text = "shared, edited by A".into();
    a.apply_document(&da).unwrap();
    let outcome = merge_page_updates(
        &b,
        &HashSet::new(),
        &[a.updates_since(&b.version()).unwrap()],
    )
    .unwrap();
    assert_eq!(outcome.recovered, 0);
    let mut da = a.to_document();
    da.blocks.retain(|x| !x.text.starts_with("shared"));
    a.apply_document(&da).unwrap();
    let outcome = merge_page_updates(
        &b,
        &HashSet::new(),
        &[a.updates_since(&b.version()).unwrap()],
    )
    .unwrap();
    assert_eq!(outcome.recovered, 0);
    assert_eq!(texts(&b), ["keep"]);
}

#[test]
fn a_deleted_parent_with_an_edited_child_recovers_the_child() {
    let a = PageCrdt::new();
    let mut parent = blk(BlockKind::BulletedListItem, "parent");
    parent.children = vec![blk(BlockKind::BulletedListItem, "child")];
    a.apply_document(&doc(vec![parent, para("tail")])).unwrap();
    let b = fork(&a);
    let mut da = a.to_document();
    da.blocks.remove(0); // delete the parent and with it the child
    a.apply_document(&da).unwrap();
    let mut db = b.to_document();
    db.blocks[0].children[0].text = "child, edited".into();
    let mut dirty = HashSet::new();
    b.apply_document_tracked(&db, &mut dirty).unwrap();
    let outcome =
        merge_page_updates(&b, &dirty, &[a.updates_since(&b.version()).unwrap()]).unwrap();
    assert_eq!(outcome.recovered, 1);
    assert_eq!(
        find(&outcome.document, "child, edited").kind,
        BlockKind::BulletedListItem
    );
}

#[test]
fn touched_blocks_are_reported_for_text_formatting_kind_attrs_and_creation_only() {
    let c = PageCrdt::new();
    let (a, b, d) = (para("a"), para("b"), para("d"));
    let mut t = HashSet::new();
    c.apply_document_tracked(&doc(vec![a.clone(), b.clone(), d.clone()]), &mut t)
        .unwrap();
    assert_eq!(t.len(), 3, "created blocks count as touched");

    let mut t = HashSet::new();
    let mut next = c.to_document();
    next.blocks.swap(0, 2); // a pure reorder touches no content
    c.apply_document_tracked(&next, &mut t).unwrap();
    assert!(t.is_empty(), "moving a block is not a content edit: {t:?}");

    let mut next = c.to_document();
    next.blocks[1].text = "b!".into();
    next.blocks[2].kind = BlockKind::Quote;
    let mut t = HashSet::new();
    c.apply_document_tracked(&next, &mut t).unwrap();
    assert_eq!(t.len(), 2);
}

// ---- delivery ------------------------------------------------------------------------------

#[test]
fn importing_is_idempotent_and_tolerates_out_of_order_delivery() {
    let a = PageCrdt::new();
    let v0 = a.version();
    a.apply_document(&doc(vec![para("one")])).unwrap();
    let u1 = a.updates_since(&v0).unwrap();
    let v1 = a.version();
    let mut d = a.to_document();
    d.blocks[0].text = "one two".into();
    a.apply_document(&d).unwrap();
    let u2 = a.updates_since(&v1).unwrap();

    let b = PageCrdt::new();
    b.import(&u2).unwrap(); // arrives first: depends on u1, so it waits
    b.import(&u1).unwrap();
    b.import(&u1).unwrap(); // duplicates are harmless
    b.import(&u2).unwrap();
    assert_eq!(b.to_document(), a.to_document());
}

#[test]
fn independently_created_blocks_with_the_same_id_stay_unique_and_deterministic() {
    let shared = para("same id on both");
    let a = PageCrdt::new();
    let b = PageCrdt::new();
    a.apply_document(&doc(vec![shared.clone()])).unwrap();
    b.apply_document(&doc(vec![shared.clone()])).unwrap();
    sync(&a, &b);
    let (da, db) = (a.to_document(), b.to_document());
    assert_eq!(da, db);
    assert_eq!(da.blocks.len(), 2, "no block is lost");
    assert_ne!(da.blocks[0].id, da.blocks[1].id, "ids stay unique");
    // Re-applying the same document must not create more duplicates.
    assert!(!a.apply_document(&da).unwrap());
}

#[test]
fn garbage_updates_are_rejected_not_applied() {
    let c = PageCrdt::new();
    c.apply_document(&doc(vec![para("x")])).unwrap();
    assert!(c.import(&[1, 2, 3, 4, 5]).is_err());
    assert_eq!(texts(&c), ["x"]);
}

// ---- randomized convergence ----------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn flat_paths(blocks: &[Block], prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (i, b) in blocks.iter().enumerate() {
        prefix.push(i);
        out.push(prefix.clone());
        flat_paths(&b.children, prefix, out);
        prefix.pop();
    }
}

fn at<'a>(blocks: &'a mut [Block], path: &[usize]) -> &'a mut Block {
    let mut cur = &mut blocks[path[0]];
    for &i in &path[1..] {
        cur = &mut cur.children[i];
    }
    cur
}

fn remove_at(blocks: &mut Vec<Block>, path: &[usize]) -> Block {
    if path.len() == 1 {
        return blocks.remove(path[0]);
    }
    let parent = at(blocks, &path[..path.len() - 1]);
    parent.children.remove(path[path.len() - 1])
}

/// One random user action on a document.
fn mutate(d: &mut Document, rng: &mut Rng, counter: &mut u32) {
    let mut paths = Vec::new();
    flat_paths(&d.blocks, &mut Vec::new(), &mut paths);
    *counter += 1;
    match rng.below(7) {
        0 | 1 if !paths.is_empty() => {
            let p = &paths[rng.below(paths.len())];
            let b = at(&mut d.blocks, p);
            let chars: Vec<char> = b.text.chars().collect();
            let pos = rng.below(chars.len() + 1);
            let mut s: String = chars[..pos].iter().collect();
            s.push_str(&format!("<{counter}>"));
            s.extend(&chars[pos..]);
            b.text = s;
            b.marks
                .retain(|m| (m.to as usize) <= b.text.chars().count());
        }
        2 if !paths.is_empty() => {
            let p = &paths[rng.below(paths.len())];
            let b = at(&mut d.blocks, p);
            let n = b.text.chars().count();
            if n > 2 {
                let from = rng.below(n - 1);
                let to = from + 1 + rng.below((n - from).min(3));
                let kept: String = b
                    .text
                    .chars()
                    .enumerate()
                    .filter(|(i, _)| *i < from || *i >= to)
                    .map(|(_, c)| c)
                    .collect();
                b.text = kept;
                b.marks.clear();
            }
        }
        3 => {
            let nb = para(&format!("new block {counter}"));
            if !paths.is_empty() && rng.below(3) == 0 {
                let p = &paths[rng.below(paths.len())];
                at(&mut d.blocks, p).children.push(nb);
            } else {
                let i = rng.below(d.blocks.len() + 1);
                d.blocks.insert(i, nb);
            }
        }
        4 if paths.len() > 1 => {
            let p = paths[rng.below(paths.len())].clone();
            remove_at(&mut d.blocks, &p);
        }
        5 if paths.len() > 1 => {
            // Move a block to the top level (never into its own subtree).
            let p = paths[rng.below(paths.len())].clone();
            let b = remove_at(&mut d.blocks, &p);
            let i = rng.below(d.blocks.len() + 1);
            d.blocks.insert(i, b);
        }
        _ if !paths.is_empty() => {
            let p = &paths[rng.below(paths.len())];
            let b = at(&mut d.blocks, p);
            let n = b.text.chars().count();
            if n > 1 {
                b.marks.clear();
                let from = rng.below(n - 1);
                b.marks.push(mark(
                    from as u32,
                    (from + 1 + rng.below(n - from - 1 + 1).min(n - from - 1)) as u32,
                    "bold",
                ));
            }
        }
        _ => d.blocks.push(para(&format!("fallback {counter}"))),
    }
}

fn unique_ids(d: &Document) -> bool {
    fn walk(bs: &[Block], seen: &mut std::collections::HashSet<BlockId>) -> bool {
        bs.iter()
            .all(|b| seen.insert(b.id) && walk(&b.children, seen))
    }
    walk(&d.blocks, &mut std::collections::HashSet::new())
}

#[test]
fn random_concurrent_editing_always_converges() {
    for seed in 1..=12u64 {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
        let a = PageCrdt::new();
        a.apply_document(&doc(vec![
            para("seed one"),
            para("seed two"),
            para("seed three"),
        ]))
        .unwrap();
        let reps = [fork(&a), fork(&a), fork(&a)];
        let mut counter = 0;
        for step in 0..120 {
            let r = &reps[rng.below(3)];
            let mut d = r.to_document();
            mutate(&mut d, &mut rng, &mut counter);
            r.apply_document(&d).unwrap();
            assert_eq!(
                r.to_document(),
                d,
                "seed {seed} step {step}: a replica must reflect its own edit exactly"
            );
            // Occasionally two replicas exchange updates (in either direction, sometimes one-way).
            if rng.below(4) == 0 {
                let (i, j) = (rng.below(3), rng.below(3));
                if i != j {
                    let u = reps[i].updates_since(&reps[j].version()).unwrap();
                    reps[j].import(&u).unwrap();
                }
            }
        }
        // Final full exchange.
        for _ in 0..2 {
            for i in 0..3 {
                for j in 0..3 {
                    if i != j {
                        let u = reps[i].updates_since(&reps[j].version()).unwrap();
                        reps[j].import(&u).unwrap();
                    }
                }
            }
        }
        let d0 = reps[0].to_document();
        assert_eq!(d0, reps[1].to_document(), "seed {seed}: replicas diverged");
        assert_eq!(d0, reps[2].to_document(), "seed {seed}: replicas diverged");
        assert!(unique_ids(&d0), "seed {seed}: duplicate block ids");
        // A fresh replica built from a snapshot reads the same thing.
        assert_eq!(
            PageCrdt::from_snapshot(&reps[0].snapshot().unwrap())
                .unwrap()
                .to_document(),
            d0
        );
    }
}

#[test]
fn updates_can_be_replayed_in_any_order_to_a_fresh_replica() {
    let a = PageCrdt::new();
    let mut log: Vec<Vec<u8>> = Vec::new();
    let mut rng = Rng(42);
    let mut counter = 0;
    for _ in 0..40 {
        let v = a.version();
        let mut d = a.to_document();
        mutate(&mut d, &mut rng, &mut counter);
        a.apply_document(&d).unwrap();
        let u = a.updates_since(&v).unwrap();
        if !u.is_empty() {
            log.push(u);
        }
    }
    let mut shuffled = log.clone();
    for i in (1..shuffled.len()).rev() {
        shuffled.swap(i, rng.below(i + 1));
    }
    let fresh = PageCrdt::new();
    for u in &shuffled {
        fresh.import(u).unwrap();
    }
    assert_eq!(fresh.to_document(), a.to_document());
    let _: Value = json!(null);
}

#[test]
fn an_edit_that_reaches_a_device_which_already_deleted_the_block_is_recovered() {
    // The editor's edit was already confirmed (not dirty), so only the deleter can recover it.
    let a = PageCrdt::new();
    a.apply_document(&doc(vec![para("keep"), para("draft")]))
        .unwrap();
    let b = fork(&a);
    let mut da = a.to_document();
    da.blocks.retain(|x| x.text != "draft");
    a.apply_document(&da).unwrap(); // A deletes first
    let v = b.version();
    let mut db = b.to_document();
    db.blocks[1].text = "draft, extended by B".into();
    b.apply_document(&db).unwrap();

    let outcome = merge_page_updates(
        &a,
        &HashSet::new(),
        &[b.updates_since(&a.version()).unwrap()],
    )
    .unwrap();
    assert_eq!(
        outcome.recovered, 1,
        "the deleter notices a concurrent edit to its deleted block"
    );
    assert!(texts(&a).contains(&"draft, extended by B".to_string()));
    // B then receives A's delete plus the recovery: nothing is recovered twice.
    let back = merge_page_updates(&b, &HashSet::new(), &[a.updates_since(&v).unwrap()]).unwrap();
    assert_eq!(back.recovered, 0);
    assert_eq!(a.to_document(), b.to_document());
    assert_eq!(
        texts(&b)
            .iter()
            .filter(|t| *t == "draft, extended by B")
            .count(),
        1
    );
}

#[test]
fn edits_made_before_the_delete_was_seen_do_not_cause_recovery_on_the_deleter() {
    // A edits then deletes. B only reads. Nothing was concurrent, so B recovers nothing.
    let a = PageCrdt::new();
    a.apply_document(&doc(vec![para("x"), para("y")])).unwrap();
    let b = fork(&a);
    let mut d = a.to_document();
    d.blocks[1].text = "y edited".into();
    a.apply_document(&d).unwrap();
    d.blocks.retain(|x| x.text != "y edited");
    a.apply_document(&d).unwrap();
    let outcome = merge_page_updates(
        &b,
        &HashSet::new(),
        &[a.updates_since(&b.version()).unwrap()],
    )
    .unwrap();
    assert_eq!(outcome.recovered, 0);
    assert_eq!(texts(&b), ["x"]);
}
