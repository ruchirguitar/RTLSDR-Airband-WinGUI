# RTLSDR-Airband-WinGUI

A native Windows app for recording radio channels (e.g. ham repeater nets) with
[RTLSDR-Airband](https://github.com/rtl-airband/RTLSDR-Airband): add frequencies
as cards, record each transmission to its own time-stamped file, listen live, and
browse recordings, without editing config files or using Linux.

**Status: early work in progress.** Nothing here is ready to install yet.

## How it fits together

```
engine/   git submodule -> ruchirguitar/RTLSDR-Airband (the C++ receiver/recorder)
          built for Windows with MSYS2; produces rtl_airband.exe + its DLLs
app/      Tauri desktop app (Rust backend + the existing web UI)
          starts/stops rtl_airband.exe, edits its .conf, plays live audio,
          manages recordings
```

The engine is unchanged in behavior; the Windows changes live in the fork behind
`_WIN32` so the Linux build keeps working.

## Getting a build

- **Download:** GitHub -> Actions -> latest green run -> Artifacts.
- **Build locally on Windows:** see [BUILD-WINDOWS.md](BUILD-WINDOWS.md).

## License

GPL-3.0, same as RTLSDR-Airband. Any binary you share must come with its source.
