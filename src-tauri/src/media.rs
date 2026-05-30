use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::bili::find_ffmpeg;

// ─── Types ────────────────────────────────────────────────────

#[derive(Serialize, Clone, Default)]
pub struct MediaInfo {
    pub path: String,
    pub filename: String,
    pub duration_secs: f64,
    pub size_bytes: u64,
    pub video_codec: String,
    pub audio_codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: String,
    pub bitrate: u64,
    pub is_audio_only: bool,
}

// ffprobe JSON types
#[derive(Deserialize, Default)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    format: FfprobeFormat,
}

#[derive(Deserialize, Default)]
struct FfprobeStream {
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    r_frame_rate: String,
}

#[derive(Deserialize, Default)]
struct FfprobeFormat {
    #[serde(default)]
    duration: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    bit_rate: String,
}

// ─── Helpers ──────────────────────────────────────────────────

fn find_ffprobe() -> Option<String> {
    let candidates = [
        "/opt/homebrew/bin/ffprobe",
        "/usr/local/bin/ffprobe",
        "/usr/bin/ffprobe",
        "ffprobe",
    ];
    candidates.iter().find(|&&p| {
        std::process::Command::new(p)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }).map(|s| s.to_string())
}

async fn run_ffmpeg_async(ffmpeg: &str, args: &[&str]) -> Result<(), String> {
    let out = tokio::process::Command::new(ffmpeg)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("ffmpeg 启动失败: {}", e))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("ffmpeg 失败: {}", &msg[..msg.len().min(400)]));
    }
    Ok(())
}

fn parse_fps(r_frame_rate: &str) -> String {
    if r_frame_rate.is_empty() { return String::new(); }
    let parts: Vec<&str> = r_frame_rate.split('/').collect();
    if parts.len() == 2 {
        if let (Ok(n), Ok(d)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
            if d > 0.0 {
                let fps = n / d;
                return format!("{:.2}", fps).trim_end_matches('0').trim_end_matches('.').to_string();
            }
        }
    }
    r_frame_rate.to_string()
}

fn output_path_for(input: &str, suffix: &str, ext: &str) -> String {
    let p = Path::new(input);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let parent = p.parent().and_then(|d| d.to_str()).unwrap_or(".");
    format!("{}/{}{}.{}", parent, stem, suffix, ext)
}

fn file_ext(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

// ─── Commands ─────────────────────────────────────────────────

#[tauri::command]
pub async fn media_open_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("选择媒体文件")
        .add_filter(
            "媒体文件",
            &["mp4", "mkv", "avi", "mov", "flv", "webm", "mp3", "aac", "flac", "wav", "m4a", "ogg"],
        )
        .add_filter("所有文件", &["*"])
        .pick_file()
        .await
        .map(|h| h.path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn media_get_info(path: String) -> Result<MediaInfo, String> {
    let ffprobe = find_ffprobe()
        .ok_or("未找到 ffprobe，请先安装: brew install ffmpeg")?;

    let out = tokio::process::Command::new(&ffprobe)
        .args(&[
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
            &path,
        ])
        .output()
        .await
        .map_err(|e| format!("ffprobe 执行失败: {}", e))?;

    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("无法读取文件信息: {}", &msg[..msg.len().min(200)]));
    }

    let probe: FfprobeOutput = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("解析失败: {}", e))?;

    let mut info = MediaInfo::default();
    info.path = path.clone();
    info.filename = Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    info.duration_secs = probe.format.duration.parse::<f64>().unwrap_or(0.0);
    info.size_bytes = probe.format.size.parse::<u64>().unwrap_or(0);
    info.bitrate = probe.format.bit_rate.parse::<u64>().unwrap_or(0);

    for stream in &probe.streams {
        match stream.codec_type.as_str() {
            "video" => {
                info.video_codec = stream.codec_name.clone();
                info.width = stream.width;
                info.height = stream.height;
                info.fps = parse_fps(&stream.r_frame_rate);
            }
            "audio" => {
                if info.audio_codec.is_empty() {
                    info.audio_codec = stream.codec_name.clone();
                }
            }
            _ => {}
        }
    }

    info.is_audio_only = info.video_codec.is_empty();
    Ok(info)
}

/// operation: "trim" | "extract" | "convert"
#[tauri::command]
pub async fn media_process(
    path: String,
    operation: String,
    start_time: Option<String>,
    end_time: Option<String>,
    output_format: Option<String>,
) -> Result<String, String> {
    let ffmpeg = find_ffmpeg()
        .ok_or("未找到 ffmpeg，请先安装: brew install ffmpeg")?;

    let out_path = match operation.as_str() {
        "trim" => {
            let start = start_time.as_deref().unwrap_or("00:00:00");
            let end = end_time.as_deref().unwrap_or("");
            let ext = file_ext(&path);
            let out = output_path_for(&path, "_trimmed", &ext);
            let mut args: Vec<&str> = vec!["-i", &path, "-ss", start];
            if !end.is_empty() { args.extend_from_slice(&["-to", end]); }
            args.extend_from_slice(&["-c", "copy", "-avoid_negative_ts", "1", "-y", &out]);
            run_ffmpeg_async(&ffmpeg, &args).await?;
            out
        }
        "extract" => {
            let fmt = output_format.as_deref().unwrap_or("mp3");
            let out = output_path_for(&path, "", fmt);
            let args: Vec<&str> = match fmt {
                "aac"  => vec!["-i", &path, "-vn", "-c:a", "aac", "-y", &out],
                "flac" => vec!["-i", &path, "-vn", "-c:a", "flac", "-y", &out],
                "wav"  => vec!["-i", &path, "-vn", "-c:a", "pcm_s16le", "-y", &out],
                _      => vec!["-i", &path, "-vn", "-acodec", "libmp3lame", "-q:a", "2", "-y", &out],
            };
            run_ffmpeg_async(&ffmpeg, &args).await?;
            out
        }
        "convert" => {
            let fmt = output_format.as_deref().unwrap_or("mp4");
            let out = output_path_for(&path, "_converted", fmt);
            let args: Vec<&str> = match fmt {
                "mp3"  => vec!["-i", &path, "-vn", "-acodec", "libmp3lame", "-q:a", "2", "-y", &out],
                "aac"  => vec!["-i", &path, "-vn", "-c:a", "aac", "-y", &out],
                "flac" => vec!["-i", &path, "-vn", "-c:a", "flac", "-y", &out],
                "wav"  => vec!["-i", &path, "-vn", "-c:a", "pcm_s16le", "-y", &out],
                "mkv"  => vec!["-i", &path, "-c:v", "libx264", "-c:a", "aac", "-y", &out],
                _      => vec!["-i", &path, "-c:v", "libx264", "-c:a", "aac", "-y", &out],
            };
            run_ffmpeg_async(&ffmpeg, &args).await?;
            out
        }
        op => return Err(format!("未知操作: {}", op)),
    };

    Ok(out_path)
}
