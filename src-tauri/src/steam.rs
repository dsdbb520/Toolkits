use serde::Serialize;
use std::path::{Path, PathBuf};

/// SteamID64 的基准值，account_id = steamid64 - BASE
const STEAMID64_BASE: u64 = 76561197960265728;

// ─── Types ────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct SteamAccount {
    pub steamid64: String,
    pub steamid32: String,
    pub account_name: String,
    pub persona_name: String,
    pub avatar: Option<String>,
    pub most_recent: bool,
    pub remember_password: bool,
    pub is_current: bool,
    pub timestamp: i64,
}

// ─── 极简 VDF 解析 ─────────────────────────────────────────────

enum VdfToken {
    Open,
    Close,
    Str(String),
}

enum VdfValue {
    Str(String),
    Obj(Vec<(String, VdfValue)>),
}

fn tokenize(input: &str) -> Vec<VdfToken> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '{' => {
                tokens.push(VdfToken::Open);
                chars.next();
            }
            '}' => {
                tokens.push(VdfToken::Close);
                chars.next();
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    match c {
                        '\\' => {
                            chars.next();
                            if let Some(&e) = chars.peek() {
                                s.push(match e {
                                    'n' => '\n',
                                    't' => '\t',
                                    'r' => '\r',
                                    other => other,
                                });
                                chars.next();
                            }
                        }
                        '"' => {
                            chars.next();
                            break;
                        }
                        _ => {
                            s.push(c);
                            chars.next();
                        }
                    }
                }
                tokens.push(VdfToken::Str(s));
            }
            '/' => {
                // 行注释 //
                chars.next();
                if chars.peek() == Some(&'/') {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    tokens
}

fn parse(tokens: &[VdfToken], pos: &mut usize) -> Vec<(String, VdfValue)> {
    let mut entries = Vec::new();
    while *pos < tokens.len() {
        match &tokens[*pos] {
            VdfToken::Close => {
                *pos += 1;
                break;
            }
            VdfToken::Open => {
                *pos += 1;
            }
            VdfToken::Str(key) => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(VdfToken::Open) => {
                        *pos += 1;
                        let obj = parse(tokens, pos);
                        entries.push((key.clone(), VdfValue::Obj(obj)));
                    }
                    Some(VdfToken::Str(val)) => {
                        *pos += 1;
                        entries.push((key.clone(), VdfValue::Str(val.clone())));
                    }
                    _ => break,
                }
            }
        }
    }
    entries
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn serialize(entries: &[(String, VdfValue)], indent: usize) -> String {
    let pad = "\t".repeat(indent);
    let mut out = String::new();
    for (k, v) in entries {
        match v {
            VdfValue::Str(s) => {
                out.push_str(&format!("{}\"{}\"\t\t\"{}\"\n", pad, escape(k), escape(s)));
            }
            VdfValue::Obj(o) => {
                out.push_str(&format!("{}\"{}\"\n{}{{\n", pad, escape(k), pad));
                out.push_str(&serialize(o, indent + 1));
                out.push_str(&format!("{}}}\n", pad));
            }
        }
    }
    out
}

fn get_str<'a>(fields: &'a [(String, VdfValue)], key: &str) -> Option<&'a str> {
    fields.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case(key) {
            if let VdfValue::Str(s) = v {
                return Some(s.as_str());
            }
        }
        None
    })
}

fn set_field(fields: &mut Vec<(String, VdfValue)>, key: &str, value: &str) {
    if let Some((_, v)) = fields.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
        *v = VdfValue::Str(value.to_string());
    } else {
        fields.push((key.to_string(), VdfValue::Str(value.to_string())));
    }
}

// ─── Windows 注册表 ────────────────────────────────────────────

#[cfg(windows)]
fn steam_install_path() -> Option<PathBuf> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(r"Software\Valve\Steam") {
        if let Ok(p) = key.get_value::<String, _>("SteamPath") {
            let pb = PathBuf::from(p.replace('/', "\\"));
            if pb.exists() {
                return Some(pb);
            }
        }
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for sub in [r"SOFTWARE\WOW6432Node\Valve\Steam", r"SOFTWARE\Valve\Steam"] {
        if let Ok(key) = hklm.open_subkey(sub) {
            if let Ok(p) = key.get_value::<String, _>("InstallPath") {
                let pb = PathBuf::from(p);
                if pb.exists() {
                    return Some(pb);
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn steam_exe_path(steam_path: &Path) -> PathBuf {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(r"Software\Valve\Steam") {
        if let Ok(p) = key.get_value::<String, _>("SteamExe") {
            let pb = PathBuf::from(p.replace('/', "\\"));
            if pb.exists() {
                return pb;
            }
        }
    }
    steam_path.join("steam.exe")
}

#[cfg(windows)]
fn current_autologin() -> Option<String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey(r"Software\Valve\Steam")
        .ok()?
        .get_value::<String, _>("AutoLoginUser")
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(windows)]
fn set_autologin(account_name: &str) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Valve\Steam")
        .map_err(|e| format!("写注册表失败: {}", e))?;
    key.set_value("AutoLoginUser", &account_name.to_string())
        .map_err(|e| format!("写 AutoLoginUser 失败: {}", e))?;
    key.set_value("RememberPassword", &1u32)
        .map_err(|e| format!("写 RememberPassword 失败: {}", e))?;
    Ok(())
}

// 非 Windows 平台的占位实现，保证可跨平台编译
#[cfg(not(windows))]
fn steam_install_path() -> Option<PathBuf> {
    None
}
#[cfg(not(windows))]
fn steam_exe_path(steam_path: &Path) -> PathBuf {
    steam_path.join("steam")
}
#[cfg(not(windows))]
fn current_autologin() -> Option<String> {
    None
}
#[cfg(not(windows))]
fn set_autologin(_account_name: &str) -> Result<(), String> {
    Err("仅支持 Windows".to_string())
}

// ─── 进程控制 ──────────────────────────────────────────────────

fn is_steam_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        if let Ok(out) = out {
            return String::from_utf8_lossy(&out.stdout).to_lowercase().contains("steam.exe");
        }
        false
    }
    #[cfg(not(windows))]
    {
        false
    }
}

// ─── 账号读取 ──────────────────────────────────────────────────

fn read_accounts(steam_path: &Path) -> Result<Vec<SteamAccount>, String> {
    let vdf_path = steam_path.join("config").join("loginusers.vdf");
    if !vdf_path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&vdf_path)
        .map_err(|e| format!("读取 loginusers.vdf 失败: {}", e))?;
    let tokens = tokenize(&content);
    let mut pos = 0;
    let root = parse(&tokens, &mut pos);

    let avatar_dir = steam_path.join("config").join("avatarcache");
    let autologin = current_autologin();

    let mut accounts = Vec::new();
    for (key, value) in &root {
        if !key.eq_ignore_ascii_case("users") {
            continue;
        }
        let VdfValue::Obj(users) = value else { continue };
        for (steamid64, uv) in users {
            let VdfValue::Obj(fields) = uv else { continue };
            let id64: u64 = steamid64.parse().unwrap_or(0);
            let id32 = id64.saturating_sub(STEAMID64_BASE);
            let account_name = get_str(fields, "AccountName").unwrap_or("").to_string();
            let persona_name = get_str(fields, "PersonaName").unwrap_or("").to_string();
            let most_recent = get_str(fields, "MostRecent") == Some("1");
            let remember_password = get_str(fields, "RememberPassword") == Some("1");
            let timestamp = get_str(fields, "Timestamp")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            let avatar = {
                let p = avatar_dir.join(format!("{}.png", steamid64));
                if p.exists() {
                    Some(p.to_string_lossy().to_string())
                } else {
                    None
                }
            };

            let is_current = autologin
                .as_deref()
                .map(|a| a.eq_ignore_ascii_case(&account_name))
                .unwrap_or(false);

            accounts.push(SteamAccount {
                steamid64: steamid64.clone(),
                steamid32: id32.to_string(),
                account_name,
                persona_name,
                avatar,
                most_recent,
                remember_password,
                is_current,
                timestamp,
            });
        }
    }

    // 最近登录的排前面
    accounts.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(accounts)
}

/// 把 loginusers.vdf 中目标账号的 MostRecent 置 1，其余置 0
fn set_most_recent(steam_path: &Path, account_name: &str) -> Result<(), String> {
    let vdf_path = steam_path.join("config").join("loginusers.vdf");
    let content = std::fs::read_to_string(&vdf_path)
        .map_err(|e| format!("读取 loginusers.vdf 失败: {}", e))?;
    let tokens = tokenize(&content);
    let mut pos = 0;
    let mut root = parse(&tokens, &mut pos);

    for (key, value) in root.iter_mut() {
        if !key.eq_ignore_ascii_case("users") {
            continue;
        }
        if let VdfValue::Obj(users) = value {
            for (_id, uv) in users.iter_mut() {
                if let VdfValue::Obj(fields) = uv {
                    let is_target = get_str(fields, "AccountName")
                        .map(|a| a.eq_ignore_ascii_case(account_name))
                        .unwrap_or(false);
                    set_field(fields, "MostRecent", if is_target { "1" } else { "0" });
                }
            }
        }
    }

    std::fs::write(&vdf_path, serialize(&root, 0))
        .map_err(|e| format!("写入 loginusers.vdf 失败: {}", e))?;
    Ok(())
}

// ─── Commands ─────────────────────────────────────────────────

#[tauri::command]
pub fn steam_get_path() -> Result<String, String> {
    steam_install_path()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "未找到 Steam 安装路径，请确认已安装 Steam".to_string())
}

#[tauri::command]
pub fn steam_get_accounts() -> Result<Vec<SteamAccount>, String> {
    let path = steam_install_path()
        .ok_or("未找到 Steam 安装路径，请确认已安装 Steam")?;
    read_accounts(&path)
}

#[tauri::command]
pub fn steam_is_running() -> bool {
    is_steam_running()
}

/// 返回该账号的 userdata 目录（不存在则回退到 userdata 根目录）
#[tauri::command]
pub fn steam_get_userdata_path(steamid32: String) -> Result<String, String> {
    let path = steam_install_path()
        .ok_or("未找到 Steam 安装路径")?;
    let userdata_root = path.join("userdata");
    let account_dir = userdata_root.join(&steamid32);
    if account_dir.exists() {
        Ok(account_dir.to_string_lossy().to_string())
    } else if userdata_root.exists() {
        Ok(userdata_root.to_string_lossy().to_string())
    } else {
        Err("未找到 userdata 文件夹".to_string())
    }
}

#[tauri::command]
pub async fn steam_switch_account(account_name: String) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("账号切换仅支持 Windows".to_string());
    }
    let steam_path = steam_install_path().ok_or("未找到 Steam 安装路径")?;
    let steam_exe = steam_exe_path(&steam_path);

    // 1) 写注册表 AutoLoginUser
    set_autologin(&account_name)?;
    // 2) 更新 loginusers.vdf 的 MostRecent（失败不阻断）
    let _ = set_most_recent(&steam_path, &account_name);

    // 3) 若 Steam 正在运行，优雅关闭
    if is_steam_running() {
        let _ = std::process::Command::new(&steam_exe)
            .arg("-shutdown")
            .spawn();
        // 最多等待 ~15 秒
        let mut closed = false;
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if !is_steam_running() {
                closed = true;
                break;
            }
        }
        // 仍未退出则强制结束
        if !closed {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/IM", "steam.exe"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    // 4) 重新启动 Steam（自动登录到目标账号）
    std::process::Command::new(&steam_exe)
        .spawn()
        .map_err(|e| format!("启动 Steam 失败: {}", e))?;

    Ok(())
}
