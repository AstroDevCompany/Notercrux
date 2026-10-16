//! Persisted account data, canonical encryption contexts and the invite code format.

use crate::{Result, SyncError};
use notercrux_crypto::{KeyRing, Purpose};
use notercrux_protocol::{decode_b64, encode_b64};
use serde::{Deserialize, Serialize};

pub const DEVICE_SECRET: &str = "device";
pub const RING_SECRET: &str = "ring";

/// Non-secret facts about the link, stored in the database next to the data they describe.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub server: String,
    pub workspace_id: String,
    pub device_id: String,
    pub label: String,
}

pub fn normalize_server(url: &str) -> Result<String> {
    let u = url.trim().trim_end_matches('/');
    if !(u.starts_with("http://") || u.starts_with("https://")) || u.len() < 9 {
        return Err(SyncError::Protocol(
            "the server address must start with https:// (or http:// on a trusted network)".into(),
        ));
    }
    Ok(u.to_string())
}

/// Authenticated data for a key ring wrapped for a device that is joining.
pub fn enroll_context(workspace: &str, device: &str, invite: &str) -> Vec<u8> {
    format!("{workspace}|{device}|{invite}").into_bytes()
}

/// Authenticated data for a key ring wrapped after a rotation.
pub fn rotate_context(workspace: &str, device: &str, epoch: u32) -> Vec<u8> {
    format!("{workspace}|{device}|rotate|{epoch}").into_bytes()
}

pub fn invite_label_aad(workspace: &str, invite: &str) -> Vec<u8> {
    format!("{workspace}|{invite}").into_bytes()
}

pub fn recovery_label_aad(workspace: &str) -> Vec<u8> {
    format!("{workspace}|recovery").into_bytes()
}

fn ring_label_aad(workspace: &str, epoch: u32) -> Vec<u8> {
    format!("{workspace}|label|{epoch}").into_bytes()
}

/// Seals a device name with the key ring. Layout: `epoch (u32 BE) || sealed`.
pub fn seal_label(ring: &KeyRing, workspace: &str, label: &str) -> Result<Vec<u8>> {
    let (epoch, sealed) = ring.seal(
        Purpose::Metadata,
        |e| ring_label_aad(workspace, e),
        label.as_bytes(),
    )?;
    let mut out = epoch.to_be_bytes().to_vec();
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn open_label(ring: &KeyRing, workspace: &str, bytes: &[u8]) -> Option<String> {
    let epoch = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?);
    let plain = ring
        .open(
            epoch,
            Purpose::Metadata,
            &ring_label_aad(workspace, epoch),
            &bytes[4..],
        )
        .ok()?;
    String::from_utf8(plain).ok()
}

/// Everything the new device needs to ask for admission: where, which workspace, which invitation,
/// and the secret that proves the invitation came from someone who holds the workspace key.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InviteCode {
    pub server: String,
    pub workspace: String,
    pub invite: String,
    pub secret: String,
}

const PREFIX: &str = "NCX1-";

impl InviteCode {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("serializable");
        let b64 = encode_b64(&json)
            .replace('+', "-")
            .replace('/', "_")
            .replace('=', "");
        format!("{PREFIX}{b64}")
    }

    pub fn decode(code: &str) -> Result<Self> {
        let body = code.trim().strip_prefix(PREFIX).ok_or(SyncError::BadCode)?;
        let mut b64 = body.replace('-', "+").replace('_', "/");
        while b64.len() % 4 != 0 {
            b64.push('=');
        }
        let bytes = decode_b64(&b64).ok_or(SyncError::BadCode)?;
        let c: InviteCode = serde_json::from_slice(&bytes).map_err(|_| SyncError::BadCode)?;
        normalize_server(&c.server).map_err(|_| SyncError::BadCode)?;
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_round_trip_and_reject_garbage() {
        let c = InviteCode {
            server: "https://sync.example.org".into(),
            workspace: "w1234567890".into(),
            invite: "abcdef012345".into(),
            secret: "AAAA-BBBB".into(),
        };
        let text = c.encode();
        assert!(text.starts_with("NCX1-") && !text.contains(['+', '/', '=', ' ']));
        assert_eq!(InviteCode::decode(&format!("  {text}\n")).unwrap(), c);
        for bad in ["", "NCX1-", "NCX1-!!!", "XYZ-abc", "NCX1-e30"] {
            assert_eq!(InviteCode::decode(bad), Err(SyncError::BadCode), "{bad}");
        }
    }

    #[test]
    fn labels_seal_with_the_ring_and_survive_rotation() {
        let mut ring = KeyRing::generate().unwrap();
        let sealed = seal_label(&ring, "ws", "Luca's laptop").unwrap();
        assert!(!sealed.windows(4).any(|w| w == b"Luca"));
        ring.rotate().unwrap();
        assert_eq!(
            open_label(&ring, "ws", &sealed).as_deref(),
            Some("Luca's laptop")
        );
        assert_eq!(open_label(&ring, "other", &sealed), None);
        assert_eq!(open_label(&ring, "ws", &sealed[..3]), None);
    }

    #[test]
    fn server_addresses_are_normalized() {
        assert_eq!(
            normalize_server(" https://a.example.org/ ").unwrap(),
            "https://a.example.org"
        );
        assert!(normalize_server("a.example.org").is_err());
        assert!(normalize_server("ftp://x.y.z").is_err());
    }
}
