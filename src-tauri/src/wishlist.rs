// 心愿单 / 降价追踪：手动粘贴 Steam / PlayStation / Nintendo Switch 商店链接添加游戏，
// 用确切的商品 ID 取价（不做模糊搜索），每次刷新记录一次价格快照，攒出历史曲线 + 历史最低价，
// 并支持目标价（低于目标时高亮，为后续降价提醒打基础）。
//
// 各平台取价方式：
//   Steam — appdetails 接口，appid + cc，官方、最干净。
//   NS    — api.ec.nintendo.com 按 nsuid + 区查价（官方）；标题/封面用欧服 Solr 按 nsuid 反查（欧/澳/新可得）。
//   PS    — 商品页 SSR 拿到 名称/封面/npTitleId，再用搜索页 SSR（含价格）按 npTitleId 精确匹配取价。

use crate::pricing::{fetch_fx_rates, http, to_cny};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ─── 建表 ──────────────────────────────────────────────────────

pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS wishlist_items (
            id TEXT PRIMARY KEY,
            platform TEXT NOT NULL,
            region TEXT NOT NULL,
            product_key TEXT NOT NULL,
            extra TEXT,
            title TEXT,
            image TEXT,
            store_url TEXT,
            target_cny REAL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT 0,
            deleted INTEGER NOT NULL DEFAULT 0,
            unseen_drop INTEGER NOT NULL DEFAULT 0,
            is_physical INTEGER NOT NULL DEFAULT 0,
            last_status TEXT,
            last_currency TEXT,
            last_final_formatted TEXT,
            last_initial_formatted TEXT,
            last_discount INTEGER,
            last_final_raw REAL,
            last_final_cny REAL,
            last_checked_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS price_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id TEXT NOT NULL,
            checked_at INTEGER NOT NULL,
            status TEXT,
            final_raw REAL,
            final_cny REAL,
            discount_percent INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_hist_item ON price_history(item_id);",
    )?;

    // 旧库迁移：补齐同步所需列
    for (col, decl) in [
        ("updated_at", "INTEGER NOT NULL DEFAULT 0"),
        ("deleted", "INTEGER NOT NULL DEFAULT 0"),
        ("unseen_drop", "INTEGER NOT NULL DEFAULT 0"),
        ("is_physical", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('wishlist_items') WHERE name=?1",
                params![col],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            conn.execute(&format!("ALTER TABLE wishlist_items ADD COLUMN {} {}", col, decl), [])?;
        }
    }
    // 给历史遗留行一个基于创建时间的 updated_at（毫秒），确保首次同步会上传
    conn.execute(
        "UPDATE wishlist_items SET updated_at = created_at*1000 WHERE updated_at = 0",
        [],
    )?;
    Ok(())
}

// ─── 类型 ──────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct PriceData {
    status: String, // ok | free | unavailable | error
    currency: Option<String>,
    final_formatted: Option<String>,
    initial_formatted: Option<String>,
    discount_percent: i64,
    final_raw: Option<f64>,
    final_cny: Option<f64>,
}

struct Resolved {
    platform: String,
    region: String,
    product_key: String,
    extra: String,
    title: String,
    image: Option<String>,
    store_url: String,
    price: PriceData,
}

#[derive(Serialize)]
struct HistPoint {
    t: i64,
    cny: Option<f64>,
}

#[derive(Serialize)]
pub struct WishItem {
    id: String,
    platform: String,
    region: String,
    product_key: String,
    title: String,
    image: Option<String>,
    store_url: String,
    target_cny: Option<f64>,
    created_at: i64,
    status: String,
    currency: Option<String>,
    final_formatted: Option<String>,
    initial_formatted: Option<String>,
    discount_percent: i64,
    final_cny: Option<f64>,
    checked_at: i64,
    /// 我们记录到的历史最低价（人民币）
    low_cny: Option<f64>,
    /// 历史快照（用于画走势）
    history: Vec<HistPoint>,
    /// 当前价 ≤ 目标价
    hit_target: bool,
    /// 自上次查看后降价了（未读红点）
    unseen_drop: bool,
    /// 自定义条目：实体卡带/游戏盘
    is_physical: bool,
}

// ─── 小工具 ────────────────────────────────────────────────────

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 毫秒时间戳（与笔记同步一致，用作 updated_at / 同步游标）
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 从价格文本取数值："HK$238.00"→238.0，"¥7,370"→7370.0，"無法使用"→None
fn parse_amount(s: &str) -> Option<f64> {
    let cleaned: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    cleaned.parse::<f64>().ok().filter(|v| *v > 0.0)
}

fn parse_percent(s: &str) -> i64 {
    s.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)
}

/// 区域 → 货币代码（PS 的价格文本不带货币代码，按区推断；也用于兜底）
fn currency_of(cc: &str) -> &'static str {
    match cc {
        "HK" => "HKD",
        "JP" => "JPY",
        "US" => "USD",
        "GB" => "GBP",
        "AU" => "AUD",
        "NZ" => "NZD",
        "CA" => "CAD",
        "TW" => "TWD",
        "CN" => "CNY",
        "AR" => "ARS",
        "TR" => "TRY",
        "RU" => "RUB",
        "BR" => "BRL",
        "IN" => "INR",
        "KR" => "KRW",
        "DE" | "FR" => "EUR",
        _ => "USD",
    }
}

fn ns_lang(cc: &str) -> &'static str {
    match cc {
        "HK" | "TW" => "zh",
        "JP" => "ja",
        _ => "en",
    }
}

/// PS 区域 cc → 商店 locale
fn ps_locale(cc: &str) -> String {
    match cc {
        "HK" => "zh-hant-hk",
        "TW" => "zh-hant-tw",
        "JP" => "ja-jp",
        "US" => "en-us",
        "GB" => "en-gb",
        "AU" => "en-au",
        "CA" => "en-ca",
        "DE" => "de-de",
        "FR" => "fr-fr",
        "AR" => "es-ar",
        "TR" => "tr-tr",
        _ => return format!("en-{}", cc.to_lowercase()),
    }
    .to_string()
}

fn extract_next_data(html: &str) -> Option<Value> {
    let mi = html.find("id=\"__NEXT_DATA__\"")?;
    let after = &html[mi..];
    let gt = after.find('>')?;
    let rest = &after[gt + 1..];
    let end = rest.find("</script>")?;
    serde_json::from_str(rest[..end].trim()).ok()
}

// ─── Steam ─────────────────────────────────────────────────────

async fn steam_fetch(
    appid: &str,
    cc: &str,
    rates: Option<&HashMap<String, f64>>,
) -> (PriceData, Option<String>, Option<String>) {
    let mut pd = PriceData {
        status: "error".to_string(),
        ..Default::default()
    };
    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&cc={}&l=schinese&filters=price_overview,basic",
        appid, cc
    );
    let json: Value = match http().get(&url).send().await {
        Ok(r) => match r.json().await {
            Ok(j) => j,
            Err(_) => return (pd, None, None),
        },
        Err(_) => return (pd, None, None),
    };
    let entry = match json.get(appid) {
        Some(e) => e,
        None => return (pd, None, None),
    };
    if !entry.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        pd.status = "unavailable".to_string();
        return (pd, None, None);
    }
    let data = match entry.get("data") {
        Some(d) => d,
        None => {
            pd.status = "unavailable".to_string();
            return (pd, None, None);
        }
    };
    let name = data.get("name").and_then(|v| v.as_str()).map(String::from);
    let image = data.get("header_image").and_then(|v| v.as_str()).map(String::from);

    if data.get("is_free").and_then(|v| v.as_bool()).unwrap_or(false) {
        pd.status = "free".to_string();
        pd.final_cny = Some(0.0);
        return (pd, name, image);
    }
    if let Some(po) = data.get("price_overview") {
        let currency = po.get("currency").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let final_minor = po.get("final").and_then(|v| v.as_i64()).unwrap_or(0);
        pd.status = "ok".to_string();
        pd.currency = Some(currency.clone());
        pd.final_formatted = po.get("final_formatted").and_then(|v| v.as_str()).map(String::from);
        pd.discount_percent = po.get("discount_percent").and_then(|v| v.as_i64()).unwrap_or(0);
        if pd.discount_percent > 0 {
            pd.initial_formatted =
                po.get("initial_formatted").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
        }
        let raw = final_minor as f64 / 100.0;
        pd.final_raw = Some(raw);
        pd.final_cny = rates.and_then(|r| to_cny(raw, &currency, r));
    } else {
        pd.status = "unavailable".to_string();
    }
    (pd, name, image)
}

// ─── Nintendo Switch ───────────────────────────────────────────

async fn ns_price(cc: &str, nsuid: &str, rates: Option<&HashMap<String, f64>>) -> PriceData {
    let mut pd = PriceData {
        status: "error".to_string(),
        ..Default::default()
    };
    let url = format!(
        "https://api.ec.nintendo.com/v1/price?country={}&lang={}&ids={}",
        cc,
        ns_lang(cc),
        nsuid
    );
    let json: Value = match http().get(&url).send().await {
        Ok(r) => match r.json().await {
            Ok(j) => j,
            Err(_) => return pd,
        },
        Err(_) => return pd,
    };
    let p = match json.get("prices").and_then(|v| v.as_array()).and_then(|a| a.first()) {
        Some(p) => p,
        None => return pd,
    };
    let status = p.get("sales_status").and_then(|v| v.as_str()).unwrap_or("");
    if status == "not_found" {
        pd.status = "unavailable".to_string();
        return pd;
    }
    let reg = match p.get("regular_price") {
        Some(r) => r,
        None => {
            pd.status = "unavailable".to_string();
            return pd;
        }
    };
    let currency = reg.get("currency").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let reg_amount = reg.get("amount").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let reg_raw: f64 = reg.get("raw_value").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    pd.currency = Some(currency.clone());

    if reg_raw <= 0.0 {
        pd.status = "free".to_string();
        pd.final_cny = Some(0.0);
        return pd;
    }
    if let Some(disc) = p.get("discount_price") {
        let d_amount = disc.get("amount").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let d_raw: f64 =
            disc.get("raw_value").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(reg_raw);
        pd.status = "ok".to_string();
        pd.initial_formatted = Some(reg_amount);
        pd.final_formatted = Some(d_amount);
        pd.discount_percent = if reg_raw > 0.0 { ((1.0 - d_raw / reg_raw) * 100.0).round() as i64 } else { 0 };
        pd.final_raw = Some(d_raw);
        pd.final_cny = rates.and_then(|r| to_cny(d_raw, &currency, r));
    } else {
        pd.status = "ok".to_string();
        pd.final_formatted = Some(reg_amount);
        pd.final_raw = Some(reg_raw);
        pd.final_cny = rates.and_then(|r| to_cny(reg_raw, &currency, r));
    }
    pd
}

/// 按 nsuid 反查标题/封面：先欧服 Solr（欧/澳/新可得），再港服清单（港区可得标题）。
/// 日/美区暂无公开的 nsuid→标题接口，取不到时由用户手动改名。
async fn ns_meta(nsuid: &str) -> (Option<String>, Option<String>) {
    // 1) 欧服 Solr（含封面）
    let url = format!(
        "https://searching.nintendo-europe.com/en/select?q=*&fq=nsuid_txt:%22{}%22&rows=1&wt=json",
        nsuid
    );
    if let Ok(resp) = http().get(&url).send().await {
        if let Ok(json) = resp.json::<Value>().await {
            let d = json.get("response").and_then(|r| r.get("docs")).and_then(|d| d.as_array()).and_then(|a| a.first());
            let title = d.and_then(|d| d.get("title")).and_then(|v| v.as_str()).map(String::from);
            if title.is_some() {
                let image = d
                    .and_then(|d| d.get("image_url_sq_s").or_else(|| d.get("wishlist_email_square_image_url_s")))
                    .and_then(|v| v.as_str())
                    .map(|s| if s.starts_with("http") { s.to_string() } else { format!("https:{}", s) });
                return (title, image);
            }
        }
    }
    // 2) 港服清单（仅标题）
    (hk_title(nsuid).await, None)
}

static HK_LIST: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();

/// 港服软件清单 nsuid→标题（进程内缓存一次）
async fn hk_title(nsuid: &str) -> Option<String> {
    if let Some(map) = HK_LIST.get() {
        return map.get(nsuid).cloned();
    }
    let arr: Vec<Value> = http()
        .get("https://www.nintendo.com.hk/data/json/switch_software.json")
        .send()
        .await
        .ok()?
        .json()
        .await
        .unwrap_or_default();
    let mut map = HashMap::new();
    for it in &arr {
        let ns = it.get("thumb_img").and_then(|v| v.as_str()).and_then(|s| s.split('.').next()).unwrap_or("");
        if ns.len() >= 13 && ns.chars().all(|c| c.is_ascii_digit()) {
            if let Some(t) = it.get("title").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                map.insert(ns.to_string(), t.to_string());
            }
        }
    }
    let map = HK_LIST.get_or_init(|| map);
    map.get(nsuid).cloned()
}

// ─── PlayStation ───────────────────────────────────────────────

/// 解析商品页：拿到 名称 / 封面（npTitleId 直接从商品 id 中段取，更可靠）
async fn ps_resolve_product(locale: &str, product_id: &str) -> Option<(String, Option<String>)> {
    let url = format!("https://store.playstation.com/{}/product/{}", locale, product_id);
    let html = http().get(&url).send().await.ok()?.text().await.ok()?;
    let data = extract_next_data(&html);
    let mut name = None;
    let mut image = None;
    if let Some(d) = &data {
        if let Some(apollo) = d.get("props").and_then(|p| p.get("apolloState")).and_then(|a| a.as_object()) {
            if let Some(prod) = apollo.values().find(|v| v.get("__typename").and_then(|t| t.as_str()) == Some("Product")) {
                name = prod.get("name").and_then(|v| v.as_str()).map(String::from);
                image = prod.get("media").and_then(|m| m.as_array()).and_then(|arr| {
                    arr.iter()
                        .find(|x| x.get("role").and_then(|r| r.as_str()) == Some("GAMEHUB_COVER_ART"))
                        .or_else(|| arr.iter().find(|x| x.get("type").and_then(|t| t.as_str()) == Some("IMAGE")))
                        .and_then(|x| x.get("url"))
                        .and_then(|u| u.as_str())
                        .map(String::from)
                });
            }
        }
    }
    // 名称兜底：<title>
    if name.is_none() {
        if let Some(start) = html.find("<title>") {
            if let Some(end) = html[start + 7..].find("</title>") {
                let t = html[start + 7..start + 7 + end].trim().to_string();
                if !t.is_empty() {
                    name = Some(t);
                }
            }
        }
    }
    Some((name?, image))
}

/// 精简标题用于搜索：去掉圆括号内的版本注释，去掉书名号等装饰括号字符（保留其中内容）
fn clean_title(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' | '（' => depth += 1,
            ')' | '）' => depth = (depth - 1).max(0),
            _ => {
                if depth == 0 && !matches!(ch, '《' | '》' | '「' | '」' | '【' | '】' | '〈' | '〉' | '[' | ']' | '™' | '®') {
                    out.push(ch);
                }
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 用搜索页（含价格）按完整商品 id 精确匹配取价。
/// 注意：npTitleId（如 PPSA13198_00）是「作品组」id，本体与各 DLC/组合包共用，会误配到便宜的 DLC；
/// 必须用完整商品 id（HP9000-PPSA13198_00-STELLARBLADE0000）唯一定位用户粘贴的那一款。
async fn ps_price(cc: &str, name: &str, product_id: &str, rates: Option<&HashMap<String, f64>>) -> PriceData {
    let mut pd = PriceData {
        status: "error".to_string(),
        ..Default::default()
    };
    let locale = ps_locale(cc);
    let term: String = crate::pricing::urlencoding(&clean_title(name));
    let url = format!("https://store.playstation.com/{}/search/{}", locale, term);
    let html = match http().get(&url).send().await {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(_) => return pd,
    };
    let apollo = match extract_next_data(&html).and_then(|d| d.get("props").and_then(|p| p.get("apolloState")).cloned()) {
        Some(a) => a,
        None => return pd,
    };
    let obj = match apollo.as_object() {
        Some(o) => o,
        None => return pd,
    };
    // 按完整商品 id 精确匹配；找不到则保持 unavailable（宁可「未取到」也不配错 SKU）
    let prod = obj.values().find(|v| {
        v.get("__typename").and_then(|t| t.as_str()) == Some("Product")
            && v.get("id").and_then(|n| n.as_str()) == Some(product_id)
    });
    let prod = match prod {
        Some(p) => p,
        None => {
            pd.status = "unavailable".to_string();
            return pd;
        }
    };
    let currency = currency_of(cc).to_string();
    pd.currency = Some(currency.clone());
    let price = prod.get("price");
    if price.and_then(|p| p.get("isFree")).and_then(|v| v.as_bool()).unwrap_or(false) {
        pd.status = "free".to_string();
        pd.final_cny = Some(0.0);
        return pd;
    }
    let disc = price.and_then(|p| p.get("discountedPrice")).and_then(|v| v.as_str());
    let base = price.and_then(|p| p.get("basePrice")).and_then(|v| v.as_str());
    let dt = price.and_then(|p| p.get("discountText")).and_then(|v| v.as_str());
    match disc.and_then(parse_amount) {
        Some(raw) => {
            pd.status = "ok".to_string();
            pd.final_formatted = disc.map(String::from);
            pd.discount_percent = dt.map(parse_percent).unwrap_or(0);
            if pd.discount_percent > 0 {
                pd.initial_formatted = base.map(String::from);
            }
            pd.final_raw = Some(raw);
            pd.final_cny = rates.and_then(|r| to_cny(raw, &currency, r));
        }
        None => pd.status = "unavailable".to_string(),
    }
    pd
}

// ─── 链接识别 + 解析 ───────────────────────────────────────────

/// 取 URL 里第一个连续数字串（≥min_len 位）
fn first_digits(s: &str, min_len: usize) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i - start >= min_len {
                return Some(s[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

/// 找 nsuid：以 700 开头的 13~14 位数字串（URL 或页面里）
fn find_nsuid(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let run = &s[start..i];
            if (13..=14).contains(&run.len()) && run.starts_with("700") {
                return Some(run.to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

/// 识别平台 + 区域 + 关键标识。region_override 非空时覆盖（主要给 Steam 用）
fn detect(input: &str, region_override: &str) -> Result<(String, String, String, String), String> {
    let s = input.trim();
    let lower = s.to_lowercase();

    // 纯数字 → Steam appid
    if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() {
        let cc = if region_override.is_empty() { "CN" } else { region_override };
        let url = format!("https://store.steampowered.com/app/{}/", s);
        return Ok(("steam".into(), cc.to_string(), s.to_string(), url));
    }

    if lower.contains("steampowered.com") || lower.contains("steamcommunity.com") {
        let appid = s
            .find("/app/")
            .and_then(|i| first_digits(&s[i + 5..], 1))
            .ok_or("无法从 Steam 链接解析 appid")?;
        let cc = if region_override.is_empty() { "CN" } else { region_override };
        return Ok(("steam".into(), cc.to_string(), appid, s.to_string()));
    }

    if lower.contains("playstation.com") {
        // locale 在 .com/ 后第一段；product id 在 /product/ 后
        let after_com = s.split("playstation.com/").nth(1).unwrap_or("");
        let locale = after_com.split('/').next().unwrap_or("").to_lowercase();
        let cc = locale.rsplit('-').next().unwrap_or("us").to_uppercase();
        let pid = s
            .split("/product/")
            .nth(1)
            .map(|r| r.split(['/', '?', '#']).next().unwrap_or("").to_string())
            .filter(|p| !p.is_empty())
            .ok_or("PS 链接需为商品页（含 /product/…）")?;
        let cc = if region_override.is_empty() { cc } else { region_override.to_string() };
        return Ok(("ps".into(), cc, pid, s.to_string()));
    }

    if lower.contains("nintendo") {
        // nsuid 可能在 URL 里；slug 链接（如 nintendo.com/us/store/products/…）则留空，由 resolve 抓页面补
        let nsuid = find_nsuid(s).unwrap_or_default();
        // 区域：域名/路径里推断
        let cc = if !region_override.is_empty() {
            region_override.to_string()
        } else if lower.contains(".com.hk") || lower.contains("/hk/") {
            "HK".into()
        } else if lower.contains("store-jp") || lower.contains(".co.jp") || lower.contains("/jp/") {
            "JP".into()
        } else if lower.contains("/us/") || lower.contains(".com/us") {
            "US".into()
        } else if lower.contains(".com.tw") || lower.contains("/tw/") {
            "TW".into()
        } else {
            // ec.nintendo.com/{REGION}/...
            s.split("ec.nintendo.com/")
                .nth(1)
                .and_then(|r| r.split('/').next())
                .map(|c| c.to_uppercase())
                .filter(|c| c.len() == 2)
                .unwrap_or_else(|| "US".into())
        };
        return Ok(("ns".into(), cc, nsuid, s.to_string()));
    }

    Err("无法识别链接，请粘贴 Steam / PlayStation / Nintendo 商店链接（或 Steam appid）".into())
}

async fn resolve(input: &str, region_override: &str) -> Result<Resolved, String> {
    let (platform, cc, key, store_url) = detect(input, region_override)?;
    let rates = fetch_fx_rates().await;
    let rates_ref = rates.as_ref();

    match platform.as_str() {
        "steam" => {
            let (price, name, image) = steam_fetch(&key, &cc, rates_ref).await;
            if name.is_none() && price.status == "error" {
                return Err("查询 Steam 失败，请检查网络/代理或 appid".into());
            }
            Ok(Resolved {
                platform,
                region: cc,
                product_key: key,
                extra: String::new(),
                title: name.unwrap_or_else(|| "未知游戏".into()),
                image,
                store_url,
                price,
            })
        }
        "ns" => {
            // slug 链接 URL 里没有 nsuid → 抓页面提取
            let mut nsuid = key.clone();
            if nsuid.is_empty() {
                if let Ok(resp) = http().get(&store_url).send().await {
                    if let Ok(txt) = resp.text().await {
                        nsuid = find_nsuid(&txt).unwrap_or_default();
                    }
                }
            }
            if nsuid.is_empty() {
                return Err("无法从该 NS 链接获取 nsuid，请用 eShop 商品页链接".into());
            }
            let (price, meta) = futures_util::future::join(ns_price(&cc, &nsuid, rates_ref), ns_meta(&nsuid)).await;
            let (title, image) = meta;
            Ok(Resolved {
                platform,
                region: cc,
                product_key: nsuid.clone(),
                extra: String::new(),
                title: title.unwrap_or_else(|| format!("Switch 游戏 {}", nsuid)),
                image,
                store_url,
                price,
            })
        }
        "ps" => {
            let locale = ps_locale(&cc);
            let (name, image) = ps_resolve_product(&locale, &key)
                .await
                .ok_or("解析 PS 商品页失败（链接需为商品页，且可能需要代理）")?;
            // 按完整商品 id 取价（key 即完整 id）
            let price = ps_price(&cc, &name, &key, rates_ref).await;
            Ok(Resolved {
                platform,
                region: cc,
                product_key: key,
                extra: String::new(),
                title: name,
                image,
                store_url,
                price,
            })
        }
        _ => Err("未知平台".into()),
    }
}

/// 刷新时按已存信息重新取价
async fn refetch(
    platform: &str,
    cc: &str,
    key: &str,
    title: &str,
    rates: Option<&HashMap<String, f64>>,
) -> PriceData {
    match platform {
        "steam" => steam_fetch(key, cc, rates).await.0,
        "ns" => ns_price(cc, key, rates).await,
        "ps" => ps_price(cc, title, key, rates).await,
        _ => PriceData {
            status: "error".to_string(),
            ..Default::default()
        },
    }
}

// ─── DB 读写 ───────────────────────────────────────────────────

fn row_to_item(conn: &Connection, row: &rusqlite::Row) -> rusqlite::Result<WishItem> {
    let id: String = row.get("id")?;
    let target_cny: Option<f64> = row.get("target_cny")?;
    let final_cny: Option<f64> = row.get("last_final_cny")?;

    // 历史 + 历史最低（仅在另一处批量取以省查询，这里逐条取，量很小）
    let mut history = Vec::new();
    let mut low: Option<f64> = None;
    {
        let mut stmt = conn.prepare(
            "SELECT checked_at, final_cny, status FROM price_history WHERE item_id=?1 ORDER BY checked_at ASC",
        )?;
        let rows = stmt.query_map(params![id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<f64>>(1)?, r.get::<_, Option<String>>(2)?))
        })?;
        for r in rows {
            let (t, cny, status) = r?;
            if matches!(status.as_deref(), Some("ok") | Some("free")) {
                if let Some(c) = cny {
                    low = Some(low.map_or(c, |l: f64| l.min(c)));
                }
            }
            history.push(HistPoint { t, cny });
        }
    }

    let hit_target = match (final_cny, target_cny) {
        (Some(f), Some(t)) => f <= t,
        _ => false,
    };

    Ok(WishItem {
        id,
        platform: row.get("platform")?,
        region: row.get("region")?,
        product_key: row.get("product_key")?,
        title: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
        image: row.get("image")?,
        store_url: row.get::<_, Option<String>>("store_url")?.unwrap_or_default(),
        target_cny,
        created_at: row.get("created_at")?,
        status: row.get::<_, Option<String>>("last_status")?.unwrap_or_else(|| "error".into()),
        currency: row.get("last_currency")?,
        final_formatted: row.get("last_final_formatted")?,
        initial_formatted: row.get("last_initial_formatted")?,
        discount_percent: row.get::<_, Option<i64>>("last_discount")?.unwrap_or(0),
        final_cny,
        checked_at: row.get::<_, Option<i64>>("last_checked_at")?.unwrap_or(0),
        low_cny: low,
        history,
        hit_target,
        unseen_drop: row.get::<_, Option<i64>>("unseen_drop")?.unwrap_or(0) != 0,
        is_physical: row.get::<_, Option<i64>>("is_physical")?.unwrap_or(0) != 0,
    })
}

fn load_items(conn: &Connection) -> rusqlite::Result<Vec<WishItem>> {
    let mut stmt =
        conn.prepare("SELECT id FROM wishlist_items WHERE deleted=0 ORDER BY created_at DESC")?;
    let ids: Vec<String> = stmt.query_map([], |r| r.get::<_, String>("id"))?.collect::<Result<_, _>>()?;
    // 重新查每条（row_to_item 需要再用 conn 查历史，避免借用冲突）
    let mut out = Vec::new();
    for id in ids {
        let item = conn.query_row("SELECT * FROM wishlist_items WHERE id=?1", params![id], |row| {
            row_to_item(conn, row)
        })?;
        out.push(item);
    }
    Ok(out)
}

/// 写一条价格快照并更新缓存最新价。价格/状态变化时 bump updated_at（驱动同步）；
/// 若较上次缓存价下降（可购买/免费），置 unseen_drop（红点）。
fn record(conn: &Connection, item_id: &str, pd: &PriceData, ts: i64) -> rusqlite::Result<()> {
    // 当前缓存价（用于降价判断与“是否变化”）
    let (prev_cny, prev_status): (Option<f64>, Option<String>) = conn
        .query_row(
            "SELECT last_final_cny, last_status FROM wishlist_items WHERE id=?1",
            params![item_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap_or((None, None));

    // 历史追加：与最近一条不同或超过 12h
    let last_hist: Option<(Option<f64>, String, i64)> = conn
        .query_row(
            "SELECT final_cny, status, checked_at FROM price_history WHERE item_id=?1 ORDER BY checked_at DESC LIMIT 1",
            params![item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let append = match &last_hist {
        Some((cny, status, at)) => *status != pd.status || *cny != pd.final_cny || (ts - at) > 12 * 3600,
        None => true,
    };
    if append {
        conn.execute(
            "INSERT INTO price_history (item_id, checked_at, status, final_raw, final_cny, discount_percent)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![item_id, ts, pd.status, pd.final_raw, pd.final_cny, pd.discount_percent],
        )?;
    }

    let buyable = |s: &str| s == "ok" || s == "free";
    let dropped = buyable(&pd.status)
        && prev_status.as_deref().map(buyable).unwrap_or(false)
        && matches!((pd.final_cny, prev_cny), (Some(n), Some(o)) if n < o - 0.001);
    let price_changed = prev_status.as_deref() != Some(pd.status.as_str()) || prev_cny != pd.final_cny;

    conn.execute(
        "UPDATE wishlist_items SET last_status=?2, last_currency=?3, last_final_formatted=?4,
            last_initial_formatted=?5, last_discount=?6, last_final_raw=?7, last_final_cny=?8, last_checked_at=?9,
            updated_at = CASE WHEN ?10<>0 THEN ?11 ELSE updated_at END,
            unseen_drop = CASE WHEN ?12<>0 THEN 1 ELSE unseen_drop END
         WHERE id=?1",
        params![
            item_id, pd.status, pd.currency, pd.final_formatted, pd.initial_formatted,
            pd.discount_percent, pd.final_raw, pd.final_cny, ts,
            price_changed as i64, now_ms(), dropped as i64
        ],
    )?;
    Ok(())
}

// ─── Tauri 命令 ────────────────────────────────────────────────

#[tauri::command]
pub async fn wishlist_add(
    state: tauri::State<'_, crate::AppState>,
    input: String,
    region: String,
    target_cny: Option<f64>,
) -> Result<WishItem, String> {
    let r = resolve(&input, region.trim()).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let ts = now();
    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        // 去重：同 平台+区域+商品 已存在（未删除）则直接返回它，避免重复添加
        if let Ok(existing) = conn.query_row(
            "SELECT id FROM wishlist_items WHERE platform=?1 AND region=?2 AND product_key=?3 AND deleted=0",
            params![r.platform, r.region, r.product_key],
            |row| row.get::<_, String>(0),
        ) {
            return conn
                .query_row("SELECT * FROM wishlist_items WHERE id=?1", params![existing], |row| row_to_item(&conn, row))
                .map_err(|e| e.to_string());
        }
        conn.execute(
            "INSERT INTO wishlist_items
                (id, platform, region, product_key, extra, title, image, store_url, target_cny, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id, r.platform, r.region, r.product_key, r.extra, r.title, r.image, r.store_url, target_cny, ts, now_ms()
            ],
        )
        .map_err(|e| e.to_string())?;
        record(&conn, &id, &r.price, ts).map_err(|e| e.to_string())?;
        conn.query_row("SELECT * FROM wishlist_items WHERE id=?1", params![id], |row| row_to_item(&conn, row))
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn wishlist_list(state: tauri::State<'_, crate::AppState>) -> Result<Vec<WishItem>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    load_items(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn wishlist_remove(state: tauri::State<'_, crate::AppState>, id: String) -> Result<(), String> {
    // 软删除：保留行并 bump updated_at，让删除能跨设备同步
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE wishlist_items SET deleted=1, unseen_drop=0, updated_at=?2 WHERE id=?1",
        params![id, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn wishlist_set_target(
    state: tauri::State<'_, crate::AppState>,
    id: String,
    target_cny: Option<f64>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE wishlist_items SET target_cny=?2, updated_at=?3 WHERE id=?1",
        params![id, target_cny, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 手动改名（自动识别不到标题的区域，如日/港/美 NS，由用户自行命名）
#[tauri::command]
pub fn wishlist_set_title(
    state: tauri::State<'_, crate::AppState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("名称不能为空".into());
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE wishlist_items SET title=?2, updated_at=?3 WHERE id=?1",
        params![id, title, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ─── 自定义条目（手动维护价格，不自动取价）──────────────────────

fn fmt_cny(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("¥{}", v as i64)
    } else {
        format!("¥{:.2}", v)
    }
}

/// 设置自定义条目的现价/原价（写 last_* 并记一条历史）
fn set_custom_price(
    conn: &Connection,
    id: &str,
    cur: Option<f64>,
    orig: Option<f64>,
    ts: i64,
) -> rusqlite::Result<()> {
    match cur {
        Some(c) if c > 0.0 => {
            let (initial, discount) = match orig {
                Some(o) if o > c => (Some(fmt_cny(o)), ((1.0 - c / o) * 100.0).round() as i64),
                _ => (None, 0),
            };
            conn.execute(
                "UPDATE wishlist_items SET last_status='ok', last_currency='CNY', last_final_formatted=?2,
                    last_initial_formatted=?3, last_discount=?4, last_final_raw=?5, last_final_cny=?5, last_checked_at=?6
                 WHERE id=?1",
                params![id, fmt_cny(c), initial, discount, c, ts],
            )?;
            conn.execute(
                "INSERT INTO price_history (item_id, checked_at, status, final_raw, final_cny, discount_percent)
                 VALUES (?1,?2,'ok',?3,?3,?4)",
                params![id, ts, c, discount],
            )?;
        }
        _ => {
            conn.execute(
                "UPDATE wishlist_items SET last_status='unavailable', last_final_cny=NULL,
                    last_final_formatted=NULL, last_initial_formatted=NULL, last_discount=0, last_checked_at=?2
                 WHERE id=?1",
                params![id, ts],
            )?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn wishlist_add_custom(
    state: tauri::State<'_, crate::AppState>,
    title: String,
    region: String,
    is_physical: bool,
    cur_cny: Option<f64>,
    orig_cny: Option<f64>,
    target_cny: Option<f64>,
) -> Result<WishItem, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("游戏名不能为空".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let ts = now();
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO wishlist_items
            (id, platform, region, product_key, title, target_cny, created_at, updated_at, is_physical)
         VALUES (?1,'custom',?2,'',?3,?4,?5,?6,?7)",
        params![id, region.trim(), title, target_cny, ts, now_ms(), is_physical as i64],
    )
    .map_err(|e| e.to_string())?;
    set_custom_price(&conn, &id, cur_cny, orig_cny, ts).map_err(|e| e.to_string())?;
    conn.query_row("SELECT * FROM wishlist_items WHERE id=?1", params![id], |row| row_to_item(&conn, row))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn wishlist_update_custom(
    state: tauri::State<'_, crate::AppState>,
    id: String,
    title: String,
    region: String,
    is_physical: bool,
    cur_cny: Option<f64>,
    orig_cny: Option<f64>,
) -> Result<WishItem, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("游戏名不能为空".into());
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE wishlist_items SET title=?2, region=?3, is_physical=?4, updated_at=?5
         WHERE id=?1 AND platform='custom'",
        params![id, title, region.trim(), is_physical as i64, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    set_custom_price(&conn, &id, cur_cny, orig_cny, now()).map_err(|e| e.to_string())?;
    conn.query_row("SELECT * FROM wishlist_items WHERE id=?1", params![id], |row| row_to_item(&conn, row))
        .map_err(|e| e.to_string())
}

/// 重新拉取所有未删除条目的当前价并写回（含降价标记）。不持锁跨 await。
async fn refetch_all(state: &tauri::State<'_, crate::AppState>) -> Result<(), String> {
    let rows: Vec<(String, String, String, String, String)> = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, platform, region, product_key, title FROM wishlist_items
                 WHERE deleted=0 AND platform != 'custom'",
            )
            .map_err(|e| e.to_string())?;
        let it = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                ))
            })
            .map_err(|e| e.to_string())?;
        it.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };

    let rates = fetch_fx_rates().await;
    let rates_ref = rates.as_ref();
    let ts = now();
    let mut results = Vec::new();
    for (id, platform, cc, key, title) in &rows {
        let pd = refetch(platform, cc, key, title, rates_ref).await;
        results.push((id.clone(), pd));
    }

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    for (id, pd) in &results {
        record(&conn, id, pd, ts).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn wishlist_refresh(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<WishItem>, String> {
    refetch_all(&state).await?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    load_items(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn wishlist_unseen_count(state: tauri::State<'_, crate::AppState>) -> Result<i64, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM wishlist_items WHERE deleted=0 AND unseen_drop=1", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn wishlist_mark_seen(state: tauri::State<'_, crate::AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute("UPDATE wishlist_items SET unseen_drop=0 WHERE unseen_drop=1", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ─── 服务器同步 ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct WireItem {
    id: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    product_key: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    store_url: Option<String>,
    #[serde(default)]
    target_cny: Option<f64>,
    #[serde(default)]
    created_at: i64,
    updated_at: i64,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    is_physical: bool,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    currency: Option<String>,
    #[serde(default)]
    final_formatted: Option<String>,
    #[serde(default)]
    initial_formatted: Option<String>,
    #[serde(default)]
    discount_percent: i64,
    #[serde(default)]
    final_cny: Option<f64>,
    #[serde(default)]
    checked_at: i64,
    #[serde(default)]
    low_cny: Option<f64>,
}

#[derive(Serialize)]
struct WireReq {
    last_sync_at: i64,
    items: Vec<WireItem>,
}

#[derive(Deserialize)]
struct WireResp {
    items: Vec<WireItem>,
    synced_at: i64,
}

/// 收集本地自 since 之后变更的条目（含已删除），带当前价快照
fn gather_changed(conn: &Connection, since: i64) -> rusqlite::Result<Vec<WireItem>> {
    let mut stmt = conn.prepare(
        "SELECT id,platform,region,product_key,title,image,store_url,target_cny,created_at,updated_at,deleted,
                last_status,last_currency,last_final_formatted,last_initial_formatted,last_discount,last_final_cny,last_checked_at,is_physical
         FROM wishlist_items WHERE updated_at > ?1",
    )?;
    let rows = stmt.query_map(params![since], |r| {
        Ok(WireItem {
            id: r.get(0)?,
            platform: r.get(1)?,
            region: r.get(2)?,
            product_key: r.get(3)?,
            title: r.get(4)?,
            image: r.get(5)?,
            store_url: r.get(6)?,
            target_cny: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
            deleted: r.get::<_, i64>(10)? != 0,
            status: r.get(11)?,
            currency: r.get(12)?,
            final_formatted: r.get(13)?,
            initial_formatted: r.get(14)?,
            discount_percent: r.get::<_, Option<i64>>(15)?.unwrap_or(0),
            final_cny: r.get(16)?,
            checked_at: r.get::<_, Option<i64>>(17)?.unwrap_or(0),
            is_physical: r.get::<_, i64>(18)? != 0,
            low_cny: None,
        })
    })?;
    rows.collect()
}

/// 应用服务端回传的条目：元数据 LWW，价格取更晚者，跨设备降价标红点，历史并入
fn apply_remote(conn: &Connection, w: &WireItem) -> rusqlite::Result<()> {
    let local: Option<(i64, Option<i64>, Option<f64>, Option<String>)> = conn
        .query_row(
            "SELECT updated_at, last_checked_at, last_final_cny, last_status FROM wishlist_items WHERE id=?1",
            params![w.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();

    let buyable = |s: &str| s == "ok" || s == "free";

    match local {
        None => {
            conn.execute(
                "INSERT INTO wishlist_items
                    (id,platform,region,product_key,title,image,store_url,target_cny,created_at,updated_at,deleted,is_physical,
                     last_status,last_currency,last_final_formatted,last_initial_formatted,last_discount,last_final_cny,last_checked_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                params![
                    w.id, w.platform, w.region, w.product_key, w.title, w.image, w.store_url, w.target_cny,
                    w.created_at, w.updated_at, w.deleted as i64, w.is_physical as i64,
                    w.status, w.currency, w.final_formatted, w.initial_formatted, w.discount_percent, w.final_cny, w.checked_at
                ],
            )?;
            if w.status.is_some() && w.checked_at > 0 {
                conn.execute(
                    "INSERT INTO price_history (item_id, checked_at, status, final_cny, discount_percent)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![w.id, w.checked_at, w.status, w.final_cny, w.discount_percent],
                )?;
            }
        }
        Some((lu, lchecked, lcny, lstatus)) => {
            if w.updated_at > lu {
                conn.execute(
                    "UPDATE wishlist_items SET platform=?2,region=?3,product_key=?4,title=?5,image=?6,
                        store_url=?7,target_cny=?8,deleted=?9,updated_at=?10,is_physical=?11 WHERE id=?1",
                    params![
                        w.id, w.platform, w.region, w.product_key, w.title, w.image, w.store_url,
                        w.target_cny, w.deleted as i64, w.updated_at, w.is_physical as i64
                    ],
                )?;
            }
            if w.status.is_some() && w.checked_at > lchecked.unwrap_or(0) {
                let dropped = w.status.as_deref().map(buyable).unwrap_or(false)
                    && lstatus.as_deref().map(buyable).unwrap_or(false)
                    && matches!((w.final_cny, lcny), (Some(n), Some(o)) if n < o - 0.001);
                conn.execute(
                    "UPDATE wishlist_items SET last_status=?2,last_currency=?3,last_final_formatted=?4,
                        last_initial_formatted=?5,last_discount=?6,last_final_cny=?7,last_checked_at=?8,
                        unseen_drop = CASE WHEN ?9<>0 THEN 1 ELSE unseen_drop END
                     WHERE id=?1",
                    params![
                        w.id, w.status, w.currency, w.final_formatted, w.initial_formatted,
                        w.discount_percent, w.final_cny, w.checked_at, dropped as i64
                    ],
                )?;
                let dup: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM price_history WHERE item_id=?1 AND checked_at=?2",
                        params![w.id, w.checked_at],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                if dup == 0 {
                    conn.execute(
                        "INSERT INTO price_history (item_id, checked_at, status, final_cny, discount_percent)
                         VALUES (?1,?2,?3,?4,?5)",
                        params![w.id, w.checked_at, w.status, w.final_cny, w.discount_percent],
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// 每天首次打开触发：重新取价 + 与服务器同步（跨设备 + 服务端史低）。
/// 复用备忘录同步的服务器地址与登录态（settings: server_url / sync_token）。
#[tauri::command]
pub async fn wishlist_sync(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Vec<WishItem>, String> {
    // 1) 本地重新取价（当日最新）
    refetch_all(&state).await?;

    // 2) 读取同步配置 + 本地变更
    let (server_url, token, last_sync_at, changes) = {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let server_url = crate::notes::get_setting(&conn, "server_url").unwrap_or_default();
        let token = crate::notes::get_setting(&conn, "sync_token").unwrap_or_default();
        let last_sync_at = crate::notes::get_setting(&conn, "wishlist_last_sync_at")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let changes = gather_changed(&conn, last_sync_at).map_err(|e| e.to_string())?;
        (server_url, token, last_sync_at, changes)
    };
    if server_url.is_empty() || token.is_empty() {
        return Err("未登录同步账户（在备忘录里登录后即可同步心愿单）".into());
    }

    // 3) 请求服务器
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{}/wishlist/sync", server_url.trim_end_matches('/')))
        .bearer_auth(&token)
        .json(&WireReq { last_sync_at, items: changes })
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let st = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("同步失败 {}: {}", st, body));
    }
    let data: WireResp = resp.json().await.map_err(|e| e.to_string())?;

    // 4) 应用 + 保存游标
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    for w in &data.items {
        apply_remote(&conn, w).map_err(|e| e.to_string())?;
    }
    crate::notes::set_setting(&conn, "wishlist_last_sync_at", &data.synced_at.to_string())
        .map_err(|e| e.to_string())?;
    load_items(&conn).map_err(|e| e.to_string())
}
