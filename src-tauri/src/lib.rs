mod notes;

use rusqlite::Connection;
use std::sync::Mutex;
use tauri::Manager;

pub struct AppState {
    pub db: Mutex<Connection>,
}

#[tauri::command]
fn get_notes(state: tauri::State<AppState>) -> Result<Vec<notes::Note>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::get_all(&db).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_note(state: tauri::State<AppState>) -> Result<notes::Note, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::create(&db).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_note(
    state: tauri::State<AppState>,
    id: String,
    title: String,
    content: String,
) -> Result<notes::Note, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::update(&db, &id, &title, &content).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_note(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::delete(&db, &id).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("notes.db");
            let conn = Connection::open(db_path).expect("failed to open database");
            notes::init_db(&conn).expect("failed to init database");
            app.manage(AppState { db: Mutex::new(conn) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_notes,
            create_note,
            update_note,
            delete_note,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
