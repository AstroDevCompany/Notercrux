//! Thin Tauri command layer. All rules live in `notercrux-domain` / `notercrux-storage`.

use notercrux_domain::{Block, BlockKind, Document, Page, PageId, DOCUMENT_SCHEMA_VERSION};
use notercrux_storage::Store;
use std::sync::{Mutex, MutexGuard};
use tauri::State;

pub struct AppState(pub Mutex<Store>);

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
pub fn search_pages(state: State<'_, AppState>, query: String) -> CmdResult<Vec<Page>> {
    store(&state)?.search(&query, 20).map_err(err)
}
