//! Sync spike: Loro CRDT documents exchanged through an *encrypted, opaque* relay.
//!
//! * [`PageDoc`] models page content as a CRDT tree of blocks (stable ids, movable, nestable,
//!   each with a collaboratively edited text).
//! * [`Relay`] is an in-memory stand-in for the sync server. It only ever sees sealed blobs.
//! * [`Replica`] is one device: a document, a sync cursor, and the logic to push/pull idempotently.
//!
//! The goal is to answer feasibility questions (see `docs/decisions/0003-crdt-sync.md`), not to be
//! the final protocol.

use loro::{ExportMode, LoroDoc, LoroError, LoroText, TreeID, TreeParentId, VersionVector};
use notercrux_crypto::{CryptoError, Purpose, WorkspaceKey};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("relay unavailable")]
    Unavailable,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("crdt error: {0}")]
    Crdt(String),
    #[error("device has been revoked")]
    Revoked,
}

impl From<LoroError> for SyncError {
    fn from(e: LoroError) -> Self {
        SyncError::Crdt(e.to_string())
    }
}

impl From<loro::LoroEncodeError> for SyncError {
    fn from(e: loro::LoroEncodeError) -> Self {
        SyncError::Crdt(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SyncError>;

// ---- document ---------------------------------------------------------------------------------

/// One page's content as a CRDT. Block order, nesting and moves are tree operations; text edits
/// are character-level merges.
pub struct PageDoc {
    pub doc: LoroDoc,
}

pub type BlockRef = TreeID;

impl PageDoc {
    pub fn new(peer: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).expect("peer id is valid");
        Self { doc }
    }

    pub fn from_snapshot(peer: u64, bytes: &[u8]) -> Result<Self> {
        let doc = LoroDoc::from_snapshot(bytes)?;
        doc.set_peer_id(peer)?;
        Ok(Self { doc })
    }

    pub fn snapshot(&self) -> Result<Vec<u8>> {
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    pub fn add_block(
        &self,
        parent: Option<BlockRef>,
        index: usize,
        kind: &str,
        text: &str,
    ) -> Result<BlockRef> {
        let tree = self.doc.get_tree("blocks");
        let parent = parent.map_or(TreeParentId::Root, TreeParentId::Node);
        let id = tree.create_at(parent, index)?;
        let meta = tree.get_meta(id)?;
        meta.insert("type", kind)?;
        let t = meta.insert_container("text", LoroText::new())?;
        t.insert(0, text)?;
        self.doc.commit();
        Ok(id)
    }

    fn text(&self, id: BlockRef) -> Result<LoroText> {
        let meta = self.doc.get_tree("blocks").get_meta(id)?;
        match meta.get("text") {
            Some(loro::ValueOrContainer::Container(loro::Container::Text(t))) => Ok(t),
            _ => Err(SyncError::Crdt("block has no text".into())),
        }
    }

    pub fn insert_text(&self, id: BlockRef, pos: usize, s: &str) -> Result<()> {
        self.text(id)?.insert(pos, s)?;
        self.doc.commit();
        Ok(())
    }

    pub fn delete_text(&self, id: BlockRef, pos: usize, len: usize) -> Result<()> {
        self.text(id)?.delete(pos, len)?;
        self.doc.commit();
        Ok(())
    }

    pub fn move_block(&self, id: BlockRef, parent: Option<BlockRef>, index: usize) -> Result<()> {
        let parent = parent.map_or(TreeParentId::Root, TreeParentId::Node);
        self.doc.get_tree("blocks").mov_to(id, parent, index)?;
        self.doc.commit();
        Ok(())
    }

    pub fn delete_block(&self, id: BlockRef) -> Result<()> {
        self.doc.get_tree("blocks").delete(id)?;
        self.doc.commit();
        Ok(())
    }

    /// Text of a block even if it has been deleted (tombstoned content stays recoverable).
    pub fn block_text(&self, id: BlockRef) -> Result<String> {
        Ok(self.text(id)?.to_string())
    }

    pub fn is_deleted(&self, id: BlockRef) -> bool {
        self.doc
            .get_tree("blocks")
            .is_node_deleted(&id)
            .unwrap_or(true)
    }

    /// Live blocks in document order, as `(depth, text)`.
    pub fn outline(&self) -> Vec<(usize, String)> {
        fn walk(d: &PageDoc, parent: TreeParentId, depth: usize, out: &mut Vec<(usize, String)>) {
            let tree = d.doc.get_tree("blocks");
            for id in tree.children(parent).unwrap_or_default() {
                out.push((depth, d.block_text(id).unwrap_or_default()));
                walk(d, TreeParentId::Node(id), depth + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(self, TreeParentId::Root, 0, &mut out);
        out
    }

    pub fn texts(&self) -> Vec<String> {
        self.outline().into_iter().map(|(_, t)| t).collect()
    }
}

// ---- relay ------------------------------------------------------------------------------------

/// Stand-in for the sync server: an append-only log of opaque blobs. It cannot decrypt anything
/// and de-duplicates pushes by content hash so retries are idempotent.
#[derive(Default)]
pub struct Relay {
    log: Vec<(String, Vec<u8>)>, // (device, sealed blob)
    seen: HashSet<[u8; 32]>,
    revoked: HashSet<String>,
    /// Number of upcoming requests that will fail, to simulate outages.
    pub fail_next: usize,
}

impl Relay {
    pub fn new() -> Self {
        Self::default()
    }

    fn gate(&mut self, device: &str) -> Result<()> {
        if self.fail_next > 0 {
            self.fail_next -= 1;
            return Err(SyncError::Unavailable);
        }
        if self.revoked.contains(device) {
            return Err(SyncError::Revoked);
        }
        Ok(())
    }

    pub fn revoke(&mut self, device: &str) {
        self.revoked.insert(device.to_string());
    }

    pub fn push(&mut self, device: &str, blob: Vec<u8>) -> Result<()> {
        self.gate(device)?;
        let key: [u8; 32] = Sha256::digest(&blob).into();
        if self.seen.insert(key) {
            self.log.push((device.to_string(), blob));
        }
        Ok(())
    }

    /// Blobs with sequence number >= `cursor`, plus the new cursor.
    pub fn pull(&mut self, device: &str, cursor: usize) -> Result<(Vec<Vec<u8>>, usize)> {
        self.gate(device)?;
        let blobs = self.log[cursor.min(self.log.len())..]
            .iter()
            .map(|(_, b)| b.clone())
            .collect();
        Ok((blobs, self.log.len()))
    }

    /// Everything the server stores, for tests asserting it never sees plaintext.
    pub fn stored_bytes(&self) -> Vec<u8> {
        self.log.iter().flat_map(|(_, b)| b.clone()).collect()
    }
}

// ---- replica ----------------------------------------------------------------------------------

pub struct Replica {
    pub device: String,
    pub page: PageDoc,
    key: WorkspaceKey,
    aad: Vec<u8>,
    cursor: usize,
    pushed: VersionVector,
}

impl Replica {
    pub fn new(device: &str, peer: u64, key: WorkspaceKey, page_id: &str) -> Self {
        let page = PageDoc::new(peer);
        Self {
            device: device.into(),
            page,
            key,
            aad: page_id.as_bytes().to_vec(),
            cursor: 0,
            pushed: VersionVector::default(),
        }
    }

    /// Simulates an app restart: only the snapshot plus sync bookkeeping survive.
    pub fn restart(&self, peer: u64) -> Result<Self> {
        Ok(Self {
            device: self.device.clone(),
            page: PageDoc::from_snapshot(peer, &self.page.snapshot()?)?,
            key: self.key.clone(),
            aad: self.aad.clone(),
            cursor: self.cursor,
            pushed: self.pushed.clone(),
        })
    }

    /// Pushes local changes, then pulls and applies remote ones. Safe to retry after any failure:
    /// the push cursor only advances once the relay accepted the blob.
    pub fn sync(&mut self, relay: &mut Relay) -> Result<()> {
        let updates = self.page.doc.export(ExportMode::updates(&self.pushed))?;
        let now = self.page.doc.oplog_vv();
        if now != self.pushed {
            let sealed = self.key.seal(Purpose::Content, &self.aad, &updates)?;
            relay.push(&self.device, sealed)?;
            self.pushed = now;
        }
        let (blobs, next) = relay.pull(&self.device, self.cursor)?;
        for b in blobs {
            let plain = self.key.open(Purpose::Content, &self.aad, &b)?;
            self.page.doc.import(&plain)?;
        }
        self.cursor = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
