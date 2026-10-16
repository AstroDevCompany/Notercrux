//! Attachment transfer: encrypted, chunked, resumable.
//!
//! The server only ever holds ciphertext chunks under a blob id derived from the file's content hash
//! with a workspace secret, so it can neither read a file nor tell whether two workspaces share one.
//! Each chunk carries its key epoch and is authenticated together with the blob id, its index and the
//! chunk count, so chunks cannot be reordered, dropped or moved between files. After the last chunk
//! arrives the plaintext hash is verified against the content id the block declared.

use crate::engine::{Engine, Session};
use crate::{Result, SyncError};
use notercrux_crypto::{chunk_aad, from_hex, Purpose};
use notercrux_protocol::{BlobStatus, CommitBlob};
use notercrux_storage::{AttachmentMeta, AttachmentState, ATTACHMENT_CHUNK};
use reqwest::Method;
use std::collections::HashSet;

/// Bytes a sealed chunk adds to its plaintext: epoch prefix (4) + envelope version (1) + nonce (24) + tag (16).
pub(crate) const CHUNK_OVERHEAD: usize = 4 + 1 + 24 + 16;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachmentReport {
    pub uploaded: usize,
    pub downloaded: usize,
    pub failed: usize,
}

impl AttachmentReport {
    pub fn moved(&self) -> usize {
        self.uploaded + self.downloaded
    }
}

fn blob_id(sess: &Session, id: &str) -> Result<String> {
    let bytes = from_hex(id).ok_or_else(|| SyncError::Protocol("invalid attachment id".into()))?;
    let hash: [u8; 32] = bytes
        .try_into()
        .map_err(|_| SyncError::Protocol("invalid attachment id".into()))?;
    Ok(sess.ring.blob_id(&hash)?)
}

impl Engine {
    /// Uploads attachments added here and downloads ones other devices added. Transient failures
    /// bubble up (to be retried with backoff); permanent ones park the attachment as `failed`.
    pub(crate) async fn sync_attachments(&self) -> Result<AttachmentReport> {
        let mut report = AttachmentReport::default();
        let work = self.with_store(|s| s.attachment_work())?;
        self.update_status(|s| {
            s.attachments_pending = (work.uploads.len() + work.downloads.len()) as i64;
            s.attachments_failed = work.failed as i64;
        });
        for meta in &work.uploads {
            match self.upload_one(meta).await {
                Ok(()) => report.uploaded += 1,
                Err(e) if e.is_transient() || e == SyncError::Revoked => return Err(e),
                Err(e) => {
                    self.park(meta, &e);
                    report.failed += 1;
                }
            }
        }
        for meta in &work.downloads {
            match self.download_one(meta).await {
                Ok(true) => report.downloaded += 1,
                Ok(false) => {}
                Err(e) if e.is_transient() || e == SyncError::Revoked => return Err(e),
                Err(e) => {
                    self.park(meta, &e);
                    report.failed += 1;
                }
            }
        }
        if report.moved() > 0 || report.failed > 0 {
            let work = self.with_store(|s| s.attachment_work())?;
            self.update_status(|s| {
                s.attachments_pending = (work.uploads.len() + work.downloads.len()) as i64;
                s.attachments_failed = work.failed as i64;
            });
        }
        if report.moved() > 0 {
            self.log(
                "info",
                format!(
                    "Attachments: {} sent, {} received.",
                    report.uploaded, report.downloaded
                ),
            );
        }
        Ok(report)
    }

    fn park(&self, meta: &AttachmentMeta, e: &SyncError) {
        self.log(
            "error",
            format!("Attachment \"{}\" could not be synced: {e}", meta.name),
        );
        let _ = self.with_store(|s| {
            s.attachment_mark(&meta.id, AttachmentState::Failed, Some(&e.to_string()))
        });
    }

    /// Puts failed attachments back in the queue and syncs again soon.
    pub fn retry_attachments(&self) -> Result<usize> {
        let n = self.with_store(|s| s.attachment_retry_failed())?;
        self.sync_soon();
        Ok(n)
    }

    async fn blob_status(&self, sess: &Session, base: &str) -> Result<Option<BlobStatus>> {
        match self
            .api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::GET,
                base,
                Vec::new(),
                None,
            )
            .await
        {
            Ok(r) => Ok(Some(r.json()?)),
            Err(SyncError::Server { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn upload_one(&self, meta: &AttachmentMeta) -> Result<()> {
        let sess = self.session()?;
        let ws = sess.account.workspace_id.clone();
        let blob = blob_id(&sess, &meta.id)?;
        let base = format!("/v1/workspaces/{ws}/blobs/{blob}");
        let status = self.blob_status(&sess, &base).await?;
        if status.as_ref().is_some_and(|s| s.complete) {
            return self.with_store(|s| s.attachment_mark(&meta.id, AttachmentState::Synced, None));
        }
        let present: HashSet<u32> = status
            .map(|s| s.present.into_iter().collect())
            .unwrap_or_default();
        let total = meta.chunks_total;
        let mut cipher_size = 0i64;
        for idx in 0..total {
            let plain_len = (meta.size as usize)
                .saturating_sub(idx as usize * ATTACHMENT_CHUNK)
                .min(ATTACHMENT_CHUNK);
            cipher_size += (plain_len + CHUNK_OVERHEAD) as i64;
            if present.contains(&idx) {
                continue; // an interrupted earlier attempt already stored it
            }
            let chunk = self
                .with_store(|s| s.attachment_chunk(&meta.id, idx))?
                .ok_or_else(|| SyncError::Storage("attachment data is incomplete".into()))?;
            let (epoch, sealed) = sess.ring.seal(
                Purpose::Attachment,
                |e| chunk_aad(&ws, &blob, idx, total, e),
                &chunk,
            )?;
            let mut body = epoch.to_be_bytes().to_vec();
            body.extend_from_slice(&sealed);
            let target = format!("{base}/chunks/{idx}");
            self.api
                .call(
                    &sess.account.server,
                    Some(&sess.keys),
                    Method::PUT,
                    &target,
                    body,
                    None,
                )
                .await?;
        }
        let commit = CommitBlob {
            total,
            epoch: sess.ring.current_epoch(),
            size: cipher_size,
        };
        let body = serde_json::to_vec(&commit).map_err(|e| SyncError::Protocol(e.to_string()))?;
        self.api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::POST,
                &format!("{base}/commit"),
                body,
                None,
            )
            .await?;
        self.with_store(|s| s.attachment_mark(&meta.id, AttachmentState::Synced, None))
    }

    /// `Ok(false)`: not on the server yet (the other device is still uploading).
    async fn download_one(&self, meta: &AttachmentMeta) -> Result<bool> {
        let sess = self.session()?;
        let ws = sess.account.workspace_id.clone();
        let blob = blob_id(&sess, &meta.id)?;
        let base = format!("/v1/workspaces/{ws}/blobs/{blob}");
        let Some(status) = self.blob_status(&sess, &base).await? else {
            return Ok(false);
        };
        if !status.complete {
            return Ok(false);
        }
        if status.total != meta.chunks_total {
            return Err(SyncError::Crypto(
                "the stored attachment does not match the block that references it".into(),
            ));
        }
        let have: HashSet<u32> = self
            .with_store(|s| s.attachment_present(&meta.id))?
            .into_iter()
            .collect();
        for idx in 0..status.total {
            if have.contains(&idx) {
                continue; // kept from an interrupted earlier download
            }
            let target = format!("{base}/chunks/{idx}");
            let reply = self
                .api
                .call(
                    &sess.account.server,
                    Some(&sess.keys),
                    Method::GET,
                    &target,
                    Vec::new(),
                    None,
                )
                .await?;
            let bytes = reply.body;
            let epoch = u32::from_be_bytes(
                bytes
                    .get(..4)
                    .and_then(|b| b.try_into().ok())
                    .ok_or_else(|| SyncError::Crypto("a downloaded chunk is truncated".into()))?,
            );
            let mut sess = self.session()?;
            if !sess.ring.has_epoch(epoch) {
                self.refresh_keys().await?;
                sess = self.session()?;
            }
            let plain = sess
                .ring
                .open(
                    epoch,
                    Purpose::Attachment,
                    &chunk_aad(&ws, &blob, idx, status.total, epoch),
                    &bytes[4..],
                )
                .map_err(|_| SyncError::Crypto("a downloaded chunk failed verification".into()))?;
            self.with_store(|s| s.attachment_put_chunk(&meta.id, idx, &plain))
                .map_err(|e| SyncError::Crypto(e.to_string()))?;
        }
        let ok = self.with_store(|s| s.attachment_finalize(&meta.id))?;
        if !ok {
            return Err(SyncError::Crypto(
                "the downloaded file failed its integrity check".into(),
            ));
        }
        Ok(true)
    }
}
