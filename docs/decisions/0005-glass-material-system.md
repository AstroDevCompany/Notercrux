# ADR 0005 — Glass material system

**Status:** Accepted

## Decision

Glass is a token-driven material system, not per-component styling.

- **Native layer:** the window is transparent. macOS uses the `sidebar` vibrancy effect and Windows
  uses `mica`, both applied through Tauri `windowEffects` and re-applied at runtime from Settings
  (`apps/desktop/src/hooks/useMaterial.ts`). Linux has no native material and uses the CSS fallback.
- **CSS layer:** three classes — `.glass-sidebar`, `.glass-overlay`, `.glass-popover` — are the only
  users of `backdrop-filter`. Blur radius, tint and edge are tokens in `styles/tokens.css`, tuned
  separately for light and dark.
- **Legibility:** the editor surface stays ≥ 86 % opaque; only navigation and floating surfaces are
  strongly translucent.
- **Accessibility:** `Glass effects → Reduced` (or the OS "reduce transparency" preference) swaps
  every material for an opaque token and clears native vibrancy. `Motion → Reduced` zeroes
  durations. The interface is designed to look intentional in this mode, not merely degraded.

## Verification status

Verified: CSS materials in a browser (light/dark), reduced-transparency tokens by inspection.
**Not yet verified:** the native macOS vibrancy and Windows Mica windows, Windows rendering of
`backdrop-filter` in WebView2, and performance with many stacked panels. See
`docs/development/testing.md`.

## Addendum — what `backdrop-filter` can and cannot blur

`backdrop-filter` only blurs **web content** behind an element, never the native window material
(macOS vibrancy, Windows Mica) behind the webview. In the native app a menu over the sidebar
therefore has nothing to blur and looks merely see-through. Consequences, applied in Phase 3:

- Floating surfaces (menus, popovers, palette, dialogs) use higher-opacity fills (about 0.9) so they
  stay legible and read as frosted where web content is behind them, instead of relying on blur.
- The sidebar drops its (no-op) `backdrop-filter` in the native app and relies on the native
  material alone; the filter remains in the browser preview.
- Menu item highlighting follows the pointer through focus, not CSS `:hover`, because WebKit may not
  repaint `:hover` inside composited, filtered layers. This is a reasoned fix for a reported
  symptom; it has **not** been verified in the native window.
