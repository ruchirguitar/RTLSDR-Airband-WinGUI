//! Access from other devices on the local network. The app's own window uses
//! 127.0.0.1 and needs no password; when the user allows it, a second listener
//! on all interfaces (REMOTE_PORT) serves the same app behind a password.

use crate::{AppState, Shared};
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const REMOTE_PORT: u16 = 5055;
const COOKIE: &str = "fr_session";
const SESSION_FOR: Duration = Duration::from_secs(30 * 24 * 3600);
const HASH_ROUNDS: u32 = 100_000;
const MIN_PASSWORD_LEN: usize = 6;
/// Wrong passwords allowed per minute, across all devices.
const MAX_FAILURES_PER_MIN: usize = 10;

/// Who is asking: put on each request by the remote listener's guard.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Client {
    pub remote: bool,
    pub read_only: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    /// "salt$hash", both hex.
    password: String,
}

#[derive(Default)]
pub struct RemoteAccess {
    password: Mutex<String>,
    sessions: Mutex<HashMap<String, Instant>>,
    failures: Mutex<Vec<Instant>>,
    running: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    pub error: Mutex<Option<String>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).expect("system random numbers");
    hex(&buf)
}

fn derive(salt: &str, password: &str) -> String {
    let mut h: [u8; 32] = Sha256::digest(format!("{salt}:{password}").as_bytes()).into();
    for _ in 0..HASH_ROUNDS {
        let mut d = Sha256::new();
        d.update(h);
        d.update(salt.as_bytes());
        h = d.finalize().into();
    }
    hex(&h)
}

pub fn hash_password(password: &str) -> String {
    let salt = random_hex(16);
    format!("{salt}${}", derive(&salt, password))
}

pub fn verify_password(stored: &str, password: &str) -> bool {
    let Some((salt, hash)) = stored.split_once('$') else { return false };
    let got = derive(salt, password);
    // compare without stopping at the first difference
    got.len() == hash.len() && got.bytes().zip(hash.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

impl RemoteAccess {
    pub fn load(data_dir: &Path) -> Self {
        let stored: Stored = std::fs::read_to_string(data_dir.join("remote.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        RemoteAccess { password: Mutex::new(stored.password), ..Default::default() }
    }

    pub fn has_password(&self) -> bool {
        !self.password.lock().unwrap().is_empty()
    }

    /// Sets a new password and signs every device out.
    pub fn set_password(&self, data_dir: &Path, password: &str) -> Result<(), String> {
        if password.chars().count() < MIN_PASSWORD_LEN {
            return Err(format!("Use at least {MIN_PASSWORD_LEN} characters."));
        }
        let stored = Stored { password: hash_password(password) };
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        std::fs::write(data_dir.join("remote.json"), serde_json::to_string_pretty(&stored).unwrap()).map_err(|e| e.to_string())?;
        *self.password.lock().unwrap() = stored.password;
        self.sessions.lock().unwrap().clear();
        Ok(())
    }

    fn new_session(&self) -> String {
        let token = random_hex(32);
        let mut s = self.sessions.lock().unwrap();
        s.retain(|_, t| t.elapsed() < SESSION_FOR);
        s.insert(token.clone(), Instant::now());
        token
    }

    fn session_valid(&self, token: &str) -> bool {
        self.sessions.lock().unwrap().get(token).is_some_and(|t| t.elapsed() < SESSION_FOR)
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }
}

/// This PC's address on the local network (no packet is sent).
pub fn lan_ip() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    sock.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
}

fn session_cookie(req: &Request) -> Option<String> {
    req.headers().get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")).map(str::to_string))
}

fn refuse(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({"error": msg}))).into_response()
}

/// Lets signed-in devices through, and only to what a remote device may do.
async fn guard(State(app): State<Shared>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let signed_in = session_cookie(&req).is_some_and(|t| app.remote.session_valid(&t));
    if !signed_in {
        return if path.starts_with("/api/") || path.starts_with("/audio/") { refuse(StatusCode::UNAUTHORIZED, "Sign in first.") } else { Redirect::to("/login").into_response() };
    }
    // these act on the PC running the app (Explorer, folder picker) or on remote access itself
    if path == "/api/reveal" || path == "/api/pick-folder" || path.starts_with("/api/remote") {
        return refuse(StatusCode::FORBIDDEN, "That only works on the PC running Frequency Recorder.");
    }
    let read_only = app.station().settings.remote_read_only;
    let changes = !matches!(*req.method(), Method::GET | Method::HEAD) && path != "/api/plan";
    if read_only && changes {
        return refuse(StatusCode::FORBIDDEN, "This device can watch and listen, but not change anything.");
    }
    req.extensions_mut().insert(Client { remote: true, read_only });
    next.run(req).await
}

#[derive(Deserialize)]
struct LoginBody {
    password: String,
}

async fn login(State(app): State<Shared>, Json(body): Json<LoginBody>) -> Response {
    {
        let mut f = app.remote.failures.lock().unwrap();
        f.retain(|t| t.elapsed() < Duration::from_secs(60));
        if f.len() >= MAX_FAILURES_PER_MIN {
            return refuse(StatusCode::TOO_MANY_REQUESTS, "Too many wrong passwords. Wait a minute and try again.");
        }
    }
    let stored = app.remote.password.lock().unwrap().clone();
    let ok = !stored.is_empty() && tokio::task::spawn_blocking(move || verify_password(&stored, &body.password)).await.unwrap_or(false);
    if !ok {
        app.remote.failures.lock().unwrap().push(Instant::now());
        tokio::time::sleep(Duration::from_millis(800)).await;
        return refuse(StatusCode::UNAUTHORIZED, "Wrong password.");
    }
    let token = app.remote.new_session();
    let cookie = format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", SESSION_FOR.as_secs());
    ([(header::SET_COOKIE, cookie)], Json(json!({"ok": true}))).into_response()
}

async fn logout(State(app): State<Shared>, req: Request) -> Response {
    if let Some(t) = session_cookie(&req) {
        app.remote.sessions.lock().unwrap().remove(&t);
    }
    ([(header::SET_COOKIE, format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"))], Json(json!({"ok": true}))).into_response()
}

async fn login_page() -> Html<&'static str> {
    Html(LOGIN_HTML)
}

/// The app as other devices see it: the sign-in page, then everything behind the guard.
pub fn remote_router(app: Shared) -> Router {
    let open = Router::new().route("/login", get(login_page)).route("/api/login", post(login)).route("/api/logout", post(logout)).with_state(app.clone());
    let guarded = crate::router(app.clone()).layer(middleware::from_fn_with_state(app, guard));
    open.merge(guarded)
}

/// Starts or stops the network listener to match the settings. Called by the supervisor.
pub async fn reconcile(app: &Shared) {
    let want = app.station().settings.remote_access && app.remote.has_password();
    let running = app.remote.is_running();
    if want && !running {
        match tokio::net::TcpListener::bind(("0.0.0.0", REMOTE_PORT)).await {
            Ok(listener) => {
                let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
                let router = remote_router(app.clone());
                tokio::spawn(async move {
                    let _ = axum::serve(listener, router).with_graceful_shutdown(async { let _ = stopped.await; }).await;
                });
                *app.remote.running.lock().unwrap() = Some(stop);
                *app.remote.error.lock().unwrap() = None;
            }
            Err(e) => *app.remote.error.lock().unwrap() = Some(format!("Could not open port {REMOTE_PORT} for other devices: {e}")),
        }
    } else if !want && running {
        if let Some(stop) = app.remote.running.lock().unwrap().take() {
            let _ = stop.send(());
        }
        app.remote.sessions.lock().unwrap().clear();
        *app.remote.error.lock().unwrap() = None;
    }
}

/// What Settings shows about remote access.
pub fn status(app: &AppState) -> serde_json::Value {
    let url = lan_ip().map(|ip| format!("http://{ip}:{REMOTE_PORT}/"));
    json!({
        "enabled": app.station().settings.remote_access,
        "has_password": app.remote.has_password(),
        "running": app.remote.is_running(),
        "port": REMOTE_PORT,
        "url": url,
        "error": app.remote.error.lock().unwrap().clone(),
    })
}

const LOGIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Sign in · Frequency Recorder</title>
<style>
:root { --bg:#0b0f14; --panel:#121821; --line:#2b3947; --text:#e6edf3; --muted:#8b9aab; --amber:#ffb020; --red:#ff7a6b; color-scheme: dark; }
@media (prefers-color-scheme: light) { :root { --bg:#f3f5f8; --panel:#fff; --line:#c3cdd8; --text:#16202b; --muted:#5a6878; --amber:#e0900a; --red:#c4372b; color-scheme: light; } }
body { margin:0; min-height:100vh; display:flex; align-items:center; justify-content:center; background:var(--bg); color:var(--text); font:14px/1.45 "Segoe UI", system-ui, sans-serif; }
form { width:min(360px, calc(100vw - 32px)); padding:28px; border:1px solid var(--line); border-radius:12px; background:var(--panel); display:flex; flex-direction:column; gap:14px; }
h1 { margin:0; font-size:20px; } p { margin:0; color:var(--muted); }
input { height:42px; padding:0 12px; border-radius:8px; border:1px solid var(--line); background:var(--bg); color:var(--text); font:inherit; }
button { height:42px; border:0; border-radius:8px; background:var(--amber); color:#0b0f14; font:inherit; font-weight:600; cursor:pointer; }
.err { color:var(--red); min-height:1.4em; }
</style></head><body>
<form id="f"><h1>Frequency Recorder</h1><p>Enter the password set in Settings on the PC that runs it.</p>
<label for="pw" style="position:absolute;left:-9999px">Password</label>
<input id="pw" type="password" autocomplete="current-password" placeholder="Password" required autofocus>
<button type="submit">Sign in</button><div class="err" id="err" role="alert"></div></form>
<script>
document.getElementById("f").addEventListener("submit", async (e) => {
  e.preventDefault();
  const err = document.getElementById("err");
  err.textContent = "";
  try {
    const r = await fetch("/api/login", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ password: document.getElementById("pw").value }) });
    if (r.ok) { location.href = "/"; return; }
    err.textContent = (await r.json().catch(() => ({}))).error || "Could not sign in.";
  } catch { err.textContent = "Can't reach Frequency Recorder."; }
});
</script></body></html>"#;
