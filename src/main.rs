#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
compile_error!(
    "Multi-Monitor Brightness currently targets Windows. Build for x86_64-pc-windows-msvc."
);

mod app;
#[cfg(feature = "ui-capture")]
mod capture;
mod config;
mod host;
mod icon;
mod model;
mod monitors;
mod platform;
mod startup;
mod updater;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    if let Err(error) = run() {
        #[cfg(feature = "updater-smoke-test")]
        if std::env::var_os("MMB_UPDATER_SMOKE").is_some() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        #[cfg(feature = "render-smoke-test")]
        if std::env::var_os("MMB_RENDER_TEST").is_some() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        platform::message(&error);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(feature = "updater-smoke-test")]
    if updater::smoke_cli(&arguments)? {
        return Ok(());
    }
    if arguments.len() == 1 && arguments[0] == "--apply-update" {
        return updater::apply_update();
    }
    let mut demo = false;
    let mut portable = false;
    let mut start_in_tray = false;
    let mut open_settings = false;
    let mut updated = false;
    let mut cleanup = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let argument = argument.to_string_lossy();
        match argument.as_ref() {
            "--demo" => demo = true,
            "--portable" => portable = true,
            "--startup" => start_in_tray = true,
            "--settings" => open_settings = true,
            "--updated" => updated = true,
            "--cleanup-update" => {
                cleanup = Some(std::path::PathBuf::from(
                    arguments.next().ok_or("Missing update cleanup folder")?,
                ))
            }
            "--help" | "-h" => {
                platform::message(&format!(
                    "Multi-Monitor Brightness {VERSION}\n\nBrightness control without leaving the keyboard.\n\n--portable   Keep settings beside the executable\n--startup    Start quietly in the system tray\n--settings   Open preferences instead of the brightness popup\n--demo       Preview with simulated displays; no hardware changes or saved settings\n\nDefault shortcuts: Ctrl + Alt + Up / Down.\nOpen brightness: Ctrl + Alt + B.\nQuit from the tray menu or the application footer."
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
    let preferences_size = platform::preferences_size(1.0);
    let viewport = egui::ViewportBuilder::default()
        .with_title(platform::PREFERENCES_TITLE)
        .with_inner_size(preferences_size)
        .with_visible(open_settings)
        .with_min_inner_size(preferences_size.min(egui::vec2(500.0, 360.0)))
        .with_icon(egui::IconData {
            rgba: icon::rgba(64),
            width: 64,
            height: 64,
        });
    host::run(viewport, move |cc| {
        let app = app::MultiMonitorBrightness::new(
            cc,
            demo,
            portable,
            start_in_tray,
            open_settings,
            updated,
        );
        if let Some(directory) = cleanup {
            updater::cleanup_after_update(directory);
        }
        Ok(Box::new(app::Application::new(app)))
    })?;
    Ok(())
}
