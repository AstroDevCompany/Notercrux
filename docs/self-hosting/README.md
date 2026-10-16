# Self-hosting the sync server

The sync server (`apps/sync-server`) stores and relays **encrypted** data. It never has the keys, so
it can be run on a small machine you do not fully trust, though it will see the metadata listed in
[docs/security](../security/README.md). You can use the app without any server.

> The server has been run and tested on macOS against PostgreSQL 17, including TLS to the database
> with a private CA. The container files below and the Linux and Windows builds have **not** been
> run. Treat them as a starting point.

## Quick trial (in memory)

```bash
cargo run --release -p notercrux-sync-server
```

Listens on `127.0.0.1:8787` and keeps everything in memory (lost on exit). In the app: Settings,
Sync, Set up, Server `http://127.0.0.1:8787`. Use `http://` only on `localhost` or a network you
trust; use HTTPS otherwise.

## Real deployment

1. **PostgreSQL 14 or newer** (tested on 17). Any plain Postgres works, including managed services
   such as Neon. Create an empty database and a user that owns it. The server creates its tables on
   start and upgrades them on later starts (safe to run from several instances).
2. **A directory for attachments** on a persistent disk (`NCX_BLOB_DIR`). Back it up together with
   the database; they belong together.
3. **A reverse proxy that terminates HTTPS** (Caddy, nginx, a cloud load balancer) in front of the
   server. Pass the client address through; allow responses of up to 30 s (long polling).
4. Start the server:

```bash
NCX_BIND=0.0.0.0:8787 \
NCX_DATABASE_URL='postgres://notercrux:SECRET@db.internal:5432/notercrux' \
NCX_BLOB_DIR=/var/lib/notercrux/blobs \
NCX_REGISTRATION=token:CHOOSE-A-LONG-RANDOM-TOKEN \
notercrux-sync-server
```

For a database that requires TLS add `?sslmode=require` to the URL. Certificates are checked
against the public roots; for a private CA also set `NCX_DATABASE_CA=/path/to/ca.pem`.

## Configuration (environment variables)

| Variable                                | Default          | Meaning                                                                        |
| --------------------------------------- | ---------------- | ------------------------------------------------------------------------------ |
| `NCX_BIND`                              | `127.0.0.1:8787` | Address to listen on.                                                          |
| `NCX_DATABASE_URL`                      | unset            | PostgreSQL URL. Unset means in memory.                                         |
| `NCX_DATABASE_CA`                       | unset            | PEM bundle of extra certificate authorities for the database connection.       |
| `NCX_BLOB_DIR`                          | unset            | Directory for encrypted attachments. Unset means in memory.                    |
| `NCX_REGISTRATION`                      | `open`           | `open`, `closed`, or `token:a,b`. With tokens, creating a workspace needs one. |
| `NCX_MAX_DEVICES`                       | 10               | Devices per workspace.                                                         |
| `NCX_MAX_OP_BYTES`                      | 2 GiB            | Stored operation bytes per workspace.                                          |
| `NCX_MAX_BLOB_BYTES`                    | 10 GiB           | Stored attachment bytes per workspace.                                         |
| `NCX_MAX_OP_SIZE` / `NCX_MAX_BLOB_SIZE` | 4 MiB / 512 MiB  | Largest single operation / attachment.                                         |
| `NCX_DEVICE_RATE` / `NCX_IP_RATE`       | 1200 / 6000      | Requests per minute per device / per address.                                  |
| `NCX_METRICS_TOKEN`                     | unset            | If set, `/metrics` requires `Authorization: Bearer <token>`.                   |
| `RUST_LOG`                              | `info`           | Log level.                                                                     |

Leave `NCX_REGISTRATION=open` only on a private network. Anyone who can reach an open server can
create workspaces and use your storage.

## Operating it

- **Health:** `GET /healthz` returns `ok` when the database answers.
- **Metrics:** `GET /metrics` (Prometheus text): request, authentication-failure, rate-limit and
  error counts, operations and bytes accepted and delivered, attachment chunks, workspaces
  created, devices revoked. No identifiers or content.
- **Backups:** `pg_dump` the database and copy `NCX_BLOB_DIR`. Backups are ciphertext. Restoring
  them restores sync for every device; devices are untouched. Without a backup, a lost server only
  loses sync history: each device still has its full copy and can create a new workspace.
- **Upgrades:** stop, replace the binary, start. Migrations run automatically. An older app talking
  to a newer server keeps working while the protocol version matches; an app that is too old gets a
  clear "update the app" error. Operations from a newer app are held by older apps, not lost.
- **Removing a workspace:** a device owner can delete it from Settings (all server data for that
  workspace is erased); devices keep their pages.
- **Logs** contain no content. They do contain errors from the database and the file system.

## Container (unverified)

```dockerfile
FROM rust:1 AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p notercrux-sync-server

FROM debian:stable-slim
RUN useradd --system --create-home notercrux && mkdir -p /var/lib/notercrux/blobs && chown notercrux /var/lib/notercrux/blobs
COPY --from=build /src/target/release/notercrux-sync-server /usr/local/bin/
USER notercrux
ENV NCX_BIND=0.0.0.0:8787 NCX_BLOB_DIR=/var/lib/notercrux/blobs
EXPOSE 8787
CMD ["notercrux-sync-server"]
```

A matching `docker-compose.yml` runs this next to `postgres:17` with volumes for `/var/lib/postgresql/data`
and `/var/lib/notercrux/blobs`, passing `NCX_DATABASE_URL=postgres://notercrux:...@db/notercrux`.
Neither file has been built here.

## What a hosted service would add

The server has a seam for it (`Entitlements`: per-workspace limits looked up at request time) but no
billing, accounts, usage dashboards or multi-region story. Those are not part of this repository.
