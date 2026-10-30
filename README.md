# Multi-Monitor Brightness

*Brightness control without leaving the keyboard.*

An unobtrusive Windows utility for controlling external monitor brightness from the keyboard. Built in Rust, with a native desktop interface and a single executable. No installer, WebView, or .NET runtime required.

## Features

- A shared slider for all linked displays, plus an individual slider for each screen.
- Global, customizable brightness shortcuts. Relative changes preserve differences between displays.
- Smooth evening dimming with configurable start time, duration, target, and morning restoration.
- Per-monitor brightness memory, restored on launch and reconnection.
- A system tray icon with open, brightness up/down, and quit actions.
- Optional Windows autostart, launching quietly in the tray, and the app version in Settings.
- Background DDC/CI operations, throttled writes, and automatic display topology detection.
- A demo mode with simulated monitors, isolated from real brightness and saved preferences.

## Run

An ordinary launch controls your real monitors. **Demo** appears only when explicitly launched with `--demo`, which uses simulated displays. Exit the preview and run the executable without arguments to use real hardware.

Launch `MultiMonitorBrightness.exe`. Enable **DDC/CI** in your external monitor's on-screen menu if the display reports that brightness control is unavailable.

| Shortcut | Action |
| --- | --- |
| `Ctrl + Alt + Up` | Increase linked displays by 5 percentage points |
| `Ctrl + Alt + Down` | Decrease linked displays by 5 percentage points |
| `Ctrl + Alt + B` | Open the window |

Change the shortcuts and step size in **Settings → Apply shortcuts**. Windows reports conflicts with other applications in the same panel.

The shared slider sets linked displays to the same percentage; hotkeys and the tray's +/- actions adjust each linked display relative to its current level. Clear **Link** to exclude a monitor from shared controls and the evening schedule. Its individual slider remains available.

Closing the window keeps the utility running in the tray by default. Use **Quit** in the window footer or tray menu to exit.

```powershell
.\MultiMonitorBrightness.exe --demo
.\MultiMonitorBrightness.exe --portable
```

Normally, settings live in `%APPDATA%\MultiMonitorBrightness\settings.json`. `--portable` stores `multi-monitor-brightness.json` beside the executable instead. The executable is self-contained; the JSON file holds your preferences. Demo mode neither reads nor writes these settings and does not register global shortcuts.

## Windows startup

Enable **Start with Windows** in **Settings → Application**. This registers the current executable for your Windows account; administrator rights are not needed. Startup is off until you opt in. The app starts quietly in the tray when you sign in. The option also preserves `--portable` when enabled from a portable session.

Keep the executable in a stable folder. If you move it, open Settings and select **Use this executable** to update its startup location. Disable **Start with Windows** before removing the app. The registration lives in the current user's Windows Run key, rather than in the JSON preferences. Windows may delay startup apps or disable them through its own Startup Apps settings.

The version is always visible at the top of Settings and comes directly from Cargo package metadata.

## Evening dimming

Enable the schedule in **Schedule**. By default, brightness fades from each screen's remembered manual level toward 25%, starting at 20:00 over 60 minutes. At 08:00, the remembered levels return. Screens already below the target never get brighter during dimming.

Changing brightness manually pauses the schedule for the current evening. Use **Resume** to re-enable it immediately. Schedule output does not overwrite the remembered manual brightness. Times use the computer's local clock, including changes to time zone or daylight saving time. The app must be running for schedules and hotkeys to work; Enable **Settings → Start with Windows** to keep it available after signing in.

## Compatibility

- Windows 10/11, x86-64, with a working OpenGL graphics driver.
- External displays that expose brightness through DDC/CI. The backend uses `GetMonitorBrightness` / `SetMonitorBrightness`, with a VCP `0x10` fallback.
- Built-in laptop panels, displays with DDC/CI disabled, and some docks, KVMs, or adapters may be unavailable. There is no software overlay or WMI fallback.
- Monitor preferences are keyed by the Windows monitor interface path, with a display-path fallback. Moving a display to a different port or adapter can create a new identity. Mirrored displays may expose ambiguous identities.
- Driver and monitor implementations vary. Physical brightness changes require validation on your hardware.

## Build from source

Install Rust (stable, MSVC toolchain) and Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK. Rust 1.95 or newer is required.

```powershell
cargo run -- --demo
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
.\scripts\build.ps1
```

The build script creates `dist\MultiMonitorBrightness.exe` and its SHA-256 checksum. `.cargo/config.toml` statically links the MSVC runtime. The icon, manifest, and default fonts are embedded. Only standard Windows and graphics-driver DLLs are needed at runtime.

CI runs formatting, Clippy, tests, and a release build on Windows, then uploads the executable as a build artifact. CI and release builds use the tested Rust 1.96.1 toolchain.

Optional read-only hardware check (requires a live desktop): `cargo test read_hardware_brightness -- --ignored --nocapture`.

## Implementation

`src/monitors.rs` owns all physical monitor handles on a dedicated worker thread and releases them on refresh or shutdown. Writes are coalesced per monitor, and revision numbers keep delayed results from overwriting newer slider positions. Failed changes leave the last successful manual brightness intact. A separate thread registers and receives Win32 global hotkeys; egui handles the window and tray actions.

Tray commands, hotkeys, monitor responses, saving, and schedules run in `eframe::App::logic`, independently of visible UI rendering. This requires eframe 0.37: older versions could stop processing pending commands when the Windows window was hidden in the tray.

Settings are written to a same-directory temporary file, synced, then replaced. An unreadable settings file is preserved and automatic saving is paused until it is fixed or moved. The evening scheduler derives its output from manual levels rather than progressively modifying its own previous output.

## Status

Current version: 0.1.2. Automated tests cover brightness scaling, schedule boundaries, shortcut validation, persistence, and queued monitor operations. Hardware compatibility is not yet certified.

Version 0.1.1 fixes unresponsive tray controls after hiding the window. A live-window regression checks Open, brightness up/down, restoring a minimized window, background dimming/restoration, and Quit with simulated monitors. To repeat it on an interactive Windows desktop, run `scripts\test-tray.ps1`. It invokes the installed menu callback with its real menu IDs and checks the native window's visibility; it does not inject mouse or keyboard input or change hardware brightness.

## Short description

Keyboard-first brightness control for external monitors, with customizable hotkeys, a shared slider, smooth evening dimming, and per-monitor settings.

## License

MIT. See [LICENSE](LICENSE).

API references: [Windows monitor configuration](https://learn.microsoft.com/en-us/windows/win32/monitor/using-the-low-level-monitor-configuration-functions), [global hotkeys](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey), and [eframe](https://docs.rs/eframe/0.37.0/eframe/).
