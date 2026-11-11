// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use airband_backend::{AppState, Paths};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// A fixed port keeps the page origin stable, so per-browser settings survive
/// restarts. Falls back to any free port if it is taken.
const PREFERRED_PORT: u16 = 5050;

fn engine_binary(resources: &Path) -> String {
    let name = if cfg!(windows) { "rtl_airband.exe" } else { "rtl_airband" };
    let bundled = resources.join("engine").join(name);
    if bundled.is_file() {
        bundled.to_string_lossy().into_owned()
    } else {
        name.to_string()
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn recording_label(state: &AppState) -> &'static str {
    if state.recording_enabled() {
        "Stop recording"
    } else {
        "Start recording"
    }
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let start_hidden = std::env::args().any(|a| a == "--hidden");

    let app = tauri::Builder::default()
        // a second copy would fight the first one for the dongle: bring the first one forward instead
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .setup(move |app| {
            let resources = app.path().resource_dir()?;
            let data = app.path().app_data_dir()?;
            let documents = app.path().document_dir().unwrap_or_else(|_| data.clone());

            let state = AppState::new(Paths {
                ui_dir: resources.join("ui"),
                data_dir: data,
                default_recordings_dir: documents.join("Frequency Recordings"),
                engine_binary: engine_binary(&resources),
                autostart_exe: std::env::current_exe().ok(),
            });
            app.manage(state.clone());

            let port = runtime.block_on(async {
                state.start_supervisor();
                match airband_backend::serve(state.clone(), PREFERRED_PORT).await {
                    Ok(p) => Ok(p),
                    Err(_) => airband_backend::serve(state.clone(), 0).await,
                }
            })?;
            // The server and supervisor run on the runtime's worker threads; keep it alive.
            std::mem::forget(runtime);

            let url: tauri::Url = format!("http://127.0.0.1:{port}/").parse().unwrap();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Frequency Recorder")
                .inner_size(1280.0, 880.0)
                .min_inner_size(720.0, 520.0)
                .visible(!start_hidden)
                .build()?;

            let open = MenuItem::with_id(app, "open", "Open Frequency Recorder", true, None::<&str>)?;
            let toggle = MenuItem::with_id(app, "toggle", recording_label(&state), true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit (stops recording)", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &toggle, &PredefinedMenuItem::separator(app)?, &quit])?;
            let toggle_item = toggle.clone();
            let mut tray = TrayIconBuilder::with_id("main").tooltip("Frequency Recorder").menu(&menu).show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(move |app, event| match event.id().as_ref() {
                "open" => show_main(app),
                "toggle" => {
                    let state = app.state::<Arc<AppState>>();
                    let _ = state.set_recording_enabled(!state.recording_enabled());
                    let _ = toggle_item.set_text(recording_label(&state));
                }
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                    show_main(tray.app_handle());
                }
            })
            .build(app)?;

            // keep the tray menu's label in step with the window's Start/Stop button
            let handle = app.handle().clone();
            let toggle_item = toggle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(2));
                let state = handle.state::<Arc<AppState>>();
                let _ = toggle_item.set_text(recording_label(&state));
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<AppState>>();
                if state.station().settings.close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the app");

    app.run(|handle, event| match event {
        // Closing the window without the tray option quits the app.
        RunEvent::ExitRequested { code: None, api, .. } => {
            let state = handle.state::<Arc<AppState>>();
            if state.station().settings.close_to_tray {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => {
            // Let the engine finish its open recordings before we go.
            if let Some(state) = handle.try_state::<Arc<AppState>>() {
                state.process.stop(Duration::from_secs(5));
            }
        }
        _ => {}
    });
}
