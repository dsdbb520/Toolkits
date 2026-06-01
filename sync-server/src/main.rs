use axum::{
    extract::{Extension, Json, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::post,
    Router,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::cors::CorsLayer;

// ─── 配置（从环境变量读取）────────────────────────────────────
// JWT_SECRET     必填
// PORT           默认 3000
// DB_PATH        默认 ./notes_server.db
// SYNC_USERNAME  可选，有则在启动时自动创建该账户（向后兼容）
// SYNC_PASSWORD  配合 SYNC_USERNAME 使用

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

// ─── 数据结构 ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Note {
    id: String,
    title: String,
    content: String,
    is_note: bool,
    deleted: bool,
    created_at: i64,
    updated_at: i64,
}

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

#[derive(Deserialize)]
struct AuthReq {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct AuthResp {
    token: String,
}

#[derive(Deserialize)]
struct SyncReq {
    last_sync_at: i64,
    notes: Vec<Note>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ConflictPair {
    mine: Note,   // 客户端版本
    theirs: Note, // 服务端版本
}

#[derive(Serialize)]
struct SyncResp {
    notes: Vec<Note>,
    conflicts: Vec<ConflictPair>,
    synced_at: i64,
}

// 通过 axum Extension 在中间件和处理函数之间传递已认证的用户名
#[derive(Clone)]
struct AuthUser(String);

// ─── 应用状态 ──────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    jwt_secret: String,
}

// ─── 数据库初始化 & 迁移 ───────────────────────────────────

fn init_db(conn: &Connection) {
    // 用户表
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS users (
            username TEXT PRIMARY KEY,
            password TEXT NOT NULL
        );",
    )
    .expect("failed to init users table");

    // 检查 notes 表是否已有 user_id 列
    let has_user_id: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name='user_id'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if has_user_id {
        return; // 已是新 schema，无需迁移
    }

    let notes_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='notes'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if notes_exists {
        // 旧 schema 迁移：将原有数据的 user_id 设为空字符串（对应旧的单用户 admin）
        conn.execute_batch(
            "ALTER TABLE notes RENAME TO _notes_v1;
             CREATE TABLE notes (
                 id         TEXT NOT NULL,
                 user_id    TEXT NOT NULL DEFAULT '',
                 title      TEXT NOT NULL DEFAULT '',
                 content    TEXT NOT NULL DEFAULT '',
                 is_note    INTEGER NOT NULL DEFAULT 0,
                 deleted    INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 PRIMARY KEY (id, user_id)
             );
             INSERT INTO notes
               SELECT id,'',title,content,is_note,deleted,created_at,updated_at
               FROM _notes_v1;
             DROP TABLE _notes_v1;",
        )
        .expect("failed to migrate notes table");
    } else {
        conn.execute_batch(
            "CREATE TABLE notes (
                 id         TEXT NOT NULL,
                 user_id    TEXT NOT NULL DEFAULT '',
                 title      TEXT NOT NULL DEFAULT '',
                 content    TEXT NOT NULL DEFAULT '',
                 is_note    INTEGER NOT NULL DEFAULT 0,
                 deleted    INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 PRIMARY KEY (id, user_id)
             );",
        )
        .expect("failed to create notes table");
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ─── JWT ───────────────────────────────────────────────────

fn make_token(sub: &str, secret: &str) -> String {
    let exp = (now_ms() / 1000) as usize + 10 * 365 * 24 * 3600; // 10 年
    let claims = Claims { sub: sub.to_string(), exp };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("jwt encode failed")
}

fn verify_token(token: &str, secret: &str) -> Option<String> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|data| data.claims.sub)
}

// ─── 认证中间件 ────────────────────────────────────────────

async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: axum::extract::Request,
    next: Next,
) -> impl IntoResponse {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");

    match verify_token(token, &state.jwt_secret) {
        Some(username) => {
            request.extensions_mut().insert(AuthUser(username));
            next.run(request).await
        }
        None => (StatusCode::UNAUTHORIZED, "Invalid token").into_response(),
    }
}

fn get_note(db: &Connection, id: &str, user_id: &str) -> Option<Note> {
    db.query_row(
        "SELECT id,title,content,is_note,deleted,created_at,updated_at
         FROM notes WHERE id=?1 AND user_id=?2",
        params![id, user_id],
        |row| Ok(Note {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            is_note: row.get::<_, i64>(3)? != 0,
            deleted: row.get::<_, i64>(4)? != 0,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        }),
    ).ok()
}

// ─── 路由处理 ──────────────────────────────────────────────

async fn login(
    State(state): State<AppState>,
    Json(req): Json<AuthReq>,
) -> impl IntoResponse {
    if req.username.is_empty() || req.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "用户名和密码不能为空"})),
        )
            .into_response();
    }
    let db = state.db.lock().unwrap();
    let stored: Option<String> = db
        .query_row(
            "SELECT password FROM users WHERE username=?1",
            params![req.username],
            |r| r.get(0),
        )
        .ok();

    match stored {
        Some(pw) if pw == req.password => {
            Json(AuthResp { token: make_token(&req.username, &state.jwt_secret) }).into_response()
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "用户名或密码错误"})),
        )
            .into_response(),
    }
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<AuthReq>,
) -> impl IntoResponse {
    if req.username.is_empty() || req.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "用户名和密码不能为空"})),
        )
            .into_response();
    }
    let db = state.db.lock().unwrap();
    match db.execute(
        "INSERT INTO users(username, password) VALUES(?1, ?2)",
        params![req.username, req.password],
    ) {
        Ok(_) => {
            Json(AuthResp { token: make_token(&req.username, &state.jwt_secret) }).into_response()
        }
        Err(_) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "用户名已存在"})),
        )
            .into_response(),
    }
}

async fn sync(
    State(state): State<AppState>,
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    Json(req): Json<SyncReq>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    let synced_at = now_ms();
    let mut conflicts: Vec<ConflictPair> = Vec::new();

    for note in &req.notes {
        let server_note = get_note(&db, &note.id, &user_id);

        if let Some(ref sv) = server_note {
            // 双方在上次同步后都修改了 → 冲突，交给客户端处理
            if sv.updated_at > req.last_sync_at && note.updated_at > req.last_sync_at {
                conflicts.push(ConflictPair { mine: note.clone(), theirs: sv.clone() });
                continue;
            }
            // 服务端版本更新或相同 → 跳过
            if sv.updated_at >= note.updated_at {
                continue;
            }
        }

        // 客户端版本更新或是新笔记 → 应用
        db.execute(
            "INSERT INTO notes(id,user_id,title,content,is_note,deleted,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id,user_id) DO UPDATE SET
               title=excluded.title, content=excluded.content,
               is_note=excluded.is_note, deleted=excluded.deleted,
               updated_at=excluded.updated_at",
            params![
                note.id, user_id, note.title, note.content,
                note.is_note as i64, note.deleted as i64,
                note.created_at, note.updated_at
            ],
        )
        .ok();
    }

    // 返回该用户上次同步后的所有变更
    let mut stmt = db
        .prepare(
            "SELECT id,title,content,is_note,deleted,created_at,updated_at
             FROM notes WHERE user_id=?1 AND updated_at > ?2",
        )
        .unwrap();

    let notes: Vec<Note> = stmt
        .query_map(params![user_id, req.last_sync_at], |row| {
            Ok(Note {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                is_note: row.get::<_, i64>(3)? != 0,
                deleted: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(SyncResp { notes, conflicts, synced_at })
}

// ─── 主入口 ────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET is required");
    let port: u16 = env("PORT", "3000").parse().expect("invalid PORT");
    let db_path = env("DB_PATH", "./notes_server.db");

    let conn = Connection::open(&db_path).expect("failed to open db");
    init_db(&conn);

    // 向后兼容：若设置了 SYNC_USERNAME/SYNC_PASSWORD，自动创建/保留该账户，
    // 并将旧 schema 遗留的 user_id='' 笔记归属到该用户
    if let (Ok(username), Ok(password)) = (
        std::env::var("SYNC_USERNAME"),
        std::env::var("SYNC_PASSWORD"),
    ) {
        conn.execute(
            "INSERT OR IGNORE INTO users(username, password) VALUES(?1, ?2)",
            params![username, password],
        )
        .ok();
        conn.execute(
            "UPDATE notes SET user_id=?1 WHERE user_id=''",
            params![username],
        )
        .ok();
        println!("sync-server: seeded user '{username}'");
    }

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        jwt_secret,
    };

    let protected = Router::new()
        .route("/sync", post(sync))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let app = Router::new()
        .route("/login", post(login))
        .route("/register", post(register))
        .merge(protected)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("sync-server listening on port {port}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
