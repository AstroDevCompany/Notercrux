# Development

## Prerequisites

Rust stable, Node 22+, pnpm 10, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/)
(Xcode command line tools on macOS; WebView2 and MSVC build tools on Windows).

## Commands

| Command          | What it does                                                   |
| ---------------- | -------------------------------------------------------------- |
| `pnpm install`   | Install JS dependencies (Cargo fetches Rust ones on build).    |
| `pnpm dev`       | Run the native desktop app with hot reload.                    |
| `pnpm web`       | Run the UI alone at http://localhost:1420 with a mock backend. |
| `pnpm build`     | Produce a desktop bundle.                                      |
| `pnpm test`      | All TypeScript and Rust tests.                                 |
| `pnpm typecheck` | TypeScript checks.                                             |
| `pnpm lint`      | ESLint and Clippy (warnings are errors).                       |
| `pnpm format`    | Prettier and rustfmt.                                          |

Individual crates: `cargo test -p notercrux-storage`, `cargo test -p notercrux-sync`, etc.

## Where the app stores data

The SQLite file `notercrux.db` is in the Tauri app-data directory (on macOS
`~/Library/Application Support/app.notercrux.desktop/`). UI preferences (theme, transparency,
motion) and sidebar state use the webview's `localStorage`.

## Testing

See [testing.md](testing.md).

## Browser preview vs native

`pnpm web` is fast but **is not the app**: there is no native window material and the backend is a
mock. Anything about the native window, SQLite behaviour or platform rendering must be checked with
`pnpm dev`.
