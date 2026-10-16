//! Wire types for the Notercrux sync API (protocol version 1).
//!
//! Everything the server stores or relays is an **opaque byte string** produced by clients:
//! sealed operations, sealed labels, wrapped key rings. The server learns only identifiers, sizes,
//! sequence numbers and timing. See `docs/security/README.md` for the exact metadata list.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PROTOCOL_VERSION: u32 = 1;

pub mod headers {
    pub const DEVICE: &str = "x-ncx-device";
    pub const TIME: &str = "x-ncx-time";
    pub const NONCE: &str = "x-ncx-nonce";
    pub const SIGNATURE: &str = "x-ncx-sig";
    pub const PROTOCOL: &str = "x-ncx-protocol";
    pub const TOTAL_CHUNKS: &str = "x-ncx-total";
    pub const EPOCH: &str = "x-ncx-epoch";
}

/// Bytes as standard base64 in JSON.
pub mod b64 {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

pub fn encode_b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode_b64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

// ---- errors ---------------------------------------------------------------------------------

/// Stable machine-readable error codes.
pub mod code {
    pub const BAD_REQUEST: &str = "bad_request";
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const REVOKED: &str = "device_revoked";
    pub const FORBIDDEN: &str = "forbidden";
    pub const NOT_FOUND: &str = "not_found";
    pub const CONFLICT: &str = "conflict";
    pub const QUOTA: &str = "quota_exceeded";
    pub const TOO_LARGE: &str = "too_large";
    pub const RATE_LIMITED: &str = "rate_limited";
    pub const UNSUPPORTED_PROTOCOL: &str = "unsupported_protocol";
    pub const REPLAY: &str = "replayed_request";
    pub const CLOCK_SKEW: &str = "clock_skew";
    pub const INTERNAL: &str = "internal";
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiError {
    pub error: String,
    pub message: String,
}

// ---- workspaces and devices -----------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspace {
    /// Chosen by the client (it is part of every ciphertext's authenticated data).
    pub workspace_id: String,
    /// The creating device's 64-byte public key (Ed25519 verifying key || X25519 public key).
    #[serde(with = "b64")]
    pub device_key: Vec<u8>,
    /// Device label sealed with the workspace key ring.
    #[serde(with = "b64")]
    pub label_sealed: Vec<u8>,
    /// Public keys of the recovery key (64 bytes, like a device). Lets the server authorize a
    /// recovery and lets every device re-wrap the key ring for the recovery key.
    #[serde(with = "b64")]
    pub recovery_key: Vec<u8>,
    /// The key ring wrapped for the recovery key. Opaque to the server.
    #[serde(with = "b64")]
    pub recovery_wrap: Vec<u8>,
    #[serde(default)]
    pub registration_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    #[serde(with = "b64")]
    pub key: Vec<u8>,
    #[serde(with = "b64")]
    pub label_sealed: Vec<u8>,
    pub created_at: i64,
    pub revoked_at: Option<i64>,
    pub last_seen: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub op_bytes: i64,
    pub op_count: i64,
    pub blob_bytes: i64,
    pub devices: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_devices: i64,
    pub max_op_bytes_total: i64,
    pub max_blob_bytes_total: i64,
    pub max_op_size: i64,
    pub max_blob_size: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub workspace_id: String,
    pub epoch: u32,
    pub devices: Vec<DeviceInfo>,
    pub usage: Usage,
    pub limits: Limits,
    pub protocol: u32,
    #[serde(with = "b64")]
    pub recovery_key: Vec<u8>,
}

// ---- enrollment -----------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteResponse {
    pub invite_id: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteRequest {
    pub workspace_id: String,
    #[serde(with = "b64")]
    pub device_key: Vec<u8>,
    /// HMAC over the device's public keys with the invite secret.
    #[serde(with = "b64")]
    pub proof: Vec<u8>,
    /// Label sealed under the invite secret.
    #[serde(with = "b64")]
    pub label_sealed: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InviteStatus {
    Pending,
    Requested,
    Approved,
    Expired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteState {
    pub status: InviteStatus,
    pub request: Option<InviteRequest>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveInvite {
    pub device_id: String,
    /// The key ring wrapped for the new device (mixed with the invite secret).
    #[serde(with = "b64")]
    pub key_wrap: Vec<u8>,
    /// The device label re-sealed under the workspace key ring.
    #[serde(with = "b64")]
    pub label_sealed: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteResult {
    pub approved: bool,
    pub epoch: u32,
    #[serde(default, with = "opt_b64")]
    pub key_wrap: Option<Vec<u8>>,
}

pub mod opt_b64 {
    use super::{decode_b64, encode_b64};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_str(&encode_b64(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        match Option::<String>::deserialize(d)? {
            Some(s) => decode_b64(&s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom("invalid base64")),
            None => Ok(None),
        }
    }
}

// ---- revocation, keys, recovery -------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyWrapEntry {
    pub device_id: String,
    #[serde(with = "b64")]
    pub key_wrap: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeDevice {
    /// Must be the current epoch + 1.
    pub new_epoch: u32,
    /// One wrap of the new ring per device that remains (including the revoking device).
    pub wraps: Vec<KeyWrapEntry>,
    /// The new ring sealed under the recovery key.
    #[serde(with = "b64")]
    pub recovery_wrap: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysResponse {
    pub epoch: u32,
    /// The latest wrap for the calling device, if it is newer than `have_epoch`.
    #[serde(default, with = "opt_b64")]
    pub key_wrap: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverRequest {
    pub workspace_id: String,
    #[serde(with = "b64")]
    pub device_key: Vec<u8>,
    /// Signature by the recovery auth key over `recovery_message(workspace_id, device_id)`.
    #[serde(with = "b64")]
    pub recovery_signature: Vec<u8>,
    #[serde(with = "b64")]
    pub label_sealed: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverResponse {
    pub epoch: u32,
    #[serde(with = "b64")]
    pub recovery_wrap: Vec<u8>,
}

// ---- operations -----------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushOp {
    /// Stable across retries: the server de-duplicates by it.
    pub op_id: String,
    pub doc_id: String,
    pub epoch: u32,
    pub format: u32,
    #[serde(with = "b64")]
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub ops: Vec<PushOp>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResponse {
    pub accepted: Vec<String>,
    pub duplicate: Vec<String>,
    pub cursor: i64,
    pub epoch: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulledOp {
    pub seq: i64,
    pub op_id: String,
    pub device_id: String,
    pub doc_id: String,
    pub epoch: u32,
    pub format: u32,
    #[serde(with = "b64")]
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResponse {
    pub ops: Vec<PulledOp>,
    pub cursor: i64,
    pub more: bool,
    /// The workspace's current key epoch, so a client learns about a rotation without polling `/keys`.
    pub epoch: u32,
}

// ---- attachments ----------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BlobStatus {
    pub complete: bool,
    pub total: u32,
    pub epoch: u32,
    pub size: i64,
    /// Chunk indexes already stored (for resuming an interrupted upload).
    pub present: Vec<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitBlob {
    pub total: u32,
    pub epoch: u32,
    /// Total ciphertext size, for quota accounting and integrity.
    pub size: i64,
}

// ---- sealed operation payloads --------------------------------------------------------------

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnvelopeError {
    #[error("truncated operation payload")]
    Truncated,
    #[error("invalid document key")]
    BadKey,
}

/// What a client puts *inside* the encryption of an operation: the payload format version, the local
/// document key (`ws` or `page:<id>`, which the server must not see) and the CRDT update bytes.
///
/// Layout: `format(u32 BE) || key_len(u16 BE) || key || update`.
pub struct OpEnvelope {
    pub format: u32,
    pub doc_key: String,
    pub update: Vec<u8>,
}

impl OpEnvelope {
    pub fn encode(&self) -> Vec<u8> {
        let key = self.doc_key.as_bytes();
        let mut out = Vec::with_capacity(6 + key.len() + self.update.len());
        out.extend_from_slice(&self.format.to_be_bytes());
        out.extend_from_slice(&(key.len() as u16).to_be_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&self.update);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, EnvelopeError> {
        if b.len() < 6 {
            return Err(EnvelopeError::Truncated);
        }
        let format = u32::from_be_bytes(b[..4].try_into().expect("4 bytes"));
        let n = u16::from_be_bytes([b[4], b[5]]) as usize;
        if b.len() < 6 + n {
            return Err(EnvelopeError::Truncated);
        }
        let doc_key = std::str::from_utf8(&b[6..6 + n])
            .map_err(|_| EnvelopeError::BadKey)?
            .to_string();
        Ok(Self {
            format,
            doc_key,
            update: b[6 + n..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_round_trip_and_reject_truncation() {
        let e = OpEnvelope {
            format: 1,
            doc_key: "page:abc".into(),
            update: vec![1, 2, 3, 4],
        };
        let d = OpEnvelope::decode(&e.encode()).unwrap();
        assert_eq!(
            (d.format, d.doc_key.as_str(), d.update.as_slice()),
            (1, "page:abc", &[1, 2, 3, 4][..])
        );
        assert_eq!(
            OpEnvelope::decode(&[]).err(),
            Some(EnvelopeError::Truncated)
        );
        assert_eq!(
            OpEnvelope::decode(&e.encode()[..8]).err(),
            Some(EnvelopeError::Truncated)
        );
        let mut bad = e.encode();
        bad[6] = 0xff;
        assert_eq!(OpEnvelope::decode(&bad).err(), Some(EnvelopeError::BadKey));
        assert!(OpEnvelope::decode(
            &OpEnvelope {
                format: 9,
                doc_key: String::new(),
                update: vec![]
            }
            .encode()
        )
        .is_ok());
    }

    #[test]
    fn json_uses_camel_case_and_base64() {
        let op = PushOp {
            op_id: "o".into(),
            doc_id: "d".into(),
            epoch: 2,
            format: 1,
            payload: vec![0, 255, 1],
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(
            json.contains("\"opId\"") && json.contains("\"payload\":\"AP8B\""),
            "{json}"
        );
        let back: PushOp = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payload, vec![0, 255, 1]);
        assert!(serde_json::from_str::<PushOp>(
            "{\"opId\":\"o\",\"docId\":\"d\",\"epoch\":1,\"format\":1,\"payload\":\"***\"}"
        )
        .is_err());
    }

    #[test]
    fn optional_wraps_serialize_as_null_or_base64() {
        let r = KeysResponse {
            epoch: 3,
            key_wrap: None,
        };
        assert!(serde_json::to_string(&r)
            .unwrap()
            .contains("\"keyWrap\":null"));
        let r = KeysResponse {
            epoch: 3,
            key_wrap: Some(vec![1, 2]),
        };
        let back: KeysResponse = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back.key_wrap, Some(vec![1, 2]));
    }
}
