// A neutral slate tile with a soft blue sun, shared by the executable and tray.
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let px = (x as f32 + 0.5) / size as f32;
            let py = (y as f32 + 0.5) / size as f32;
            let corner = ((px - 0.5).abs() - 0.28)
                .max(0.0)
                .hypot(((py - 0.5).abs() - 0.28).max(0.0));
            let dx = px - 0.5;
            let dy = py - 0.5;
            let radius = dx.hypot(dy);
            let ray = (0..8).any(|i| {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let along = dx * a.cos() + dy * a.sin();
                let across = -dx * a.sin() + dy * a.cos();
                (0.23..=0.32).contains(&along) && across.abs() < 0.024
            });
            let sun = radius < 0.16 || ray;
            let color = if sun { [185, 204, 245] } else { [44, 48, 59] };
            pixels.extend_from_slice(&[
                color[0],
                color[1],
                color[2],
                if corner <= 0.19 { 255 } else { 0 },
            ]);
        }
    }
    pixels
}

// ICO with a standard 32-bit DIB: no image file or decoder is needed at runtime.
#[allow(dead_code)]
pub fn ico() -> Vec<u8> {
    let size = 64u32;
    let mask_size = size.div_ceil(32) * 4 * size;
    let image_size = 40 + size * size * 4 + mask_size;
    let mut out = Vec::new();
    for value in [0u16, 1, 1] {
        out.extend(value.to_le_bytes());
    }
    out.extend([size as u8, size as u8, 0, 0]);
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(image_size.to_le_bytes());
    out.extend(22u32.to_le_bytes());
    for value in [40, size, size * 2] {
        out.extend(value.to_le_bytes());
    }
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    for value in [0, size * size * 4, 0, 0, 0, 0] {
        out.extend(value.to_le_bytes());
    }
    let pixels = rgba(size);
    for row in (0..size as usize).rev() {
        for pixel in pixels[row * size as usize * 4..(row + 1) * size as usize * 4].chunks_exact(4)
        {
            out.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    out.resize(out.len() + mask_size as usize, 0);
    out
}
