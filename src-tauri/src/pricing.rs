// 跨平台价格查询的共享基础设施：HTTP 客户端 + 代理 + 汇率 + 通用结果类型。
// Steam / PlayStation / Nintendo Switch 三个工具共用，代理设置（DB 中的 steam_proxy_*）也全局共享。
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

// ─── 共享 HTTP 客户端（可按代理配置重建）──────────────────────────
// 全局复用一个 Client：连接池 + TLS 会话可复用。代理模式：
//   "system" 跟随系统/环境变量代理（默认）
//   "none"   强制直连，忽略系统代理
//   "manual" 使用手动指定的 http(s) 代理

static CLIENT: RwLock<Option<reqwest::Client>> = RwLock::new(None);

#[derive(Serialize)]
pub struct ProxyConf {
    pub mode: String,
    pub url: String,
}

fn build_client(mode: &str, url: &str) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(16);
    match mode {
        "none" => b = b.no_proxy(),
        "manual" => {
            if let Ok(p) = reqwest::Proxy::all(url) {
                b = b.proxy(p);
            }
        }
        _ => {} // system：保持默认（自动读取系统/环境变量代理）
    }
    b.build().unwrap_or_else(|_| reqwest::Client::new())
}

/// 按给定代理配置重建全局客户端（启动初始化 / 用户修改设置时调用）
pub fn apply_proxy(mode: &str, url: &str) {
    let c = build_client(mode, url);
    if let Ok(mut g) = CLIENT.write() {
        *g = Some(c);
    }
}

/// 取当前客户端（廉价 clone，内部 Arc，共享连接池）；未初始化则按 system 建一次
pub fn http() -> reqwest::Client {
    if let Ok(g) = CLIENT.read() {
        if let Some(c) = g.as_ref() {
            return c.clone();
        }
    }
    let c = build_client("system", "");
    if let Ok(mut g) = CLIENT.write() {
        *g = Some(c.clone());
    }
    c
}

// ─── 汇率（以 USD 为基准）──────────────────────────────────────

pub async fn fetch_fx_rates() -> Option<HashMap<String, f64>> {
    let resp = http()
        .get("https://open.er-api.com/v6/latest/USD")
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    let rates = json.get("rates")?.as_object()?;
    let mut map = HashMap::new();
    for (k, v) in rates {
        if let Some(f) = v.as_f64() {
            map.insert(k.clone(), f);
        }
    }
    if map.contains_key("CNY") {
        Some(map)
    } else {
        None
    }
}

/// 把以本币计价的金额估算成人民币：price_human 为本币金额（如 9.99 美元、1980 日元）
pub fn to_cny(price_human: f64, currency: &str, rates: &HashMap<String, f64>) -> Option<f64> {
    let rate_c = rates.get(currency)?; // 1 USD = rate_c 本币
    let rate_cny = rates.get("CNY")?;
    if *rate_c <= 0.0 {
        return None;
    }
    let usd = price_human / rate_c;
    Some(usd * rate_cny)
}

// ─── 简易 URL 编码（只处理查询词，避免引入额外依赖）────────────────

pub fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

