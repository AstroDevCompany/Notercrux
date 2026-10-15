# ADR 0006: Databases, properties and page links

**Status:** Accepted for the desktop app; interaction with sync is **not yet designed** (see below).

## Decisions

- **A database is a page** (`kind = database`) whose **child pages are its records**. Records are
  ordinary pages with a title and a document, so every page feature (editor, archive, duplicate,
  search, links) works on them. Records are hidden from the sidebar tree and managed in the table.
- **Schema** (`DatabaseSchema`): typed properties (text, number, checkbox, select, multi-select,
  date, URL, relation) and saved views. Defined and validated in Rust (`crates/domain`), mirrored
  in TypeScript (`packages/document-model/src/database.ts`).
- **Values** live in `page_properties` (one row per non-empty value), validated against the schema
  on every write. Changing a property's type, or removing a property or select option, deletes the
  values it invalidates **in the same transaction**, so a stored value never disagrees with its
  property. The UI asks for confirmation before destructive schema changes.
- **Views** hold sorts, filters (AND/OR) and hidden columns, and are part of the schema. Select
  properties sort by option order; empty cells always sort last.
- **Queries run on the client.** Rust validates structure but never evaluates filters or sorts, so
  the model is unchanged when the server only sees ciphertext (blueprint §6).
- **Relations** are one-way lists of page ids. Targets that were deleted or archived are hidden in
  the UI. There are no two-way relations, rollups or formulas yet.
- **Records cannot be moved into or out of a database**: their values are defined by that
  database's schema. Duplicating a database copies its schema, records and values.
- **Page links** are a `pageLink` mark over the link text, carrying the target id (`[[` opens the
  picker). The `page_links` table is a **rebuildable cache** derived from documents and powers the
  "Linked from" list. Visible link text is a snapshot of the title and is not rewritten on rename.
  Markdown export uses `notercrux://page/<id>`.

## Not decided / risks

- **Sync:** schema edits and property values must merge across devices (concurrent type change vs
  value edit, option removal vs selection). The current rules assume a single writer. This needs a
  design alongside ADR 0003 before sync ships.
- Property values are not full-text searched.
- An editor "table" block is not implemented; "table" means the database table view.
