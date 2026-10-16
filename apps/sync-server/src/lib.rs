//! Notercrux sync server.
//!
//! The server stores and relays **ciphertext only**. It authenticates devices by their signatures,
//! orders encrypted operations per workspace, stores encrypted attachment chunks, and enforces
//! quotas. It never holds a key that can decrypt content.

pub mod api;
pub mod auth;
pub mod blobstore;
pub mod config;
pub mod mem;
pub mod metrics;
pub mod pg;
pub mod repo;

use axum::extract::{ConnectInfo, DefaultBodyLimit, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Router;
use blobstore::{BlobStore, FsBlobStore, MemBlobStore};
use config::{Config, Entitlements, StaticEntitlements};
use metrics::{Limiter, Metrics};
use notercrux_protocol::{code, headers as h, ApiError, PROTOCOL_VERSION};
use repo::Repo;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Wakes long-polling pullers when new operations arrive on this instance.
#[derive(Default)]
pub struct Hub {
    channels: Mutex<HashMap<String, Arc<Notify>>>,
}

impl Hub {
    pub fn channel(&self, ws: &str) -> Arc<Notify> {
        self.channels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(ws.to_string())
            .or_default()
            .clone()
    }

    pub fn notify(&self, ws: &str) {
        self.channel(ws).notify_waiters();
    }
}

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone)]
pub struct AppState {
    pub repo: Arc<dyn Repo>,
    pub blobs: Arc<dyn BlobStore>,
    pub entitlements: Arc<dyn Entitlements>,
    pub config: Arc<Config>,
    pub metrics: Arc<Metrics>,
    pub limiter: Arc<Limiter>,
    pub hub: Arc<Hub>,
    pub clock: Clock,
}

impl AppState {
    pub fn new(config: Config, repo: Arc<dyn Repo>, blobs: Arc<dyn BlobStore>) -> Self {
        let limits = config.limits.clone();
        Self {
            repo,
            blobs,
            entitlements: Arc::new(StaticEntitlements(limits)),
            config: Arc::new(config),
            metrics: Arc::default(),
            limiter: Arc::default(),
            hub: Arc::default(),
            clock: Arc::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64)
            }),
        }
    }

    /// In-memory repository and blob store.
    pub fn in_memory(config: Config) -> Self {
        Self::new(
            config,
            Arc::new(mem::MemRepo::new()),
            Arc::new(MemBlobStore::new()),
        )
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }
}

/// Builds the state described by the configuration (PostgreSQL when a URL is set).
pub async fn build_state(config: Config) -> Result<AppState, String> {
    let blobs: Arc<dyn BlobStore> = match &config.blob_dir {
        Some(dir) => Arc::new(FsBlobStore::new(dir.clone())),
        None => Arc::new(MemBlobStore::new()),
    };
    let repo: Arc<dyn Repo> = match &config.database_url {
        Some(url) => {
            let ca = match &config.database_ca {
                Some(path) => Some(
                    std::fs::read(path)
                        .map_err(|e| format!("cannot read {}: {e}", path.display()))?,
                ),
                None => None,
            };
            let pg = pg::PgRepo::connect_with(url, ca.as_deref())?;
            pg.migrate().await.map_err(|e| e.to_string())?;
            Arc::new(pg)
        }
        None => Arc::new(mem::MemRepo::new()),
    };
    Ok(AppState::new(config, repo, blobs))
}

async fn edge(State(st): State<AppState>, req: Request, next: Next) -> Response {
    Metrics::add(&st.metrics.requests, 1);
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(|| "local".to_string(), |c| c.0.ip().to_string());
    if !st
        .limiter
        .allow(&format!("ip:{ip}"), st.config.ip_rate, st.now())
    {
        Metrics::add(&st.metrics.rate_limited, 1);
        return auth::ApiErr::new(
            StatusCode::TOO_MANY_REQUESTS,
            code::RATE_LIMITED,
            "too many requests",
        )
        .into_response();
    }
    if let Err(e) = auth::check_protocol(req.headers()) {
        return e.into_response();
    }
    let mut res = next.run(req).await;
    if res.status().is_server_error() {
        Metrics::add(&st.metrics.server_errors, 1);
    }
    res.headers_mut()
        .insert(h::PROTOCOL, HeaderValue::from(PROTOCOL_VERSION));
    res
}

async fn fallback() -> Response {
    auth::ApiErr::not_found().into_response()
}

pub fn router(state: AppState) -> Router {
    let w = "/v1/workspaces/{ws}";
    Router::new()
        .route("/healthz", get(api::healthz))
        .route("/metrics", get(api::metrics))
        .route("/v1/workspaces", post(api::create_workspace))
        .route(w, get(api::workspace_info).delete(api::delete_workspace))
        .route(&format!("{w}/invites"), post(api::create_invite))
        .route(&format!("{w}/invites/{{id}}"), get(api::invite_state))
        .route(
            &format!("{w}/invites/{{id}}/approve"),
            post(api::approve_invite),
        )
        .route("/v1/invites/{id}/request", post(api::invite_request))
        .route("/v1/invites/{id}/result", get(api::invite_result))
        .route(
            &format!("{w}/devices/{{device}}/revoke"),
            post(api::revoke_device),
        )
        .route(&format!("{w}/keys"), get(api::keys))
        .route("/v1/recover", post(api::recover))
        .route(&format!("{w}/ops"), post(api::push_ops).get(api::pull_ops))
        .route(&format!("{w}/blobs/{{blob}}"), get(api::blob_status))
        .route(
            &format!("{w}/blobs/{{blob}}/chunks/{{index}}"),
            put(api::put_chunk).get(api::get_chunk),
        )
        .route(
            &format!("{w}/blobs/{{blob}}/commit"),
            post(api::commit_blob),
        )
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(state.config.max_body))
        .layer(middleware::from_fn_with_state(state.clone(), edge))
        .with_state(state)
}

/// Starts the server and the background janitor. Returns when the server stops.
pub async fn serve(config: Config) -> Result<(), String> {
    let bind = config.bind;
    let state = build_state(config).await?;
    let janitor = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            if let Err(e) = janitor.repo.purge_expired(janitor.now()).await {
                tracing::warn!("cleanup failed: {e}");
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| format!("cannot listen on {bind}: {e}"))?;
    tracing::info!("notercrux sync server listening on {bind}");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .map_err(|e| e.to_string())
}

#[allow(dead_code)]
fn _error_shape(_: ApiError) {}
