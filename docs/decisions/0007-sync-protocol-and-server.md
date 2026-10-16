# ADR 0007: Sync protocol, server and PostgreSQL persistence

**Status:** Accepted for implementation (Phase 4). Not independently reviewed.

## Context

Phase 4 needs a server that lets a person's devices exchange changes without the server being able
to read them, that anyone can self-host, and that can also be run as a hosted service. Devices come
and go, networks fail, and apps are not updated in lockstep.

## Decision

**The server is a dumb, ordered, authenticated mailbox for ciphertext.**

- One total order of operations per workspace: the server assigns a sequence number (`seq`) when it
  accepts an operation. Clients pull "everything after cursor N". Pulling can long-poll (up to 25 s)
  and is woken by pushes on the same instance; other instances re-check every 2 s.
- An operation is `{ op_id, doc_id, epoch, format, payload }`. `payload` is an XChaCha20-Poly1305
  envelope whose plaintext carries the real document key and the CRDT update (see ADR 0009).
  `doc_id` is an HMAC of the document key under a workspace secret, so the server can group
  operations per document but cannot tell what they belong to. `op_id` is chosen by the client and
  stable across retries; the server stores each `op_id` once, which makes every push idempotent.
- **Authentication is by device signature**, not by a password or token. Every request carries the
  device id, a timestamp, a nonce and an Ed25519 signature over
  `NCX1 | method | path+query | time | nonce | sha256(body)`. The server checks the signature
  against the stored device key, rejects clocks more than 5 minutes off, and remembers nonces
  (in PostgreSQL, so a replay is caught on any instance). A revoked device's valid signature is
  answered with `device_revoked`, so it can tell the person what happened.
- Endpoints (all JSON except attachment chunks): create workspace, workspace info, delete workspace;
  create/inspect/approve invite, request to join, poll join result; revoke device; fetch key update;
  recover; push and pull operations; attachment chunk upload/download, status and commit; plus
  `/healthz` and `/metrics`.
- **Versioning.** Clients send `x-ncx-protocol`; a server that does not speak it answers `426` with
  `unsupported_protocol`. Operation payloads carry their own `format`. An app that meets a higher
  format keeps the operation (`sync_held`) instead of dropping or misapplying it, and applies it
  after an update.
- **Quotas and entitlements.** Limits (devices, operation bytes, attachment bytes, per-operation and
  per-attachment size) come from an `Entitlements` trait. Self-hosted servers use the static values
  from the environment; a hosted deployment can implement the trait against billing without
  touching the rest. Registration is `open`, `closed`, or gated by tokens.
- **Rate limiting** per device and per client address; **metrics** are Prometheus counters that
  never include identifiers or content.
- **Persistence** is behind a `Repo` trait with two implementations: in-memory (tests and trials)
  and PostgreSQL (`tokio-postgres` with a `deadpool` pool, own SQL migration runner guarded by an
  advisory lock, TLS through rustls with an optional extra CA). Plain PostgreSQL features only, so
  managed services such as Neon work. Attachment chunks live in a `BlobStore` (directory on disk, or
  memory), not in PostgreSQL, so large files do not bloat the database.
- Sequence numbers are allocated under a row lock on the workspace, so concurrent pushes through
  several instances still produce one gap-free order.

## Alternatives considered

- **WebSocket-only transport.** Better latency, harder to run behind simple proxies and to test.
  Long polling over plain HTTPS gives near-instant delivery and a single request style.
- **Server-side merge.** Would need plaintext. Rejected by the privacy goal.
- **Bearer tokens / OAuth for devices.** Adds a server-held secret per device and an account
  system. Signatures need only the device's public key, which the server already stores.
- **Storing chunks in PostgreSQL `bytea`.** Simpler, but expensive on hosted databases.

## Consequences

- The server learns metadata (see `docs/security/README.md`): workspace and device ids, document
  ids, sizes, timing, IP addresses, key epoch, counts. Not content, titles, names or structure.
- Operation history is append-only and unbounded until compaction exists (not built). The quota
  bounds the cost; compaction of long-lived workspaces is future work.
- Clock skew over 5 minutes locks a device out until its clock is fixed (the error says so).
- TLS termination for the HTTP API is expected from a reverse proxy; the server itself speaks plain
  HTTP. The PostgreSQL TLS path was tested locally with a private CA, not against a hosted service.
