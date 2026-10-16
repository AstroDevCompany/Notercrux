//! CRDT documents for Notercrux.
//!
//! The relational SQLite store stays the queryable, canonical *local* view. These documents are the
//! merge layer between devices:
//!
//! * [`PageCrdt`]: one per page. A Loro tree of blocks (stable ids, movable, nestable) whose text is
//!   a Loro rich-text container, so concurrent typing merges per character and formatting follows the
//!   text.
//! * [`WorkspaceCrdt`]: one per workspace. The page tree and its metadata, database schemas and record
//!   property values.
//! * [`merge`]: applying remote updates without silently losing local edits.
//!
//! Design notes are in `docs/decisions/0007-crdt-merge-layer.md`.

mod merge;
mod page;
mod workspace;

pub use merge::{merge_page_updates, MergeOutcome, RECOVERED_ICON};
pub use page::PageCrdt;
pub use workspace::{repair_schema, WorkspaceCrdt};

use loro::{ExportMode, LoroDoc, VersionVector};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CrdtError {
    #[error("crdt error: {0}")]
    Loro(String),
    #[error("malformed document state: {0}")]
    Malformed(String),
}

impl From<loro::LoroError> for CrdtError {
    fn from(e: loro::LoroError) -> Self {
        CrdtError::Loro(e.to_string())
    }
}
impl From<loro::LoroEncodeError> for CrdtError {
    fn from(e: loro::LoroEncodeError) -> Self {
        CrdtError::Loro(e.to_string())
    }
}
impl From<serde_json::Error> for CrdtError {
    fn from(e: serde_json::Error) -> Self {
        CrdtError::Malformed(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CrdtError>;

/// Opaque version of a document: which operations a replica has seen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Version(pub(crate) VersionVector);

impl Version {
    pub fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Ok(Self(VersionVector::decode(bytes)?))
    }
}

/// Shared behaviour of both document kinds.
pub(crate) fn snapshot(doc: &LoroDoc) -> Result<Vec<u8>> {
    Ok(doc.export(ExportMode::Snapshot)?)
}

pub(crate) fn updates_since(doc: &LoroDoc, since: &Version) -> Result<Vec<u8>> {
    Ok(doc.export(ExportMode::updates(&since.0))?)
}

pub(crate) fn version(doc: &LoroDoc) -> Version {
    Version(doc.oplog_vv())
}

pub(crate) fn import(doc: &LoroDoc, bytes: &[u8]) -> Result<()> {
    doc.import(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_workspace;
