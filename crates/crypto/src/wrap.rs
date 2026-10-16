use crate::{hkdf_expand, open_with, random, seal_with, CryptoError, DeviceKeys, KeyRing, Result};
use x25519_dalek::{PublicKey, StaticSecret};

fn wrap_key(
    shared: &[u8],
    eph_pub: &[u8; 32],
    recipient_pub: &[u8; 32],
    context: &[u8],
    secret: Option<&[u8]>,
) -> [u8; 32] {
    let mut ikm = shared.to_vec();
    ikm.extend_from_slice(eph_pub);
    ikm.extend_from_slice(recipient_pub);
    if let Some(s) = secret {
        ikm.extend_from_slice(s);
    }
    let mut info = b"notercrux/v1/wrap|".to_vec();
    info.extend_from_slice(context);
    hkdf_expand(&ikm, &info)
}

/// Encrypts a key ring for one device: ephemeral X25519 agreement with the device's public key, then
/// XChaCha20-Poly1305. `context` (workspace, epoch, device, invite) is authenticated, so a wrapped
/// ring cannot be moved to another purpose. During enrollment `secret` (the invite secret) is mixed
/// into the key, so only a party that knows it can produce or read the package.
///
/// Layout: `ephemeral_public(32) || sealed`.
pub fn wrap_ring(
    ring: &KeyRing,
    recipient_agree: &[u8; 32],
    context: &[u8],
    secret: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let eph = StaticSecret::from(random::<32>()?);
    let eph_pub = PublicKey::from(&eph).to_bytes();
    let shared = eph.diffie_hellman(&PublicKey::from(*recipient_agree));
    let key = wrap_key(
        shared.as_bytes(),
        &eph_pub,
        recipient_agree,
        context,
        secret,
    );
    let sealed = seal_with(&key, context, &ring.to_bytes())?;
    let mut out = eph_pub.to_vec();
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn unwrap_ring(
    keys: &DeviceKeys,
    blob: &[u8],
    context: &[u8],
    secret: Option<&[u8]>,
) -> Result<KeyRing> {
    if blob.len() < 32 {
        return Err(CryptoError::Malformed);
    }
    let eph_pub: [u8; 32] = blob[..32].try_into().expect("32 bytes");
    let shared = keys.diffie_hellman(&eph_pub);
    let key = wrap_key(&shared, &eph_pub, &keys.public().agree, context, secret);
    let plain = zeroize::Zeroizing::new(open_with(&key, context, &blob[32..])?);
    KeyRing::from_bytes(&plain)
}
