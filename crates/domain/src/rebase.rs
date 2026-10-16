//! Three-way merge of a page the editor changed while other devices changed it too.
//!
//! The editor holds a copy of the page (`base`) that it changed into `local`; meanwhile the stored
//! page moved on to `current` (remote operations were applied). [`rebase`] replays the editor's
//! changes onto `current` instead of overwriting it, so remote edits are never clobbered by a stale
//! editor and local typing is never lost to a refresh.
//!
//! Granularity: whole blocks, plus a text-level merge inside a plain-text block when the two edits do
//! not overlap. When both sides changed the same part of a block, the local version stays and the
//! other version is kept in a callout, so nothing is silently dropped.

use crate::{Block, BlockId, BlockKind, Document};
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// Icon of the callout holding the other device's version of a block both sides edited.
pub const CONFLICT_ICON: &str = "🔀";

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rebased {
    pub document: Document,
    /// Blocks edited on both sides in the same place (the other version went to a callout).
    pub conflicts: usize,
}

#[derive(Default)]
struct Tree {
    kids: HashMap<Option<BlockId>, Vec<BlockId>>,
    content: HashMap<BlockId, Block>,
    parent: HashMap<BlockId, Option<BlockId>>,
}

impl Tree {
    fn from(doc: &Document) -> Self {
        let mut t = Tree::default();
        t.add_level(None, &doc.blocks);
        t
    }

    fn add_level(&mut self, parent: Option<BlockId>, blocks: &[Block]) {
        for b in blocks {
            if self.content.contains_key(&b.id) {
                continue; // duplicate ids: the first one wins
            }
            let mut own = b.clone();
            own.children = Vec::new();
            self.content.insert(b.id, own);
            self.parent.insert(b.id, parent);
            self.kids.entry(parent).or_default().push(b.id);
            self.add_level(Some(b.id), &b.children);
        }
    }

    fn contains(&self, id: BlockId) -> bool {
        self.content.contains_key(&id)
    }

    fn siblings(&self, parent: Option<BlockId>) -> &[BlockId] {
        self.kids.get(&parent).map_or(&[], |v| v.as_slice())
    }

    fn prev_sibling(&self, id: BlockId) -> Option<BlockId> {
        let p = *self.parent.get(&id)?;
        let sibs = self.siblings(p);
        let i = sibs.iter().position(|s| *s == id)?;
        i.checked_sub(1).map(|j| sibs[j])
    }

    fn is_inside(&self, id: BlockId, ancestor: BlockId) -> bool {
        let mut cur = self.parent.get(&id).copied().flatten();
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent.get(&c).copied().flatten();
        }
        false
    }

    /// Detaches `id` from its parent, keeping its content (children stay attached by id).
    fn detach(&mut self, id: BlockId) {
        if let Some(p) = self.parent.get(&id).copied() {
            if let Some(v) = self.kids.get_mut(&p) {
                v.retain(|x| *x != id);
            }
        }
    }

    /// Inserts `id` under `parent`, right after the closest of `wanted_before` (nearest first) that is
    /// present there, else first.
    fn attach(&mut self, id: BlockId, parent: Option<BlockId>, wanted_before: &[BlockId]) {
        let parent = parent.filter(|p| self.contains(*p) && *p != id && !self.is_inside(*p, id));
        let list = self.kids.entry(parent).or_default();
        let at = wanted_before
            .iter()
            .find_map(|b| list.iter().position(|x| x == b))
            .map_or(0, |i| i + 1);
        list.insert(at.min(list.len()), id);
        self.parent.insert(id, parent);
    }

    fn remove(&mut self, id: BlockId) {
        let p = self.parent.get(&id).copied().flatten();
        let pos = self
            .siblings(self.parent.get(&id).copied().flatten())
            .iter()
            .position(|x| *x == id);
        let children: Vec<BlockId> = self.kids.remove(&Some(id)).unwrap_or_default();
        self.detach(id);
        // Children that survive take the removed block's place.
        let list = self.kids.entry(p).or_default();
        let at = pos.unwrap_or(list.len()).min(list.len());
        for (i, c) in children.iter().enumerate() {
            list.insert((at + i).min(list.len()), *c);
        }
        for c in children {
            self.parent.insert(c, p);
        }
        self.content.remove(&id);
        self.parent.remove(&id);
    }

    fn to_blocks(&self, parent: Option<BlockId>) -> Vec<Block> {
        self.siblings(parent)
            .iter()
            .filter_map(|id| {
                let mut b = self.content.get(id)?.clone();
                b.children = self.to_blocks(Some(*id));
                Some(b)
            })
            .collect()
    }
}

fn same_content(a: &Block, b: &Block) -> bool {
    a.kind == b.kind && a.attrs == b.attrs && a.text == b.text && a.marks == b.marks
}

/// Replays `base -> local` onto `current`.
pub fn rebase(base: &Document, local: &Document, current: &Document) -> Rebased {
    let (tb, tl) = (Tree::from(base), Tree::from(local));
    let mut out = Tree::from(current);
    let mut conflicts: Vec<Block> = Vec::new();

    // Deleted here: remove unless someone else changed it meanwhile (their edit wins over our delete).
    let removed: Vec<BlockId> = base_order(base)
        .into_iter()
        .filter(|id| !tl.contains(*id))
        .collect();
    for id in removed {
        if let (Some(cur), Some(old)) = (out.content.get(&id), tb.content.get(&id)) {
            if same_content(cur, old) {
                out.remove(id);
            }
        }
    }

    // Everything else, in local document order so earlier siblings are placed first.
    for id in base_order(local) {
        let Some(mine) = tl.content.get(&id) else {
            continue;
        };
        let wanted_before = local_predecessors(&tl, id);
        let parent = tl.parent.get(&id).copied().flatten();
        match tb.content.get(&id) {
            None => {
                // Added here.
                if !out.contains(id) {
                    out.content.insert(id, mine.clone());
                    out.attach(id, parent, &wanted_before);
                }
            }
            Some(old) => {
                let content_changed = !same_content(mine, old);
                let moved = tb.parent.get(&id) != tl.parent.get(&id)
                    || tb.prev_sibling(id) != tl.prev_sibling(id);
                if !out.contains(id) {
                    // Deleted elsewhere but edited or kept here: our version comes back.
                    if content_changed || moved {
                        out.content.insert(id, mine.clone());
                        out.attach(id, parent, &wanted_before);
                    }
                    continue;
                }
                if content_changed {
                    let cur = out.content.get(&id).expect("present").clone();
                    let merged = if same_content(&cur, old) || same_content(&cur, mine) {
                        mine.clone()
                    } else if let Some(m) = merge_text(old, mine, &cur) {
                        m
                    } else {
                        conflicts.push(cur.clone());
                        mine.clone()
                    };
                    out.content.insert(id, merged);
                }
                if moved && !out.is_inside_or_self(parent, id) {
                    // Only move when our own position actually differs from the base's. A remote move of
                    // a block we did not touch stays.
                    out.detach(id);
                    out.attach(id, parent, &wanted_before);
                }
            }
        }
    }

    let mut document = Document {
        version: current.version.max(local.version),
        blocks: out.to_blocks(None),
    };
    let n = conflicts.len();
    if n > 0 {
        let mut callout = Block::new(
            BlockKind::Callout,
            "Edits from another device: this block was changed on both devices at once. Your version is kept above.",
        );
        callout.attrs.insert("icon".into(), json!(CONFLICT_ICON));
        callout.attrs.insert("tone".into(), json!("yellow"));
        callout.children = conflicts
            .into_iter()
            .map(|mut b| {
                b.id = BlockId::new();
                b.children = Vec::new();
                b
            })
            .collect();
        document.blocks.push(callout);
    }
    Rebased {
        document,
        conflicts: n,
    }
}

impl Tree {
    fn is_inside_or_self(&self, parent: Option<BlockId>, id: BlockId) -> bool {
        parent.is_some_and(|p| p == id || self.is_inside(p, id))
    }
}

/// Block ids in document order (depth first).
fn base_order(doc: &Document) -> Vec<BlockId> {
    fn walk(blocks: &[Block], out: &mut Vec<BlockId>, seen: &mut HashSet<BlockId>) {
        for b in blocks {
            if seen.insert(b.id) {
                out.push(b.id);
                walk(&b.children, out, seen);
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out, &mut HashSet::new());
    out
}

/// The siblings before `id` in the local tree, nearest first.
fn local_predecessors(t: &Tree, id: BlockId) -> Vec<BlockId> {
    let p = t.parent.get(&id).copied().flatten();
    let sibs = t.siblings(p);
    let i = sibs.iter().position(|s| *s == id).unwrap_or(0);
    sibs[..i].iter().rev().copied().collect()
}

/// Merges two edits of a plain-text block when they touch different stretches of it.
fn merge_text(base: &Block, local: &Block, remote: &Block) -> Option<Block> {
    if base.kind != local.kind
        || base.kind != remote.kind
        || base.attrs != local.attrs
        || base.attrs != remote.attrs
        || !base.marks.is_empty()
        || !local.marks.is_empty()
        || !remote.marks.is_empty()
    {
        return None;
    }
    let (b, l, r): (Vec<char>, Vec<char>, Vec<char>) = (
        base.text.chars().collect(),
        local.text.chars().collect(),
        remote.text.chars().collect(),
    );
    let (lp, ls) = affix(&b, &l);
    let (rp, rs) = affix(&b, &r);
    // Each side replaced a range of the base: local [lp, l_end), remote [rp, r_end).
    let (l_end, r_end) = (b.len() - ls, b.len() - rs);
    let l_ins: String = l[lp..l.len() - ls].iter().collect();
    let r_ins: String = r[rp..r.len() - rs].iter().collect();
    let both_inserts_at_one_point = lp == l_end && rp == r_end && lp == rp;
    let slice = |from: usize, to: usize| -> String { b[from..to].iter().collect() };
    let text = if !both_inserts_at_one_point && l_end <= rp {
        format!(
            "{}{}{}{}{}",
            slice(0, lp),
            l_ins,
            slice(l_end, rp),
            r_ins,
            slice(r_end, b.len())
        )
    } else if !both_inserts_at_one_point && r_end <= lp {
        format!(
            "{}{}{}{}{}",
            slice(0, rp),
            r_ins,
            slice(r_end, lp),
            l_ins,
            slice(l_end, b.len())
        )
    } else {
        return None;
    };
    Some(Block {
        text,
        ..remote.clone()
    })
}

/// Lengths of the common prefix and suffix of `a` and `b` (not overlapping).
fn affix(a: &[char], b: &[char]) -> (usize, usize) {
    let max = a.len().min(b.len());
    let mut p = 0;
    while p < max && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0;
    while s < max - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }
    (p, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Block {
        Block::new(BlockKind::Paragraph, text)
    }

    fn doc(blocks: Vec<Block>) -> Document {
        Document { version: 1, blocks }
    }

    fn texts(d: &Document) -> Vec<String> {
        fn walk(b: &[Block], out: &mut Vec<String>) {
            for x in b {
                out.push(x.text.clone());
                walk(&x.children, out);
            }
        }
        let mut v = Vec::new();
        walk(&d.blocks, &mut v);
        v
    }

    fn edited(d: &Document, i: usize, text: &str) -> Document {
        let mut d = d.clone();
        d.blocks[i].text = text.into();
        d
    }

    #[test]
    fn with_no_remote_change_the_result_is_the_local_document() {
        let base = doc(vec![p("a"), p("b")]);
        let local = edited(&base, 0, "a!");
        let r = rebase(&base, &local, &base);
        assert_eq!(r.document, local);
        assert_eq!(r.conflicts, 0);
        // Rebasing onto a document that already has the change is a no-op.
        assert_eq!(rebase(&base, &local, &local).document, local);
    }

    #[test]
    fn edits_to_different_blocks_and_remote_additions_all_survive() {
        let base = doc(vec![p("one"), p("two"), p("three")]);
        let local = edited(&base, 0, "one (local)");
        let mut current = edited(&base, 2, "three (remote)");
        current.blocks.push(p("remote tail"));
        let r = rebase(&base, &local, &current);
        assert_eq!(
            texts(&r.document),
            vec!["one (local)", "two", "three (remote)", "remote tail"]
        );
        assert_eq!(r.conflicts, 0);
    }

    #[test]
    fn non_overlapping_edits_inside_one_paragraph_are_merged() {
        let base = doc(vec![p("the quick brown fox")]);
        let local = edited(&base, 0, "the very quick brown fox");
        let current = edited(&base, 0, "the quick brown fox jumps");
        let r = rebase(&base, &local, &current);
        assert_eq!(texts(&r.document), vec!["the very quick brown fox jumps"]);
        assert_eq!(r.conflicts, 0);
        // And the other way around.
        let r = rebase(&base, &current, &local);
        assert_eq!(texts(&r.document), vec!["the very quick brown fox jumps"]);
    }

    #[test]
    fn overlapping_edits_keep_ours_and_preserve_theirs_in_a_callout() {
        let base = doc(vec![p("the quick brown fox")]);
        let local = edited(&base, 0, "the slow brown fox");
        let current = edited(&base, 0, "the fast brown fox");
        let r = rebase(&base, &local, &current);
        assert_eq!(r.conflicts, 1);
        assert_eq!(r.document.blocks[0].text, "the slow brown fox");
        let callout = r.document.blocks.last().unwrap();
        assert_eq!(callout.kind, BlockKind::Callout);
        assert_eq!(callout.children[0].text, "the fast brown fox");
    }

    #[test]
    fn an_edit_here_beats_a_delete_there_and_an_edit_there_beats_a_delete_here() {
        let base = doc(vec![p("keep"), p("x"), p("y")]);
        // We edit x; they deleted x.
        let local = edited(&base, 1, "x edited");
        let mut current = base.clone();
        current.blocks.remove(1);
        let r = rebase(&base, &local, &current);
        assert_eq!(texts(&r.document), vec!["keep", "x edited", "y"]);
        // We delete y; they edited y.
        let mut local = base.clone();
        local.blocks.remove(2);
        let current = edited(&base, 2, "y edited remotely");
        let r = rebase(&base, &local, &current);
        assert_eq!(texts(&r.document), vec!["keep", "x", "y edited remotely"]);
        // We delete y; nobody touched it.
        let r = rebase(&base, &local, &base);
        assert_eq!(texts(&r.document), vec!["keep", "x"]);
    }

    #[test]
    fn new_blocks_land_next_to_their_neighbours_even_if_a_neighbour_was_deleted() {
        let base = doc(vec![p("a"), p("b"), p("c")]);
        let mut local = base.clone();
        local.blocks.insert(2, p("new after b"));
        let mut current = base.clone();
        current.blocks.remove(1); // b deleted remotely
        let r = rebase(&base, &local, &current);
        assert_eq!(texts(&r.document), vec!["a", "new after b", "c"]);
        // Remote insertions elsewhere do not move ours.
        let mut current = base.clone();
        current.blocks.insert(0, p("remote first"));
        let r = rebase(&base, &local, &current);
        assert_eq!(
            texts(&r.document),
            vec!["remote first", "a", "b", "new after b", "c"]
        );
    }

    #[test]
    fn moves_apply_and_nesting_is_preserved() {
        let mut parent = p("parent");
        parent.children.push(p("child"));
        let base = doc(vec![parent.clone(), p("other")]);
        // We move "other" under "parent"; they edit "child".
        let mut local = base.clone();
        let other = local.blocks.remove(1);
        local.blocks[0].children.push(other);
        let mut current = base.clone();
        current.blocks[0].children[0].text = "child edited".into();
        let r = rebase(&base, &local, &current);
        assert_eq!(texts(&r.document), vec!["parent", "child edited", "other"]);
        assert_eq!(r.document.blocks.len(), 1);
        assert_eq!(r.document.blocks[0].children.len(), 2);
    }

    #[test]
    fn moving_a_block_into_its_own_subtree_is_refused_not_a_cycle() {
        let mut a = p("a");
        a.children.push(p("b"));
        let base = doc(vec![a]);
        let mut local = base.clone();
        // Local keeps a above b. Remote (current) moved b to the top level.
        let mut current = base.clone();
        let b = current.blocks[0].children.remove(0);
        current.blocks.push(b);
        local.blocks[0].text = "a edited".into();
        let r = rebase(&base, &local, &current);
        assert_eq!(r.document.block_count(), 2);
        assert_eq!(r.document.blocks[0].text, "a edited");
    }

    #[test]
    fn rebasing_twice_is_stable() {
        let base = doc(vec![p("a"), p("b")]);
        let local = edited(&base, 0, "a1");
        let mut current = base.clone();
        current.blocks.push(p("c"));
        let once = rebase(&base, &local, &current);
        let twice = rebase(&local, &once.document, &once.document);
        assert_eq!(twice.document, once.document);
    }
}
