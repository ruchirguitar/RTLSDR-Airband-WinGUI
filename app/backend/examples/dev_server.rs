//! Runs the backend against the repo's UI and sample configs, for trying the UI
//! in a normal browser without Tauri: `cargo run -p airband-backend --example dev_server`
use airband_backend::{AppState, Paths};
use std::path::Path;

#[tokio::main]
async fn main() {
    let app_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let data = std::env::temp_dir().join("airband-dev");
    let configs = data.join("configs");
    std::fs::create_dir_all(&configs).unwrap();
    for entry in std::fs::read_dir(app_dir.join("configs")).unwrap().flatten() {
        let target = configs.join(entry.file_name());
        if !target.exists() {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
    let state = AppState::new(Paths {
        ui_dir: app_dir.join("ui"),
        configs_dir: configs,
        state_path: data.join("state.json"),
        default_recordings_dir: data.join("recordings"),
        default_binary: "rtl_airband".into(),
    });
    let port = airband_backend::serve(state, 5050).await.unwrap();
    println!("http://127.0.0.1:{port}/");
    std::future::pending::<()>().await;
}
