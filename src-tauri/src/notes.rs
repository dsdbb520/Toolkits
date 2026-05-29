use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
            id         TEXT PRIMARY KEY,
            title      TEXT NOT NULL DEFAULT '无标题',
            content    TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            deleted    INTEGER NOT NULL DEFAULT 0
        );",
    )
}

pub fn get_all(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, created_at, updated_at
         FROM notes WHERE deleted = 0
         ORDER BY updated_at DESC",
    )?;
    let notes = stmt.query_map([], |row| {
        Ok(Note {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    notes.collect()
}

pub fn create(conn: &Connection) -> Result<Note> {
    let note = Note {
        id: uuid::Uuid::new_v4().to_string(),
        title: "无标题".to_string(),
        content: String::new(),
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    conn.execute(
        "INSERT INTO notes (id, title, content, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![note.id, note.title, note.content, note.created_at, note.updated_at],
    )?;
    Ok(note)
}

pub fn update(conn: &Connection, id: &str, title: &str, content: &str) -> Result<Note> {
    let updated_at = now_ms();
    conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, updated_at, id],
    )?;
    let note = conn.query_row(
        "SELECT id, title, content, created_at, updated_at FROM notes WHERE id = ?1",
        params![id],
        |row| Ok(Note {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
        }),
    )?;
    Ok(note)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("UPDATE notes SET deleted = 1 WHERE id = ?1", params![id])?;
    Ok(())
}
