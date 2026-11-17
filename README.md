# Frequency Recorder

A Windows app that turns an RTL-SDR dongle into a recorder for radio
frequencies: ham repeaters, simplex channels and scheduled nets. It records
every transmission on the frequencies you choose, shows a live waterfall of
the band, lets you listen live, and keeps the recordings sorted by frequency,
day and net. It's built on the
[RTLSDR-Airband](https://github.com/rtl-airband/RTLSDR-Airband) receiver
engine, and you never need to edit config files or use Linux.

**Status: early, but usable.** It's tested on an RTL-SDR Blog V4 with two
repeaters. Some parts haven't been tried by hand yet (see
[Known gaps](#known-gaps)).

## What it does

- **Frequencies as tiles.** Add a frequency (for example your repeater's
  output) and it's watched from then on. All frequencies have to fit in one
  dongle's window of about 2 MHz; the app tunes the dongle for you and warns
  you when a frequency is near the edge or too far out.
- **Auto-record or Listen only.** In *Auto-record* every transmission that
  opens a frequency's squelch is saved. The header shows "Armed" while it
  waits and turns red ("Recording 145.400") only while a file is being
  written. *Listen only* keeps the waterfall and live listening running but
  saves nothing.
- **Waterfall.** A live spectrum and waterfall of the whole band, with:
  - Floor, ceiling and contrast controls, plus Auto levels.
  - Peak hold, zoom and pan.
  - A choice of scroll speed.
  - A gain slider.

  Each frequency has its own colour and a squelch line you can drag. Click
  the waterfall to add a frequency there.
- **Squelch per frequency**, 0 to 9 like the SQL knob on a handheld. Changes
  apply immediately without interrupting recording. An optional CTCSS tone
  ignores other stations on the same frequency.
- **Nets.** Give a net a name, a frequency, days and times. While it's on,
  every over goes into one recording with the silences cut out:
  - Optional soft beeps mark where each cut was.
  - Loudness is evened out.
  - A list of the time of every over is saved next to the recording, for
    transcripts.

  A net that runs late keeps recording while the frequency is still busy.
- **Recordings.** A day timeline per frequency, a summary of the day (overs
  and time on air), filters by frequency, a player with marks between overs,
  "Show in folder" and delete. Old everyday recordings can be deleted
  automatically; net recordings never are.
- **Other devices on your network.** Phones, tablets and other PCs on the
  same Wi-Fi can open the app with a password, optionally as *watch and
  listen only*.
- **Windows extras:**
  - A tray icon, and keeping running when the window is closed.
  - Start with Windows.
  - A notification when a frequency becomes active.
  - A warning when nothing has been heard for a long time.
  - Light, dark or same-as-Windows theme.

## Install

1. **Get the installer.** On GitHub, open **Actions → Windows app**, pick the
   latest green run for your branch, and download the
   `frequency-recorder-windows-installer` artifact. Run the `.exe` inside.
   It's a per-user install and doesn't need administrator rights. There are
   no signed releases yet, so Windows SmartScreen may warn you the first
   time.
2. **Install the USB driver for the dongle, once per PC.** Windows doesn't
   let programs talk to an RTL-SDR until it uses the WinUSB driver:
   1. Plug in the dongle.
   2. Run [Zadig](https://zadig.akeo.ie/).
   3. Choose **Options → List All Devices** and select **Bulk-In, Interface
      (Interface 0)**.
   4. Pick **WinUSB** and click **Replace Driver**.
3. **Start Frequency Recorder** and add your first frequency.

Only one program can use the dongle at a time. Close SDR#, SDR++ or any
other copy of the app first.

## Using it

- **Squelch:** 5 suits most repeaters. Raise it if noise gets recorded; lower
  it if weak stations are missed. Squelch 0 is always open, so the frequency
  records constantly.
- **Gain:** 25–35 dB suits most antennas. The waterfall shows a hint if
  the gain looks too high or too low. Changing the gain restarts the receiver
  for about 2 seconds.
- **Recordings folder:** `Documents\Frequency Recordings` by default. You can
  change it in Settings. Files are organised like this:
  ```
  145.400\2026\11\12\145.400_20261112_070201.mp3
  Nets\Morning net\2026\11\12\Morning net_20261112_070012.mp3
  ```
  Each recording has a `.segments.tsv` file next to it with the time of every over.
- **From another device:**
  1. In **Settings → Other devices on your network**, set a password and
     turn the switch on.
  2. Settings shows the address to open, such as `http://192.168.1.20:5055`.
  3. If Windows asks about the firewall, allow **Private networks**.

  This works on your own network only. It isn't meant to be opened to the
  internet.
- **Closing the window** keeps it recording in the tray when "Keep recording
  when the window is closed" is on. To stop, right-click the tray icon and
  choose **Quit**. That finishes any open recording and releases the dongle.

## Troubleshooting

| Problem | What to try |
|---|---|
| "Receiver not running" | Is the dongle plugged in and on the WinUSB driver (Zadig)? Is another SDR program or another copy of the app using it? |
| Nothing gets recorded | Is the header on *Auto-record*? Is squelch too high (watch the dot on the waterfall reach the squelch line)? Is "Save recordings" on for that frequency? |
| Recordings are hissy | Weak signals sound hissy in FM; raise squelch a step, check the antenna and gain. |
| Other devices can't connect | Same Wi-Fi? Network set to *Private* in Windows? Firewall allowed for Frequency Recorder? |
| Settings → "Recorder log" | Shows what the engine is doing, and the config the app wrote for it. |

## Known gaps

- Not yet tried by hand:
  - The tray menu, close-to-tray and start with Windows.
  - Notifications.
  - Unplugging and replugging the dongle.
  - A scheduled net switching on by itself.
  - Opening the app from a real second device.
- Only one dongle is supported at a time.
- Background hiss on weak signals isn't filtered yet. A voice filter and
  squelch-tail trimming are planned.

## How it fits together

```
engine/          git submodule -> ruchirguitar/RTLSDR-Airband (C++ receiver/recorder),
                 built for Windows with MSYS2 into rtl_airband.exe + DLLs
app/backend/     Rust (axum): the app's JSON API, the engine supervisor, recordings,
                 live audio, waterfall data, network access
app/ui/          the interface: plain HTML/CSS/JS, no build step
app/src-tauri/   the Windows shell (Tauri 2): window, tray, notifications, installer
```

The app writes the engine's config itself from your setup and keeps the
engine running. The engine's Windows changes and its new features live in the
fork. Windows-only changes are behind `_WIN32`, so the Linux build keeps working:
- spectrum output for the waterfall
- live squelch control
- gap tones, loudness evening and segment logs for nets

## Building from source

See [BUILD-WINDOWS.md](BUILD-WINDOWS.md). Notes for contributors (and AI
coding assistants) are in [CLAUDE.md](CLAUDE.md).

## Legal

Recording radio traffic is regulated differently in each country. It's up to
you to follow the rules where you are. The app is meant for amateur radio nets
and similar.

## License

GPL-3.0, the same as RTLSDR-Airband. Anyone you give a build to must be able
to get its source. The engine is RTLSDR-Airband by its authors; this fork and
the Windows app are by Ruchir (VU22PR).
