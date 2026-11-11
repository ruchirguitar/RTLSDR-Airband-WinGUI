# CLAUDE.md

Guidance for Claude Code (and humans) working in this repo.

## Working norms

- Keep this file and BUILD-WINDOWS.md updated as things change.
- Ask rather than guess when requirements are unclear.
- Add tests with code changes where practical.

## What this is

A Windows desktop app for RTLSDR-Airband aimed at recording ham repeater nets.

- `engine/`: git submodule of `ruchirguitar/RTLSDR-Airband`, tracking branch
  `claude/determined-lamport-0pugtu` for now. Windows fixes to the C++ engine go
  **in that repo**, behind `#ifdef _WIN32`, never here. Its Linux web GUI lives in
  `engine/gui/` (Python/Flask) and is the reference implementation for this app.
- `app/ui/`: copy of that web UI (`static/`, `templates/`).
- `app/configs/`: sample engine configs.
- `app/backend/`: Rust library (axum) that serves the same JSON API as the Flask
  app, plus the libconfig reader/writer (`libconfig.rs`), engine process manager
  (`process.rs`) and live-audio UDP listener (`live.rs`). Platform-neutral and
  fully testable on Linux: `cd app && cargo test -p airband-backend`.
- `app/src-tauri/`: thin Tauri 2 shell. Starts the backend on 127.0.0.1:5050
  (falls back to a free port), opens a window on it, and stops the engine on exit.
  Bundles `app/ui`, `app/configs` and `src-tauri/engine-bin/` (the engine exe +
  DLLs, filled in by CI or by hand, not committed).
- `.github/workflows/windows.yml`: builds the engine with MSYS2, runs backend
  tests and produces the NSIS/MSI installers as artifacts.

## App architecture plan

`app/ui/static/app.js` works unchanged because the Rust backend serves the same
HTTP API the Flask app does (an axum server on 127.0.0.1, which the Tauri window
loads). Keep `app/ui` in sync with `engine/gui/` when the UI changes. The endpoints to port are the `@app.*` routes in `engine/gui/app.py`:
configs (list/create/select/get/save/raw/validate), settings, live audio chunks,
recordings (list/open folder/delete/serve audio), process (status/logs/start/stop/restart).

Windows differences from the Python version:
- The engine runs hidden (`CREATE_NO_WINDOW`) with stdin piped. Closing that
  pipe is the stop request (the engine watches piped stdin on Windows and exits
  cleanly, finishing open recordings); it is killed only after a 5 s timeout.
  This also stops an orphaned engine if the app crashes.
- Open folders with `explorer.exe`.
- Default paths go under the user's Documents / AppData, not `~/`.
- libconfig parsing is our own `libconfig.rs` (no maintained crate exists). It
  must keep the scalar wrapping and write lowercase bools and floats with a
  decimal point; its tests round-trip every file in `app/configs/`.

## Engine build

See BUILD-WINDOWS.md. Must be built with `-DNFM=TRUE` (FM repeaters) and
`-DPULSEAUDIO=OFF`. The engine sends live audio as 32-bit float mono UDP at
16 kHz (NFM builds).

## Naming

The app is called **Frequency Recorder** in everything a user sees (installer,
window title, Start menu, default `Documents\Frequency Recordings` folder,
`%APPDATA%\org.frequencyrecorder.app`). Don't use "airband" in user-facing names:
recording aviation traffic is illegal in some countries and the app is meant for
ham nets and similar. Crediting the engine as RTLSDR-Airband is fine (and needed
for the GPL).

## Status (hand-off from the cloud session)

- Engine builds on Windows (MSYS2/UCRT64) and passes its unit tests; Windows
  changes are on branch `claude/determined-lamport-0pugtu` of the engine fork.
- This repo's `windows.yml` builds a working installer; backend tests pass.
- **Not yet tested on real hardware.** First things to check on a Windows PC
  with an RTL-SDR (after Zadig/WinUSB): engine starts from the app (Process
  Control log), a transmission produces a file in Recordings, live listening
  plays at the right speed, Stop leaves a complete MP3, closing the app leaves
  no `rtl_airband.exe` running. Unconfirmed: whether the engine creates the
  per-frequency recordings folder itself.
- To debug the app locally, run the backend without Tauri:
  `cd app && cargo run -p airband-backend --example dev_server`, then open
  http://127.0.0.1:5050. Point the binary path setting at the built engine.
