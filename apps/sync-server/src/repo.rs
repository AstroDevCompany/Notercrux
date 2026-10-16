//! Persistence interface. Two implementations: in-memory (tests, quick self-hosting trials) and
//! PostgreSQL (real deployments). Both must behave identically; `conformance` in `tests` runs the
//! same scenarios against each.

use async_trait::async_trait;
use notercrux_protocol::{
    ApproveInvite, CommitBlob, CreateWorkspace, DeviceInfo, InviteRequest, InviteResult, Limits,
    PullResponse, PushOp, PushResponse, RevokeDevice, Usage,
};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(&'static str),
    #[error("{0}")]
    Quota(&'static str),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("backend failure: {0}")]
    Backend(String),
}

pub type RepoResult<T> = Result<T, RepoError>;

#[derive(Clone, Debug)]
pub struct WorkspaceRow {
    pub id: String,
    pub epoch: u32,
    pub head: i64,
    pub recovery_key: Vec<u8>,
    pub recovery_wrap: Vec<u8>,
    pub usage: Usage,
}

#[derive(Clone, Debug)]
pub struct InviteRow {
    pub id: String,
    pub workspace_id: String,
    pub created_by: String,
    pub expires_at: i64,
    pub request: Option<InviteRequest>,
    pub request_device: Option<String>,
    pub approved_device: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BlobMeta {
    pub complete: bool,
    pub total: u32,
    pub epoch: u32,
    pub size: i64,
    /// (chunk index, ciphertext size)
    pub chunks: Vec<(u32, i64)>,
}

#[async_trait]
pub trait Repo: Send + Sync {
    async fn create_workspace(
        &self,
        req: &CreateWorkspace,
        device_id: &str,
        now: i64,
    ) -> RepoResult<()>;
    async fn delete_workspace(&self, ws: &str) -> RepoResult<()>;
    async fn workspace(&self, ws: &str) -> RepoResult<Option<WorkspaceRow>>;

    async fn device(&self, ws: &str, id: &str) -> RepoResult<Option<DeviceInfo>>;
    async fn devices(&self, ws: &str) -> RepoResult<Vec<DeviceInfo>>;
    async fn touch_device(&self, ws: &str, id: &str, now: i64) -> RepoResult<()>;
    /// Records a request nonce; `false` means it was already seen (a replay).
    async fn record_nonce(
        &self,
        ws: &str,
        device: &str,
        nonce: &str,
        now: i64,
        ttl: i64,
    ) -> RepoResult<bool>;

    async fn create_invite(
        &self,
        ws: &str,
        id: &str,
        by: &str,
        expires_at: i64,
        now: i64,
    ) -> RepoResult<()>;
    async fn invite(&self, id: &str) -> RepoResult<Option<InviteRow>>;
    async fn submit_invite_request(
        &self,
        id: &str,
        req: &InviteRequest,
        device_id: &str,
        now: i64,
    ) -> RepoResult<()>;
    async fn approve_invite(
        &self,
        ws: &str,
        id: &str,
        a: &ApproveInvite,
        now: i64,
        max_devices: i64,
    ) -> RepoResult<u32>;
    async fn invite_result(&self, id: &str, device_id: &str) -> RepoResult<Option<InviteResult>>;

    async fn revoke_device(
        &self,
        ws: &str,
        device_id: &str,
        r: &RevokeDevice,
        now: i64,
    ) -> RepoResult<()>;
    /// The newest key wrap stored for a device: (epoch, wrap).
    async fn key_wrap(&self, ws: &str, device_id: &str) -> RepoResult<Option<(u32, Vec<u8>)>>;
    /// Registers a device authorized by the recovery key. Returns (epoch, recovery wrap).
    async fn recover_device(
        &self,
        ws: &str,
        device_key: &[u8],
        device_id: &str,
        label_sealed: &[u8],
        now: i64,
        max_devices: i64,
    ) -> RepoResult<(u32, Vec<u8>)>;

    async fn push_ops(
        &self,
        ws: &str,
        device_id: &str,
        ops: &[PushOp],
        now: i64,
        limits: &Limits,
    ) -> RepoResult<PushResponse>;
    async fn pull_ops(&self, ws: &str, after: i64, limit: i64) -> RepoResult<PullResponse>;

    async fn blob(&self, ws: &str, blob_id: &str) -> RepoResult<Option<BlobMeta>>;
    /// Reserves a chunk (quota check, metadata). Bytes are stored by the caller afterwards.
    async fn reserve_chunk(
        &self,
        ws: &str,
        blob_id: &str,
        index: u32,
        size: i64,
        now: i64,
        limits: &Limits,
    ) -> RepoResult<()>;
    async fn remove_chunk(&self, ws: &str, blob_id: &str, index: u32) -> RepoResult<()>;
    async fn commit_blob(&self, ws: &str, blob_id: &str, c: &CommitBlob) -> RepoResult<()>;

    /// Drops expired invites and nonces.
    async fn purge_expired(&self, now: i64) -> RepoResult<()>;
}

/// The wraps in a revocation must cover exactly the devices that remain.
pub fn check_revoke(
    ws: &WorkspaceRow,
    devices: &[DeviceInfo],
    target: &str,
    r: &RevokeDevice,
) -> RepoResult<()> {
    let Some(t) = devices.iter().find(|d| d.id == target) else {
        return Err(RepoError::NotFound);
    };
    if t.revoked_at.is_some() {
        return Err(RepoError::Conflict("device already revoked"));
    }
    if r.new_epoch != ws.epoch + 1 {
        return Err(RepoError::Conflict("stale key epoch"));
    }
    let remaining: HashSet<&str> = devices
        .iter()
        .filter(|d| d.revoked_at.is_none() && d.id != target)
        .map(|d| d.id.as_str())
        .collect();
    if remaining.is_empty() {
        return Err(RepoError::Conflict("cannot revoke the last device"));
    }
    let given: HashSet<&str> = r.wraps.iter().map(|w| w.device_id.as_str()).collect();
    if given.len() != r.wraps.len() || given != remaining {
        return Err(RepoError::Invalid(
            "key wraps must cover exactly the remaining devices",
        ));
    }
    if r.wraps.iter().any(|w| w.key_wrap.is_empty()) || r.recovery_wrap.is_empty() {
        return Err(RepoError::Invalid("empty key wrap"));
    }
    Ok(())
}

pub fn check_op(op: &PushOp, epoch: u32) -> RepoResult<()> {
    if op.epoch > epoch {
        return Err(RepoError::Conflict("operation uses an unknown key epoch"));
    }
    Ok(())
}
