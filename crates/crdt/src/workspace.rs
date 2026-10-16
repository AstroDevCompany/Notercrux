use crate::{import, snapshot, updates_since, version, Result, Version};
use loro::{
    LoroDoc, LoroMap, LoroMovableList, LoroTree, LoroValue, TreeID, TreeParentId, ValueOrContainer,
};
use notercrux_domain::{DatabaseSchema, Page, PageId, PageKind, Property, View};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// The workspace as a CRDT: page tree and metadata, database schemas, record property values.
///
/// Merge rules (see `docs/decisions/0007-crdt-merge-layer.md`):
/// * the page tree is a movable tree: concurrent moves never create cycles and nothing is lost;
/// * page metadata fields and each property definition, view and value are last-writer-wins on their own;
/// * schema and values are independent, so a concurrent type change and a value edit both survive in the
///   CRDT; the local (materialized) view validates values against the merged schema and hides those that
///   no longer fit, without deleting them.
pub struct WorkspaceCrdt {
    doc: LoroDoc,
    index: RefCell<Option<HashMap<PageId, TreeID>>>,
}

impl Default for WorkspaceCrdt {
    fn default() -> Self {
        Self::new()
    }
}

fn kind_name(k: PageKind) -> &'static str {
    match k {
        PageKind::Page => "page",
        PageKind::Database => "database",
    }
}

fn meta_str(meta: &LoroMap, key: &str) -> Option<String> {
    match meta.get(key) {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

fn meta_i64(meta: &LoroMap, key: &str) -> Option<i64> {
    match meta.get(key) {
        Some(ValueOrContainer::Value(LoroValue::I64(n))) => Some(n),
        _ => None,
    }
}

fn set_opt_str(meta: &LoroMap, key: &str, v: Option<&str>) -> Result<()> {
    match v {
        Some(v) => {
            if meta_str(meta, key).as_deref() != Some(v) {
                meta.insert(key, v)?;
            }
        }
        None => {
            if meta.get(key).is_some() {
                meta.delete(key)?;
            }
        }
    }
    Ok(())
}

fn set_opt_i64(meta: &LoroMap, key: &str, v: Option<i64>) -> Result<()> {
    match v {
        Some(v) => {
            if meta_i64(meta, key) != Some(v) {
                meta.insert(key, v)?;
            }
        }
        None => {
            if meta.get(key).is_some() {
                meta.delete(key)?;
            }
        }
    }
    Ok(())
}

fn list_ids(list: &LoroMovableList) -> Vec<String> {
    match list.get_value() {
        LoroValue::List(items) => items
            .iter()
            .filter_map(|v| match v {
                LoroValue::String(s) => Some(s.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Makes `list` equal `want` with as few operations as possible (inserts, moves, deletes).
fn sync_list(list: &LoroMovableList, want: &[String]) -> Result<()> {
    let mut cur = list_ids(list);
    for (i, w) in want.iter().enumerate() {
        if cur.get(i) == Some(w) {
            continue;
        }
        match cur.iter().skip(i + 1).position(|c| c == w) {
            Some(off) => {
                let from = i + 1 + off;
                list.mov(from, i)?;
                let item = cur.remove(from);
                cur.insert(i, item);
            }
            None => {
                list.insert(i, w.as_str())?;
                cur.insert(i, w.clone());
            }
        }
    }
    if cur.len() > want.len() {
        list.delete(want.len(), cur.len() - want.len())?;
    }
    Ok(())
}

fn json_map(map: &LoroMap) -> HashMap<String, String> {
    let mut out = HashMap::new();
    map.for_each(|k, v| {
        if let ValueOrContainer::Value(LoroValue::String(s)) = v {
            out.insert(k.to_string(), s.to_string());
        }
    });
    out
}

/// Repairs a merged schema so it validates: removes sorts, filters and hidden columns that point at
/// properties another device deleted, and guarantees at least one view.
pub fn repair_schema(schema: &mut DatabaseSchema) {
    let known: HashSet<String> = schema.properties.iter().map(|p| p.id.clone()).collect();
    let ok = |id: &str| id == notercrux_domain::TITLE_PROPERTY || known.contains(id);
    for v in &mut schema.views {
        v.sorts.retain(|s| ok(&s.property));
        v.filters.retain(|f| ok(&f.property));
        v.hidden.retain(|h| ok(h));
    }
    if schema.views.is_empty() {
        schema.views = DatabaseSchema::default().views;
    }
    // A relation without a target cannot be satisfied: fall back to plain text rather than reject the schema.
    for p in &mut schema.properties {
        if p.kind == notercrux_domain::PropertyKind::Relation && p.relation_database.is_none() {
            p.kind = notercrux_domain::PropertyKind::Text;
        }
    }
}

impl WorkspaceCrdt {
    pub fn new() -> Self {
        Self {
            doc: LoroDoc::new(),
            index: RefCell::new(None),
        }
    }

    pub fn from_snapshot(bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::new();
        doc.import(bytes)?;
        Ok(Self {
            doc,
            index: RefCell::new(None),
        })
    }

    pub fn snapshot(&self) -> Result<Vec<u8>> {
        snapshot(&self.doc)
    }

    pub fn version(&self) -> Version {
        version(&self.doc)
    }

    pub fn updates_since(&self, since: &Version) -> Result<Vec<u8>> {
        updates_since(&self.doc, since)
    }

    pub fn import(&self, bytes: &[u8]) -> Result<()> {
        self.index.replace(None);
        import(&self.doc, bytes)
    }

    fn tree(&self) -> LoroTree {
        self.doc.get_tree("pages")
    }

    /// Page id -> tree node, rebuilt after imports and structural changes.
    fn ids(&self) -> HashMap<PageId, TreeID> {
        if let Some(m) = self.index.borrow().as_ref() {
            return m.clone();
        }
        let tree = self.tree();
        let mut m = HashMap::new();
        for n in tree.nodes() {
            // `nodes()` includes deleted nodes; a deleted page must be re-created, not written into.
            if tree.is_node_deleted(&n).unwrap_or(true) {
                continue;
            }
            if let Some(id) = tree
                .get_meta(n)
                .ok()
                .and_then(|meta| meta_str(&meta, "id"))
                .and_then(|s| s.parse().ok())
            {
                m.entry(id).or_insert(n);
            }
        }
        self.index.replace(Some(m.clone()));
        m
    }

    // ---- pages -------------------------------------------------------------------------------------

    fn page_of(
        &self,
        tree: &LoroTree,
        node: TreeID,
        parent: Option<PageId>,
        position: u32,
    ) -> Option<Page> {
        let meta = tree.get_meta(node).ok()?;
        Some(Page {
            id: meta_str(&meta, "id")?.parse().ok()?,
            parent_id: parent,
            kind: if meta_str(&meta, "kind").as_deref() == Some("database") {
                PageKind::Database
            } else {
                PageKind::Page
            },
            title: meta_str(&meta, "title").unwrap_or_default(),
            icon: meta_str(&meta, "icon"),
            position,
            created_at: meta_i64(&meta, "createdAt").unwrap_or(0),
            updated_at: meta_i64(&meta, "createdAt").unwrap_or(0),
            archived_at: meta_i64(&meta, "archivedAt"),
        })
    }

    /// All live pages with their parent and position (index among siblings). Pages whose id is
    /// duplicated in the tree appear once (the first in document order).
    pub fn pages(&self) -> Vec<Page> {
        let tree = self.tree();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        fn walk(
            this: &WorkspaceCrdt,
            tree: &LoroTree,
            parent: TreeParentId,
            parent_id: Option<PageId>,
            seen: &mut HashSet<PageId>,
            out: &mut Vec<Page>,
        ) {
            for (i, node) in tree
                .children(parent)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
            {
                let Some(page) = this.page_of(tree, node, parent_id, i as u32) else {
                    continue;
                };
                if !seen.insert(page.id) {
                    continue;
                }
                let id = page.id;
                out.push(page);
                walk(this, tree, TreeParentId::Node(node), Some(id), seen, out);
            }
        }
        walk(self, &tree, TreeParentId::Root, None, &mut seen, &mut out);
        out
    }

    fn write_meta(&self, tree: &LoroTree, node: TreeID, p: &Page) -> Result<()> {
        let meta = tree.get_meta(node)?;
        if meta_str(&meta, "id").as_deref() != Some(&p.id.to_string()) {
            meta.insert("id", p.id.to_string())?;
        }
        if meta_str(&meta, "kind").as_deref() != Some(kind_name(p.kind)) {
            meta.insert("kind", kind_name(p.kind))?;
        }
        if meta_str(&meta, "title").as_deref() != Some(p.title.as_str()) {
            meta.insert("title", p.title.as_str())?;
        }
        set_opt_str(&meta, "icon", p.icon.as_deref())?;
        set_opt_i64(&meta, "archivedAt", p.archived_at)?;
        if meta_i64(&meta, "createdAt").is_none() {
            meta.insert("createdAt", p.created_at)?;
        }
        Ok(())
    }

    /// Brings the page tree in line with `target` (every live page, parent and position included).
    /// Returns false if nothing changed. Used after structural changes that touch many pages.
    pub fn apply_pages(&self, target: &[Page]) -> Result<bool> {
        let same = |a: &Page, b: &Page| {
            (
                a.id,
                a.parent_id,
                a.kind,
                &a.title,
                &a.icon,
                a.position,
                a.archived_at,
            ) == (
                b.id,
                b.parent_id,
                b.kind,
                &b.title,
                &b.icon,
                b.position,
                b.archived_at,
            )
        };
        let current = self.pages();
        if current.len() == target.len()
            && current.iter().zip(sorted(target)).all(|(a, b)| same(a, &b))
        {
            return Ok(false);
        }
        let tree = self.tree();
        let mut by_id = self.ids();
        let mut children: HashMap<Option<PageId>, Vec<&Page>> = HashMap::new();
        for p in target {
            children.entry(p.parent_id).or_default().push(p);
        }
        for list in children.values_mut() {
            list.sort_by_key(|p| p.position);
        }
        let wanted: HashSet<PageId> = target.iter().map(|p| p.id).collect();
        let mut cache: HashMap<TreeParentId, Vec<TreeID>> = HashMap::new();

        // Parents before children, so a moved subtree finds its new parent.
        let mut stack: Vec<(Option<PageId>, TreeParentId)> = vec![(None, TreeParentId::Root)];
        while let Some((pid, parent)) = stack.pop() {
            let Some(kids) = children.get(&pid) else {
                continue;
            };
            for (i, p) in kids.iter().enumerate() {
                let list = cache
                    .entry(parent)
                    .or_insert_with(|| tree.children(parent).unwrap_or_default());
                let node = match by_id.get(&p.id).copied() {
                    Some(node) => {
                        let cur_parent = tree.parent(node).unwrap_or(TreeParentId::Root);
                        if !(cur_parent == parent && list.get(i) == Some(&node)) {
                            if let Some(old) = cache.get_mut(&cur_parent) {
                                old.retain(|x| *x != node);
                            }
                            let list = cache
                                .entry(parent)
                                .or_insert_with(|| tree.children(parent).unwrap_or_default());
                            list.retain(|x| *x != node);
                            let at = i.min(list.len());
                            tree.mov_to(node, parent, at)?;
                            list.insert(at, node);
                        }
                        node
                    }
                    None => {
                        let at = i.min(list.len());
                        let node = tree.create_at(parent, at)?;
                        list.insert(at, node);
                        by_id.insert(p.id, node);
                        node
                    }
                };
                self.write_meta(&tree, node, p)?;
                stack.push((Some(p.id), TreeParentId::Node(node)));
            }
        }
        for (id, node) in &by_id {
            if !wanted.contains(id)
                && tree.contains(*node)
                && !tree.is_node_deleted(node).unwrap_or(true)
            {
                tree.delete(*node)?;
            }
        }
        self.doc.commit();
        self.index.replace(None);
        Ok(true)
    }

    /// Updates one page's metadata (title, icon, archive state, kind) in place. Creates the page if it
    /// is not in the tree yet (at the end of its parent). Cheap: touches only that page.
    pub fn set_page(&self, p: &Page) -> Result<()> {
        let tree = self.tree();
        let ids = self.ids();
        match ids.get(&p.id) {
            Some(node) => self.write_meta(&tree, *node, p)?,
            None => {
                let parent = match p.parent_id {
                    Some(pid) => ids
                        .get(&pid)
                        .map(|n| TreeParentId::Node(*n))
                        .unwrap_or(TreeParentId::Root),
                    None => TreeParentId::Root,
                };
                let at = tree.children_num(parent).unwrap_or(0);
                let node = tree.create_at(parent, at.min(p.position as usize))?;
                self.write_meta(&tree, node, p)?;
            }
        }
        self.doc.commit();
        self.index.replace(None);
        Ok(())
    }

    // ---- database schemas ----------------------------------------------------------------------------

    fn schemas_map(&self) -> LoroMap {
        self.doc.get_map("schemas")
    }

    /// Stores a schema: each property and view is its own entry, so devices editing different ones merge.
    pub fn set_schema(&self, db: PageId, schema: &DatabaseSchema) -> Result<()> {
        let entry = self.schemas_map().ensure_mergeable_map(&db.to_string())?;
        let props = entry.ensure_mergeable_map("properties")?;
        let views = entry.ensure_mergeable_map("views")?;
        let porder = entry.ensure_mergeable_movable_list("propertyOrder")?;
        let vorder = entry.ensure_mergeable_movable_list("viewOrder")?;
        if entry.get("version").is_none() {
            entry.insert("version", schema.version as i64)?;
        }

        let put = |map: &LoroMap, id: &str, json: String| -> Result<()> {
            if json_map(map).get(id) != Some(&json) {
                map.insert(id, json)?;
            }
            Ok(())
        };
        for p in &schema.properties {
            put(&props, &p.id, serde_json::to_string(p)?)?;
        }
        for v in &schema.views {
            put(&views, &v.id, serde_json::to_string(v)?)?;
        }
        let keep_p: HashSet<&str> = schema.properties.iter().map(|p| p.id.as_str()).collect();
        for k in json_map(&props).keys() {
            if !keep_p.contains(k.as_str()) {
                props.delete(k)?;
            }
        }
        let keep_v: HashSet<&str> = schema.views.iter().map(|v| v.id.as_str()).collect();
        for k in json_map(&views).keys() {
            if !keep_v.contains(k.as_str()) {
                views.delete(k)?;
            }
        }
        sync_list(
            &porder,
            &schema
                .properties
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>(),
        )?;
        sync_list(
            &vorder,
            &schema
                .views
                .iter()
                .map(|v| v.id.clone())
                .collect::<Vec<_>>(),
        )?;
        self.doc.commit();
        Ok(())
    }

    fn read_schema(&self, entry: &LoroMap) -> Option<DatabaseSchema> {
        let get_map = |k: &str| match entry.get(k) {
            Some(ValueOrContainer::Container(loro::Container::Map(m))) => Some(m),
            _ => None,
        };
        let get_list = |k: &str| match entry.get(k) {
            Some(ValueOrContainer::Container(loro::Container::MovableList(l))) => Some(l),
            _ => None,
        };
        let props = json_map(&get_map("properties")?);
        let views = json_map(&get_map("views")?);
        let order =
            |list: Option<LoroMovableList>, have: &HashMap<String, String>| -> Vec<String> {
                let mut seen = HashSet::new();
                let mut out: Vec<String> = list
                    .map(|l| list_ids(&l))
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| have.contains_key(id) && seen.insert(id.clone()))
                    .collect();
                // Entries missing from the order list (concurrent adds) are kept, in a stable order.
                let mut rest: Vec<&String> = have.keys().filter(|k| !seen.contains(*k)).collect();
                rest.sort();
                out.extend(rest.into_iter().cloned());
                out
            };
        let properties: Vec<Property> = order(get_list("propertyOrder"), &props)
            .iter()
            .filter_map(|id| serde_json::from_str(&props[id]).ok())
            .collect();
        let views: Vec<View> = order(get_list("viewOrder"), &views)
            .iter()
            .filter_map(|id| serde_json::from_str(&views[id]).ok())
            .collect();
        let mut schema = DatabaseSchema {
            version: meta_i64(entry, "version").unwrap_or(1) as u32,
            properties,
            views,
        };
        repair_schema(&mut schema);
        Some(schema)
    }

    pub fn schemas(&self) -> HashMap<PageId, DatabaseSchema> {
        let mut out = HashMap::new();
        let map = self.schemas_map();
        let mut entries = Vec::new();
        map.for_each(|k, v| {
            if let (Ok(id), ValueOrContainer::Container(loro::Container::Map(m))) =
                (k.parse::<PageId>(), v)
            {
                entries.push((id, m));
            }
        });
        for (id, m) in entries {
            if let Some(s) = self.read_schema(&m) {
                out.insert(id, s);
            }
        }
        out
    }

    // ---- property values -----------------------------------------------------------------------------

    fn values_map(&self) -> LoroMap {
        self.doc.get_map("values")
    }

    /// Sets (or, with `None`, clears) one value. Each cell is last-writer-wins on its own.
    pub fn set_value(&self, record: PageId, property: &str, value: Option<&Value>) -> Result<()> {
        let row = self
            .values_map()
            .ensure_mergeable_map(&record.to_string())?;
        match value {
            Some(v) => {
                let json = serde_json::to_string(v)?;
                if json_map(&row).get(property) != Some(&json) {
                    row.insert(property, json)?;
                }
            }
            None => {
                if row.get(property).is_some() {
                    row.delete(property)?;
                }
            }
        }
        self.doc.commit();
        Ok(())
    }

    /// Replaces all values of a record (used when a record is created, duplicated or its schema changes).
    pub fn set_record_values(&self, record: PageId, values: &HashMap<String, Value>) -> Result<()> {
        let row = self
            .values_map()
            .ensure_mergeable_map(&record.to_string())?;
        let have = json_map(&row);
        for (k, v) in values {
            let json = serde_json::to_string(v)?;
            if have.get(k) != Some(&json) {
                row.insert(k, json)?;
            }
        }
        for k in have.keys() {
            if !values.contains_key(k) {
                row.delete(k)?;
            }
        }
        self.doc.commit();
        Ok(())
    }

    pub fn values(&self) -> HashMap<PageId, HashMap<String, Value>> {
        let mut rows = Vec::new();
        self.values_map().for_each(|k, v| {
            if let (Ok(id), ValueOrContainer::Container(loro::Container::Map(m))) =
                (k.parse::<PageId>(), v)
            {
                rows.push((id, m));
            }
        });
        rows.into_iter()
            .map(|(id, m)| {
                let vals = json_map(&m)
                    .into_iter()
                    .filter_map(|(k, s)| Some((k, serde_json::from_str(&s).ok()?)))
                    .collect();
                (id, vals)
            })
            .collect()
    }
}

fn sorted(pages: &[Page]) -> Vec<Page> {
    // `pages()` yields parents before children in tree order; compare against the same order.
    let mut by_parent: HashMap<Option<PageId>, Vec<&Page>> = HashMap::new();
    for p in pages {
        by_parent.entry(p.parent_id).or_default().push(p);
    }
    for l in by_parent.values_mut() {
        l.sort_by_key(|p| p.position);
    }
    let mut out = Vec::new();
    fn walk(by: &HashMap<Option<PageId>, Vec<&Page>>, parent: Option<PageId>, out: &mut Vec<Page>) {
        for p in by.get(&parent).into_iter().flatten() {
            out.push((*p).clone());
            walk(by, Some(p.id), out);
        }
    }
    walk(&by_parent, None, &mut out);
    out
}
