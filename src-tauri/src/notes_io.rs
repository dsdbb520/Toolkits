// 备忘录导入/导出：
//   导出 —— 把笔记（TipTap HTML）转成 Markdown / HTML / 纯文本 存到用户选的文件（便于喂给 AI 修改、分享给别人）
//   导入 —— 读取 .md / .txt / .html 文件转回 HTML，新建一条笔记（AI 改完再导入回来）
use crate::notes;
use rusqlite::params;

// ─── 转换 ──────────────────────────────────────────────────────

fn html_to_md(html: &str) -> String {
    html2md::parse_html(html)
}

fn md_to_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let parser = Parser::new_ext(md, Options::all());
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// 粗略的 HTML → 纯文本（块级标签转换行后去标签、解码常见实体）
fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for t in ["</p>", "</h1>", "</h2>", "</h3>", "</li>", "<br>", "<br/>", "<br />"] {
        s = s.replace(t, "\n");
    }
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// 文件名安全化：去掉 Windows 不允许的字符
fn sanitize_filename(s: &str) -> String {
    let name: String = s
        .chars()
        .map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let name = name.trim().trim_matches('.').to_string();
    if name.is_empty() { "笔记".to_string() } else { name }
}

/// 从 Markdown 取首个 `# 标题` 作为笔记标题（并从正文移除该行）；没有则用 fallback
fn split_md_title(md: &str, fallback: &str) -> (String, String) {
    // 跳过前导空行
    let mut leading_blanks = 0;
    let first = loop {
        match md.lines().nth(leading_blanks) {
            Some(l) if l.trim().is_empty() => leading_blanks += 1,
            other => break other,
        }
    };
    if let Some(l) = first {
        let t = l.trim_start();
        if let Some(rest) = t.strip_prefix("# ") {
            // 移除标题行，正文为其后内容
            let body: String = md
                .lines()
                .skip(leading_blanks + 1)
                .collect::<Vec<_>>()
                .join("\n");
            return (rest.trim().to_string(), body.trim_start().to_string());
        }
    }
    (fallback.to_string(), md.to_string())
}

// ─── 命令 ──────────────────────────────────────────────────────

/// 导出当前笔记到用户选择的文件。format: "md" | "html" | "txt"。返回保存路径（取消则 None）
#[tauri::command]
pub async fn notes_export(
    state: tauri::State<'_, crate::AppState>,
    id: String,
    format: String,
) -> Result<Option<String>, String> {
    let (title, content): (String, String) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.query_row(
            "SELECT title, content FROM notes WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?
    };
    let title = if title.trim().is_empty() { "无标题".to_string() } else { title };

    let (ext, data) = match format.as_str() {
        "html" => (
            "html",
            format!(
                "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{0}</title></head>\n<body>\n<h1>{0}</h1>\n{1}\n</body></html>",
                html_escape(&title),
                content
            ),
        ),
        "txt" => ("txt", format!("{}\n\n{}", title, html_to_text(&content))),
        _ => ("md", format!("# {}\n\n{}", title, html_to_md(&content))),
    };

    let default_name = format!("{}.{}", sanitize_filename(&title), ext);
    let dest = rfd::AsyncFileDialog::new()
        .set_file_name(&default_name)
        .add_filter("笔记", &[ext])
        .save_file()
        .await;

    match dest {
        Some(handle) => {
            let path = handle.path().to_path_buf();
            std::fs::write(&path, data).map_err(|e| format!("写入失败: {}", e))?;
            Ok(Some(path.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

/// 从文件导入为一条新笔记。返回新建的笔记（取消则 None）
#[tauri::command]
pub async fn notes_import(
    state: tauri::State<'_, crate::AppState>,
) -> Result<Option<notes::Note>, String> {
    let file = rfd::AsyncFileDialog::new()
        .add_filter("笔记文件", &["md", "markdown", "txt", "html", "htm"])
        .pick_file()
        .await;
    let file = match file {
        Some(f) => f,
        None => return Ok(None),
    };
    let path = file.path().to_path_buf();
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {}", e))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("导入笔记").to_string();

    let (title, html) = match ext.as_str() {
        "html" | "htm" => (stem, raw),
        _ => {
            let (t, body) = split_md_title(&raw, &stem);
            (t, md_to_html(&body))
        }
    };

    let note = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        notes::create_with_content(&db, &title, &html).map_err(|e| e.to_string())?
    };
    Ok(Some(note))
}
