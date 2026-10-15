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

## App icons and logos

The two master logos live in `branding/` (`Notercrux-logo-light.png`, `Notercrux-logo-dark.png`,
2000×2000, transparent rounded corners). Everything else is generated from them:

```bash
./scripts/generate-icons.sh   # macOS only: needs sips, iconutil, swift and pnpm
```

| Output                                                                  | Used for                                                                |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `src-tauri/icons/icon.icns`                                             | macOS app and Dock icon (artwork on Apple's 824/1024 grid, with shadow) |
| `src-tauri/icons/icon.ico`, `*.png`, `Square*Logo.png`, `StoreLogo.png` | Windows exe, taskbar, installers, Store tiles; Linux                    |
| `public/icon-{light,dark}.png`                                          | Browser/preview favicon (follows the OS theme)                          |
| `src/assets/logo-{light,dark}.png`                                      | In-app logo (Settings, empty state), follows the app theme              |
| `docs/assets/logo-{light,dark}.png`                                     | README header                                                           |

The bundle icons use the **light** logo. Rendered icons were checked by extracting the `.icns`
back out; the Windows taskbar/exe rendering has not been looked at on a Windows machine.
