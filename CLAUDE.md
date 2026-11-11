# CLAUDE.md

Guidance for Claude Code (and humans) working in this repo.

## Working norms

- Keep this file and BUILD-WINDOWS.md updated as things change.
- Ask rather than guess when requirements are unclear.
- Add tests with code changes where practical.

## What this is

A Windows desktop app for RTLSDR-Airband aimed at recording ham repeater nets.

- `engine/`: git submodule of `ruchirguitar/RTLSDR-Airband`, tracking branch
  `claude/determined-lamport-0pugtu` for now. Engine changes go **in that repo**
  (Windows-only ones behind `#ifdef _WIN32`), never here. Its Linux web GUI in
  `engine/gui/` (Python/Flask) is the older design this app started from; the
  Windows app no longer mirrors it.
- `app/ui/`: the app's UI (`index.html`, `static/app.js`, `static/style.css`),
  vanilla JS, no build step. Screens: Monitor, Recordings, Nets, frequency
  editor, Settings (hash routes). Design mock-ups live in a Claude Design canvas.
- `app/backend/`: Rust library (axum) with the app's JSON API and logic.
  Platform-neutral and testable on Linux: `cd app && cargo test -p airband-backend`.
  - `station.rs`: the user's setup (receiver, frequencies, nets, recording
    options, settings) saved as `station.json`, its validation, the receiver
    centre (`plan`) and the engine config generated from it (`engine_config`).
    Users never see engine config files.
  - `supervisor.rs`: every 2 s, makes the engine match the station: starts it,
    restarts it when the generated config changes (station edit, a net starting
    or ending), restarts after a crash or dongle replug with 5-60 s backoff,
    applies retention hourly. Nets run over their end time while the frequency
    is still busy (up to `net_overrun_max_min`).
  - `recordings.rs`: scans the recordings folder and reads each mp3's
    `.segments.tsv`; retention deletes old clips but never `Nets/`.
  - `live.rs`: UDP listeners for the engine's live audio (port 17300 + index);
    a packet in the last 0.6 s means the frequency is on air.
  - `process.rs`, `libconfig.rs` (parser, used to check generated configs;
    tests round-trip `engine/config/*.conf`), `system.rs` (disk space,
    Explorer, folder picker via `rfd`, start with Windows via `auto-launch`).
- `app/src-tauri/`: Tauri 2 shell. Starts the backend on 127.0.0.1:5050 (or a
  free port) and the supervisor, opens a window on it, adds a tray icon
  (Open / Start-Stop recording / Quit), hides to the tray on close when
  `close_to_tray` is set, starts hidden with `--hidden` (used by autostart), and
  allows one instance only. Bundles `app/ui` and `src-tauri/engine-bin/` (the
  engine exe + DLLs, filled in by CI or by hand, not committed).
- `.github/workflows/windows.yml`: builds the engine with MSYS2, runs backend
  tests and produces the NSIS/MSI installers as artifacts.

## How recording works

Every frequency gets a `file` output with `split_on_transmission`, gap tones,
normalization and a segment log (engine features), plus a `udp_stream` for live
audio. Outside nets: `<recordings>/<freq>/YYYY/MM/DD/<freq>_<time>.mp3`, overs
less than `clip_join_sec` apart joined. While a net is active its frequency
writes `<recordings>/Nets/<net name>/...` with `net_join_sec`. Switching costs
an engine restart (~2 s). The engine runs hidden with stdin piped; closing the
pipe makes it finish its recordings and exit (killed after 5 s). Data lives in
`%APPDATA%\org.frequencyrecorder.app` (`station.json`, `state.json` for the
on/off switch, generated `engine.conf`).

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

## Status

- Engine builds on Windows (MSYS2/UCRT64) and its unit tests run and pass
  there (they did not before 2026-11-12: a missing `fft_size` and `/tmp`).
  Windows changes are on branch `claude/determined-lamport-0pugtu` of the
  engine fork.
- Tested on real hardware (RTL-SDR Blog V4, 2026-11-12): the engine opens the
  dongle, records and stops cleanly on stdin close. It creates the output
  `directory` itself (one level only: the parent must exist).
- Fixed 2026-11-12: the engine opened mp3 files in text mode, so every
  recording made on Windows before then is corrupt (decodes to a fraction of
  its length).
- Net recording is in the engine: `split_gap_tone`, `split_normalize`,
  `split_normalize_target`, `split_segment_log` on split_on_transmission file
  outputs (see `engine/src/CLAUDE.md`, `engine/config/split_join_net.conf`).
- UI rebuilt 2026-11-12 (station model, supervisor, nets, tray, retention,
  autostart, silence warning, CTCSS, per-tile squelch). Checked through the dev
  server with the dongle: adding a frequency starts the engine, squelch changes
  reach the engine, Recordings lists and plays clips and nets with over marks.
  Not yet checked by hand: tray menu and close-to-tray, start with Windows,
  single instance, live listening speed, a scheduled net switching over, a real
  dongle unplug/replug.
- Next: the waterfall needs a new engine output (spectrum over UDP); the band
  view on Monitor is a placeholder with frequency markers until then.
- To debug the app locally without Tauri:
  `cd app && cargo run -p airband-backend --example dev_server [engine exe]`
  (defaults to `src-tauri/engine-bin/rtl_airband.exe`, data in
  `%TEMP%\frequency-recorder-dev`), then open http://127.0.0.1:5050.
