//! HTTP handlers.

use crate::auth::{authenticate, parse_signed, ApiErr};
use crate::config::Registration;
use crate::metrics::Metrics;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use notercrux_crypto::{recovery_message, verify_signature, DevicePublic};
use notercrux_protocol::*;
use serde::Deserialize;
use std::time::Duration;

type ApiResult<T> = Result<T, ApiErr>;

const MAX_LABEL: usize = 4096;
const MAX_WRAP: usize = 64 * 1024;
const MAX_BATCH: usize = 256;
const MAX_PULL_BYTES: usize = 8 * 1024 * 1024;
const MAX_CHUNKS: u32 = 10_000;

fn valid_id(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn valid_op_id(s: &str) -> bool {
    (1..=80).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
}

fn json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> ApiResult<T> {
    serde_json::from_slice(body).map_err(|e| ApiErr::bad(format!("invalid request body: {e}")))
}

fn ws_id(ws: &str) -> ApiResult<()> {
    if valid_id(ws, 8, 64) {
        Ok(())
    } else {
        Err(ApiErr::bad("invalid workspace id"))
    }
}

async fn info_of(state: &AppState, ws: &str) -> ApiResult<WorkspaceInfo> {
    let row = state
        .repo
        .workspace(ws)
        .await?
        .ok_or_else(ApiErr::not_found)?;
    Ok(WorkspaceInfo {
        workspace_id: ws.to_string(),
        epoch: row.epoch,
        devices: state.repo.devices(ws).await?,
        usage: row.usage,
        limits: state.entitlements.limits(ws),
        protocol: PROTOCOL_VERSION,
        recovery_key: row.recovery_key,
    })
}

// ---- workspaces -------------------------------------------------------------------------------

pub async fn create_workspace(
    State(st): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<WorkspaceInfo>)> {
    let req: CreateWorkspace = json(&body)?;
    ws_id(&req.workspace_id)?;
    match &st.config.registration {
        Registration::Open => {}
        Registration::Closed => {
            return Err(ApiErr::new(
                StatusCode::FORBIDDEN,
                code::FORBIDDEN,
                "this server is not accepting new workspaces",
            ))
        }
        Registration::Tokens(tokens) => {
            let ok = req.registration_token.as_deref().is_some_and(|t| {
                tokens
                    .iter()
                    .any(|x| notercrux_crypto::constant_time_eq(x.as_bytes(), t.as_bytes()))
            });
            if !ok {
                return Err(ApiErr::new(
                    StatusCode::FORBIDDEN,
                    code::FORBIDDEN,
                    "a valid registration token is required",
                ));
            }
        }
    }
    let public =
        DevicePublic::from_bytes(&req.device_key).map_err(|_| ApiErr::bad("invalid device key"))?;
    if DevicePublic::from_bytes(&req.recovery_key).is_err()
        || req.label_sealed.is_empty()
        || req.label_sealed.len() > MAX_LABEL
        || req.recovery_wrap.is_empty()
        || req.recovery_wrap.len() > MAX_WRAP
    {
        return Err(ApiErr::bad("invalid workspace parameters"));
    }
    // The creating device proves it holds the private key by signing this very request.
    let signed = parse_signed(&headers, &st)?;
    if signed.device_id != public.id() || !signed.verify(&req.device_key, &method, &uri, &body) {
        return Err(ApiErr::unauthorized());
    }
    st.repo
        .create_workspace(&req, &public.id(), st.now())
        .await?;
    Metrics::add(&st.metrics.workspaces_created, 1);
    Ok((
        StatusCode::CREATED,
        Json(info_of(&st, &req.workspace_id).await?),
    ))
}

pub async fn workspace_info(
    State(st): State<AppState>,
    Path(ws): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<WorkspaceInfo>> {
    ws_id(&ws)?;
    authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    Ok(Json(info_of(&st, &ws).await?))
}

pub async fn delete_workspace(
    State(st): State<AppState>,
    Path(ws): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    ws_id(&ws)?;
    authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    st.repo.delete_workspace(&ws).await?;
    st.blobs.delete_workspace(&ws).await.map_err(io_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- enrollment -------------------------------------------------------------------------------

pub async fn create_invite(
    State(st): State<AppState>,
    Path(ws): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<CreateInviteResponse>)> {
    ws_id(&ws)?;
    let who = authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    let now = st.now();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let expires_at = now + st.config.invite_ttl;
    st.repo
        .create_invite(&ws, &id, &who.device.id, expires_at, now)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateInviteResponse {
            invite_id: id,
            expires_at,
        }),
    ))
}

pub async fn invite_state(
    State(st): State<AppState>,
    Path((ws, id)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<InviteState>> {
    ws_id(&ws)?;
    authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    let inv = st
        .repo
        .invite(&id)
        .await?
        .filter(|i| i.workspace_id == ws)
        .ok_or_else(ApiErr::not_found)?;
    let status = if inv.approved_device.is_some() {
        InviteStatus::Approved
    } else if inv.expires_at <= st.now() {
        InviteStatus::Expired
    } else if inv.request.is_some() {
        InviteStatus::Requested
    } else {
        InviteStatus::Pending
    };
    Ok(Json(InviteState {
        status,
        request: inv.request,
    }))
}

pub async fn approve_invite(
    State(st): State<AppState>,
    Path((ws, id)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    ws_id(&ws)?;
    authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    let a: ApproveInvite = json(&body)?;
    if a.key_wrap.len() > MAX_WRAP || a.label_sealed.len() > MAX_LABEL {
        return Err(ApiErr::bad("oversized enrollment package"));
    }
    let limits = st.entitlements.limits(&ws);
    let epoch = st
        .repo
        .approve_invite(&ws, &id, &a, st.now(), limits.max_devices)
        .await?;
    Ok(Json(serde_json::json!({ "epoch": epoch })))
}

/// Called by the new device (not yet enrolled), so it is unauthenticated: the invite secret that
/// `proof` is derived from is what the approving device checks.
pub async fn invite_request(
    State(st): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    let req: InviteRequest = json(&body)?;
    ws_id(&req.workspace_id)?;
    let public =
        DevicePublic::from_bytes(&req.device_key).map_err(|_| ApiErr::bad("invalid device key"))?;
    if req.proof.len() != 32
        || req.label_sealed.is_empty()
        || req.label_sealed.len() > MAX_LABEL
        || !valid_id(&id, 8, 64)
    {
        return Err(ApiErr::bad("invalid enrollment request"));
    }
    st.repo
        .submit_invite_request(&id, &req, &public.id(), st.now())
        .await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
pub struct ResultQuery {
    device: String,
}

pub async fn invite_result(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ResultQuery>,
) -> ApiResult<Json<InviteResult>> {
    if !valid_id(&id, 8, 64) || q.device.len() > 64 {
        return Err(ApiErr::bad("invalid invite"));
    }
    Ok(Json(
        st.repo
            .invite_result(&id, &q.device)
            .await?
            .ok_or_else(ApiErr::not_found)?,
    ))
}

// ---- revocation, keys, recovery ---------------------------------------------------------------

pub async fn revoke_device(
    State(st): State<AppState>,
    Path((ws, device)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    ws_id(&ws)?;
    authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    let r: RevokeDevice = json(&body)?;
    if r.wraps.iter().any(|w| w.key_wrap.len() > MAX_WRAP) || r.recovery_wrap.len() > MAX_WRAP {
        return Err(ApiErr::bad("oversized key wrap"));
    }
    st.repo.revoke_device(&ws, &device, &r, st.now()).await?;
    Metrics::add(&st.metrics.devices_revoked, 1);
    st.hub.notify(&ws);
    Ok(Json(serde_json::json!({ "epoch": r.new_epoch })))
}

#[derive(Deserialize)]
pub struct KeysQuery {
    #[serde(default)]
    have: Option<u32>,
}

pub async fn keys(
    State(st): State<AppState>,
    Path(ws): Path<String>,
    Query(q): Query<KeysQuery>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> ApiResult<Json<KeysResponse>> {
    ws_id(&ws)?;
    let who = authenticate(&st, &ws, &method, &uri, &headers, &[]).await?;
    let wrap = st.repo.key_wrap(&ws, &who.device.id).await?;
    let have = q.have;
    let key_wrap = wrap.and_then(|(epoch, w)| (have.is_none_or(|h| epoch > h)).then_some(w));
    Ok(Json(KeysResponse {
        epoch: who.workspace.epoch,
        key_wrap,
    }))
}

pub async fn recover(State(st): State<AppState>, body: Bytes) -> ApiResult<Json<RecoverResponse>> {
    let req: RecoverRequest = json(&body)?;
    ws_id(&req.workspace_id)?;
    let public =
        DevicePublic::from_bytes(&req.device_key).map_err(|_| ApiErr::bad("invalid device key"))?;
    if req.label_sealed.is_empty() || req.label_sealed.len() > MAX_LABEL {
        return Err(ApiErr::bad("invalid recovery request"));
    }
    let ws = st
        .repo
        .workspace(&req.workspace_id)
        .await?
        .ok_or_else(ApiErr::unauthorized)?;
    let auth: [u8; 32] = ws
        .recovery_key
        .get(..32)
        .and_then(|k| k.try_into().ok())
        .ok_or_else(ApiErr::unauthorized)?;
    let device_id = public.id();
    if !verify_signature(
        &auth,
        &recovery_message(&req.workspace_id, &device_id),
        &req.recovery_signature,
    ) {
        Metrics::add(&st.metrics.auth_failures, 1);
        return Err(ApiErr::unauthorized());
    }
    let limits = st.entitlements.limits(&req.workspace_id);
    let (epoch, recovery_wrap) = st
        .repo
        .recover_device(
            &req.workspace_id,
            &req.device_key,
            &device_id,
            &req.label_sealed,
            st.now(),
            limits.max_devices,
        )
        .await?;
    Ok(Json(RecoverResponse {
        epoch,
        recovery_wrap,
    }))
}

// ---- operations -------------------------------------------------------------------------------

pub async fn push_ops(
    State(st): State<AppState>,
    Path(ws): Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<PushResponse>> {
    ws_id(&ws)?;
    let who = authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    let req: PushRequest = json(&body)?;
    let limits = st.entitlements.limits(&ws);
    if req.ops.is_empty() || req.ops.len() > MAX_BATCH {
        return Err(ApiErr::bad(format!(
            "send between 1 and {MAX_BATCH} operations per request"
        )));
    }
    for op in &req.ops {
        if !valid_op_id(&op.op_id) || !valid_id(&op.doc_id, 8, 128) || op.payload.is_empty() {
            return Err(ApiErr::bad("invalid operation"));
        }
        if op.payload.len() as i64 > limits.max_op_size {
            return Err(ApiErr::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                code::TOO_LARGE,
                "operation is too large",
            ));
        }
    }
    let res = st
        .repo
        .push_ops(&ws, &who.device.id, &req.ops, st.now(), &limits)
        .await?;
    Metrics::add(&st.metrics.ops_pushed, res.accepted.len() as u64);
    Metrics::add(&st.metrics.ops_duplicate, res.duplicate.len() as u64);
    let bytes: usize = req
        .ops
        .iter()
        .filter(|o| res.accepted.contains(&o.op_id))
        .map(|o| o.payload.len())
        .sum();
    Metrics::add(&st.metrics.op_bytes_in, bytes as u64);
    if !res.accepted.is_empty() {
        st.hub.notify(&ws);
    }
    Ok(Json(res))
}

#[derive(Deserialize)]
pub struct PullQuery {
    #[serde(default)]
    after: i64,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    wait: Option<u64>,
}

pub async fn pull_ops(
    State(st): State<AppState>,
    Path(ws): Path<String>,
    Query(q): Query<PullQuery>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> ApiResult<Json<PullResponse>> {
    ws_id(&ws)?;
    authenticate(&st, &ws, &method, &uri, &headers, &[]).await?;
    let limit = q.limit.unwrap_or(500).clamp(1, 1000);
    let wait = Duration::from_secs(q.wait.unwrap_or(0).min(st.config.long_poll_max));
    let deadline = tokio::time::Instant::now() + wait;
    let notify = st.hub.channel(&ws);
    let mut res = loop {
        let wake = notify.notified();
        tokio::pin!(wake);
        wake.as_mut().enable();
        let res = st.repo.pull_ops(&ws, q.after.max(0), limit).await?;
        if !res.ops.is_empty() || wait.is_zero() || tokio::time::Instant::now() >= deadline {
            break res;
        }
        // Other server instances share the database but not this wake-up, so also re-check periodically.
        let step = deadline.min(tokio::time::Instant::now() + Duration::from_secs(2));
        let _ = tokio::time::timeout_at(step, wake).await;
    };
    // Keep responses bounded: trim the batch once it carries enough bytes (always at least one op).
    let mut total = 0usize;
    let keep = res
        .ops
        .iter()
        .position(|o| {
            total += o.payload.len();
            total > MAX_PULL_BYTES
        })
        .map_or(res.ops.len(), |i| i.max(1));
    if keep < res.ops.len() {
        res.ops.truncate(keep);
        res.more = true;
    }
    res.cursor = res.ops.last().map_or(q.after.max(0), |o| o.seq);
    Metrics::add(&st.metrics.ops_pulled, res.ops.len() as u64);
    Ok(Json(res))
}

// ---- attachments ------------------------------------------------------------------------------

fn io_err(e: std::io::Error) -> ApiErr {
    tracing::error!("blob store failure: {e}");
    ApiErr::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        code::INTERNAL,
        "internal error",
    )
}

fn blob_args(ws: &str, blob: &str) -> ApiResult<()> {
    ws_id(ws)?;
    if valid_id(blob, 16, 128) {
        Ok(())
    } else {
        Err(ApiErr::bad("invalid attachment id"))
    }
}

pub async fn put_chunk(
    State(st): State<AppState>,
    Path((ws, blob, index)): Path<(String, String, u32)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<serde_json::Value>> {
    blob_args(&ws, &blob)?;
    authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    if index >= MAX_CHUNKS || body.is_empty() {
        return Err(ApiErr::bad("invalid chunk"));
    }
    if let Some(existing) = st.repo.blob(&ws, &blob).await? {
        if existing.complete {
            // Content-addressed and already stored: an interrupted client finishing a retry.
            return Ok(Json(serde_json::json!({ "complete": true })));
        }
    }
    let limits = st.entitlements.limits(&ws);
    st.repo
        .reserve_chunk(&ws, &blob, index, body.len() as i64, st.now(), &limits)
        .await?;
    if let Err(e) = st.blobs.put(&ws, &blob, index, &body).await {
        let _ = st.repo.remove_chunk(&ws, &blob, index).await;
        return Err(io_err(e));
    }
    Metrics::add(&st.metrics.blob_chunks_in, 1);
    Metrics::add(&st.metrics.blob_bytes_in, body.len() as u64);
    Ok(Json(serde_json::json!({ "complete": false })))
}

pub async fn blob_status(
    State(st): State<AppState>,
    Path((ws, blob)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> ApiResult<Json<BlobStatus>> {
    blob_args(&ws, &blob)?;
    authenticate(&st, &ws, &method, &uri, &headers, &[]).await?;
    let m = st
        .repo
        .blob(&ws, &blob)
        .await?
        .ok_or_else(ApiErr::not_found)?;
    Ok(Json(BlobStatus {
        complete: m.complete,
        total: m.total,
        epoch: m.epoch,
        size: m.size,
        present: m.chunks.iter().map(|(i, _)| *i).collect(),
    }))
}

pub async fn get_chunk(
    State(st): State<AppState>,
    Path((ws, blob, index)): Path<(String, String, u32)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> ApiResult<Response> {
    blob_args(&ws, &blob)?;
    authenticate(&st, &ws, &method, &uri, &headers, &[]).await?;
    st.repo
        .blob(&ws, &blob)
        .await?
        .ok_or_else(ApiErr::not_found)?;
    let data = st
        .blobs
        .get(&ws, &blob, index)
        .await
        .map_err(io_err)?
        .ok_or_else(ApiErr::not_found)?;
    Metrics::add(&st.metrics.blob_chunks_out, 1);
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data).into_response())
}

pub async fn commit_blob(
    State(st): State<AppState>,
    Path((ws, blob)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    blob_args(&ws, &blob)?;
    authenticate(&st, &ws, &method, &uri, &headers, &body).await?;
    let c: CommitBlob = json(&body)?;
    if c.total == 0 || c.total > MAX_CHUNKS {
        return Err(ApiErr::bad("invalid chunk count"));
    }
    for i in 0..c.total {
        if !st.blobs.has(&ws, &blob, i).await.map_err(io_err)? {
            return Err(ApiErr::bad("chunks are missing"));
        }
    }
    st.repo.commit_blob(&ws, &blob, &c).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- operations endpoints ---------------------------------------------------------------------

pub async fn healthz(State(st): State<AppState>) -> impl IntoResponse {
    match st.repo.purge_expired(0).await {
        Ok(()) => (StatusCode::OK, "ok"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "storage unavailable"),
    }
}

pub async fn metrics(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = &st.config.metrics_token {
        let given = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        if !given
            .is_some_and(|g| notercrux_crypto::constant_time_eq(g.as_bytes(), token.as_bytes()))
        {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        st.metrics.render(),
    )
        .into_response()
}
