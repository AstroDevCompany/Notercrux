# Security

> **Status: design spike only.** The shipping desktop app does not encrypt data and does not sync.
> Nothing here has been independently reviewed. See [ADR 0004](../decisions/0004-e2ee-key-hierarchy.md).

## Goals

- Content is encrypted on-device before it leaves it; the server stores ciphertext only.
- No server-held key can decrypt user content. Login and content encryption are separate.
- Recovery without the server, via a user-held recovery key.

## What the spike demonstrates (`crates/crypto`, `crates/sync`)

- Authenticated encryption with context binding; tampering, wrong keys and replay into another page
  fail.
- A relay that stores only ciphertext can still serve idempotent sync.
- Recovery-key wrap/unwrap with typo detection.
- Device enrollment via ECDH with a short authentication string.

## Known gaps

- Revocation does not rotate keys: a revoked device keeps the workspace key it already has.
- No OS keychain storage, passphrase unlock, backup format, or key/format migration.
- No attachments encryption wiring, no metadata minimisation analysis.
- Enrollment SAS strength and rate limiting are unevaluated.
- If every device and the recovery key are lost, data is unrecoverable by design.

## Threat model (initial, to be extended)

| Adversary                      | Intended protection                                           | Today         |
| ------------------------------ | ------------------------------------------------------------- | ------------- |
| Honest-but-curious sync server | Sees only ciphertext and minimal operational metadata         | Spike         |
| Active network/server attacker | Cannot read content; enrollment MITM caught by SAS comparison | Spike         |
| Stolen revoked device          | Should lose access after key rotation                         | Not designed  |
| Malware on an unlocked device  | Out of scope                                                  | —             |
| Local disk access              | Local database is currently **unencrypted**                   | Not addressed |
