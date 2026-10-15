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
