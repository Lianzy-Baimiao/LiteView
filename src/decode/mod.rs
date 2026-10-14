//! 解码：常用格式用自带的纯 Rust 解码器（各平台结果一致），其余交给系统组件（Windows 的 WIC）。
//! 结果统一成 8 位 RGBA（非预乘）；静态图另外生成逐级缩小一半的图，缩小显示时用（没有摩尔纹）。
//! 解码器内部出错（panic）只影响这一张图。

mod anim;
pub mod extra;
mod raster;
pub mod sniff;
#[cfg(windows)]
mod wic;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use image::{ImageFormat, RgbaImage};

pub use raster::halve;
pub use sniff::Kind;

/// 单边最大像素
pub const MAX_SIDE: u32 = 65535;
/// 总像素上限（10 亿）
pub const MAX_PIXELS: u64 = 1_000_000_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    Unsupported,
    Corrupt(String),
    /// 缺少系统解码扩展（如 HEIC 需要 HEVC 视频扩展）
    MissingCodec(Kind),
    Denied,
    NotFound,
    TooLarge { width: u32, height: u32 },
}

#[derive(Clone, Debug)]
pub struct Info {
    /// 格式名，如 "JPEG"、"APNG"
    pub format: &'static str,
    pub width: u32,
    pub height: u32,
    /// 每通道位数
    pub bits: u16,
    pub channels: u8,
    pub has_alpha: bool,
    pub file_size: u64,
    pub exif: Option<Arc<Vec<u8>>>,
    /// 颜色配置文件（编辑后保存时写回）
    pub icc: Option<Arc<Vec<u8>>>,
}

pub struct Frame {
    pub image: Arc<RgbaImage>,
    pub delay: Duration,
}

pub enum Content {
    Still {
        image: Arc<RgbaImage>,
        /// 逐级缩小一半：levels[0] 是原图的 1/2，依次类推，直到长边 ≤ 512
        levels: Vec<Arc<RgbaImage>>,
    },
    Animation {
        frames: Vec<Frame>,
        /// 播放次数；None = 无限循环
        plays: Option<u32>,
        /// 帧太多、超出内存上限，只缓存了前面的帧
        truncated: bool,
    },
}

pub struct Decoded {
    pub info: Info,
    pub content: Content,
}

impl Decoded {
    /// 占用内存（字节），给缓存算账
    pub fn bytes(&self) -> usize {
        match &self.content {
            Content::Still { image, levels } => image.len() + levels.iter().map(|l| l.len()).sum::<usize>(),
            Content::Animation { frames, .. } => frames.iter().map(|f| f.image.len()).sum(),
        }
    }

    /// 静态图本身，或动图的第一帧
    pub fn first_image(&self) -> &Arc<RgbaImage> {
        match &self.content {
            Content::Still { image, .. } => image,
            Content::Animation { frames, .. } => &frames[0].image,
        }
    }

    pub fn is_animated(&self) -> bool {
        matches!(self.content, Content::Animation { .. })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// 单张图解码最多用多少内存（字节）
    pub max_alloc: u64,
    /// 动图缓存的帧最多占多少内存（字节）
    pub anim_cache: usize,
    /// 生成逐级缩小的图
    pub pyramid: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { max_alloc: 4 << 30, anim_cache: 256 << 20, pyramid: true }
    }
}

/// 已经是像素的图（剪贴板）
pub fn from_rgba(img: RgbaImage, format: &'static str) -> Decoded {
    let has_alpha = img.pixels().any(|p| p.0[3] != 255);
    let levels = raster::pyramid(&img);
    Decoded {
        info: Info {
            format,
            width: img.width(),
            height: img.height(),
            bits: 8,
            channels: if has_alpha { 4 } else { 3 },
            has_alpha,
            file_size: 0,
            exif: None,
            icc: None,
        },
        content: Content::Still { image: Arc::new(img), levels },
    }
}

pub fn decode_file(path: &Path, opts: &Options) -> Result<Decoded, Error> {
    let data = std::fs::read(path).map_err(io_error)?;
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    decode_bytes(&data, &ext, opts)
}

pub fn decode_bytes(data: &[u8], ext: &str, opts: &Options) -> Result<Decoded, Error> {
    let kind = sniff::sniff(data, ext);
    let mut d = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_kind(data, kind, opts)))
        .unwrap_or_else(|_| Err(Error::Corrupt("decoder panicked".into())))?;
    d.info.file_size = data.len() as u64;
    if opts.pyramid
        && let Content::Still { image, levels } = &mut d.content
    {
        *levels = raster::pyramid(image);
    }
    Ok(d)
}

fn decode_kind(data: &[u8], kind: Kind, opts: &Options) -> Result<Decoded, Error> {
    match kind {
        Kind::Raster(f) => {
            if let Some(r) = anim::decode(data, f, opts) {
                return r;
            }
            raster::decode(data, f, opts).or_else(|e| system(data, kind).map_err(|_| e))
        }
        Kind::Svg => extra::svg(data),
        Kind::Jxl => extra::jxl(data, opts),
        Kind::Psd => extra::psd(data).or_else(|e| system(data, kind).map_err(|_| e)),
        // RAW：先用内嵌的 JPEG 预览（快、各平台都能看），没有时交给系统解码
        Kind::Raw => extra::raw_preview(data, opts).or_else(|_| system(data, kind)),
        _ => system(data, kind),
    }
}

/// 系统解码组件
fn system(data: &[u8], kind: Kind) -> Result<Decoded, Error> {
    #[cfg(windows)]
    {
        wic::decode(data, kind)
    }
    #[cfg(not(windows))]
    {
        let _ = (data, kind);
        Err(Error::Unsupported)
    }
}

pub(crate) fn check_size(width: u32, height: u32) -> Result<(), Error> {
    if width == 0 || height == 0 {
        return Err(Error::Corrupt("empty image".into()));
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::TooLarge { width, height });
    }
    Ok(())
}

pub(crate) fn img_error(e: image::ImageError) -> Error {
    match e {
        image::ImageError::Unsupported(_) => Error::Unsupported,
        image::ImageError::Limits(_) => Error::TooLarge { width: 0, height: 0 },
        image::ImageError::IoError(e) => io_error(e),
        other => Error::Corrupt(other.to_string()),
    }
}

fn io_error(e: std::io::Error) -> Error {
    match (e.kind(), e.raw_os_error()) {
        (std::io::ErrorKind::NotFound, _) => Error::NotFound,
        (std::io::ErrorKind::PermissionDenied, _) | (_, Some(32 | 33)) => Error::Denied,
        _ => Error::Corrupt(e.to_string()),
    }
}

pub(crate) fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Dds => "DDS",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Hdr => "HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Farbfeld => "Farbfeld",
        ImageFormat::Avif => "AVIF",
        ImageFormat::Qoi => "QOI",
        _ => "Image",
    }
}
