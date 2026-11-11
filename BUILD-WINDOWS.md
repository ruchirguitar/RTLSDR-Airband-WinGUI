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
| Configs (seeded from the samples on first run) | `%APPDATA%\org.frequencyrecorder.app\configs\` |
| App settings | `%APPDATA%\org.frequencyrecorder.app\state.json` |
| Recordings (default) | `Documents\Frequency Recordings\` |
