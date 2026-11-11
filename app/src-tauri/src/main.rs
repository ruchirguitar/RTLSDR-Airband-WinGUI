// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use airband_backend::{AppState, Paths};
use std::path::Path;
use std::time::Duration;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

/// A fixed port keeps the page origin stable, so per-browser settings such as
/// the theme survive restarts. Falls back to any free port if it is taken.
const PREFERRED_PORT: u16 = 5050;

fn copy_missing_configs(from: &Path, to: &Path) {
    let _ = std::fs::create_dir_all(to);
    let Ok(entries) = std::fs::read_dir(from) else { return };
    for entry in entries.flatten() {
        let target = to.join(entry.file_name());
        if !target.exists() {
            let _ = std::fs::copy(entry.path(), target);
        }
    }
}

fn engine_binary(resources: &Path) -> String {
    let name = if cfg!(windows) { "rtl_airband.exe" } else { "rtl_airband" };
    let bundled = resources.join("engine").join(name);
    if bundled.is_file() {
        bundled.to_string_lossy().into_owned()
    } else {
        name.to_string()
    }
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

    let app = tauri::Builder::default()
        .setup(move |app| {
            let resources = app.path().resource_dir()?;
            let data = app.path().app_data_dir()?;
            let documents = app.path().document_dir().unwrap_or_else(|_| data.clone());

            let configs_dir = data.join("configs");
            copy_missing_configs(&resources.join("configs"), &configs_dir);

            let state = AppState::new(Paths {
                ui_dir: resources.join("ui"),
                configs_dir,
                state_path: data.join("state.json"),
                default_recordings_dir: documents.join("airband-recordings"),
                default_binary: engine_binary(&resources),
            });
            app.manage(state.clone());

            let port = runtime.block_on(async {
                match airband_backend::serve(state.clone(), PREFERRED_PORT).await {
                    Ok(p) => Ok(p),
                    Err(_) => airband_backend::serve(state, 0).await,
                }
            })?;
            // The server runs on the runtime's worker threads; keep the runtime alive.
            std::mem::forget(runtime);

            let url: tauri::Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Airband Recorder")
                .inner_size(1200.0, 860.0)
                .min_inner_size(720.0, 500.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the app");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Let the engine finish its open recordings before we go.
            if let Some(state) = handle.try_state::<std::sync::Arc<AppState>>() {
                state.process.stop(Duration::from_secs(5));
            }
        }
    });
}

