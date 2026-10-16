//! Attachments: image and file blocks reference content stored here by its SHA-256.
//!
//! The bytes live in the local database in fixed-size chunks. The sync engine uploads chunks of
//! `local` attachments and downloads chunks of `missing` ones; this module only stores and verifies.

use crate::{now_ms, Result, Store, StoreError};
use notercrux_domain::{Block, BlockKind};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Plaintext chunk size. Uploads and downloads move one chunk at a time.
pub const ATTACHMENT_CHUNK: usize = 1024 * 1024;
/// Largest attachment this app accepts.
pub const MAX_ATTACHMENT_BYTES: usize = 100 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentState {
    Local,
    Synced,
    Missing,
    Failed,
}

impl AttachmentState {
    fn as_str(self) -> &'static str {
        match self {
            AttachmentState::Local => "local",
            AttachmentState::Synced => "synced",
            AttachmentState::Missing => "missing",
            AttachmentState::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "synced" => AttachmentState::Synced,
            "missing" => AttachmentState::Missing,
            "failed" => AttachmentState::Failed,
            _ => AttachmentState::Local,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMeta {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub state: AttachmentState,
    pub error: Option<String>,
    /// Chunks stored so far / chunks the whole file needs.
    pub chunks_present: u32,
    pub chunks_total: u32,
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentWork {
    pub uploads: Vec<AttachmentMeta>,
    pub downloads: Vec<AttachmentMeta>,
    pub failed: usize,
}

pub fn chunk_count(size: i64) -> u32 {
    (size.max(0) as usize).div_ceil(ATTACHMENT_CHUNK).max(1) as u32
}

fn expected_chunk_len(size: i64, idx: u32) -> usize {
    let start = idx as usize * ATTACHMENT_CHUNK;
    (size.max(0) as usize)
        .saturating_sub(start)
        .min(ATTACHMENT_CHUNK)
}

pub fn content_id(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn is_content_id(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let base: String = base.chars().filter(|c| !c.is_control()).take(200).collect();
    if base.is_empty() {
        "attachment".into()
    } else {
        base
    }
}

fn clean_mime(mime: &str) -> String {
    let m: String = mime
        .trim()
        .chars()
        .filter(|c| c.is_ascii_graphic())
        .take(100)
        .collect();
    if m.contains('/') {
        m.to_ascii_lowercase()
    } else {
        "application/octet-stream".into()
    }
}

fn meta_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentMeta> {
    let size: i64 = r.get("size")?;
    let present: i64 = r.get("present")?;
    Ok(AttachmentMeta {
        id: r.get("id")?,
        name: r.get("name")?,
        mime: r.get("mime")?,
        size,
        state: AttachmentState::parse(&r.get::<_, String>("state")?),
        error: r.get("error")?,
        chunks_present: present as u32,
        chunks_total: chunk_count(size),
    })
}

const META_SQL: &str = "SELECT a.id, a.name, a.mime, a.size, a.state, a.error, \
     (SELECT COUNT(*) FROM attachment_chunks c WHERE c.id = a.id) AS present FROM attachments a";

/// Records every attachment a document refers to (as `missing` unless already known), so the sync
/// engine knows what to download. Runs whenever a document is written, local or remote.
pub(crate) fn register_references(tx: &Transaction<'_>, blocks: &[Block]) -> Result<()> {
    for b in blocks {
        if matches!(b.kind, BlockKind::Image | BlockKind::File) {
            let id = b.attrs.get("attachment").and_then(|v| v.as_str());
            if let Some(id) = id.filter(|id| is_content_id(id)) {
                let size = b.attrs.get("size").and_then(|v| v.as_i64()).unwrap_or(0);
                if (0..=MAX_ATTACHMENT_BYTES as i64).contains(&size) {
                    let name =
                        clean_name(b.attrs.get("name").and_then(|v| v.as_str()).unwrap_or(""));
                    let mime =
                        clean_mime(b.attrs.get("mime").and_then(|v| v.as_str()).unwrap_or(""));
                    tx.execute(
                        "INSERT OR IGNORE INTO attachments (id, name, mime, size, state, created_at) VALUES (?1, ?2, ?3, ?4, 'missing', ?5)",
                        params![id, name, mime, size, now_ms()],
                    )?;
                }
            }
        }
        register_references(tx, &b.children)?;
    }
    Ok(())
}

impl Store {
    /// Stores a file. Returns the existing record when the same bytes are already stored.
    pub fn attachment_add(
        &mut self,
        name: &str,
        mime: &str,
        data: &[u8],
    ) -> Result<AttachmentMeta> {
        if data.is_empty() {
            return Err(StoreError::Invalid("the file is empty".into()));
        }
        if data.len() > MAX_ATTACHMENT_BYTES {
            return Err(StoreError::Invalid(format!(
                "files larger than {} MB are not supported",
                MAX_ATTACHMENT_BYTES / (1024 * 1024)
            )));
        }
        let id = content_id(data);
        let tx = self.conn.transaction()?;
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT state, size FROM attachments WHERE id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((state, _)) if state != "missing" && state != "failed" => {}
            _ => {
                let name = clean_name(name);
                let mime = clean_mime(mime);
                tx.execute(
                    "INSERT INTO attachments (id, name, mime, size, state, created_at) VALUES (?1, ?2, ?3, ?4, 'local', ?5)
                     ON CONFLICT(id) DO UPDATE SET state = 'local', error = NULL, size = ?4",
                    params![id, name, mime, data.len() as i64, now_ms()],
                )?;
                tx.execute("DELETE FROM attachment_chunks WHERE id = ?1", [&id])?;
                for (i, chunk) in data.chunks(ATTACHMENT_CHUNK).enumerate() {
                    tx.execute(
                        "INSERT INTO attachment_chunks (id, idx, data) VALUES (?1, ?2, ?3)",
                        params![id, i as i64, chunk],
                    )?;
                }
            }
        }
        let meta = tx.query_row(&format!("{META_SQL} WHERE a.id = ?1"), [&id], meta_from)?;
        tx.commit()?;
        Ok(meta)
    }

    pub fn attachment(&self, id: &str) -> Result<Option<AttachmentMeta>> {
        Ok(self
            .conn
            .query_row(&format!("{META_SQL} WHERE a.id = ?1"), [id], meta_from)
            .optional()?)
    }

    /// The whole file, or `None` when it has not been fully stored.
    pub fn attachment_data(&self, id: &str) -> Result<Option<Vec<u8>>> {
        let Some(meta) = self.attachment(id)? else {
            return Ok(None);
        };
        if meta.chunks_present < meta.chunks_total {
            return Ok(None);
        }
        let mut st = self
            .conn
            .prepare("SELECT data FROM attachment_chunks WHERE id = ?1 ORDER BY idx")?;
        let mut out = Vec::with_capacity(meta.size.max(0) as usize);
        for chunk in st.query_map([id], |r| r.get::<_, Vec<u8>>(0))? {
            out.extend_from_slice(&chunk?);
        }
        Ok(Some(out))
    }

    pub fn attachment_chunk(&self, id: &str, idx: u32) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row(
                "SELECT data FROM attachment_chunks WHERE id = ?1 AND idx = ?2",
                params![id, idx],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Indexes of the chunks already stored.
    pub fn attachment_present(&self, id: &str) -> Result<Vec<u32>> {
        let mut st = self
            .conn
            .prepare("SELECT idx FROM attachment_chunks WHERE id = ?1 ORDER BY idx")?;
        let rows = st.query_map([id], |r| r.get::<_, u32>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// What the sync engine has to move.
    pub fn attachment_work(&self) -> Result<AttachmentWork> {
        let mut st = self
            .conn
            .prepare(&format!("{META_SQL} ORDER BY a.created_at, a.id"))?;
        let all: Vec<AttachmentMeta> = st
            .query_map([], meta_from)?
            .collect::<rusqlite::Result<_>>()?;
        let mut work = AttachmentWork::default();
        for m in all {
            match m.state {
                AttachmentState::Local => work.uploads.push(m),
                AttachmentState::Missing => work.downloads.push(m),
                AttachmentState::Failed => work.failed += 1,
                AttachmentState::Synced => {}
            }
        }
        Ok(work)
    }

    /// Stores one downloaded chunk of a `missing` attachment. Wrong sizes are refused so a hostile
    /// server cannot make the file larger than the block declared.
    pub fn attachment_put_chunk(&mut self, id: &str, idx: u32, data: &[u8]) -> Result<()> {
        let Some(meta) = self.attachment(id)? else {
            return Err(StoreError::Invalid("unknown attachment".into()));
        };
        if meta.state != AttachmentState::Missing {
            return Ok(());
        }
        if idx >= meta.chunks_total || data.len() != expected_chunk_len(meta.size, idx) {
            return Err(StoreError::Invalid(
                "the downloaded chunk does not match the attachment".into(),
            ));
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO attachment_chunks (id, idx, data) VALUES (?1, ?2, ?3)",
            params![id, idx, data],
        )?;
        Ok(())
    }

    /// Checks a fully downloaded attachment against its content hash. On success it becomes `synced`;
    /// on a mismatch the chunks are discarded and it is marked `failed`.
    pub fn attachment_finalize(&mut self, id: &str) -> Result<bool> {
        let Some(meta) = self.attachment(id)? else {
            return Ok(false);
        };
        if meta.chunks_present < meta.chunks_total {
            return Ok(false);
        }
        let ok = {
            let mut hasher = Sha256::new();
            let mut st = self
                .conn
                .prepare("SELECT data FROM attachment_chunks WHERE id = ?1 ORDER BY idx")?;
            for chunk in st.query_map([id], |r| r.get::<_, Vec<u8>>(0))? {
                hasher.update(chunk?);
            }
            hex(&hasher.finalize()) == id
        };
        if ok {
            self.attachment_mark(id, AttachmentState::Synced, None)?;
        } else {
            self.conn
                .execute("DELETE FROM attachment_chunks WHERE id = ?1", [id])?;
            self.attachment_mark(
                id,
                AttachmentState::Failed,
                Some("the downloaded file failed its integrity check"),
            )?;
        }
        Ok(ok)
    }

    pub fn attachment_mark(
        &self,
        id: &str,
        state: AttachmentState,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE attachments SET state = ?2, error = ?3 WHERE id = ?1",
            params![id, state.as_str(), error],
        )?;
        Ok(())
    }

    /// Puts failed attachments back in the queue. Returns how many.
    pub fn attachment_retry_failed(&mut self) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let n = tx.execute(
            "UPDATE attachments SET state = CASE WHEN (SELECT COUNT(*) FROM attachment_chunks c WHERE c.id = attachments.id) >= ((size + ?1 - 1) / ?1) AND size > 0 THEN 'local' ELSE 'missing' END, error = NULL WHERE state = 'failed'",
            [ATTACHMENT_CHUNK as i64],
        )?;
        tx.commit()?;
        Ok(n)
    }

    pub(crate) fn attachments_wipe(tx: &Transaction<'_>) -> Result<()> {
        tx.execute("DELETE FROM attachment_chunks", [])?;
        tx.execute("DELETE FROM attachments", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notercrux_domain::{Block, BlockKind, Document};

    fn image_block(id: &str, size: i64) -> Block {
        let mut b = Block::new(BlockKind::Image, "caption");
        b.attrs.insert("attachment".into(), id.into());
        b.attrs.insert("name".into(), "../../etc/pic.png".into());
        b.attrs.insert("mime".into(), "IMAGE/PNG".into());
        b.attrs.insert("size".into(), size.into());
        b
    }

    #[test]
    fn files_are_stored_chunked_deduplicated_and_sanitized() {
        let mut s = Store::open_in_memory().unwrap();
        let data: Vec<u8> = (0..ATTACHMENT_CHUNK * 2 + 17)
            .map(|i| (i % 251) as u8)
            .collect();
        let m = s
            .attachment_add("../../evil/name.bin", "Application/X-Thing", &data)
            .unwrap();
        assert_eq!(m.name, "name.bin");
        assert_eq!(m.mime, "application/x-thing");
        assert_eq!((m.chunks_present, m.chunks_total), (3, 3));
        assert_eq!(m.state, AttachmentState::Local);
        assert_eq!(m.id, content_id(&data));
        assert_eq!(s.attachment_data(&m.id).unwrap().unwrap(), data);
        assert_eq!(s.attachment_chunk(&m.id, 2).unwrap().unwrap().len(), 17);
        // The same bytes again do not duplicate anything.
        let again = s.attachment_add("other.bin", "text/plain", &data).unwrap();
        assert_eq!(again, m);
        assert!(s.attachment_add("x", "x", &[]).is_err());
        assert!(s
            .attachment_add("big", "x/y", &vec![0u8; MAX_ATTACHMENT_BYTES + 1])
            .is_err());
    }

    #[test]
    fn documents_register_references_that_wait_for_their_data() {
        let mut s = Store::open_in_memory().unwrap();
        let page = s.create_page(None, "P").unwrap();
        let id = content_id(b"hello");
        let mut doc = Document::default();
        doc.blocks.push(image_block(&id, 5));
        doc.blocks.push(image_block("not-a-hash", 5));
        doc.blocks.push(image_block(
            &content_id(b"huge"),
            MAX_ATTACHMENT_BYTES as i64 + 1,
        ));
        s.save_document(page.id, &doc).unwrap();
        let m = s.attachment(&id).unwrap().unwrap();
        assert_eq!(
            (m.state, m.name.as_str(), m.mime.as_str()),
            (AttachmentState::Missing, "pic.png", "image/png")
        );
        assert_eq!(s.attachment_data(&id).unwrap(), None);
        assert!(
            s.attachment("not-a-hash").unwrap().is_none(),
            "malformed ids are ignored"
        );
        assert!(
            s.attachment(&content_id(b"huge")).unwrap().is_none(),
            "oversized declarations are ignored"
        );
        let work = s.attachment_work().unwrap();
        assert_eq!((work.downloads.len(), work.uploads.len()), (1, 0));

        // Providing the data locally (e.g. the same file is added here) completes it.
        let m = s
            .attachment_add("hello.txt", "text/plain", b"hello")
            .unwrap();
        assert_eq!(m.state, AttachmentState::Local);
        assert_eq!(s.attachment_data(&id).unwrap().unwrap(), b"hello");
    }

    #[test]
    fn downloads_are_verified_chunk_by_chunk_and_end_to_end() {
        let mut s = Store::open_in_memory().unwrap();
        let page = s.create_page(None, "P").unwrap();
        let data: Vec<u8> = (0..ATTACHMENT_CHUNK + 100)
            .map(|i| (i * 7 % 256) as u8)
            .collect();
        let id = content_id(&data);
        let mut doc = Document::default();
        doc.blocks.push(image_block(&id, data.len() as i64));
        s.save_document(page.id, &doc).unwrap();

        // Wrong sizes and indexes are refused.
        assert!(s.attachment_put_chunk(&id, 0, &data[..10]).is_err());
        assert!(s.attachment_put_chunk(&id, 2, &data[..100]).is_err());
        assert!(s.attachment_put_chunk("unknown", 0, b"x").is_err());
        s.attachment_put_chunk(&id, 0, &data[..ATTACHMENT_CHUNK])
            .unwrap();
        assert_eq!(s.attachment_present(&id).unwrap(), vec![0]);
        assert!(!s.attachment_finalize(&id).unwrap(), "incomplete");
        // A chunk with the right size but wrong content is caught by the hash.
        s.attachment_put_chunk(&id, 1, &[9u8; 100]).unwrap();
        assert!(!s.attachment_finalize(&id).unwrap());
        let m = s.attachment(&id).unwrap().unwrap();
        assert_eq!(m.state, AttachmentState::Failed);
        assert_eq!(m.chunks_present, 0, "tampered data is discarded");
        // Retry puts it back in the queue, and the right data then verifies.
        assert_eq!(s.attachment_retry_failed().unwrap(), 1);
        assert_eq!(
            s.attachment(&id).unwrap().unwrap().state,
            AttachmentState::Missing
        );
        s.attachment_put_chunk(&id, 0, &data[..ATTACHMENT_CHUNK])
            .unwrap();
        s.attachment_put_chunk(&id, 1, &data[ATTACHMENT_CHUNK..])
            .unwrap();
        assert!(s.attachment_finalize(&id).unwrap());
        assert_eq!(
            s.attachment(&id).unwrap().unwrap().state,
            AttachmentState::Synced
        );
        assert_eq!(s.attachment_data(&id).unwrap().unwrap(), data);
    }

    #[test]
    fn chunk_arithmetic() {
        assert_eq!(chunk_count(0), 1);
        assert_eq!(chunk_count(1), 1);
        assert_eq!(chunk_count(ATTACHMENT_CHUNK as i64), 1);
        assert_eq!(chunk_count(ATTACHMENT_CHUNK as i64 + 1), 2);
        assert_eq!(expected_chunk_len(ATTACHMENT_CHUNK as i64 + 5, 1), 5);
        assert!(is_content_id(&content_id(b"x")) && !is_content_id("ABC"));
    }
}
