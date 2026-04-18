#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
        .join("multi-monitor-brightness.ico");
    std::fs::write(&path, icon::ico()).expect("write application icon");
    winresource::WindowsResource::new()
        .set_icon(path.to_str().unwrap())
        .set("ProductName", "Multi-Monitor Brightness")
        .set(
            "FileDescription",
            "Multi-Monitor Brightness - Monitor brightness",
        )
        .set(
            "LegalCopyright",
            "Copyright 2026 siarheisinelnikau. MIT License.",
        )
        .set_manifest_file("assets/app.manifest")
        .compile()
        .expect("compile Windows resources (install the Windows SDK)");
}
