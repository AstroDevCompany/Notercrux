#!/usr/bin/env bash
# Regenerates every app icon from the two master logos in branding/.
#
#   branding/Notercrux-logo-light.png  -> bundle icons (.icns/.ico/PNGs), favicon, README
#   branding/Notercrux-logo-dark.png   -> dark-theme variants (favicon, in-app logo)
#
# Needs: macOS (sips, iconutil, swift) and pnpm. Run from anywhere.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/apps/desktop"
ICONS="$APP/src-tauri/icons"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# 1. Full-bleed master for Windows/Linux (these platforms do not add margins around icons).
sips -z 1024 1024 "$ROOT/branding/Notercrux-logo-light.png" --out "$TMP/master.png" >/dev/null
(cd "$APP" && pnpm tauri icon "$TMP/master.png" --output "$ICONS" >/dev/null)
rm -rf "$ICONS/android" "$ICONS/ios"   # mobile sets are not used yet

# 2. macOS icon: same art on Apple's 824-in-1024 grid, with shadow.
swift "$ROOT/scripts/macos-icon.swift" "$ROOT/branding/Notercrux-logo-light.png" "$TMP/mac1024.png"
SET="$TMP/icon.iconset"; mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$TMP/mac1024.png" --out "$SET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) "$TMP/mac1024.png" --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$ICONS/icon.icns"

# 3. Web/UI assets (favicon, in-app logo, README).
mkdir -p "$APP/public" "$APP/src/assets"
sips -z 512 512 "$ROOT/branding/Notercrux-logo-light.png" --out "$APP/public/icon-light.png" >/dev/null
sips -z 512 512 "$ROOT/branding/Notercrux-logo-dark.png"  --out "$APP/public/icon-dark.png"  >/dev/null
sips -z 256 256 "$ROOT/branding/Notercrux-logo-light.png" --out "$APP/src/assets/logo-light.png" >/dev/null
sips -z 256 256 "$ROOT/branding/Notercrux-logo-dark.png"  --out "$APP/src/assets/logo-dark.png"  >/dev/null
mkdir -p "$ROOT/docs/assets"
sips -z 512 512 "$ROOT/branding/Notercrux-logo-light.png" --out "$ROOT/docs/assets/logo-light.png" >/dev/null
sips -z 512 512 "$ROOT/branding/Notercrux-logo-dark.png"  --out "$ROOT/docs/assets/logo-dark.png"  >/dev/null
echo "icons generated"
