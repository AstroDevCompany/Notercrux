//! Shared document model for Notercrux.
//!
//! The model is deliberately free of storage, UI, and network concerns so that
//! the desktop app, the sync layer, and a future iOS client apply identical rules.

pub mod block;
pub mod ids;
pub mod page;
pub mod tree;

pub use block::{Block, BlockKind, Document, Mark, DOCUMENT_SCHEMA_VERSION};
pub use ids::{BlockId, PageId};
pub use page::Page;
pub use tree::{PageTree, TreeError};
