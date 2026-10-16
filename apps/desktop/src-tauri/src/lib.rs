mod commands;
mod secure;
mod sync_commands;

use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

/// Keeps the shutdown sender alive for the life of the app; dropping it stops the sync loop.
struct StopSync(#[allow(dead_code)] tokio::sync::watch::Sender<bool>);

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let mut store = notercrux_storage::Store::open(dir.join("notercrux.db"))?;
            commands::seed_if_empty(&mut store)?;
            let store = Arc::new(Mutex::new(store));
            app.manage(commands::AppState(store.clone()));

            // Sync: the engine runs for the life of the app and tells the UI what it is doing.
            let secrets = Arc::new(secure::AutoSecrets::new(dir.join("secrets")));
            let engine = notercrux_sync::Engine::open(
                store,
                secrets.clone(),
                notercrux_sync::EngineConfig::default(),
            )?;
            let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
            app.manage(sync_commands::SyncHandle {
                engine: engine.clone(),
                secrets,
            });
            app.manage(StopSync(stop_tx));
            tauri::async_runtime::spawn(engine.clone().run(stop_rx));
            let (handle, mut status) = (app.handle().clone(), engine.subscribe());
            tauri::async_runtime::spawn(async move {
                let _ = handle.emit("sync-status", status.borrow().clone());
                while status.changed().await.is_ok() {
                    let _ = handle.emit("sync-status", status.borrow().clone());
                }
            });
            let (handle, mut changes) = (app.handle().clone(), engine.subscribe_changes());
            tauri::async_runtime::spawn(async move {
                loop {
                    match changes.recv().await {
                        Ok(report) => {
                            let _ = handle.emit("sync-changes", report);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            // Missed some reports: tell the UI to reload everything.
                            let _ = handle.emit("sync-changes", serde_json::json!({ "pagesChanged": true, "documentsChanged": [], "recoveredBlocks": 0 }));
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_pages,
            commands::create_page,
            commands::rename_page,
            commands::set_page_icon,
            commands::move_page,
            commands::duplicate_page,
            commands::archive_page,
            commands::restore_page,
            commands::delete_page,
            commands::load_document,
            commands::save_document,
            commands::save_document_rebased,
            commands::search_pages,
            commands::backlinks,
            commands::create_database,
            commands::get_schema,
            commands::set_schema,
            commands::list_records,
            commands::set_property,
            commands::get_properties,
            commands::export_file,
            commands::import_file,
            sync_commands::sync_overview,
            sync_commands::sync_events,
            sync_commands::sync_create_workspace,
            sync_commands::sync_create_invite,
            sync_commands::sync_pending_join,
            sync_commands::sync_approve_join,
            sync_commands::sync_begin_join,
            sync_commands::sync_poll_join,
            sync_commands::sync_cancel_join,
            sync_commands::sync_devices,
            sync_commands::sync_info,
            sync_commands::sync_revoke_device,
            sync_commands::sync_recover,
            sync_commands::sync_leave,
            sync_commands::sync_leave_local,
            sync_commands::sync_delete_remote,
            sync_commands::sync_now,
            sync_commands::sync_retry_attachments,
            sync_commands::add_attachment,
            sync_commands::attachment_info,
            sync_commands::read_attachment,
            sync_commands::save_attachment_as,
            sync_commands::attachment_chunk_size,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Notercrux");
}
