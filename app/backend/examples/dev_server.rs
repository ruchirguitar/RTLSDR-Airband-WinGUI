//! Runs the backend and supervisor against the repo's UI, for trying the app in
//! a normal browser without Tauri:
//!   cargo run -p airband-backend --example dev_server [path to rtl_airband.exe]
//! Data goes to %TEMP%\frequency-recorder-dev.
use airband_backend::{AppState, Paths};
use std::path::Path;

#[tokio::main]
async fn main() {
    let app_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let data = std::env::temp_dir().join("frequency-recorder-dev");
    let engine = std::env::args().nth(1).unwrap_or_else(|| app_dir.join("src-tauri/engine-bin/rtl_airband.exe").to_string_lossy().into_owned());
    let state = AppState::new(Paths {
        ui_dir: app_dir.join("ui"),
        data_dir: data.clone(),
        default_recordings_dir: data.join("recordings"),
        engine_binary: engine,
        autostart_exe: None,
    });
    state.start_supervisor();
    // PORT lets it run beside the installed app, which uses 5050
    let wanted = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(5050);
    let port = airband_backend::serve(state, wanted).await.unwrap();
    println!("http://127.0.0.1:{port}/  (data in {})", data.display());
    std::future::pending::<()>().await;
}
