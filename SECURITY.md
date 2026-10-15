# Security policy

Notercrux is **pre-alpha and has not been security reviewed**. Encryption code in this repository
is a feasibility spike (see [ADR 0004](docs/decisions/0004-e2ee-key-hierarchy.md)); the shipping app
does not yet encrypt data or sync. Do not store sensitive information in it.

## Reporting a vulnerability

Please report privately rather than in a public issue. Use GitHub's private vulnerability reporting
("Security" tab → "Report a vulnerability") on the repository once it is published. Include the
affected version or commit, reproduction steps, and the impact you expect. We aim to acknowledge
reports within a few days; this is a volunteer project, so no formal SLA is promised.

## Scope

In scope: the desktop app, the Rust crates, and (later) the sync server. Out of scope: social
engineering, denial of service against hosted infrastructure, and issues in third-party
dependencies without a Notercrux-specific impact (report those upstream).

See [docs/security](docs/security/README.md) for the threat model.
