//! 保存：编码各格式（都经过 fsutil::atomic_write，不在原文件上覆盖写入）；按显示方向转像素；
//! JPEG 保存旋转时只改 EXIF 方向标记，画质无损。

mod gif;
mod webp;

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

    pub fn supports_exif(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::WebP)
    }

    pub fn supports_icc(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::WebP | Self::Tiff)
    }

    pub fn full_alpha(self) -> bool {
        matches!(self, Self::Png | Self::WebP | Self::Tiff)
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
    let add_meta = |enc: &mut dyn ImageEncoderMeta| -> Result<(), String> {
        if let Some(icc) = &meta.icc {
            enc.icc(icc.clone())?;
        }
        if let Some(exif) = &meta.exif {
            enc.exif(exif.clone())?;
        }
        Ok(())
    };
    let r = match fmt {
        Format::Jpeg => {
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
            add_meta(&mut enc)?;
            enc.write_image(bytes, w, h, color)
        }
        Format::Png => {
            let mut enc = image::codecs::png::PngEncoder::new(&mut out);
            add_meta(&mut enc)?;
            enc.write_image(bytes, w, h, color)
        }
        Format::WebP => {
            let mut enc = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
            add_meta(&mut enc)?;
            enc.write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        }
        Format::Tiff => {
            let mut enc = image::codecs::tiff::TiffEncoder::new(&mut out);
            if let Some(icc) = &meta.icc {
                enc.set_icc_profile(icc.clone()).map_err(|e| e.to_string())?;
            }
            enc.write_image(bytes, w, h, color)
        }
        Format::Bmp => image::codecs::bmp::BmpEncoder::new(&mut out).write_image(bytes, w, h, color),
        Format::Gif => unreachable!("handled before flattening"),
    };
    r.map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

pub fn encode_with_mode(img: &RgbaImage, fmt: Format, quality: u8, meta: &Meta, webp_lossless: bool) -> Result<Vec<u8>, String> {
    if fmt == Format::WebP && !webp_lossless { webp::lossy(img, quality, meta) } else { encode_with(img, fmt, quality, meta) }
}

/// Metadata errors from formats that claim support must not be ignored.
trait ImageEncoderMeta {
    fn icc(&mut self, icc: Vec<u8>) -> Result<(), String>;
    fn exif(&mut self, exif: Vec<u8>) -> Result<(), String>;
}

impl<T: image::ImageEncoder> ImageEncoderMeta for T {
    fn icc(&mut self, icc: Vec<u8>) -> Result<(), String> {
        self.set_icc_profile(icc).map_err(|e| e.to_string())
    }
    fn exif(&mut self, exif: Vec<u8>) -> Result<(), String> {
        self.set_exif_metadata(exif).map_err(|e| e.to_string())
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
    write_with_mode(path, img, fmt, quality, meta, true)
}

pub fn write_with_mode(path: &Path, img: &RgbaImage, fmt: Format, quality: u8, meta: &Meta, webp_lossless: bool) -> Result<(), String> {
    let data = encode_with_mode(img, fmt, quality, meta, webp_lossless)?;
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

/// 能否把旋转存回原文件。`format` 是按文件内容认出的格式（`decode::Info::format`），不看扩展名：
/// JPEG、TIFF 只改方向标记；PNG、BMP、TGA、QOI 按原格式无损重新编码。APNG（动图）、GIF、WebP、HEIC 等不行，
/// 只取第一帧写回会把动画和别的帧丢掉
pub fn can_save_rotation(format: &str) -> bool {
    matches!(format, "JPEG" | "TIFF" | "PNG" | "BMP" | "TGA" | "QOI")
}

/// 把显示变换写回原文件（按文件内容认格式，写回同一种格式）：
/// - JPEG、TIFF 只改方向标记，图像数据一个字节都不动（TIFF 的其它页也不动）；
/// - PNG、BMP、TGA、QOI 按原格式、原位深无损重新编码，方向转进像素；PNG 留着颜色配置文件、EXIF（方向改成 1）、
///   文字、分辨率等；
/// - 别的（APNG 动图等）不写，返回错误
pub fn save_rotation(path: &Path, xf: Transform, opts: &crate::decode::Options) -> Result<(), String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > opts.max_source {
        return Err(crate::i18n::tr().err_too_large.into());
    }
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    // 坏文件让解码器 panic 时只算这次没存成（这里可能在界面线程上，关窗口前保存）
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rotated_file(data, ext, xf, opts)))
        .unwrap_or_else(|_| Err(crate::i18n::tr().err_corrupt.into()))?;
    crate::fsutil::atomic_write(path, |w| w.write_all(&out)).map_err(|e| e.to_string())
}

/// 转好的整个文件
fn rotated_file(data: Vec<u8>, ext: &str, xf: Transform, opts: &crate::decode::Options) -> Result<Vec<u8>, String> {
    use crate::decode::Kind;
    let t = crate::i18n::tr();
    match crate::decode::sniff::sniff(&data, ext) {
        Kind::Raster(ImageFormat::Jpeg) => {
            let value = compose(jpeg_orientation(&data).unwrap_or(1), xf);
            jpeg_set_orientation(&data, value).ok_or_else(|| t.rotate_tag_unwritable.to_owned())
        }
        Kind::Raster(ImageFormat::Tiff) => {
            let value = compose(tiff_orientation(&data).unwrap_or(1), xf);
            tiff_set_orientation(data, value).ok_or_else(|| t.rotate_tag_unwritable.to_owned())
        }
        Kind::Raster(ImageFormat::Png) if is_apng(&data) => Err(t.rotate_cannot_save.into()),
        Kind::Raster(f @ (ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Tga | ImageFormat::Qoi)) => reencode(&data, f, xf, opts),
        _ => Err(t.rotate_cannot_save.into()),
    }
}

/// PNG、BMP、TGA、QOI：按原格式、原像素格式（16 位的还是 16 位）解码，把原来的方向和这次的变换一起转进像素，再编码回同一种格式
fn reencode(data: &[u8], format: ImageFormat, xf: Transform, opts: &crate::decode::Options) -> Result<Vec<u8>, String> {
    use image::ImageDecoder;
    let corrupt = |e: image::ImageError| {
        log::warn!("保存旋转时解码失败：{e}");
        crate::i18n::tr().err_corrupt.to_owned()
    };
    let mut reader = image::ImageReader::new(Cursor::new(data));
    reader.set_format(format);
    reader.limits(opts.image_limits());
    let mut decoder = reader.into_decoder().map_err(corrupt)?;
    let (w, h) = decoder.dimensions();
    // 解出来的一份、转好方向的一份、编码出来的文件
    opts.check_working(w, h, u64::from(decoder.color_type().bytes_per_pixel()) * 3).map_err(|_| crate::i18n::tr().err_too_large.to_owned())?;
    let icc = decoder.icc_profile().ok().flatten();
    let exif = decoder.exif_metadata().ok().flatten();
    // 与看图时一样认方向（raster::decode）
    let shown = match decoder.orientation() {
        Ok(Orientation::NoTransforms) | Err(_) => exif.as_deref().and_then(Orientation::from_exif_chunk).unwrap_or(Orientation::NoTransforms),
        Ok(o) => o,
    };
    let mut img = DynamicImage::from_decoder(decoder).map_err(corrupt)?;
    img.apply_orientation(Orientation::from_exif(compose(shown.to_exif(), xf)).unwrap_or(Orientation::NoTransforms));
    match format {
        ImageFormat::Png => png_keeping_metadata(&img, data, icc, exif.as_deref()),
        _ => encode_native(&img, format),
    }
}

/// 按原来的像素格式编码；编码器不收这种像素格式（比如 16 位的 TGA）时转成 8 位再编
fn encode_native(img: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    if img.write_to(&mut out, format).is_ok() {
        return Ok(out.into_inner());
    }
    let eight = if img.color().has_alpha() { DynamicImage::ImageRgba8(img.to_rgba8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
    let mut out = Cursor::new(Vec::new());
    eight.write_to(&mut out, format).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// PNG 按原位深重新编码：颜色配置文件、EXIF（清理后、方向为 1：已经转进像素）照写；文字（XMP 和 exiftool 的 Raw profile
/// 除外，里面可能还记着旧的方向）、分辨率（pHYs）、时间，以及没有颜色配置文件时的 gAMA / cHRM / sRGB 从原文件原样搬过来
fn png_keeping_metadata(img: &DynamicImage, original: &[u8], icc: Option<Vec<u8>>, exif: Option<&[u8]>) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let has_icc = icc.is_some();
    let mut out = Cursor::new(Vec::new());
    {
        let mut enc = image::codecs::png::PngEncoder::new(&mut out);
        if let Some(icc) = icc {
            enc.set_icc_profile(icc).map_err(|e| e.to_string())?;
        }
        if let Some(exif) = exif.and_then(clean_exif) {
            enc.set_exif_metadata(exif).map_err(|e| e.to_string())?;
        }
        enc.write_image(img.as_bytes(), img.width(), img.height(), img.color().into()).map_err(|e| e.to_string())?;
    }
    let encoded = out.into_inner();
    // 搬过来的块放在 IHDR 后面（都要求在 PLTE / IDAT 前面或不限位置）
    let ihdr_end = png_chunks(&encoded).next().map_or(0, |(_, end, _)| end);
    if ihdr_end == 0 {
        return Ok(encoded);
    }
    let mut kept = Vec::new();
    for (start, end, kind) in png_chunks(original) {
        if keep_png_chunk(kind, &original[start + 8..end - 4], has_icc) {
            kept.extend_from_slice(&original[start..end]);
        }
    }
    Ok([&encoded[..ihdr_end], &kept, &encoded[ihdr_end..]].concat())
}

/// PNG 的块：(开始, 结束（含 CRC）, 类型)；结构不对就停
fn png_chunks(data: &[u8]) -> impl Iterator<Item = (usize, usize, [u8; 4])> + '_ {
    let mut at = if data.starts_with(b"\x89PNG\r\n\x1a\n") { 8 } else { data.len() };
    std::iter::from_fn(move || {
        let head = data.get(at..at + 8)?;
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let end = at.checked_add(12)?.checked_add(len).filter(|&e| e <= data.len())?;
        let item = (at, end, [head[4], head[5], head[6], head[7]]);
        at = end;
        Some(item)
    })
}

/// 重新编码时从原文件搬过去的块
fn keep_png_chunk(kind: [u8; 4], body: &[u8], has_icc: bool) -> bool {
    match &kind {
        b"pHYs" | b"tIME" | b"cICP" | b"mDCv" | b"cLLi" => true,
        // 有颜色配置文件时按规范不该再有 sRGB，gAMA / cHRM 也被它取代
        b"gAMA" | b"cHRM" | b"sRGB" => !has_icc,
        b"tEXt" | b"zTXt" | b"iTXt" => {
            let key = body.split(|&b| b == 0).next().unwrap_or_default();
            !(key == b"XML:com.adobe.xmp" || key.starts_with(b"Raw profile type"))
        }
        _ => false,
    }
}

/// 是 APNG（acTL 在第一个 IDAT 前面）
fn is_apng(data: &[u8]) -> bool {
    png_chunks(data).take_while(|(_, _, kind)| kind != b"IDAT").any(|(_, _, kind)| &kind == b"acTL")
}

/// JPEG 开头那些段（SOS 之前）里和方向有关的位置
struct JpegHead {
    /// 第一个 APP1 Exif 段：(段开头 0xFF 的位置, 段长度字段的值)
    exif: Option<(usize, usize)>,
    /// 没有 Exif 段时插在这里：SOI 后面，有 JFIF（APP0）时在它后面
    insert_at: usize,
    /// Exif 段前面有 MPF（APP2，多图 JPEG）：它记的后面那些图的位置是从它自己算的，Exif 段变长后就不对了
    mpf_before_exif: bool,
}

/// 扫到 SOS 为止；段的结构不对（截断、不是标记、标记不带长度）返回 None：这时不知道后面还有没有 Exif，不能再插一个
fn jpeg_head(data: &[u8]) -> Option<JpegHead> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut head = JpegHead { exif: None, insert_at: 2, mpf_before_exif: false };
    let mut i = 2;
    loop {
        if *data.get(i)? != 0xFF {
            return None;
        }
        let marker = *data.get(i + 1)?;
        match marker {
            // 填充字节
            0xFF => {
                i += 1;
                continue;
            }
            0xDA => return Some(head),
            // 这些标记后面没有长度，不该出现在 SOS 之前
            0x01 | 0xD0..=0xD9 | 0x00 => return None,
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*data.get(i + 2)?, *data.get(i + 3)?]));
        if len < 2 || i + 2 + len > data.len() {
            return None;
        }
        let body = &data[i + 4..i + 2 + len];
        if marker == 0xE0 && i == head.insert_at {
            head.insert_at = i + 2 + len;
        }
        if marker == 0xE1 && head.exif.is_none() && body.len() >= 14 && body.starts_with(b"Exif\0\0") {
            head.exif = Some((i, len));
        }
        if marker == 0xE2 && head.exif.is_none() && body.starts_with(b"MPF\0") {
            head.mpf_before_exif = true;
        }
        i += 2 + len;
    }
}

/// TIFF 结构（TIFF 文件本身，或 JPEG Exif 段里 TIFF 头开始的那一块）
struct Tiff<'a> {
    d: &'a [u8],
    be: bool,
    /// BigTIFF：偏移 8 字节，一项 20 字节
    big: bool,
}

/// IFD0 里方向标记的样子
enum Slot {
    /// 一个 SHORT（标准写法）：值在这个位置（相对 TIFF 头）
    Short(usize),
    /// 一个 LONG
    Long(usize),
    /// 没有，或类型、个数不对（JPEG 的解码器只认一个 SHORT，按没有方向算）
    Other,
}

impl<'a> Tiff<'a> {
    fn parse(d: &'a [u8]) -> Option<Self> {
        let be = match d.get(..2)? {
            b"MM" => true,
            b"II" => false,
            _ => return None,
        };
        let t = Self { d, be, big: false };
        match t.u16_at(2)? {
            42 => Some(t),
            43 if t.u16_at(4)? == 8 => Some(Self { big: true, ..t }),
            _ => None,
        }
    }

    fn bytes<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        self.d.get(at..at.checked_add(N)?)?.try_into().ok()
    }

    fn u16_at(&self, at: usize) -> Option<u16> {
        self.bytes(at).map(|b| if self.be { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        self.bytes(at).map(|b| if self.be { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
    }

    fn u64_at(&self, at: usize) -> Option<u64> {
        self.bytes(at).map(|b| if self.be { u64::from_be_bytes(b) } else { u64::from_le_bytes(b) })
    }

    fn put16(&self, v: u16) -> [u8; 2] {
        if self.be { v.to_be_bytes() } else { v.to_le_bytes() }
    }

    fn put32(&self, v: u32) -> [u8; 4] {
        if self.be { v.to_be_bytes() } else { v.to_le_bytes() }
    }

    fn entry_size(&self) -> usize {
        if self.big { 20 } else { 12 }
    }

    /// IFD0：(位置, 项数)；整个目录（连同后面「下一个 IFD」的偏移）都要在范围里
    fn ifd0(&self) -> Option<(usize, usize)> {
        let (at, n) = if self.big {
            let at = usize::try_from(self.u64_at(8)?).ok()?;
            (at, usize::try_from(self.u64_at(at)?).ok()?)
        } else {
            let at = self.u32_at(4)? as usize;
            (at, usize::from(self.u16_at(at)?))
        };
        let count = if self.big { 8 } else { 2 };
        let next = if self.big { 8 } else { 4 };
        let end = at.checked_add(count)?.checked_add(n.checked_mul(self.entry_size())?)?.checked_add(next)?;
        (at >= 8 && end <= self.d.len()).then_some((at, n))
    }

    /// 方向标记在 IFD0 里的样子；IFD0 读不出来时 None
    fn orientation_slot(&self) -> Option<Slot> {
        let (at, n) = self.ifd0()?;
        let start = at + if self.big { 8 } else { 2 };
        for k in 0..n {
            let e = start + k * self.entry_size();
            if self.u16_at(e)? != 0x0112 {
                continue;
            }
            let kind = self.u16_at(e + 2)?;
            let count = if self.big { self.u64_at(e + 4)? } else { u64::from(self.u32_at(e + 4)?) };
            let value = e + if self.big { 12 } else { 8 };
            return Some(match (kind, count) {
                (3, 1) => Slot::Short(value),
                (4, 1) => Slot::Long(value),
                _ => Slot::Other,
            });
        }
        Some(Slot::Other)
    }

    /// 方向值（1 ~ 8）；long：也认一个 LONG（TIFF 的解码器认，JPEG 的不认）
    fn orientation(&self, long: bool) -> Option<u8> {
        let v = match self.orientation_slot()? {
            Slot::Short(at) => u32::from(self.u16_at(at)?),
            Slot::Long(at) if long => self.u32_at(at)?,
            _ => return None,
        };
        u8::try_from(v).ok().filter(|v| (1..=8).contains(v))
    }

    /// 普通 TIFF：新的 IFD0 = 原来的各项（去掉方向）+ 一个 SHORT 方向，按标签号排好；指向的下一个 IFD（第 2 页、缩略图）不变。
    /// 各项的值原样搬：放不下 4 字节的值存在别处，偏移从 TIFF 头算，新目录放在哪都还对
    fn ifd0_with_orientation(&self, value: u8) -> Option<Vec<u8>> {
        if self.big {
            return None;
        }
        let (at, n) = self.ifd0()?;
        let mut orient = Vec::with_capacity(12);
        orient.extend_from_slice(&self.put16(0x0112));
        orient.extend_from_slice(&self.put16(3));
        orient.extend_from_slice(&self.put32(1));
        orient.extend_from_slice(&self.put16(u16::from(value)));
        orient.extend_from_slice(&[0, 0]);
        let tag = |e: &[u8]| if self.be { u16::from_be_bytes([e[0], e[1]]) } else { u16::from_le_bytes([e[0], e[1]]) };
        let mut list: Vec<&[u8]> = self.d[at + 2..at + 2 + n * 12].as_chunks::<12>().0.iter().map(|e| &e[..]).filter(|e| tag(e) != 0x0112).collect();
        let pos = list.iter().position(|e| tag(e) > 0x0112).unwrap_or(list.len());
        list.insert(pos, &orient);
        let mut out = Vec::with_capacity(2 + list.len() * 12 + 4);
        out.extend_from_slice(&self.put16(u16::try_from(list.len()).ok()?));
        for e in list {
            out.extend_from_slice(e);
        }
        out.extend_from_slice(&self.d[at + 2 + n * 12..at + 2 + n * 12 + 4]);
        Some(out)
    }
}

/// JPEG 里 Exif 段的 TIFF 部分（段内容去掉开头的 "Exif\0\0"）
fn jpeg_tiff<'a>(data: &'a [u8], head: &JpegHead) -> Option<(Tiff<'a>, usize)> {
    let (seg, len) = head.exif?;
    let start = seg + 10;
    Some((Tiff::parse(&data[start..seg + 2 + len])?, start))
}

/// JPEG 的方向值（解码时只认 IFD0 里一个 SHORT，与 image 一样）
pub fn jpeg_orientation(data: &[u8]) -> Option<u8> {
    let head = jpeg_head(data)?;
    jpeg_tiff(data, &head)?.0.orientation(false)
}

/// TIFF 的方向值（IFD0 里的方向标记，SHORT 或 LONG，与 image 的 TIFF 解码一样）
pub fn tiff_orientation(data: &[u8]) -> Option<u8> {
    Tiff::parse(data)?.orientation(true)
}

/// 改 TIFF 的方向标记，图像数据不动：有方向标记就原地改值；没有（或写法不对）时在文件末尾加一个带方向的新 IFD0，
/// 文件头指向它（原来的 IFD0 留在原处不用了；第 2 页起照旧接在它后面）。BigTIFF 没有方向标记时、或结构读不出来时返回 None
pub fn tiff_set_orientation(mut data: Vec<u8>, value: u8) -> Option<Vec<u8>> {
    enum Edit {
        At(usize, Vec<u8>),
        Append(Vec<u8>),
    }
    let edit = {
        let t = Tiff::parse(&data)?;
        match t.orientation_slot()? {
            Slot::Short(at) => Edit::At(at, t.put16(u16::from(value)).to_vec()),
            Slot::Long(at) => Edit::At(at, t.put32(u32::from(value)).to_vec()),
            Slot::Other => Edit::Append(t.ifd0_with_orientation(value)?),
        }
    };
    match edit {
        Edit::At(at, bytes) => data[at..at + bytes.len()].copy_from_slice(&bytes),
        Edit::Append(ifd) => {
            if data.len() % 2 == 1 {
                data.push(0);
            }
            let offset = u32::try_from(data.len()).ok()?;
            data.extend_from_slice(&ifd);
            let be = data.starts_with(b"MM");
            data[4..8].copy_from_slice(&if be { offset.to_be_bytes() } else { offset.to_le_bytes() });
        }
    }
    Some(data)
}

/// 改 JPEG 的方向标记，图像数据一个字节都不动：
/// - 有方向标记（一个 SHORT）：原地改值；
/// - 有 EXIF 但没有方向标记（编辑软件导出的图常见）：在 Exif 段末尾加一个带方向的新 IFD0，TIFF 头指向它；
///   原来的各项、缩略图、厂商数据都不动（它们的偏移从 TIFF 头算，没有挪）；
/// - 没有 EXIF：插一个只有方向的最小 Exif 段。
///
/// 段的结构读不出来、Exif 段会超过 64 KB、或多图 JPEG 的 MPF 段在 Exif 前面（它记的后面那些图的位置会错）时返回 None
pub fn jpeg_set_orientation(data: &[u8], value: u8) -> Option<Vec<u8>> {
    let head = jpeg_head(data)?;
    let Some((seg, len)) = head.exif else {
        let mut ins = vec![0xFF, 0xE1, 0, 34];
        ins.extend_from_slice(b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01");
        ins.extend_from_slice(&[0, value, 0, 0, 0, 0, 0, 0]);
        return Some([&data[..head.insert_at], &ins, &data[head.insert_at..]].concat());
    };
    let (t, base) = jpeg_tiff(data, &head)?;
    match t.orientation_slot()? {
        Slot::Short(at) => {
            let mut out = data.to_vec();
            out[base + at..base + at + 2].copy_from_slice(&t.put16(u16::from(value)));
            Some(out)
        }
        Slot::Long(_) | Slot::Other => {
            if head.mpf_before_exif {
                return None;
            }
            let ifd = t.ifd0_with_orientation(value)?;
            let pad = t.d.len() % 2;
            let new_len = u16::try_from(len + pad + ifd.len()).ok()?;
            let offset = t.put32(u32::try_from(t.d.len() + pad).ok()?);
            let end = seg + 2 + len;
            let mut out = Vec::with_capacity(data.len() + pad + ifd.len());
            out.extend_from_slice(&data[..end]);
            out.resize(end + pad, 0);
            out.extend_from_slice(&ifd);
            out.extend_from_slice(&data[end..]);
            out[seg + 2..seg + 4].copy_from_slice(&new_len.to_be_bytes());
            out[base + 4..base + 8].copy_from_slice(&offset);
            Some(out)
        }
    }
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

    #[test]
    fn format_capabilities_match_encoded_metadata_and_alpha() {
        let img = RgbaImage::from_pixel(8, 6, image::Rgba([20, 40, 60, 128]));
        let meta = Meta { exif: clean_exif(&sample_exif()), icc: Some(b"test ICC profile".to_vec()) };
        for fmt in Format::ALL {
            let bytes = encode_with(&img, fmt, 90, &meta).unwrap();
            let decoded = crate::decode::decode_bytes(&bytes, fmt.exts()[0], &Default::default()).unwrap();
            assert_eq!(decoded.info.exif.is_some(), fmt.supports_exif(), "{fmt:?}");
            assert_eq!(decoded.info.icc.is_some(), fmt.supports_icc(), "{fmt:?}");
            if fmt.full_alpha() {
                assert_eq!(decoded.first_image().as_ref(), &img, "{fmt:?}");
            } else if fmt != Format::Gif {
                assert!(decoded.first_image().pixels().all(|p| p[3] == 255));
            }
        }
    }

    // ---- 保存旋转 ----

    fn opts() -> crate::decode::Options {
        crate::decode::Options { pyramid: false, ..Default::default() }
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("liteview-rot-{}-{n}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 写成文件、保存旋转：(看到的旧图, 存完重新打开的, 存完的文件)
    fn save_rotated(bytes: &[u8], ext: &str, xf: Transform) -> (crate::decode::Decoded, crate::decode::Decoded, Vec<u8>) {
        let dir = tmp(&format!("{ext}-{}", bytes.len()));
        let p = dir.join(format!("x.{ext}"));
        std::fs::write(&p, bytes).unwrap();
        let before = crate::decode::decode_file(&p, &opts()).unwrap();
        save_rotation(&p, xf, &opts()).unwrap();
        let after = crate::decode::decode_file(&p, &opts()).unwrap();
        let out = std::fs::read(&p).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        (before, after, out)
    }

    fn cw() -> Transform {
        Transform { rotation: 1, ..Default::default() }
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in bytes {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
        !c
    }

    fn png_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut c = (body.len() as u32).to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(body);
        let crc = crc32(&c[4..]);
        c.extend_from_slice(&crc.to_be_bytes());
        c
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn only_formats_whose_whole_file_can_be_written_back_offer_saving_the_rotation() {
        for f in ["JPEG", "TIFF", "PNG", "BMP", "TGA", "QOI"] {
            assert!(can_save_rotation(f), "{f}");
        }
        for f in ["APNG", "GIF", "WebP", "HEIF", "AVIF", "RAW", "SVG", "PSD", "JPEG XL", "WebM", "ICO"] {
            assert!(!can_save_rotation(f), "{f}");
        }
    }

    #[test]
    fn rotating_an_apng_is_refused_and_leaves_the_file_alone() {
        let data = include_bytes!("../tests/fixtures/animation.png");
        let dir = tmp("apng");
        let p = dir.join("anim.png");
        std::fs::write(&p, data).unwrap();
        assert!(save_rotation(&p, cw(), &opts()).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), data);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn re_encoded_formats_keep_their_format_and_pixels() {
        let rgba = RgbaImage::from_fn(6, 4, |x, y| image::Rgba([x as u8 * 40, y as u8 * 60, 90, if x == 0 { 128 } else { 255 }]));
        let rgb = DynamicImage::ImageRgba8(RgbaImage::from_fn(6, 4, |x, y| image::Rgba([x as u8 * 40, y as u8 * 60, 90, 255]))).to_rgb8();
        let cases: [(&str, ImageFormat, DynamicImage); 4] = [
            ("tga", ImageFormat::Tga, DynamicImage::ImageRgba8(rgba.clone())),
            ("qoi", ImageFormat::Qoi, DynamicImage::ImageRgba8(rgba.clone())),
            ("bmp", ImageFormat::Bmp, DynamicImage::ImageRgb8(rgb)),
            ("png", ImageFormat::Png, DynamicImage::ImageRgba8(rgba)),
        ];
        for (ext, format, img) in cases {
            let mut bytes = Cursor::new(Vec::new());
            img.write_to(&mut bytes, format).unwrap();
            for xf in [cw(), Transform { rotation: 2, flip_h: true, ..Default::default() }] {
                let (before, after, out) = save_rotated(bytes.get_ref(), ext, xf);
                assert_eq!(crate::decode::sniff::sniff(&out, ext), crate::decode::Kind::Raster(format), "{ext}: written in the same format");
                assert_eq!(after.first_image().as_ref(), &apply_transform(before.first_image(), xf), "{ext} {xf:?}");
            }
        }
    }

    #[test]
    fn a_16_bit_png_stays_16_bit() {
        let img: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> = image::ImageBuffer::from_fn(5, 3, |x, y| image::Rgba([x as u16 * 13_000 + 7, y as u16 * 20_000 + 3, 1234, 65535]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba16(img).write_to(&mut bytes, ImageFormat::Png).unwrap();
        let (before, after, _) = save_rotated(bytes.get_ref(), "png", cw());
        assert_eq!((before.info.bits, after.info.bits), (16, 16));
        assert_eq!(after.first_image().as_ref(), &apply_transform(before.first_image(), cw()));
    }

    #[test]
    fn a_png_keeps_its_profile_exif_and_text_and_its_old_orientation_is_applied_once() {
        let img = RgbaImage::from_fn(8, 6, |x, y| image::Rgba([x as u8 * 30, y as u8 * 40, 200, 255]));
        // EXIF 里方向是 6：打开时已经转过一次
        let base = encode_with(&img, Format::Png, 90, &Meta { exif: Some(sample_exif()), icc: Some(b"profile bytes".to_vec()) }).unwrap();
        let text = png_chunk(b"tEXt", b"Author\0LiteView");
        let xmp = png_chunk(b"iTXt", b"XML:com.adobe.xmp\0\0\0\0\0<x:xmpmeta tiff:Orientation=\"6\"/>");
        let bytes = [&base[..33], &text, &xmp, &base[33..]].concat();
        let (before, after, out) = save_rotated(&bytes, "png", cw());
        assert_eq!((before.info.width, before.info.height), (6, 8), "opened with orientation 6 applied");
        assert_eq!(after.first_image().as_ref(), &apply_transform(before.first_image(), cw()));
        assert_eq!(after.info.icc.as_deref().map(Vec::as_slice), Some(&b"profile bytes"[..]));
        let exif = exif::Reader::new().read_raw(after.info.exif.as_deref().expect("EXIF kept").clone()).unwrap();
        assert_eq!(exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY).and_then(|f| f.value.get_uint(0)), Some(1));
        assert!(exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).is_some());
        assert!(contains(&out, b"Author\0LiteView"), "text chunk copied");
        assert!(!contains(&out, b"XML:com.adobe.xmp"), "XMP with a stale orientation dropped");
    }
    /// EXIF 里有相机型号、拍摄时间，没有方向标记（编辑软件导出的图常见）
    fn jpeg_with_exif_but_no_orientation() -> Vec<u8> {
        use exif::{Field, In, Tag, Value};
        let fields = [
            Field { tag: Tag::Make, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"TestCam".to_vec()]) },
            Field { tag: Tag::DateTime, ifd_num: In::PRIMARY, value: Value::Ascii(vec![b"2026:11:04 10:00:00".to_vec()]) },
        ];
        let mut w = exif::experimental::Writer::new();
        for f in &fields {
            w.push_field(f);
        }
        let mut exif = Cursor::new(Vec::new());
        w.write(&mut exif, false).unwrap();
        let img = RgbaImage::from_fn(16, 8, |x, y| image::Rgba([(x * 15) as u8, (y * 30) as u8, 90, 255]));
        encode_with(&img, Format::Jpeg, 95, &Meta { exif: Some(exif.into_inner()), icc: Some(b"profile bytes".to_vec()) }).unwrap()
    }

    #[test]
    fn a_jpeg_with_exif_but_no_orientation_gets_one_without_touching_the_image_data() {
        let jpeg = jpeg_with_exif_but_no_orientation();
        assert_eq!(jpeg_orientation(&jpeg), None);
        let (seg, len) = jpeg_head(&jpeg).unwrap().exif.unwrap();
        let (before, after, out) = save_rotated(&jpeg, "jpg", cw());
        assert_eq!(jpeg_orientation(&out), Some(6));
        // Exif 段之前、之后（量化表、图像数据……）一个字节不差；段里原来的内容都在，只在末尾加了一块
        let end = seg + 2 + len;
        assert_eq!(&out[..seg + 2], &jpeg[..seg + 2]);
        assert_eq!(&out[out.len() - (jpeg.len() - end)..], &jpeg[end..]);
        assert_eq!(&out[seg + 10..seg + 14], &jpeg[seg + 10..seg + 14], "same TIFF header byte order");
        assert_eq!(&out[seg + 18..end], &jpeg[seg + 18..end], "old IFD0 and its values untouched");
        assert_eq!((after.info.width, after.info.height), (8, 16));
        assert_eq!(after.info.icc, before.info.icc);
        let exif = exif::Reader::new().read_raw(after.info.exif.as_deref().unwrap().clone()).unwrap();
        assert!(matches!(exif.get_field(exif::Tag::Make, exif::In::PRIMARY).map(|f| &f.value), Some(exif::Value::Ascii(v)) if v == &[b"TestCam".to_vec()]));
        assert!(exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY).is_some());
        // 再转一次：这回原地改值，文件长度不变
        let again = jpeg_set_orientation(&out, 3).unwrap();
        assert_eq!((again.len(), jpeg_orientation(&again)), (out.len(), Some(3)));
    }

    #[test]
    fn broken_jpeg_headers_are_refused_instead_of_guessed() {
        let jpeg = jpeg_with_exif_but_no_orientation();
        // 段长度越界
        let mut bad = jpeg.clone();
        bad[4] = 0xFF;
        bad[5] = 0xFF;
        assert!(jpeg_set_orientation(&bad, 6).is_none());
        // 截断在头里
        assert!(jpeg_set_orientation(&jpeg[..5], 6).is_none());
        assert!(jpeg_set_orientation(b"not a jpeg", 6).is_none());
        // MPF（多图 JPEG）在 Exif 前面：加长 Exif 段会让它记的位置错掉
        let (seg, _) = jpeg_head(&jpeg).unwrap().exif.unwrap();
        let mpf = [&[0xFFu8, 0xE2, 0, 8][..], b"MPF\0", &[0, 0]].concat();
        let with_mpf = [&jpeg[..seg], &mpf, &jpeg[seg..]].concat();
        assert!(jpeg_head(&with_mpf).unwrap().mpf_before_exif);
        assert!(jpeg_set_orientation(&with_mpf, 6).is_none());
    }

    /// 手写的两页 TIFF（各 1 × 1 RGB，不压缩），大端；第 1 页红、第 2 页蓝
    fn two_page_tiff() -> Vec<u8> {
        let page = |data_at: u32, next: u32| {
            let e = |tag: u16, kind: u16, value: u32| {
                let mut v = tag.to_be_bytes().to_vec();
                v.extend_from_slice(&kind.to_be_bytes());
                v.extend_from_slice(&1u32.to_be_bytes());
                if kind == 3 {
                    v.extend_from_slice(&(value as u16).to_be_bytes());
                    v.extend_from_slice(&[0, 0]);
                } else {
                    v.extend_from_slice(&value.to_be_bytes());
                }
                v
            };
            let entries = [e(256, 3, 1), e(257, 3, 1), e(258, 3, 8), e(259, 3, 1), e(262, 3, 2), e(273, 4, data_at), e(277, 3, 3), e(278, 3, 1), e(279, 4, 3)];
            let mut ifd = (entries.len() as u16).to_be_bytes().to_vec();
            for x in entries {
                ifd.extend_from_slice(&x);
            }
            ifd.extend_from_slice(&next.to_be_bytes());
            ifd
        };
        // 头 8 + IFD0（2 + 9 × 12 + 4 = 114）+ IFD1（114）+ 两页像素
        let (ifd1, data0) = (8 + 114, 8 + 114 + 114);
        let mut t = b"MM\0*\0\0\0\x08".to_vec();
        t.extend_from_slice(&page(data0, ifd1));
        t.extend_from_slice(&page(data0 + 3, 0));
        t.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        t
    }

    #[test]
    fn a_tiff_without_orientation_gets_a_new_first_directory_and_keeps_its_other_pages() {
        let tiff = two_page_tiff();
        let before = crate::decode::decode_bytes(&tiff, "tif", &opts()).unwrap();
        assert_eq!(before.first_image().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(tiff_orientation(&tiff), None);
        let out = tiff_set_orientation(tiff.clone(), 6).unwrap();
        assert_eq!(tiff_orientation(&out), Some(6));
        // 原来的字节（含第 2 页的目录和像素）一个不差，只在后面加了新目录、改了文件头的指向
        assert_eq!(&out[8..tiff.len()], &tiff[8..]);
        let t = Tiff::parse(&out).unwrap();
        let (at, n) = t.ifd0().unwrap();
        assert_eq!(n, 10);
        assert_eq!(t.u32_at(at + 2 + n * 12), Some(8 + 114), "still points at page 2");
        let after = crate::decode::decode_bytes(&out, "tif", &opts()).unwrap();
        assert_eq!(after.first_image().get_pixel(0, 0).0, [255, 0, 0, 255]);
        // 文件里已经有方向标记：原地改，长度不变
        let again = tiff_set_orientation(out.clone(), 3).unwrap();
        assert_eq!((again.len(), tiff_orientation(&again)), (out.len(), Some(3)));
    }

    #[test]
    fn saving_a_tiff_rotation_only_changes_its_orientation() {
        let img = RgbaImage::from_fn(6, 4, |x, y| image::Rgba([x as u8 * 40, y as u8 * 60, 90, 255]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img).write_to(&mut bytes, ImageFormat::Tiff).unwrap();
        let (before, after, out) = save_rotated(bytes.get_ref(), "tif", cw());
        assert_eq!(tiff_orientation(&out), Some(6));
        assert_eq!(&out[8..bytes.get_ref().len()], &bytes.get_ref()[8..]);
        assert_eq!(after.first_image().as_ref(), &apply_transform(before.first_image(), cw()));
    }
}
