use axum::{
    extract::{Json, State},
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
// SYNC_USERNAME  默认 admin
// SYNC_PASSWORD  必填
// JWT_SECRET     必填
// PORT           默认 3000
// DB_PATH        默认 ./notes_server.db

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
struct LoginReq {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResp {
    token: String,
}

#[derive(Deserialize)]
struct SyncReq {
    last_sync_at: i64,
    notes: Vec<Note>,
}

#[derive(Serialize)]
struct SyncResp {
    notes: Vec<Note>,
    synced_at: i64,
}

// ─── 应用状态 ──────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    username: String,
    password: String,
    jwt_secret: String,
}

// ─── 数据库初始化 ──────────────────────────────────────────

fn init_db(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
            id         TEXT PRIMARY KEY,
            title      TEXT NOT NULL DEFAULT '',
            content    TEXT NOT NULL DEFAULT '',
            is_note    INTEGER NOT NULL DEFAULT 0,
            deleted    INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );",
    )
    .expect("failed to init db");
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

fn verify_token(token: &str, secret: &str) -> bool {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .is_ok()
}

// ─── 认证中间件 ────────────────────────────────────────────

async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> impl IntoResponse {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");

    if !verify_token(token, &state.jwt_secret) {
        return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
    }
    next.run(request).await
}

// ─── 路由处理 ──────────────────────────────────────────────

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> impl IntoResponse {
    if req.username != state.username || req.password != state.password {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid credentials"}))).into_response();
    }
    let token = make_token(&req.username, &state.jwt_secret);
    Json(LoginResp { token }).into_response()
}

async fn sync(
    State(state): State<AppState>,
    Json(req): Json<SyncReq>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    let synced_at = now_ms();

    // 合并客户端发来的笔记（last-write-wins by updated_at）
    for note in &req.notes {
        let local_ts: Option<i64> = db
            .query_row("SELECT updated_at FROM notes WHERE id=?1", params![note.id], |r| r.get(0))
            .ok();

        match local_ts {
            Some(ts) if ts >= note.updated_at => continue,
            _ => {
                db.execute(
                    "INSERT INTO notes(id,title,content,is_note,deleted,created_at,updated_at)
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
                )
                .ok();
            }
        }
    }

    // 返回客户端上次同步之后服务端有变化的笔记
    let mut stmt = db
        .prepare(
            "SELECT id,title,content,is_note,deleted,created_at,updated_at
             FROM notes WHERE updated_at > ?1",
        )
        .unwrap();

    let notes: Vec<Note> = stmt
        .query_map(params![req.last_sync_at], |row| {
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

    Json(SyncResp { notes, synced_at })
}

// ─── 主入口 ────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let username = env("SYNC_USERNAME", "admin");
    let password = std::env::var("SYNC_PASSWORD").expect("SYNC_PASSWORD is required");
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET is required");
    let port: u16 = env("PORT", "3000").parse().expect("invalid PORT");
    let db_path = env("DB_PATH", "./notes_server.db");

    let conn = Connection::open(&db_path).expect("failed to open db");
    init_db(&conn);

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        username,
        password,
        jwt_secret,
    };

    let protected = Router::new()
        .route("/sync", post(sync))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let app = Router::new()
        .route("/login", post(login))
        .merge(protected)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("sync-server listening on port {port}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
