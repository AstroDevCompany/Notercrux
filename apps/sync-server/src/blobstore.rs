//! Storage for encrypted attachment chunks. The server only ever holds ciphertext.

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put(&self, ws: &str, blob: &str, index: u32, data: &[u8]) -> std::io::Result<()>;
    async fn get(&self, ws: &str, blob: &str, index: u32) -> std::io::Result<Option<Vec<u8>>>;
    async fn has(&self, ws: &str, blob: &str, index: u32) -> std::io::Result<bool>;
    async fn delete_workspace(&self, ws: &str) -> std::io::Result<()>;
}

#[derive(Default)]
pub struct MemBlobStore {
    chunks: Mutex<HashMap<(String, String, u32), Vec<u8>>>,
}

impl MemBlobStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test helper: every stored byte string.
    pub fn all(&self) -> Vec<Vec<u8>> {
        self.chunks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl BlobStore for MemBlobStore {
    async fn put(&self, ws: &str, blob: &str, index: u32, data: &[u8]) -> std::io::Result<()> {
        self.chunks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((ws.into(), blob.into(), index), data.to_vec());
        Ok(())
    }

    async fn get(&self, ws: &str, blob: &str, index: u32) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self
            .chunks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(ws.to_string(), blob.to_string(), index))
            .cloned())
    }

    async fn has(&self, ws: &str, blob: &str, index: u32) -> std::io::Result<bool> {
        Ok(self
            .chunks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&(ws.to_string(), blob.to_string(), index)))
    }

    async fn delete_workspace(&self, ws: &str) -> std::io::Result<()> {
        self.chunks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(w, _, _), _| w != ws);
        Ok(())
    }
}

/// One file per chunk under `<dir>/<workspace>/<blob>/<index>`. Ids are validated by the API layer
/// (`[A-Za-z0-9_-]`), so they cannot escape the directory.
pub struct FsBlobStore {
    dir: PathBuf,
}

impl FsBlobStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, ws: &str, blob: &str, index: u32) -> PathBuf {
        self.dir.join(ws).join(blob).join(format!("{index}.chunk"))
    }
}

#[async_trait]
impl BlobStore for FsBlobStore {
    async fn put(&self, ws: &str, blob: &str, index: u32, data: &[u8]) -> std::io::Result<()> {
        let path = self.path(ws, blob, index);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Write then rename so a crash never leaves a half-written chunk that looks complete.
        let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
        tokio::fs::write(&tmp, data).await?;
        tokio::fs::rename(&tmp, &path).await
    }

    async fn get(&self, ws: &str, blob: &str, index: u32) -> std::io::Result<Option<Vec<u8>>> {
        match tokio::fs::read(self.path(ws, blob, index)).await {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn has(&self, ws: &str, blob: &str, index: u32) -> std::io::Result<bool> {
        Ok(tokio::fs::try_exists(self.path(ws, blob, index)).await?)
    }

    async fn delete_workspace(&self, ws: &str) -> std::io::Result<()> {
        match tokio::fs::remove_dir_all(self.dir.join(ws)).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}
