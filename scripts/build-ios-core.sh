#!/usr/bin/env bash
# Builds the Rust core for iOS (device + simulator), generates the Swift bindings and packs an
# XCFramework into apps/ios/Core. Needs: rustup targets aarch64-apple-ios and aarch64-apple-ios-sim,
# Xcode command line tools.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=apps/ios/Core
PROFILE=${PROFILE:-release}
FLAG=$([ "$PROFILE" = release ] && echo --release || echo "")
export IPHONEOS_DEPLOYMENT_TARGET=17.0

for t in aarch64-apple-ios aarch64-apple-ios-sim; do
  cargo build -p notercrux-mobile --target "$t" $FLAG
done

# Bindings come from the compiled library's embedded metadata.
cargo build -p notercrux-mobile --features cli --bin uniffi-bindgen $FLAG
GEN=target/ios-bindings
rm -rf "$GEN" "$OUT/NotercruxCore.xcframework" && mkdir -p "$GEN" "$OUT"
LIB=target/aarch64-apple-ios/$PROFILE/libnotercrux_mobile.a
target/$PROFILE/uniffi-bindgen generate --language swift --library "$LIB" --out-dir "$GEN"

# Headers + module map for the framework (the module must be named like the generated FFI module).
HDR="$GEN/headers"
mkdir -p "$HDR"
cp "$GEN"/*.h "$HDR/"
cp "$GEN"/*.modulemap "$HDR/module.modulemap"

xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/$PROFILE/libnotercrux_mobile.a -headers "$HDR" \
  -library target/aarch64-apple-ios-sim/$PROFILE/libnotercrux_mobile.a -headers "$HDR" \
  -output "$OUT/NotercruxCore.xcframework"
cp "$GEN"/notercrux_mobile.swift "$OUT/notercrux_mobile.swift"
echo "built $OUT"
