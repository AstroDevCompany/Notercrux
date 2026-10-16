//! Sync client engine.
//!
//! The engine connects a local [`notercrux_storage::Store`] to a Notercrux sync server. It encrypts
//! queued local operations before upload, decrypts and applies remote ones, and runs the account
//! lifecycle: creating a workspace, enrolling devices, revoking them, and recovery. The server never
//! sees plaintext or keys.

mod account;
mod blobs;
mod engine;
mod enroll;
mod http;
pub use http::install_tls_provider;
mod secrets;
mod status;

pub use account::{Account, InviteCode, DEVICE_SECRET, RING_SECRET};
pub use blobs::AttachmentReport;
pub use engine::{CycleReport, Engine, EngineConfig};
pub use enroll::{CreatedWorkspace, DeviceView, InviteOffer, JoinMode, JoinProgress, JoinRequest};
pub use secrets::{FileSecrets, MemorySecrets, SecretStore};
pub use status::{Phase, SyncEvent, SyncStatus};

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SyncError {
    #[error("this workspace is not connected to a sync server")]
    NotLinked,
    #[error("this workspace is already connected to a sync server")]
    AlreadyLinked,
    #[error("{0}")]
    Network(String),
    #[error("{message}")]
    Server {
        status: u16,
        code: String,
        message: String,
    },
    #[error("this device was removed from the workspace")]
    Revoked,
    #[error("encryption keys unavailable: {0}")]
    Keys(String),
    #[error("that code is not valid")]
    BadCode,
    #[error("{0}")]
    Crypto(String),
    #[error("storage: {0}")]
    Storage(String),
    #[error("secure storage: {0}")]
    Secrets(String),
    #[error("{0}")]
    Protocol(String),
}

impl SyncError {
    /// Worth retrying with backoff (the network or the server is struggling), as opposed to a
    /// rejection that will repeat until something changes.
    pub fn is_transient(&self) -> bool {
        match self {
            SyncError::Network(_) => true,
            SyncError::Server { status, .. } => *status >= 500 || *status == 429 || *status == 408,
            _ => false,
        }
    }

    pub fn code(&self) -> &str {
        match self {
            SyncError::Server { code, .. } => code,
            SyncError::Revoked => notercrux_protocol::code::REVOKED,
            _ => "",
        }
    }
}

impl From<notercrux_storage::StoreError> for SyncError {
    fn from(e: notercrux_storage::StoreError) -> Self {
        SyncError::Storage(e.to_string())
    }
}

impl From<notercrux_crypto::CryptoError> for SyncError {
    fn from(e: notercrux_crypto::CryptoError) -> Self {
        SyncError::Crypto(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SyncError>;
