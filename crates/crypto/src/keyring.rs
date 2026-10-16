use crate::{hex, hkdf_expand, CryptoError, Purpose, Result, WorkspaceKey};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

const RING_VERSION: u8 = 1;

/// Workspace keys by epoch. Epoch 0 is created with the workspace; each revocation adds one.
/// Old epochs are kept so history stays readable; devices revoked at epoch N never receive N+1.
#[derive(Clone)]
pub struct KeyRing {
    keys: BTreeMap<u32, WorkspaceKey>,
}

impl KeyRing {
    /// A new ring with a fresh epoch 0.
    pub fn generate() -> Result<Self> {
        let mut keys = BTreeMap::new();
        keys.insert(0, WorkspaceKey::generate()?);
        Ok(Self { keys })
    }

    pub fn current_epoch(&self) -> u32 {
        *self
            .keys
            .keys()
            .next_back()
            .expect("a ring always has at least one key")
    }

    pub fn epochs(&self) -> Vec<u32> {
        self.keys.keys().copied().collect()
    }

    pub fn has_epoch(&self, epoch: u32) -> bool {
        self.keys.contains_key(&epoch)
    }

    pub fn key(&self, epoch: u32) -> Result<&WorkspaceKey> {
        self.keys
            .get(&epoch)
            .ok_or(CryptoError::UnknownEpoch(epoch))
    }

    /// Adds a new current epoch (used when a device is revoked). Returns its number.
    pub fn rotate(&mut self) -> Result<u32> {
        let next = self.current_epoch() + 1;
        self.keys.insert(next, WorkspaceKey::generate()?);
        Ok(next)
    }

    /// Seals with the current epoch. Returns `(epoch, sealed)`; the epoch travels in the clear next
    /// to the ciphertext so the reader knows which key to use.
    pub fn seal(
        &self,
        purpose: Purpose,
        aad_for_epoch: impl Fn(u32) -> Vec<u8>,
        plaintext: &[u8],
    ) -> Result<(u32, Vec<u8>)> {
        let epoch = self.current_epoch();
        let sealed = self
            .key(epoch)?
            .seal(purpose, &aad_for_epoch(epoch), plaintext)?;
        Ok((epoch, sealed))
    }

    pub fn open(&self, epoch: u32, purpose: Purpose, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        self.key(epoch)?.open(purpose, aad, sealed)
    }

    /// Opaque, stable document id for a page (or other local id). The server sees only this, so it
    /// learns neither page ids (which embed creation time) nor how documents relate to pages.
    /// Derived from epoch 0, which every device that holds a ring has.
    /// True when `newer` contains every key of this ring unchanged (and possibly more). A key ring
    /// delivered by someone else is only accepted if it proves its author already knew all the keys
    /// we have, so a server cannot substitute a ring it made up.
    pub fn is_extended_by(&self, newer: &KeyRing) -> bool {
        self.epochs()
            .into_iter()
            .all(|e| match (self.key(e), newer.key(e)) {
                (Ok(a), Ok(b)) => crate::constant_time_eq(a.expose(), b.expose()),
                _ => false,
            })
    }

    pub fn doc_id(&self, local_id: &str) -> Result<String> {
        let k = self.key(0)?.derive_docid_key();
        let mut h = Sha256::new();
        h.update(k);
        h.update(b"doc|");
        h.update(local_id.as_bytes());
        Ok(hex(&h.finalize()[..16]))
    }

    /// Opaque blob id for attachment content (keyed hash of the plaintext hash, so the server cannot
    /// confirm that a known file is stored).
    pub fn blob_id(&self, plaintext_sha256: &[u8; 32]) -> Result<String> {
        let k = self.key(0)?.derive_docid_key();
        Ok(hex(&hkdf_expand(
            &[&k[..], plaintext_sha256].concat(),
            b"notercrux/v1/blobid",
        )[..16]))
    }

    /// Serialized form, only ever stored sealed (wrapped for a device, or under the recovery key).
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut out = vec![RING_VERSION];
        out.extend_from_slice(&(self.keys.len() as u16).to_be_bytes());
        for (epoch, key) in &self.keys {
            out.extend_from_slice(&epoch.to_be_bytes());
            out.extend_from_slice(key.expose());
        }
        Zeroizing::new(out)
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        let (&version, rest) = b.split_first().ok_or(CryptoError::Malformed)?;
        if version != RING_VERSION || rest.len() < 2 {
            return Err(CryptoError::Malformed);
        }
        let n = u16::from_be_bytes([rest[0], rest[1]]) as usize;
        let body = &rest[2..];
        if n == 0 || body.len() != n * 36 {
            return Err(CryptoError::Malformed);
        }
        let mut keys = BTreeMap::new();
        let (entries, _) = body.as_chunks::<36>();
        for chunk in entries {
            let epoch = u32::from_be_bytes(chunk[..4].try_into().expect("4 bytes"));
            let key: [u8; 32] = chunk[4..].try_into().expect("32 bytes");
            keys.insert(epoch, WorkspaceKey::from_bytes(key));
        }
        if keys.len() != n || !keys.contains_key(&0) {
            return Err(CryptoError::Malformed);
        }
        Ok(Self { keys })
    }
}

/// Associated data for sealed document updates: binds a ciphertext to its workspace, document and epoch,
/// so it cannot be replayed into another document or under another epoch.
pub fn content_aad(workspace_id: &str, doc_id: &str, epoch: u32) -> Vec<u8> {
    format!("NCX-content|{workspace_id}|{doc_id}|{epoch}").into_bytes()
}

/// Associated data for one sealed attachment chunk.
pub fn chunk_aad(workspace_id: &str, blob_id: &str, index: u32, total: u32, epoch: u32) -> Vec<u8> {
    format!("NCX-chunk|{workspace_id}|{blob_id}|{index}|{total}|{epoch}").into_bytes()
}
