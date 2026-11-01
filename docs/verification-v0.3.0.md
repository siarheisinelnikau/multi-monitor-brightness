# Version 0.3.0 verification

## Automated checks

- `cargo fmt --check` and `git diff --check` passed.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings` passed.
- `cargo test --locked --offline --all-targets --all-features`: 23 passed, 2 intentionally ignored. The ignored checks read real monitor hardware or write an isolated Windows registry fixture.
- The translation catalog contains 136 explicit English message keys with complete Russian, French, German, Spanish, and Italian values. Tests reject empty translations and changed message arguments. Persistence tests cover older settings and all six languages with a saved light theme.
- Optimized Windows x64 builds passed both with developer diagnostics enabled and with the normal release features.
- `MultiMonitorBrightness-v0.3.0.exe` has matching 0.3.0 file/product version resources. All eight packaged icon sizes (16 through 256 pixels) loaded and rendered through the native Windows icon-resource path without executing the EXE.

## Actual interface captures

The application rendered and saved its own client-area bitmaps using the production CPU renderer and Windows GDI host. Each isolated `--demo` capture completed successfully and exited within an external timeout. Preview mode used simulated displays, disabled update requests, and did not load or save user preferences or change physical brightness. No mouse or keyboard input was injected.

Reviewed captures include English dark Settings, brightness popup and Schedule; Russian light Settings; German light brightness popup; and French light Schedule at 150% UI scale. The scaled French screen scrolls to keep the footer and controls accessible. The first review found that the theme choices wrapped onto two lines; they now remain together in one compact row.

The README contains refreshed primary, Settings and Schedule captures plus Russian light Settings. These images establish application layout, translated labels, theme contrast, and real font rendering. They do not establish the appearance of the Windows title bar, taskbar, or native context menu.

## Rendering and lifecycle

The bounded optimized renderer regression used simulated displays, three seconds of warm-up and a five-second counter sample per state. CPU was sampled separately over three seconds and is expressed as a percentage of one logical core. The machine exposed 20 logical processors.

| State | CPU, one core | Application rasterizations | UI passes | Hidden paints |
| --- | ---: | ---: | ---: | ---: |
| Settings, settled | 0.00% | 0 | 0 | 0 |
| Brightness popup, settled | 1.56% | 0 | 0 | 0 |
| Hidden startup | 0.00% | 0 | 0 | 0 |
| Controlled slider movement | 9.88% | 126 | 952 | 0 |

The hidden preferences viewport performed zero UI passes during the slider workload. The active popup's frame work had a measured median of 0.24 ms and p95 of 1.26 ms. The settled and hidden cases produced no application scene reconstruction, rasterization or presentation during their counter samples. No OpenGL or DirectX graphics modules were loaded in the diagnostic processes; the source and compiled dependency path use CPU rasterization and Windows GDI presentation. Windows desktop composition remains separate.

A separate optimized native tray callback regression passed 19 checks: quiet startup, popup open/close/reopen, brightness commands while hidden, Settings and Schedule, minimize/restore beside the tray, hidden schedule changes and restoration, and Quit. These checks used the installed callbacks and real disposable windows with simulated monitors, without injected OS input.

Normal hardware compatibility, subjective mouse latency on other machines, and live installation of an Internet update were not exercised. The optional diagnostics were removed from the normal release build after verification. Existing startup-update opt-out and user-initiated installation behavior remain implemented.
