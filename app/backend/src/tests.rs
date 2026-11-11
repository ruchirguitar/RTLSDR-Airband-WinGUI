use super::*;
use axum::http::Request;
use http_body_util::BodyExt;

struct Fixture {
    dir: tempfile::TempDir,
    app: Shared,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let ui = dir.path().join("ui");
    std::fs::create_dir_all(ui.join("static")).unwrap();
    std::fs::write(ui.join("index.html"), "<html>hi</html>").unwrap();
    std::fs::write(ui.join("static/app.js"), "// js").unwrap();
    let app = AppState::new(Paths {
        ui_dir: ui,
        data_dir: dir.path().join("data"),
        default_recordings_dir: dir.path().join("rec"),
        engine_binary: dir.path().join("no-such-engine.exe").to_string_lossy().into_owned(),
        autostart_exe: None,
    });
    Fixture { dir, app }
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

fn two_repeaters() -> Value {
    json!({
        "frequencies": [
            {"mhz": 145.4, "name": "Repeater A"},
            {"mhz": 145.2, "name": "Repeater B", "squelch": 6}
        ],
        "nets": [{"name": "Morning net", "mhz": 145.4, "start": "07:00", "end": "08:00"}]
    })
}

#[tokio::test]
async fn serves_ui() {
    let f = fixture();
    assert_eq!(call(&f.app, "GET", "/", None).await, (StatusCode::OK, b"<html>hi</html>".to_vec()));
    assert_eq!(call(&f.app, "GET", "/static/app.js", None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn station_starts_empty_with_defaults() {
    let f = fixture();
    let (s, v) = call_json(&f.app, "GET", "/api/station", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["station"]["frequencies"], json!([]));
    assert_eq!(v["station"]["recording"]["net_join_sec"], 60.0);
    assert_eq!(v["station"]["settings"]["retention_days"], 90);
    assert_eq!(v["autostart_available"], false);
    assert!(v["ctcss_tones"].as_array().unwrap().len() >= 50);
}

#[tokio::test]
async fn station_is_validated_saved_and_reloaded() {
    let f = fixture();
    let (s, v) = call_json(&f.app, "PUT", "/api/station", Some(two_repeaters())).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let id = v["station"]["frequencies"][0]["id"].as_str().unwrap().to_string();
    assert!(!id.is_empty(), "ids are filled in");
    assert_eq!(v["station"]["frequencies"][1]["squelch"], 6);
    assert_eq!(v["station"]["frequencies"][0]["squelch"], 5, "missing fields get defaults");

    let again = AppState::new(f.app.paths.clone());
    assert_eq!(again.station().frequencies[0].id, id, "persisted to station.json");

    let mut bad = two_repeaters();
    bad["frequencies"][1]["mhz"] = json!(150.0);
    let (s, v) = call_json(&f.app, "PUT", "/api/station", Some(bad)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("too far"), "{v}");
    assert_eq!(f.app.station().frequencies[1].mhz, 145.2, "a rejected station is not saved");
}

#[tokio::test]
async fn autostart_needs_the_installed_app() {
    let f = fixture();
    let (s, v) = call_json(&f.app, "PUT", "/api/station", Some(json!({"settings": {"autostart": true}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("installed app"));
}

#[tokio::test]
async fn plan_endpoint() {
    let f = fixture();
    let (_, v) = call_json(&f.app, "POST", "/api/plan", Some(json!({"mhz": [145.2, 145.4]}))).await;
    assert_eq!(v["plan"]["centre_mhz"], 145.3);
    assert_eq!(v["plan"]["fits"], json!(["good", "good"]));
    let (_, v) = call_json(&f.app, "POST", "/api/plan", Some(json!({"mhz": []}))).await;
    assert_eq!(v["plan"], Value::Null);
}

#[tokio::test]
async fn status_and_recording_switch() {
    let f = fixture();
    let (_, v) = call_json(&f.app, "GET", "/api/status", None).await;
    assert_eq!(v["state"], "setup");
    assert_eq!(v["recording_enabled"], true);

    call_json(&f.app, "PUT", "/api/station", Some(two_repeaters())).await;
    let (_, v) = call_json(&f.app, "GET", "/api/status", None).await;
    assert_eq!(v["state"], "waiting", "engine not running yet");
    assert_eq!(v["frequencies"][1]["port"], 17301);
    assert_eq!(v["frequencies"][0]["on_air"], false);
    assert_eq!(v["plan"]["centre_mhz"], 145.3);
    assert_eq!(v["recordings_dir_exists"], true, "created when the station is saved");

    call_json(&f.app, "POST", "/api/recording", Some(json!({"enabled": false}))).await;
    let (_, v) = call_json(&f.app, "GET", "/api/status", None).await;
    assert_eq!(v["state"], "off");
    assert!(!AppState::new(f.app.paths.clone()).recording_enabled(), "persisted");
}

#[tokio::test]
async fn supervisor_reports_a_missing_engine() {
    let f = fixture();
    call_json(&f.app, "PUT", "/api/station", Some(two_repeaters())).await;
    let app = f.app.clone();
    tokio::task::spawn_blocking(move || supervisor::tick(&app, chrono::Local::now().naive_local())).await.unwrap();
    let (_, v) = call_json(&f.app, "GET", "/api/status", None).await;
    assert_eq!(v["state"], "waiting");
    assert!(v["problem"].as_str().unwrap().contains("Could not start"), "{v}");
    let conf = std::fs::read_to_string(f.dir.path().join("data/engine.conf")).unwrap();
    assert!(conf.contains("centerfreq = 145.3;"), "{conf}");
    assert!(f.dir.path().join("rec/Nets").is_dir());
}

#[tokio::test]
async fn recordings_list_serve_delete() {
    let f = fixture();
    let clip = f.dir.path().join("rec/145.400/2026/11/12/145.400_20261112_070201.mp3");
    std::fs::create_dir_all(clip.parent().unwrap()).unwrap();
    std::fs::write(&clip, b"ID3").unwrap();
    std::fs::write(clip.with_file_name("145.400_20261112_070201.segments.tsv"), "start_sec\tend_sec\tstart_unix\tstart_utc\tfrequency_hz\n0.000\t2.000\t1794431093.0\tx\t145400000\n").unwrap();

    let (_, v) = call_json(&f.app, "GET", "/api/recordings", None).await;
    let recs = v["recordings"].as_array().unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["freq"], "145.400");
    assert_eq!(recs[0]["duration_sec"], 2.0);
    let (_, v) = call_json(&f.app, "GET", "/api/recordings?from=1794431094", None).await;
    assert_eq!(v["recordings"].as_array().unwrap().len(), 0);

    let path = "145.400/2026/11/12/145.400_20261112_070201.mp3";
    assert_eq!(call(&f.app, "GET", &format!("/audio/{path}"), None).await, (StatusCode::OK, b"ID3".to_vec()));
    assert_eq!(call(&f.app, "GET", "/audio/..%2F..%2Fsecret", None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(call(&f.app, "DELETE", &format!("/api/recordings/{path}"), None).await.0, StatusCode::OK);
    assert!(!clip.exists());
    assert!(!clip.with_file_name("145.400_20261112_070201.segments.tsv").exists());
    assert_eq!(call(&f.app, "DELETE", &format!("/api/recordings/{path}"), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn live_port_must_be_in_range() {
    let f = fixture();
    assert_eq!(call(&f.app, "GET", "/api/live/80/chunk", None).await.0, StatusCode::BAD_REQUEST);
}
