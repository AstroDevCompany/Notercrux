//! Thin Tauri command layer. All rules live in `notercrux-domain` / `notercrux-storage`.

use notercrux_domain::{
    Block, BlockKind, DatabaseSchema, Document, Page, PageId, DOCUMENT_SCHEMA_VERSION,
};
use notercrux_storage::{Record, SearchHit, Store};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::State;

pub struct AppState(pub Arc<Mutex<Store>>);

type CmdResult<T> = Result<T, String>;

fn store<'a>(state: &'a State<'a, AppState>) -> CmdResult<MutexGuard<'a, Store>> {
    state
        .0
        .lock()
        .map_err(|_| "storage lock poisoned".to_string())
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn id(s: &str) -> CmdResult<PageId> {
    s.parse().map_err(|_| format!("invalid page id: {s}"))
}

/// First-run content so the app opens onto something useful.
pub fn seed_if_empty(store: &mut Store) -> Result<(), Box<dyn std::error::Error>> {
    if !store.list_pages()?.is_empty() {
        return Ok(());
    }
    let welcome = store.create_page(None, "Welcome to Notercrux")?;
    store.set_icon(welcome.id, Some("👋"))?;
    store.save_document(
        welcome.id,
        &Document {
            version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![
                Block::heading(1, "Welcome to Notercrux"),
                Block::new(
                    BlockKind::Paragraph,
                    "Press Cmd/Ctrl+K to open the command palette. Everything here is stored locally.",
                ),
                Block::new(BlockKind::Todo, "Try typing in this page"),
            ],
        },
    )?;
    store.create_page(Some(welcome.id), "A nested page")?;
    Ok(())
}

#[tauri::command]
pub fn list_pages(state: State<'_, AppState>) -> CmdResult<Vec<Page>> {
    store(&state)?.list_pages().map_err(err)
}

#[tauri::command]
pub fn create_page(
    state: State<'_, AppState>,
    parent_id: Option<String>,
    title: String,
) -> CmdResult<Page> {
    let parent = parent_id.as_deref().map(id).transpose()?;
    store(&state)?.create_page(parent, &title).map_err(err)
}

#[tauri::command]
pub fn rename_page(state: State<'_, AppState>, page_id: String, title: String) -> CmdResult<()> {
    store(&state)?
        .rename_page(id(&page_id)?, &title)
        .map_err(err)
}

#[tauri::command]
pub fn set_page_icon(
    state: State<'_, AppState>,
    page_id: String,
    icon: Option<String>,
) -> CmdResult<()> {
    store(&state)?
        .set_icon(id(&page_id)?, icon.as_deref())
        .map_err(err)
}

#[tauri::command]
pub fn move_page(
    state: State<'_, AppState>,
    page_id: String,
    new_parent_id: Option<String>,
    index: u32,
) -> CmdResult<()> {
    let parent = new_parent_id.as_deref().map(id).transpose()?;
    store(&state)?
        .move_page(id(&page_id)?, parent, index)
        .map_err(err)
}

#[tauri::command]
pub fn duplicate_page(state: State<'_, AppState>, page_id: String) -> CmdResult<Page> {
    store(&state)?.duplicate_page(id(&page_id)?).map_err(err)
}

#[tauri::command]
pub fn archive_page(state: State<'_, AppState>, page_id: String) -> CmdResult<()> {
    store(&state)?.archive_page(id(&page_id)?).map_err(err)
}

#[tauri::command]
pub fn restore_page(state: State<'_, AppState>, page_id: String) -> CmdResult<()> {
    store(&state)?.restore_page(id(&page_id)?).map_err(err)
}

#[tauri::command]
pub fn delete_page(state: State<'_, AppState>, page_id: String) -> CmdResult<()> {
    store(&state)?
        .delete_page_permanently(id(&page_id)?)
        .map_err(err)
}

#[tauri::command]
pub fn load_document(state: State<'_, AppState>, page_id: String) -> CmdResult<Document> {
    store(&state)?.load_document(id(&page_id)?).map_err(err)
}

/// Saves the editor's changes on top of the page as it is now (other devices may have changed it
/// since the editor loaded `base`). Returns the merged page for the editor to show.
#[tauri::command]
pub fn save_document_rebased(
    state: State<'_, AppState>,
    page_id: String,
    base: Document,
    document: Document,
) -> CmdResult<notercrux_domain::Rebased> {
    store(&state)?
        .save_document_rebased(id(&page_id)?, &base, &document)
        .map_err(err)
}

#[tauri::command]
pub fn save_document(
    state: State<'_, AppState>,
    page_id: String,
    document: Document,
) -> CmdResult<()> {
    store(&state)?
        .save_document(id(&page_id)?, &document)
        .map_err(err)
}

#[tauri::command]
pub fn search_pages(state: State<'_, AppState>, query: String) -> CmdResult<Vec<SearchHit>> {
    store(&state)?.search_with_snippets(&query, 20).map_err(err)
}

#[tauri::command]
pub fn backlinks(state: State<'_, AppState>, page_id: String) -> CmdResult<Vec<Page>> {
    store(&state)?.backlinks(id(&page_id)?).map_err(err)
}

#[tauri::command]
pub fn create_database(
    state: State<'_, AppState>,
    parent_id: Option<String>,
    title: String,
) -> CmdResult<Page> {
    let parent = parent_id.as_deref().map(id).transpose()?;
    store(&state)?.create_database(parent, &title).map_err(err)
}

#[tauri::command]
pub fn get_schema(state: State<'_, AppState>, database_id: String) -> CmdResult<DatabaseSchema> {
    store(&state)?.get_schema(id(&database_id)?).map_err(err)
}

#[tauri::command]
pub fn set_schema(
    state: State<'_, AppState>,
    database_id: String,
    schema: DatabaseSchema,
) -> CmdResult<()> {
    store(&state)?
        .set_schema(id(&database_id)?, &schema)
        .map_err(err)
}

#[tauri::command]
pub fn list_records(state: State<'_, AppState>, database_id: String) -> CmdResult<Vec<Record>> {
    store(&state)?.list_records(id(&database_id)?).map_err(err)
}

#[tauri::command]
pub fn set_property(
    state: State<'_, AppState>,
    page_id: String,
    property_id: String,
    value: Value,
) -> CmdResult<()> {
    store(&state)?
        .set_property(id(&page_id)?, &property_id, &value)
        .map_err(err)
}

#[tauri::command]
pub fn get_properties(
    state: State<'_, AppState>,
    page_id: String,
) -> CmdResult<HashMap<String, Value>> {
    store(&state)?.get_properties(id(&page_id)?).map_err(err)
}

/// Largest file the importer will read.
const MAX_IMPORT_BYTES: u64 = 10 * 1024 * 1024;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedFile {
    pub name: String,
    pub contents: String,
}

/// Asks the user where to save, then writes there. The webview never supplies a path, so it cannot
/// write anywhere the user did not choose. Returns `false` if the dialog was cancelled.
#[tauri::command]
pub async fn export_file(
    app: tauri::AppHandle,
    default_name: String,
    extension: String,
    contents: String,
) -> CmdResult<bool> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_file_name(&default_name)
        .add_filter(&extension, &[extension.as_str()])
        .save_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(path) = rx.recv().map_err(err)? else {
        return Ok(false);
    };
    let path = path.into_path().map_err(err)?;
    std::fs::write(path, contents).map_err(err)?;
    Ok(true)
}

/// Asks the user to pick a Markdown or JSON file and returns its text. `None` if cancelled.
#[tauri::command]
pub async fn import_file(app: tauri::AppHandle) -> CmdResult<Option<ImportedFile>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .add_filter(
            "Markdown or Notercrux page",
            &["md", "markdown", "txt", "json"],
        )
        .pick_file(move |p| {
            let _ = tx.send(p);
        });
    let Some(path) = rx.recv().map_err(err)? else {
        return Ok(None);
    };
    let path = path.into_path().map_err(err)?;
    let size = std::fs::metadata(&path).map_err(err)?.len();
    if size > MAX_IMPORT_BYTES {
        return Err(format!(
            "That file is too large to import ({} MB, limit 10 MB).",
            size / 1_048_576
        ));
    }
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("Could not read the file as text: {e}"))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Some(ImportedFile { name, contents }))
}
