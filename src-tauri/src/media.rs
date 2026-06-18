use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

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

async fn run_ffmpeg_owned(ffmpeg: &str, args: &[String]) -> Result<(), String> {
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

fn clean_lyric_text(text: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut in_brace = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            '{' => in_brace = true,
            '}' if in_brace => in_brace = false,
            '\\' => {
                if matches!(chars.peek(), Some('N') | Some('n')) {
                    let _ = chars.next();
                    out.push(' ');
                } else if !in_tag && !in_brace {
                    out.push(ch);
                }
            }
            _ if !in_tag && !in_brace => out.push(ch),
            _ => {}
        }
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn timestamp_to_lrc(ts: &str) -> Option<String> {
    let normalized = ts.trim().replace(',', ".");
    let parts: Vec<&str> = normalized.split(':').collect();
    let (minutes, seconds) = match parts.as_slice() {
        [h, m, s] => {
            let h = h.trim().parse::<u32>().ok()?;
            let m = m.trim().parse::<u32>().ok()?;
            (h * 60 + m, *s)
        }
        [m, s] => {
            let m = m.trim().parse::<u32>().ok()?;
            (m, *s)
        }
        _ => return None,
    };

    let sec_parts: Vec<&str> = seconds.trim().split('.').collect();
    let sec = sec_parts.first()?.parse::<u32>().ok()?;
    let centis = sec_parts
        .get(1)
        .map(|frac| {
            let mut s = frac.chars().take(2).collect::<String>();
            while s.len() < 2 { s.push('0'); }
            s.parse::<u32>().unwrap_or(0)
        })
        .unwrap_or(0);

    Some(format!("[{:02}:{:02}.{:02}]", minutes, sec, centis.min(99)))
}

fn parse_timed_text(content: &str) -> String {
    let mut lines = Vec::new();
    let mut iter = content.lines().peekable();

    while let Some(line) = iter.next() {
        if !line.contains("-->") {
            continue;
        }

        let start = line.split("-->").next().unwrap_or("").trim();
        let Some(tag) = timestamp_to_lrc(start) else { continue };

        let mut text_lines = Vec::new();
        while let Some(next) = iter.peek() {
            let t = next.trim();
            if t.is_empty() || t.contains("-->") {
                break;
            }
            if t != "WEBVTT" && t.parse::<usize>().is_err() {
                let cleaned = clean_lyric_text(t);
                if !cleaned.is_empty() {
                    text_lines.push(cleaned);
                }
            }
            let _ = iter.next();
        }

        let text = text_lines.join(" ");
        if !text.is_empty() {
            lines.push(format!("{}{}", tag, text));
        }
    }

    lines.join("\n")
}

fn parse_ass_text(content: &str) -> String {
    let mut lines = Vec::new();
    for line in content.lines() {
        let Some(rest) = line.strip_prefix("Dialogue:") else { continue };
        let parts: Vec<&str> = rest.trim_start().splitn(10, ',').collect();
        if parts.len() < 10 {
            continue;
        }
        let Some(tag) = timestamp_to_lrc(parts[1]) else { continue };
        let text = clean_lyric_text(parts[9]);
        if !text.is_empty() {
            lines.push(format!("{}{}", tag, text));
        }
    }
    lines.join("\n")
}

fn subtitle_to_lyrics(path: &str) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("读取字幕失败: {}", e))?;
    let ext = file_ext(path);
    let lyrics = match ext.as_str() {
        "lrc" => content,
        "ass" | "ssa" => parse_ass_text(&content),
        "srt" | "vtt" => parse_timed_text(&content),
        _ => content
            .lines()
            .map(clean_lyric_text)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    };

    let lyrics = lyrics.trim().to_string();
    if lyrics.is_empty() {
        return Err("字幕文件里没有可写入歌词的文本".into());
    }
    Ok(lyrics)
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
pub async fn media_open_subtitle_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("选择字幕/歌词文件")
        .add_filter("字幕/歌词文件", &["srt", "ass", "ssa", "vtt", "lrc", "txt"])
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

#[tauri::command]
pub async fn media_embed_lyrics(
    audio_path: String,
    subtitle_path: String,
) -> Result<String, String> {
    let ffmpeg = find_ffmpeg()
        .ok_or("未找到 ffmpeg，请先安装: brew install ffmpeg")?;

    let audio_ext = file_ext(&audio_path);
    if !matches!(audio_ext.as_str(), "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg") {
        return Err("请选择音频文件（MP3/WAV/FLAC/AAC/M4A/OGG）".into());
    }

    let subtitle_ext = file_ext(&subtitle_path);
    if !matches!(subtitle_ext.as_str(), "srt" | "ass" | "ssa" | "vtt" | "lrc" | "txt") {
        return Err("请选择字幕/歌词文件（SRT/ASS/SSA/VTT/LRC/TXT）".into());
    }

    let lyrics = subtitle_to_lyrics(&subtitle_path)?;
    let out = output_path_for(&audio_path, "_with_lyrics", &audio_ext);
    let lrc_out = output_path_for(&audio_path, "_with_lyrics", "lrc");
    fs::write(&lrc_out, format!("{}\n", lyrics))
        .map_err(|e| format!("写同名 LRC 失败: {}", e))?;

    let mut args: Vec<String> = vec![
        "-i".into(), audio_path.clone(),
        "-map".into(), "0".into(),
        "-c".into(), "copy".into(),
    ];

    // 不同播放器认的字段差异很大：MP3 常见是 USLT/TXXX，FLAC/OGG 常见是 Vorbis comments。
    // ffmpeg 会把部分标准字段映射到容器对应的歌词帧；其余字段作为通用 tag 保留。
    for key in ["lyrics", "LYRICS", "Lyrics", "UNSYNCEDLYRICS", "unsyncedlyrics"] {
        args.push("-metadata".into());
        args.push(format!("{}={}", key, lyrics));
    }

    if audio_ext == "mp3" {
        args.extend(["-id3v2_version".into(), "3".into()]);
    }
    args.extend(["-y".into(), out.clone()]);

    run_ffmpeg_owned(&ffmpeg, &args).await?;
    Ok(out)
}
