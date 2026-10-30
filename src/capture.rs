//! Opt-in visual QA. Capture the application's framebuffer after fonts and window sizing settle.
use eframe::egui;
use std::{
    fs::File,
    io::BufWriter,
    time::{Duration, Instant},
};

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
