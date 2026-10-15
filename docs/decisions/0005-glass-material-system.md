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

## Addendum — what `backdrop-filter` can and cannot blur (revised)

`backdrop-filter` only blurs **web content** behind an element, never the native window material
(macOS vibrancy, Windows Mica) behind the webview. Two lessons so far:

1. _First attempt (wrong):_ because menus over the sidebar had nothing to blur, fills were raised to
   about 0.9 opacity. That made the frost invisible everywhere, including over page content where
   blurring works. Reported as "not frosted glass".
2. _Current approach:_ floating surfaces use **low-opacity fills (about 0.5–0.6) with strong blur
   (36–44 px), saturation 1.9 and a diagonal sheen gradient** (`--mat-sheen`). Over page content the
   text behind is visibly smeared; over the sidebar the surface still reads as glass because of the
   sheen, edge highlight and tint, even though the native material under it cannot be blurred.
   Reduced transparency removes the sheen and uses solid fills.

Other decisions:

- The sidebar drops its (no-op) `backdrop-filter` in the native app and relies on the native
  material alone; the filter remains in the browser preview.
- Menu item highlighting follows the pointer through focus, not CSS `:hover`, because WebKit may not
  repaint `:hover` inside composited, filtered layers.
- Autocomplete lists (slash menu, page picker, palette) hide their scrollbars; they still scroll.

Verification: the frosted look and the hover behaviour were checked in a browser preview
(Chromium). They have **not** been confirmed in the native WKWebView/WebView2 windows.
