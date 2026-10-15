# Architecture decision records

Each ADR records one significant choice: the context, the options considered, the decision, and
its consequences. Status is one of _proposed_, _accepted_, _superseded_.

| ADR                                       | Title                                       | Status   |
| ----------------------------------------- | ------------------------------------------- | -------- |
| [0001](0001-desktop-stack.md)             | Tauri 2 + React desktop shell               | Accepted |
| [0002](0002-document-model-and-editor.md) | Structured block documents and Tiptap       | Accepted |
| [0003](0003-crdt-sync.md)                 | Loro CRDT documents over an opaque relay    | Proposed |
| [0004](0004-e2ee-key-hierarchy.md)        | E2EE key hierarchy, recovery and enrollment | Proposed |
| [0005](0005-glass-material-system.md)     | Glass material system                       | Accepted |

"Proposed" means a spike supports the direction but the architecture review (blueprint §14,
step 6) has not signed off.
