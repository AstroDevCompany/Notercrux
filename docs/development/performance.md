# Performance baselines

Measured, not promised. These are the numbers from the machine used during development (Apple
silicon Mac, release build for Rust, Chromium-based browser preview for the UI). Re-measure on your
hardware before drawing conclusions, and never quote a number here that was not measured.

> The UI numbers were taken in the **browser preview**, not the native WKWebView/WebView2 windows,
> and with the preview pane hidden (paint time excluded). They show relative cost and algorithmic
> behaviour; native end-to-end numbers (cold start, input-to-paint latency, memory, idle CPU) are
> **not measured yet**.

## Storage (Rust, SQLite, in memory, `--release`)

Workspace: 11,110 pages in a 4-level tree. Reproduce with
`cargo test -p notercrux-storage --release --test large_workspace -- --ignored --nocapture`.

| Operation                                                 | Time                 |
| --------------------------------------------------------- | -------------------- |
| Create 11,110 pages (one transaction each, with indexing) | 4.4 s (≈0.4 ms/page) |
| `list_pages` (11,110 rows)                                | 7 ms                 |
| Save 500 documents (~50 blocks each)                      | 0.44 s               |
| Save a 20,000-block document                              | 18 ms                |
| Load a 20,000-block document                              | 3 ms                 |
| Search by title (20 hits)                                 | 1–5 ms               |
| Search by body text (20 hits, with excerpts)              | 8 ms                 |
| Move a page inside the big tree                           | 8 ms                 |
| Rebuild the whole search index                            | 6 s                  |

Finding that changed the code: FTS5's `snippet()` took **1.3 s** per search when one hit was a
20,000-block document, even though ranking took 0.6 ms. Excerpts are now built in Rust in one
linear pass (8 ms). The index rebuild is a rare recovery operation; it re-parses every document.

## Query engine and conversions (TypeScript, Node)

| Operation                                                | Time          |
| -------------------------------------------------------- | ------------- |
| Filter + two-level sort, 20,000 records                  | 11 ms         |
| Sort 20,000 records by title                             | 47 ms         |
| Document ⇄ editor model, 10,000 blocks (each way)        | ~13 ms        |
| Document → Markdown / Markdown → document, 10,000 blocks | 21 ms / 49 ms |

Filtering and sorting run on the client by design (a server cannot evaluate them over encrypted data).
`packages/document-model/src/scale.test.ts` guards these with generous budgets.

## Interface (browser preview)

Workspace: 3,000 root pages and one 10,000-block page.

| Operation                                                    | Before     | After          |
| ------------------------------------------------------------ | ---------- | -------------- |
| Open a small page with 3,000 pages in the sidebar            | 250–350 ms | 13–15 ms       |
| Sidebar rows in the DOM                                      | 3,000      | ~30 (windowed) |
| Open a 1,000-block page                                      | n/a        | 70–100 ms      |
| Open a 10,000-block page                                     | n/a        | 1.0–2.0 s      |
| Leave a 10,000-block page                                    | n/a        | ~0.46 s        |
| Keystroke in a 10,000-block page (script + layout, no paint) | n/a        | ~9 ms median   |

Finding that changed the code: every selection re-rendered all 3,000 sidebar rows. The tree now
renders only the visible window with memoized rows and stable handlers, and the database table is
windowed the same way.

## Sync capture cost (release build, in-memory SQLite)

Local changes are also written into the CRDT documents and queued as operations.

| Operation (sync on)                 | Cost         |
| ----------------------------------- | ------------ |
| Create a page (2,800 pages created) | 0.94 ms each |
| Rename a page                       | 1.9 ms       |
| Move a page (2,800-page tree)       | 8.3 ms       |
| Save a 100-block page               | 0.78 ms      |
| Save a 1,000-block page             | 5.0 ms       |
| Save a 5,000-block page             | 25.8 ms      |

Measured by `benchmark_sync_capture_cost` in `crates/storage/tests/large_workspace.rs` (ignored by
default; run with `-- --ignored --nocapture`). Not measured: network transfer, applying large remote
batches in the native app, CRDT history growth over months, memory over long sessions.

## Known limits

- A page with ~10,000 blocks (roughly 200 pages of text) takes 1–2 s to open: ProseMirror renders
  the whole document. Pages of a few hundred blocks open in well under 100 ms. Virtualizing the
  editor is not planned.
- Typing in the filter box of a database saves the view on every keystroke; very large databases
  would benefit from debouncing.
- Sync history is never compacted, so a heavily edited page document grows over time.
- Attachments up to 100 MB are held in memory while being displayed or transferred.
