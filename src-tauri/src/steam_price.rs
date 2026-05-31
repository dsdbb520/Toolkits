use futures_util::{stream, StreamExt};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::Duration;

// ─── 查询的区域列表（cc 代码 / 中文名 / 旗帜 emoji）─────────────────
// 选取常见的、价格差异有代表性的区域。Steam 用 cc 参数决定定价区。
const REGIONS: &[(&str, &str, &str)] = &[
    ("CN", "中国", "🇨🇳"),
    ("US", "美国", "🇺🇸"),
    ("AR", "阿根廷", "🇦🇷"),
    ("TR", "土耳其", "🇹🇷"),
    ("RU", "俄罗斯", "🇷🇺"),
    ("UA", "乌克兰", "🇺🇦"),
    ("KZ", "哈萨克斯坦", "🇰🇿"),
    ("IN", "印度", "🇮🇳"),
    ("BR", "巴西", "🇧🇷"),
    ("ID", "印尼", "🇮🇩"),
    ("VN", "越南", "🇻🇳"),
    ("HK", "香港", "🇭🇰"),
    ("TW", "台湾", "🇹🇼"),
    ("JP", "日本", "🇯🇵"),
    ("KR", "韩国", "🇰🇷"),
    ("SG", "新加坡", "🇸🇬"),
    ("MY", "马来西亚", "🇲🇾"),
    ("DE", "德国(欧元)", "🇩🇪"),
    ("GB", "英国", "🇬🇧"),
    ("PL", "波兰", "🇵🇱"),
    ("CA", "加拿大", "🇨🇦"),
    ("AU", "澳大利亚", "🇦🇺"),
    ("MX", "墨西哥", "🇲🇽"),
    ("ZA", "南非", "🇿🇦"),
];

// ─── 类型 ─────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct AppSearchResult {
    pub appid: u32,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct RegionPrice {
    pub cc: String,
    pub region_name: String,
    pub flag: String,
    /// "ok" 可购买 | "free" 免费 | "unavailable" 该区不可购买 | "comingsoon" 未发售 | "error" 查询失败
    pub status: String,
    pub currency: Option<String>,
    pub initial_formatted: Option<String>,
    pub final_formatted: Option<String>,
    pub discount_percent: i64,
    /// 换算成人民币的估算值（用于排序/对比），免费=0，不可购买=null
    pub final_cny: Option<f64>,
}

#[derive(Serialize)]
pub struct PriceQueryResult {
    pub appid: u32,
    pub name: String,
    pub header_image: Option<String>,
    pub regions: Vec<RegionPrice>,
    /// 汇率是否成功获取（false 时人民币估算列不可用）
    pub fx_available: bool,
}

// ─── 共享 HTTP 客户端（可按代理配置重建）──────────────────────────
// 全局复用一个 Client：连接池 + TLS 会话可复用，避免每次请求都重建连接。
// 这是多区域查询的主要提速点。代理模式：
//   "system" 跟随系统/环境变量代理（默认）
//   "none"   强制直连，忽略系统代理
//   "manual" 使用手动指定的 http(s) 代理

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

static CLIENT: RwLock<Option<reqwest::Client>> = RwLock::new(None);

#[derive(serde::Serialize)]
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
fn http() -> reqwest::Client {
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

// ─── 代理设置命令 ──────────────────────────────────────────────

#[tauri::command]
pub fn steam_price_get_proxy(state: tauri::State<crate::AppState>) -> ProxyConf {
    let db = state.db.lock().unwrap();
    ProxyConf {
        mode: crate::notes::get_setting(&db, "steam_proxy_mode")
            .unwrap_or_else(|| "system".to_string()),
        url: crate::notes::get_setting(&db, "steam_proxy_url").unwrap_or_default(),
    }
}

#[tauri::command]
pub fn steam_price_set_proxy(
    state: tauri::State<crate::AppState>,
    mode: String,
    url: String,
) -> Result<(), String> {
    let url = url.trim().to_string();
    if mode == "manual" {
        if url.is_empty() {
            return Err("请填写代理地址".to_string());
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("代理地址需以 http:// 或 https:// 开头（暂不支持 socks）".to_string());
        }
        if reqwest::Proxy::all(&url).is_err() {
            return Err("代理地址格式无效".to_string());
        }
    }
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        crate::notes::set_setting(&db, "steam_proxy_mode", &mode).map_err(|e| e.to_string())?;
        crate::notes::set_setting(&db, "steam_proxy_url", &url).map_err(|e| e.to_string())?;
    }
    apply_proxy(&mode, &url);
    Ok(())
}

/// Steam 商店封面图（给「链接/appid 直查」的结果配图，storesearch 自带 tiny_image）
fn app_capsule(appid: u32) -> Option<String> {
    Some(format!(
        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{}/capsule_231x87.jpg",
        appid
    ))
}

// ─── 搜索游戏（名称 → appid）────────────────────────────────────

/// 调一次 Steam storesearch（指定语言库与区），失败返回空
async fn storesearch(term: &str, lang: &str, cc: &str) -> Vec<AppSearchResult> {
    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&cc={}&l={}",
        urlencoding(term),
        cc,
        lang
    );
    let resp = match http().get(&url).send().await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let json: Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Some(items) = json.get("items").and_then(|v| v.as_array()) {
        for it in items {
            let appid = it.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if appid == 0 {
                continue;
            }
            let name = it.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let icon = it.get("tiny_image").and_then(|v| v.as_str()).map(String::from);
            out.push(AppSearchResult { appid, name, icon });
        }
    }
    out
}

#[tauri::command]
pub async fn steam_price_search(term: String) -> Result<Vec<AppSearchResult>, String> {
    let term = term.trim().to_string();
    if term.is_empty() {
        return Ok(Vec::new());
    }

    let mut results: Vec<AppSearchResult> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();

    // 1) 商店链接（…/app/123/…）→ 直接解析 appid，放最前
    if let Some(appid) = parse_url_appid(&term) {
        if let Some(name) = fetch_app_name(appid).await {
            if seen.insert(appid) {
                results.push(AppSearchResult { appid, name, icon: app_capsule(appid) });
            }
        }
    }

    // 2) 中文库 + 英文库 并行模糊搜索，合并去重（中文名优先 → 先放 cn）
    let (cn, en) = futures_util::future::join(
        storesearch(&term, "schinese", "cn"),
        storesearch(&term, "english", "us"),
    )
    .await;
    for r in cn.into_iter().chain(en.into_iter()) {
        if seen.insert(r.appid) {
            results.push(r);
        }
    }

    // 3) 纯数字兜底：作为 appid 直查放最后（让 "1245620" 这类直接 appid 也能命中，
    //    但不抢占关键词搜索——"2077" 仍优先显示《赛博朋克2077》）
    if let Ok(appid) = term.parse::<u32>() {
        if !seen.contains(&appid) {
            if let Some(name) = fetch_app_name(appid).await {
                seen.insert(appid);
                results.push(AppSearchResult { appid, name, icon: app_capsule(appid) });
            }
        }
    }

    Ok(results)
}

/// 只从商店链接（…/app/123456/…）解析 appid；纯数字不在此处理（交给关键词搜索）
fn parse_url_appid(input: &str) -> Option<u32> {
    let idx = input.find("/app/")?;
    let rest = &input[idx + 5..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

async fn fetch_app_name(appid: u32) -> Option<String> {
    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&cc=cn&l=schinese&filters=basic",
        appid
    );
    let resp = http().get(&url).send().await.ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get(&appid.to_string())?
        .get("data")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

// ─── 汇率（以 USD 为基准）──────────────────────────────────────

async fn fetch_fx_rates() -> Option<HashMap<String, f64>> {
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

/// 把以本币计价的人民币估算：price_human 为本币金额（如 9.99 美元、1980 日元）
fn to_cny(price_human: f64, currency: &str, rates: &HashMap<String, f64>) -> Option<f64> {
    let rate_c = rates.get(currency)?; // 1 USD = rate_c 本币
    let rate_cny = rates.get("CNY")?;
    if *rate_c <= 0.0 {
        return None;
    }
    let usd = price_human / rate_c;
    Some(usd * rate_cny)
}

// ─── 查询单个区域 ──────────────────────────────────────────────

async fn query_region(
    appid: u32,
    cc: &str,
    name: &str,
    flag: &str,
    rates: Option<&HashMap<String, f64>>,
) -> RegionPrice {
    let mut rp = RegionPrice {
        cc: cc.to_string(),
        region_name: name.to_string(),
        flag: flag.to_string(),
        status: "error".to_string(),
        currency: None,
        initial_formatted: None,
        final_formatted: None,
        discount_percent: 0,
        final_cny: None,
    };

    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&cc={}&l=schinese&filters=price_overview,basic",
        appid, cc
    );

    let resp = match http().get(&url).send().await {
        Ok(r) => r,
        Err(_) => return rp,
    };
    let json: Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return rp,
    };

    let entry = match json.get(&appid.to_string()) {
        Some(e) => e,
        None => return rp,
    };

    let success = entry.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    if !success {
        rp.status = "unavailable".to_string();
        return rp;
    }

    let data = match entry.get("data") {
        Some(d) => d,
        None => {
            rp.status = "unavailable".to_string();
            return rp;
        }
    };

    // 未发售
    if data
        .get("release_date")
        .and_then(|r| r.get("coming_soon"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        rp.status = "comingsoon".to_string();
        return rp;
    }

    // 免费游戏
    if data.get("is_free").and_then(|v| v.as_bool()).unwrap_or(false) {
        rp.status = "free".to_string();
        rp.final_cny = Some(0.0);
        return rp;
    }

    // 价格信息
    if let Some(po) = data.get("price_overview") {
        let currency = po.get("currency").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let final_minor = po.get("final").and_then(|v| v.as_i64()).unwrap_or(0);
        rp.status = "ok".to_string();
        rp.currency = Some(currency.clone());
        rp.initial_formatted = po
            .get("initial_formatted")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        rp.final_formatted = po
            .get("final_formatted")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        rp.discount_percent = po.get("discount_percent").and_then(|v| v.as_i64()).unwrap_or(0);

        // Steam 价格字段统一为 ×100，换算人民币用于对比
        if let Some(rates) = rates {
            let price_human = final_minor as f64 / 100.0;
            rp.final_cny = to_cny(price_human, &currency, rates);
        }
    } else {
        // success 但无价格、非免费 → 该区不上架/区域限制
        rp.status = "unavailable".to_string();
    }

    rp
}

// ─── 主查询命令 ────────────────────────────────────────────────

#[tauri::command]
pub async fn steam_price_query(appid: u32) -> Result<PriceQueryResult, String> {
    // 汇率与基础信息（名称、头图）并行获取，二者都不致命
    let (rates, (name, header_image)) =
        futures_util::future::join(fetch_fx_rates(), fetch_app_basic(appid)).await;
    let fx_available = rates.is_some();
    let rates_ref = rates.as_ref();

    // 并发查询各区域（并发 12，复用连接池；过高易触发 Steam 限流）
    let futures: Vec<_> = REGIONS
        .iter()
        .map(|&(cc, rname, flag)| query_region(appid, cc, rname, flag, rates_ref))
        .collect();
    let results: Vec<RegionPrice> = stream::iter(futures)
        .buffer_unordered(12)
        .collect()
        .await;

    // 按查询列表的固定顺序排序（buffer_unordered 会乱序）
    let order: HashMap<&str, usize> =
        REGIONS.iter().enumerate().map(|(i, (cc, _, _))| (*cc, i)).collect();
    let mut regions = results;
    regions.sort_by_key(|r| *order.get(r.cc.as_str()).unwrap_or(&usize::MAX));

    Ok(PriceQueryResult {
        appid,
        name: name.unwrap_or_else(|| format!("App {}", appid)),
        header_image,
        regions,
        fx_available,
    })
}

async fn fetch_app_basic(appid: u32) -> (Option<String>, Option<String>) {
    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&cc=cn&l=schinese&filters=basic",
        appid
    );
    let json: Value = match http().get(&url).send().await {
        Ok(r) => match r.json().await {
            Ok(j) => j,
            Err(_) => return (None, None),
        },
        Err(_) => return (None, None),
    };
    let data = json.get(&appid.to_string()).and_then(|e| e.get("data"));
    let name = data
        .and_then(|d| d.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let header = data
        .and_then(|d| d.get("header_image"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (name, header)
}

// ─── 简易 URL 编码（只处理查询词，避免引入额外依赖）────────────────

fn urlencoding(s: &str) -> String {
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
