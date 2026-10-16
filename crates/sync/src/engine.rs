//! The sync cycle: push queued operations, pull and apply remote ones, refresh keys, report status.

use crate::account::{rotate_context, Account};
use crate::http::{now_secs, Api};
use crate::secrets::SecretStore;
use crate::status::{Phase, SyncEvent, SyncStatus};
use crate::{Result, SyncError};
use notercrux_crypto::{content_aad, unwrap_ring, DeviceKeys, KeyRing, Purpose};
use notercrux_protocol::*;
use notercrux_storage::{ApplyReport, LocalOp, RemoteOp, Store, SYNC_FORMAT};
use reqwest::Method;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{broadcast, watch, Notify};

#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// How long a pull request waits at the server for news before returning empty.
    pub long_poll: Duration,
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    /// Upper bound of ciphertext per upload request.
    pub batch_bytes: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            long_poll: Duration::from_secs(20),
            backoff_base: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
            batch_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Session {
    pub account: Account,
    pub keys: Arc<DeviceKeys>,
    pub ring: Arc<KeyRing>,
}

#[derive(Clone, Debug, Default)]
pub struct CycleReport {
    pub sent: usize,
    pub received: usize,
    pub applied: ApplyReport,
    pub attachments: crate::AttachmentReport,
}

pub struct Engine {
    pub(crate) store: Arc<Mutex<Store>>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) api: Api,
    pub(crate) config: EngineConfig,
    pub(crate) session: Mutex<Option<Session>>,
    pub(crate) status: watch::Sender<SyncStatus>,
    changes: broadcast::Sender<ApplyReport>,
    events: Mutex<VecDeque<SyncEvent>>,
    /// A local edit happened: abort a long poll and push.
    wake: Notify,
    /// Retry now (user pressed "Sync now", or the network came back).
    kick: Notify,
    /// Serializes sync cycles and account changes.
    pub(crate) gate: tokio::sync::Mutex<()>,
    /// How many times each server sequence number failed verification in a row.
    failures: Mutex<std::collections::HashMap<i64, u32>>,
}

/// A failed operation is re-fetched this many times (a corrupted response is retried) before it is
/// skipped as permanently invalid.
const VERIFY_ATTEMPTS: u32 = 3;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    /// Loads the link (if any) from the database and secure storage.
    pub fn open(
        store: Arc<Mutex<Store>>,
        secrets: Arc<dyn SecretStore>,
        config: EngineConfig,
    ) -> Result<Arc<Engine>> {
        let (status, _) = watch::channel(SyncStatus::default());
        let (changes, _) = broadcast::channel(64);
        let engine = Arc::new(Engine {
            store,
            secrets,
            api: Api::new()?,
            config,
            session: Mutex::new(None),
            status,
            changes,
            events: Mutex::new(VecDeque::new()),
            wake: Notify::new(),
            kick: Notify::new(),
            gate: tokio::sync::Mutex::new(()),
            failures: Mutex::new(Default::default()),
        });
        engine.reload()?;
        Ok(engine)
    }

    /// Re-reads the account and keys (after enrollment, recovery or leaving).
    pub(crate) fn reload(&self) -> Result<()> {
        let account: Option<Account> = self
            .with_store(|s| s.sync_get("acct"))?
            .and_then(|j| serde_json::from_str(&j).ok());
        let Some(account) = account else {
            *lock(&self.session) = None;
            self.status.send_replace(SyncStatus::default());
            return Ok(());
        };
        let keys = self
            .secrets
            .get(crate::DEVICE_SECRET)
            .map_err(SyncError::Secrets)?;
        let ring = self
            .secrets
            .get(crate::RING_SECRET)
            .map_err(SyncError::Secrets)?;
        let revoked = self.with_store(|s| s.sync_get("revoked"))?.as_deref() == Some("1");
        let loaded = match (keys, ring) {
            (Some(k), Some(r)) => {
                let keys =
                    DeviceKeys::from_bytes(&k).map_err(|e| SyncError::Keys(e.to_string()))?;
                let ring = KeyRing::from_bytes(&r).map_err(|e| SyncError::Keys(e.to_string()))?;
                Some(Session {
                    account: account.clone(),
                    keys: Arc::new(keys),
                    ring: Arc::new(ring),
                })
            }
            _ => None,
        };
        let phase = if loaded.is_none() {
            Phase::KeysMissing
        } else if revoked {
            Phase::Revoked
        } else {
            Phase::Idle
        };
        let epoch = loaded.as_ref().map_or(0, |s| s.ring.current_epoch());
        *lock(&self.session) = loaded;
        let rejected = self
            .with_store(|s| s.sync_get("rejected_ops"))?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        self.status.send_replace(SyncStatus {
            phase,
            server: Some(account.server),
            workspace_id: Some(account.workspace_id),
            device_id: Some(account.device_id),
            epoch,
            rejected_ops: rejected,
            last_error: match phase {
                Phase::KeysMissing => Some("The encryption keys for this device are missing from secure storage. Recover the workspace with your recovery key.".into()),
                Phase::Revoked => Some(SyncError::Revoked.to_string()),
                _ => None,
            },
            ..SyncStatus::default()
        });
        self.refresh_counts();
        Ok(())
    }

    pub(crate) fn with_store<T>(
        &self,
        f: impl FnOnce(&mut Store) -> notercrux_storage::Result<T>,
    ) -> Result<T> {
        let mut guard = lock(&self.store);
        Ok(f(&mut guard)?)
    }

    pub(crate) fn session(&self) -> Result<Session> {
        lock(&self.session).clone().ok_or(SyncError::NotLinked)
    }

    pub(crate) fn set_session(&self, s: Session) {
        *lock(&self.session) = Some(s);
    }

    // ---- observation ------------------------------------------------------------------------

    pub fn status(&self) -> SyncStatus {
        self.status.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<SyncStatus> {
        self.status.subscribe()
    }

    /// Fires after remote changes were applied to the local database (refresh what is on screen).
    pub fn subscribe_changes(&self) -> broadcast::Receiver<ApplyReport> {
        self.changes.subscribe()
    }

    /// The last events, oldest first.
    pub fn events(&self) -> Vec<SyncEvent> {
        lock(&self.events).iter().cloned().collect()
    }

    pub(crate) fn log(&self, level: &'static str, message: impl Into<String>) {
        let mut q = lock(&self.events);
        if q.len() >= 100 {
            q.pop_front();
        }
        q.push_back(SyncEvent {
            at: now_secs(),
            level,
            message: message.into(),
        });
    }

    pub(crate) fn update_status(&self, f: impl FnOnce(&mut SyncStatus)) {
        self.status.send_modify(f);
    }

    pub(crate) fn refresh_counts(&self) {
        if let Ok(stats) = self.with_store(|s| s.sync_stats()) {
            self.update_status(|st| {
                st.pending_ops = stats.pending_ops;
                st.held_ops = stats.held_ops;
            });
        }
    }

    /// Call after any local change: wakes a waiting sync loop so the change goes out promptly.
    pub fn notify_local_change(&self) {
        self.wake.notify_one();
        self.refresh_counts();
    }

    /// Retry immediately instead of waiting out a backoff.
    pub fn sync_soon(&self) {
        self.kick.notify_one();
        self.wake.notify_one();
    }

    /// Aborts a waiting long poll so a pending account change can take the gate.
    pub(crate) fn sync_soon_wake(&self) {
        self.wake.notify_one();
    }

    // ---- the cycle --------------------------------------------------------------------------

    /// One push and pull pass without long polling.
    pub async fn sync_once(&self) -> Result<CycleReport> {
        self.cycle(Duration::ZERO).await
    }

    pub(crate) async fn cycle(&self, wait: Duration) -> Result<CycleReport> {
        let _gate = self.gate.lock().await;
        let sess = self.session()?;
        if self.status().phase == Phase::Revoked {
            return Err(SyncError::Revoked);
        }
        self.update_status(|s| {
            s.phase = Phase::Syncing;
            s.retry_in_secs = None;
        });
        let result = self.cycle_inner(wait).await;
        let _ = &sess;
        match &result {
            Ok(r) => {
                self.update_status(|s| {
                    s.phase = Phase::Idle;
                    s.last_error = None;
                    s.last_sync_at = Some(now_secs());
                    s.ops_sent += r.sent as u64;
                    s.ops_received += r.received as u64;
                    s.recovered_blocks += r.applied.recovered_blocks as u64;
                });
            }
            Err(SyncError::Revoked) => {
                let _ = self.with_store(|s| s.sync_set("revoked", "1"));
                self.log(
                    "error",
                    "This device was removed from the workspace. Local data is kept.",
                );
                self.update_status(|s| {
                    s.phase = Phase::Revoked;
                    s.last_error = Some(SyncError::Revoked.to_string());
                });
            }
            Err(e) => {
                let phase = if e.is_transient() {
                    Phase::Offline
                } else {
                    Phase::Error
                };
                self.log(
                    if e.is_transient() { "warn" } else { "error" },
                    e.to_string(),
                );
                self.update_status(|s| {
                    s.phase = phase;
                    s.last_error = Some(e.to_string());
                });
            }
        }
        self.refresh_counts();
        result
    }

    async fn cycle_inner(&self, wait: Duration) -> Result<CycleReport> {
        let mut report = CycleReport {
            sent: self.push_all().await?,
            ..CycleReport::default()
        };
        if self.with_store(|s| s.sync_stats())?.held_ops > 0 {
            let r = self.with_store(|s| s.sync_apply_held())?;
            if r.applied > 0 {
                self.log("info", "Applied changes that needed a newer app version.");
                merge(&mut report.applied, &r);
                self.announce(&r);
            }
        }
        // Drain everything the server already has.
        loop {
            let Some(page) = self.pull_page(Duration::ZERO).await? else {
                break;
            };
            report.received += page.0;
            merge(&mut report.applied, &page.1);
            if !page.2 {
                break;
            }
        }
        // Attachments follow the operations: a block must arrive before its file can be fetched.
        report.attachments = self.sync_attachments().await?;
        // Nothing to do: listen for news (a local edit wakes this early).
        let idle = report.sent == 0 && report.received == 0 && report.attachments.moved() == 0;
        if idle && !wait.is_zero() {
            if let Some(page) = self.pull_page(wait).await? {
                report.received += page.0;
                merge(&mut report.applied, &page.1);
            }
        }
        Ok(report)
    }

    /// Uploads every queued operation. Returns how many the server confirmed.
    async fn push_all(&self) -> Result<usize> {
        let mut total = 0;
        loop {
            let sess = self.session()?;
            let pending = self.with_store(|s| s.sync_pending(256))?;
            if pending.is_empty() {
                return Ok(total);
            }
            let mut batch = Vec::new();
            let mut bytes = 0usize;
            for op in &pending {
                let sealed = seal_op(&sess, op)?;
                bytes += sealed.payload.len();
                batch.push(sealed);
                if bytes >= self.config.batch_bytes {
                    break;
                }
            }
            let body = serde_json::to_vec(&PushRequest { ops: batch })
                .map_err(|e| SyncError::Protocol(e.to_string()))?;
            let target = format!("/v1/workspaces/{}/ops", sess.account.workspace_id);
            let reply = self
                .api
                .call(
                    &sess.account.server,
                    Some(&sess.keys),
                    Method::POST,
                    &target,
                    body,
                    None,
                )
                .await?;
            let res: PushResponse = reply.json()?;
            let confirmed: Vec<String> = res
                .accepted
                .iter()
                .chain(res.duplicate.iter())
                .cloned()
                .collect();
            if confirmed.is_empty() {
                return Err(SyncError::Protocol("the server confirmed nothing".into()));
            }
            self.with_store(|s| s.sync_ack(&confirmed))?;
            total += confirmed.len();
            self.refresh_counts();
            if res.epoch > sess.ring.current_epoch() {
                self.refresh_keys().await?;
            }
        }
    }

    /// Pulls one page. `None` means a long poll was cut short by a local change. Otherwise returns
    /// (operations received from other devices, what applying them did, more pages available).
    async fn pull_page(&self, wait: Duration) -> Result<Option<(usize, ApplyReport, bool)>> {
        let sess = self.session()?;
        let after = self.with_store(|s| s.sync_cursor())?;
        let target = format!(
            "/v1/workspaces/{}/ops?after={after}&limit=500&wait={}",
            sess.account.workspace_id,
            wait.as_secs()
        );
        if !wait.is_zero() {
            // Everything local is sent and nothing is waiting: from here we are just listening.
            self.update_status(|s| {
                s.phase = Phase::Idle;
                s.last_error = None;
                s.last_sync_at = Some(now_secs());
            });
        }
        let reply = {
            let call = self.api.call(
                &sess.account.server,
                Some(&sess.keys),
                Method::GET,
                &target,
                Vec::new(),
                Some(wait + Duration::from_secs(30)),
            );
            tokio::pin!(call);
            if wait.is_zero() {
                call.await?
            } else {
                tokio::select! {
                    r = &mut call => r?,
                    _ = self.wake.notified() => return Ok(None),
                }
            }
        };
        let page: PullResponse = reply.json()?;

        // A rotation we have not seen yet, or operations sealed under an epoch we lack.
        let mut sess = sess;
        let missing = page
            .ops
            .iter()
            .any(|o| o.device_id != sess.account.device_id && !sess.ring.has_epoch(o.epoch));
        if page.epoch > sess.ring.current_epoch() || missing {
            self.refresh_keys().await?;
            sess = self.session()?;
            if page
                .ops
                .iter()
                .any(|o| o.device_id != sess.account.device_id && !sess.ring.has_epoch(o.epoch))
            {
                return Err(SyncError::Keys(
                    "operations use an encryption key this device has not received yet".into(),
                ));
            }
        }

        let mut remote = Vec::new();
        let mut bad = Vec::new();
        for op in &page.ops {
            if op.device_id == sess.account.device_id {
                continue; // our own operation, already applied locally
            }
            match open_op(&sess, op) {
                Some(r) => remote.push(r),
                None => bad.push(op.seq),
            }
        }
        let mut rejected = 0i64;
        if !bad.is_empty() {
            // A response can arrive damaged. Retry before concluding the stored operation is invalid.
            let give_up = {
                let mut f = lock(&self.failures);
                let mut give_up = true;
                for seq in &bad {
                    let n = f.entry(*seq).or_insert(0);
                    *n += 1;
                    give_up &= *n >= VERIFY_ATTEMPTS;
                }
                if give_up {
                    for seq in &bad {
                        f.remove(seq);
                    }
                }
                give_up
            };
            if !give_up {
                return Err(SyncError::Protocol(format!(
                    "{} operation(s) failed verification; fetching them again",
                    bad.len()
                )));
            }
            rejected = bad.len() as i64;
        }
        if rejected > 0 {
            self.log(
                "warn",
                format!("Skipped {rejected} operation(s) that failed verification."),
            );
            let total = self.with_store(|s| {
                let n = s
                    .sync_get("rejected_ops")?
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0)
                    + rejected;
                s.sync_set("rejected_ops", &n.to_string())?;
                Ok(n)
            })?;
            self.update_status(|s| s.rejected_ops = total);
        }
        let received = remote.len();
        let report = self.with_store(|s| s.sync_apply_remote(remote, page.cursor))?;
        if report.recovered_blocks > 0 {
            self.log(
                "warn",
                format!("Restored {} block(s) that were edited here and deleted elsewhere (see the callout at the end of the page).", report.recovered_blocks),
            );
        }
        self.announce(&report);
        Ok(Some((received, report, page.more)))
    }

    fn announce(&self, r: &ApplyReport) {
        if r.pages_changed || !r.documents_changed.is_empty() || r.recovered_blocks > 0 {
            let _ = self.changes.send(r.clone());
        }
    }

    /// Fetches this device's newest key wrap, if the workspace key was rotated.
    pub(crate) async fn refresh_keys(&self) -> Result<bool> {
        let sess = self.session()?;
        let have = sess.ring.current_epoch();
        let target = format!(
            "/v1/workspaces/{}/keys?have={have}",
            sess.account.workspace_id
        );
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
        let k: KeysResponse = reply.json()?;
        let Some(wrap) = k.key_wrap else {
            return Ok(false);
        };
        let ring = unwrap_ring(
            &sess.keys,
            &wrap,
            &rotate_context(&sess.account.workspace_id, &sess.account.device_id, k.epoch),
            None,
        )
        .map_err(|e| SyncError::Keys(format!("could not open the new key: {e}")))?;
        if ring.current_epoch() <= have {
            return Ok(false);
        }
        // Only accept a ring whose author already knew every key we hold (the server cannot).
        if !sess.ring.is_extended_by(&ring) {
            self.log(
                "error",
                "Rejected a key update that did not come from a workspace member.",
            );
            return Err(SyncError::Keys(
                "the key update could not be verified".into(),
            ));
        }
        self.secrets
            .set(crate::RING_SECRET, &ring.to_bytes())
            .map_err(SyncError::Secrets)?;
        let epoch = ring.current_epoch();
        self.set_session(Session {
            ring: Arc::new(ring),
            ..sess
        });
        self.update_status(|s| s.epoch = epoch);
        self.log("info", format!("Encryption key updated (epoch {epoch})."));
        Ok(true)
    }

    // ---- the long-running loop --------------------------------------------------------------

    /// Runs until `shutdown` turns true: long-polls the server, pushes local edits promptly, and
    /// retries with exponential backoff and jitter when the server is unreachable.
    pub async fn run(self: Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        // Edits are noticed by looking at the outbox, so callers need not tell the engine about each one.
        let watcher = {
            let me = self.clone();
            let mut stop = shutdown.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(750)) => {
                            if me.has_outgoing_work() {
                                me.wake.notify_one();
                            }
                        }
                        _ = stop.changed() => {
                            if *stop.borrow() {
                                return;
                            }
                        }
                    }
                }
            })
        };
        self.run_loop(&mut shutdown).await;
        watcher.abort();
    }

    /// Local operations or attachments are waiting to be sent.
    fn has_outgoing_work(&self) -> bool {
        if self.status().phase != Phase::Idle {
            return false;
        }
        self.with_store(|s| {
            let ops = s.sync_pending(1)?;
            Ok(!ops.is_empty() || !s.attachment_work()?.uploads.is_empty())
        })
        .unwrap_or(false)
    }

    async fn run_loop(self: &Arc<Self>, shutdown: &mut watch::Receiver<bool>) {
        let mut failures = 0u32;
        loop {
            if *shutdown.borrow() {
                return;
            }
            let phase = self.status().phase;
            if phase == Phase::Disabled || phase == Phase::KeysMissing || phase == Phase::Revoked {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    _ = shutdown.changed() => {}
                    _ = self.kick.notified() => {}
                }
                // Pick up a link made, or keys restored, while idle.
                if phase != Phase::Revoked {
                    let _ = self.reload();
                }
                continue;
            }
            match self.cycle(self.config.long_poll).await {
                Ok(_) => failures = 0,
                Err(SyncError::NotLinked | SyncError::Revoked) => {}
                Err(e) => {
                    failures += 1;
                    let delay = backoff_delay(failures, &self.config, e.is_transient(), jitter());
                    self.update_status(|s| s.retry_in_secs = Some(delay.as_secs().max(1)));
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = shutdown.changed() => {}
                        _ = self.kick.notified() => {}
                    }
                }
            }
        }
    }
}

fn merge(into: &mut ApplyReport, from: &ApplyReport) {
    into.applied += from.applied;
    into.held += from.held;
    into.recovered_blocks += from.recovered_blocks;
    into.pages_changed |= from.pages_changed;
    for d in &from.documents_changed {
        if !into.documents_changed.contains(d) {
            into.documents_changed.push(d.clone());
        }
    }
    into.cursor = from.cursor.max(into.cursor);
}

fn seal_op(sess: &Session, op: &LocalOp) -> Result<PushOp> {
    let ws = &sess.account.workspace_id;
    let doc_id = sess.ring.doc_id(&op.doc_key)?;
    let inner = OpEnvelope {
        format: SYNC_FORMAT,
        doc_key: op.doc_key.clone(),
        update: op.update.clone(),
    }
    .encode();
    let (epoch, payload) =
        sess.ring
            .seal(Purpose::Content, |e| content_aad(ws, &doc_id, e), &inner)?;
    Ok(PushOp {
        op_id: op.op_id.clone(),
        doc_id,
        epoch,
        format: SYNC_FORMAT,
        payload,
    })
}

/// Decrypts and validates one remote operation. `None` means it failed verification.
fn open_op(sess: &Session, op: &PulledOp) -> Option<RemoteOp> {
    let ws = &sess.account.workspace_id;
    let plain = sess
        .ring
        .open(
            op.epoch,
            Purpose::Content,
            &content_aad(ws, &op.doc_id, op.epoch),
            &op.payload,
        )
        .ok()?;
    let env = OpEnvelope::decode(&plain).ok()?;
    // The inner document key must be the one the outer, authenticated document id was derived from.
    if sess.ring.doc_id(&env.doc_key).ok()? != op.doc_id {
        return None;
    }
    Some(RemoteOp {
        seq: op.seq,
        op_id: op.op_id.clone(),
        doc_key: env.doc_key,
        format: env.format,
        update: env.update,
    })
}

/// Exponential backoff with a cap. Rejections that will not fix themselves wait at least 30 s.
pub(crate) fn backoff_delay(
    failures: u32,
    cfg: &EngineConfig,
    transient: bool,
    jitter: f64,
) -> Duration {
    let exp = cfg.backoff_base.as_secs_f64() * 2f64.powi(failures.saturating_sub(1).min(16) as i32);
    let mut secs = exp.min(cfg.backoff_max.as_secs_f64());
    if !transient {
        secs = secs.max(30f64.min(cfg.backoff_max.as_secs_f64()));
    }
    Duration::from_secs_f64((secs * (0.8 + 0.4 * jitter)).max(0.001))
}

fn jitter() -> f64 {
    let b = uuid::Uuid::new_v4().as_bytes()[0];
    b as f64 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_exponentially_is_capped_and_jittered() {
        let cfg = EngineConfig::default();
        let d = |n, t, j| backoff_delay(n, &cfg, t, j).as_secs_f64();
        assert!((d(1, true, 0.5) - 1.0).abs() < 1e-6);
        assert!((d(2, true, 0.5) - 2.0).abs() < 1e-6);
        assert!((d(4, true, 0.5) - 8.0).abs() < 1e-6);
        assert!(d(30, true, 0.5) <= 60.0 * 1.2);
        assert!(d(30, true, 0.5) >= 60.0 * 0.8);
        assert!(d(3, true, 0.0) < d(3, true, 1.0), "jitter spreads retries");
        assert!(
            d(1, false, 0.5) >= 30.0 * 0.8,
            "permanent rejections back off hard"
        );
    }
}
