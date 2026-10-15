mod commands;

use std::sync::Mutex;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let mut store = notercrux_storage::Store::open(dir.join("notercrux.db"))?;
            commands::seed_if_empty(&mut store)?;
            app.manage(commands::AppState(Mutex::new(store)));
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Notercrux");
}
