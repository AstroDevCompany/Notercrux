//! PostgreSQL repository (plain Postgres; also works on managed services such as Neon).

use crate::repo::*;
use async_trait::async_trait;
use deadpool_postgres::{Object, Pool};
use notercrux_protocol::{
    ApproveInvite, CommitBlob, CreateWorkspace, DeviceInfo, InviteRequest, InviteResult, Limits,
    PullResponse, PulledOp, PushOp, PushResponse, RevokeDevice, Usage,
};
use std::collections::HashSet;
use tokio_postgres::Row;

const MIGRATIONS: &[(i32, &str)] = &[(1, include_str!("../migrations/0001_init.sql"))];

pub struct PgRepo {
    pool: Pool,
    ready: tokio::sync::OnceCell<()>,
}

fn be<E: std::fmt::Display>(e: E) -> RepoError {
    RepoError::Backend(e.to_string())
}

impl PgRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            ready: tokio::sync::OnceCell::new(),
        }
    }

    /// Builds a connection pool from a `postgres://` URL. `sslmode=require` (or stricter) uses TLS,
    /// verified against the bundled public roots plus, optionally, a PEM bundle in `extra_ca`.
    pub fn connect(url: &str) -> Result<Self, String> {
        Self::connect_with(url, None)
    }

    pub fn connect_with(url: &str, extra_ca: Option<&[u8]>) -> Result<Self, String> {
        let cfg: tokio_postgres::Config = url
            .parse()
            .map_err(|e| format!("invalid database URL: {e}"))?;
        let mgr_cfg = deadpool_postgres::ManagerConfig {
            recycling_method: deadpool_postgres::RecyclingMethod::Fast,
        };
        let manager = match cfg.get_ssl_mode() {
            tokio_postgres::config::SslMode::Disable | tokio_postgres::config::SslMode::Prefer => {
                deadpool_postgres::Manager::from_config(cfg, tokio_postgres::NoTls, mgr_cfg)
            }
            _ => deadpool_postgres::Manager::from_config(cfg, tls_connector(extra_ca)?, mgr_cfg),
        };
        let pool = Pool::builder(manager)
            .max_size(16)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self::new(pool))
    }

    /// Applies pending migrations. Safe to run on every start and from several instances at once.
    pub async fn migrate(&self) -> RepoResult<()> {
        self.ready
            .get_or_try_init(|| self.run_migrations())
            .await
            .map(|_| ())
    }

    async fn run_migrations(&self) -> RepoResult<()> {
        let mut c = self.pool.get().await.map_err(be)?;
        let tx = c.transaction().await.map_err(be)?;
        // Serialize concurrent starters.
        tx.batch_execute("select pg_advisory_xact_lock(7461726300)")
            .await
            .map_err(be)?;
        tx.batch_execute("create table if not exists ncx_migrations (version integer primary key, applied_at bigint not null)")
            .await
            .map_err(be)?;
        for (version, sql) in MIGRATIONS {
            let done = tx
                .query_opt(
                    "select 1 from ncx_migrations where version = $1",
                    &[version],
                )
                .await
                .map_err(be)?;
            if done.is_none() {
                tx.batch_execute(sql).await.map_err(be)?;
                tx.execute(
                    "insert into ncx_migrations (version, applied_at) values ($1, extract(epoch from now())::bigint)",
                    &[version],
                )
                .await
                .map_err(be)?;
            }
        }
        tx.commit().await.map_err(be)
    }

    /// A pooled connection; the first call also brings the schema up to date.
    async fn conn(&self) -> RepoResult<Object> {
        self.migrate().await?;
        self.pool.get().await.map_err(be)
    }
}

fn device_from(r: &Row) -> DeviceInfo {
    DeviceInfo {
        id: r.get("id"),
        key: r.get("key"),
        label_sealed: r.get("label_sealed"),
        created_at: r.get("created_at"),
        revoked_at: r.get("revoked_at"),
        last_seen: r.get("last_seen"),
    }
}

const DEVICE_COLS: &str = "id, key, label_sealed, created_at, revoked_at, last_seen";

fn workspace_from(id: &str, r: &Row) -> WorkspaceRow {
    WorkspaceRow {
        id: id.to_string(),
        epoch: r.get::<_, i32>("epoch") as u32,
        head: r.get("head"),
        recovery_key: r.get("recovery_key"),
        recovery_wrap: r.get("recovery_wrap"),
        usage: Usage {
            op_bytes: r.get("op_bytes"),
            op_count: r.get("op_count"),
            blob_bytes: r.get("blob_bytes"),
            devices: r.get("devices"),
        },
    }
}

const WORKSPACE_SQL: &str = "select epoch, head, recovery_key, recovery_wrap, op_bytes, op_count, blob_bytes, \
     (select count(*) from devices d where d.workspace_id = w.id and d.revoked_at is null) as devices \
     from workspaces w where id = $1";

fn invite_from(r: &Row) -> RepoResult<InviteRow> {
    let request: Option<String> = r.get("request");
    Ok(InviteRow {
        id: r.get("id"),
        workspace_id: r.get("workspace_id"),
        created_by: r.get("created_by"),
        expires_at: r.get("expires_at"),
        request: match request {
            Some(s) => Some(serde_json::from_str::<InviteRequest>(&s).map_err(be)?),
            None => None,
        },
        request_device: r.get("request_device"),
        approved_device: r.get("approved_device"),
    })
}

#[async_trait]
impl Repo for PgRepo {
    async fn create_workspace(
        &self,
        req: &CreateWorkspace,
        device_id: &str,
        now: i64,
    ) -> RepoResult<()> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let n = tx
            .execute(
                "insert into workspaces (id, recovery_key, recovery_wrap, created_at) values ($1, $2, $3, $4) on conflict do nothing",
                &[&req.workspace_id, &req.recovery_key, &req.recovery_wrap, &now],
            )
            .await
            .map_err(be)?;
        if n == 0 {
            return Err(RepoError::Conflict("workspace already exists"));
        }
        tx.execute(
            "insert into devices (workspace_id, id, key, label_sealed, created_at, last_seen) values ($1, $2, $3, $4, $5, $5)",
            &[&req.workspace_id, &device_id, &req.device_key, &req.label_sealed, &now],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)
    }

    async fn delete_workspace(&self, ws: &str) -> RepoResult<()> {
        let c = self.conn().await?;
        let n = c
            .execute("delete from workspaces where id = $1", &[&ws])
            .await
            .map_err(be)?;
        if n == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    async fn workspace(&self, ws: &str) -> RepoResult<Option<WorkspaceRow>> {
        let c = self.conn().await?;
        Ok(c.query_opt(WORKSPACE_SQL, &[&ws])
            .await
            .map_err(be)?
            .map(|r| workspace_from(ws, &r)))
    }

    async fn device(&self, ws: &str, id: &str) -> RepoResult<Option<DeviceInfo>> {
        let c = self.conn().await?;
        let sql = format!("select {DEVICE_COLS} from devices where workspace_id = $1 and id = $2");
        Ok(c.query_opt(&sql, &[&ws, &id])
            .await
            .map_err(be)?
            .map(|r| device_from(&r)))
    }

    async fn devices(&self, ws: &str) -> RepoResult<Vec<DeviceInfo>> {
        let c = self.conn().await?;
        let sql = format!(
            "select {DEVICE_COLS} from devices where workspace_id = $1 order by created_at, id"
        );
        Ok(c.query(&sql, &[&ws])
            .await
            .map_err(be)?
            .iter()
            .map(device_from)
            .collect())
    }

    async fn touch_device(&self, ws: &str, id: &str, now: i64) -> RepoResult<()> {
        let c = self.conn().await?;
        c.execute(
            "update devices set last_seen = $3 where workspace_id = $1 and id = $2",
            &[&ws, &id, &now],
        )
        .await
        .map_err(be)?;
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
        let c = self.conn().await?;
        let exp = now + ttl;
        let row = c
            .query_opt(
                "insert into nonces (workspace_id, device_id, nonce, expires_at) values ($1, $2, $3, $4) \
                 on conflict (workspace_id, device_id, nonce) do update set expires_at = excluded.expires_at \
                 where nonces.expires_at <= $5 returning 1",
                &[&ws, &device, &nonce, &exp, &now],
            )
            .await
            .map_err(be)?;
        Ok(row.is_some())
    }

    async fn create_invite(
        &self,
        ws: &str,
        id: &str,
        by: &str,
        expires_at: i64,
        now: i64,
    ) -> RepoResult<()> {
        let c = self.conn().await?;
        let n = c
            .execute(
                "insert into invites (id, workspace_id, created_by, expires_at, created_at) values ($1, $2, $3, $4, $5) on conflict do nothing",
                &[&id, &ws, &by, &expires_at, &now],
            )
            .await
            .map_err(be)?;
        if n == 0 {
            return Err(RepoError::Conflict("invite id in use"));
        }
        Ok(())
    }

    async fn invite(&self, id: &str) -> RepoResult<Option<InviteRow>> {
        let c = self.conn().await?;
        match c
            .query_opt("select * from invites where id = $1", &[&id])
            .await
            .map_err(be)?
        {
            Some(r) => Ok(Some(invite_from(&r)?)),
            None => Ok(None),
        }
    }

    async fn submit_invite_request(
        &self,
        id: &str,
        req: &InviteRequest,
        device_id: &str,
        now: i64,
    ) -> RepoResult<()> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let r = tx
            .query_opt("select * from invites where id = $1 for update", &[&id])
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?;
        let inv = invite_from(&r)?;
        if inv.workspace_id != req.workspace_id {
            return Err(RepoError::NotFound);
        }
        if inv.expires_at <= now {
            return Err(RepoError::Conflict("invite expired"));
        }
        if inv.approved_device.is_some() {
            return Err(RepoError::Conflict("invite already used"));
        }
        let json = serde_json::to_string(req).map_err(be)?;
        tx.execute(
            "update invites set request = $2, request_device = $3 where id = $1",
            &[&id, &json, &device_id],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)
    }

    async fn approve_invite(
        &self,
        ws: &str,
        id: &str,
        a: &ApproveInvite,
        now: i64,
        max_devices: i64,
    ) -> RepoResult<u32> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let r = tx
            .query_opt("select * from invites where id = $1 for update", &[&id])
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?;
        let inv = invite_from(&r)?;
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
        let w = tx
            .query_one(
                "select epoch from workspaces where id = $1 for update",
                &[&ws],
            )
            .await
            .map_err(be)?;
        let epoch: i32 = w.get(0);
        let exists = tx
            .query_opt(
                "select 1 from devices where workspace_id = $1 and id = $2",
                &[&ws, &dev],
            )
            .await
            .map_err(be)?;
        if exists.is_some() {
            return Err(RepoError::Conflict("device already enrolled"));
        }
        let count: i64 = tx
            .query_one(
                "select count(*) from devices where workspace_id = $1 and revoked_at is null",
                &[&ws],
            )
            .await
            .map_err(be)?
            .get(0);
        if count >= max_devices {
            return Err(RepoError::Quota("device limit reached"));
        }
        tx.execute(
            "insert into devices (workspace_id, id, key, label_sealed, created_at) values ($1, $2, $3, $4, $5)",
            &[&ws, &dev, &req.device_key, &a.label_sealed, &now],
        )
        .await
        .map_err(be)?;
        tx.execute(
            "insert into key_wraps (workspace_id, device_id, epoch, wrap) values ($1, $2, $3, $4)",
            &[&ws, &dev, &epoch, &a.key_wrap],
        )
        .await
        .map_err(be)?;
        tx.execute(
            "update invites set approved_device = $2 where id = $1",
            &[&id, &dev],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)?;
        Ok(epoch as u32)
    }

    async fn invite_result(&self, id: &str, device_id: &str) -> RepoResult<Option<InviteResult>> {
        let c = self.conn().await?;
        let Some(r) = c
            .query_opt("select * from invites where id = $1", &[&id])
            .await
            .map_err(be)?
        else {
            return Ok(None);
        };
        let inv = invite_from(&r)?;
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
        let w = c
            .query_opt(
                "select epoch, wrap from key_wraps where workspace_id = $1 and device_id = $2 order by epoch desc limit 1",
                &[&inv.workspace_id, &device_id],
            )
            .await
            .map_err(be)?;
        Ok(Some(InviteResult {
            approved: true,
            epoch: w.as_ref().map_or(0, |r| r.get::<_, i32>(0) as u32),
            key_wrap: w.map(|r| r.get(1)),
        }))
    }

    async fn revoke_device(
        &self,
        ws: &str,
        device_id: &str,
        r: &RevokeDevice,
        now: i64,
    ) -> RepoResult<()> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let w = tx
            .query_opt(&format!("{WORKSPACE_SQL} for update of w"), &[&ws])
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?;
        let row = workspace_from(ws, &w);
        let sql = format!("select {DEVICE_COLS} from devices where workspace_id = $1");
        let devices: Vec<DeviceInfo> = tx
            .query(&sql, &[&ws])
            .await
            .map_err(be)?
            .iter()
            .map(device_from)
            .collect();
        check_revoke(&row, &devices, device_id, r)?;
        let epoch = r.new_epoch as i32;
        tx.execute(
            "update devices set revoked_at = $3 where workspace_id = $1 and id = $2",
            &[&ws, &device_id, &now],
        )
        .await
        .map_err(be)?;
        tx.execute(
            "update workspaces set epoch = $2, recovery_wrap = $3 where id = $1",
            &[&ws, &epoch, &r.recovery_wrap],
        )
        .await
        .map_err(be)?;
        for e in &r.wraps {
            tx.execute(
                "insert into key_wraps (workspace_id, device_id, epoch, wrap) values ($1, $2, $3, $4) \
                 on conflict (workspace_id, device_id, epoch) do update set wrap = excluded.wrap",
                &[&ws, &e.device_id, &epoch, &e.key_wrap],
            )
            .await
            .map_err(be)?;
        }
        tx.commit().await.map_err(be)
    }

    async fn key_wrap(&self, ws: &str, device_id: &str) -> RepoResult<Option<(u32, Vec<u8>)>> {
        let c = self.conn().await?;
        Ok(c.query_opt(
            "select epoch, wrap from key_wraps where workspace_id = $1 and device_id = $2 order by epoch desc limit 1",
            &[&ws, &device_id],
        )
        .await
        .map_err(be)?
        .map(|r| (r.get::<_, i32>(0) as u32, r.get(1))))
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
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let w = tx
            .query_opt(&format!("{WORKSPACE_SQL} for update of w"), &[&ws])
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?;
        let row = workspace_from(ws, &w);
        let existing = tx
            .query_opt(
                "select revoked_at from devices where workspace_id = $1 and id = $2",
                &[&ws, &device_id],
            )
            .await
            .map_err(be)?;
        match existing {
            Some(r) => {
                if r.get::<_, Option<i64>>(0).is_some() {
                    return Err(RepoError::Conflict("device was revoked"));
                }
            }
            None => {
                if row.usage.devices >= max_devices {
                    return Err(RepoError::Quota("device limit reached"));
                }
                tx.execute(
                    "insert into devices (workspace_id, id, key, label_sealed, created_at, last_seen) values ($1, $2, $3, $4, $5, $5)",
                    &[&ws, &device_id, &device_key, &label_sealed, &now],
                )
                .await
                .map_err(be)?;
            }
        }
        tx.commit().await.map_err(be)?;
        Ok((row.epoch, row.recovery_wrap))
    }

    async fn push_ops(
        &self,
        ws: &str,
        device_id: &str,
        ops: &[PushOp],
        now: i64,
        limits: &Limits,
    ) -> RepoResult<PushResponse> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let w = tx
            .query_opt(&format!("{WORKSPACE_SQL} for update of w"), &[&ws])
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?;
        let row = workspace_from(ws, &w);
        let ids: Vec<&str> = ops.iter().map(|o| o.op_id.as_str()).collect();
        let existing: HashSet<String> = tx
            .query(
                "select op_id from ops where workspace_id = $1 and op_id = any($2)",
                &[&ws, &ids],
            )
            .await
            .map_err(be)?
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect();
        let mut seen = HashSet::new();
        let mut fresh: Vec<&PushOp> = Vec::new();
        let mut duplicate = Vec::new();
        let mut added = 0i64;
        for op in ops {
            check_op(op, row.epoch)?;
            if existing.contains(&op.op_id) || !seen.insert(op.op_id.as_str()) {
                duplicate.push(op.op_id.clone());
            } else {
                added += op.payload.len() as i64;
                fresh.push(op);
            }
        }
        if row.usage.op_bytes + added > limits.max_op_bytes_total {
            return Err(RepoError::Quota("operation storage quota exceeded"));
        }
        let insert = tx
            .prepare("insert into ops (workspace_id, seq, op_id, device_id, doc_id, epoch, format, payload, created_at) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
            .await
            .map_err(be)?;
        let mut seq = row.head;
        let mut accepted = Vec::new();
        for op in &fresh {
            seq += 1;
            tx.execute(
                &insert,
                &[
                    &ws,
                    &seq,
                    &op.op_id,
                    &device_id,
                    &op.doc_id,
                    &(op.epoch as i32),
                    &(op.format as i32),
                    &op.payload,
                    &now,
                ],
            )
            .await
            .map_err(be)?;
            accepted.push(op.op_id.clone());
        }
        if !fresh.is_empty() {
            tx.execute(
                "update workspaces set head = $2, op_bytes = op_bytes + $3, op_count = op_count + $4 where id = $1",
                &[&ws, &seq, &added, &(fresh.len() as i64)],
            )
            .await
            .map_err(be)?;
        }
        tx.commit().await.map_err(be)?;
        Ok(PushResponse {
            accepted,
            duplicate,
            cursor: seq,
            epoch: row.epoch,
        })
    }

    async fn pull_ops(&self, ws: &str, after: i64, limit: i64) -> RepoResult<PullResponse> {
        let c = self.conn().await?;
        let w = c
            .query_opt("select epoch from workspaces where id = $1", &[&ws])
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?;
        let epoch = w.get::<_, i32>(0) as u32;
        let take = limit.max(1);
        let rows = c
            .query(
                "select seq, op_id, device_id, doc_id, epoch, format, payload from ops \
                 where workspace_id = $1 and seq > $2 order by seq limit $3",
                &[&ws, &after, &(take + 1)],
            )
            .await
            .map_err(be)?;
        let more = rows.len() as i64 > take;
        let ops: Vec<PulledOp> = rows
            .iter()
            .take(take as usize)
            .map(|r| PulledOp {
                seq: r.get(0),
                op_id: r.get(1),
                device_id: r.get(2),
                doc_id: r.get(3),
                epoch: r.get::<_, i32>(4) as u32,
                format: r.get::<_, i32>(5) as u32,
                payload: r.get(6),
            })
            .collect();
        let cursor = ops.last().map_or(after, |o| o.seq);
        Ok(PullResponse {
            ops,
            cursor,
            more,
            epoch,
        })
    }

    async fn blob(&self, ws: &str, blob_id: &str) -> RepoResult<Option<BlobMeta>> {
        let c = self.conn().await?;
        let Some(b) = c
            .query_opt(
                "select complete, total, epoch, size from blobs where workspace_id = $1 and blob_id = $2",
                &[&ws, &blob_id],
            )
            .await
            .map_err(be)?
        else {
            return Ok(None);
        };
        let chunks = c
            .query(
                "select idx, size from blob_chunks where workspace_id = $1 and blob_id = $2 order by idx",
                &[&ws, &blob_id],
            )
            .await
            .map_err(be)?
            .iter()
            .map(|r| (r.get::<_, i32>(0) as u32, r.get::<_, i64>(1)))
            .collect();
        Ok(Some(BlobMeta {
            complete: b.get(0),
            total: b.get::<_, i32>(1) as u32,
            epoch: b.get::<_, i32>(2) as u32,
            size: b.get(3),
            chunks,
        }))
    }

    async fn reserve_chunk(
        &self,
        ws: &str,
        blob_id: &str,
        index: u32,
        size: i64,
        now: i64,
        limits: &Limits,
    ) -> RepoResult<()> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let used: i64 = tx
            .query_opt(
                "select blob_bytes from workspaces where id = $1 for update",
                &[&ws],
            )
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?
            .get(0);
        tx.execute(
            "insert into blobs (workspace_id, blob_id, created_at) values ($1, $2, $3) on conflict do nothing",
            &[&ws, &blob_id, &now],
        )
        .await
        .map_err(be)?;
        let complete: bool = tx
            .query_one(
                "select complete from blobs where workspace_id = $1 and blob_id = $2 for update",
                &[&ws, &blob_id],
            )
            .await
            .map_err(be)?
            .get(0);
        if complete {
            return Err(RepoError::Conflict("blob already complete"));
        }
        let idx = index as i32;
        let previous: i64 = tx
            .query_opt(
                "select size from blob_chunks where workspace_id = $1 and blob_id = $2 and idx = $3",
                &[&ws, &blob_id, &idx],
            )
            .await
            .map_err(be)?
            .map_or(0, |r| r.get(0));
        let sum: i64 = tx
            .query_one(
                "select coalesce(sum(size), 0)::bigint from blob_chunks where workspace_id = $1 and blob_id = $2",
                &[&ws, &blob_id],
            )
            .await
            .map_err(be)?
            .get(0);
        let delta = size - previous;
        if sum + delta > limits.max_blob_size {
            return Err(RepoError::Quota("blob exceeds the size limit"));
        }
        if used + delta > limits.max_blob_bytes_total {
            return Err(RepoError::Quota("attachment storage quota exceeded"));
        }
        tx.execute(
            "insert into blob_chunks (workspace_id, blob_id, idx, size) values ($1, $2, $3, $4) \
             on conflict (workspace_id, blob_id, idx) do update set size = excluded.size",
            &[&ws, &blob_id, &idx, &size],
        )
        .await
        .map_err(be)?;
        tx.execute(
            "update workspaces set blob_bytes = blob_bytes + $2 where id = $1",
            &[&ws, &delta],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)
    }

    async fn remove_chunk(&self, ws: &str, blob_id: &str, index: u32) -> RepoResult<()> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let idx = index as i32;
        let complete = tx
            .query_opt(
                "select complete from blobs where workspace_id = $1 and blob_id = $2 for update",
                &[&ws, &blob_id],
            )
            .await
            .map_err(be)?
            .map(|r| r.get::<_, bool>(0));
        if complete == Some(false) {
            let removed = tx
                .query_opt(
                    "delete from blob_chunks where workspace_id = $1 and blob_id = $2 and idx = $3 returning size",
                    &[&ws, &blob_id, &idx],
                )
                .await
                .map_err(be)?;
            if let Some(r) = removed {
                let n: i64 = r.get(0);
                tx.execute(
                    "update workspaces set blob_bytes = blob_bytes - $2 where id = $1",
                    &[&ws, &n],
                )
                .await
                .map_err(be)?;
            }
            tx.execute(
                "delete from blobs b where workspace_id = $1 and blob_id = $2 and not exists \
                 (select 1 from blob_chunks c where c.workspace_id = b.workspace_id and c.blob_id = b.blob_id)",
                &[&ws, &blob_id],
            )
            .await
            .map_err(be)?;
        }
        tx.commit().await.map_err(be)
    }

    async fn commit_blob(&self, ws: &str, blob_id: &str, cm: &CommitBlob) -> RepoResult<()> {
        let mut c = self.conn().await?;
        let tx = c.transaction().await.map_err(be)?;
        let epoch: i32 = tx
            .query_opt("select epoch from workspaces where id = $1", &[&ws])
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?
            .get(0);
        if cm.epoch as i32 > epoch {
            return Err(RepoError::Conflict("unknown key epoch"));
        }
        let b = tx
            .query_opt(
                "select complete from blobs where workspace_id = $1 and blob_id = $2 for update",
                &[&ws, &blob_id],
            )
            .await
            .map_err(be)?
            .ok_or(RepoError::NotFound)?;
        if b.get::<_, bool>(0) {
            return Ok(());
        }
        let chunks = tx
            .query("select idx, size from blob_chunks where workspace_id = $1 and blob_id = $2 order by idx", &[&ws, &blob_id])
            .await
            .map_err(be)?;
        let contiguous = cm.total > 0
            && chunks.len() == cm.total as usize
            && chunks
                .iter()
                .enumerate()
                .all(|(i, r)| r.get::<_, i32>(0) as usize == i);
        if !contiguous {
            return Err(RepoError::Invalid("chunks are missing"));
        }
        if chunks.iter().map(|r| r.get::<_, i64>(1)).sum::<i64>() != cm.size {
            return Err(RepoError::Invalid("blob size does not match its chunks"));
        }
        tx.execute(
            "update blobs set complete = true, total = $3, epoch = $4, size = $5 where workspace_id = $1 and blob_id = $2",
            &[&ws, &blob_id, &(cm.total as i32), &(cm.epoch as i32), &cm.size],
        )
        .await
        .map_err(be)?;
        tx.commit().await.map_err(be)
    }

    async fn purge_expired(&self, now: i64) -> RepoResult<()> {
        let c = self.conn().await?;
        c.execute("delete from nonces where expires_at <= $1", &[&now])
            .await
            .map_err(be)?;
        c.execute("delete from invites where expires_at + 3600 <= $1", &[&now])
            .await
            .map_err(be)?;
        Ok(())
    }
}

fn tls_connector(
    extra_ca: Option<&[u8]>,
) -> Result<tokio_postgres_rustls::MakeRustlsConnect, String> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(pem) = extra_ca {
        for cert in CertificateDer::pem_slice_iter(pem) {
            let cert = cert.map_err(|e| format!("invalid CA certificate: {e}"))?;
            roots
                .add(cert)
                .map_err(|e| format!("invalid CA certificate: {e}"))?;
        }
    }
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}
