use crate::{import, snapshot, updates_since, version, CrdtError, Result, Version};
use loro::{
    ExpandType, LoroDoc, LoroMap, LoroText, LoroValue, StyleConfig, StyleConfigMap, TextDelta,
    TreeID, TreeParentId, ValueOrContainer,
};
use notercrux_domain::{Block, BlockId, BlockKind, Document, Mark, DOCUMENT_SCHEMA_VERSION};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Mark types the editor can produce. Their expand behaviour must be identical on every replica.
const MARK_TYPES: &[(&str, ExpandType)] = &[
    ("bold", ExpandType::After),
    ("italic", ExpandType::After),
    ("strike", ExpandType::After),
    ("underline", ExpandType::After),
    ("code", ExpandType::After),
    ("link", ExpandType::None),
    ("pageLink", ExpandType::None),
];

/// One page's content as a CRDT.
pub struct PageCrdt {
    doc: LoroDoc,
}

impl Default for PageCrdt {
    fn default() -> Self {
        Self::new()
    }
}

fn configure(doc: &LoroDoc) {
    let mut styles = StyleConfigMap::new();
    for (key, expand) in MARK_TYPES {
        styles.insert((*key).into(), StyleConfig { expand: *expand });
    }
    doc.config_text_style(styles);
}

fn kind_name(k: BlockKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "paragraph".into())
}

fn kind_from(s: &str) -> BlockKind {
    serde_json::from_value(Value::String(s.to_string())).unwrap_or(BlockKind::Paragraph)
}

/// Marks as stored in the text: `true` when the mark has no attributes, else the attribute map as JSON.
fn mark_value(attrs: &Map<String, Value>) -> LoroValue {
    if attrs.is_empty() {
        LoroValue::from(true)
    } else {
        LoroValue::from(serde_json::to_string(attrs).unwrap_or_default())
    }
}

fn mark_attrs(v: &LoroValue) -> Map<String, Value> {
    match v {
        LoroValue::String(s) => serde_json::from_str(s.as_ref()).unwrap_or_default(),
        _ => Map::new(),
    }
}

type Runs = BTreeMap<String, Vec<(usize, usize, LoroValue)>>;

/// Canonical per-type runs of a block's marks: paints marks in order (later wins on overlap, like the
/// CRDT does) and run-length encodes the result, so two mark sets compare equal exactly when they
/// would render the same.
fn runs_of_marks(marks: &[Mark], len: usize) -> Runs {
    let mut painted: BTreeMap<&str, Vec<Option<LoroValue>>> = BTreeMap::new();
    for m in marks {
        let (from, to) = (m.from as usize, (m.to as usize).min(len));
        if from >= to {
            continue;
        }
        let row = painted
            .entry(m.kind.as_str())
            .or_insert_with(|| vec![None; len]);
        let v = mark_value(&m.attrs);
        for slot in &mut row[from..to] {
            *slot = Some(v.clone());
        }
    }
    painted
        .into_iter()
        .map(|(k, row)| (k.to_string(), encode_runs(&row)))
        .filter(|(_, r)| !r.is_empty())
        .collect()
}

fn encode_runs(row: &[Option<LoroValue>]) -> Vec<(usize, usize, LoroValue)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        if let Some(v) = &row[i] {
            let start = i;
            while i < row.len() && row[i].as_ref() == Some(v) {
                i += 1;
            }
            out.push((start, i, v.clone()));
        } else {
            i += 1;
        }
    }
    out
}

fn runs_of_text(text: &LoroText) -> Runs {
    let mut painted: BTreeMap<String, Vec<Option<LoroValue>>> = BTreeMap::new();
    let len = text.len_unicode();
    let mut pos = 0;
    for seg in text.to_delta() {
        if let TextDelta::Insert { insert, attributes } = seg {
            let n = insert.chars().count();
            for (k, v) in attributes.unwrap_or_default() {
                let row = painted.entry(k).or_insert_with(|| vec![None; len]);
                for slot in &mut row[pos..pos + n] {
                    *slot = Some(v.clone());
                }
            }
            pos += n;
        }
    }
    painted
        .into_iter()
        .map(|(k, row)| (k, encode_runs(&row)))
        .filter(|(_, r)| !r.is_empty())
        .collect()
}

fn marks_of_text(text: &LoroText) -> Vec<Mark> {
    let mut marks: Vec<Mark> = runs_of_text(text)
        .into_iter()
        .flat_map(|(kind, runs)| {
            runs.into_iter().map(move |(from, to, v)| Mark {
                from: from as u32,
                to: to as u32,
                kind: kind.clone(),
                attrs: mark_attrs(&v),
            })
        })
        .collect();
    marks.sort_by(|a, b| (a.from, a.to, &a.kind).cmp(&(b.from, b.to, &b.kind)));
    marks
}

/// A deterministic id for a duplicate of `raw` (two replicas that independently created nodes with
/// the same block id). Every replica derives the same one, so ids stay stable and unique.
fn derived_id(raw: &str, node: TreeID) -> BlockId {
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    h.update(node.to_string().as_bytes());
    let d = h.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    BlockId(uuid::Uuid::from_bytes(b))
}

impl PageCrdt {
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        configure(&doc);
        Self { doc }
    }

    pub fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::new();
        configure(&doc);
        doc.import(bytes)?;
        Ok(Self { doc })
    }

    pub fn snapshot(&self) -> Result<Vec<u8>> {
        snapshot(&self.doc)
    }

    pub fn version(&self) -> Version {
        version(&self.doc)
    }

    /// Operations made since `since`, to send to other devices.
    pub fn updates_since(&self, since: &Version) -> Result<Vec<u8>> {
        updates_since(&self.doc, since)
    }

    /// Applies updates received from another device (idempotent).
    pub fn import(&self, bytes: &[u8]) -> Result<()> {
        import(&self.doc, bytes)
    }

    pub fn is_empty(&self) -> bool {
        self.doc
            .get_tree("blocks")
            .children(TreeParentId::Root)
            .is_none_or(|c| c.is_empty())
    }

    // ---- reading ----------------------------------------------------------------------------------

    fn text_of(&self, id: TreeID) -> Result<LoroText> {
        let meta = self.doc.get_tree("blocks").get_meta(id)?;
        match meta.get("text") {
            Some(ValueOrContainer::Container(loro::Container::Text(t))) => Ok(t),
            _ => Err(CrdtError::Malformed("block without text".into())),
        }
    }

    fn meta_string(meta: &LoroMap, key: &str) -> Option<String> {
        match meta.get(key) {
            Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
            _ => None,
        }
    }

    /// Live nodes in document order with their effective (unique) block ids.
    fn index(&self) -> HashMap<TreeID, BlockId> {
        let tree = self.doc.get_tree("blocks");
        let mut seen = HashSet::new();
        let mut out = HashMap::new();
        fn walk(
            doc: &LoroDoc,
            parent: TreeParentId,
            seen: &mut HashSet<String>,
            out: &mut HashMap<TreeID, BlockId>,
        ) {
            let tree = doc.get_tree("blocks");
            for child in tree.children(parent).unwrap_or_default() {
                let raw = tree
                    .get_meta(child)
                    .ok()
                    .and_then(|m| PageCrdt::meta_string(&m, "id"))
                    .unwrap_or_default();
                let id = match raw.parse::<BlockId>() {
                    Ok(id) if seen.insert(raw.clone()) => id,
                    _ => derived_id(&raw, child),
                };
                out.insert(child, id);
                walk(doc, TreeParentId::Node(child), seen, out);
            }
        }
        walk(&self.doc, TreeParentId::Root, &mut seen, &mut out);
        let _ = tree;
        out
    }

    fn block(&self, tree_id: TreeID, ids: &HashMap<TreeID, BlockId>) -> Result<Block> {
        let tree = self.doc.get_tree("blocks");
        let meta = tree.get_meta(tree_id)?;
        let text = self.text_of(tree_id)?;
        let attrs = Self::meta_string(&meta, "attrs")
            .and_then(|s| serde_json::from_str::<Map<String, Value>>(&s).ok())
            .unwrap_or_default();
        let children = tree
            .children(TreeParentId::Node(tree_id))
            .unwrap_or_default()
            .into_iter()
            .map(|c| self.block(c, ids))
            .collect::<Result<Vec<_>>>()?;
        Ok(Block {
            id: ids.get(&tree_id).copied().unwrap_or_else(BlockId::new),
            kind: kind_from(&Self::meta_string(&meta, "type").unwrap_or_default()),
            attrs,
            text: text.to_string(),
            marks: marks_of_text(&text),
            children,
        })
    }

    pub fn to_document(&self) -> Document {
        let tree = self.doc.get_tree("blocks");
        let ids = self.index();
        let blocks = tree
            .children(TreeParentId::Root)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| self.block(c, &ids).ok())
            .collect();
        Document {
            version: DOCUMENT_SCHEMA_VERSION,
            blocks,
        }
    }

    // ---- writing ----------------------------------------------------------------------------------

    /// Brings the CRDT in line with `target` using the smallest set of operations: unchanged blocks
    /// produce nothing, edited text becomes a single splice, moved blocks become tree moves, blocks
    /// missing from `target` are deleted. Returns false if nothing changed.
    pub fn apply_document(&self, target: &Document) -> Result<bool> {
        self.apply_document_tracked(target, &mut HashSet::new())
    }

    /// Like [`PageCrdt::apply_document`], and records in `touched` every block whose content (text,
    /// formatting, attributes, kind) was created or changed by this call. The merge layer uses it to know
    /// which blocks this device edited, so it can recover them if another device deletes them.
    pub fn apply_document_tracked(
        &self,
        target: &Document,
        touched: &mut HashSet<BlockId>,
    ) -> Result<bool> {
        if self.to_document() == *target {
            return Ok(false);
        }
        let tree = self.doc.get_tree("blocks");
        let ids = self.index();
        let mut by_id: HashMap<BlockId, TreeID> = ids.iter().map(|(t, b)| (*b, *t)).collect();
        let mut wanted: HashSet<BlockId> = HashSet::new();
        let mut cache: HashMap<TreeParentId, Vec<TreeID>> = HashMap::new();

        fn children_of<'a>(
            tree: &loro::LoroTree,
            cache: &'a mut HashMap<TreeParentId, Vec<TreeID>>,
            parent: TreeParentId,
        ) -> &'a mut Vec<TreeID> {
            cache
                .entry(parent)
                .or_insert_with(|| tree.children(parent).unwrap_or_default())
        }

        #[allow(clippy::too_many_arguments)]
        fn sync_level(
            this: &PageCrdt,
            tree: &loro::LoroTree,
            parent: TreeParentId,
            target: &[Block],
            by_id: &mut HashMap<BlockId, TreeID>,
            wanted: &mut HashSet<BlockId>,
            cache: &mut HashMap<TreeParentId, Vec<TreeID>>,
            touched: &mut HashSet<BlockId>,
        ) -> Result<()> {
            for (i, tb) in target.iter().enumerate() {
                wanted.insert(tb.id);
                let tid = match by_id.get(&tb.id).copied() {
                    Some(tid) => {
                        let cur_parent = tree.parent(tid).unwrap_or(TreeParentId::Root);
                        let in_place = cur_parent == parent
                            && children_of(tree, cache, parent).get(i) == Some(&tid);
                        if !in_place {
                            // Remove from wherever the cache believes it is, then move.
                            if let Some(list) = cache.get_mut(&cur_parent) {
                                list.retain(|x| *x != tid);
                            }
                            let list = children_of(tree, cache, parent);
                            list.retain(|x| *x != tid);
                            let at = i.min(list.len());
                            tree.mov_to(tid, parent, at)?;
                            list.insert(at, tid);
                        }
                        tid
                    }
                    None => {
                        let list = children_of(tree, cache, parent);
                        let at = i.min(list.len());
                        let tid = tree.create_at(parent, at)?;
                        list.insert(at, tid);
                        let meta = tree.get_meta(tid)?;
                        meta.insert("id", tb.id.to_string())?;
                        meta.insert_container("text", LoroText::new())?;
                        by_id.insert(tb.id, tid);
                        touched.insert(tb.id);
                        tid
                    }
                };
                if this.update_node(tree, tid, tb)? {
                    touched.insert(tb.id);
                }
                sync_level(
                    this,
                    tree,
                    TreeParentId::Node(tid),
                    &tb.children,
                    by_id,
                    wanted,
                    cache,
                    touched,
                )?;
            }
            Ok(())
        }

        sync_level(
            self,
            &tree,
            TreeParentId::Root,
            &target.blocks,
            &mut by_id,
            &mut wanted,
            &mut cache,
            touched,
        )?;

        // Blocks that are no longer in the document. Deleting a parent removes its subtree, so only
        // topmost leftovers need an explicit delete.
        for (tid, id) in &ids {
            if !wanted.contains(id)
                && tree.contains(*tid)
                && !tree.is_node_deleted(tid).unwrap_or(true)
            {
                tree.delete(*tid)?;
            }
        }
        self.doc.commit();
        Ok(true)
    }

    /// Returns true if the block's content changed.
    fn update_node(&self, tree: &loro::LoroTree, tid: TreeID, tb: &Block) -> Result<bool> {
        let mut changed = false;
        let meta = tree.get_meta(tid)?;
        let kind = kind_name(tb.kind);
        if Self::meta_string(&meta, "type").as_deref() != Some(kind.as_str()) {
            meta.insert("type", kind)?;
            changed = true;
        }
        let attrs = serde_json::to_string(&tb.attrs)?;
        if Self::meta_string(&meta, "attrs").as_deref().unwrap_or("{}") != attrs {
            meta.insert("attrs", attrs)?;
            changed = true;
        }
        let text = self.text_of(tid)?;
        let current: Vec<char> = text.to_string().chars().collect();
        let wanted: Vec<char> = tb.text.chars().collect();
        if current != wanted {
            changed = true;
            let prefix = current
                .iter()
                .zip(&wanted)
                .take_while(|(a, b)| a == b)
                .count();
            let max_suffix = current.len().min(wanted.len()) - prefix;
            let suffix = current
                .iter()
                .rev()
                .zip(wanted.iter().rev())
                .take(max_suffix)
                .take_while(|(a, b)| a == b)
                .count();
            let del = current.len() - prefix - suffix;
            let ins: String = wanted[prefix..wanted.len() - suffix].iter().collect();
            if del > 0 {
                text.delete(prefix, del)?;
            }
            if !ins.is_empty() {
                text.insert(prefix, &ins)?;
            }
        }
        let len = text.len_unicode();
        let want = runs_of_marks(&tb.marks, len);
        let have = runs_of_text(&text);
        if want != have {
            changed = true;
            for key in want.keys().chain(have.keys()).collect::<HashSet<_>>() {
                if want.get(key) == have.get(key) {
                    continue;
                }
                if have.contains_key(key) && len > 0 {
                    text.unmark(0..len, key)?;
                }
                for (from, to, v) in want.get(key).into_iter().flatten() {
                    text.mark(*from..*to, key, v.clone())?;
                }
            }
        }
        Ok(changed)
    }

    /// Deep value of one block's text, including text of deleted blocks (for recovery).
    pub fn all_text_including_deleted(&self) -> Vec<(BlockId, String, bool)> {
        let tree = self.doc.get_tree("blocks");
        tree.get_nodes(true)
            .into_iter()
            .filter_map(|n| {
                let meta = tree.get_meta(n.id).ok()?;
                let raw = Self::meta_string(&meta, "id")?;
                let deleted = tree.is_node_deleted(&n.id).unwrap_or(false);
                Some((
                    raw.parse().ok()?,
                    self.text_of(n.id).ok()?.to_string(),
                    deleted,
                ))
            })
            .collect()
    }
}
