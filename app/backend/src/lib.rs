//! HTTP backend for the Windows GUI. A port of engine/gui/app.py: it serves the
//! same JSON API, so the web UI in app/ui runs unchanged.

pub mod libconfig;
pub mod live;
pub mod process;

use axum::body::Body;
use axum::extract::{Path as UrlPath, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tower::ServiceExt as _;
use tower_http::services::{ServeDir, ServeFile};

pub const LIVE_PORT_BASE: u16 = 17300;
pub const LIVE_PORT_MAX: u16 = LIVE_PORT_BASE + 999;
const DEFAULT_LIVE_SAMPLE_RATE: u32 = 16000; // engine's WAVE_RATE for an NFM build
const AUDIO_EXTENSIONS: [&str; 3] = ["mp3", "wav", "ogg"];

/// Where things live on disk. The desktop shell decides these per platform.
#[derive(Clone, Debug)]
pub struct Paths {
    pub ui_dir: PathBuf,
    pub configs_dir: PathBuf,
    pub state_path: PathBuf,
    pub default_recordings_dir: PathBuf,
    pub default_binary: String,
}

pub struct AppState {
    pub paths: Paths,
    pub process: process::ProcessManager,
    pub live: live::LiveAudioManager,
    state_lock: std::sync::Mutex<()>,
}

type Shared = Arc<AppState>;

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

type ApiResult = Result<Json<Value>, ApiError>;

impl AppState {
    pub fn new(paths: Paths) -> Shared {
        Arc::new(AppState { paths, process: process::ProcessManager::new(), live: live::LiveAudioManager::default(), state_lock: Default::default() })
    }

    fn load_state(&self) -> serde_json::Map<String, Value> {
        let mut state = std::fs::read_to_string(&self.paths.state_path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        state.entry("active_config").or_insert(json!("basic_multichannel.conf"));
        state.entry("binary_path").or_insert(json!(self.paths.default_binary));
        state.entry("recordings_dir").or_insert(json!(self.paths.default_recordings_dir.to_string_lossy()));
        state.entry("live_sample_rate").or_insert(json!(DEFAULT_LIVE_SAMPLE_RATE));
        state
    }

    fn save_state(&self, state: &serde_json::Map<String, Value>) -> Result<(), ApiError> {
        if let Some(dir) = self.paths.state_path.parent() {
            std::fs::create_dir_all(dir).map_err(internal)?;
        }
        std::fs::write(&self.paths.state_path, serde_json::to_string_pretty(state).unwrap()).map_err(internal)
    }

    fn get_str(state: &serde_json::Map<String, Value>, key: &str) -> String {
        state.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
    }

    fn config_path(&self, filename: &str) -> Result<PathBuf, ApiError> {
        let valid = filename.ends_with(".conf") && filename.len() > 5 && filename.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-')) && !filename.contains("..");
        if !valid {
            return Err(bad_request("Invalid config filename"));
        }
        Ok(self.paths.configs_dir.join(filename))
    }

    fn active_config_path(&self) -> Result<(String, PathBuf), ApiError> {
        let name = Self::get_str(&self.load_state(), "active_config");
        let path = self.config_path(&name)?;
        Ok((name, path))
    }

    fn recordings_dir(&self) -> PathBuf {
        PathBuf::from(Self::get_str(&self.load_state(), "recordings_dir"))
    }
}

/// Resolves `relpath` under `base`, refusing anything that could escape it.
fn safe_subpath(base: &Path, relpath: &str) -> Result<PathBuf, ApiError> {
    let mut out = base.to_path_buf();
    for part in relpath.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return Err(ApiError(StatusCode::FORBIDDEN, "Path escapes recordings directory".into()));
        }
        out.push(part);
    }
    Ok(out)
}

fn body_str(body: &Value, key: &str) -> String {
    body.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

async fn list_configs(State(app): State<Shared>) -> ApiResult {
    let mut files: Vec<String> = std::fs::read_dir(&app.paths.configs_dir)
        .map_err(internal)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| f.ends_with(".conf"))
        .collect();
    files.sort();
    Ok(Json(json!({"files": files, "active": app.load_state().get("active_config")})))
}

async fn create_config(State(app): State<Shared>, Json(body): Json<Value>) -> ApiResult {
    let mut filename = body_str(&body, "filename");
    if !filename.ends_with(".conf") {
        filename.push_str(".conf");
    }
    let path = app.config_path(&filename)?;
    if path.exists() {
        return Err(ApiError(StatusCode::CONFLICT, "A config with that name already exists".into()));
    }
    std::fs::write(&path, "devices:\n(\n);\n").map_err(internal)?;
    Ok(Json(json!({"ok": true, "filename": filename})))
}

async fn select_config(State(app): State<Shared>, Json(body): Json<Value>) -> ApiResult {
    let filename = body_str(&body, "filename");
    app.config_path(&filename)?;
    let _guard = app.state_lock.lock().unwrap();
    let mut state = app.load_state();
    state.insert("active_config".into(), json!(filename));
    app.save_state(&state)?;
    Ok(Json(json!({"ok": true})))
}

async fn get_config(State(app): State<Shared>) -> ApiResult {
    let (name, path) = app.active_config_path()?;
    let text = std::fs::read_to_string(&path).map_err(|e| bad_request(format!("Cannot read {}: {e}", path.display())))?;
    let (data, error) = match libconfig::parse(&text) {
        Ok(tree) => (tree, Value::Null),
        Err(e) => (Value::Null, json!(e.to_string())),
    };
    Ok(Json(json!({"filename": name, "data": data, "raw": text, "error": error})))
}

async fn save_config(State(app): State<Shared>, Json(body): Json<Value>) -> ApiResult {
    let (_, path) = app.active_config_path()?;
    let text = libconfig::dump(body.get("data").unwrap_or(&Value::Null)).map_err(|e| bad_request(format!("Could not serialize config: {e}")))?;
    std::fs::write(&path, &text).map_err(internal)?;
    Ok(Json(json!({"ok": true, "raw": text})))
}

async fn save_raw_config(State(app): State<Shared>, Json(body): Json<Value>) -> ApiResult {
    let (_, path) = app.active_config_path()?;
    let text = body_str(&body, "raw");
    libconfig::parse(&text).map_err(|e| bad_request(format!("Syntax error: {e}")))?;
    std::fs::write(&path, &text).map_err(internal)?;
    Ok(Json(json!({"ok": true})))
}

async fn validate_raw_config(Json(body): Json<Value>) -> ApiResult {
    Ok(Json(match libconfig::parse(&body_str(&body, "raw")) {
        Ok(_) => json!({"ok": true}),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    }))
}

async fn get_settings(State(app): State<Shared>) -> ApiResult {
    let state = app.load_state();
    Ok(Json(json!({
        "binary_path": state["binary_path"],
        "recordings_dir": state["recordings_dir"],
        "live_sample_rate": state["live_sample_rate"],
        "live_port_base": LIVE_PORT_BASE,
        "live_port_max": LIVE_PORT_MAX,
    })))
}

async fn update_settings(State(app): State<Shared>, Json(body): Json<Value>) -> ApiResult {
    let _guard = app.state_lock.lock().unwrap();
    let mut state = app.load_state();
    for key in ["binary_path", "recordings_dir", "live_sample_rate"] {
        if let Some(v) = body.get(key) {
            state.insert(key.into(), v.clone());
        }
    }
    app.save_state(&state)?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct Since {
    #[serde(default)]
    since: u64,
}

async fn live_chunk(State(app): State<Shared>, UrlPath(port): UrlPath<u16>, Query(q): Query<Since>) -> ApiResult {
    if !(LIVE_PORT_BASE..=LIVE_PORT_MAX).contains(&port) {
        return Err(bad_request("Port out of range"));
    }
    let channel = app.live.get(port);
    if let Some(err) = &channel.error {
        return Err(bad_request(format!("Could not listen on port {port}: {err}")));
    }
    let (chunk, next) = channel.read_since(q.since);
    Ok(Json(json!({"chunk": base64::engine::general_purpose::STANDARD.encode(chunk), "next": next})))
}

fn walk_audio(base: &Path, dir: &Path, out: &mut Vec<Value>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk_audio(base, &path, out);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let date = chrono::DateTime::from_timestamp(mtime as i64, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let rel: Vec<String> = path.strip_prefix(base).unwrap_or(&path).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        out.push(json!({
            "path": rel.join("/"),
            "name": entry.file_name().to_string_lossy(),
            "bytes": meta.len(),
            "mtime": mtime,
            "date": date,
        }));
    }
}

async fn list_recordings(State(app): State<Shared>) -> ApiResult {
    let base = app.recordings_dir();
    let mut entries = Vec::new();
    let b = base.clone();
    let entries = tokio::task::spawn_blocking(move || {
        walk_audio(&b, &b, &mut entries);
        entries.sort_by(|a, b| b["mtime"].as_f64().partial_cmp(&a["mtime"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        entries
    })
    .await
    .map_err(internal)?;
    Ok(Json(json!({"base_dir": base.to_string_lossy(), "exists": base.is_dir(), "recordings": entries})))
}

fn folder_opener() -> Option<&'static str> {
    if cfg!(windows) {
        Some("explorer")
    } else if cfg!(target_os = "macos") {
        Some("open")
    } else {
        Some("xdg-open")
    }
}

async fn open_recordings_folder(State(app): State<Shared>) -> ApiResult {
    let base = app.recordings_dir();
    if !base.is_dir() {
        return Err(bad_request(format!("Folder does not exist yet: {}", base.display())));
    }
    let opener = folder_opener().ok_or_else(|| bad_request("No file manager opener found on this machine."))?;
    std::process::Command::new(opener).arg(&base).spawn().map_err(internal)?;
    Ok(Json(json!({"ok": true})))
}

async fn delete_recording(State(app): State<Shared>, UrlPath(relpath): UrlPath<String>) -> ApiResult {
    let full = safe_subpath(&app.recordings_dir(), &relpath)?;
    if !full.is_file() {
        return Err(ApiError(StatusCode::NOT_FOUND, "Not found".into()));
    }
    std::fs::remove_file(&full).map_err(internal)?;
    Ok(Json(json!({"ok": true})))
}

async fn serve_audio(State(app): State<Shared>, UrlPath(relpath): UrlPath<String>, req: Request) -> Result<Response, ApiError> {
    let full = safe_subpath(&app.recordings_dir(), &relpath)?;
    if !full.is_file() {
        return Err(ApiError(StatusCode::NOT_FOUND, "Not found".into()));
    }
    ServeFile::new(full).oneshot(req).await.map(|r| r.map(Body::new)).map_err(internal)
}

async fn process_status(State(app): State<Shared>) -> ApiResult {
    Ok(Json(serde_json::to_value(app.process.status()).unwrap()))
}

#[derive(Deserialize)]
struct LogSince {
    #[serde(default)]
    since: usize,
}

async fn process_logs(State(app): State<Shared>, Query(q): Query<LogSince>) -> ApiResult {
    let lines = app.process.logs(q.since);
    let next = q.since + lines.len();
    Ok(Json(json!({"lines": lines, "next": next})))
}

async fn run_blocking<F: FnOnce() -> Result<(), String> + Send + 'static>(f: F) -> ApiResult {
    tokio::task::spawn_blocking(f).await.map_err(internal)?.map_err(bad_request)?;
    Ok(Json(json!({"ok": true})))
}

async fn process_start(State(app): State<Shared>) -> ApiResult {
    let (_, path) = app.active_config_path()?;
    let binary = AppState::get_str(&app.load_state(), "binary_path");
    run_blocking(move || app.process.start(&binary, &path)).await
}

async fn process_stop(State(app): State<Shared>) -> ApiResult {
    run_blocking(move || {
        app.process.stop(Duration::from_secs(5));
        Ok(())
    })
    .await
}

async fn process_restart(State(app): State<Shared>) -> ApiResult {
    let (_, path) = app.active_config_path()?;
    let binary = AppState::get_str(&app.load_state(), "binary_path");
    run_blocking(move || app.process.restart(&binary, &path)).await
}

pub fn router(app: Shared) -> Router {
    let index = app.paths.ui_dir.join("templates").join("index.html");
    Router::new()
        .route_service("/", ServeFile::new(index))
        .nest_service("/static", ServeDir::new(app.paths.ui_dir.join("static")))
        .route("/api/configs", get(list_configs).post(create_config))
        .route("/api/configs/select", post(select_config))
        .route("/api/config", get(get_config).post(save_config))
        .route("/api/config/raw", post(save_raw_config))
        .route("/api/config/validate", post(validate_raw_config))
        .route("/api/settings", get(get_settings).post(update_settings))
        .route("/api/live/:port/chunk", get(live_chunk))
        .route("/api/recordings", get(list_recordings))
        .route("/api/recordings/open", post(open_recordings_folder))
        .route("/api/recordings/*relpath", delete(delete_recording))
        .route("/audio/*relpath", get(serve_audio))
        .route("/api/process/status", get(process_status))
        .route("/api/process/logs", get(process_logs))
        .route("/api/process/start", post(process_start))
        .route("/api/process/stop", post(process_stop))
        .route("/api/process/restart", post(process_restart))
        .with_state(app)
}

/// Binds 127.0.0.1 on `port` (0 = any free port) and serves until the process exits.
/// Returns the port actually bound.
pub async fn serve(app: Shared, port: u16) -> std::io::Result<u16> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let bound = listener.local_addr()?.port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(app)).await;
    });
    Ok(bound)
}

#[cfg(test)]
mod tests;
