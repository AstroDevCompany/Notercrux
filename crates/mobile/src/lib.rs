//! Swift-facing interface to the Notercrux core.
//!
//! The iOS app uses exactly the same storage, merge layer, encryption and sync engine as the desktop
//! app. This crate only adapts them for UniFFI: structured values cross the boundary as the **same
//! JSON** the desktop UI uses (pages, documents, status), so there is one contract to keep stable
//! and the Swift side decodes it with `Codable`.

use notercrux_domain::{Document, PageId};
use notercrux_storage::Store;
use notercrux_sync::{Engine, EngineConfig, JoinMode, SecretStore};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CoreError {
    #[error("{0}")]
    Message(String),
}

fn err<E: std::fmt::Display>(e: E) -> CoreError {
    CoreError::Message(e.to_string())
}

type CoreResult<T> = Result<T, CoreError>;

fn json<T: Serialize>(v: &T) -> CoreResult<String> {
    serde_json::to_string(v).map_err(err)
}

fn page_id(s: &str) -> CoreResult<PageId> {
    s.parse()
        .map_err(|_| CoreError::Message(format!("invalid page id: {s}")))
}

/// Where the app keeps device keys. Implemented in Swift on the Keychain.
#[uniffi::export(with_foreign)]
pub trait SecretVault: Send + Sync {
    fn get(&self, name: String) -> Option<Vec<u8>>;
    fn set(&self, name: String, value: Vec<u8>) -> bool;
    fn delete(&self, name: String) -> bool;
}

struct VaultAdapter(Arc<dyn SecretVault>);

impl SecretStore for VaultAdapter {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.get(name.to_string()))
    }
    fn set(&self, name: &str, value: &[u8]) -> Result<(), String> {
        if self.0.set(name.to_string(), value.to_vec()) {
            Ok(())
        } else {
            Err("the Keychain refused to store the key".into())
        }
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        if self.0.delete(name.to_string()) {
            Ok(())
        } else {
            Err("the Keychain refused to delete the key".into())
        }
    }
}

/// Told when sync state changes or another device's changes were applied.
#[uniffi::export(with_foreign)]
pub trait CoreListener: Send + Sync {
    /// `SyncStatus` as JSON.
    fn status_changed(&self, status_json: String);
    /// `ApplyReport` as JSON (`pagesChanged`, `documentsChanged`, `recoveredBlocks`).
    fn data_changed(&self, report_json: String);
}

#[derive(uniffi::Object)]
pub struct Core {
    store: Arc<Mutex<Store>>,
    engine: Arc<Engine>,
    rt: std::mem::ManuallyDrop<tokio::runtime::Runtime>,
    stop: Mutex<Option<watch::Sender<bool>>>,
}

impl Drop for Core {
    fn drop(&mut self) {
        self.stop_sync();
        // SAFETY: the runtime is never used after this. `shutdown_background` does not block, so
        // dropping a Core is allowed from any thread, including inside another runtime.
        unsafe { std::mem::ManuallyDrop::take(&mut self.rt) }.shutdown_background();
    }
}

impl Core {
    fn with_store<T>(
        &self,
        f: impl FnOnce(&mut Store) -> notercrux_storage::Result<T>,
    ) -> CoreResult<T> {
        let mut s = self
            .store
            .lock()
            .map_err(|_| CoreError::Message("storage lock poisoned".into()))?;
        f(&mut s).map_err(err)
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl Core {
    /// Opens (or creates) the database in `data_dir`.
    #[uniffi::constructor]
    pub fn new(data_dir: String, vault: Arc<dyn SecretVault>) -> CoreResult<Arc<Self>> {
        let dir = PathBuf::from(data_dir);
        std::fs::create_dir_all(&dir).map_err(err)?;
        let store = Arc::new(Mutex::new(
            Store::open(dir.join("notercrux.db")).map_err(err)?,
        ));
        let engine = Engine::open(
            store.clone(),
            Arc::new(VaultAdapter(vault)),
            EngineConfig::default(),
        )
        .map_err(err)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("notercrux-core")
            .build()
            .map_err(err)?;
        Ok(Arc::new(Self {
            store,
            engine,
            rt: std::mem::ManuallyDrop::new(rt),
            stop: Mutex::new(None),
        }))
    }

    // ---- pages and documents (JSON, same shapes as the desktop app) ---------------------------

    pub fn list_pages_json(&self) -> CoreResult<String> {
        json(&self.with_store(|s| s.list_pages())?)
    }

    pub fn create_page_json(&self, parent_id: Option<String>, title: String) -> CoreResult<String> {
        let parent = parent_id.as_deref().map(page_id).transpose()?;
        json(&self.with_store(|s| s.create_page(parent, &title))?)
    }

    pub fn rename_page(&self, id: String, title: String) -> CoreResult<()> {
        let id = page_id(&id)?;
        self.with_store(|s| s.rename_page(id, &title))
    }

    pub fn archive_page(&self, id: String) -> CoreResult<()> {
        let id = page_id(&id)?;
        self.with_store(|s| s.archive_page(id))
    }

    pub fn restore_page(&self, id: String) -> CoreResult<()> {
        let id = page_id(&id)?;
        self.with_store(|s| s.restore_page(id))
    }

    pub fn delete_page(&self, id: String) -> CoreResult<()> {
        let id = page_id(&id)?;
        self.with_store(|s| s.delete_page_permanently(id))
    }

    pub fn search_json(&self, query: String) -> CoreResult<String> {
        json(&self.with_store(|s| s.search_with_snippets(&query, 30))?)
    }

    pub fn load_document_json(&self, id: String) -> CoreResult<String> {
        let id = page_id(&id)?;
        json(&self.with_store(|s| s.load_document(id))?)
    }

    /// Saves the editor's changes on top of the page as it is now and returns
    /// `{ "document": ..., "conflicts": n }`.
    pub fn save_document_rebased_json(
        &self,
        id: String,
        base_json: String,
        doc_json: String,
    ) -> CoreResult<String> {
        let id = page_id(&id)?;
        let base: Document = serde_json::from_str(&base_json).map_err(err)?;
        let doc: Document = serde_json::from_str(&doc_json).map_err(err)?;
        json(&self.with_store(|s| s.save_document_rebased(id, &base, &doc))?)
    }

    pub fn attachment_data(&self, id: String) -> CoreResult<Option<Vec<u8>>> {
        self.with_store(|s| s.attachment_data(&id))
    }

    pub fn attachment_json(&self, id: String) -> CoreResult<Option<String>> {
        match self.with_store(|s| s.attachment(&id))? {
            Some(m) => Ok(Some(json(&m)?)),
            None => Ok(None),
        }
    }

    // ---- sync ---------------------------------------------------------------------------------

    /// Starts the background sync loop and reports changes to `listener`. Safe to call again.
    pub fn start_sync(&self, listener: Arc<dyn CoreListener>) {
        let mut stop = self.stop.lock().unwrap_or_else(|e| e.into_inner());
        if stop.is_some() {
            return;
        }
        let (tx, rx) = watch::channel(false);
        *stop = Some(tx);
        self.rt.spawn(self.engine.clone().run(rx));
        let (l, mut status) = (listener.clone(), self.engine.subscribe());
        self.rt.spawn(async move {
            if let Ok(j) = serde_json::to_string(&*status.borrow()) {
                l.status_changed(j);
            }
            while status.changed().await.is_ok() {
                if let Ok(j) = serde_json::to_string(&*status.borrow()) {
                    l.status_changed(j);
                }
            }
        });
        let mut changes = self.engine.subscribe_changes();
        self.rt.spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(r) => {
                        if let Ok(j) = serde_json::to_string(&r) {
                            listener.data_changed(j);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => listener.data_changed(
                        r#"{"applied":0,"held":0,"recoveredBlocks":0,"pagesChanged":true,"documentsChanged":[]}"#.into(),
                    ),
                    Err(_) => break,
                }
            }
        });
    }

    pub fn stop_sync(&self) {
        if let Some(tx) = self.stop.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(true);
        }
    }

    pub fn sync_status_json(&self) -> CoreResult<String> {
        json(&self.engine.status())
    }

    pub fn sync_events_json(&self) -> CoreResult<String> {
        json(&self.engine.events())
    }

    /// Syncs soon (also wakes a waiting long poll so local edits go out promptly).
    pub fn sync_soon(&self) {
        self.engine.sync_soon();
    }

    pub fn join_in_progress(&self) -> bool {
        self.engine.join_in_progress()
    }

    pub async fn begin_join(&self, code: String, label: String) -> CoreResult<String> {
        self.engine.begin_join(&code, &label).await.map_err(err)
    }

    /// `mode` is `keepLocal` or `replaceLocal`. Returns `waiting` or `joined`.
    pub async fn poll_join(&self, mode: String) -> CoreResult<String> {
        let mode: JoinMode = serde_json::from_str(&format!("\"{mode}\"")).map_err(err)?;
        let p = self.engine.poll_join(mode).await.map_err(err)?;
        if p == notercrux_sync::JoinProgress::Joined {
            self.engine.sync_soon();
        }
        Ok(match p {
            notercrux_sync::JoinProgress::Waiting => "waiting",
            notercrux_sync::JoinProgress::Joined => "joined",
        }
        .into())
    }

    pub fn cancel_join(&self) -> CoreResult<()> {
        self.engine.cancel_join().map_err(err)
    }

    pub async fn recover(
        &self,
        server: String,
        workspace_id: String,
        phrase: String,
        label: String,
        mode: String,
    ) -> CoreResult<()> {
        let mode: JoinMode = serde_json::from_str(&format!("\"{mode}\"")).map_err(err)?;
        self.engine
            .recover(&server, &workspace_id, &phrase, &label, mode)
            .await
            .map_err(err)?;
        self.engine.sync_soon();
        Ok(())
    }

    pub async fn create_workspace(
        &self,
        server: String,
        token: Option<String>,
        label: String,
    ) -> CoreResult<String> {
        let c = self
            .engine
            .create_workspace(&server, token, &label)
            .await
            .map_err(err)?;
        self.engine.sync_soon();
        json(&c)
    }

    pub async fn create_invite(&self) -> CoreResult<String> {
        json(&self.engine.create_invite().await.map_err(err)?)
    }

    pub async fn pending_join_json(&self, invite_id: String) -> CoreResult<String> {
        json(&self.engine.pending_join(&invite_id).await.map_err(err)?)
    }

    pub async fn approve_join(&self, invite_id: String) -> CoreResult<()> {
        self.engine.approve_join(&invite_id).await.map_err(err)
    }

    pub async fn devices_json(&self) -> CoreResult<String> {
        json(&self.engine.devices().await.map_err(err)?)
    }

    pub async fn revoke_device(&self, id: String) -> CoreResult<()> {
        self.engine.revoke_device(&id).await.map_err(err)
    }

    pub async fn leave(&self) -> CoreResult<()> {
        self.engine.leave().await.map_err(err)
    }

    pub async fn leave_local(&self) -> CoreResult<()> {
        self.engine.leave_local().await.map_err(err)
    }
}
