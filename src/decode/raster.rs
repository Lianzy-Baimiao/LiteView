//! 静态图：image crate 解码 → 按 EXIF 方向转正 → 8 位 RGBA；HDR / EXR 从线性转到 sRGB 显示。

use std::io::Cursor;
use std::sync::Arc;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use rayon::prelude::*;

use super::{Content, Decoded, Error, Info, Options, check_size, format_name, img_error};

pub fn decode(data: &[u8], format: ImageFormat, opts: &Options) -> Result<Decoded, Error> {
    let mut reader = ImageReader::new(Cursor::new(data));
    reader.set_format(format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(opts.max_alloc);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(img_error)?;
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    if u64::from(w) * u64::from(h) * 4 > opts.max_alloc {
        return Err(Error::TooLarge { width: w, height: h });
    }
    let exif = decoder.exif_metadata().ok().flatten();
    let icc = decoder.icc_profile().ok().flatten();
    let orientation = match decoder.orientation() {
        Ok(Orientation::NoTransforms) | Err(_) => {
            exif.as_deref().and_then(Orientation::from_exif_chunk).unwrap_or(Orientation::NoTransforms)
        }
        Ok(o) => o,
    };
    let color = decoder.color_type();
    let mut img = DynamicImage::from_decoder(decoder).map_err(img_error)?;
    img.apply_orientation(orientation);
    let channels = color.channel_count();
    let rgba = to_rgba8(img, format);
    Ok(Decoded {
        info: Info {
            format: format_name(format),
            width: rgba.width(),
            height: rgba.height(),
            bits: color.bits_per_pixel() / u16::from(channels.max(1)),
            channels,
            has_alpha: color.has_alpha(),
            file_size: 0,
            exif: exif.map(Arc::new),
            icc: icc.map(Arc::new),
        },
        content: Content::Still { image: Arc::new(rgba), levels: Vec::new() },
    })
}

fn to_rgba8(img: DynamicImage, format: ImageFormat) -> RgbaImage {
    let hdr = matches!(format, ImageFormat::Hdr | ImageFormat::OpenExr);
    match img {
        DynamicImage::ImageRgb32F(f) if hdr => tonemap(f.width(), f.height(), f.as_raw(), 3),
        DynamicImage::ImageRgba32F(f) if hdr => tonemap(f.width(), f.height(), f.as_raw(), 4),
        other => other.into_rgba8(),
    }
}

/// 线性 → sRGB；1.0 为白，更亮的部分截断
fn tonemap(w: u32, h: u32, src: &[f32], ch: usize) -> RgbaImage {
    let mut out = vec![0u8; w as usize * h as usize * 4];
    out.par_chunks_mut(4).zip(src.par_chunks(ch)).for_each(|(o, s)| {
        for i in 0..3 {
            o[i] = (linear_to_srgb(s[i]) * 255.0 + 0.5) as u8;
        }
        o[3] = if ch == 4 { (s[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8 } else { 255 };
    });
    RgbaImage::from_raw(w, h, out).expect("buffer size")
}

fn linear_to_srgb(v: f32) -> f32 {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// 逐级缩小一半，直到长边 ≤ 512
pub fn pyramid(img: &RgbaImage) -> Vec<Arc<RgbaImage>> {
    let mut out: Vec<Arc<RgbaImage>> = Vec::new();
    loop {
        let src: &RgbaImage = out.last().map_or(img, |l| l);
        if src.width().max(src.height()) <= 512 {
            break;
        }
        let half = Arc::new(halve(src));
        out.push(half);
    }
    out
}

/// 2×2 平均缩小一半（按不透明度加权，透明像素的颜色不会渗进来）；奇数边重复最后一行 / 列
pub fn halve(src: &RgbaImage) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let s = src.as_raw();
    let mut out = vec![0u8; nw * nh * 4];
    out.par_chunks_mut(nw * 4).enumerate().for_each(|(y, row)| {
        let (y0, y1) = ((y * 2).min(h - 1), (y * 2 + 1).min(h - 1));
        for x in 0..nw {
            let (x0, x1) = ((x * 2).min(w - 1), (x * 2 + 1).min(w - 1));
            let idx = [(y0 * w + x0) * 4, (y0 * w + x1) * 4, (y1 * w + x0) * 4, (y1 * w + x1) * 4];
            let a: u32 = idx.iter().map(|&i| u32::from(s[i + 3])).sum();
            let o = &mut row[x * 4..x * 4 + 4];
            if a == 0 {
                o.fill(0);
                continue;
            }
            for c in 0..3 {
                let sum: u32 = idx.iter().map(|&i| u32::from(s[i + c]) * u32::from(s[i + 3])).sum();
                o[c] = ((sum + a / 2) / a) as u8;
            }
            o[3] = ((a + 2) / 4) as u8;
        }
    });
    RgbaImage::from_raw(nw as u32, nh as u32, out).expect("buffer size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halve_weights_alpha_and_handles_odd_sizes() {
        let mut img = RgbaImage::new(3, 3);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 255, 0, 0])); // 透明，颜色不应渗入
        let h = halve(&img);
        assert_eq!(h.dimensions(), (2, 2));
        let p = h.get_pixel(0, 0).0;
        assert_eq!(&p[..3], &[255, 0, 0]);
        assert_eq!(p[3], 64);
        assert_eq!(h.get_pixel(1, 1).0, [0, 0, 0, 0]);
    }

    #[test]
    fn pyramid_stops_at_512() {
        let levels = pyramid(&RgbaImage::new(2000, 300));
        let sizes: Vec<_> = levels.iter().map(|l| l.dimensions()).collect();
        assert_eq!(sizes, [(1000, 150), (500, 75)]);
    }

    #[test]
    fn exif_orientation_applied() {
        // 2×1 JPEG，EXIF Orientation = 6（顺时针 90°）→ 显示为 1×2
        let mut jpeg = Vec::new();
        let img = image::RgbImage::from_pixel(2, 1, image::Rgb([200, 10, 10]));
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&img).unwrap();
        let exif: &[u8] = b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
        let mut app1 = vec![0xFF, 0xE1];
        app1.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        app1.extend_from_slice(exif);
        let with_exif = [&jpeg[..2], &app1, &jpeg[2..]].concat();
        let d = super::super::decode_bytes(&with_exif, "jpg", &Options::default()).unwrap();
        assert_eq!((d.info.width, d.info.height), (1, 2));
    }
}
