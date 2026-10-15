//! Cryptographic building blocks for Notercrux.
//!
//! Status: **feasibility spike**. No custom algorithms are used; everything composes
//! RustCrypto implementations of XChaCha20-Poly1305, HKDF-SHA256 and X25519. The design has not
//! been independently reviewed and must not be described as production-secure until it is.
//!
//! Key hierarchy:
//!
//! ```text
//! WorkspaceKey (random 256-bit, never leaves devices unwrapped)
//!   ├─ HKDF "content"     -> encrypts document updates
//!   ├─ HKDF "attachment"  -> encrypts blobs
//!   └─ HKDF "metadata"    -> encrypts titles / index payloads
//! RecoveryKey (random 256-bit, shown once to the user)  -> wraps WorkspaceKey
//! Device enrollment (X25519 + HKDF)                      -> wraps WorkspaceKey for a new device
//! ```

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};
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
    #[error("system randomness unavailable")]
    Random,
}

type Result<T> = std::result::Result<T, CryptoError>;

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| CryptoError::Random)?;
    Ok(b)
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

    /// Encrypts `plaintext`. `aad` binds the ciphertext to its context (e.g. page id + purpose);
    /// opening with a different `aad` fails.
    pub fn seal(&self, purpose: Purpose, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        seal_with(&self.derive(purpose), aad, plaintext)
    }

    pub fn open(&self, purpose: Purpose, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
        open_with(&self.derive(purpose), aad, sealed)
    }
}

fn hkdf_expand(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info, &mut out)
        .expect("32 bytes is a valid length");
    out
}

/// Sealed layout: `version(1) || nonce(24) || ciphertext+tag`. Random 192-bit nonces make
/// collisions negligible without a counter that devices would have to coordinate.
fn seal_with(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
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

fn open_with(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
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

// ---- recovery key -----------------------------------------------------------------------------

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
        let d = Sha256::digest(bytes);
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

    /// Wraps the workspace key so it can be stored on the server without the server being able to
    /// read it.
    pub fn wrap(&self, wk: &WorkspaceKey) -> Result<Vec<u8>> {
        seal_with(
            &hkdf_expand(&self.0, b"notercrux/v1/recovery-wrap"),
            b"workspace-key",
            wk.expose(),
        )
    }

    pub fn unwrap(&self, wrapped: &[u8]) -> Result<WorkspaceKey> {
        let raw = open_with(
            &hkdf_expand(&self.0, b"notercrux/v1/recovery-wrap"),
            b"workspace-key",
            wrapped,
        )?;
        Ok(WorkspaceKey::from_bytes(
            raw.try_into().map_err(|_| CryptoError::Malformed)?,
        ))
    }
}

// ---- device enrollment ------------------------------------------------------------------------

/// Held by a device that wants to join. The public half travels through the server.
pub struct PendingDevice {
    secret: StaticSecret,
    pub public: [u8; 32],
}

impl PendingDevice {
    pub fn new() -> Result<Self> {
        let secret = StaticSecret::from(random::<32>()?);
        let public = PublicKey::from(&secret).to_bytes();
        Ok(Self { secret, public })
    }

    /// Completes enrollment with a package from an existing device.
    pub fn accept(&self, package: &EnrollmentPackage) -> Result<WorkspaceKey> {
        let shared = self
            .secret
            .diffie_hellman(&PublicKey::from(package.sender_public));
        let key = enrollment_key(shared.as_bytes(), &package.sender_public, &self.public);
        let raw = open_with(&key, b"enroll", &package.wrapped)?;
        Ok(WorkspaceKey::from_bytes(
            raw.try_into().map_err(|_| CryptoError::Malformed)?,
        ))
    }
}

pub struct EnrollmentPackage {
    pub sender_public: [u8; 32],
    pub wrapped: Vec<u8>,
}

fn enrollment_key(shared: &[u8], sender_pub: &[u8; 32], receiver_pub: &[u8; 32]) -> [u8; 32] {
    let mut ikm = shared.to_vec();
    ikm.extend_from_slice(sender_pub);
    ikm.extend_from_slice(receiver_pub);
    hkdf_expand(&ikm, b"notercrux/v1/enroll")
}

/// Run on an already-trusted device. The user must compare [`short_auth_string`] on both devices
/// before calling this; otherwise a malicious server could substitute its own public key.
pub fn enroll_device(wk: &WorkspaceKey, new_device_public: &[u8; 32]) -> Result<EnrollmentPackage> {
    let eph = StaticSecret::from(random::<32>()?);
    let sender_public = PublicKey::from(&eph).to_bytes();
    let shared = eph.diffie_hellman(&PublicKey::from(*new_device_public));
    let key = enrollment_key(shared.as_bytes(), &sender_public, new_device_public);
    Ok(EnrollmentPackage {
        sender_public,
        wrapped: seal_with(&key, b"enroll", wk.expose())?,
    })
}

/// Six digits both devices display; equal digits mean both saw the same public key.
pub fn short_auth_string(new_device_public: &[u8; 32]) -> String {
    let d = Sha256::digest(new_device_public);
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{n:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_round_trip_and_context_binding() {
        let k = WorkspaceKey::generate().unwrap();
        let sealed = k.seal(Purpose::Content, b"page-1", b"secret text").unwrap();
        assert!(!sealed.windows(6).any(|w| w == b"secret"));
        assert_eq!(
            k.open(Purpose::Content, b"page-1", &sealed).unwrap(),
            b"secret text"
        );
        // Wrong page, wrong purpose, wrong key, and tampering must all fail.
        assert_eq!(
            k.open(Purpose::Content, b"page-2", &sealed),
            Err(CryptoError::Decrypt)
        );
        assert_eq!(
            k.open(Purpose::Metadata, b"page-1", &sealed),
            Err(CryptoError::Decrypt)
        );
        let other = WorkspaceKey::generate().unwrap();
        assert_eq!(
            other.open(Purpose::Content, b"page-1", &sealed),
            Err(CryptoError::Decrypt)
        );
        let mut bad = sealed.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(
            k.open(Purpose::Content, b"page-1", &bad),
            Err(CryptoError::Decrypt)
        );
    }

    #[test]
    fn nonces_are_unique_per_message() {
        let k = WorkspaceKey::generate().unwrap();
        let a = k.seal(Purpose::Content, b"", b"same").unwrap();
        let b = k.seal(Purpose::Content, b"", b"same").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn future_envelope_versions_are_rejected() {
        let k = WorkspaceKey::generate().unwrap();
        let mut s = k.seal(Purpose::Content, b"", b"x").unwrap();
        s[0] = 2;
        assert_eq!(
            k.open(Purpose::Content, b"", &s),
            Err(CryptoError::UnsupportedVersion(2))
        );
        assert_eq!(
            k.open(Purpose::Content, b"", &[]),
            Err(CryptoError::Malformed)
        );
        assert_eq!(
            k.open(Purpose::Content, b"", &[1, 2, 3]),
            Err(CryptoError::Malformed)
        );
    }

    #[test]
    fn recovery_key_round_trip_and_typo_detection() {
        let wk = WorkspaceKey::generate().unwrap();
        let rk = RecoveryKey::generate().unwrap();
        let wrapped = rk.wrap(&wk).unwrap();
        let phrase = rk.to_phrase();
        // Lower-case and re-spaced input is accepted.
        let typed = phrase.to_lowercase().replace('-', " ");
        let restored = RecoveryKey::from_phrase(&typed)
            .unwrap()
            .unwrap(&wrapped)
            .unwrap();
        assert_eq!(restored.expose(), wk.expose());

        let mut chars: Vec<char> = phrase.chars().collect();
        chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert!(matches!(
            RecoveryKey::from_phrase(&typo),
            Err(CryptoError::InvalidRecoveryKey)
        ));

        let wrong = RecoveryKey::generate().unwrap();
        assert_eq!(wrong.unwrap(&wrapped).err(), Some(CryptoError::Decrypt));
    }

    #[test]
    fn device_enrollment_and_relay_blindness() {
        let wk = WorkspaceKey::generate().unwrap();
        let new_dev = PendingDevice::new().unwrap();
        let pkg = enroll_device(&wk, &new_dev.public).unwrap();
        assert_eq!(new_dev.accept(&pkg).unwrap().expose(), wk.expose());

        // A different device (or the server) cannot open the package.
        let eve = PendingDevice::new().unwrap();
        assert_eq!(eve.accept(&pkg).err(), Some(CryptoError::Decrypt));
    }

    #[test]
    fn substituted_public_key_changes_the_short_auth_string() {
        let real = PendingDevice::new().unwrap();
        let mitm = PendingDevice::new().unwrap();
        assert_ne!(
            short_auth_string(&real.public),
            short_auth_string(&mitm.public)
        );
        assert_eq!(short_auth_string(&real.public).len(), 6);
    }
}
