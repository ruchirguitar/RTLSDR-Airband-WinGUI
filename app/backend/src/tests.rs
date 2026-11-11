use super::*;
use axum::http::Request;
use http_body_util::BodyExt;

struct Fixture {
    _dir: tempfile::TempDir,
    app: Shared,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let configs = dir.path().join("configs");
    std::fs::create_dir_all(&configs).unwrap();
    std::fs::write(configs.join("basic_multichannel.conf"), "devices: ( { type = \"rtlsdr\"; centerfreq = 145.0; } );\n").unwrap();
    let ui = dir.path().join("ui");
    std::fs::create_dir_all(ui.join("templates")).unwrap();
    std::fs::create_dir_all(ui.join("static")).unwrap();
    std::fs::write(ui.join("templates/index.html"), "<html>hi</html>").unwrap();
    std::fs::write(ui.join("static/app.js"), "// js").unwrap();
    let app = AppState::new(Paths {
        ui_dir: ui,
        configs_dir: configs,
        state_path: dir.path().join("state.json"),
        default_recordings_dir: dir.path().join("rec"),
        default_binary: "rtl_airband".into(),
    });
    Fixture { _dir: dir, app }
}

async fn call(app: &Shared, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let resp = router(app.clone()).oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    (status, resp.into_body().collect().await.unwrap().to_bytes().to_vec())
}

async fn call_json(app: &Shared, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let (s, b) = call(app, method, uri, body).await;
    (s, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

#[tokio::test]
async fn serves_ui() {
    let f = fixture();
    assert_eq!(call(&f.app, "GET", "/", None).await, (StatusCode::OK, b"<html>hi</html>".to_vec()));
    assert_eq!(call(&f.app, "GET", "/static/app.js", None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn config_lifecycle() {
    let f = fixture();
    let (s, v) = call_json(&f.app, "GET", "/api/configs", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["files"], json!(["basic_multichannel.conf"]));
    assert_eq!(v["active"], json!("basic_multichannel.conf"));

    let (_, v) = call_json(&f.app, "GET", "/api/config", None).await;
    assert_eq!(v["data"]["devices"][0]["centerfreq"]["type"], json!("float"));

    let mut data = v["data"].clone();
    data["devices"][0]["gain"] = libconfig::scalar("int", json!(25));
    let (s, v) = call_json(&f.app, "POST", "/api/config", Some(json!({"data": data}))).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["raw"].as_str().unwrap().contains("gain = 25;"));

    let (s, _) = call_json(&f.app, "POST", "/api/configs", Some(json!({"filename": "net"}))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call_json(&f.app, "POST", "/api/configs", Some(json!({"filename": "net"}))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call_json(&f.app, "POST", "/api/configs", Some(json!({"filename": "../evil"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    call_json(&f.app, "POST", "/api/configs/select", Some(json!({"filename": "net.conf"}))).await;
    let (_, v) = call_json(&f.app, "GET", "/api/config", None).await;
    assert_eq!(v["filename"], json!("net.conf"));
}

#[tokio::test]
async fn raw_config_is_validated_before_saving() {
    let f = fixture();
    let (s, v) = call_json(&f.app, "POST", "/api/config/raw", Some(json!({"raw": "a = ;"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().starts_with("Syntax error"));
    let (_, v) = call_json(&f.app, "POST", "/api/config/validate", Some(json!({"raw": "a = 1;"}))).await;
    assert_eq!(v["ok"], json!(true));
    let (s, _) = call_json(&f.app, "POST", "/api/config/raw", Some(json!({"raw": "a = 1;"}))).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn settings_persist() {
    let f = fixture();
    call_json(&f.app, "POST", "/api/settings", Some(json!({"live_sample_rate": 8000, "binary_path": "x.exe"}))).await;
    let (_, v) = call_json(&f.app, "GET", "/api/settings", None).await;
    assert_eq!(v["live_sample_rate"], json!(8000));
    assert_eq!(v["binary_path"], json!("x.exe"));
    assert_eq!(v["live_port_base"], json!(LIVE_PORT_BASE));
}

#[tokio::test]
async fn recordings_list_serve_delete() {
    let f = fixture();
    let rec = f.app.recordings_dir();
    std::fs::create_dir_all(rec.join("145.400/2026/09/30")).unwrap();
    std::fs::write(rec.join("145.400/2026/09/30/a.mp3"), b"ID3").unwrap();
    std::fs::write(rec.join("notes.txt"), b"x").unwrap();

    let (_, v) = call_json(&f.app, "GET", "/api/recordings", None).await;
    assert_eq!(v["exists"], json!(true));
    let recs = v["recordings"].as_array().unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["path"], json!("145.400/2026/09/30/a.mp3"));

    assert_eq!(call(&f.app, "GET", "/audio/145.400/2026/09/30/a.mp3", None).await, (StatusCode::OK, b"ID3".to_vec()));
    assert_eq!(call(&f.app, "GET", "/audio/..%2Fstate.json", None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&f.app, "DELETE", "/api/recordings/../state.json", None).await.0, StatusCode::FORBIDDEN);

    let (s, _) = call_json(&f.app, "DELETE", "/api/recordings/145.400/2026/09/30/a.mp3", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!rec.join("145.400/2026/09/30/a.mp3").exists());
}

#[tokio::test]
async fn live_port_must_be_in_range() {
    let f = fixture();
    assert_eq!(call(&f.app, "GET", "/api/live/80/chunk", None).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn process_start_reports_missing_binary() {
    let f = fixture();
    call_json(&f.app, "POST", "/api/settings", Some(json!({"binary_path": "/definitely/not/here"}))).await;
    let (s, v) = call_json(&f.app, "POST", "/api/process/start", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("Could not start"));
    let (_, v) = call_json(&f.app, "GET", "/api/process/status", None).await;
    assert_eq!(v["running"], json!(false));
}
