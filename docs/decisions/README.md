# Architecture decision records

Each ADR records one significant choice: the context, the options considered, the decision, and
its consequences. Status is one of _proposed_, _accepted_, _superseded_.

| ADR                                                    | Title                                            | Status              |
| ------------------------------------------------------ | ------------------------------------------------ | ------------------- |
| [0001](0001-desktop-stack.md)                          | Tauri 2 + React desktop shell                    | Accepted            |
| [0002](0002-document-model-and-editor.md)              | Structured block documents and Tiptap            | Accepted            |
| [0003](0003-crdt-sync.md)                              | Loro CRDT documents over an opaque relay         | Accepted (see 0009) |
| [0004](0004-e2ee-key-hierarchy.md)                     | E2EE key hierarchy, recovery and enrollment      | Accepted (see 0008) |
| [0005](0005-glass-material-system.md)                  | Glass material system                            | Accepted            |
| [0006](0006-databases-and-links.md)                    | Databases, properties and page links             | Accepted            |
| [0007](0007-sync-protocol-and-server.md)               | Sync protocol, server and PostgreSQL             | Accepted            |
| [0008](0008-key-management-enrollment-and-recovery.md) | Key management, enrollment, revocation, recovery | Accepted            |
| [0009](0009-merge-layer-and-recovered-edits.md)        | CRDT merge layer, recovered edits, rebased saves | Accepted            |
| [0010](0010-ios-app.md)                                | iOS app on the shared Rust core                  | Accepted            |

"Accepted" means implemented and tested, not externally reviewed. Nothing in the security design
has had an independent review.
