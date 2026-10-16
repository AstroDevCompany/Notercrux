//! Tauri commands for sync, enrollment, recovery and attachments. Thin: the rules live in
//! `notercrux-sync` and `notercrux-storage`.

use crate::commands::AppState;
use crate::secure::AutoSecrets;
use notercrux_protocol::WorkspaceInfo;
use notercrux_storage::{AttachmentMeta, ATTACHMENT_CHUNK};
use notercrux_sync::{
    CreatedWorkspace, DeviceView, Engine, InviteOffer, JoinMode, JoinProgress, JoinRequest,
    SyncEvent, SyncStatus,
};
use std::sync::Arc;
use tauri::{ipc::Response, State};

pub struct SyncHandle {
    pub engine: Arc<Engine>,
    pub secrets: Arc<AutoSecrets>,
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOverview {
    pub status: SyncStatus,
    /// `os`, `file`, `none`, or null while no secret has been stored or read yet.
    pub secure_storage: Option<&'static str>,
    pub join_in_progress: bool,
}

#[tauri::command]
pub fn sync_overview(h: State<'_, SyncHandle>) -> SyncOverview {
    SyncOverview {
        status: h.engine.status(),
        secure_storage: h.secrets.kind(),
        join_in_progress: h.engine.join_in_progress(),
    }
}

#[tauri::command]
pub fn sync_events(h: State<'_, SyncHandle>) -> Vec<SyncEvent> {
    h.engine.events()
}

#[tauri::command]
pub async fn sync_create_workspace(
    h: State<'_, SyncHandle>,
    server: String,
    token: Option<String>,
    label: String,
) -> CmdResult<CreatedWorkspace> {
    let r = h
        .engine
        .create_workspace(&server, token, &label)
        .await
        .map_err(err)?;
    h.engine.sync_soon();
    Ok(r)
}

#[tauri::command]
pub async fn sync_create_invite(h: State<'_, SyncHandle>) -> CmdResult<InviteOffer> {
    h.engine.create_invite().await.map_err(err)
}

#[tauri::command]
pub async fn sync_pending_join(
    h: State<'_, SyncHandle>,
    invite_id: String,
) -> CmdResult<Option<JoinRequest>> {
    h.engine.pending_join(&invite_id).await.map_err(err)
}

#[tauri::command]
pub async fn sync_approve_join(h: State<'_, SyncHandle>, invite_id: String) -> CmdResult<()> {
    h.engine.approve_join(&invite_id).await.map_err(err)
}

#[tauri::command]
pub async fn sync_begin_join(
    h: State<'_, SyncHandle>,
    code: String,
    label: String,
) -> CmdResult<String> {
    h.engine.begin_join(&code, &label).await.map_err(err)
}

#[tauri::command]
pub async fn sync_poll_join(h: State<'_, SyncHandle>, mode: JoinMode) -> CmdResult<JoinProgress> {
    let p = h.engine.poll_join(mode).await.map_err(err)?;
    if p == JoinProgress::Joined {
        h.engine.sync_soon();
    }
    Ok(p)
}

#[tauri::command]
pub fn sync_cancel_join(h: State<'_, SyncHandle>) -> CmdResult<()> {
    h.engine.cancel_join().map_err(err)
}

#[tauri::command]
pub async fn sync_devices(h: State<'_, SyncHandle>) -> CmdResult<Vec<DeviceView>> {
    h.engine.devices().await.map_err(err)
}

#[tauri::command]
pub async fn sync_info(h: State<'_, SyncHandle>) -> CmdResult<WorkspaceInfo> {
    h.engine.server_info().await.map_err(err)
}

#[tauri::command]
pub async fn sync_revoke_device(h: State<'_, SyncHandle>, device_id: String) -> CmdResult<()> {
    h.engine.revoke_device(&device_id).await.map_err(err)
}

#[tauri::command]
pub async fn sync_recover(
    h: State<'_, SyncHandle>,
    server: String,
    workspace_id: String,
    phrase: String,
    label: String,
    mode: JoinMode,
) -> CmdResult<()> {
    h.engine
        .recover(&server, &workspace_id, &phrase, &label, mode)
        .await
        .map_err(err)?;
    h.engine.sync_soon();
    Ok(())
}

#[tauri::command]
pub async fn sync_leave(h: State<'_, SyncHandle>) -> CmdResult<()> {
    h.engine.leave().await.map_err(err)
}

/// Forget the link on this device only (also the way out after being removed from the workspace).
#[tauri::command]
pub async fn sync_leave_local(h: State<'_, SyncHandle>) -> CmdResult<()> {
    h.engine.leave_local().await.map_err(err)
}

#[tauri::command]
pub async fn sync_delete_remote(h: State<'_, SyncHandle>) -> CmdResult<()> {
    h.engine.delete_remote_workspace().await.map_err(err)
}

#[tauri::command]
pub fn sync_now(h: State<'_, SyncHandle>) {
    h.engine.sync_soon();
}

#[tauri::command]
pub fn sync_retry_attachments(h: State<'_, SyncHandle>) -> CmdResult<usize> {
    h.engine.retry_attachments().map_err(err)
}

// ---- attachments ------------------------------------------------------------------------------

/// The file's bytes are the raw request body (no JSON round trip); `name` and `mime` come as
/// percent-encoded headers.
#[tauri::command]
pub fn add_attachment(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> CmdResult<AttachmentMeta> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the file as raw bytes".into());
    };
    let header = |k: &str| {
        request
            .headers()
            .get(k)
            .and_then(|v| v.to_str().ok())
            .map(percent_decode)
            .unwrap_or_default()
    };
    let (name, mime) = (header("x-name"), header("x-mime"));
    state
        .0
        .lock()
        .map_err(|_| "storage lock poisoned".to_string())?
        .attachment_add(&name, &mime, bytes)
        .map_err(err)
}

#[tauri::command]
pub fn attachment_info(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<Option<AttachmentMeta>> {
    state
        .0
        .lock()
        .map_err(|_| "storage lock poisoned".to_string())?
        .attachment(&id)
        .map_err(err)
}

/// The file's bytes, or an error while it has not fully arrived.
#[tauri::command]
pub fn read_attachment(state: State<'_, AppState>, id: String) -> CmdResult<Response> {
    let data = state
        .0
        .lock()
        .map_err(|_| "storage lock poisoned".to_string())?
        .attachment_data(&id)
        .map_err(err)?;
    data.map(Response::new)
        .ok_or_else(|| "this file has not been downloaded yet".to_string())
}

/// Asks where to save, then writes the file there. Returns `false` if cancelled.
#[tauri::command]
pub async fn save_attachment_as(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let (meta, data) = {
        let store = state
            .0
            .lock()
            .map_err(|_| "storage lock poisoned".to_string())?;
        (
            store.attachment(&id).map_err(err)?,
            store.attachment_data(&id).map_err(err)?,
        )
    };
    let (Some(meta), Some(data)) = (meta, data) else {
        return Err("this file has not been downloaded yet".into());
    };
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_file_name(&meta.name)
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(path) = rx.recv().map_err(err)? else {
        return Ok(false);
    };
    std::fs::write(path.into_path().map_err(err)?, data).map_err(err)?;
    Ok(true)
}

/// Plaintext chunk size, so the UI can show progress in whole chunks if it wants to.
#[tauri::command]
pub fn attachment_chunk_size() -> usize {
    ATTACHMENT_CHUNK
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_percent_encoded_headers() {
        assert_eq!(
            percent_decode("My%20photo%20%E2%9C%93.png"),
            "My photo ✓.png"
        );
        assert_eq!(percent_decode("plain.txt"), "plain.txt");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
