//! Device-signed request authentication.
//!
//! Every request carries `x-ncx-device`, `x-ncx-time`, `x-ncx-nonce` and `x-ncx-sig`. The signature
//! covers method, path and query, timestamp, nonce and the SHA-256 of the body, so a captured request
//! cannot be altered, redirected to another endpoint or (thanks to the nonce store) replayed.

use crate::repo::WorkspaceRow;
use crate::AppState;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use notercrux_crypto::verify_request;
use notercrux_protocol::{code, decode_b64, headers as h, ApiError, DeviceInfo, PROTOCOL_VERSION};

#[derive(Debug)]
pub struct ApiErr {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiErr {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code::BAD_REQUEST, message)
    }

    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            code::UNAUTHORIZED,
            "authentication failed",
        )
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, code::NOT_FOUND, "not found")
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiError {
                error: self.code.to_string(),
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl From<crate::repo::RepoError> for ApiErr {
    fn from(e: crate::repo::RepoError) -> Self {
        use crate::repo::RepoError::*;
        match e {
            NotFound => Self::not_found(),
            Conflict(m) => Self::new(StatusCode::CONFLICT, code::CONFLICT, m),
            Quota(m) => Self::new(StatusCode::FORBIDDEN, code::QUOTA, m),
            Invalid(m) => Self::bad(m),
            Backend(m) => {
                tracing::error!("storage failure: {m}");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    code::INTERNAL,
                    "internal error",
                )
            }
        }
    }
}

pub struct Authed {
    pub device: DeviceInfo,
    pub workspace: WorkspaceRow,
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Rejects clients that speak a protocol version this server does not.
pub fn check_protocol(headers: &HeaderMap) -> Result<(), ApiErr> {
    match header(headers, h::PROTOCOL).map(|v| v.parse::<u32>()) {
        None => Ok(()),
        Some(Ok(v)) if (1..=PROTOCOL_VERSION).contains(&v) => Ok(()),
        _ => Err(ApiErr::new(
            StatusCode::UPGRADE_REQUIRED,
            code::UNSUPPORTED_PROTOCOL,
            format!("this server speaks protocol {PROTOCOL_VERSION}; update the app or the server"),
        )),
    }
}

pub struct Signed<'a> {
    pub device_id: &'a str,
    time: i64,
    nonce: &'a str,
    sig: Vec<u8>,
}

pub fn parse_signed<'a>(headers: &'a HeaderMap, state: &AppState) -> Result<Signed<'a>, ApiErr> {
    check_protocol(headers)?;
    let device_id = header(headers, h::DEVICE).ok_or_else(ApiErr::unauthorized)?;
    let time: i64 = header(headers, h::TIME)
        .and_then(|v| v.parse().ok())
        .ok_or_else(ApiErr::unauthorized)?;
    let nonce = header(headers, h::NONCE)
        .filter(|n| (8..=64).contains(&n.len()))
        .ok_or_else(ApiErr::unauthorized)?;
    let sig = header(headers, h::SIGNATURE)
        .and_then(decode_b64)
        .ok_or_else(ApiErr::unauthorized)?;
    if (state.now() - time).abs() > state.config.max_clock_skew {
        return Err(ApiErr::new(
            StatusCode::UNAUTHORIZED,
            code::CLOCK_SKEW,
            "the device clock differs too much from the server clock",
        ));
    }
    Ok(Signed {
        device_id,
        time,
        nonce,
        sig,
    })
}

impl Signed<'_> {
    pub fn verify(&self, key: &[u8], method: &Method, uri: &Uri, body: &[u8]) -> bool {
        let Ok(sign): Result<[u8; 32], _> = key.get(..32).unwrap_or_default().try_into() else {
            return false;
        };
        let target = uri.path_and_query().map_or(uri.path(), |p| p.as_str());
        verify_request(
            &sign,
            method.as_str(),
            target,
            self.time,
            self.nonce,
            body,
            &self.sig,
        )
    }
}

/// Authenticates a request made by an enrolled device of `ws`.
pub async fn authenticate(
    state: &AppState,
    ws: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Authed, ApiErr> {
    let result = authenticate_inner(state, ws, method, uri, headers, body).await;
    if let Err(e) = &result {
        if e.status == StatusCode::UNAUTHORIZED || e.code == code::REVOKED {
            crate::metrics::Metrics::add(&state.metrics.auth_failures, 1);
        }
    }
    result
}

async fn authenticate_inner(
    state: &AppState,
    ws: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Authed, ApiErr> {
    let signed = parse_signed(headers, state)?;
    let device = state
        .repo
        .device(ws, signed.device_id)
        .await?
        .ok_or_else(ApiErr::unauthorized)?;
    if !signed.verify(&device.key, method, uri, body) {
        return Err(ApiErr::unauthorized());
    }
    // Only a valid signature learns whether a device was revoked.
    if device.revoked_at.is_some() {
        return Err(ApiErr::new(
            StatusCode::FORBIDDEN,
            code::REVOKED,
            "this device has been revoked",
        ));
    }
    let now = state.now();
    if !state
        .repo
        .record_nonce(
            ws,
            &device.id,
            signed.nonce,
            now,
            state.config.max_clock_skew * 2 + 5,
        )
        .await?
    {
        return Err(ApiErr::new(
            StatusCode::UNAUTHORIZED,
            code::REPLAY,
            "request already seen",
        ));
    }
    if !state.limiter.allow(
        &format!("d:{ws}:{}", device.id),
        state.config.device_rate,
        now,
    ) {
        crate::metrics::Metrics::add(&state.metrics.rate_limited, 1);
        return Err(ApiErr::new(
            StatusCode::TOO_MANY_REQUESTS,
            code::RATE_LIMITED,
            "too many requests",
        ));
    }
    if device.last_seen.is_none_or(|t| now - t > 60) {
        state.repo.touch_device(ws, &device.id, now).await?;
    }
    let workspace = state
        .repo
        .workspace(ws)
        .await?
        .ok_or_else(ApiErr::unauthorized)?;
    Ok(Authed { device, workspace })
}
