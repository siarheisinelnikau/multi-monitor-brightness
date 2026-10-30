#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!(
    "Multi-Monitor Brightness currently targets Windows. Build for x86_64-pc-windows-msvc."
);

mod app;
#[cfg(feature = "ui-capture")]
mod capture;
mod config;
mod icon;
mod model;
mod monitors;
mod platform;
mod startup;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    if let Err(error) = run() {
        platform::message(&error);
    }
}

fn run() -> Result<(), String> {
    let mut demo = false;
    let mut portable = false;
    let mut start_in_tray = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--demo" => demo = true,
            "--portable" => portable = true,
            "--startup" => start_in_tray = true,
            "--help" | "-h" => {
                platform::message(&format!(
                    "Multi-Monitor Brightness {VERSION}\n\nBrightness control without leaving the keyboard.\n\n--portable   Keep settings beside the executable\n--startup    Start quietly in the system tray\n--demo       Preview with simulated displays; no hardware changes or saved settings\n\nDefault shortcuts: Ctrl + Alt + Up / Down.\nOpen the window: Ctrl + Alt + B.\nQuit from the tray menu or the application footer."
                ));
                return Ok(());
            }
            _ => {
                return Err(format!(
                    "Unknown option: {argument}\n\nUse --help to see available options."
                ));
            }
        }
    }
    let Some(_instance) = platform::Instance::acquire(demo)? else {
        if start_in_tray {
            return Ok(());
        }
        return Err("Multi-Monitor Brightness is already running. Open it from the sun icon in the system tray.".into());
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(if demo {
                "Multi-Monitor Brightness · Demo"
            } else {
                "Multi-Monitor Brightness"
            })
            .with_inner_size([620.0, 780.0])
            .with_visible(!start_in_tray)
            .with_min_inner_size([500.0, 520.0])
            .with_icon(eframe::egui::IconData {
                rgba: icon::rgba(64),
                width: 64,
                height: 64,
            }),
        renderer: eframe::Renderer::Glow,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Multi-Monitor Brightness",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::MultiMonitorBrightness::new(
                cc,
                demo,
                portable,
                start_in_tray,
            )))
        }),
    )
    .map_err(|e| format!("Could not open Multi-Monitor Brightness: {e}"))
}
