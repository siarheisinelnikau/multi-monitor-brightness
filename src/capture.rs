//! Opt-in visual QA. Capture the application's framebuffer after fonts and window sizing settle.
use std::{
    fs::File,
    io::BufWriter,
    time::{Duration, Instant},
};

fn requested_scale() -> Option<f32> {
    std::env::var_os("MMB_CAPTURE_TO")?;
    let scale = std::env::var("MMB_CAPTURE_SCALE")
        .ok()?
        .parse::<f32>()
        .ok()?;
    (scale.is_finite() && (1.0..=2.0).contains(&scale)).then_some(scale)
}

pub fn configure_scale(ctx: &egui::Context) {
    if let Some(scale) = requested_scale() {
        ctx.options_mut(|options| options.zoom_factor = scale);
    }
}

pub fn configure_viewport(ctx: &egui::Context) {
    let Some(scale) = requested_scale() else {
        return;
    };
    let id = egui::Id::unique("capture-viewport-sized");
    if ctx.data(|data| data.get_temp::<bool>(id).unwrap_or(false)) {
        return;
    }
    let size = crate::platform::preferences_size(scale);
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(
        size.min(egui::vec2(500.0, 360.0)),
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    ctx.data_mut(|data| data.insert_temp(id, true));
}

pub fn update(ctx: &egui::Context) -> Result<bool, String> {
    let Some(path) = std::env::var_os("MMB_CAPTURE_TO") else {
        return Ok(false);
    };
    let id = egui::Id::unique("visual-qa-capture");
    let image =
        ctx.data(|data| data.get_temp::<std::sync::Arc<egui::ColorImage>>(id.with("image")));
    if let Some(image) = image {
        let file = File::create(path).map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(
            BufWriter::new(file),
            image.width() as u32,
            image.height() as u32,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        let bytes: Vec<_> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect();
        writer.write_image_data(&bytes).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        let metadata = serde_json::json!({
            "version": crate::VERSION,
            "renderer": "CPU / Windows GDI",
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "zoom": ctx.zoom_factor(),
            "native_pixels_per_point": ctx.native_pixels_per_point(),
            "pixels_per_point": ctx.pixels_per_point(),
            "physical_size": [image.width(), image.height()],
            "logical_size": [ctx.viewport_rect().width(), ctx.viewport_rect().height()],
            "mode": "simulated monitors; renderer scale test, not a Windows DPI setting change"
        });
        let metadata_path = std::path::PathBuf::from(std::env::var_os("MMB_CAPTURE_TO").unwrap())
            .with_extension("json");
        std::fs::write(
            metadata_path,
            serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        return Ok(true);
    }
    let (started, requested) = ctx.data_mut(|data| {
        let started = *data.get_temp_mut_or_insert_with(id, Instant::now);
        let requested = data.get_temp::<bool>(id.with("requested")).unwrap_or(false);
        (started, requested)
    });
    if !requested && started.elapsed() >= Duration::from_millis(450) {
        let context = ctx.clone();
        ctx.request_screenshot(move |image| {
            context.data_mut(|data| data.insert_temp(id.with("image"), image));
            context.request_repaint_of(egui::ViewportId::ROOT);
        });
        ctx.data_mut(|data| data.insert_temp(id.with("requested"), true));
    }
    ctx.request_repaint_after(Duration::from_millis(40));
    Ok(false)
}
