//! In-memory repository. Data is lost on restart; meant for tests and quick trials.

use crate::repo::*;
use async_trait::async_trait;
use notercrux_protocol::{
    ApproveInvite, CommitBlob, CreateWorkspace, DeviceInfo, InviteRequest, InviteResult, Limits,
    PullResponse, PulledOp, PushOp, PushResponse, RevokeDevice, Usage,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

struct OpRow {
    seq: i64,
    op_id: String,
    device_id: String,
    doc_id: String,
    epoch: u32,
    format: u32,
    payload: Vec<u8>,
}

struct BlobEntry {
    epoch: u32,
    total: u32,
    size: i64,
    complete: bool,
    chunks: BTreeMap<u32, i64>,
}

struct Ws {
    epoch: u32,
    recovery_key: Vec<u8>,
    recovery_wrap: Vec<u8>,
    devices: Vec<DeviceInfo>,
    /// (device, epoch) -> wrap
    wraps: HashMap<(String, u32), Vec<u8>>,
    ops: Vec<OpRow>,
    op_ids: HashSet<String>,
    op_bytes: i64,
    blobs: HashMap<String, BlobEntry>,
    blob_bytes: i64,
}

#[derive(Default)]
struct State {
    workspaces: HashMap<String, Ws>,
    invites: HashMap<String, InviteRow>,
    nonces: HashMap<(String, String, String), i64>,
}

#[derive(Default)]
pub struct MemRepo {
    state: Mutex<State>,
}

impl MemRepo {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn usage(w: &Ws) -> Usage {
    Usage {
        op_bytes: w.op_bytes,
        op_count: w.ops.len() as i64,
        blob_bytes: w.blob_bytes,
        devices: w.devices.iter().filter(|d| d.revoked_at.is_none()).count() as i64,
    }
}

fn row(id: &str, w: &Ws) -> WorkspaceRow {
    WorkspaceRow {
        id: id.to_string(),
        epoch: w.epoch,
        head: w.ops.last().map_or(0, |o| o.seq),
        recovery_key: w.recovery_key.clone(),
        recovery_wrap: w.recovery_wrap.clone(),
        usage: usage(w),
    }
}

#[async_trait]
impl Repo for MemRepo {
    async fn create_workspace(
        &self,
        req: &CreateWorkspace,
        device_id: &str,
        now: i64,
    ) -> RepoResult<()> {
        let mut s = self.lock();
        if s.workspaces.contains_key(&req.workspace_id) {
            return Err(RepoError::Conflict("workspace already exists"));
        }
        s.workspaces.insert(
            req.workspace_id.clone(),
            Ws {
                epoch: 0,
                recovery_key: req.recovery_key.clone(),
                recovery_wrap: req.recovery_wrap.clone(),
                devices: vec![DeviceInfo {
                    id: device_id.to_string(),
                    key: req.device_key.clone(),
                    label_sealed: req.label_sealed.clone(),
                    created_at: now,
                    revoked_at: None,
                    last_seen: Some(now),
                }],
                wraps: HashMap::new(),
                ops: Vec::new(),
                op_ids: HashSet::new(),
                op_bytes: 0,
                blobs: HashMap::new(),
                blob_bytes: 0,
            },
        );
        Ok(())
    }

    async fn delete_workspace(&self, ws: &str) -> RepoResult<()> {
        let mut s = self.lock();
        s.workspaces.remove(ws).ok_or(RepoError::NotFound)?;
        s.invites.retain(|_, i| i.workspace_id != ws);
        s.nonces.retain(|(w, _, _), _| w != ws);
        Ok(())
    }

    async fn workspace(&self, ws: &str) -> RepoResult<Option<WorkspaceRow>> {
        Ok(self.lock().workspaces.get(ws).map(|w| row(ws, w)))
    }

    async fn device(&self, ws: &str, id: &str) -> RepoResult<Option<DeviceInfo>> {
        Ok(self
            .lock()
            .workspaces
            .get(ws)
            .and_then(|w| w.devices.iter().find(|d| d.id == id).cloned()))
    }

    async fn devices(&self, ws: &str) -> RepoResult<Vec<DeviceInfo>> {
        Ok(self
            .lock()
            .workspaces
            .get(ws)
            .map(|w| w.devices.clone())
            .unwrap_or_default())
    }

    async fn touch_device(&self, ws: &str, id: &str, now: i64) -> RepoResult<()> {
        if let Some(d) = self
            .lock()
            .workspaces
            .get_mut(ws)
            .and_then(|w| w.devices.iter_mut().find(|d| d.id == id))
        {
            d.last_seen = Some(now);
        }
        Ok(())
    }

    async fn record_nonce(
        &self,
        ws: &str,
        device: &str,
        nonce: &str,
        now: i64,
        ttl: i64,
    ) -> RepoResult<bool> {
        let mut s = self.lock();
        let key = (ws.to_string(), device.to_string(), nonce.to_string());
        if s.nonces.get(&key).is_some_and(|exp| *exp > now) {
            return Ok(false);
        }
        s.nonces.insert(key, now + ttl);
        Ok(true)
    }

    async fn create_invite(
        &self,
        ws: &str,
        id: &str,
        by: &str,
        expires_at: i64,
        _now: i64,
    ) -> RepoResult<()> {
        let mut s = self.lock();
        if !s.workspaces.contains_key(ws) {
            return Err(RepoError::NotFound);
        }
        if s.invites.contains_key(id) {
            return Err(RepoError::Conflict("invite id in use"));
        }
        s.invites.insert(
            id.to_string(),
            InviteRow {
                id: id.to_string(),
                workspace_id: ws.to_string(),
                created_by: by.to_string(),
                expires_at,
                request: None,
                request_device: None,
                approved_device: None,
            },
        );
        Ok(())
    }

    async fn invite(&self, id: &str) -> RepoResult<Option<InviteRow>> {
        Ok(self.lock().invites.get(id).cloned())
    }

    async fn submit_invite_request(
        &self,
        id: &str,
        req: &InviteRequest,
        device_id: &str,
        now: i64,
    ) -> RepoResult<()> {
        let mut s = self.lock();
        let inv = s.invites.get_mut(id).ok_or(RepoError::NotFound)?;
        if inv.workspace_id != req.workspace_id {
            return Err(RepoError::NotFound);
        }
        if inv.expires_at <= now {
            return Err(RepoError::Conflict("invite expired"));
        }
        if inv.approved_device.is_some() {
            return Err(RepoError::Conflict("invite already used"));
        }
        inv.request = Some(req.clone());
        inv.request_device = Some(device_id.to_string());
        Ok(())
    }

    async fn approve_invite(
        &self,
        ws: &str,
        id: &str,
        a: &ApproveInvite,
        now: i64,
        max_devices: i64,
    ) -> RepoResult<u32> {
        let mut s = self.lock();
        let inv = s.invites.get(id).cloned().ok_or(RepoError::NotFound)?;
        if inv.workspace_id != ws {
            return Err(RepoError::NotFound);
        }
        if inv.expires_at <= now {
            return Err(RepoError::Conflict("invite expired"));
        }
        if inv.approved_device.is_some() {
            return Err(RepoError::Conflict("invite already used"));
        }
        let (Some(req), Some(dev)) = (inv.request, inv.request_device) else {
            return Err(RepoError::Conflict("no device has requested to join"));
        };
        if dev != a.device_id {
            return Err(RepoError::Conflict("the requesting device changed"));
        }
        if a.key_wrap.is_empty() || a.label_sealed.is_empty() {
            return Err(RepoError::Invalid("empty key wrap"));
        }
        let w = s.workspaces.get_mut(ws).ok_or(RepoError::NotFound)?;
        if w.devices.iter().any(|d| d.id == dev) {
            return Err(RepoError::Conflict("device already enrolled"));
        }
        if usage(w).devices >= max_devices {
            return Err(RepoError::Quota("device limit reached"));
        }
        w.devices.push(DeviceInfo {
            id: dev.clone(),
            key: req.device_key,
            label_sealed: a.label_sealed.clone(),
            created_at: now,
            revoked_at: None,
            last_seen: None,
        });
        let epoch = w.epoch;
        w.wraps.insert((dev.clone(), epoch), a.key_wrap.clone());
        s.invites.get_mut(id).expect("present").approved_device = Some(dev);
        Ok(epoch)
    }

    async fn invite_result(&self, id: &str, device_id: &str) -> RepoResult<Option<InviteResult>> {
        let s = self.lock();
        let Some(inv) = s.invites.get(id) else {
            return Ok(None);
        };
        if inv.request_device.as_deref() != Some(device_id) {
            return Ok(None);
        }
        if inv.approved_device.is_none() {
            return Ok(Some(InviteResult {
                approved: false,
                epoch: 0,
                key_wrap: None,
            }));
        }
        let w = s
            .workspaces
            .get(&inv.workspace_id)
            .ok_or(RepoError::NotFound)?;
        let best = w
            .wraps
            .iter()
            .filter(|((d, _), _)| d == device_id)
            .max_by_key(|((_, e), _)| *e);
        Ok(Some(InviteResult {
            approved: true,
            epoch: best.map_or(0, |((_, e), _)| *e),
            key_wrap: best.map(|(_, wrap)| wrap.clone()),
        }))
    }

    async fn revoke_device(
        &self,
        ws: &str,
        device_id: &str,
        r: &RevokeDevice,
        now: i64,
    ) -> RepoResult<()> {
        let mut s = self.lock();
        let w = s.workspaces.get_mut(ws).ok_or(RepoError::NotFound)?;
        check_revoke(&row(ws, w), &w.devices, device_id, r)?;
        for d in w.devices.iter_mut().filter(|d| d.id == device_id) {
            d.revoked_at = Some(now);
        }
        w.epoch = r.new_epoch;
        for e in &r.wraps {
            w.wraps
                .insert((e.device_id.clone(), r.new_epoch), e.key_wrap.clone());
        }
        w.recovery_wrap = r.recovery_wrap.clone();
        Ok(())
    }

    async fn key_wrap(&self, ws: &str, device_id: &str) -> RepoResult<Option<(u32, Vec<u8>)>> {
        let s = self.lock();
        let w = s.workspaces.get(ws).ok_or(RepoError::NotFound)?;
        Ok(w.wraps
            .iter()
            .filter(|((d, _), _)| d == device_id)
            .max_by_key(|((_, e), _)| *e)
            .map(|((_, e), wrap)| (*e, wrap.clone())))
    }

    async fn recover_device(
        &self,
        ws: &str,
        device_key: &[u8],
        device_id: &str,
        label_sealed: &[u8],
        now: i64,
        max_devices: i64,
    ) -> RepoResult<(u32, Vec<u8>)> {
        let mut s = self.lock();
        let w = s.workspaces.get_mut(ws).ok_or(RepoError::NotFound)?;
        match w.devices.iter_mut().find(|d| d.id == device_id) {
            Some(d) if d.revoked_at.is_some() => {
                return Err(RepoError::Conflict("device was revoked"))
            }
            Some(_) => {}
            None => {
                if usage(w).devices >= max_devices {
                    return Err(RepoError::Quota("device limit reached"));
                }
                w.devices.push(DeviceInfo {
                    id: device_id.to_string(),
                    key: device_key.to_vec(),
                    label_sealed: label_sealed.to_vec(),
                    created_at: now,
                    revoked_at: None,
                    last_seen: Some(now),
                });
            }
        }
        Ok((w.epoch, w.recovery_wrap.clone()))
    }

    async fn push_ops(
        &self,
        ws: &str,
        device_id: &str,
        ops: &[PushOp],
        now: i64,
        limits: &Limits,
    ) -> RepoResult<PushResponse> {
        let mut s = self.lock();
        let w = s.workspaces.get_mut(ws).ok_or(RepoError::NotFound)?;
        // Validate the whole batch first so a failure leaves nothing half-written.
        let mut fresh = Vec::new();
        let mut duplicate = Vec::new();
        let mut seen = HashSet::new();
        let mut added = 0i64;
        for op in ops {
            check_op(op, w.epoch)?;
            if w.op_ids.contains(&op.op_id) || !seen.insert(op.op_id.clone()) {
                duplicate.push(op.op_id.clone());
            } else {
                added += op.payload.len() as i64;
                fresh.push(op);
            }
        }
        if w.op_bytes + added > limits.max_op_bytes_total {
            return Err(RepoError::Quota("operation storage quota exceeded"));
        }
        let mut seq = w.ops.last().map_or(0, |o| o.seq);
        let mut accepted = Vec::new();
        for op in fresh {
            seq += 1;
            w.op_ids.insert(op.op_id.clone());
            w.ops.push(OpRow {
                seq,
                op_id: op.op_id.clone(),
                device_id: device_id.to_string(),
                doc_id: op.doc_id.clone(),
                epoch: op.epoch,
                format: op.format,
                payload: op.payload.clone(),
            });
            accepted.push(op.op_id.clone());
        }
        w.op_bytes += added;
        let _ = now;
        Ok(PushResponse {
            accepted,
            duplicate,
            cursor: seq,
            epoch: w.epoch,
        })
    }

    async fn pull_ops(&self, ws: &str, after: i64, limit: i64) -> RepoResult<PullResponse> {
        let s = self.lock();
        let w = s.workspaces.get(ws).ok_or(RepoError::NotFound)?;
        let start = w.ops.partition_point(|o| o.seq <= after);
        let rest = &w.ops[start..];
        let take = (limit.max(1) as usize).min(rest.len());
        let ops: Vec<PulledOp> = rest[..take]
            .iter()
            .map(|o| PulledOp {
                seq: o.seq,
                op_id: o.op_id.clone(),
                device_id: o.device_id.clone(),
                doc_id: o.doc_id.clone(),
                epoch: o.epoch,
                format: o.format,
                payload: o.payload.clone(),
            })
            .collect();
        let cursor = ops.last().map_or(after, |o| o.seq);
        Ok(PullResponse {
            ops,
            cursor,
            more: take < rest.len(),
            epoch: w.epoch,
        })
    }

    async fn blob(&self, ws: &str, blob_id: &str) -> RepoResult<Option<BlobMeta>> {
        let s = self.lock();
        let w = s.workspaces.get(ws).ok_or(RepoError::NotFound)?;
        Ok(w.blobs.get(blob_id).map(|b| BlobMeta {
            complete: b.complete,
            total: b.total,
            epoch: b.epoch,
            size: b.size,
            chunks: b.chunks.iter().map(|(i, n)| (*i, *n)).collect(),
        }))
    }

    async fn reserve_chunk(
        &self,
        ws: &str,
        blob_id: &str,
        index: u32,
        size: i64,
        _now: i64,
        limits: &Limits,
    ) -> RepoResult<()> {
        let mut s = self.lock();
        let w = s.workspaces.get_mut(ws).ok_or(RepoError::NotFound)?;
        let used = w.blob_bytes;
        let entry = w.blobs.entry(blob_id.to_string()).or_insert(BlobEntry {
            epoch: 0,
            total: 0,
            size: 0,
            complete: false,
            chunks: BTreeMap::new(),
        });
        if entry.complete {
            return Err(RepoError::Conflict("blob already complete"));
        }
        let previous = entry.chunks.get(&index).copied().unwrap_or(0);
        let delta = size - previous;
        let blob_total: i64 = entry.chunks.values().sum::<i64>() + delta;
        if blob_total > limits.max_blob_size {
            return Err(RepoError::Quota("blob exceeds the size limit"));
        }
        if used + delta > limits.max_blob_bytes_total {
            if entry.chunks.is_empty() {
                w.blobs.remove(blob_id);
            }
            return Err(RepoError::Quota("attachment storage quota exceeded"));
        }
        entry.chunks.insert(index, size);
        w.blob_bytes += delta;
        Ok(())
    }

    async fn remove_chunk(&self, ws: &str, blob_id: &str, index: u32) -> RepoResult<()> {
        let mut s = self.lock();
        let w = s.workspaces.get_mut(ws).ok_or(RepoError::NotFound)?;
        if let Some(b) = w.blobs.get_mut(blob_id) {
            if !b.complete {
                if let Some(n) = b.chunks.remove(&index) {
                    w.blob_bytes -= n;
                }
                if b.chunks.is_empty() {
                    w.blobs.remove(blob_id);
                }
            }
        }
        Ok(())
    }

    async fn commit_blob(&self, ws: &str, blob_id: &str, c: &CommitBlob) -> RepoResult<()> {
        let mut s = self.lock();
        let w = s.workspaces.get_mut(ws).ok_or(RepoError::NotFound)?;
        if c.epoch > w.epoch {
            return Err(RepoError::Conflict("unknown key epoch"));
        }
        let b = w.blobs.get_mut(blob_id).ok_or(RepoError::NotFound)?;
        if b.complete {
            return Ok(());
        }
        let all = c.total > 0
            && (0..c.total).all(|i| b.chunks.contains_key(&i))
            && b.chunks.len() == c.total as usize;
        if !all {
            return Err(RepoError::Invalid("chunks are missing"));
        }
        if b.chunks.values().sum::<i64>() != c.size {
            return Err(RepoError::Invalid("blob size does not match its chunks"));
        }
        b.complete = true;
        b.total = c.total;
        b.epoch = c.epoch;
        b.size = c.size;
        Ok(())
    }

    async fn purge_expired(&self, now: i64) -> RepoResult<()> {
        let mut s = self.lock();
        s.nonces.retain(|_, exp| *exp > now);
        s.invites.retain(|_, i| i.expires_at + 3600 > now);
        Ok(())
    }
}
