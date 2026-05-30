use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub content: String,
    pub is_note: bool,
    pub deleted: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        is_note: row.get::<_, i64>(3)? != 0,
        deleted: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
            id         TEXT PRIMARY KEY,
            title      TEXT NOT NULL DEFAULT '无标题',
            content    TEXT NOT NULL DEFAULT '',
            is_note    INTEGER NOT NULL DEFAULT 0,
            deleted    INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS download_history (
            id         TEXT PRIMARY KEY,
            bvid       TEXT NOT NULL DEFAULT '',
            title      TEXT NOT NULL DEFAULT '',
            cover      TEXT NOT NULL DEFAULT '',
            part       TEXT NOT NULL DEFAULT '',
            quality    INTEGER NOT NULL DEFAULT 0,
            mode       TEXT NOT NULL DEFAULT '',
            file_path  TEXT NOT NULL DEFAULT '',
            file_size  INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    // 迁移：为旧数据库添加缺失列
    let _ = conn.execute_batch("ALTER TABLE notes ADD COLUMN is_note INTEGER NOT NULL DEFAULT 0");
    Ok(())
}

pub fn get_all(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, is_note, deleted, created_at, updated_at
         FROM notes WHERE deleted = 0
         ORDER BY updated_at DESC",
    )?;
    let result: Result<Vec<Note>> = stmt.query_map([], row_to_note)?.collect();
    result
}

pub fn create(conn: &Connection) -> Result<Note> {
    let note = Note {
        id: uuid::Uuid::new_v4().to_string(),
        title: "无标题".to_string(),
        content: String::new(),
        is_note: false,
        deleted: false,
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    conn.execute(
        "INSERT INTO notes (id, title, content, is_note, deleted, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![note.id, note.title, note.content, 0i64, 0i64, note.created_at, note.updated_at],
    )?;
    Ok(note)
}

pub fn update(conn: &Connection, id: &str, title: &str, content: &str) -> Result<Note> {
    let updated_at = now_ms();
    conn.execute(
        "UPDATE notes SET title=?1, content=?2, updated_at=?3 WHERE id=?4",
        params![title, content, updated_at, id],
    )?;
    conn.query_row(
        "SELECT id, title, content, is_note, deleted, created_at, updated_at FROM notes WHERE id=?1",
        params![id],
        row_to_note,
    )
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let updated_at = now_ms();
    conn.execute(
        "UPDATE notes SET deleted=1, updated_at=?1 WHERE id=?2",
        params![updated_at, id],
    )?;
    Ok(())
}

pub fn toggle_note_type(conn: &Connection, id: &str) -> Result<Note> {
    let updated_at = now_ms();
    conn.execute(
        "UPDATE notes SET is_note = CASE WHEN is_note=0 THEN 1 ELSE 0 END, updated_at=?1 WHERE id=?2",
        params![updated_at, id],
    )?;
    conn.query_row(
        "SELECT id, title, content, is_note, deleted, created_at, updated_at FROM notes WHERE id=?1",
        params![id],
        row_to_note,
    )
}

// ─── 同步相关 ───────────────────────────────────────────────

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![key],
        |r| r.get(0),
    ).ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// 获取 last_sync_at 之后修改过的所有笔记（含软删除）
pub fn get_changes_since(conn: &Connection, since: i64) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, is_note, deleted, created_at, updated_at
         FROM notes WHERE updated_at > ?1",
    )?;
    let result: Result<Vec<Note>> = stmt.query_map(params![since], row_to_note)?.collect();
    result
}

/// 将服务端返回的笔记合并到本地（last-write-wins by updated_at）
pub fn apply_remote_notes(conn: &Connection, remote: &[Note]) -> Result<()> {
    for note in remote {
        let local_updated: Option<i64> = conn
            .query_row("SELECT updated_at FROM notes WHERE id=?1", params![note.id], |r| r.get(0))
            .ok();

        match local_updated {
            Some(local_ts) if local_ts >= note.updated_at => continue,
            _ => {
                conn.execute(
                    "INSERT INTO notes(id, title, content, is_note, deleted, created_at, updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7)
                     ON CONFLICT(id) DO UPDATE SET
                       title=excluded.title, content=excluded.content,
                       is_note=excluded.is_note, deleted=excluded.deleted,
                       updated_at=excluded.updated_at",
                    params![
                        note.id, note.title, note.content,
                        note.is_note as i64, note.deleted as i64,
                        note.created_at, note.updated_at
                    ],
                )?;
            }
        }
    }
    Ok(())
}
