//! 静态 GIF 导出：少色图保持原色，多色图 NeuQuant + 蛇形 Floyd–Steinberg。
//! GIF 只有透明/不透明：沿用非零 alpha 视作不透明的规则；透明像素不扩散色误差。

use std::collections::{BTreeMap, BTreeSet};

use color_quant::NeuQuant;
use image::RgbaImage;

pub(super) fn encode(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width > u16::MAX.into() || height > u16::MAX.into() {
        return Err("GIF dimensions must be between 1 and 65535".into());
    }
    let transparent = image.pixels().any(|p| p[3] == 0);
    let limit = if transparent { 255 } else { 256 };
    let mut colors = BTreeSet::new();
    for p in image.pixels().filter(|p| p[3] != 0) {
        colors.insert([p[0], p[1], p[2]]);
        if colors.len() > limit {
            break;
        }
    }
    let (mut palette, indices) = if colors.len() <= limit {
        let lookup: BTreeMap<_, _> = colors.iter().copied().enumerate().map(|(i, c)| (c, i as u8)).collect();
        let trans = colors.len() as u8;
        let indices = image.pixels().map(|p| if p[3] == 0 { trans } else { lookup[&[p[0], p[1], p[2]]] }).collect();
        (colors.into_iter().flatten().collect::<Vec<u8>>(), indices)
    } else {
        // Exclude hidden RGB from palette training and reserve a separate transparent index.
        let pixels: Vec<u8> = image.pixels().filter(|p| p[3] != 0).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        let quantizer = NeuQuant::new(10, limit, &pixels);
        let palette = quantizer.color_map_rgb();
        (palette, dither(image, &quantizer, limit as u8))
    };
    let transparent = transparent.then(|| {
        let index = (palette.len() / 3) as u8;
        palette.extend_from_slice(&[0, 0, 0]);
        index
    });
    let frame = ::gif::Frame::from_palette_pixels(width as u16, height as u16, indices, palette, transparent);
    let mut bytes = Vec::new();
    {
        let mut encoder = ::gif::Encoder::new(&mut bytes, width as u16, height as u16, &[]).map_err(|e| e.to_string())?;
        encoder.write_frame(&frame).map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}

fn dither(image: &RgbaImage, quantizer: &NeuQuant, transparent: u8) -> Vec<u8> {
    let width = image.width() as usize;
    let mut indices = vec![0; width * image.height() as usize];
    let (mut row, mut next) = (vec![[0.0f32; 3]; width + 2], vec![[0.0f32; 3]; width + 2]);
    for y in 0..image.height() as usize {
        let forward = y % 2 == 0;
        for step in 0..width {
            let x = if forward { step } else { width - 1 - step };
            let p = image.get_pixel(x as u32, y as u32);
            if p[3] == 0 {
                indices[y * width + x] = transparent;
                continue;
            }
            let pos = x + 1;
            let rgb: [f32; 3] = std::array::from_fn(|c| (f32::from(p[c]) + row[pos][c]).clamp(0.0, 255.0));
            let pixel = [rgb[0].round() as u8, rgb[1].round() as u8, rgb[2].round() as u8, 255];
            let index = quantizer.index_of(&pixel);
            let mapped = quantizer.lookup(index).expect("palette index");
            indices[y * width + x] = index as u8;
            let ahead = if forward { pos + 1 } else { pos - 1 };
            let behind = if forward { pos - 1 } else { pos + 1 };
            for c in 0..3 {
                let error = rgb[c] - f32::from(mapped[c]);
                row[ahead][c] += error * (7.0 / 16.0);
                next[behind][c] += error * (3.0 / 16.0);
                next[pos][c] += error * (5.0 / 16.0);
                next[ahead][c] += error * (1.0 / 16.0);
            }
        }
        std::mem::swap(&mut row, &mut next);
        next.fill([0.0; 3]);
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn decode(bytes: &[u8]) -> RgbaImage {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Gif).unwrap().into_rgba8()
    }

    #[test]
    fn low_color_art_stays_exact_and_transparency_is_preserved() {
        let image = RgbaImage::from_fn(256, 2, |x, y| if y == 0 || x == 255 { Rgba([91, 73, 200, 0]) } else { Rgba([x as u8, 12, 190, 255]) });
        let result = decode(&encode(&image).unwrap());
        for (a, b) in image.pixels().zip(result.pixels()) {
            assert_eq!(a[3], b[3]);
            if a[3] != 0 {
                assert_eq!(a, b);
            }
        }
        let opaque = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, 23, 61, 255]));
        assert_eq!(decode(&encode(&opaque).unwrap()), opaque);
        let clear = RgbaImage::from_pixel(1, 1, Rgba([12, 80, 33, 0]));
        assert_eq!(decode(&encode(&clear).unwrap()).get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn high_color_export_never_turns_opaque_pixels_transparent() {
        let image = RgbaImage::from_fn(80, 60, |x, y| Rgba([(x * 3) as u8, (y * 4) as u8, ((x * y) % 256) as u8, if x % 7 == 0 { 0 } else { 255 }]));
        let bytes = encode(&image).unwrap();
        assert_eq!(bytes, encode(&image).unwrap(), "deterministic export");
        let result = decode(&bytes);
        for (a, b) in image.pixels().zip(result.pixels()) {
            assert_eq!(a[3], b[3]);
        }
    }

    #[test]
    fn error_diffusion_improves_local_average_on_gradient() {
        let image = RgbaImage::from_fn(256, 64, |x, _| Rgba([x as u8, x as u8, x as u8, 255]));
        // Deliberately small palette isolates the diffusion quality from palette selection.
        let quantizer = NeuQuant::new(1, 8, image.as_raw());
        let plain: Vec<_> = image.pixels().map(|p| quantizer.index_of(&p.0) as u8).collect();
        let diffused = dither(&image, &quantizer, 0);
        let error = |indices: &[u8]| -> f64 {
            let mut sum = 0.0;
            for by in (0..64).step_by(8) {
                for bx in (0..256).step_by(8) {
                    let mut delta = 0.0;
                    for y in by..by + 8 {
                        for x in bx..bx + 8 {
                            delta += f64::from(quantizer.lookup(indices[y * 256 + x] as usize).unwrap()[0]) - x as f64;
                        }
                    }
                    sum += (delta / 64.0).powi(2);
                }
            }
            sum
        };
        assert!(error(&diffused) < error(&plain) * 0.5, "dither={} plain={}", error(&diffused), error(&plain));
    }

    #[test]
    fn invalid_dimensions_return_error_instead_of_panicking() {
        assert!(encode(&RgbaImage::new(0, 1)).is_err());
        assert!(encode(&RgbaImage::new(65536, 1)).is_err());
        for (w, h) in [(1, 400), (400, 1)] {
            let image = RgbaImage::from_fn(w, h, |x, y| Rgba([(x + y) as u8, ((x + y) / 2) as u8, 10, 255]));
            assert_eq!(decode(&encode(&image).unwrap()).dimensions(), (w, h));
        }
    }
}
