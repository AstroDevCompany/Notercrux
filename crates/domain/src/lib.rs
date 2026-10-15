//! Shared document model for Notercrux.
//!
//! The model is deliberately free of storage, UI, and network concerns so that
//! the desktop app, the sync layer, and a future iOS client apply identical rules.

pub mod block;
pub mod database;
pub mod ids;
pub mod page;
pub mod tree;

pub use block::{Block, BlockKind, Document, Mark, DOCUMENT_SCHEMA_VERSION};
pub use database::{
    validate_value, DatabaseSchema, Filter, FilterMode, FilterOp, Property, PropertyKind,
    SchemaError, SelectOption, Sort, SortDirection, View, TITLE_PROPERTY,
};
pub use ids::{BlockId, PageId};
pub use page::{Page, PageKind};
pub use tree::{PageTree, TreeError};
