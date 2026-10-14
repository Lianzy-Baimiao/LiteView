//! 保存：编码各格式（都经过 fsutil::atomic_write，不在原文件上覆盖写入）；按显示方向转像素；
//! JPEG 保存旋转时只改 EXIF 方向标记，画质无损。

mod gif;

use std::io::Cursor;
use std::path::Path;

use image::metadata::Orientation;
use image::{DynamicImage, ImageFormat, RgbaImage};

use crate::viewer::view::Transform;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Jpeg,
    WebP,
    Bmp,
    Gif,
    Tiff,
}

impl Format {
    pub const ALL: [Format; 6] = [Format::Png, Format::Jpeg, Format::WebP, Format::Bmp, Format::Gif, Format::Tiff];

    pub fn name(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::WebP => "WebP",
            Format::Bmp => "BMP",
            Format::Gif => "GIF",
            Format::Tiff => "TIFF",
        }
    }

    pub fn exts(self) -> &'static [&'static str] {
        match self {
            Format::Png => &["png"],
            Format::Jpeg => &["jpg", "jpeg", "jpe", "jfif"],
            Format::WebP => &["webp"],
            Format::Bmp => &["bmp", "dib"],
            Format::Gif => &["gif"],
            Format::Tiff => &["tif", "tiff"],
        }
    }

    pub fn from_path(p: &Path) -> Option<Format> {
        let e = p.extension()?.to_str()?.to_ascii_lowercase();
        Format::ALL.into_iter().find(|f| f.exts().contains(&e.as_str()))
    }
}

/// 写进文件的元数据
#[derive(Clone, Default)]
pub struct Meta {
    /// EXIF（TIFF 结构，不含 "Exif\0\0"）；保存前先用 clean_exif 清理
    pub exif: Option<Vec<u8>>,
    /// 颜色配置文件（不是个人信息，打码后也保留，否则颜色会变）
    pub icc: Option<Vec<u8>>,
}

/// 编码成文件内容。JPEG / BMP 没有透明通道：透明部分铺白色
pub fn encode(img: &RgbaImage, fmt: Format, quality: u8) -> Result<Vec<u8>, String> {
    encode_with(img, fmt, quality, &Meta::default())
}

pub fn encode_with(img: &RgbaImage, fmt: Format, quality: u8, meta: &Meta) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    if fmt == Format::Gif {
        return gif::encode(img);
    }
    let opaque = img.pixels().all(|p| p.0[3] == 255);
    let rgb = opaque || matches!(fmt, Format::Jpeg | Format::Bmp);
    let flat;
    let (bytes, color): (&[u8], image::ExtendedColorType) = if rgb {
        flat = flatten(img);
        (flat.as_raw(), image::ExtendedColorType::Rgb8)
    } else {
        (img.as_raw(), image::ExtendedColorType::Rgba8)
    };
    let (w, h) = img.dimensions();
    let mut out = Cursor::new(Vec::new());
    let add_meta = |enc: &mut dyn ImageEncoderMeta| {
        if let Some(icc) = &meta.icc {
            enc.icc(icc.clone());
        }
        if let Some(exif) = &meta.exif {
            enc.exif(exif.clone());
        }
    };
    let r = match fmt {
        Format::Jpeg => {
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
            add_meta(&mut enc);
            enc.write_image(bytes, w, h, color)
        }
        Format::Png => {
            let mut enc = image::codecs::png::PngEncoder::new(&mut out);
            add_meta(&mut enc);
            enc.write_image(bytes, w, h, color)
        }
        Format::WebP => {
            let mut enc = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            add_meta(&mut enc);
            enc.write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        }
        Format::Tiff => {
            let mut enc = image::codecs::tiff::TiffEncoder::new(&mut out);
            if let Some(icc) = &meta.icc {
                let _ = enc.set_icc_profile(icc.clone());
            }
            enc.write_image(bytes, w, h, color)
        }
        Format::Bmp => image::codecs::bmp::BmpEncoder::new(&mut out).write_image(bytes, w, h, color),
        Format::Gif => unreachable!("handled before flattening"),
    };
    r.map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// 各编码器设置元数据（不支持时忽略）
trait ImageEncoderMeta {
    fn icc(&mut self, icc: Vec<u8>);
    fn exif(&mut self, exif: Vec<u8>);
}

impl<T: image::ImageEncoder> ImageEncoderMeta for T {
    fn icc(&mut self, icc: Vec<u8>) {
        let _ = self.set_icc_profile(icc);
    }
    fn exif(&mut self, exif: Vec<u8>) {
        let _ = self.set_exif_metadata(exif);
    }
}

/// 编辑后保存时清理 EXIF：去掉缩略图（可能是改之前的原图）、厂商私有数据（MakerNote，里面也可能有预览图）、
/// XMP 与指向文件内其它位置的字段；方向改成 1（像素已经转正）；图片尺寸字段去掉（裁剪、调整大小后不对）。
/// 解析或重写失败时返回 None（不写 EXIF）
pub fn clean_exif(raw: &[u8]) -> Option<Vec<u8>> {
    use exif::{Context, In, Tag, Value};
    let parsed = exif::Reader::new().read_raw(raw.to_vec()).ok()?;
    let drop = |t: Tag| {
        matches!(t, Tag::MakerNote | Tag::ImageWidth | Tag::ImageLength | Tag::PixelXDimension | Tag::PixelYDimension)
            || matches!(t, Tag(Context::Tiff, 0x02BC | 0x014A | 0x8773 | 0x83BB | 0x8649 | 0x935C))
    };
    let mut fields: Vec<exif::Field> =
        parsed.fields().filter(|f| f.ifd_num == In::PRIMARY && !drop(f.tag) && !matches!(f.value, Value::Unknown(..))).cloned().collect();
    for f in &mut fields {
        if f.tag == Tag::Orientation {
            f.value = Value::Short(vec![1]);
        }
    }
    if fields.is_empty() {
        return None;
    }
    let mut w = exif::experimental::Writer::new();
    for f in &fields {
        w.push_field(f);
    }
    let mut out = Cursor::new(Vec::new());
    w.write(&mut out, parsed.little_endian()).ok()?;
    Some(out.into_inner())
}

/// 透明部分与白色混合
fn flatten(img: &RgbaImage) -> image::RgbImage {
    image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let a = u32::from(p[3]);
        let mix = |c: u8| ((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

pub fn write(path: &Path, img: &RgbaImage, fmt: Format, quality: u8) -> Result<(), String> {
    write_with(path, img, fmt, quality, &Meta::default())
}

/// 编码后安全写入（临时文件 → 替换原文件）
pub fn write_with(path: &Path, img: &RgbaImage, fmt: Format, quality: u8, meta: &Meta) -> Result<(), String> {
    let data = encode_with(img, fmt, quality, meta)?;
    crate::fsutil::atomic_write(path, |w| w.write_all(&data)).map_err(|e| e.to_string())
}

/// 按显示变换转像素
pub fn apply_transform(img: &RgbaImage, xf: Transform) -> RgbaImage {
    if xf.is_identity() {
        return img.clone();
    }
    let mut d = DynamicImage::ImageRgba8(img.clone());
    d.apply_orientation(Orientation::from_exif(exif_of(xf)).unwrap_or(Orientation::NoTransforms));
    d.into_rgba8()
}

/// EXIF 方向值（1 ~ 8）对应的显示变换
pub fn transform_of(exif: u8) -> Transform {
    let t = |rotation, flip_h, flip_v| Transform { rotation, flip_h, flip_v };
    match exif {
        2 => t(0, true, false),
        3 => t(2, false, false),
        4 => t(0, false, true),
        5 => t(1, true, false),
        6 => t(1, false, false),
        7 => t(1, false, true),
        8 => t(3, false, false),
        _ => t(0, false, false),
    }
}

fn corners(f: impl Fn(f32, f32) -> egui::Pos2) -> [egui::Pos2; 3] {
    [f(0.0, 0.0), f(1.0, 0.0), f(0.0, 1.0)]
}

fn unit() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0))
}

/// 显示变换对应的 EXIF 方向值
pub fn exif_of(xf: Transform) -> u8 {
    let want = corners(|u, v| xf.map(unit(), u, v));
    (1..=8).find(|&e| corners(|u, v| transform_of(e).map(unit(), u, v)) == want).unwrap_or(1)
}

/// 先按 EXIF 方向显示、再做显示变换，合起来的 EXIF 方向值
pub fn compose(exif: u8, xf: Transform) -> u8 {
    let first = transform_of(exif);
    let want = corners(|u, v| {
        let p = first.map(unit(), u, v);
        xf.map(unit(), p.x, p.y)
    });
    (1..=8).find(|&e| corners(|u, v| transform_of(e).map(unit(), u, v)) == want).unwrap_or(1)
}

/// 能否把旋转写回原文件
pub fn can_save_rotation(path: &Path) -> bool {
    let e = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    matches!(e.as_str(), "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "bmp" | "tif" | "tiff" | "tga" | "qoi")
}

/// 把显示变换写回原文件
pub fn save_rotation(path: &Path, xf: Transform, quality: u8) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    if let crate::decode::Kind::Raster(ImageFormat::Jpeg) = crate::decode::sniff::sniff(&data, ext)
        && let Some(patched) = jpeg_set_orientation(&data, compose(jpeg_orientation(&data).unwrap_or(1), xf))
    {
        return crate::fsutil::atomic_write(path, |w| w.write_all(&patched)).map_err(|e| e.to_string());
    }
    let d = crate::decode::decode_bytes(&data, ext, &crate::decode::Options { pyramid: false, ..Default::default() }).map_err(|e| format!("{e:?}"))?;
    let fmt = Format::from_path(path).unwrap_or(Format::Png);
    write(path, &apply_transform(d.first_image(), xf), fmt, quality)
}

/// 找到 JPEG 的 APP1 Exif 段：返回 TIFF 头在文件里的位置
fn find_exif(data: &[u8]) -> Option<usize> {
    let mut i = 2;
    while i + 4 <= data.len() && data[i] == 0xFF {
        let marker = data[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if marker == 0xE1 && data.get(i + 4..i + 10) == Some(b"Exif\0\0") {
            return Some(i + 10);
        }
        i += 2 + len;
    }
    None
}

/// IFD0 里方向标记的值所在的位置与字节序
fn orientation_slot(data: &[u8]) -> Option<(usize, bool)> {
    let tiff = find_exif(data)?;
    let be = match data.get(tiff..tiff + 2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let rd16 = |o: usize| data.get(o..o + 2).map(|b| if be { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) });
    let rd32 =
        |o: usize| data.get(o..o + 4).map(|b| if be { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) });
    let ifd = tiff + rd32(tiff + 4)? as usize;
    let n = rd16(ifd)? as usize;
    (0..n).map(|k| ifd + 2 + k * 12).find(|&e| rd16(e) == Some(0x0112)).map(|e| (e + 8, be))
}

pub fn jpeg_orientation(data: &[u8]) -> Option<u8> {
    let (at, be) = orientation_slot(data)?;
    let b = data.get(at..at + 2)?;
    Some((if be { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) }) as u8)
}

/// 改 JPEG 的方向标记；没有 EXIF 时插入一个最小的 EXIF 段。有 EXIF 但没有方向标记时返回 None（改走重新编码）
pub fn jpeg_set_orientation(data: &[u8], value: u8) -> Option<Vec<u8>> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    if let Some((at, be)) = orientation_slot(data) {
        let mut out = data.to_vec();
        let v = u16::from(value);
        out[at..at + 2].copy_from_slice(&if be { v.to_be_bytes() } else { v.to_le_bytes() });
        return Some(out);
    }
    if find_exif(data).is_some() {
        return None;
    }
    let mut seg = vec![0xFF, 0xE1, 0, 34];
    seg.extend_from_slice(b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01");
    seg.extend_from_slice(&[0, value, 0, 0, 0, 0, 0, 0]);
    // 放在 JFIF（APP0）之后
    let at = if data.get(2..4) == Some(&[0xFF, 0xE0]) { 4 + u16::from_be_bytes([data[4], data[5]]) as usize } else { 2 };
    Some([&data[..at], &seg, &data[at..]].concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_table_matches_image_crate() {
        let src = RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8 * 50, y as u8 * 50, 0, 255]));
        for e in 1..=8u8 {
            let mut d = DynamicImage::ImageRgba8(src.clone());
            d.apply_orientation(Orientation::from_exif(e).unwrap());
            let got = d.to_rgba8();
            let xf = transform_of(e);
            let r = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(got.width() as f32, got.height() as f32));
            for (x, y, p) in src.enumerate_pixels() {
                let q = xf.map(r, (x as f32 + 0.5) / 3.0, (y as f32 + 0.5) / 2.0);
                assert_eq!(got.get_pixel(q.x as u32, q.y as u32), p, "exif {e}");
            }
            assert_eq!(exif_of(xf), e);
        }
    }

    #[test]
    fn compose_rotations() {
        let cw = Transform { rotation: 1, ..Default::default() };
        assert_eq!(compose(1, cw), 6);
        assert_eq!(compose(6, cw), 3);
        assert_eq!(compose(8, cw), 1);
    }

    #[test]
    fn jpeg_orientation_patch_is_lossless() {
        let img = image::RgbImage::from_pixel(4, 2, image::Rgb([10, 200, 30]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&img).unwrap();
        assert_eq!(jpeg_orientation(&jpeg), None);
        let with = jpeg_set_orientation(&jpeg, 6).unwrap();
        assert_eq!(jpeg_orientation(&with), Some(6));
        let again = jpeg_set_orientation(&with, 3).unwrap();
        assert_eq!(again.len(), with.len());
        assert_eq!(jpeg_orientation(&again), Some(3));
        let d = crate::decode::decode_bytes(&with, "jpg", &crate::decode::Options::default()).unwrap();
        assert_eq!((d.info.width, d.info.height), (2, 4));
    }

    /// 带缩略图、厂商数据、GPS、方向 6 的 EXIF
    fn sample_exif() -> Vec<u8> {
        use exif::{Field, In, Rational, Tag, Value};
        let f = |tag, ifd_num, value| Field { tag, ifd_num, value };
        let fields = [
            f(Tag::Make, In::PRIMARY, Value::Ascii(vec![b"TestCam".to_vec()])),
            f(Tag::Orientation, In::PRIMARY, Value::Short(vec![6])),
            f(Tag::MakerNote, In::PRIMARY, Value::Undefined(vec![1, 2, 3, 4], 0)),
            f(Tag::PixelXDimension, In::PRIMARY, Value::Long(vec![4000])),
            f(
                Tag::GPSLatitude,
                In::PRIMARY,
                Value::Rational(vec![Rational { num: 31, denom: 1 }, Rational { num: 14, denom: 1 }, Rational { num: 0, denom: 1 }]),
            ),
            f(Tag::Compression, In::THUMBNAIL, Value::Short(vec![6])),
        ];
        let thumb = [0xFFu8, 0xD8, 0xFF, 0xD9];
        let mut w = exif::experimental::Writer::new();
        for x in &fields {
            w.push_field(x);
        }
        w.set_jpeg(&thumb, In::THUMBNAIL);
        let mut out = Cursor::new(Vec::new());
        w.write(&mut out, false).unwrap();
        out.into_inner()
    }

    #[test]
    fn clean_exif_drops_thumbnail_and_private_data() {
        use exif::{In, Tag};
        let raw = sample_exif();
        let before = exif::Reader::new().read_raw(raw.clone()).unwrap();
        assert!(before.fields().any(|f| f.ifd_num == In::THUMBNAIL));
        let cleaned = clean_exif(&raw).unwrap();
        let after = exif::Reader::new().read_raw(cleaned).unwrap();
        assert!(after.fields().all(|f| f.ifd_num == In::PRIMARY), "thumbnail IFD removed");
        assert!(after.get_field(Tag::MakerNote, In::PRIMARY).is_none());
        assert!(after.get_field(Tag::PixelXDimension, In::PRIMARY).is_none());
        assert_eq!(after.get_field(Tag::Orientation, In::PRIMARY).and_then(|f| f.value.get_uint(0)), Some(1));
        assert!(after.get_field(Tag::Make, In::PRIMARY).is_some());
        assert!(after.get_field(Tag::GPSLatitude, In::PRIMARY).is_some());
    }

    #[test]
    fn metadata_written_only_when_given() {
        let img = RgbaImage::from_pixel(8, 6, image::Rgba([90, 120, 200, 255]));
        let exif = clean_exif(&sample_exif());
        for fmt in [Format::Jpeg, Format::Png, Format::WebP] {
            let with = encode_with(&img, fmt, 90, &Meta { exif: exif.clone(), icc: None }).unwrap();
            let d = crate::decode::decode_bytes(&with, fmt.exts()[0], &crate::decode::Options::default()).unwrap();
            let e = d.info.exif.expect("exif kept");
            let r = exif::Reader::new().read_raw(e.to_vec()).unwrap();
            assert!(r.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).is_some(), "{fmt:?}");
            // 不给元数据时（打过码）文件里没有 EXIF
            let without = encode_with(&img, fmt, 90, &Meta::default()).unwrap();
            let d = crate::decode::decode_bytes(&without, fmt.exts()[0], &crate::decode::Options::default()).unwrap();
            assert!(d.info.exif.is_none(), "{fmt:?}");
        }
    }

    #[test]
    fn encode_formats_roundtrip() {
        let img = RgbaImage::from_pixel(5, 3, image::Rgba([1, 2, 3, 128]));
        for f in Format::ALL {
            let data = encode(&img, f, 90).unwrap();
            let d = crate::decode::decode_bytes(&data, f.exts()[0], &crate::decode::Options::default()).unwrap();
            assert_eq!((d.info.width, d.info.height), (5, 3), "{f:?}");
        }
    }
}
