//! Signed HTTP calls to the sync server.

use crate::SyncError;
use notercrux_crypto::{sign_request, DeviceKeys};
use notercrux_protocol::{code, encode_b64, headers as h, ApiError, PROTOCOL_VERSION};
use reqwest::Method;
use std::time::Duration;

pub struct Reply {
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, SyncError> {
        serde_json::from_slice(&self.body)
            .map_err(|e| SyncError::Protocol(format!("unexpected server response: {e}")))
    }
}

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Installs the TLS crypto provider the HTTP client needs. Safe to call any number of times, and
/// when the host application already installed one.
pub fn install_tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

impl Api {
    pub fn new() -> Result<Self, SyncError> {
        install_tls_provider();
        let http = reqwest::Client::builder()
            .user_agent(concat!("notercrux/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| SyncError::Network(e.to_string()))?;
        Ok(Self { http })
    }

    /// Sends one request. `keys` signs it (omit for the few unauthenticated endpoints). A non-2xx
    /// answer becomes an error carrying the server's code.
    pub async fn call(
        &self,
        base: &str,
        keys: Option<&DeviceKeys>,
        method: Method,
        target: &str,
        body: Vec<u8>,
        timeout: Option<Duration>,
    ) -> Result<Reply, SyncError> {
        let url = format!("{}{}", base.trim_end_matches('/'), target);
        let mut req = self
            .http
            .request(method.clone(), &url)
            .header(h::PROTOCOL, PROTOCOL_VERSION.to_string());
        if let Some(k) = keys {
            let nonce = uuid::Uuid::new_v4().simple().to_string();
            let time = now_secs();
            let sig = sign_request(k, method.as_str(), target, time, &nonce, &body);
            req = req
                .header(h::DEVICE, k.id())
                .header(h::TIME, time.to_string())
                .header(h::NONCE, nonce)
                .header(h::SIGNATURE, encode_b64(&sig));
        }
        if let Some(t) = timeout {
            req = req.timeout(t);
        }
        if !body.is_empty() {
            req = req.body(body);
        }
        let res = req
            .send()
            .await
            .map_err(|e| SyncError::Network(describe(&e)))?;
        let status = res.status().as_u16();
        let bytes = res
            .bytes()
            .await
            .map_err(|e| SyncError::Network(describe(&e)))?
            .to_vec();
        if (200..300).contains(&status) {
            return Ok(Reply { body: bytes });
        }
        let err: Option<ApiError> = serde_json::from_slice(&bytes).ok();
        let (code, message) = err.map_or_else(
            || ("http".to_string(), format!("the server answered {status}")),
            |e| (e.error, e.message),
        );
        if code == code::REVOKED {
            return Err(SyncError::Revoked);
        }
        Err(SyncError::Server {
            status,
            code,
            message,
        })
    }
}

fn describe(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "the server did not answer in time".into()
    } else if e.is_connect() {
        "cannot reach the server".into()
    } else {
        e.to_string()
    }
}
