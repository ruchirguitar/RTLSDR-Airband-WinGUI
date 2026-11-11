//! HTTP backend for Frequency Recorder. The UI in app/ui talks only to this
//! API; the app writes the engine config itself from the user's station
//! (station.rs) and a supervisor (supervisor.rs) keeps the engine running.

pub mod libconfig;
pub mod live;
pub mod process;
pub mod recordings;
pub mod spectrum;
pub mod station;
pub mod supervisor;
mod system;

use axum::body::Body;
use axum::extract::{Path as UrlPath, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use station::{Station, LIVE_PORT_BASE};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt as _;
use tower_http::services::{ServeDir, ServeFile};

pub const LIVE_PORT_MAX: u16 = LIVE_PORT_BASE + 999;
/// The engine's audio rate in an NFM build (WAVE_RATE).
pub const LIVE_SAMPLE_RATE: u32 = 16000;
/// The engine reports "starting" until it has run this long.
const STARTING_FOR: Duration = Duration::from_secs(4);

/// Where things live on disk. The desktop shell decides these per platform.
#[derive(Clone, Debug)]
pub struct Paths {
    pub ui_dir: PathBuf,
    /// station.json, state.json and the generated engine.conf.
    pub data_dir: PathBuf,
    pub default_recordings_dir: PathBuf,
    pub engine_binary: String,
    /// The app's own executable, for "Start with Windows". None where that makes no sense (dev server).
    pub autostart_exe: Option<PathBuf>,
}

pub struct AppState {
    pub paths: Paths,
    pub process: process::ProcessManager,
    pub live: live::LiveAudioManager,
    pub spectrum: spectrum::SpectrumFeed,
    pub supervisor: Mutex<supervisor::SupervisorState>,
    station: Mutex<Station>,
    recording_enabled: Mutex<bool>,
}

type Shared = Arc<AppState>;

struct ApiError(StatusCode, Value);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, json!({"error": msg.into()}))
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, json!({"error": "Not found"}))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": e.to_string()}))
}

type ApiResult = Result<Json<Value>, ApiError>;

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value).unwrap())?;
    std::fs::rename(tmp, path)
}

#[derive(serde::Serialize, Deserialize)]
#[serde(default)]
struct Prefs {
    recording_enabled: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { recording_enabled: true }
    }
}

impl AppState {
    pub fn new(paths: Paths) -> Shared {
        let station: Station = read_json(&paths.data_dir.join("station.json"));
        let prefs: Prefs = read_json(&paths.data_dir.join("state.json"));
        Arc::new(AppState {
            paths,
            process: process::ProcessManager::new(),
            live: live::LiveAudioManager::default(),
            spectrum: Default::default(),
            supervisor: Mutex::new(Default::default()),
            station: Mutex::new(station),
            recording_enabled: Mutex::new(prefs.recording_enabled),
        })
    }

    pub fn station(&self) -> Station {
        self.station.lock().unwrap().clone()
    }

    pub fn recording_enabled(&self) -> bool {
        *self.recording_enabled.lock().unwrap()
    }

    pub fn set_recording_enabled(&self, enabled: bool) -> std::io::Result<()> {
        *self.recording_enabled.lock().unwrap() = enabled;
        write_json(&self.paths.data_dir.join("state.json"), &Prefs { recording_enabled: enabled })
    }

    pub fn recordings_dir(&self) -> PathBuf {
        let s = self.station.lock().unwrap();
        if s.settings.recordings_dir.trim().is_empty() {
            self.paths.default_recordings_dir.clone()
        } else {
            PathBuf::from(s.settings.recordings_dir.trim())
        }
    }

    /// Makes sure the recordings folder exists, so it can be opened before anything is recorded.
    pub fn ensure_recordings_dir(&self) -> std::io::Result<PathBuf> {
        let dir = self.recordings_dir();
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Starts the background supervisor. Call once, inside a tokio runtime.
    pub fn start_supervisor(self: &Arc<Self>) {
        let _ = self.ensure_recordings_dir();
        self.spectrum.ensure_listening(station::SPECTRUM_PORT);
        supervisor::spawn(self.clone());
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
            return Err(ApiError(StatusCode::FORBIDDEN, json!({"error": "Path escapes the recordings folder"})));
        }
        out.push(part);
    }
    Ok(out)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(internal)
}

async fn get_station(State(app): State<Shared>) -> ApiResult {
    Ok(Json(json!({
        "station": app.station(),
        "ctcss_tones": station::CTCSS_TONES.to_vec(),
        "default_recordings_dir": app.paths.default_recordings_dir,
        "autostart_available": app.paths.autostart_exe.is_some(),
    })))
}

fn fill_ids(s: &mut Station) {
    let stamp = supervisor::now_unix() as u64;
    for (i, f) in s.frequencies.iter_mut().enumerate().filter(|(_, f)| f.id.is_empty()) {
        f.id = format!("f{stamp}{i}");
    }
    for (i, n) in s.nets.iter_mut().enumerate().filter(|(_, n)| n.id.is_empty()) {
        n.id = format!("n{stamp}{i}");
    }
}

async fn put_station(State(app): State<Shared>, Json(mut new): Json<Station>) -> ApiResult {
    fill_ids(&mut new);
    let errors = new.validate();
    if !errors.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, json!({"error": errors[0], "errors": errors})));
    }
    let old = app.station();
    if new.settings.autostart != old.settings.autostart {
        let exe = app.paths.autostart_exe.clone().ok_or_else(|| bad_request("Starting with Windows only works in the installed app."))?;
        let enabled = new.settings.autostart;
        blocking(move || system::set_autostart(&exe, enabled)).await?.map_err(bad_request)?;
    }
    write_json(&app.paths.data_dir.join("station.json"), &new).map_err(internal)?;
    *app.station.lock().unwrap() = new.clone();
    let _ = app.ensure_recordings_dir();
    Ok(Json(json!({"station": new})))
}

#[derive(Deserialize)]
struct PlanBody {
    mhz: Vec<f64>,
}

async fn post_plan(Json(body): Json<PlanBody>) -> ApiResult {
    Ok(Json(json!({"plan": station::plan(&body.mhz)})))
}

async fn get_status(State(app): State<Shared>) -> ApiResult {
    let s = app.station();
    let sup = app.supervisor.lock().unwrap().clone();
    let enabled = app.recording_enabled();
    let running = app.process.status().running;
    let now = supervisor::now_unix();

    let state = if s.frequencies.is_empty() {
        "setup"
    } else if running && sup.engine_started.is_some_and(|t| t.elapsed() >= STARTING_FOR) {
        if enabled {
            "recording"
        } else {
            "monitoring"
        }
    } else if running {
        "starting"
    } else {
        "waiting"
    };

    let active_nets: Vec<&station::Net> = s.nets.iter().filter(|n| sup.active_nets.contains(&n.id)).collect();
    let frequencies: Vec<Value> = s
        .frequencies
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let port = LIVE_PORT_BASE + i as u16;
            let a = app.live.get(port).activity();
            let net = active_nets.iter().find(|n| (n.mhz - f.mhz).abs() < 0.0005).map(|n| n.name.clone());
            json!({"id": f.id, "mhz": f.mhz, "name": f.name, "port": port, "on_air": a.on_air && running, "level_dbfs": a.level_dbfs, "last_heard_unix": a.last_heard_unix, "net": net})
        })
        .collect();

    let mut warnings = Vec::new();
    let hours = s.settings.silence_warning_hours;
    if (state == "recording" || state == "monitoring") && hours > 0 {
        let reference = sup.last_heard_any.unwrap_or(0.0).max(sup.watching_since_unix.unwrap_or(now));
        if now - reference > hours as f64 * 3600.0 {
            warnings.push(format!("Nothing has been heard on any frequency for over {hours} hours. Check the antenna and the dongle."));
        }
    }
    let dir = app.recordings_dir();
    let mhz: Vec<f64> = s.frequencies.iter().map(|f| f.mhz).collect();
    Ok(Json(json!({
        "recording_enabled": enabled,
        "state": state,
        "problem": sup.problem,
        "started_unix": if running { sup.engine_started_unix } else { None },
        "restarts": sup.restarts,
        "plan": station::plan(&mhz),
        "frequencies": frequencies,
        "active_nets": active_nets.iter().map(|n| &n.name).collect::<Vec<_>>(),
        "warnings": warnings,
        "recordings_dir": dir,
        "recordings_dir_exists": dir.is_dir(),
        "disk": {"used_bytes": sup.disk_used, "free_bytes": system::free_space(&dir)},
        "live_sample_rate": LIVE_SAMPLE_RATE,
    })))
}

#[derive(Deserialize)]
struct RecordingBody {
    enabled: bool,
}

async fn post_recording(State(app): State<Shared>, Json(body): Json<RecordingBody>) -> ApiResult {
    app.set_recording_enabled(body.enabled).map_err(internal)?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct RecordingsQuery {
    /// Unix time range, inclusive start.
    from: Option<f64>,
    to: Option<f64>,
    limit: Option<usize>,
}

async fn list_recordings(State(app): State<Shared>, Query(q): Query<RecordingsQuery>) -> ApiResult {
    let base = app.recordings_dir();
    let b = base.clone();
    let mut recs = blocking(move || recordings::scan(&b)).await?;
    recs.retain(|r| q.from.map_or(true, |f| r.start_unix >= f) && q.to.map_or(true, |t| r.start_unix < t));
    if let Some(limit) = q.limit {
        recs.truncate(limit);
    }
    Ok(Json(json!({"base_dir": base, "recordings": recs})))
}

async fn delete_recording(State(app): State<Shared>, UrlPath(relpath): UrlPath<String>) -> ApiResult {
    let full = safe_subpath(&app.recordings_dir(), &relpath)?;
    if !full.is_file() || full.extension().and_then(|e| e.to_str()) != Some("mp3") {
        return Err(not_found());
    }
    std::fs::remove_file(&full).map_err(internal)?;
    let _ = std::fs::remove_file(recordings::segment_log_for(&full));
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RevealBody {
    path: Option<String>,
}

async fn reveal(State(app): State<Shared>, body: Option<Json<RevealBody>>) -> ApiResult {
    let base = app.ensure_recordings_dir().map_err(internal)?;
    let target = match body.and_then(|b| b.0.path) {
        Some(rel) => {
            let full = safe_subpath(&base, &rel)?;
            if !full.exists() {
                return Err(not_found());
            }
            Some(full)
        }
        None => None,
    };
    system::reveal(&base, target.as_deref()).map_err(internal)?;
    Ok(Json(json!({"ok": true})))
}

async fn pick_folder(State(app): State<Shared>) -> ApiResult {
    let start = app.recordings_dir();
    let picked = blocking(move || system::pick_folder(&start)).await?.map_err(bad_request)?;
    Ok(Json(json!({"path": picked})))
}

async fn serve_audio(State(app): State<Shared>, UrlPath(relpath): UrlPath<String>, req: Request) -> Result<Response, ApiError> {
    let full = safe_subpath(&app.recordings_dir(), &relpath)?;
    if !full.is_file() {
        return Err(not_found());
    }
    ServeFile::new(full).oneshot(req).await.map(|r| r.map(Body::new)).map_err(internal)
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

/// New waterfall lines since `since`, as one base64 block of little-endian f32 dB values.
async fn get_spectrum(State(app): State<Shared>, Query(q): Query<Since>) -> ApiResult {
    if let Some(err) = app.spectrum.ensure_listening(station::SPECTRUM_PORT) {
        return Err(bad_request(format!("Could not listen for the spectrum: {err}")));
    }
    let u = app.spectrum.since(q.since);
    let last = u.lines.last();
    let bytes: Vec<u8> = u.lines.iter().flat_map(|l| l.db.iter().flat_map(|v| v.to_le_bytes())).collect();
    Ok(Json(json!({
        "next": u.next,
        "live": u.live,
        "count": u.lines.len(),
        "bins": last.map(|l| l.db.len()),
        "centre_hz": last.map(|l| l.centre_hz),
        "sample_rate": last.map(|l| l.sample_rate),
        "channels": last.map(|l| l.channels.clone()).unwrap_or_default(),
        "data": base64::engine::general_purpose::STANDARD.encode(bytes),
    })))
}

async fn engine_log(State(app): State<Shared>, Query(q): Query<Since>) -> ApiResult {
    let since = q.since as usize;
    let lines = app.process.logs(since);
    let next = since + lines.len();
    let config = std::fs::read_to_string(app.paths.data_dir.join("engine.conf")).unwrap_or_default();
    Ok(Json(json!({"lines": lines, "next": next, "config": config, "binary": app.paths.engine_binary})))
}

pub fn router(app: Shared) -> Router {
    let index = app.paths.ui_dir.join("index.html");
    // the UI changes with app updates: always revalidate instead of running a cached copy
    let no_cache = tower_http::set_header::SetResponseHeaderLayer::overriding(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-cache"));
    let ui = Router::new().route_service("/", ServeFile::new(index)).nest_service("/static", ServeDir::new(app.paths.ui_dir.join("static"))).layer(no_cache);
    Router::new()
        .merge(ui)
        .route("/api/station", get(get_station).put(put_station))
        .route("/api/plan", post(post_plan))
        .route("/api/status", get(get_status))
        .route("/api/recording", post(post_recording))
        .route("/api/recordings", get(list_recordings))
        .route("/api/recordings/*relpath", delete(delete_recording))
        .route("/api/reveal", post(reveal))
        .route("/api/pick-folder", post(pick_folder))
        .route("/api/live/:port/chunk", get(live_chunk))
        .route("/api/engine/log", get(engine_log))
        .route("/api/spectrum", get(get_spectrum))
        .route("/audio/*relpath", get(serve_audio))
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
