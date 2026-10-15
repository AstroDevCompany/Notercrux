# Architecture overview

> Describes what is **implemented today**. Planned components are marked as such. The full target
> architecture is in the [blueprint](../../Implementation/IMPLEMENTATION.md).

```
┌─────────────── apps/desktop (Tauri 2) ───────────────┐
│ React UI ── invoke() ──► src-tauri commands (thin)   │
│   Tiptap editor            │                         │
│   packages/document-model  ▼                         │
└────────────────────  crates/storage ─► SQLite file   │
                             │                         │
                       crates/domain  (pages, blocks)  │
└──────────────────────────────────────────────────────┘

crates/crypto + crates/sync: spikes, tested in isolation, not yet wired into the app.
```

## Components

| Component                 | Responsibility                                                                             |
| ------------------------- | ------------------------------------------------------------------------------------------ |
| `crates/domain`           | `Page`, `Block`, `Document`, id types, page-tree cycle rules. No I/O.                      |
| `crates/storage`          | SQLite schema + migrations, page operations (each one transaction), documents, FTS5 index. |
| `apps/desktop/src-tauri`  | Opens the database in the OS app-data dir, exposes storage as Tauri commands.              |
| `packages/document-model` | TypeScript types mirroring the domain; ProseMirror ⇄ `Document` conversion.                |
| `apps/desktop/src`        | UI: state store, sidebar tree, palette, dialogs, editor, design tokens.                    |
| `crates/crypto` (spike)   | Key hierarchy, XChaCha20-Poly1305 envelopes, recovery key, device enrollment.              |
| `crates/sync` (spike)     | Loro CRDT page documents and an encrypted-relay replica protocol.                          |

## Data model

- **Page**: `id`, `parentId`, `title`, `icon`, `position` (index among siblings), timestamps,
  `archivedAt`. Archiving applies to the whole subtree; restoring a child of an archived parent moves
  it to the root so it stays reachable.
- **Document** (one per page): `{ version, blocks[] }`. **Block**: `id`, `type`, `attrs`, `text`,
  `marks[]` (code-point ranges), `children[]`.
- **SQLite**: `pages` and `documents` are canonical. `search_index` (FTS5) is a cache rebuilt by
  `Store::rebuild_search_index`. Migrations are numbered SQL files applied in transactions and
  recorded in `schema_migrations`.

## Frontend ↔ backend

`apps/desktop/src/api` defines one `Api` interface with two implementations: Tauri `invoke`
(real) and a localStorage mock (browser preview only).

## UI structure

Tokens (`styles/tokens.css`) → materials (`styles/glass.css`) → components. Components use tokens,
never literal colours. Overlays use `Dialog`/`useDialog` (focus trap, Escape, focus restore). The
page tree follows the ARIA tree pattern with roving tab index.
