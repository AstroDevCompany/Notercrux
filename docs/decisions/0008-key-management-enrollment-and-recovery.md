# ADR 0008: Key management, enrollment, revocation and recovery

**Status:** Accepted for implementation (Phase 4). Refines ADR 0004. **Not independently reviewed.**

## Decisions

- **Key ring with epochs.** The workspace key is a ring of 256-bit keys, one per epoch. New data is
  sealed under the newest epoch; old epochs stay so history remains readable. Purpose keys (content,
  attachment, metadata) are derived from each epoch key with HKDF-SHA256. Document and attachment
  ids are derived from the **epoch 0** key and so never change when keys rotate.
- **Device keys.** Each device has an Ed25519 key (request signing) and an X25519 key (receiving
  key rings). The device id is a hash of both public keys, so an id cannot be claimed for other
  keys. Secret halves are kept in the OS secure storage (Keychain, Credential Manager, Secret
  Service) and nowhere on the server. If the OS store is unusable the app falls back to a
  user-only-readable file and says so in Settings.
- **Enrollment.** An existing device creates an invite at the server and shows a code containing a
  random 128-bit **invite secret**. The server never sees the secret. The new device sends its public
  keys with `HMAC(secret, keys)` and its name sealed under the secret. The existing device checks
  the HMAC (so a request that did not come from someone holding the code, such as one the server
  made up, is refused), re-seals the name under the ring, and wraps the key ring for the new device
  with an ephemeral X25519 agreement mixed with the invite secret. Both screens also show a short
  authentication string derived from the new device's keys. Invites expire after 15 minutes and
  work once.
- **Revocation rotates the key.** Removing a device adds a new epoch to the ring and wraps the new
  ring for every remaining device in one request; the server only accepts the request if the wraps
  cover exactly the remaining devices. The revoked device is locked out of the API immediately and
  cannot read anything sealed under the new epoch. Anything it already downloaded stays readable by
  it; that cannot be undone.
- **Key updates are verified.** A key ring delivered by anyone is only adopted if it contains every
  key the device already holds, unchanged. The server (which can wrap a ring for any public key) can
  therefore not substitute a ring of its own.
- **Recovery key.** 256 random bits, shown once as 14 groups of base32 with a checksum. It derives
  a key pair that behaves like a device that exists only on paper: the server stores its public
  half; every device wraps the current key ring for it (so the recovery copy stays up to date after
  each rotation without any device ever holding the secret). To recover, a new device proves
  possession of the recovery key by signing its own id, the server registers it and returns the
  recovery copy, and the person's recovery key opens it. Recovery does not remove lost devices;
  the person removes them afterwards, which rotates the key again.
- **Joining or recovering with existing local pages** offers two explicit choices: keep them and
  add them to the workspace, or replace them.

## What this does not do

- No passphrase or biometric unlock of the local database, and the local SQLite file is not
  encrypted at rest.
- A stolen unlocked device, or malware running as the user, can read everything on it.
- Lost devices **and** a lost recovery key mean the data cannot be recovered. That is the price of
  the server being unable to help.
- Device names are visible to every member of the workspace but not to the server.
- No cryptographic review has taken place. No custom primitives are used.
