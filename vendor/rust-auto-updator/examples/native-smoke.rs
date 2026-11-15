//! Standalone installer regression; no application UI or network is involved.
use rust_auto_updator::Config;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = Config::new(
        "example",
        "sample-app",
        env!("CARGO_PKG_VERSION"),
        "SampleApp",
    )?;
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(feature = "smoke-test")]
    if rust_auto_updator::smoke_cli(&arguments, &config)? {
        return Ok(());
    }
    if arguments.len() == 1 && arguments[0] == "--apply-update" {
        return rust_auto_updator::apply_update(&config);
    }
    Err("Run this example with --features smoke-test -- --test-updater".into())
}
