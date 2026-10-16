# Notercrux for iOS

SwiftUI app on the shared Rust core ([ADR 0010](../../docs/decisions/0010-ios-app.md)).

## Build and run

Requirements: Xcode 16 or newer, `xcodegen` (`brew install xcodegen`), Rust with the iOS targets
(`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`).

```bash
./scripts/build-ios-core.sh          # Rust core -> apps/ios/Core (XCFramework + Swift bindings)
cd apps/ios && xcodegen generate     # creates Notercrux.xcodeproj
open Notercrux.xcodeproj             # run on a simulator, or a device with your own team
```

On a physical device set your team in `project.yml` (`DEVELOPMENT_TEAM`) and, with a free Apple
account, remember the limit of three installed development apps.

## Try sync against a stand-in desktop

```bash
NCX_BIND=0.0.0.0:8787 cargo run -p notercrux-sync-server        # a server the phone can reach
cargo run -p notercrux-sync --example peer -- http://<your-mac-ip>:8787 /tmp/peer
```

The peer prints an invite code and approves the first device that asks to join. In a debug build the
invite can be passed at launch (`NCX_INVITE=<code>`), which opens the sync sheet and submits it;
`xcrun devicectl device process launch --environment-variables '{"NCX_INVITE":"..."}' app.notercrux.ios`
does this on a device. iOS asks once for Local Network access.

## Tests

```bash
xcodebuild -project Notercrux.xcodeproj -scheme Notercrux \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' test -only-testing:NotercruxTests
```

`NotercruxTests` runs the real Rust core inside the test host. `NotercruxUITests` drives the join
flow against a running server and peer; it needs room for one more installed app when run on a
device.
