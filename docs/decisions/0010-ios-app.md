# ADR 0010: iOS app on the shared Rust core

**Status:** Accepted for implementation (Phase 5). Not independently reviewed.

## Context

The iOS app must read, edit and sync the same data as the desktop app, work offline, enroll securely,
and never lose edits made on either side. Re-implementing storage, merging and encryption in Swift
would create a second implementation of the most delicate code and a second place for the two to
drift apart.

## Decision

- **One core.** The iOS app links the same Rust crates as the desktop app (`storage`, `crdt`,
  `crypto`, `sync`) through a thin crate, `crates/mobile`, exposed to Swift with UniFFI
  (generated bindings, an XCFramework with device and simulator slices, built by
  `scripts/build-ios-core.sh`). Swift contains no merge, crypto or protocol logic.
- **JSON is the contract.** Pages, documents, search hits, sync status and reports cross the
  boundary as the same JSON the desktop UI uses, and Swift decodes them with `Codable`. Block
  attributes the iOS app does not understand are preserved on save (a `JSONValue` type that
  round-trips numbers, so integers stay integers), so edits on the phone never strip what the desktop
  wrote.
- **SwiftUI, native.** Page tree (outline with search), a block editor with one text field per block,
  Return to split a block, Backspace at the start of a block to leave a list, delete an empty block
  or merge into the previous one (detected through an invisible anchor character at the start of each
  field, since SwiftUI reports nothing for Backspace in an empty field), a `/` command list under the
  block being typed in, to-do toggles, turn-into, duplicate, delete, move up and down, archive and
  restore. Titles save as they are typed.
- **Saves are rebased**, exactly as on desktop (ADR 0009): the editor remembers the document it last
  matched and the core merges `base to editor` onto the page as it is now, so a change arriving
  while typing is merged, not overwritten.
- **Inline formatting is preserved, not edited.** Bold, links and page links keep their position as
  text around them changes (marks are shifted by the common-prefix/suffix of the edit, in Unicode
  scalars); there are no formatting controls yet. Databases are not shown on iOS; their pages are
  kept in sync but hidden from the list. Images show; files show as a card; neither can be added yet.
- **Keys** live in the iOS Keychain (`AfterFirstUnlockThisDeviceOnly`: not in backups, not synced
  to iCloud), supplied to the core through a small `SecretVault` callback interface.
- **Enrollment** is joining by invite code (typed or pasted) with the verification code shown, or
  recovery with the recovery key. Creating a workspace on the phone is possible in the core but the
  UI offers only join and recover for now. Scanning a QR code is not built.
- **Sync** runs in the foreground (long poll through the same engine). When the app is backgrounded
  it asks iOS for a few seconds to finish a push. There is no push notification or background
  fetch, so changes from other devices arrive when the app is opened.

## Consequences

- Any change to a Rust type that crosses the boundary needs the Swift `Codable` mirror updated;
  `ContractTests` decode a desktop-shaped document to catch drift.
- The static library is about 50 MB per slice (debug symbols included); the release app is far
  smaller. Apple-silicon simulators and devices only (no x86_64 simulator).
- HTTP to a server on the local network works for testing; use HTTPS elsewhere. The Rust HTTP stack
  does not go through App Transport Security, but iOS Local Network permission applies.
- The app has not been through App Store review, has no icon variants beyond one 1024 px image, and
  no localization.
