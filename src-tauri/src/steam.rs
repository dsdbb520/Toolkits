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

// ─── 进程控制（原生 Win32 API，避免 tasklist/taskkill 的进程启动开销）──

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForMultipleObjects, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE,
    };

    /// 需要一并结束的 Steam 相关进程。其中 steamwebhelper.exe 是新版 UI 的
    /// CEF 宿主，若残留会让重启的 steam.exe 等它退出（约 10 秒超时）。
    const STEAM_PROCESSES: [&str; 5] = [
        "steam.exe",
        "steamwebhelper.exe",
        "gameoverlayui.exe",
        "steamerrorreporter.exe",
        "steamservice.exe",
    ];

    fn wide_to_string(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    /// 枚举所有进程，返回 (pid, exe 名)
    fn snapshot() -> Vec<(u32, String)> {
        let mut procs = Vec::new();
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return procs;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    procs.push((entry.th32ProcessID, wide_to_string(&entry.szExeFile)));
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
        }
        procs
    }

    fn is_steam_process(name: &str) -> bool {
        STEAM_PROCESSES.iter().any(|n| name.eq_ignore_ascii_case(n))
    }

    pub fn is_running() -> bool {
        snapshot().iter().any(|(_, name)| name.eq_ignore_ascii_case("steam.exe"))
    }

    /// 强制结束 Steam 及其相关进程（steamwebhelper 等），并等待它们真正退出
    /// （所有句柄合计最多 wait_ms 毫秒）。
    ///
    /// 关键点：只对 `TerminateProcess` **真正成功** 的进程才加入等待，否则
    /// 像 steamservice.exe 这类「能拿到句柄但杀不掉」的进程会让等待一直
    /// 卡到满超时。用 WaitForMultipleObjects 一次性等全部，封顶总耗时。
    pub fn kill_and_wait(wait_ms: u32) {
        unsafe {
            let mut handles: Vec<HANDLE> = Vec::new();
            for (pid, name) in snapshot() {
                if !is_steam_process(&name) {
                    continue;
                }
                // 同时请求 TERMINATE 与 SYNCHRONIZE，便于结束后等待退出
                let h = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
                if h.is_null() {
                    continue;
                }
                if TerminateProcess(h, 1) != 0 {
                    handles.push(h); // 仅等待确实已结束的进程
                } else {
                    CloseHandle(h);
                }
            }
            if !handles.is_empty() {
                // bWaitAll = TRUE，等全部结束；总耗时封顶 wait_ms
                WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 1, wait_ms);
                for h in handles {
                    CloseHandle(h);
                }
            }
        }
    }

    /// 借助 explorer.exe 启动 Steam：我们 spawn 的 explorer 会立即把启动交给
    /// 桌面 shell 并退出，启动出的 Steam 与本进程完全解耦——不在同一 Job、
    /// 不继承句柄、不会触发 WaitForInputIdle/DDE 等同步等待。直接 CreateProcess
    /// 或 ShellExecuteW 拉起刚被强杀的同名大进程都实测阻塞 ~15s，此法可绕开。
    pub fn launch_detached(exe: &std::path::Path) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("explorer.exe")
            .arg(exe)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("启动 Steam 失败: {}", e))
    }
}

fn is_steam_running() -> bool {
    #[cfg(windows)]
    {
        win::is_running()
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

/// 直接启动 Steam（不切换账号，用于当前账号已正确但 Steam 未运行的情况）
#[tauri::command]
pub fn steam_launch() -> Result<(), String> {
    let steam_path = steam_install_path().ok_or("未找到 Steam 安装路径")?;
    let steam_exe = steam_exe_path(&steam_path);
    if is_steam_running() {
        return Ok(()); // 已在运行，无需重复启动
    }
    #[cfg(windows)]
    win::launch_detached(&steam_exe)?;
    #[cfg(not(windows))]
    std::process::Command::new(&steam_exe)
        .spawn()
        .map_err(|e| format!("启动 Steam 失败: {}", e))?;
    Ok(())
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

    // 先强杀再写配置，避免 Steam 退出时回写覆盖注册表和 vdf。
    #[cfg(windows)]
    win::kill_and_wait(3000);

    set_autologin(&account_name)?;
    let _ = set_most_recent(&steam_path, &account_name);

    // 经 explorer 解耦启动，避免 CreateProcess 同步阻塞。
    #[cfg(windows)]
    win::launch_detached(&steam_exe)?;
    #[cfg(not(windows))]
    std::process::Command::new(&steam_exe)
        .spawn()
        .map_err(|e| format!("启动 Steam 失败: {}", e))?;

    Ok(())
}
