use crate::{PageCrdt, Result};
use notercrux_domain::{Block, BlockId, BlockKind, Document};
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// Icon of the callout that collects edits a merge would otherwise have dropped.
pub const RECOVERED_ICON: &str = "♻️";

#[derive(Debug, Default)]
pub struct MergeOutcome {
    /// The page content after merging (including any recovered blocks).
    pub document: Document,
    /// How many blocks were recovered into a "recovered edits" callout.
    pub recovered: usize,
    /// Operations created by the recovery itself (to send to other devices), if any.
    pub local_update: Option<Vec<u8>>,
}

fn flatten<'a>(blocks: &'a [Block], out: &mut Vec<&'a Block>) {
    for b in blocks {
        out.push(b);
        flatten(&b.children, out);
    }
}

/// Imports remote updates into `crdt` and returns the merged document.
///
/// Rule against silent loss. A merge never drops text that someone typed concurrently with a delete:
///
/// 1. **This device edited a block and the merge deletes it.** `dirty` holds the blocks this device
///    edited since the server last confirmed its operations (from [`PageCrdt::apply_document_tracked`]).
///    If one of them is deleted by the imported updates, it is recovered.
/// 2. **This device had deleted a block and an edit to it arrives.** The deleter never saw that edit
///    (anything it had seen is already in its tombstone), so a change to a deleted block's text caused by
///    an import is by definition concurrent with the delete. It is recovered.
///
/// Between them, any concurrent delete/edit pair is recovered by at least one device (rule 1 by the
/// editor if its edit was still unconfirmed, rule 2 by the deleter when the edit reaches it). Recovered
/// text goes into a "Recovered edits" callout at the end of the page; a block whose text already appears
/// in a live block is not recovered again. Deleted blocks nobody edited concurrently are simply deleted.
pub fn merge_page_updates(
    crdt: &PageCrdt,
    dirty: &HashSet<BlockId>,
    updates: &[Vec<u8>],
) -> Result<MergeOutcome> {
    let before = crdt.to_document();
    let tomb_before: HashMap<BlockId, (String, bool)> = crdt
        .all_text_including_deleted()
        .into_iter()
        .map(|(id, text, deleted)| (id, (text, deleted)))
        .collect();
    for u in updates {
        crdt.import(u)?;
    }
    let after = crdt.to_document();
    let imported = crdt.version();
    let tomb_after: HashMap<BlockId, (String, bool)> = crdt
        .all_text_including_deleted()
        .into_iter()
        .map(|(id, text, deleted)| (id, (text, deleted)))
        .collect();

    let mut before_flat = Vec::new();
    flatten(&before.blocks, &mut before_flat);
    let before_by_id: HashMap<BlockId, &Block> = before_flat.iter().map(|b| (b.id, *b)).collect();
    let mut after_flat = Vec::new();
    flatten(&after.blocks, &mut after_flat);
    let alive: HashSet<BlockId> = after_flat.iter().map(|b| b.id).collect();
    let live_texts: HashSet<&str> = after_flat.iter().map(|b| b.text.as_str()).collect();

    let mut lost: Vec<Block> = Vec::new();
    let mut seen_text: HashSet<String> = HashSet::new();
    let mut candidates: Vec<(BlockId, Block)> = Vec::new();

    // Rule 1: edited here, deleted by the merge.
    for b in &before_flat {
        if !alive.contains(&b.id) && dirty.contains(&b.id) {
            let (text, _) = tomb_after
                .get(&b.id)
                .cloned()
                .unwrap_or((b.text.clone(), true));
            let mut copy = (*b).clone();
            copy.text = text;
            candidates.push((b.id, copy));
        }
    }
    // Rule 2: deleted here earlier, and an import changed the text of the deleted block.
    for (id, (text_after, deleted_after)) in &tomb_after {
        if !deleted_after || alive.contains(id) {
            continue;
        }
        if let Some((text_before, true)) = tomb_before.get(id) {
            if text_before != text_after {
                let mut copy = before_by_id
                    .get(id)
                    .map(|b| (*b).clone())
                    .unwrap_or_else(|| Block::new(BlockKind::Paragraph, ""));
                copy.text = text_after.clone();
                candidates.push((*id, copy));
            }
        }
    }
    candidates.sort_by_key(|(id, _)| *id);
    for (_, mut copy) in candidates {
        if copy.text.is_empty()
            || live_texts.contains(copy.text.as_str())
            || !seen_text.insert(copy.text.clone())
        {
            continue;
        }
        copy.children.clear();
        copy.id = BlockId::new();
        lost.push(copy);
    }

    if lost.is_empty() {
        return Ok(MergeOutcome {
            document: after,
            recovered: 0,
            local_update: None,
        });
    }

    let mut doc = after;
    let mut callout = Block::new(
        BlockKind::Callout,
        "Recovered edits: another device deleted these blocks while they were being edited.",
    );
    callout.attrs.insert("icon".into(), json!(RECOVERED_ICON));
    callout.attrs.insert("tone".into(), json!("yellow"));
    let n = lost.len();
    callout.children = lost;
    doc.blocks.push(callout);
    crdt.apply_document(&doc)?;
    let local_update = Some(crdt.updates_since(&imported)?);
    Ok(MergeOutcome {
        document: crdt.to_document(),
        recovered: n,
        local_update,
    })
}
