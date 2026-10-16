#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use notercrux_crypto::{sign_request, wrap_for_recovery, DeviceKeys, KeyRing, RecoveryKey};
use notercrux_protocol::*;
use notercrux_sync_server::config::Config;
use notercrux_sync_server::{router, AppState};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

pub const T0: i64 = 1_800_000_000;

pub struct TestApp {
    pub state: AppState,
    pub router: Router,
    pub clock: Arc<AtomicI64>,
    nonce: AtomicU64,
}

pub struct Reply {
    pub status: StatusCode,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&self.body)))
    }

    pub fn error_code(&self) -> String {
        self.json::<ApiError>().error
    }
}

impl TestApp {
    /// In-memory by default; set NOTERCRUX_TEST_DATABASE_URL to run the same tests on PostgreSQL.
    pub fn new(config: Config) -> Self {
        match std::env::var("NOTERCRUX_TEST_DATABASE_URL") {
            Ok(url) if !url.is_empty() => {
                let repo = notercrux_sync_server::pg::PgRepo::connect(&url).expect("database URL");
                let blobs = Arc::new(notercrux_sync_server::blobstore::MemBlobStore::new());
                Self::with_state(AppState::new(config, Arc::new(repo), blobs))
            }
            _ => Self::with_state(AppState::in_memory(config)),
        }
    }

    pub fn with_state(mut state: AppState) -> Self {
        let clock = Arc::new(AtomicI64::new(T0));
        let c = clock.clone();
        state.clock = Arc::new(move || c.load(Ordering::SeqCst));
        Self {
            router: router(state.clone()),
            state,
            clock,
            nonce: AtomicU64::new(1),
        }
    }

    pub fn advance(&self, secs: i64) {
        self.clock.fetch_add(secs, Ordering::SeqCst);
    }

    pub fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst)
    }

    pub fn fresh_nonce(&self) -> String {
        format!(
            "n{}-{}",
            self.nonce.fetch_add(1, Ordering::SeqCst),
            &uuid::Uuid::new_v4().simple().to_string()[..16]
        )
    }

    /// Sends a request signed by `keys` (or unsigned when `None`).
    pub async fn send(
        &self,
        keys: Option<&DeviceKeys>,
        method: Method,
        target: &str,
        body: Vec<u8>,
    ) -> Reply {
        let nonce = self.fresh_nonce();
        self.send_with(keys, method, target, body, self.now(), &nonce, None)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_with(
        &self,
        keys: Option<&DeviceKeys>,
        method: Method,
        target: &str,
        body: Vec<u8>,
        time: i64,
        nonce: &str,
        protocol: Option<u32>,
    ) -> Reply {
        let mut req = Request::builder().method(method.clone()).uri(target);
        if let Some(k) = keys {
            let sig = sign_request(k, method.as_str(), target, time, nonce, &body);
            req = req
                .header(headers::DEVICE, k.id())
                .header(headers::TIME, time.to_string())
                .header(headers::NONCE, nonce)
                .header(headers::SIGNATURE, encode_b64(&sig));
        }
        if let Some(p) = protocol {
            req = req.header(headers::PROTOCOL, p.to_string());
        }
        let res = self
            .router
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        Reply { status, body }
    }

    pub async fn post<T: Serialize>(&self, keys: &DeviceKeys, target: &str, body: &T) -> Reply {
        self.send(
            Some(keys),
            Method::POST,
            target,
            serde_json::to_vec(body).unwrap(),
        )
        .await
    }

    pub async fn get(&self, keys: &DeviceKeys, target: &str) -> Reply {
        self.send(Some(keys), Method::GET, target, Vec::new()).await
    }
}

pub struct Workspace {
    pub id: String,
    pub owner: DeviceKeys,
    pub ring: KeyRing,
    pub recovery: RecoveryKey,
}

pub fn ws_name(tag: &str) -> String {
    format!(
        "ws-{tag}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

pub fn create_request(
    id: &str,
    owner: &DeviceKeys,
    ring: &KeyRing,
    recovery: &RecoveryKey,
    token: Option<&str>,
) -> CreateWorkspace {
    CreateWorkspace {
        workspace_id: id.to_string(),
        device_key: owner.public().to_bytes().to_vec(),
        label_sealed: vec![7; 40],
        recovery_key: recovery.public().to_bytes().to_vec(),
        recovery_wrap: wrap_for_recovery(ring, &recovery.public(), id).unwrap(),
        registration_token: token.map(str::to_string),
    }
}

pub async fn make_workspace(app: &TestApp, tag: &str) -> Workspace {
    let id = ws_name(tag);
    let owner = DeviceKeys::generate().unwrap();
    let ring = KeyRing::generate().unwrap();
    let recovery = RecoveryKey::generate().unwrap();
    let req = create_request(&id, &owner, &ring, &recovery, None);
    let res = app.post(&owner, "/v1/workspaces", &req).await;
    assert_eq!(
        res.status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    Workspace {
        id,
        owner,
        ring,
        recovery,
    }
}

pub fn op(id: &str, doc: &str, payload: &[u8]) -> PushOp {
    PushOp {
        op_id: id.to_string(),
        doc_id: doc.to_string(),
        epoch: 0,
        format: 1,
        payload: payload.to_vec(),
    }
}

pub fn doc(n: u32) -> String {
    format!("doc-{n:010}")
}
