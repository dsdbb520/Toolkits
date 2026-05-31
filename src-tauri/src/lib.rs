mod bili;
mod media;
mod image_editor;
mod notes;
mod steam;
mod telegram;

use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tauri::Manager;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub tg: telegram::TgState,
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

#[tauri::command]
fn toggle_note_type(state: tauri::State<AppState>, id: String) -> Result<notes::Note, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::toggle_note_type(&db, &id).map_err(|e| e.to_string())
}

// ─── 同步设置 ──────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct SyncSettings {
    server_url: String,
    token: String,
    last_sync_at: i64,
}

#[tauri::command]
fn get_sync_settings(state: tauri::State<AppState>) -> SyncSettings {
    let db = state.db.lock().unwrap();
    SyncSettings {
        server_url: notes::get_setting(&db, "server_url").unwrap_or_default(),
        token: notes::get_setting(&db, "sync_token").unwrap_or_default(),
        last_sync_at: notes::get_setting(&db, "last_sync_at")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    }
}

#[tauri::command]
fn save_sync_settings(
    state: tauri::State<AppState>,
    server_url: String,
    token: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::set_setting(&db, "server_url", &server_url).map_err(|e| e.to_string())?;
    notes::set_setting(&db, "sync_token", &token).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── 同步 ──────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct SyncRequest {
    last_sync_at: i64,
    notes: Vec<notes::Note>,
}

#[derive(serde::Deserialize)]
struct SyncResponse {
    notes: Vec<notes::Note>,
    synced_at: i64,
}

#[tauri::command]
async fn sync_notes(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    let (server_url, token, last_sync_at, local_changes) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let server_url = notes::get_setting(&db, "server_url").unwrap_or_default();
        let token = notes::get_setting(&db, "sync_token").unwrap_or_default();
        let last_sync_at = notes::get_setting(&db, "last_sync_at")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0i64);
        let changes = notes::get_changes_since(&db, last_sync_at).map_err(|e| e.to_string())?;
        (server_url, token, last_sync_at, changes)
    };

    if server_url.is_empty() || token.is_empty() {
        return Err("未配置同步服务器".to_string());
    }

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{}/sync", server_url.trim_end_matches('/')))
        .bearer_auth(&token)
        .json(&SyncRequest { last_sync_at, notes: local_changes })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("同步失败 {}: {}", status, body));
    }

    let sync_resp: SyncResponse = resp.json().await.map_err(|e| e.to_string())?;
    let count = sync_resp.notes.len();

    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        notes::apply_remote_notes(&db, &sync_resp.notes).map_err(|e| e.to_string())?;
        notes::set_setting(&db, "last_sync_at", &sync_resp.synced_at.to_string())
            .map_err(|e| e.to_string())?;
    }

    Ok(count)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = Connection::open(data_dir.join("notes.db"))
                .expect("failed to open database");
            notes::init_db(&conn).expect("failed to init database");
            let tg = Arc::new(tokio::sync::Mutex::new(telegram::TgRuntimeState::new()));
            app.manage(tg.clone()); // 让 tauri::State<'_, TgState> 注入可用
            app.manage(AppState { db: Mutex::new(conn), tg });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_notes,
            create_note,
            update_note,
            delete_note,
            toggle_note_type,
            get_sync_settings,
            save_sync_settings,
            sync_notes,
            bili::bili_check_ffmpeg,
            bili::bili_get_settings,
            bili::bili_save_settings,
            bili::bili_pick_dir,
            bili::bili_get_video_info,
            bili::bili_get_play_info,
            bili::bili_download,
            bili::bili_get_history,
            bili::bili_delete_history,
            bili::bili_clear_history,
            media::media_open_file,
            media::media_get_info,
            media::media_process,
            image_editor::image_open_file,
            image_editor::image_get_info,
            image_editor::image_save_copy,
            image_editor::image_crop,
            image_editor::image_rotate,
            image_editor::image_resize,
            image_editor::image_compress,
            image_editor::image_convert,
            steam::steam_get_path,
            steam::steam_get_accounts,
            steam::steam_is_running,
            steam::steam_launch,
            steam::steam_get_userdata_path,
            steam::steam_switch_account,
            telegram::tg_get_settings,
            telegram::tg_save_settings,
            telegram::tg_is_authenticated,
            telegram::tg_request_code,
            telegram::tg_sign_in,
            telegram::tg_sign_out,
            telegram::tg_get_dialogs,
            telegram::tg_download_media,
            telegram::tg_scan_cache,
            telegram::tg_pick_dir,
            telegram::tg_get_suggested_paths,
            telegram::tg_get_watch_dirs,
            telegram::tg_watch_start,
            telegram::tg_watch_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
