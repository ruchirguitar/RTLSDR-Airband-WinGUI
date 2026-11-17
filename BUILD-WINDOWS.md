# Building on Windows

These are the same steps the GitHub Actions workflows run.

## 1. One-time setup

1. Install [MSYS2](https://www.msys2.org/) (default location `C:\msys64`).
2. Open **"MSYS2 UCRT64"** from the Start menu and run:
   ```bash
   pacman -Syu   # may close the window; reopen UCRT64 and run it again
   pacman -S --needed git \
     mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-cmake mingw-w64-ucrt-x86_64-ninja \
     mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-libconfig mingw-w64-ucrt-x86_64-lame \
     mingw-w64-ucrt-x86_64-libshout mingw-w64-ucrt-x86_64-fftw \
     mingw-w64-ucrt-x86_64-rtl-sdr mingw-w64-ucrt-x86_64-soapysdr
   ```
3. Clone with the engine submodule:
   ```bash
   git clone --recurse-submodules https://github.com/ruchirguitar/RTLSDR-Airband-WinGUI
   ```

## 2. Build the engine (in the UCRT64 window)

```bash
cd RTLSDR-Airband-WinGUI/engine
cmake -G Ninja -B build -DCMAKE_BUILD_TYPE=Release -DNFM=TRUE -DPLATFORM=generic -DPULSEAUDIO=OFF
cmake --build build
./build/src/rtl_airband.exe -v
```

Unit tests (downloads GoogleTest on the first run; `builds/` is git-ignored):

```bash
cmake -G Ninja -B builds/windows-tests -DCMAKE_BUILD_TYPE=Debug -DNFM=TRUE -DPLATFORM=generic -DPULSEAUDIO=OFF -DBUILD_UNITTESTS=TRUE
cmake --build builds/windows-tests
./builds/windows-tests/src/unittests.exe
```

## 3. USB driver for the dongle (once per PC)

Windows doesn't let programs talk to an RTL-SDR until it uses the WinUSB driver.
Plug in the dongle, run [Zadig](https://zadig.akeo.ie/), choose **Options -> List All
Devices**, select **"Bulk-In, Interface (Interface 0)"**, pick **WinUSB**, and click
**Replace Driver**.

## 4. The desktop app

Install [Rust](https://rustup.rs/) (default options) and [Node.js](https://nodejs.org/) LTS.
Then, in the UCRT64 window, copy the engine and its DLLs into the app:

```bash
cd RTLSDR-Airband-WinGUI
out=app/src-tauri/engine-bin
cp engine/build/src/rtl_airband.exe "$out/"
ldd engine/build/src/rtl_airband.exe | awk '/ucrt64/ {print $3}' | xargs -r -I{} cp {} "$out/"
cp -r /ucrt64/lib/SoapySDR "$out/" 2>/dev/null || true
```

and in a normal PowerShell window:

```powershell
cd RTLSDR-Airband-WinGUI\app
cargo test -p airband-backend           # backend tests
cd src-tauri
npx --yes @tauri-apps/cli@2 dev         # run the app from source
npx --yes @tauri-apps/cli@2 build       # make the installers
```

Installers land in `app\target\release\bundle\nsis\` (`.exe`) and `...\bundle\msi\`.

## Where the app keeps things

| What | Where |
|---|---|
| Frequencies, nets and settings | `%APPDATA%\org.frequencyrecorder.app\station.json` |
| Auto-record / Listen only switch | `%APPDATA%\org.frequencyrecorder.app\state.json` |
| Network-access password (salted hash only) | `%APPDATA%\org.frequencyrecorder.app\remote.json` |
| Engine config the app writes (don't edit) | `%APPDATA%\org.frequencyrecorder.app\engine.conf` |
| Recordings (default, changeable in Settings) | `Documents\Frequency Recordings\<freq>\` and `...\Nets\<net name>\` |

"Start with Windows" adds `Frequency Recorder --hidden` under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; turning it off removes it.

## Ports

| Port | Used for |
|---|---|
| 5050 (127.0.0.1) | The app's own window (falls back to a free port if taken) |
| 5055 (all interfaces) | Other devices on the network, only while that's switched on in Settings |
| 17299 (UDP, 127.0.0.1) | Waterfall lines from the engine |
| 17300 + n (UDP, 127.0.0.1) | Live audio of frequency n from the engine |

The first time network access is switched on, Windows Firewall asks whether to
allow the app; allow **Private networks**.

## Trying the UI without Tauri

Runs the backend and the engine against `app/ui` in a normal browser (uses the
dongle; data in `%TEMP%\frequency-recorder-dev`). Quit the installed app first:
they can't share the dongle or port 5050.

```powershell
cd RTLSDR-Airband-WinGUI\app
cargo run -p airband-backend --example dev_server        # http://127.0.0.1:5050
$env:PORT = 5060; cargo run -p airband-backend --example dev_server   # another port
```

Notifications, the tray and "Start with Windows" only work in the Tauri app.
