// A neutral slate tile with a soft blue sun for the executable and window.
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

/// A full-size sun with heavier rays, drawn directly at the Windows tray size.
#[allow(dead_code)] // This module is also compiled by build.rs for the executable icon.
pub fn tray_rgba(size: u32) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    const BLUE: [u8; 3] = [104, 155, 240];
    let rays: Vec<_> = (0..8)
        .map(|i| (i as f32 * std::f32::consts::FRAC_PI_4).sin_cos())
        .collect();
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut covered = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let dx = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32 - 0.5;
                    let dy = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32 - 0.5;
                    let sun = dx.hypot(dy) <= 0.22
                        || rays.iter().any(|&(sin, cos)| {
                            let along = dx * cos + dy * sin;
                            let across = -dx * sin + dy * cos;
                            (along - along.clamp(0.335, 0.43)).hypot(across) <= 0.055
                        });
                    covered += u32::from(sun);
                }
            }
            pixels.extend_from_slice(&[
                BLUE[0],
                BLUE[1],
                BLUE[2],
                ((covered * 255 + SAMPLES * SAMPLES / 2) / (SAMPLES * SAMPLES)) as u8,
            ]);
        }
    }
    pixels
}

// ICO with a standard 32-bit DIB: no image file or decoder is needed at runtime.
#[allow(dead_code)]
pub fn ico() -> Vec<u8> {
    const SIZES: [u32; 8] = [16, 20, 24, 32, 48, 64, 128, 256];
    let images: Vec<_> = SIZES.iter().copied().map(dib).collect();
    let mut out = Vec::new();
    for value in [0u16, 1, SIZES.len() as u16] {
        out.extend(value.to_le_bytes());
    }
    let mut offset = 6 + SIZES.len() as u32 * 16;
    for (&size, image) in SIZES.iter().zip(&images) {
        out.extend([size as u8, size as u8, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((image.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend(image);
    }
    out
}

#[allow(dead_code)]
fn dib(size: u32) -> Vec<u8> {
    let mask_size = size.div_ceil(32) * 4 * size;
    let mut out = Vec::new();
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
        for pixel in pixels[row * size as usize * 4..(row + 1) * size as usize * 4]
            .as_chunks::<4>()
            .0
        {
            out.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    let mask_start = out.len();
    out.resize(mask_start + mask_size as usize, 0);
    let stride = size.div_ceil(32) as usize * 4;
    for y in 0..size as usize {
        for x in 0..size as usize {
            if pixels[(y * size as usize + x) * 4 + 3] == 0 {
                out[mask_start + (size as usize - 1 - y) * stride + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_directory_contains_complete_native_images() {
        let icon = ico();
        assert_eq!(&icon[..6], &[0, 0, 1, 0, 8, 0]);
        let mut end = 6 + 8 * 16;
        for (index, size) in [16usize, 20, 24, 32, 48, 64, 128, 256]
            .into_iter()
            .enumerate()
        {
            let entry = &icon[6 + index * 16..6 + (index + 1) * 16];
            assert_eq!(entry[0], size as u8);
            assert_eq!(entry[1], size as u8);
            let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
            assert_eq!(offset, end);
            assert_eq!(length, 40 + size * size * 4 + size.div_ceil(32) * 4 * size);
            let image = &icon[offset..offset + length];
            assert_eq!(
                u32::from_le_bytes(image[4..8].try_into().unwrap()),
                size as u32
            );
            assert_eq!(
                u32::from_le_bytes(image[8..12].try_into().unwrap()),
                size as u32 * 2
            );
            assert!(
                image[40..40 + size * size * 4]
                    .chunks_exact(4)
                    .any(|pixel| pixel[3] == 255)
            );
            assert!(image[40 + size * size * 4..].iter().any(|byte| *byte != 0));
            end += length;
        }
        assert_eq!(end, icon.len());
    }

    #[test]
    fn tray_artwork_remains_visible_at_native_small_sizes() {
        for size in [16u32, 20, 24, 32] {
            let pixels = tray_rgba(size);
            let points: Vec<_> = pixels
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 32)
                .map(|(i, _)| (i as u32 % size, i as u32 / size))
                .collect();
            let width = points.iter().map(|p| p.0).max().unwrap()
                - points.iter().map(|p| p.0).min().unwrap()
                + 1;
            let height = points.iter().map(|p| p.1).max().unwrap()
                - points.iter().map(|p| p.1).min().unwrap()
                + 1;
            assert!(width >= size * 7 / 8 && height >= size * 7 / 8);
            assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 0));
            assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 255));
        }
    }
}
