# ADR 0004: E2EE key hierarchy, recovery and device enrollment

**Status:** Proposed (spike: `crates/crypto`). **Not independently reviewed.**

## Decision under test

- A random 256-bit **workspace key** never leaves a device unwrapped. Purpose keys (content,
  attachment, metadata) are derived with HKDF-SHA256 so one key is never used for two jobs.
- **Payloads** use XChaCha20-Poly1305 (RustCrypto). The 192-bit random nonce removes the need for
  devices to coordinate a counter. The envelope is `version ‖ nonce ‖ ciphertext+tag`, and
  associated data binds each ciphertext to its context (e.g. a page id), so ciphertexts cannot be
  replayed into another page.
- **Recovery key:** 256 random bits shown once as grouped base32 with a 2-byte checksum so typos are
  detected before any decryption. It wraps the workspace key; the wrapped blob may be stored on the
  server, which cannot open it.
- **Device enrollment:** the new device generates an X25519 key pair; a trusted device performs ECDH
  with an ephemeral key, derives a key with HKDF (binding both public keys) and wraps the workspace
  key. Both devices show a 6-digit **short authentication string** derived from the new device's
  public key; the user must confirm it matches or a malicious relay could substitute a key.
- No custom primitives. Authentication (account login) is separate from content keys.

## Consequences and open questions

- Losing every device **and** the recovery key means the data cannot be recovered. This must be
  explained plainly in the UI and docs; it is a property of the design, not a bug.
- A 6-digit SAS gives ~20 bits against an active attacker per attempt; enrollment must be rate
  limited and single-use. A stronger scheme (longer SAS, QR with key hash) needs evaluation.
- Revocation currently only blocks a device at the relay. A revoked device still holds the workspace
  key; real revocation requires **key rotation** and re-encryption, which is not designed yet.
- Local search over encrypted content must be client-side; the index lives only on the device.
- Key storage in the OS keychain, passphrase-based unlock, backup format, and format migration are
  not implemented.
- Independent cryptographic review is required before this is described as secure.
