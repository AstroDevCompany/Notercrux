use crate::{hex, random, sha256, CryptoError, Result};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// A device's two public keys. The device id is derived from them, so a device cannot claim an id
/// that is not bound to its keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevicePublic {
    /// Ed25519 verifying key (request authentication).
    pub sign: [u8; 32],
    /// X25519 public key (receiving key rings).
    pub agree: [u8; 32],
}

impl DevicePublic {
    pub fn id(&self) -> String {
        let mut b = [0u8; 64];
        b[..32].copy_from_slice(&self.sign);
        b[32..].copy_from_slice(&self.agree);
        hex(&sha256(&b)[..16])
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut b = [0u8; 64];
        b[..32].copy_from_slice(&self.sign);
        b[32..].copy_from_slice(&self.agree);
        b
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        if b.len() != 64 {
            return Err(CryptoError::Malformed);
        }
        let sign: [u8; 32] = b[..32].try_into().expect("32 bytes");
        VerifyingKey::from_bytes(&sign).map_err(|_| CryptoError::Malformed)?;
        Ok(Self {
            sign,
            agree: b[32..].try_into().expect("32 bytes"),
        })
    }
}

/// Secret keys of one device. Stored in the operating system's secure storage, never on the server.
pub struct DeviceKeys {
    signing: SigningKey,
    agree: StaticSecret,
}

impl DeviceKeys {
    pub fn generate() -> Result<Self> {
        let seed: [u8; 32] = random()?;
        let agree: [u8; 32] = random()?;
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
            agree: StaticSecret::from(agree),
        })
    }

    pub fn public(&self) -> DevicePublic {
        DevicePublic {
            sign: self.signing.verifying_key().to_bytes(),
            agree: PublicKey::from(&self.agree).to_bytes(),
        }
    }

    pub fn id(&self) -> String {
        self.public().id()
    }

    pub fn to_bytes(&self) -> Zeroizing<[u8; 64]> {
        let mut b = Zeroizing::new([0u8; 64]);
        b[..32].copy_from_slice(&self.signing.to_bytes());
        b[32..].copy_from_slice(self.agree.as_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        if b.len() != 64 {
            return Err(CryptoError::Malformed);
        }
        let seed: [u8; 32] = b[..32].try_into().expect("32 bytes");
        let agree: [u8; 32] = b[32..].try_into().expect("32 bytes");
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
            agree: StaticSecret::from(agree),
        })
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }

    pub(crate) fn diffie_hellman(&self, peer: &[u8; 32]) -> [u8; 32] {
        *self
            .agree
            .diffie_hellman(&PublicKey::from(*peer))
            .as_bytes()
    }
}

pub fn verify_signature(public_sign: &[u8; 32], message: &[u8], signature: &[u8]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(public_sign) else {
        return false;
    };
    let Ok(sig) = <[u8; 64]>::try_from(signature) else {
        return false;
    };
    vk.verify(message, &ed25519_dalek::Signature::from_bytes(&sig))
        .is_ok()
}

// ---- request authentication ---------------------------------------------------------------------

/// The exact bytes a device signs for one HTTP request. Binding method, target, a timestamp, a
/// nonce and the body hash prevents tampering, cross-endpoint reuse and (with server-side nonce
/// tracking) replay.
pub fn request_payload(
    method: &str,
    target: &str,
    timestamp: i64,
    nonce: &str,
    body: &[u8],
) -> Vec<u8> {
    format!(
        "NCX1\n{method}\n{target}\n{timestamp}\n{nonce}\n{}",
        hex(&sha256(body))
    )
    .into_bytes()
}

pub fn sign_request(
    keys: &DeviceKeys,
    method: &str,
    target: &str,
    timestamp: i64,
    nonce: &str,
    body: &[u8],
) -> [u8; 64] {
    keys.sign(&request_payload(method, target, timestamp, nonce, body))
}

pub fn verify_request(
    public_sign: &[u8; 32],
    method: &str,
    target: &str,
    timestamp: i64,
    nonce: &str,
    body: &[u8],
    signature: &[u8],
) -> bool {
    verify_signature(
        public_sign,
        &request_payload(method, target, timestamp, nonce, body),
        signature,
    )
}

// ---- enrollment ---------------------------------------------------------------------------------

/// A 128-bit secret the existing device shows (as text or QR) to the person enrolling a new device.
/// It never goes to the server, so the server cannot impersonate either side.
#[derive(Clone)]
pub struct InviteSecret([u8; 16]);

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

impl InviteSecret {
    pub fn generate() -> Result<Self> {
        Ok(Self(random()?))
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Grouped base32, e.g. `ABCD-EFGH-...` (26 characters).
    pub fn to_phrase(&self) -> String {
        let (mut buf, mut bits) = (0u32, 0u32);
        let mut s = String::new();
        for b in self.0 {
            buf = (buf << 8) | b as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                s.push(B32[((buf >> bits) & 31) as usize] as char);
            }
        }
        if bits > 0 {
            s.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
        }
        s.as_bytes()
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
                .ok_or(CryptoError::Malformed)? as u32;
            buf = (buf << 5) | v;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes.push(((buf >> bits) & 0xff) as u8);
            }
        }
        if bytes.len() != 16 {
            return Err(CryptoError::Malformed);
        }
        Ok(Self(bytes.try_into().expect("16 bytes")))
    }
}

/// Seals a short value (a device label) under a shared secret (the invite secret or the recovery key).
/// Used before the workspace key ring is available, so the server never sees device names.
pub fn seal_with_secret(secret: &[u8], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    crate::seal_with(
        &crate::hkdf_expand(secret, b"notercrux/v1/label"),
        aad,
        plaintext,
    )
}

pub fn open_with_secret(secret: &[u8], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    crate::open_with(
        &crate::hkdf_expand(secret, b"notercrux/v1/label"),
        aad,
        sealed,
    )
}

/// What the recovery key signs to authorize a replacement device.
pub fn recovery_message(workspace_id: &str, device_id: &str) -> Vec<u8> {
    format!("NCX-recover|{workspace_id}|{device_id}").into_bytes()
}

/// Proof that the party who sent `public` knows the invite secret (HMAC over the public keys).
/// The enrolling device sends it with its request; the approver checks it before wrapping keys, so a
/// malicious server cannot slip in its own keys.
pub fn invite_proof(secret: &InviteSecret, public: &DevicePublic) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&secret.0).expect("any key length");
    mac.update(b"NCX-invite|");
    mac.update(&public.to_bytes());
    mac.finalize().into_bytes().into()
}

/// Six digits both screens display; equal digits mean both saw the same pair of public keys. A second
/// check on top of the invite proof, so a person can confirm they approve the device they expect.
pub fn short_auth_string(public: &DevicePublic) -> String {
    let d = sha256(&public.to_bytes());
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 1_000_000;
    format!("{n:06}")
}
