//! Cryptographic building blocks for Notercrux.
//!
//! Status: **not independently reviewed**. No custom algorithms: everything composes RustCrypto
//! implementations of XChaCha20-Poly1305, HKDF-SHA256, HMAC-SHA256, X25519 and Ed25519.
//!
//! Key hierarchy (see `docs/decisions/0008-key-management.md`):
//!
//! ```text
//! KeyRing: workspace keys by epoch (random 256-bit each; a new epoch is created on revocation)
//!   ├─ HKDF "content"/"attachment"/"metadata" -> payload encryption (per epoch)
//!   └─ epoch 0 -> HKDF "docid" -> stable, opaque document ids (HMAC of the page id)
//! DeviceKeys: Ed25519 (request signing) + X25519 (receiving key rings)
//! RecoveryKey: 256 random bits, shown once. Seals every epoch; also derives a signing key so the
//!              server can authorize a recovery without ever learning the recovery key.
//! Invite secret: 128 random bits shown to the user while enrolling a device; never sent to the server.
//! ```

mod device;
mod keyring;
mod recovery;
mod wrap;

pub use device::{
    constant_time_eq, invite_proof, open_with_secret, recovery_message, request_payload,
    seal_with_secret, short_auth_string, sign_request, verify_request, verify_signature,
    DeviceKeys, DevicePublic, InviteSecret,
};
pub use keyring::{chunk_aad, content_aad, KeyRing};
pub use recovery::{wrap_for_recovery, RecoveryKey};
pub use wrap::{unwrap_ring, wrap_ring};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Envelope format version, stored as the first byte of every sealed payload.
pub const ENVELOPE_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("decryption failed: wrong key, wrong context, or corrupted data")]
    Decrypt,
    #[error("unsupported envelope version {0}")]
    UnsupportedVersion(u8),
    #[error("malformed envelope")]
    Malformed,
    #[error("invalid recovery key")]
    InvalidRecoveryKey,
    #[error("no key for epoch {0}")]
    UnknownEpoch(u32),
    #[error("system randomness unavailable")]
    Random,
}

pub type Result<T> = std::result::Result<T, CryptoError>;

pub(crate) fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| CryptoError::Random)?;
    Ok(b)
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok())
        .collect()
}

pub(crate) fn hkdf_expand(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info, &mut out)
        .expect("32 bytes is a valid length");
    out
}

/// What a derived key is used for; part of the HKDF info string, so keys never cross purposes.
#[derive(Clone, Copy, Debug)]
pub enum Purpose {
    Content,
    Attachment,
    Metadata,
}

impl Purpose {
    fn label(self) -> &'static [u8] {
        match self {
            Purpose::Content => b"notercrux/v1/content",
            Purpose::Attachment => b"notercrux/v1/attachment",
            Purpose::Metadata => b"notercrux/v1/metadata",
        }
    }
}

#[derive(Zeroize, ZeroizeOnDrop, Clone)]
pub struct WorkspaceKey([u8; 32]);

impl WorkspaceKey {
    pub fn generate() -> Result<Self> {
        Ok(Self(random()?))
    }

    pub fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    fn derive(&self, purpose: Purpose) -> [u8; 32] {
        hkdf_expand(&self.0, purpose.label())
    }

    /// Encrypts `plaintext`. `aad` binds the ciphertext to its context (workspace, document, epoch);
    /// opening with a different `aad` fails.
    pub fn seal(&self, purpose: Purpose, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        seal_with(&self.derive(purpose), aad, plaintext)
    }

    pub fn open(&self, purpose: Purpose, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        open_with(&self.derive(purpose), aad, sealed)
    }

    /// A stable key for deriving opaque identifiers (never used to encrypt).
    pub(crate) fn derive_docid_key(&self) -> [u8; 32] {
        hkdf_expand(&self.0, b"notercrux/v1/docid")
    }
}

/// Sealed layout: `version(1) || nonce(24) || ciphertext+tag`. Random 192-bit nonces make
/// collisions negligible without a counter that devices would have to coordinate.
pub(crate) fn seal_with(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let nonce: [u8; NONCE_LEN] = random()?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut aad_full = vec![ENVELOPE_VERSION];
    aad_full.extend_from_slice(aad);
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad_full,
            },
        )
        .map_err(|_| CryptoError::Decrypt)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
    out.push(ENVELOPE_VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub(crate) fn open_with(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    let (&version, rest) = sealed.split_first().ok_or(CryptoError::Malformed)?;
    if version != ENVELOPE_VERSION {
        return Err(CryptoError::UnsupportedVersion(version));
    }
    if rest.len() < NONCE_LEN + 16 {
        return Err(CryptoError::Malformed);
    }
    let (nonce, ct) = rest.split_at(NONCE_LEN);
    let mut aad_full = vec![version];
    aad_full.extend_from_slice(aad);
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: &aad_full,
            },
        )
        .map_err(|_| CryptoError::Decrypt)
}

#[cfg(test)]
mod tests;
