//! 静态图：image crate 解码 → 按 EXIF 方向转正 → 8 位 RGBA；HDR / EXR 从线性转到 sRGB 显示。

use std::io::Cursor;
use std::sync::Arc;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use rayon::prelude::*;

use super::{Content, Decoded, Error, Info, Options, format_name, img_error};

pub fn decode(data: &[u8], format: ImageFormat, opts: &Options) -> Result<Decoded, Error> {
    let mut reader = ImageReader::new(Cursor::new(data));
    reader.set_format(format);
    reader.limits(opts.image_limits());
    let mut decoder = reader.into_decoder().map_err(img_error)?;
    let (w, h) = decoder.dimensions();
    // Source pixels, orientation copy and RGBA conversion can coexist.
    opts.check_working(w, h, u64::from(decoder.color_type().bytes_per_pixel()) * 2 + 4)?;
    let exif = decoder.exif_metadata().ok().flatten();
    let icc = if format == ImageFormat::Tiff { super::tiff_meta::icc(data, opts)? } else { decoder.icc_profile().ok().flatten() };
    let orientation = match decoder.orientation() {
        Ok(Orientation::NoTransforms) | Err(_) => exif.as_deref().and_then(Orientation::from_exif_chunk).unwrap_or(Orientation::NoTransforms),
        Ok(o) => o,
    };
    let color = decoder.color_type();
    let channels = color.channel_count();
    let rgba = if let (Some(bpp), true) = (expandable(color), in_place(orientation)) {
        // 8 位灰度 / RGB / RGBA（大多数 JPEG、PNG）：直接解到最后的 RGBA 缓冲区开头，再原地展开；
        // 不旋转 90° 时翻转也原地做。以前原始像素和 RGBA 同时在内存里（1200 万像素 RGB 多 36 MB）
        let mut buf = vec![0u8; w as usize * h as usize * 4];
        decoder.read_image(&mut buf[..w as usize * h as usize * bpp]).map_err(img_error)?;
        expand_in_place(&mut buf, bpp);
        let mut img = RgbaImage::from_raw(w, h, buf).expect("buffer size");
        orient_in_place(&mut img, orientation);
        img
    } else {
        let mut img = DynamicImage::from_decoder(decoder).map_err(img_error)?;
        img.apply_orientation(orientation);
        to_rgba8(img, format)
    };
    Ok(Decoded {
        info: Info {
            format: format_name(format),
            width: rgba.width(),
            height: rgba.height(),
            bits: color.bits_per_pixel() / u16::from(channels.max(1)),
            channels,
            has_alpha: color.has_alpha(),
            file_size: 0,
            jpeg_quality: (format == ImageFormat::Jpeg).then(|| super::jpeg_quality::estimate(data)).flatten(),
            exif: exif.map(Arc::new),
            icc: icc.map(Arc::new),
        },
        content: Content::Still { image: Arc::new(rgba), levels: Vec::new() },
    })
}

/// 能在 RGBA 缓冲区里原地展开的像素格式：每像素几个字节
fn expandable(c: image::ColorType) -> Option<usize> {
    use image::ColorType::{L8, La8, Rgb8, Rgba8};
    matches!(c, L8 | La8 | Rgb8 | Rgba8).then(|| usize::from(c.bytes_per_pixel()))
}

/// 不改变宽高的方向（不转 90°）：可以在 RGBA 上原地翻转
fn in_place(o: Orientation) -> bool {
    matches!(o, Orientation::NoTransforms | Orientation::Rotate180 | Orientation::FlipHorizontal | Orientation::FlipVertical)
}

/// buf 开头是每像素 bpp 字节的灰度 / 灰度 + 透明 / RGB / RGBA，从后往前展开成 RGBA（写的位置总不在还没读的前面）
fn expand_in_place(buf: &mut [u8], bpp: usize) {
    let n = buf.len() / 4;
    for i in (0..n).rev() {
        let s = i * bpp;
        let p = match bpp {
            1 => [buf[s], buf[s], buf[s], 255],
            2 => [buf[s], buf[s], buf[s], buf[s + 1]],
            3 => [buf[s], buf[s + 1], buf[s + 2], 255],
            _ => return,
        };
        buf[i * 4..i * 4 + 4].copy_from_slice(&p);
    }
}

fn orient_in_place(img: &mut RgbaImage, o: Orientation) {
    use image::imageops::{flip_horizontal_in_place, flip_vertical_in_place, rotate180_in_place};
    match o {
        Orientation::Rotate180 => rotate180_in_place(img),
        Orientation::FlipHorizontal => flip_horizontal_in_place(img),
        Orientation::FlipVertical => flip_vertical_in_place(img),
        _ => {}
    }
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

    /// 原地展开 + 原地翻转与以前的「解成原始格式 → 转方向 → 转 RGBA」逐像素相同
    #[test]
    fn in_place_expansion_matches_the_old_conversion() {
        use image::{GrayAlphaImage, GrayImage, RgbImage};
        let (w, h) = (5u32, 3u32);
        let byte = |i: usize| (i * 37 % 251) as u8;
        let bufs: Vec<(DynamicImage, usize)> = vec![
            (DynamicImage::ImageLuma8(GrayImage::from_fn(w, h, |x, y| image::Luma([byte((y * w + x) as usize)]))), 1),
            (DynamicImage::ImageLumaA8(GrayAlphaImage::from_fn(w, h, |x, y| image::LumaA([byte((y * w + x) as usize), byte(x as usize * 9)]))), 2),
            (DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| image::Rgb([byte(x as usize), byte(y as usize * 5), byte((x + y) as usize * 3)]))), 3),
            (DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| image::Rgba([byte(x as usize), byte(y as usize), 7, byte(x as usize * 11)]))), 4),
        ];
        for (src, bpp) in bufs {
            assert_eq!(expandable(src.color()), Some(bpp));
            for o in [Orientation::NoTransforms, Orientation::Rotate180, Orientation::FlipHorizontal, Orientation::FlipVertical] {
                let mut old = src.clone();
                old.apply_orientation(o);
                let old = old.into_rgba8();
                let mut buf = vec![0u8; (w * h * 4) as usize];
                buf[..src.as_bytes().len()].copy_from_slice(src.as_bytes());
                expand_in_place(&mut buf, bpp);
                let mut new = RgbaImage::from_raw(w, h, buf).unwrap();
                orient_in_place(&mut new, o);
                assert_eq!(new, old, "{:?} {o:?}", src.color());
            }
        }
        // 转 90° 的、16 位的还走以前的路
        assert!(!in_place(Orientation::Rotate90));
        assert_eq!(expandable(image::ColorType::Rgb16), None);
    }

    /// 整个解码：8 位 RGB 的 PNG 走新路，结果与 image 自己转的一样
    #[test]
    fn rgb_png_decodes_to_the_same_pixels() {
        let src = image::RgbImage::from_fn(7, 4, |x, y| image::Rgb([(x * 30) as u8, (y * 60) as u8, (x * y) as u8]));
        let mut png = Vec::new();
        src.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let d = decode(&png, ImageFormat::Png, &Options::default()).unwrap();
        let Content::Still { image, .. } = d.content else { panic!("still") };
        assert_eq!(*image, DynamicImage::ImageRgb8(src).into_rgba8());
        assert_eq!((d.info.channels, d.info.has_alpha), (3, false));
    }

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
