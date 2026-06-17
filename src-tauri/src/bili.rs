use futures_util::StreamExt;
use reqwest::Client;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

use crate::notes;
use crate::AppState;

// ─── Public types ─────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct BiliSettings {
    pub sessdata: String,
    pub download_dir: String,
    pub default_quality: i32,
}

#[derive(Serialize, Clone)]
pub struct VideoInfo {
    pub bvid: String,
    pub title: String,
    pub cover: String,
    pub parts: Vec<VideoPart>,
}

#[derive(Serialize, Clone)]
pub struct VideoPart {
    pub cid: i64,
    pub page: i32,
    pub part: String,
}

#[derive(Serialize, Clone)]
pub struct PlayInfo {
    pub quality: i32,
    pub accept_quality: Vec<i32>,
    pub accept_description: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct DownloadRecord {
    pub id: String,
    pub bvid: String,
    pub title: String,
    pub cover: String,
    pub part: String,
    pub quality: i32,
    pub mode: String,
    pub file_path: String,
    pub file_size: i64,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    task_id: String,
    downloaded: u64,
    total: u64,
    /// "downloading" | "merging" | "converting" | "done" | "error"
    status: String,
    /// "video" | "audio" | "flv" | ""
    phase: String,
    message: String,
}

// ─── B站 API types ────────────────────────────────────────────

#[derive(Deserialize)]
struct ApiResp<T> {
    code: i32,
    message: String,
    data: Option<T>,
}

#[derive(Deserialize)]
struct ViewData {
    bvid: String,
    title: String,
    pic: String,
    cid: i64,
    pages: Vec<ViewPage>,
}

#[derive(Deserialize)]
struct ViewPage {
    cid: i64,
    page: i32,
    part: String,
}

#[derive(Deserialize)]
struct PlayData {
    quality: i32,
    accept_quality: Vec<i32>,
    accept_description: Vec<String>,
    #[serde(default)]
    durl: Vec<DUrlItem>,
    dash: Option<DashData>,
}

#[derive(Deserialize)]
struct DashData {
    video: Vec<DashStream>,
    audio: Vec<DashStream>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DashStream {
    id: i32,
    base_url: String,
    bandwidth: u64,
    #[serde(default)]
    codecid: i32,
}

#[derive(Deserialize)]
struct DUrlItem {
    url: String,
    size: i64,
}

// ─── Helpers ──────────────────────────────────────────────────

fn ffmpeg_runs(p: &str) -> bool {
    std::process::Command::new(p)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn find_ffmpeg() -> Option<String> {
    let candidates = [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
        "ffmpeg",
    ];
    if let Some(p) = candidates.iter().find(|&&p| ffmpeg_runs(p)) {
        return Some(p.to_string());
    }
    // Windows：进程 PATH 可能是 app 启动时的旧快照（装 ffmpeg 后没重启）。
    // 回读注册表里持久化的 PATH，去里面搜 ffmpeg.exe。
    #[cfg(windows)]
    {
        if let Some(p) = find_ffmpeg_from_registry_path() {
            return Some(p);
        }
    }
    None
}

#[cfg(windows)]
fn find_ffmpeg_from_registry_path() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut dirs = String::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(env) =
        hklm.open_subkey(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")
    {
        if let Ok(p) = env.get_value::<String, _>("Path") {
            dirs.push_str(&p);
            dirs.push(';');
        }
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(env) = hkcu.open_subkey("Environment") {
        if let Ok(p) = env.get_value::<String, _>("Path") {
            dirs.push_str(&p);
        }
    }

    for dir in dirs.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let exe = std::path::Path::new(dir).join("ffmpeg.exe");
        if exe.exists() {
            let s = exe.to_string_lossy().to_string();
            if ffmpeg_runs(&s) {
                return Some(s);
            }
        }
    }
    None
}

async fn run_ffmpeg(ffmpeg: &str, args: &[&str]) -> Result<(), String> {
    let out = tokio::process::Command::new(ffmpeg)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("ffmpeg 启动失败: {}", e))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("ffmpeg 失败: {}", &msg[..msg.len().min(300)]));
    }
    Ok(())
}

fn build_client(sessdata: &str) -> reqwest::Result<Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if !sessdata.is_empty() {
        if let Ok(v) = format!("SESSDATA={}", sessdata).parse() {
            headers.insert(reqwest::header::COOKIE, v);
        }
    }
    if let Ok(v) = "https://www.bilibili.com".parse() {
        headers.insert(reqwest::header::REFERER, v);
    }
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .default_headers(headers)
        .build()
}

fn extract_bvid(input: &str) -> Option<String> {
    let s = input.trim();
    if s.starts_with("BV") {
        let end = s.find(|c: char| !c.is_alphanumeric()).unwrap_or(s.len());
        if end >= 12 { return Some(s[..end].to_string()); }
    }
    if let Some(idx) = s.find("/BV") {
        let after = &s[idx + 1..];
        let end = after.find(|c: char| !c.is_alphanumeric()).unwrap_or(after.len());
        if end >= 12 { return Some(after[..end].to_string()); }
    }
    None
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| match c {
        '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
        c => c,
    }).collect()
}

fn base_filename(title: &str, part: &str, page: i32, is_multi: bool) -> String {
    if is_multi {
        format!("{} - P{} {}", sanitize(title), page, sanitize(part))
    } else {
        sanitize(title)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ─── Commands ─────────────────────────────────────────────────

#[tauri::command]
pub fn bili_check_ffmpeg() -> Option<String> {
    let ffmpeg = find_ffmpeg()?;
    let out = std::process::Command::new(&ffmpeg).arg("-version").output().ok()?;
    String::from_utf8(out.stdout).ok()?.lines().next().map(|l| l.to_string())
}

#[tauri::command]
pub fn bili_get_settings(state: tauri::State<AppState>) -> BiliSettings {
    let db = state.db.lock().unwrap();
    BiliSettings {
        sessdata: notes::get_setting(&db, "bili_sessdata").unwrap_or_default(),
        download_dir: notes::get_setting(&db, "bili_download_dir").unwrap_or_default(),
        default_quality: notes::get_setting(&db, "bili_default_quality")
            .and_then(|v| v.parse().ok())
            .unwrap_or(80),
    }
}

#[tauri::command]
pub fn bili_save_settings(
    state: tauri::State<AppState>,
    sessdata: String,
    download_dir: String,
    default_quality: i32,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    notes::set_setting(&db, "bili_sessdata", &sessdata).map_err(|e| e.to_string())?;
    notes::set_setting(&db, "bili_download_dir", &download_dir).map_err(|e| e.to_string())?;
    notes::set_setting(&db, "bili_default_quality", &default_quality.to_string())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn bili_pick_dir() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("选择下载目录")
        .pick_folder()
        .await
        .map(|h| h.path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn bili_get_video_info(url: String, sessdata: String) -> Result<VideoInfo, String> {
    let bvid = extract_bvid(&url)
        .ok_or_else(|| "无法识别BV号，请检查URL或BV号格式".to_string())?;
    let client = build_client(&sessdata).map_err(|e| e.to_string())?;

    let resp: ApiResp<ViewData> = client
        .get("https://api.bilibili.com/x/web-interface/view")
        .query(&[("bvid", &bvid)])
        .send().await.map_err(|e| format!("网络错误: {}", e))?
        .json().await.map_err(|e| format!("解析失败: {}", e))?;

    if resp.code != 0 {
        return Err(format!("B站API错误 {}: {}", resp.code, resp.message));
    }
    let data = resp.data.ok_or("返回数据为空")?;

    let parts = if data.pages.len() <= 1 {
        vec![VideoPart { cid: data.cid, page: 1, part: data.title.clone() }]
    } else {
        data.pages.iter().map(|p| VideoPart { cid: p.cid, page: p.page, part: p.part.clone() }).collect()
    };

    Ok(VideoInfo { bvid: data.bvid, title: data.title, cover: data.pic, parts })
}

#[tauri::command]
pub async fn bili_get_play_info(
    bvid: String,
    cid: i64,
    sessdata: String,
) -> Result<PlayInfo, String> {
    let client = build_client(&sessdata).map_err(|e| e.to_string())?;

    let resp: ApiResp<PlayData> = client
        .get("https://api.bilibili.com/x/player/playurl")
        .query(&[
            ("bvid", bvid.as_str()),
            ("cid", &cid.to_string()),
            ("qn", "0"),
            ("fnval", "16"),
            ("fnver", "0"),
            ("fourk", "1"),
        ])
        .send().await.map_err(|e| format!("网络错误: {}", e))?
        .json().await.map_err(|e| format!("解析失败: {}", e))?;

    if resp.code != 0 {
        return Err(format!("B站API错误 {}: {}", resp.code, resp.message));
    }
    let data = resp.data.ok_or("返回数据为空")?;

    Ok(PlayInfo {
        quality: data.quality,
        accept_quality: data.accept_quality,
        accept_description: data.accept_description,
    })
}

// ─── History commands ─────────────────────────────────────────

#[tauri::command]
pub fn bili_get_history(state: tauri::State<AppState>) -> Vec<DownloadRecord> {
    let db = state.db.lock().unwrap();
    let mut stmt = match db.prepare(
        "SELECT id, bvid, title, cover, part, quality, mode, file_path, file_size, created_at
         FROM download_history ORDER BY created_at DESC LIMIT 200",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let rows = stmt.query_map([], |row| {
        Ok(DownloadRecord {
            id: row.get(0)?,
            bvid: row.get(1)?,
            title: row.get(2)?,
            cover: row.get(3)?,
            part: row.get(4)?,
            quality: row.get(5)?,
            mode: row.get(6)?,
            file_path: row.get(7)?,
            file_size: row.get(8)?,
            created_at: row.get(9)?,
        })
    });
    match rows {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
        Err(_) => vec![],
    }
}

#[tauri::command]
pub fn bili_delete_history(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute("DELETE FROM download_history WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn bili_clear_history(state: tauri::State<AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute("DELETE FROM download_history", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Download ─────────────────────────────────────────────────

#[tauri::command]
pub async fn bili_download(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    task_id: String,
    bvid: String,
    cid: i64,
    quality: i32,
    mode: String,
    title: String,
    part: String,
    page: i32,
    is_multi: bool,
    cover: String,
    sessdata: String,
    download_dir: String,
) -> Result<String, String> {
    let client = build_client(&sessdata).map_err(|e| e.to_string())?;
    let base = base_filename(&title, &part, page, is_multi);

    let path = match mode.as_str() {
        "audio_mp3" => {
            download_mp3(&app_handle, &task_id, &client, &bvid, cid, &base, &download_dir).await
        }
        _ => {
            if quality >= 112 {
                download_dash_mp4(&app_handle, &task_id, &client, &bvid, cid, quality, &base, &download_dir).await
            } else {
                download_flv_mp4(&app_handle, &task_id, &client, &bvid, cid, quality, &base, &download_dir).await
            }
        }
    }?;

    // 写入下载历史
    let file_size = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
    let record_id = uuid::Uuid::new_v4().to_string();
    if let Ok(db) = state.db.lock() {
        let _ = db.execute(
            "INSERT INTO download_history (id, bvid, title, cover, part, quality, mode, file_path, file_size, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![record_id, bvid, title, cover, part, quality, mode, path, file_size, now_ms()],
        );
    }

    Ok(path)
}

// ─── FLV → MP4 (qn < 112) ────────────────────────────────────

async fn download_flv_mp4(
    app: &tauri::AppHandle,
    task_id: &str,
    client: &Client,
    bvid: &str,
    cid: i64,
    quality: i32,
    base: &str,
    dir: &str,
) -> Result<String, String> {
    let resp: ApiResp<PlayData> = client
        .get("https://api.bilibili.com/x/player/playurl")
        .query(&[
            ("bvid", bvid), ("cid", &cid.to_string()),
            ("qn", &quality.to_string()), ("fnval", "0"), ("fnver", "0"), ("fourk", "1"),
        ])
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp.code != 0 { return Err(format!("B站API错误 {}: {}", resp.code, resp.message)); }
    let data = resp.data.ok_or("返回数据为空")?;
    if data.durl.is_empty() { return Err("无可用下载链接，请检查登录状态或视频权限".to_string()); }

    let ffmpeg = find_ffmpeg();
    let flv_path = PathBuf::from(dir).join(format!("{}.flv.tmp", base));
    let final_path = if ffmpeg.is_some() {
        PathBuf::from(dir).join(format!("{}.mp4", base))
    } else {
        PathBuf::from(dir).join(format!("{}.flv", base))
    };

    if let Some(p) = flv_path.parent() { tokio::fs::create_dir_all(p).await.map_err(|e| e.to_string())?; }

    let total: u64 = data.durl.iter().map(|d| d.size as u64).sum();
    let mut file = tokio::fs::File::create(&flv_path).await.map_err(|e| format!("创建文件失败: {}", e))?;
    let mut downloaded = 0u64;

    for seg in &data.durl {
        let seg_resp = client.get(&seg.url).send().await.map_err(|e| format!("下载失败: {}", e))?;
        let mut stream = seg_resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| e.to_string())?;
            file.write_all(&bytes).await.map_err(|e| e.to_string())?;
            downloaded += bytes.len() as u64;
            let _ = app.emit("bili_progress", ProgressEvent {
                task_id: task_id.to_string(), downloaded, total,
                status: "downloading".to_string(), phase: "flv".to_string(), message: String::new(),
            });
        }
    }
    drop(file);

    if let Some(ref ff) = ffmpeg {
        let _ = app.emit("bili_progress", ProgressEvent {
            task_id: task_id.to_string(), downloaded: 0, total: 0,
            status: "merging".to_string(), phase: String::new(), message: String::new(),
        });
        run_ffmpeg(ff, &["-i", flv_path.to_str().unwrap(), "-c", "copy", "-y", final_path.to_str().unwrap()]).await?;
        let _ = tokio::fs::remove_file(&flv_path).await;
    }

    let path_str = final_path.to_string_lossy().to_string();
    let _ = app.emit("bili_progress", ProgressEvent {
        task_id: task_id.to_string(), downloaded, total,
        status: "done".to_string(), phase: String::new(), message: path_str.clone(),
    });
    Ok(path_str)
}

// ─── DASH → MP4 (qn >= 112) ──────────────────────────────────

async fn download_dash_mp4(
    app: &tauri::AppHandle,
    task_id: &str,
    client: &Client,
    bvid: &str,
    cid: i64,
    quality: i32,
    base: &str,
    dir: &str,
) -> Result<String, String> {
    let ffmpeg = find_ffmpeg().ok_or("未找到 ffmpeg，请先安装: brew install ffmpeg")?;

    let resp: ApiResp<PlayData> = client
        .get("https://api.bilibili.com/x/player/playurl")
        .query(&[
            ("bvid", bvid), ("cid", &cid.to_string()),
            ("qn", &quality.to_string()), ("fnval", "16"), ("fnver", "0"), ("fourk", "1"),
        ])
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp.code != 0 { return Err(format!("B站API错误 {}: {}", resp.code, resp.message)); }
    let data = resp.data.ok_or("返回数据为空")?;
    let dash = data.dash.ok_or("未获取到DASH流，请检查登录状态")?;

    let video_stream = dash.video.iter()
        .filter(|v| v.id == quality)
        .max_by_key(|v| if v.codecid == 7 { 1u8 } else { 0 })
        .or_else(|| dash.video.iter().find(|v| v.id == quality))
        .ok_or_else(|| format!("未找到清晰度 {} 的视频流", quality))?;

    let audio_stream = dash.audio.iter().max_by_key(|a| a.bandwidth).ok_or("未找到音频流")?;

    let tmp = std::env::temp_dir();
    let video_tmp = tmp.join(format!("bili_{}_v.m4s", task_id));
    let audio_tmp = tmp.join(format!("bili_{}_a.m4s", task_id));
    let out_path = PathBuf::from(dir).join(format!("{}.mp4", base));

    if let Some(p) = out_path.parent() { tokio::fs::create_dir_all(p).await.map_err(|e| e.to_string())?; }

    // 下载视频流
    {
        let vr = client.get(&video_stream.base_url).send().await.map_err(|e| e.to_string())?;
        let total = vr.content_length().unwrap_or(0);
        let mut f = tokio::fs::File::create(&video_tmp).await.map_err(|e| e.to_string())?;
        let mut dl = 0u64;
        let mut stream = vr.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| e.to_string())?;
            f.write_all(&bytes).await.map_err(|e| e.to_string())?;
            dl += bytes.len() as u64;
            let _ = app.emit("bili_progress", ProgressEvent {
                task_id: task_id.to_string(), downloaded: dl, total,
                status: "downloading".to_string(), phase: "video".to_string(), message: String::new(),
            });
        }
    }

    // 下载音频流
    {
        let ar = client.get(&audio_stream.base_url).send().await.map_err(|e| e.to_string())?;
        let total = ar.content_length().unwrap_or(0);
        let mut f = tokio::fs::File::create(&audio_tmp).await.map_err(|e| e.to_string())?;
        let mut dl = 0u64;
        let mut stream = ar.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| e.to_string())?;
            f.write_all(&bytes).await.map_err(|e| e.to_string())?;
            dl += bytes.len() as u64;
            let _ = app.emit("bili_progress", ProgressEvent {
                task_id: task_id.to_string(), downloaded: dl, total,
                status: "downloading".to_string(), phase: "audio".to_string(), message: String::new(),
            });
        }
    }

    // 合并
    let _ = app.emit("bili_progress", ProgressEvent {
        task_id: task_id.to_string(), downloaded: 0, total: 0,
        status: "merging".to_string(), phase: String::new(), message: String::new(),
    });
    run_ffmpeg(&ffmpeg, &[
        "-i", video_tmp.to_str().unwrap(),
        "-i", audio_tmp.to_str().unwrap(),
        "-c", "copy", "-y",
        out_path.to_str().unwrap(),
    ]).await?;

    let _ = tokio::fs::remove_file(&video_tmp).await;
    let _ = tokio::fs::remove_file(&audio_tmp).await;

    let path_str = out_path.to_string_lossy().to_string();
    let _ = app.emit("bili_progress", ProgressEvent {
        task_id: task_id.to_string(), downloaded: 0, total: 0,
        status: "done".to_string(), phase: String::new(), message: path_str.clone(),
    });
    Ok(path_str)
}

// ─── 音频 → MP3 ───────────────────────────────────────────────

async fn download_mp3(
    app: &tauri::AppHandle,
    task_id: &str,
    client: &Client,
    bvid: &str,
    cid: i64,
    base: &str,
    dir: &str,
) -> Result<String, String> {
    let ffmpeg = find_ffmpeg().ok_or("未找到 ffmpeg，请先安装: brew install ffmpeg")?;

    let resp: ApiResp<PlayData> = client
        .get("https://api.bilibili.com/x/player/playurl")
        .query(&[
            ("bvid", bvid), ("cid", &cid.to_string()),
            ("qn", "16"), ("fnval", "16"), ("fnver", "0"), ("fourk", "0"),
        ])
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    if resp.code != 0 { return Err(format!("B站API错误 {}: {}", resp.code, resp.message)); }
    let data = resp.data.ok_or("返回数据为空")?;
    let dash = data.dash.ok_or("未获取到DASH音频流")?;
    let audio_stream = dash.audio.iter().max_by_key(|a| a.bandwidth).ok_or("未找到音频流")?;

    let tmp = std::env::temp_dir().join(format!("bili_{}_a.m4s", task_id));
    let out_path = PathBuf::from(dir).join(format!("{}.mp3", base));
    if let Some(p) = out_path.parent() { tokio::fs::create_dir_all(p).await.map_err(|e| e.to_string())?; }

    {
        let ar = client.get(&audio_stream.base_url).send().await.map_err(|e| e.to_string())?;
        let total = ar.content_length().unwrap_or(0);
        let mut f = tokio::fs::File::create(&tmp).await.map_err(|e| e.to_string())?;
        let mut dl = 0u64;
        let mut stream = ar.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| e.to_string())?;
            f.write_all(&bytes).await.map_err(|e| e.to_string())?;
            dl += bytes.len() as u64;
            let _ = app.emit("bili_progress", ProgressEvent {
                task_id: task_id.to_string(), downloaded: dl, total,
                status: "downloading".to_string(), phase: "audio".to_string(), message: String::new(),
            });
        }
    }

    let _ = app.emit("bili_progress", ProgressEvent {
        task_id: task_id.to_string(), downloaded: 0, total: 0,
        status: "converting".to_string(), phase: String::new(), message: String::new(),
    });
    run_ffmpeg(&ffmpeg, &[
        "-i", tmp.to_str().unwrap(),
        "-acodec", "libmp3lame", "-q:a", "2", "-y",
        out_path.to_str().unwrap(),
    ]).await?;
    let _ = tokio::fs::remove_file(&tmp).await;

    let path_str = out_path.to_string_lossy().to_string();
    let _ = app.emit("bili_progress", ProgressEvent {
        task_id: task_id.to_string(), downloaded: 0, total: 0,
        status: "done".to_string(), phase: String::new(), message: path_str.clone(),
    });
    Ok(path_str)
}
