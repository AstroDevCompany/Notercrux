# Security

> **Status: implemented, not independently reviewed.** Encryption and sync work end to end and are
> covered by automated tests, but no outside cryptographer or security engineer has examined the
> design or the code. Do not rely on Notercrux to protect information whose exposure would cause
> serious harm. Design: [ADR 0004](../decisions/0004-e2ee-key-hierarchy.md),
> [ADR 0007](../decisions/0007-sync-protocol-and-server.md),
> [ADR 0008](../decisions/0008-key-management-enrollment-and-recovery.md).

## Goals

- Content is encrypted on the device before it leaves it; the server stores ciphertext only.
- No key held by the server can decrypt content. Authentication (device signatures) is separate
  from content encryption.
- A removed device cannot read anything written after its removal.
- Recovery without the server's help, through a recovery key the person keeps.

## Primitives

XChaCha20-Poly1305 (authenticated encryption, random 192-bit nonces), HKDF-SHA256 and HMAC-SHA256,
X25519 (key agreement), Ed25519 (request signatures), SHA-256, all from the RustCrypto and
dalek crates. No custom constructions. Every ciphertext is bound to its context (workspace id,
document id, key epoch; for attachment chunks also the blob id, chunk index and chunk count), so
ciphertexts cannot be moved between documents, reordered or replayed.

## What the server can see

| Visible to the server                                                     | Hidden from the server                                |
| ------------------------------------------------------------------------- | ----------------------------------------------------- |
| Workspace id, device ids and public keys, when devices joined, last seen  | Page titles, text, structure, databases, properties   |
| Opaque document ids (keyed hashes), sizes and times of operations         | Which document an operation belongs to (only grouped) |
| Opaque attachment ids, chunk sizes and counts, total size                 | File names, types, contents, whether two files match  |
| Key epoch, number of operations, storage use, IP addresses of connections | Device names (sealed with the workspace key)          |
| That a recovery key exists (its public half)                              | The recovery key itself                               |

Timing and sizes can reveal activity patterns (for example that a large attachment was added at a
certain time). They are not padded or batched.

## Threat model

| Adversary                                     | Protection                                                                                                                                                    | Tested by                                                                         |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Honest-but-curious server operator            | Sees only the metadata above                                                                                                                                  | `security.rs`: scan of everything stored (operations, labels, wraps, attachments) |
| Server that tampers with data                 | Modified, swapped or re-addressed operations fail authentication and are skipped and counted; reordered attachment chunks fail; a damaged response is retried | `security.rs`, `attachments.rs`                                                   |
| Server that forges a key update               | A ring is only adopted if it contains every key the device already holds                                                                                      | `a_forged_key_update_from_the_server_is_refused`                                  |
| Server that interferes with joining           | A request without the invite secret is flagged and cannot be approved; invites are single use and expire                                                      | `a_join_request_without_the_invite_secret_is_never_approved`                      |
| Network attacker                              | TLS (from the reverse proxy) plus signed requests with timestamp and nonce; tampered bodies, other paths, replays and skewed clocks are rejected              | `api.rs` in `apps/sync-server`                                                    |
| Removed (revoked) device                      | Locked out of the API at once; the key rotates, so new data is sealed under a key it never receives                                                           | `a_revoked_device_...`, `remaining_devices_pick_up_the_rotated_key...`            |
| Another workspace's device                    | Cannot authenticate against this workspace                                                                                                                    | `rejects_unsigned_badly_signed_and_foreign_requests`                              |
| Lost device, no other device                  | Recovery key restores the workspace, including after key rotation                                                                                             | `recovery_restores_everything_from_the_phrase_alone`                              |
| Malware or a thief on an unlocked device      | **Out of scope**                                                                                                                                              | n/a                                                                               |
| Someone with the disk of a powered-off device | The local SQLite database is **not encrypted at rest**; device keys are in the OS keychain                                                                    | Not addressed                                                                     |

## Known gaps and limits

- **No independent review** of the protocol or implementation.
- The local database and its attachment copies are plaintext on disk. Full-disk encryption is the
  recommended protection. Searching works on the plaintext index.
- Device keys use the OS secure storage when available. Otherwise a private file is used (clearly
  shown in Settings); the macOS Keychain path has not been exercised in this repository's automated
  tests, and the Windows and Linux paths have not been run at all.
- A removed device keeps whatever it already downloaded.
- Rate limiting is per device and per address and is basic; there is no account lockout, abuse
  detection or CAPTCHA, because there are no passwords.
- The server does not check that a device's clock is honest beyond a 5 minute window.
- Operation history grows without compaction; deleting a workspace on the server is possible, but
  deleting single old operations is not.
- Attachments are not removed when the last block referencing them is deleted.
- If every device and the recovery key are lost, the data cannot be recovered. This is by design.

## Reporting vulnerabilities

See [SECURITY.md](../../SECURITY.md).
