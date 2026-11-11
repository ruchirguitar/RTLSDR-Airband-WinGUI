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
- `app/` will become a Tauri app (not created yet).

## App architecture plan

Keep `app/ui/static/app.js` working as-is by having the Rust backend serve the same
HTTP API the Flask app does (an axum server on 127.0.0.1, which the Tauri window
loads). The endpoints to port are the `@app.*` routes in `engine/gui/app.py`:
configs (list/create/select/get/save/raw/validate), settings, live audio chunks,
recordings (list/open folder/delete/serve audio), process (status/logs/start/stop/restart).

Windows differences from the Python version:
- Start the engine with `CREATE_NEW_PROCESS_GROUP` and stop it by sending
  `CTRL_BREAK_EVENT`; the engine handles SIGBREAK and closes recordings cleanly.
  Kill only if it hasn't exited after a timeout.
- Open folders with `explorer.exe`.
- Default paths go under the user's Documents / AppData, not `~/`.
- libconfig parsing: the Python app uses `libconf`. A Rust equivalent needs
  choosing or writing; this is the biggest unknown in the port.

## Engine build

See BUILD-WINDOWS.md. Must be built with `-DNFM=TRUE` (FM repeaters) and
`-DPULSEAUDIO=OFF`. The engine sends live audio as 32-bit float mono UDP at
16 kHz (NFM builds).
