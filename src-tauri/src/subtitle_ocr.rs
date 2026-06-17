// 硬字幕 OCR 自动对轴 → 导出外挂 SRT 字幕
//
// 架构：Rust 负责「调度」，真正的 OCR 在 Python sidecar 里完成。
//   - 选择视频 / 用 ffmpeg 抽一帧预览图供前端框选字幕区域
//   - 启动 Python sidecar，按行读取其 stdout 的 JSON 进度消息，转发成 Tauri 事件
//   - 支持取消（向任务发取消信号 → kill 子进程）
//
// OCR worker 在 Python 层，想替换/优化只需改 python/ 目录，Rust 侧不动。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;

use crate::bili::find_ffmpeg;
use crate::{notes, AppState};

// ─── 任务状态（用于取消） ──────────────────────────────────────

/// 记录每个进行中任务的取消信号发送端。`subtitle_ocr_cancel` 通过它通知任务结束。
#[derive(Default)]
pub struct OcrState {
    tasks: Mutex<HashMap<String, oneshot::Sender<()>>>,
}

// ─── 参数与消息类型 ───────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct SubtitleRegion {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// sidecar 配置：序列化成临时 JSON 文件传给 Python（避免命令行转义问题）。
#[derive(Serialize, Clone)]
struct SidecarConfig {
    video_path: String,
    output_path: String,
    subtitle_region: SubtitleRegion,
    sample_fps: f64,
    /// OCR 引擎："rapidocr" | "paddle"
    engine: String,
    /// PaddleOCR lang 代码：简体=ch，繁体=chinese_cht，通用=ch
    language: String,
    similarity_threshold: f64,
    min_duration: f64,
    max_gap: f64,
    min_confidence: f64,
}

/// 转发给前端的事件载荷。`kind`: progress | log | done | error | cancelled
#[derive(Serialize, Clone)]
struct OcrEvent {
    task_id: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_path: Option<String>,
}

/// Python stdout 每行的 JSON 结构。
#[derive(Deserialize)]
struct SidecarMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    current: Option<u64>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    output_path: Option<String>,
}

// ─── 小工具 ───────────────────────────────────────────────────

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// 取某个 Python 的版本字符串（如 "Python 3.12.4"）。
fn python_version(python: &str) -> Option<String> {
    let out = std::process::Command::new(python)
        .arg("--version")
        .output()
        .ok()?;
    // 老版本把版本写到 stderr，新版本写 stdout，两个都看
    let s = if !out.stdout.is_empty() {
        String::from_utf8_lossy(&out.stdout)
    } else {
        String::from_utf8_lossy(&out.stderr)
    };
    let v = s.trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// 解析 "Python 3.12.4" → (3, 12)。
fn parse_py_version(v: &str) -> Option<(u32, u32)> {
    let nums = v.trim_start_matches(|c: char| !c.is_ascii_digit());
    let mut it = nums.split('.');
    let major = it.next()?.parse().ok()?;
    let minor: u32 = it
        .next()?
        .trim_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    Some((major, minor))
}

/// PaddlePaddle 当前支持的范围：Python 3.8 ~ 3.13。
fn is_compatible(v: &str) -> bool {
    matches!(parse_py_version(v), Some((3, m)) if (8..=13).contains(&m))
}

/// 探测任意可用的 Python 解释器（不挑版本）。
fn find_python() -> Option<String> {
    ["python", "python3", "py"]
        .iter()
        .find(|&&p| python_version(p).is_some())
        .map(|s| s.to_string())
}

/// 通过 py 启动器问某个具体版本的真实可执行文件路径。
fn py_launcher_executable(ver: &str) -> Option<String> {
    let out = std::process::Command::new("py")
        .arg(format!("-{}", ver))
        .args(["-c", "import sys;print(sys.executable)"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() || !Path::new(&p).exists() {
        None
    } else {
        Some(p)
    }
}

/// 自动挑选一个「兼容」的 Python：优先用 py 启动器找 3.13~3.10，
/// 其次看 python/python3 是否兼容，最后退而求其次返回任意 python（可能不兼容，由 UI 提示）。
fn auto_python() -> Option<String> {
    for ver in ["3.13", "3.12", "3.11", "3.10"] {
        if let Some(p) = py_launcher_executable(ver) {
            return Some(p);
        }
    }
    for cand in ["python", "python3"] {
        if let Some(v) = python_version(cand) {
            if is_compatible(&v) {
                return Some(cand.to_string());
            }
        }
    }
    find_python()
}

/// 解析最终使用的 Python：用户在设置里手动指定的优先，否则自动挑选。
fn resolve_python(state: &tauri::State<AppState>) -> Option<String> {
    if let Ok(db) = state.db.lock() {
        if let Some(p) = notes::get_setting(&db, "subocr_python") {
            if !p.trim().is_empty() {
                return Some(p);
            }
        }
    }
    auto_python()
}

/// 解析 sidecar 所在的 python 目录。开发期用编译期注入的 manifest 目录；
/// 打包后优先用资源目录（后续接 tauri resources 时生效）。
fn sidecar_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("python"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("python"));
    candidates.into_iter().find(|p| p.join("main.py").exists())
}

fn sidecar_main_py(app: &tauri::AppHandle) -> Option<PathBuf> {
    sidecar_dir(app).map(|d| d.join("main.py"))
}

/// 简体/繁体/自动 → PaddleOCR lang 代码。
fn map_language(lang: &str) -> String {
    match lang {
        "cht" | "traditional" | "chinese_cht" => "chinese_cht".to_string(),
        _ => "ch".to_string(),
    }
}

fn default_output_path(video_path: &str) -> String {
    let p = Path::new(video_path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let parent = p.parent().and_then(|d| d.to_str()).unwrap_or(".");
    format!("{}/{}.srt", parent, stem)
}

// ─── Commands ─────────────────────────────────────────────────

#[tauri::command]
pub async fn subtitle_ocr_pick_video() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("选择带硬字幕的视频")
        .add_filter("视频文件", &["mp4", "mkv", "avi", "mov", "flv", "webm", "ts", "m4v"])
        .add_filter("所有文件", &["*"])
        .pick_file()
        .await
        .map(|h| h.path().to_string_lossy().to_string())
}

/// 环境自检：ffmpeg + python 是否就绪，sidecar 是否存在。
#[derive(Serialize)]
pub struct OcrEnv {
    ffmpeg: bool,
    python: Option<String>,
    sidecar: bool,
}

#[tauri::command]
pub fn subtitle_ocr_check_env(app: tauri::AppHandle, state: tauri::State<AppState>) -> OcrEnv {
    OcrEnv {
        ffmpeg: find_ffmpeg().is_some(),
        python: resolve_python(&state),
        sidecar: sidecar_main_py(&app).is_some(),
    }
}

// ─── Python 解释器配置 ────────────────────────────────────────

#[derive(Serialize)]
pub struct PythonInfo {
    /// 最终使用的解释器路径/命令
    path: Option<String>,
    /// 版本字符串，如 "Python 3.12.4"
    version: Option<String>,
    /// 是否兼容 PaddlePaddle（3.8~3.13）
    compatible: bool,
    /// 是否由用户手动指定（区别于自动探测）
    configured: bool,
}

fn make_python_info(path: Option<String>, configured: bool) -> PythonInfo {
    let version = path.as_deref().and_then(python_version);
    let compatible = version.as_deref().map(is_compatible).unwrap_or(false);
    PythonInfo { path, version, compatible, configured }
}

/// 返回当前生效的 Python 信息（含版本与兼容性）。
#[tauri::command]
pub fn subtitle_ocr_get_python(state: tauri::State<AppState>) -> PythonInfo {
    let configured = state
        .db
        .lock()
        .ok()
        .and_then(|db| notes::get_setting(&db, "subocr_python"))
        .filter(|p| !p.trim().is_empty());
    match configured {
        Some(p) => make_python_info(Some(p), true),
        None => make_python_info(auto_python(), false),
    }
}

/// 让用户手动指定 Python 解释器路径。校验可运行后存库。
#[tauri::command]
pub fn subtitle_ocr_set_python(
    state: tauri::State<AppState>,
    path: String,
) -> Result<PythonInfo, String> {
    let path = path.trim().to_string();
    // 空字符串 = 清除手动设置，回到自动探测
    if path.is_empty() {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        notes::set_setting(&db, "subocr_python", "").map_err(|e| e.to_string())?;
        drop(db);
        return Ok(make_python_info(auto_python(), false));
    }
    let version = python_version(&path)
        .ok_or("该路径无法作为 Python 运行，请确认选择的是 python 可执行文件")?;
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        notes::set_setting(&db, "subocr_python", &path).map_err(|e| e.to_string())?;
    }
    let compatible = is_compatible(&version);
    Ok(PythonInfo { path: Some(path), version: Some(version), compatible, configured: true })
}

/// 弹文件对话框选择 python 可执行文件。
#[tauri::command]
pub async fn subtitle_ocr_pick_python() -> Option<String> {
    let mut dlg = rfd::AsyncFileDialog::new().set_title("选择 Python 解释器（python.exe）");
    #[cfg(windows)]
    {
        dlg = dlg.add_filter("可执行文件", &["exe"]);
    }
    dlg.add_filter("所有文件", &["*"])
        .pick_file()
        .await
        .map(|h| h.path().to_string_lossy().to_string())
}

// ─── Python 依赖检测 / 一键安装 ───────────────────────────────

// 依赖按「公共」+「引擎专属」拆分：(import 名, pip 包名)
const CORE_DEPS: &[(&str, &str)] = &[
    ("cv2", "opencv-python"),
    ("numpy", "numpy"),
    ("rapidfuzz", "rapidfuzz"),
];
const PADDLE_DEPS: &[(&str, &str)] = &[
    ("paddleocr", "paddleocr"),
    ("paddle", "paddlepaddle"),
];
const RAPIDOCR_DEPS: &[(&str, &str)] = &[("rapidocr_onnxruntime", "rapidocr-onnxruntime")];

/// 按引擎返回需要的依赖（公共 + 引擎专属）。
fn deps_for(engine: &str) -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<(&'static str, &'static str)> = CORE_DEPS.to_vec();
    match engine {
        "rapidocr" => v.extend_from_slice(RAPIDOCR_DEPS),
        _ => v.extend_from_slice(PADDLE_DEPS),
    }
    v
}

#[derive(Serialize)]
pub struct DepItem {
    /// import 名（如 cv2）
    module: String,
    /// pip 包名（如 opencv-python）
    package: String,
    installed: bool,
}

#[derive(Serialize)]
pub struct DepStatus {
    python: Option<String>,
    python_version: Option<String>,
    /// Python 版本是否兼容 PaddlePaddle（3.8~3.13）
    compatible: bool,
    /// 是否为用户手动指定的解释器
    configured: bool,
    deps: Vec<DepItem>,
    all_ok: bool,
}

/// 转发给前端的安装日志事件。`kind`: log | done | error
#[derive(Serialize, Clone)]
struct DepEvent {
    kind: String,
    message: String,
}

/// 检测某引擎所需的 Python 依赖是否已安装（用 importlib.find_spec，不真正导入大模型，很快）。
#[tauri::command]
pub fn subtitle_ocr_check_deps(state: tauri::State<AppState>, engine: String) -> DepStatus {
    let info = subtitle_ocr_get_python(state);
    let required = deps_for(&engine);
    let none_deps = || -> Vec<DepItem> {
        required
            .iter()
            .map(|(m, p)| DepItem {
                module: m.to_string(),
                package: p.to_string(),
                installed: false,
            })
            .collect()
    };

    let python = match &info.path {
        Some(p) => p.clone(),
        None => {
            return DepStatus {
                python: None,
                python_version: None,
                compatible: false,
                configured: info.configured,
                deps: none_deps(),
                all_ok: false,
            };
        }
    };

    let mods_py = required
        .iter()
        .map(|(m, _)| format!("'{}'", m))
        .collect::<Vec<_>>()
        .join(",");
    let code = format!(
        "import json,importlib.util\n\
def c(m):\n    try:\n        return importlib.util.find_spec(m) is not None\n    except Exception:\n        return False\n\
mods=[{}]\n\
print(json.dumps({{m:c(m) for m in mods}}))",
        mods_py
    );

    let installed: HashMap<String, bool> = std::process::Command::new(&python)
        .args(["-c", &code])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_default();

    let deps: Vec<DepItem> = required
        .iter()
        .map(|(m, p)| DepItem {
            module: m.to_string(),
            package: p.to_string(),
            installed: *installed.get(*m).unwrap_or(&false),
        })
        .collect();
    let all_ok = deps.iter().all(|d| d.installed);

    DepStatus {
        python: info.path,
        python_version: info.version,
        compatible: info.compatible,
        configured: info.configured,
        deps,
        all_ok,
    }
}

/// 一键安装某引擎所需依赖：pip install <公共包 + 引擎包>，输出实时转成事件
/// `subtitle_ocr_dep_event`（kind: log/done/error）。`mirror` 可传国内镜像 index URL。
#[tauri::command]
pub async fn subtitle_ocr_install_deps(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    engine: String,
    mirror: Option<String>,
) -> Result<(), String> {
    let python = resolve_python(&state).ok_or("未找到 Python，请先安装 Python 3 并加入 PATH")?;
    let pkgs: Vec<&str> = deps_for(&engine).iter().map(|(_, p)| *p).collect();

    let emit_log = |app: &tauri::AppHandle, msg: String| {
        let _ = app.emit("subtitle_ocr_dep_event", DepEvent { kind: "log".into(), message: msg });
    };

    emit_log(&app, format!("使用 Python: {}", python));
    emit_log(&app, format!("安装依赖（引擎 {}）: {}", engine, pkgs.join(" ")));

    let mut cmd = tokio::process::Command::new(&python);
    cmd.args(["-m", "pip", "install", "--disable-pip-version-check"]);
    cmd.args(&pkgs);
    if let Some(m) = mirror.as_deref().filter(|s| !s.trim().is_empty()) {
        cmd.args(["-i", m]);
        emit_log(&app, format!("使用镜像: {}", m));
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("启动 pip 失败: {}", e))?;
    let stdout = child.stdout.take().ok_or("无法获取 pip stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取 pip stderr")?;

    // stderr 单独排空，避免管道写满阻塞
    {
        let app2 = app.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    let _ = app2.emit("subtitle_ocr_dep_event", DepEvent { kind: "log".into(), message: line });
                }
            }
        });
    }

    let mut reader = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        if !line.trim().is_empty() {
            emit_log(&app, line);
        }
    }

    let status = child.wait().await.map_err(|e| format!("等待 pip 失败: {}", e))?;
    if status.success() {
        let _ = app.emit("subtitle_ocr_dep_event", DepEvent { kind: "done".into(), message: "依赖安装完成".into() });
        Ok(())
    } else {
        let msg = format!("pip 退出码 {}，安装可能未完成", status.code().unwrap_or(-1));
        let _ = app.emit("subtitle_ocr_dep_event", DepEvent { kind: "error".into(), message: msg.clone() });
        Err(msg)
    }
}

/// 用 ffmpeg 在指定时间点抽一帧 PNG，返回临时图片路径（前端用 convertFileSrc 显示）。
#[tauri::command]
pub async fn subtitle_ocr_extract_frame(
    video_path: String,
    time_secs: f64,
) -> Result<String, String> {
    let ffmpeg = find_ffmpeg().ok_or("未找到 ffmpeg，请先安装并加入 PATH")?;
    let out = std::env::temp_dir().join(format!("subocr_preview_{}.png", now_ms()));
    let out_str = out.to_string_lossy().to_string();
    let ts = format!("{:.3}", time_secs.max(0.0));

    let status = tokio::process::Command::new(&ffmpeg)
        .args([
            "-ss", &ts,
            "-i", &video_path,
            "-frames:v", "1",
            "-q:v", "2",
            "-y", &out_str,
        ])
        .output()
        .await
        .map_err(|e| format!("ffmpeg 启动失败: {}", e))?;

    if !status.status.success() || !out.exists() {
        let msg = String::from_utf8_lossy(&status.stderr);
        return Err(format!("抽帧失败: {}", &msg[..msg.len().min(300)]));
    }
    Ok(out_str)
}

/// 读取生成的 SRT 文本，供前端预览（限制大小，避免超长字幕卡 UI）。
#[tauri::command]
pub fn subtitle_ocr_read_text(path: String) -> Result<String, String> {
    let data = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {}", e))?;
    const MAX: usize = 200 * 1024;
    if data.len() > MAX {
        Ok(format!("{}\n\n…（内容过长，已截断，请直接打开文件查看）", &data[..MAX]))
    } else {
        Ok(data)
    }
}

/// 取消进行中的任务。
#[tauri::command]
pub fn subtitle_ocr_cancel(state: tauri::State<OcrState>, task_id: String) -> Result<(), String> {
    let mut tasks = state.tasks.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = tasks.remove(&task_id) {
        let _ = tx.send(()); // 通知任务循环 → kill 子进程
    }
    Ok(())
}

/// 启动 OCR 任务：运行到结束（成功返回 SRT 路径）。过程中通过事件推送进度，
/// 同时把最终 done/error/cancelled 也作为事件发出，前端可统一依赖事件。
#[tauri::command]
pub async fn subtitle_ocr_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, OcrState>,
    app_state: tauri::State<'_, AppState>,
    task_id: String,
    video_path: String,
    output_path: Option<String>,
    subtitle_region: SubtitleRegion,
    sample_fps: Option<f64>,
    engine: Option<String>,
    language: String,
    similarity_threshold: Option<f64>,
    min_duration: Option<f64>,
    max_gap: Option<f64>,
    min_confidence: Option<f64>,
) -> Result<String, String> {
    let python = resolve_python(&app_state).ok_or("未找到 Python，请安装 Python 3 并加入 PATH")?;
    let main_py = sidecar_main_py(&app)
        .ok_or("未找到 OCR sidecar（python/main.py）")?;

    let out_path = output_path
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_output_path(&video_path));

    let config = SidecarConfig {
        video_path: video_path.clone(),
        output_path: out_path.clone(),
        subtitle_region,
        sample_fps: sample_fps.unwrap_or(4.0),
        engine: engine.unwrap_or_else(|| "rapidocr".to_string()),
        language: map_language(&language),
        similarity_threshold: similarity_threshold.unwrap_or(0.90),
        min_duration: min_duration.unwrap_or(0.4),
        max_gap: max_gap.unwrap_or(0.4),
        min_confidence: min_confidence.unwrap_or(0.6),
    };

    // 写配置到临时 JSON
    let cfg_path = std::env::temp_dir().join(format!("subocr_cfg_{}.json", task_id));
    let cfg_json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    tokio::fs::write(&cfg_path, cfg_json)
        .await
        .map_err(|e| format!("写配置失败: {}", e))?;

    // 启动 sidecar
    // -u：无缓冲，进度实时；-B：不写 __pycache__/.pyc，避免 dev 文件监视器误判重启
    let mut child = tokio::process::Command::new(&python)
        .arg("-B")
        .arg("-u")
        .arg(&main_py)
        .arg("--config")
        .arg(&cfg_path)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")          // 强制 UTF-8 模式
        .env("PYTHONIOENCODING", "utf-8") // stdout/stderr 用 UTF-8 编码
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 sidecar 失败: {}", e))?;

    let stdout = child.stdout.take().ok_or("无法获取 sidecar stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取 sidecar stderr")?;

    // 注册取消通道
    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    {
        let mut tasks = state.tasks.lock().map_err(|e| e.to_string())?;
        tasks.insert(task_id.clone(), cancel_tx);
    }

    // stderr → log 事件（独立任务，避免管道写满阻塞）。
    // 按字节读 + 有损解码：第三方库（paddle）可能输出非 UTF-8 字节，不能让它崩流。
    {
        let app2 = app.clone();
        let tid = task_id.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        let _ = app2.emit(
                            "subtitle_ocr_event",
                            OcrEvent {
                                task_id: tid.clone(),
                                kind: "log".into(),
                                current: None,
                                total: None,
                                message: Some(line),
                                output_path: None,
                            },
                        );
                    }
                }
            }
        });
    }

    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    let mut final_result: Result<String, String> = Err("sidecar 未返回结果".to_string());
    let mut cancelled = false;

    loop {
        buf.clear();
        tokio::select! {
            res = reader.read_until(b'\n', &mut buf) => {
                match res {
                    Ok(0) => break, // stdout 关闭，sidecar 结束
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf);
                        let trimmed = line.trim();
                        if trimmed.is_empty() { continue; }
                        // 每行应为一条 JSON 消息；非 JSON 当普通日志
                        match serde_json::from_str::<SidecarMessage>(trimmed) {
                            Ok(msg) => {
                                match msg.kind.as_str() {
                                    "done" => {
                                        final_result = Ok(msg.output_path.clone().unwrap_or_else(|| out_path.clone()));
                                    }
                                    "error" => {
                                        final_result = Err(msg.message.clone().unwrap_or_else(|| "OCR 失败".into()));
                                    }
                                    _ => {}
                                }
                                let _ = app.emit("subtitle_ocr_event", OcrEvent {
                                    task_id: task_id.clone(),
                                    kind: msg.kind,
                                    current: msg.current,
                                    total: msg.total,
                                    message: msg.message,
                                    output_path: msg.output_path,
                                });
                            }
                            Err(_) => {
                                let _ = app.emit("subtitle_ocr_event", OcrEvent {
                                    task_id: task_id.clone(),
                                    kind: "log".into(),
                                    current: None, total: None,
                                    message: Some(trimmed.to_string()),
                                    output_path: None,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        final_result = Err(format!("读取 sidecar 输出失败: {}", e));
                        break;
                    }
                }
            }
            _ = &mut cancel_rx => {
                let _ = child.start_kill();
                cancelled = true;
                break;
            }
        }
    }

    let _ = child.wait().await;
    let _ = tokio::fs::remove_file(&cfg_path).await;
    {
        if let Ok(mut tasks) = state.tasks.lock() {
            tasks.remove(&task_id);
        }
    }

    if cancelled {
        let _ = app.emit("subtitle_ocr_event", OcrEvent {
            task_id: task_id.clone(),
            kind: "cancelled".into(),
            current: None, total: None,
            message: Some("已取消".into()),
            output_path: None,
        });
        return Err("已取消".to_string());
    }

    // 若 sidecar 直接退出却没发 error/done，补一条 error 事件
    if let Err(ref e) = final_result {
        let _ = app.emit("subtitle_ocr_event", OcrEvent {
            task_id: task_id.clone(),
            kind: "error".into(),
            current: None, total: None,
            message: Some(e.clone()),
            output_path: None,
        });
    }

    final_result
}
