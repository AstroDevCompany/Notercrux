**Notercrux — Master Implementation Blueprint**

Project specification · Architecture, design system, implementation roadmap, and agent execution protocol

Notercrux is an open-source, local-first Notion alternative designed for individuals who want a powerful page editor, a flexible organization system, and dependable cross-device synchronization without being locked into a proprietary cloud.

The goal is not to reproduce every Notion feature. It is to build a polished, faster, lighter, more transparent alternative that does the essential things exceptionally well, while also implementing as many high value features as possible.

Keep in mind that the mobile app is the last thing to do, start by focusing on the desktop version.

**1. Product principles**

Every architectural and product decision should follow these principles.

Editor and pages first

Nested pages, rich content, databases, navigation, shortcuts, and the editing experience receive the majority of development effort.

Private by design

End-to-end encryption, user-controlled data, explicit recovery procedures, and no hidden server-side decryption.

Local-first, fully offline

Core reading, editing, navigation, and local search work without a network connection. Changes synchronize when connectivity returns.

Exceptional visual design

A refined, glass-inspired interface with translucent surfaces, subtle depth, carefully controlled blur, and consistent motion.

Open and maintainable

AGPL-3.0, documented architecture, portable data, comprehensive tests, and a self-hostable synchronization service.

**Explicit exclusions for the initial product**

Do not implement AI assistants, agents, AI-generated content, charts, or unrelated advanced features simply because they exist in Notion. Avoid building a general-purpose workspace platform for teams.

The initial audience is individuals. Collaboration may be considered later, but it must not dictate the initial architecture or delay the core product.

**2. Technology stack**

Use established, actively maintained components where they meaningfully reduce complexity. Notercrux should have its own architecture and identity without reinventing rich-text editing, cryptography, or synchronization primitives.

| **Area**                | **Recommended foundation**                                                                                   |
| ----------------------- | ------------------------------------------------------------------------------------------------------------ |
| Desktop                 | Tauri 2, React, TypeScript, Rust                                                                             |
| iOS                     | Native SwiftUI                                                                                               |
| Shared core             | Rust crates for domain logic, cryptography, and sync protocol, exposed to Swift through a suitable FFI layer |
| Rich-text editor        | ProseMirror-based engine, such as Tiptap, with a custom Notercrux block and interaction layer                |
| Local database          | SQLite                                                                                                       |
| Sync backend            | Rust API service with PostgreSQL                                                                             |
| Initial hosted database | Neon/PostgreSQL                                                                                              |
| Attachments             | Separate encrypted blob storage                                                                              |
| Authentication          | Device-aware account and pairing system                                                                      |
| Encryption              | Established, audited cryptographic libraries and standard constructions                                      |
| Licensing               | AGPL-3.0                                                                                                     |

These are architectural starting points, not permission to add dependencies indiscriminately. Before selecting major libraries, agents must check licensing, maintenance, platform compatibility, security history, and performance.

**Important implementation boundary**

The desktop and iOS apps should share the data model, cryptographic rules, and synchronization protocol. They do not need to share the entire UI implementation.

Desktop gets a full workspace and advanced editing experience. iOS gets a native interface that preserves the same document semantics and synchronization behavior. Avoid maintaining two independent implementations of core data rules.

**3. Glass-first visual design system**

This is a core product requirement, not a final styling pass.

Visual references for the intended direction, not final Notercrux mockups.

**3.1 Window and application surfaces**

The desktop app should feel as though its UI is composed of carefully layered glass rather than opaque boxes.

- Use native window transparency where the platform and Tauri configuration support it reliably.
- Explore translucent backgrounds, background blur, vibrancy, and subtle gradients.
- Make the sidebar, floating controls, overlays, and content surfaces visually distinct through controlled transparency and depth.
- Allow the desktop environment to contribute to the visual appearance where appropriate.
- Support light and dark modes with deliberately tuned materials rather than simply inverting colors.
- Avoid making the entire page editor so transparent that text becomes difficult to read.

**3.2 Apply glass wherever it makes sense**

Create a reusable material system covering:

- Command palette and search overlays.
- Modal dialogs and confirmation prompts.
- Popovers, contextual menus, and dropdowns.
- Floating toolbars and selection controls.
- Settings overlays and auxiliary panels.
- Navigation surfaces and side panels where suitable.
- Tooltips and transient interface elements when the effect remains legible.
- Any future floating component that benefits from the same treatment.

Not every surface must have identical opacity or blur. The objective is consistent glass language, not indiscriminate transparency.

**3.3 Performance and accessibility**

Glass effects can be expensive and can reduce legibility. Therefore:

- Prefer native platform materials where they provide the desired appearance reliably.
- Reuse material tokens instead of independently tuning every component.
- Avoid excessive nested blurs, giant backdrop filters, and unnecessary animated effects.
- Provide reduced-transparency and reduced-motion accommodations.
- Maintain contrast, keyboard focus visibility, and accessible interaction sizes.
- Test the real application on macOS and Windows; don’t assume that a web preview accurately represents a native transparent window.

Acceptance criterion: the app looks intentionally designed even when effects are disabled, and remains responsive when multiple floating panels are visible.

**4. Application architecture**

flowchart TB

subgraph Clients["Notercrux clients"]

Desktop["Tauri desktop"]

IOS["SwiftUI iOS"]

end

subgraph Core["Shared domain and security layer"]

Model["Document and block model"]

Crypto["Key management and encryption"]

Sync["Sync protocol and conflict handling"]

end

subgraph Local["Per-device local state"]

SQLite["SQLite database and indexes"]

Blobs["Encrypted attachment cache"]

Keychain["OS secure key storage"]

end

subgraph Server["Optional sync service"]

API["Authentication and sync API"]

PG["PostgreSQL / Neon"]

Storage["Encrypted blob storage"]

end

Desktop --> Model

IOS --> Model

Model --> Crypto

Model --> Sync

Desktop <--> SQLite

IOS <--> SQLite

Crypto <--> Keychain

Sync <--> API

SQLite <--> Blobs

API <--> PG

API <--> Storage

This diagram describes logical responsibilities. The exact process boundaries and bindings should be validated during the architecture spike.

**Repository structure**

Use a monorepo to keep shared schemas, client code, backend code, tests, and documentation versioned together.

notercrux/

├── apps/

│   ├── desktop/

│   ├── ios/

│   └── sync-server/

├── packages/

│   ├── editor/

│   ├── ui/

│   ├── document-model/

│   ├── sync-protocol/

│   └── shared-types/

├── crates/

│   ├── domain/

│   ├── crypto/

│   ├── sync/

│   └── storage/

├── migrations/

├── tests/

│   ├── integration/

│   ├── sync/

│   ├── security/

│   └── fixtures/

├── docs/

│   ├── architecture/

│   ├── development/

│   ├── user-guides/

│   ├── security/

│   ├── self-hosting/

│   └── decisions/

├── scripts/

├── .github/

│   └── workflows/

├── README.md

├── CONTRIBUTING.md

├── SECURITY.md

├── LICENSE

└── ...

The exact folder names may change after the repository is initialized, but shared contracts must remain clearly separated from platform-specific UI.

**5. Document model and local persistence**

**5.1 Pages and blocks**

Represent content as structured documents rather than storing the entire workspace as one large HTML string or Markdown file.

A page needs stable identifiers, metadata, a parent relationship, and structured content. Blocks need stable IDs, explicit types, typed attributes, and well-defined ordering.

Initial block types should cover the common editing workflow:

- Paragraphs and headings.
- Bulleted, numbered, and check lists.
- To-do items.
- Quotes and callouts.
- Code blocks with language metadata.
- Dividers.
- Links, images, files, and other embedded content.
- Tables and other essential structured content.
- Page links and nested page references.

Support block movement, nesting where appropriate, duplication, deletion, and keyboard-driven editing.

**5.2 SQLite**

SQLite provides local persistence and fast queries. Use transactions for logically related updates and maintain a local index for navigation and search.

Keep the canonical document representation distinct from the local query cache. If an index is corrupted or becomes stale, it should be rebuildable from persisted documents.

Store large attachments separately. The database should hold metadata and references, not become a giant binary-file container.

**5.3 Databases**

Database views and properties are important enough to include in the core roadmap, but they should follow the foundational page and editor system.

Start with a minimal viable structured database model: records, typed properties, relations, sorting, filtering, and useful views. Define how those structures behave under encryption before implementing sophisticated views.

Charts and unrelated visualization features are out of scope.

**6. End-to-end encryption and key recovery**

This needs to be designed before the sync service is treated as production-ready.

**Security requirements**

- Content is encrypted on the user’s device before leaving it.
- The server stores encrypted document payloads and encrypted attachments.
- The server does not possess a hidden key that can decrypt users’ content.
- Keys are stored locally using platform-appropriate secure storage.
- New-device enrollment is an explicit, authenticated process.
- Backups and recovery follow the same encryption guarantees.
- Cryptographic formats, key derivation, nonce handling, and versioning use established constructions and libraries—not custom cryptographic algorithms.

Account authentication and content encryption are separate concerns. A successful login must not automatically imply that the server can read the workspace.

**Recovery and device transfer**

Implement a documented recovery-key flow. Users must be able to recover their encrypted workspace on a replacement device without requiring the server to decrypt it for them.

The product must clearly explain the consequences of losing both the available devices and the recovery material. There must be no misleading promise that the service can recover plaintext after every possible key-loss scenario.

Document:

- Initial key generation.
- Device enrollment and revocation.
- Recovery-key generation and secure storage.
- Restoring access on a new device.
- Encrypted backup and restore.
- What happens when a recovery key is lost.
- How encryption-key and format migrations work.

Include threat-model documentation and tests. Do not describe the product as fully secure merely because encryption code exists; security-sensitive design deserves independent review before a production release.

**Search and databases under E2EE**

Encryption creates an important constraint: the server cannot run ordinary full-text search over encrypted content.

Plan for local indexing and client-side search. Likewise, formulas, filters, relations, and database views that need content access must be computed on the client where necessary. The server may process limited operational metadata, but that metadata should be minimized and documented.

**7. Offline editing and synchronization**

Offline support is a primary feature, not a later optimization.

A user must be able to open pages, edit content, navigate, and search local data while disconnected. Pending changes must survive application restarts and synchronize after reconnection.

**Synchronization requirements**

- Durable local change tracking.
- Resumable uploads and downloads.
- Retry behavior with backoff.
- Idempotent server operations.
- Device tracking and revocation.
- Attachment synchronization.
- Explicit schema and protocol versioning.
- Clear status for pending, synchronized, and failed operations.
- Conflict detection and safe conflict resolution.
- Recovery from interrupted transfers and partial failures.

Do not use naive last-write-wins as the general document-conflict strategy. It can silently destroy legitimate edits made on different devices.

Choose and test an appropriate document-merging approach—potentially a CRDT-based approach for rich-text documents—after a focused technical spike. The chosen approach must handle block ordering, deletion, movement, concurrent edits, and schema evolution. Cross-page operations and database changes need their own consistency rules.

**Required sync test scenarios**

- Two devices edit different parts of the same page offline.
- Two devices edit the same text simultaneously.
- One device deletes a block while another edits it.
- A page moves while another device edits its content.
- A device goes offline for an extended period.
- The app closes during a sync.
- The server becomes unavailable during attachment transfer.
- A device is revoked while it has unsynchronized changes.
- A newer app version synchronizes with an older supported version.

No release should pass if these scenarios can produce silent data loss.

**8. Sync service and deployment**

Notercrux must work without a mandatory proprietary cloud.

**Self-hosted deployment**

Provide a documented deployment that includes the required server components: API, authentication, sync coordination, database migrations, and attachment storage. Do not assume PostgreSQL alone provides synchronization.

Neon is a potential initial hosted PostgreSQL target, not the entire cloud backend. The server should use a configuration-driven database connection and avoid depending on Neon-only behavior unless explicitly isolated.

**Hosted cloud foundation**

The initial architecture should support a future hosted service with an indicative target price of approximately €3/month. This is a planning assumption, not a validated price or promise.

Include early support for:

- A backend configuration abstraction.
- Self-hosted versus hosted endpoint selection.
- Account and device management.
- Sync quotas and attachment limits.
- Subscription-entitlement abstractions.
- Server-side metering and operational observability that do not expose document contents.
- Migration and export paths if a hosted service is discontinued.

Do not build a full billing system before the application has a working sync engine and credible operating-cost estimates. Do not personally commit to running a free public sync service.

Core application features should not be paywalled. Any future subscription should primarily fund hosted infrastructure and service costs.

**9. Product feature roadmap**

The following order is dependency-driven. Each phase must produce a working, tested increment before the next one expands the scope.

**Phase 0 — Repository and technical spikes**

Foundation

- Initialize the monorepo, license, CI, formatting, linting, and tests.
- Build minimal desktop and iOS shells.
- Validate the editor engine and structured document model.
- Test desktop glass materials and native window transparency.
- Prototype the sync and E2EE architecture with two simulated devices.
- Write architecture decision records (ADRs).

**Phase 1 — Desktop shell and visual system**

- Build navigation, sidebar, page layout, command palette, menus, dialogs, and settings.
- Implement reusable glass materials and design tokens.
- Add keyboard navigation, focus management, and accessibility behavior.
- Create the initial polished README and documentation structure.

**Phase 2 — Pages and editor**

- Implement page creation, nested hierarchy, rename, move, duplicate, archive/delete, and restoration behavior.
- Implement core rich-text and block editing.
- Add Markdown shortcuts, selection behavior, slash commands, drag-and-drop, undo/redo, and keyboard shortcuts.
- Persist everything locally in SQLite.
- Add export and import of common portable formats where feasible.

**Phase 3 — Essential productivity features**

- Finish command palette, search, breadcrumbs, navigation history, and page links.
- Add page properties and the core database model.
- Add essential table, filter, sorting, and relation functionality.
- Complete settings, keyboard customization where justified, and appearance preferences.
- Test the editor against realistic long documents and large page trees.

**Phase 4 — Encryption and sync**

- Finalize key management and recovery.
- Implement encrypted payloads, device enrollment, revocation, and attachment handling.
- Build the API and PostgreSQL persistence layer.
- Implement conflict resolution, retry, reconnection, and sync observability.
- Pass multi-device data-integrity and security tests.

**Phase 5 — iOS companion**

- Build a native SwiftUI interface using the shared document and sync contracts.
- Support page browsing, reading, editing, local persistence, and offline operation.
- Add secure device enrollment and synchronization with desktop.
- Verify that desktop and iOS can independently make edits without losing data.

**Phase 6 — Import, hardening, and release**

- Implement the Notion importer after the core editor and page model are mature.
- Preserve supported pages, hierarchy, attachments, and database data.
- Report unsupported conversions explicitly.
- Finish backup/restore, migrations, crash handling, accessibility, and performance tests.
- Complete self-hosting guides, release packaging, and security review.

Phases can overlap when dependencies permit, but agents must not implement dependent features against unstable interfaces merely to appear faster.

**10. REA: targeted research into Notion**

Use the REA MCP⁠ as a research tool, not as a reason to reverse-engineer the entire application.

The goal is to answer specific questions for which actual observation is more valuable than guessing or asking you to describe familiar behavior.

**High-value investigations**

- Editor behavior: slash-menu semantics, keyboard shortcuts, block selection, dragging, paste handling, undo/redo, and nested editing.
- Page navigation: tree behavior, breadcrumbs, search interactions, navigation history, and page actions.
- Database essentials: only the property editors, relations, sorting, and filtering needed for the planned core feature set.
- Interaction details: contextual menus, command palette behavior, dialogs, focus handling, and small UX details that materially affect usability.

Do not spend time investigating AI features, agents, charts, advanced team collaboration, or other excluded features.

**Research workflow**

Each REA task should produce a compact research artifact containing:

- The precise behavior being investigated.
- Evidence from observation or inspection.
- Expected edge cases.
- Any uncertainty or platform-specific differences.
- A Notercrux implementation recommendation.
- Acceptance tests that describe the behavior to reproduce.

Use passive inspection wherever possible. For authenticated runtime research, use a separate browser profile or disposable test workspace when practical. Never authorize destructive operations on the real workspace merely to discover a feature.

The objective is to reproduce useful behavior independently—not to copy proprietary source code or indiscriminately duplicate Notion’s internal implementation.

**11. Documentation requirements**

Documentation is a permanent deliverable. Every major feature and architectural change should update the relevant Markdown documentation in the same workstream.

**Required documents**

| **Document**       | **Purpose**                                                                                                                                 |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| README.md          | A beautiful, carefully curated project introduction, screenshots, feature overview, platform support, architecture summary, and setup links |
| CONTRIBUTING.md    | Development setup, conventions, testing, PR workflow, and contribution expectations                                                         |
| SECURITY.md        | Vulnerability reporting and security policy                                                                                                 |
| docs/architecture/ | System overview, component boundaries, data model, and architectural decisions                                                              |
| docs/development/  | Build instructions, environment configuration, testing, debugging, and release workflow                                                     |
| docs/user-guides/  | Page editing, navigation, commands, settings, import/export, and everyday usage                                                             |
| docs/security/     | Threat model, encryption, device management, key storage, and recovery guide                                                                |
| docs/self-hosting/ | Server deployment, configuration, migrations, storage, backups, upgrades, and troubleshooting                                               |
| docs/decisions/    | ADRs documenting significant choices, alternatives, trade-offs, and consequences                                                            |

The README should feel like the front page of a serious open-source product. Use curated screenshots, concise explanations, consistent badges only where useful, clear installation paths, and polished visual presentation.

The documentation must be accurate to the implementation. Agents must not document unimplemented functionality as available.

**12. Engineering and coding-agent protocol**

This is especially important when multiple strong coding agents are working in parallel.

**Rules for every task**

- Inspect before changing. Read the relevant code, current architecture, tests, and repository instructions.
- Define acceptance criteria first. A task must specify observable behavior, failure cases, and required tests.
- Respect ownership boundaries. Parallel agents should not independently redesign the same shared interfaces or edit the same critical files without coordination.
- Prefer small, verifiable changes. Keep commits and pull requests focused.
- Test actual behavior. Compilation, type checking, and a visually plausible UI are necessary but insufficient.
- Update documentation. Include relevant documentation and ADR changes in the task.
- Report honestly. Distinguish implemented, tested, partially implemented, and unverified behavior.
- Never silently weaken requirements. If a feature is too complex or a test fails, report it and propose a bounded solution.
- Protect data. Migration, deletion, encryption, backup, and synchronization code requires explicit failure-path testing.
- Verify integration. A feature is not complete until it works with the actual surrounding application.

**Recommended task format**

# **Task: [Specific feature]**

# **Objective**

Describe the behavior the user should experience.

# **Context**

Relevant architecture, existing code, and dependencies.

# **Scope**

What must be implemented.

# **Explicit exclusions**

What must not be changed or added.

# **Acceptance criteria**

- Observable success cases.
- Edge cases and failure behavior.
- Accessibility and keyboard requirements where applicable.
- Performance requirements where applicable.

# **Required verification**

- Unit tests.
- Integration tests.
- UI or end-to-end tests where relevant.
- Regression tests.
- Documentation updates.

# **Deliverables**

- Code and tests.
- Documentation and ADR updates where needed.
- Commit or reviewable diff.
- Verification results and known limitations.

# **Completion report**

State exactly what changed, which tests ran, their results, and what remains unverified.

**Quality gates**

A feature cannot be marked complete simply because its code compiles.

- Editor: test selection, cursor behavior, keyboard shortcuts, undo/redo, paste, and document persistence.
- Sync: test concurrent changes, reconnection, retries, and data integrity.
- Security: test unauthorized access, key handling, device revocation, and recovery paths.
- UI: test light/dark modes, keyboard-only operation, resizing, and platform-specific glass effects.
- Performance: benchmark large documents and realistic navigation workloads.
- Documentation: verify setup steps and examples against the current code.

Automate checks in CI wherever practical. Reserve manual testing for behavior that automated tests cannot reliably validate.

**13. Performance and release criteria**

Establish measurable baselines during Phase 0, then maintain regression thresholds. Do not invent performance claims before measuring the application.

Track:

- Cold-start and warm-start times.
- Time to open an existing page.
- Input-to-render latency during editing.
- Search latency on realistic local datasets.
- Memory consumption and idle CPU usage.
- Behavior with large page trees, long documents, and many attachments.
- Sync recovery time and correctness after network interruptions.

The non-negotiable release criteria are:

- Core editing works fully offline.
- Local edits persist reliably across restarts.
- Synchronization does not silently lose changes.
- Encryption and recovery flows are documented and tested.
- Self-hosted sync can be deployed using the published instructions.
- Import operations report unsupported content rather than silently discarding it.
- Desktop and iOS share compatible document and synchronization semantics.
- The interface remains readable and responsive with glass effects enabled.
- The repository contains complete, maintained documentation.

**14. First implementation sprint**

**Initial task sequence**

- Repository audit and setup. Establish the monorepo, tooling, CI, license, agent instructions, and initial documentation.
- Visual prototype. Build a small Tauri window with the proposed glass system, command palette, sidebar, and editor surface. Validate macOS and Windows behavior.
- Document-model prototype. Implement a page tree and a minimal rich-text editor backed by SQLite, including persistence and undo/redo.
- Two-device sync experiment. Test concurrent offline edits and conflict handling using representative documents.
- E2EE feasibility prototype. Validate encryption, device enrollment, encrypted payload storage, recovery, and local search boundaries.
- Architecture review. Compare the results, resolve the high-risk design choices, and approve the interfaces before large-scale implementation.

Do not postpone the sync and encryption prototypes until after a polished editor is finished. They impose constraints on document identity, persistence, attachment handling, and recovery that are expensive to retrofit.

**Final execution principle**

Notercrux should be built as a serious application, not as a large collection of AI-generated screens. The differentiator is the combination of a polished glass-first interface, a capable page editor, local-first reliability, and user-controlled encrypted synchronization.

Keep the scope narrow enough to finish, the architecture explicit enough for agents to follow, and the quality bar high enough that the result can plausibly replace your everyday Notion workflow.
