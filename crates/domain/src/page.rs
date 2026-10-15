use crate::ids::PageId;
use serde::{Deserialize, Serialize};

/// Page metadata. Page content lives in a [`crate::Document`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: PageId,
    pub parent_id: Option<PageId>,
    pub title: String,
    pub icon: Option<String>,
    /// Zero-based position among siblings.
    pub position: u32,
    /// Unix milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
    /// Set when the page sits in the archive; `None` for live pages.
    pub archived_at: Option<i64>,
}
