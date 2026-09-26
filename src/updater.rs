//! Product configuration and compatibility with update plans created before extraction.
//! The updater implementation, workflow and UI live in rust-auto-updator.
use rust_auto_updator::Config;
pub use rust_auto_updator::{Controller, Prepared, handoff, now};
use std::path::PathBuf;

pub fn config(portable: bool) -> Result<Config, String> {
    let mut config = Config::new(
        "siarheisinelnikau",
        "multi-monitor-brightness",
        crate::VERSION,
        "MultiMonitorBrightness",
    )?;
    // Preserve the staging protocol used by released versions of this application.
    config.install.stage_prefix = ".mmb-update-".into();
    config.install.helper_name = "MultiMonitorBrightness.exe".into();
    config.install.restart_args = vec!["--settings".into(), "--updated".into()];
    config.install.rollback_args = vec!["--startup".into()];
    if portable {
        config.install.restart_args.push("--portable".into());
        config.install.rollback_args.push("--portable".into());
    }
    Ok(config)
}

pub fn start(ctx: egui::Context, portable: bool) -> Result<Controller, String> {
    rust_auto_updator_egui::start(config(portable)?, ctx)
}

pub fn apply_update() -> Result<(), String> {
    // Old releases store a portable boolean instead of explicit restart arguments.
    // Read only that product-specific setting; the library validates the full plan.
    let helper = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = helper.parent().ok_or("Update staging folder missing.")?;
    let legacy: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.join("update.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let portable = legacy
        .get("portable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    rust_auto_updator::apply_update(&config(portable)?)
}

pub fn cleanup_after_update(directory: PathBuf) {
    if let Ok(config) = config(false) {
        rust_auto_updator::cleanup_after_update(directory, config.install);
    }
}

#[cfg(feature = "updater-smoke-test")]
pub fn smoke_cli(arguments: &[std::ffi::OsString]) -> Result<bool, String> {
    rust_auto_updator::smoke_cli(arguments, &config(false)?)
}
