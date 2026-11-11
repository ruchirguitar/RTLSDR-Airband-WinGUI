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

Not written yet. This section will cover installing Rust and the Tauri build.
