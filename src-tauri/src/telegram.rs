use std::path::PathBuf;
use std::sync::Arc;

use grammers_client::types::{Downloadable, Media};
use grammers_client::{Client, Config, SignInError};
use grammers_session::Session;
use serde::{Deserialize, Serialize};
use notify::Watcher;
use tauri::{Emitter, Manager};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::notes;
use crate::AppState;

// ─── Runtime state ────────────────────────────────────────────

pub struct TgRuntimeState {
    pub client: Option<Client>,
    pub login_token: Option<grammers_client::types::LoginToken>,
    pub password_token: Option<grammers_client::types::PasswordToken>,
    /// Cached packed chats from last tg_get_dialogs call (indexed by position)
    pub packed_chats: Vec<grammers_client::types::PackedChat>,
    /// Active file system watcher (dropping it stops the watch)
    pub file_watcher: Option<notify::RecommendedWatcher>,
}

impl TgRuntimeState {
    pub fn new() -> Self {
        Self {
            client: None,
            login_token: None,
            password_token: None,
            packed_chats: Vec::new(),
            file_watcher: None,
        }
    }
}

pub type TgState = Arc<Mutex<TgRuntimeState>>;

// ─── Public types ─────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct TgSettings {
    pub api_id: i32,
    pub api_hash: String,
}

#[derive(Serialize, Clone)]
pub struct TgDialogInfo {
    pub name: String,
    pub username: Option<String>,
    pub kind: String,   // "user" | "group" | "channel"
    pub packed: String, // index into TgRuntimeState.packed_chats, as string
}

#[derive(Serialize, Clone)]
pub struct TgProgress {
    pub task_id: String,
    pub current: u32,
    pub file_name: String,
    /// "saving" | "done" | "error" | "scanning"
    pub status: String,
    pub message: String,
}

// ─── Helpers ──────────────────────────────────────────────────

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}

/// Creates or reuses the grammers Client. Uses a file-backed session for persistence.
async fn ensure_client(
    tg: &TgState,
    api_id: i32,
    api_hash: String,
    session_path: &PathBuf,
) -> Result<Client, String> {
    let existing = { tg.lock().await.client.clone() };
    if let Some(c) = existing {
        return Ok(c);
    }

    // FileSession auto-saves on update (auth key negotiation, login, etc.)
    let session = Session::load_file_or_create(session_path)
        .map_err(|e| format!("会话文件错误: {}", e))?;

    let client = Client::connect(Config {
        session,
        api_id,
        api_hash,
        params: Default::default(),
    })
    .await
    .map_err(|e| format!("连接 Telegram 失败: {}", e))?;

    tg.lock().await.client = Some(client.clone());
    Ok(client)
}

fn get_api_creds(state: &tauri::State<AppState>) -> Result<(i32, String), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let id = notes::get_setting(&db, "tg_api_id")
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|&v| v != 0)
        .ok_or("请先配置 API ID")?;
    let hash = notes::get_setting(&db, "tg_api_hash")
        .filter(|h| !h.is_empty())
        .ok_or("请先配置 API Hash")?;
    Ok((id, hash))
}

// ─── Commands: settings ───────────────────────────────────────

#[tauri::command]
pub fn tg_get_settings(state: tauri::State<AppState>) -> TgSettings {
    let db = state.db.lock().unwrap();
    TgSettings {
        api_id: notes::get_setting(&db, "tg_api_id")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        api_hash: notes::get_setting(&db, "tg_api_hash").unwrap_or_default(),
    }
}

#[tauri::command]
pub fn tg_save_settings(
    state: tauri::State<AppState>,
    api_id: i32,
    api_hash: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::set_setting(&db, "tg_api_id", &api_id.to_string()).map_err(|e| e.to_string())?;
    notes::set_setting(&db, "tg_api_hash", &api_hash).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Commands: auth ───────────────────────────────────────────

#[tauri::command]
pub async fn tg_is_authenticated(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    tg: tauri::State<'_, TgState>,
) -> Result<bool, String> {
    let (api_id, api_hash) = match get_api_creds(&state) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };

    let data_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let session_path = data_dir.join("telegram.session");

    if !session_path.exists() {
        return Ok(false);
    }

    let client = ensure_client(&tg, api_id, api_hash, &session_path).await?;
    client.is_authorized().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tg_request_code(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    tg: tauri::State<'_, TgState>,
    phone: String,
) -> Result<(), String> {
    let (api_id, api_hash) = get_api_creds(&state)?;
    let data_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let session_path = data_dir.join("telegram.session");

    let client = ensure_client(&tg, api_id, api_hash, &session_path).await?;
    let token = client
        .request_login_code(&phone)
        .await
        .map_err(|e| format!("发送验证码失败: {}", e))?;

    tg.lock().await.login_token = Some(token);
    Ok(())
}

#[tauri::command]
pub async fn tg_sign_in(
    tg: tauri::State<'_, TgState>,
    code: String,
    password: String,
) -> Result<String, String> {
    // Check if we're in the 2FA step (pw_token stored from previous call)
    let pw_token = { tg.lock().await.password_token.take() };
    let client = { tg.lock().await.client.clone().ok_or("请先请求验证码")? };

    let user = if let Some(pw_token) = pw_token {
        // 2FA step
        if password.is_empty() {
            return Err("请输入两步验证密码".to_string());
        }
        client
            .check_password(pw_token, &password)
            .await
            .map_err(|e| format!("两步验证失败: {}", e))?
    } else {
        // Consume login_token (code can only be used once)
        let login_token = { tg.lock().await.login_token.take().ok_or("请先请求验证码")? };
        match client.sign_in(&login_token, &code).await {
            Ok(user) => user,
            Err(SignInError::PasswordRequired(pw_token)) => {
                if password.is_empty() {
                    tg.lock().await.password_token = Some(pw_token);
                    return Err("2FA_REQUIRED".to_string());
                }
                client
                    .check_password(pw_token, &password)
                    .await
                    .map_err(|e| format!("两步验证失败: {}", e))?
            }
            Err(e) => return Err(format!("验证失败: {}", e)),
        }
    };

    // FileSession auto-saves; clear pending tokens
    let mut tg_state = tg.lock().await;
    tg_state.login_token = None;
    tg_state.password_token = None;

    Ok(user.first_name().to_string())
}

#[tauri::command]
pub async fn tg_sign_out(
    app_handle: tauri::AppHandle,
    tg: tauri::State<'_, TgState>,
) -> Result<(), String> {
    let client = { tg.lock().await.client.clone().ok_or("未登录")? };

    client.sign_out().await.map_err(|e| format!("退出失败: {}", e))?;

    let data_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_file(data_dir.join("telegram.session")).await;

    let mut state = tg.lock().await;
    state.client = None;
    state.login_token = None;
    state.password_token = None;
    state.packed_chats.clear();

    Ok(())
}

// ─── Commands: dialogs ────────────────────────────────────────

#[tauri::command]
pub async fn tg_get_dialogs(tg: tauri::State<'_, TgState>) -> Result<Vec<TgDialogInfo>, String> {
    let client = { tg.lock().await.client.clone().ok_or("未登录，请先完成验证")? };

    let mut iter = client.iter_dialogs();
    let mut results: Vec<TgDialogInfo> = Vec::new();
    let mut packed_chats: Vec<grammers_client::types::PackedChat> = Vec::new();

    while let Some(dialog) = iter.next().await.map_err(|e| e.to_string())? {
        let chat = dialog.chat();
        let kind = match chat {
            grammers_client::types::Chat::User(_) => "user",
            grammers_client::types::Chat::Group(_) => "group",
            grammers_client::types::Chat::Channel(_) => "channel",
        }
        .to_string();

        let idx = packed_chats.len();
        packed_chats.push(chat.pack());

        results.push(TgDialogInfo {
            name: chat.name().to_string(),
            username: chat.username().map(|u| u.to_string()),
            kind,
            packed: idx.to_string(),
        });

        if results.len() >= 200 {
            break;
        }
    }

    tg.lock().await.packed_chats = packed_chats;
    Ok(results)
}

// ─── Commands: download ───────────────────────────────────────

#[tauri::command]
pub async fn tg_download_media(
    app_handle: tauri::AppHandle,
    tg: tauri::State<'_, TgState>,
    task_id: String,
    packed_chat: String, // index into TgRuntimeState.packed_chats
    media_types: Vec<String>,
    output_dir: String,
    limit: i32,
) -> Result<u32, String> {
    let (client, packed) = {
        let state = tg.lock().await;
        let c = state.client.clone().ok_or("未登录")?;
        let idx: usize = packed_chat.parse().map_err(|_| "无效的对话索引")?;
        let p = state.packed_chats.get(idx).cloned().ok_or("对话已失效，请刷新列表")?;
        (c, p)
    };

    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| format!("创建目录失败: {}", e))?;

    let want_photo = media_types.contains(&"photo".to_string());
    let want_video = media_types.contains(&"video".to_string());
    let want_doc = media_types.contains(&"document".to_string());

    let mut iter = if limit > 0 {
        client.iter_messages(packed).limit(limit as usize)
    } else {
        client.iter_messages(packed)
    };

    let mut downloaded = 0u32;

    while let Some(message) = iter.next().await.map_err(|e| e.to_string())? {
        let media = match message.media() {
            Some(m) => m,
            None => continue,
        };

        // Determine file info from a borrow; borrow ends before we consume media
        let (file_name, out_path) = match &media {
            Media::Photo(_) => {
                if !want_photo { continue; }
                let n = format!("msg_{:08}.jpg", message.id());
                let p = PathBuf::from(&output_dir).join(&n);
                (n, p)
            }
            Media::Document(doc) => {
                let mime = doc.mime_type().unwrap_or("");
                let is_video = mime.starts_with("video/");
                if is_video && !want_video { continue; }
                if !is_video && !want_doc { continue; }
                let ext = if is_video { "mp4" } else { mime.split('/').last().unwrap_or("bin") };
                let raw = doc.name();  // &str, empty if unnamed
                let n = if raw.is_empty() {
                    sanitize(&format!("msg_{:08}.{}", message.id(), ext))
                } else {
                    sanitize(raw)
                };
                let p = PathBuf::from(&output_dir).join(&n);
                (n, p)
            }
            _ => continue,
        };
        // All borrows of media end here

        let _ = app_handle.emit("tg_progress", TgProgress {
            task_id: task_id.clone(), current: downloaded,
            file_name: file_name.clone(), status: "saving".to_string(), message: String::new(),
        });

        // Consume media into Downloadable::Media for iter_download
        let d = Downloadable::Media(media);
        let mut dl = client.iter_download(&d);
        let result: Result<(), String> = async {
            let mut f = tokio::fs::File::create(&out_path).await
                .map_err(|e| format!("创建文件失败: {}", e))?;
            while let Some(chunk) = dl.next().await.map_err(|e| e.to_string())? {
                f.write_all(&chunk).await.map_err(|e| e.to_string())?;
            }
            Ok(())
        }.await;

        match result {
            Ok(_) => downloaded += 1,
            Err(e) => { let _ = app_handle.emit("tg_progress", TgProgress {
                task_id: task_id.clone(), current: downloaded,
                file_name, status: "error".to_string(), message: e,
            }); }
        }
    }

    let _ = app_handle.emit("tg_progress", TgProgress {
        task_id,
        current: downloaded,
        file_name: String::new(),
        status: "done".to_string(),
        message: format!("共下载 {} 个文件", downloaded),
    });

    Ok(downloaded)
}

// ─── Commands: cache scan ─────────────────────────────────────

#[tauri::command]
pub async fn tg_scan_cache(
    app_handle: tauri::AppHandle,
    task_id: String,
    source_dirs: Vec<String>,
    output_dir: String,
) -> Result<u32, String> {
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| format!("创建输出目录失败: {}", e))?;

    const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "heic"];
    const VIDEO_EXTS: &[&str] = &["mp4", "mov", "avi", "mkv", "webm", "flv", "m4v", "3gp"];

    let mut total = 0u32;

    for source_dir in &source_dirs {
        let path = PathBuf::from(source_dir);
        if !path.exists() { continue; }
        let _ = app_handle.emit("tg_progress", TgProgress {
            task_id: task_id.clone(), current: total,
            file_name: source_dir.clone(), status: "scanning".to_string(), message: String::new(),
        });
        total += scan_and_copy(&app_handle, &task_id, &path, &PathBuf::from(&output_dir), IMAGE_EXTS, VIDEO_EXTS, 8).await;
    }

    let _ = app_handle.emit("tg_progress", TgProgress {
        task_id,
        current: total,
        file_name: String::new(),
        status: "done".to_string(),
        message: format!("共复制 {} 个文件", total),
    });

    Ok(total)
}

fn scan_and_copy<'a>(
    app: &'a tauri::AppHandle,
    task_id: &'a str,
    dir: &'a PathBuf,
    output: &'a PathBuf,
    image_exts: &'a [&'static str],
    video_exts: &'a [&'static str],
    depth: u32,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = u32> + Send + 'a>> {
    Box::pin(async move {
        if depth == 0 { return 0; }
        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => return 0,
        };
        let mut count = 0u32;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                count += scan_and_copy(app, task_id, &path, output, image_exts, video_exts, depth - 1).await;
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
            // For extensionless files (Telegram cache), detect type by magic bytes.
            // For files that already have an extension, filter by extension list.
            let file_name: String = if ext.is_empty() {
                match detect_media_type(&path) {
                    Some((det_ext, _)) => {
                        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
                        format!("{}.{}", stem, det_ext)
                    }
                    None => continue,
                }
            } else {
                if !image_exts.contains(&ext.as_str()) && !video_exts.contains(&ext.as_str()) { continue; }
                path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()
            };
            let dest = output.join(&file_name);
            if dest.exists() { continue; }
            if tokio::fs::copy(&path, &dest).await.is_ok() {
                count += 1;
                let _ = app.emit("tg_progress", TgProgress {
                    task_id: task_id.to_string(), current: count,
                    file_name, status: "saving".to_string(), message: String::new(),
                });
            }
        }
        count
    })
}

// ─── Commands: utilities ──────────────────────────────────────

#[tauri::command]
pub async fn tg_pick_dir() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("选择目录")
        .pick_folder()
        .await
        .map(|h| h.path().to_string_lossy().to_string())
}

#[tauri::command]
pub fn tg_get_suggested_paths() -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        // Main media cache — extensionless files, detected by magic bytes
        let user_data = PathBuf::from(&appdata)
            .join("Telegram Desktop").join("tdata").join("user_data");
        if user_data.exists() { paths.push(user_data.to_string_lossy().to_string()); }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        // Explicitly saved / auto-downloaded files
        let tg_dl = PathBuf::from(&userprofile).join("Downloads").join("Telegram Desktop");
        if tg_dl.exists() { paths.push(tg_dl.to_string_lossy().to_string()); }
    }
    paths
}

// ─── Commands: real-time file watch ───────────────────────────

/// Returns directories to watch.
#[tauri::command]
pub fn tg_get_watch_dirs() -> Vec<String> {
    let mut dirs = Vec::new();

    if let Ok(appdata) = std::env::var("APPDATA") {
        // Primary Telegram Desktop media cache (extensionless files)
        let user_data = PathBuf::from(&appdata)
            .join("Telegram Desktop").join("tdata").join("user_data");
        if user_data.exists() { dirs.push(user_data.to_string_lossy().to_string()); }
    }

    // Windows TEMP — catches files opened via "Open with external app"
    if let Ok(temp) = std::env::var("TEMP") {
        dirs.push(temp);
    }

    // Telegram Desktop auto-download / saved files directory
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        let tg = PathBuf::from(&userprofile).join("Downloads").join("Telegram Desktop");
        if tg.exists() { dirs.push(tg.to_string_lossy().to_string()); }
    }

    dirs
}

/// Starts watching directories and auto-copies detected media files.
/// Emits "tg_progress" events. Dropping the internal watcher (via tg_watch_stop) stops the watch.
#[tauri::command]
pub async fn tg_watch_start(
    app_handle: tauri::AppHandle,
    tg: tauri::State<'_, TgState>,
    task_id: String,
    watch_dirs: Vec<String>,
    output_dir: String,
    min_size_kb: u64,
) -> Result<(), String> {
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| format!("创建输出目录失败: {}", e))?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<notify::Result<notify::Event>>(200);

    // Drop any previous watcher first
    { tg.lock().await.file_watcher = None; }

    let mut watcher = notify::RecommendedWatcher::new(
        move |res| { let _ = tx.blocking_send(res); },
        notify::Config::default(),
    )
    .map_err(|e| format!("创建文件监控失败: {}", e))?;

    for dir in &watch_dirs {
        let path = PathBuf::from(dir);
        if path.exists() {
            let _ = watcher.watch(&path, notify::RecursiveMode::Recursive);
        }
    }

    tg.lock().await.file_watcher = Some(watcher);

    let min_bytes = min_size_kb * 1024;
    let out = output_dir.clone();
    tokio::spawn(async move {
        let mut captured = 0u32;
        while let Some(res) = rx.recv().await {
            let event = match res { Ok(e) => e, Err(_) => continue };
            let is_create = matches!(
                event.kind,
                notify::EventKind::Create(_) | notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
            );
            if !is_create { continue; }

            for src_path in &event.paths {
                // Wait briefly for the write to finish
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                // Size filter
                let size = match std::fs::metadata(src_path) {
                    Ok(m) => m.len(),
                    Err(_) => continue,
                };
                if size < min_bytes { continue; }

                // Detect media type by magic bytes
                let Some((ext, _)) = detect_media_type(src_path) else { continue };

                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let out_name = format!("tg_watch_{}.{}", ts, ext);
                let dest = PathBuf::from(&out).join(&out_name);
                if dest.exists() { continue; }

                if tokio::fs::copy(src_path, &dest).await.is_ok() {
                    captured += 1;
                    let _ = app_handle.emit("tg_progress", TgProgress {
                        task_id: task_id.clone(),
                        current: captured,
                        file_name: out_name,
                        status: "saving".to_string(),
                        message: src_path.to_string_lossy().to_string(),
                    });
                }
            }
        }
        // Channel closed: watcher was dropped (tg_watch_stop called)
        let _ = app_handle.emit("tg_progress", TgProgress {
            task_id,
            current: captured,
            file_name: String::new(),
            status: "done".to_string(),
            message: format!("监控已停止，共捕获 {} 个文件", captured),
        });
    });

    Ok(())
}

#[tauri::command]
pub async fn tg_watch_stop(tg: tauri::State<'_, TgState>) -> Result<(), String> {
    // Dropping the watcher closes the channel → spawned loop exits
    tg.lock().await.file_watcher = None;
    Ok(())
}

/// Identify media type from the first bytes of a file (magic bytes).
/// Returns (extension, mime) or None if not a recognized media type.
fn detect_media_type(path: &PathBuf) -> Option<(&'static str, &'static str)> {
    use std::io::Read;
    let mut buf = [0u8; 12];
    let mut f = std::fs::File::open(path).ok()?;
    let n = f.read(&mut buf).ok()?;
    if n < 4 { return None; }

    // JPEG
    if buf[..3] == [0xFF, 0xD8, 0xFF] { return Some(("jpg", "image/jpeg")); }
    // PNG
    if buf[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] { return Some(("png", "image/png")); }
    // GIF
    if buf[..4] == *b"GIF8" { return Some(("gif", "image/gif")); }
    // WEBP
    if n >= 12 && buf[..4] == *b"RIFF" && buf[8..12] == *b"WEBP" { return Some(("webp", "image/webp")); }
    // MP4 / MOV — ISO Base Media file (ftyp box at offset 4)
    if n >= 8 && buf[4..8] == *b"ftyp" { return Some(("mp4", "video/mp4")); }
    // WebM / MKV
    if buf[..4] == [0x1A, 0x45, 0xDF, 0xA3] { return Some(("webm", "video/webm")); }
    // OGG
    if buf[..4] == *b"OggS" { return Some(("ogg", "video/ogg")); }
    // AVI
    if n >= 8 && buf[..4] == *b"RIFF" && buf[8..12] == *b"AVI " { return Some(("avi", "video/avi")); }

    None
}
