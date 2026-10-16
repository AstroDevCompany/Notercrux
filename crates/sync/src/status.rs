//! What the UI shows: current state plus a short activity log.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Not connected to a server.
    #[default]
    Disabled,
    Idle,
    Syncing,
    /// The server cannot be reached; retrying with backoff.
    Offline,
    /// The server rejected something that retrying will not fix.
    Error,
    /// This device was removed from the workspace. Local data is kept.
    Revoked,
    /// The account exists but its keys are missing from secure storage.
    KeysMissing,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub phase: Phase,
    pub server: Option<String>,
    pub workspace_id: Option<String>,
    pub device_id: Option<String>,
    pub epoch: u32,
    /// Local operations not yet confirmed by the server.
    pub pending_ops: i64,
    /// Operations from a newer app version, kept until this app is updated.
    pub held_ops: i64,
    pub last_sync_at: Option<i64>,
    pub last_error: Option<String>,
    pub retry_in_secs: Option<u64>,
    pub ops_sent: u64,
    pub ops_received: u64,
    /// Remote operations that failed verification and were skipped.
    pub rejected_ops: i64,
    /// Blocks restored from a delete-versus-edit conflict since the app started.
    pub recovered_blocks: u64,
    pub attachments_pending: i64,
    /// Attachments that could not be synced and wait for a manual retry.
    pub attachments_failed: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEvent {
    pub at: i64,
    /// `info`, `warn` or `error`.
    pub level: &'static str,
    pub message: String,
}
