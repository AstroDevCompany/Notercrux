use crate::{
    hkdf_expand, random, sha256, unwrap_ring, wrap_ring, CryptoError, DeviceKeys, DevicePublic,
    KeyRing, Result,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// A 256-bit secret the user stores offline. Rendered as grouped base32 with a 2-byte checksum so
/// typos are caught before any decryption is attempted.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct RecoveryKey([u8; 32]);

impl RecoveryKey {
    pub fn generate() -> Result<Self> {
        Ok(Self(random()?))
    }

    fn checksum(bytes: &[u8; 32]) -> [u8; 2] {
        let d = sha256(bytes);
        [d[0], d[1]]
    }

    /// e.g. `ABCD-EFGH-...` (11 groups of 4 characters for 34 bytes).
    pub fn to_phrase(&self) -> String {
        let mut data = self.0.to_vec();
        data.extend_from_slice(&Self::checksum(&self.0));
        let mut chars = String::new();
        let (mut buf, mut bits) = (0u32, 0u32);
        for b in data {
            buf = (buf << 8) | b as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                chars.push(B32[((buf >> bits) & 31) as usize] as char);
            }
        }
        if bits > 0 {
            chars.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
        }
        chars
            .as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).expect("ascii"))
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn from_phrase(phrase: &str) -> Result<Self> {
        let (mut buf, mut bits) = (0u32, 0u32);
        let mut bytes = Vec::new();
        for ch in phrase.chars().filter(|c| !matches!(c, '-' | ' ')) {
            let v = B32
                .iter()
                .position(|&b| b as char == ch.to_ascii_uppercase())
                .ok_or(CryptoError::InvalidRecoveryKey)? as u32;
            buf = (buf << 5) | v;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes.push(((buf >> bits) & 0xff) as u8);
            }
        }
        if bytes.len() != 34 {
            return Err(CryptoError::InvalidRecoveryKey);
        }
        let key: [u8; 32] = bytes[..32].try_into().expect("len checked");
        if bytes[32..] != Self::checksum(&key) {
            return Err(CryptoError::InvalidRecoveryKey);
        }
        Ok(Self(key))
    }

    /// The recovery key behaves like a device that only exists on paper: a key pair derived from the
    /// secret. Every enrolled device can wrap the workspace key ring for its *public* half (so the
    /// recovery copy stays current when keys rotate) without ever holding the secret. Only the person
    /// with the recovery phrase can derive the private half and open the ring.
    pub fn keys(&self) -> DeviceKeys {
        let mut b = [0u8; 64];
        b[..32].copy_from_slice(&hkdf_expand(&self.0, b"notercrux/v1/recovery-sign"));
        b[32..].copy_from_slice(&hkdf_expand(&self.0, b"notercrux/v1/recovery-agree"));
        DeviceKeys::from_bytes(&b).expect("64 bytes")
    }

    /// What the server stores (and hands to devices) to authorize and re-wrap for recovery.
    pub fn public(&self) -> DevicePublic {
        self.keys().public()
    }

    /// Opens the recovery copy of the ring.
    pub fn unwrap_ring(&self, wrapped: &[u8], workspace_id: &str) -> Result<KeyRing> {
        unwrap_ring(&self.keys(), wrapped, &recovery_context(workspace_id), None)
    }

    /// Signs `message` (authorizes a replacement device at the server).
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.keys().sign(message).to_vec()
    }

    /// Secret material for sealing a device label before a key ring exists (recovery flow).
    pub fn label_secret(&self) -> [u8; 32] {
        hkdf_expand(&self.0, b"notercrux/v1/recovery-label")
    }
}

fn recovery_context(workspace_id: &str) -> Vec<u8> {
    format!("NCX-recovery|{workspace_id}").into_bytes()
}

/// Wraps `ring` for the recovery key's public half. Any device can do this.
pub fn wrap_for_recovery(
    ring: &KeyRing,
    recovery_public: &DevicePublic,
    workspace_id: &str,
) -> Result<Vec<u8>> {
    wrap_ring(
        ring,
        &recovery_public.agree,
        &recovery_context(workspace_id),
        None,
    )
}
