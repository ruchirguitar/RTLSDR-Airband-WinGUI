# AGENTS.md

Guidance for Codex (and humans) working in this repo.

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
  - `spectrum.rs`: receives the engine's spectrum lines (port 17299) for the waterfall.
  - `process.rs`, `libconfig.rs` (parser, used to check generated configs;
    tests round-trip `engine/config/*.conf`), `system.rs` (disk space,
    Explorer, folder picker via `rfd`, start with Windows via `auto-launch`).
  - `remote.rs`: access from other devices on the network (password,
    sessions, the second listener on port 5055).
- `README.md` is for users (features, install, troubleshooting);
  `BUILD-WINDOWS.md` for building, data locations and ports. Keep both
  current when features change.
- `app/src-tauri/`: Tauri 2 shell. Starts the backend on 127.0.0.1:5050 (or a
  free port) and the supervisor, opens a window on it, adds a tray icon
  (Open / Switch to listen only-auto-record / Quit), shows notifications
  (tauri-plugin-notification), hides to the tray on close when
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
Auto-record / Listen only switch, `remote.json` for the network password
hash, generated `engine.conf`).

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
- Waterfall (2026-11-12): the engine's `spectrum` device output (512 bins,
  15 lines/s, UDP 17299) -> `spectrum.rs` (last 600 lines) -> `/api/spectrum`
  -> Monitor draws a trace and a waterfall, auto-levelled from the noise
  floor, resizable by dragging its bottom edge (height kept in localStorage),
  click to add or open a frequency. Checked on the dongle.
- Squelch is live: the supervisor compares the new config with the running
  one with squelch left out (`config_structure`); if only squelch changed it
  sends `squelch 0 <channel> <dB>` to the engine's stdin instead of
  restarting. Gain still restarts the engine: live gain fails on Windows
  (RTL-SDR tuner i2c error -9 while streaming). Squelch 0 = always open
  (records constantly; the UI warns).
- Each frequency has a colour (tile swatch, slider, waterfall marker and its
  squelch line). The squelch line on the trace is the engine's measured
  noise floor for that channel (spectrum packet v2) + 2 dB per slider step;
  the dot is its signal level, amber while open. Gain slider in the
  waterfall header saves on release.
- Header (2026-11-12): an `Auto-record | Listen only` switch replaced
  Start/Stop. Status states are `armed` / `listening` (were recording /
  monitoring); each frequency in /api/status has `recording` (on air, auto-record
  on, `record` set), so the pill turns red "Recording 145.400" and tiles show REC
  only while a file is written.
- Waterfall toolbar: Auto levels / Floor / Ceiling (dB) / Contrast (gamma) /
  Peak hold / Zoom (− + Fit Full, Ctrl+wheel, drag to pan) are per-viewer
  (localStorage `wfOpts`); Speed is `receiver.spectrum_rate` (5-30, engine
  restart). Squelch lines on the trace can be dragged. A gain hint shows when a
  peak is above −8 dB, the floor above −55 dB, or gain under 12 dB with a very
  low floor (thresholds are guesses, not tuned).
- Theme: `settings.theme` system/light/dark (header button + Settings),
  mirrored to localStorage so index.html sets it before first paint. Colours
  are tokens in style.css; the waterfall stays dark.
- Per-frequency `notify`: the supervisor calls `AppState.notifier` (set by the
  Tauri shell via tauri-plugin-notification) when a frequency is heard after
  2 min of quiet. Not yet seen working in the installed app.
- Recordings shows a day summary (overs, time on air, per frequency, nets).
- Other devices on the network (`remote.rs`): the app's own window stays on
  127.0.0.1 with no password. With `settings.remote_access` and a password
  (salted, iterated SHA-256 in `remote.json`, never in station.json), the
  supervisor opens a second listener on 0.0.0.0:5055 that serves the same
  router behind `guard`: sign-in page `/login`, 30-day in-memory sessions
  (cookie `fr_session`), 10 wrong passwords a minute max. Remote devices can't
  use `/api/reveal`, `/api/pick-folder` or `/api/remote*`, can't change
  remote access, the recordings folder or autostart, and get download
  buttons (`/audio/...?download=1`) instead of folder buttons. With
  `remote_read_only` they can only GET (and `/api/plan`). `/api/station`
  returns `client: {remote, read_only}`; the UI hides `.rw` controls for
  read-only devices, and a remote device's theme is its own (localStorage).
  Checked 2026-11-12 through the dev server: sign-in, wrong password,
  downloads, read-only refusals, LAN address answering, switching off closes
  the port. Not yet tried from a real second device or with the Windows
  firewall prompt.
- Explorer "show in folder" passes `/select,"path"` raw: Rust's own quoting
  made Explorer open Documents for paths with spaces.
- "Listen only" only stops saving: the engine keeps running without file
  outputs (state "listening"), so live listening and the waterfall go on.
  Quitting the app releases the dongle.
- Recordings can be hissy on weak signals; that's FM demodulation in the
  channel, not wideband noise. Discussed 2026-11-17, not built: set the
  engine's per-channel `highpass`/`lowpass` (~300-3000 Hz), trim squelch
  tails, expose NFM de-emphasis (`tau`), optional speech denoiser.
- To debug the app locally without Tauri:
  `cd app && cargo run -p airband-backend --example dev_server [engine exe]`
  (defaults to `src-tauri/engine-bin/rtl_airband.exe`, data in
  `%TEMP%\frequency-recorder-dev`), then open http://127.0.0.1:5050
  (`PORT` env picks another port). It can't share the dongle or port 5050
  with a running installed app.
