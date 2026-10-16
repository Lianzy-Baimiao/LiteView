//! 解码：常用格式用自带的纯 Rust 解码器（各平台结果一致），其余交给系统组件（Windows 的 WIC）。
//! 结果统一成 8 位 RGBA（非预乘）；静态图另外生成逐级缩小一半的图，缩小显示时用（没有摩尔纹）。
//! 解码器内部出错（panic）只影响这一张图。

mod anim;
pub mod extra;
mod jpeg_quality;
mod raster;
mod tiff_meta;
#[cfg(test)]
mod limits_tests;
pub mod sniff;
pub mod timing;
#[cfg(windows)]
mod wic;

use std::path::Path;
use std::io::Read;
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
    TooLarge {
        width: u32,
        height: u32,
    },
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
    /// 与标准 IJG 量化表精确匹配的原 JPEG 品质；其它格式或自定义表为 None。
    pub jpeg_quality: Option<u8>,
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
        /// 缓存的帧（帧太多、超出内存上限时只有前面的）
        frames: Vec<Frame>,
        /// 所有帧的延时（帧数 = 它的长度，可能比 frames 多）
        delays: Vec<Duration>,
        /// 播放次数；None = 无限循环
        plays: Option<u32>,
        /// 缓存放不下全部帧：后面的帧边播边解
        source: Option<Arc<AnimSource>>,
    },
}

/// 动图的原始数据（后面的帧边播边解时用）
pub struct AnimSource {
    pub data: Arc<[u8]>,
    pub format: AnimFormat,
    pub options: Options,
}

#[derive(Clone, Copy)]
pub enum AnimFormat {
    Raster(ImageFormat),
    Jxl,
}

impl AnimSource {
    /// 从头顺序解码；回调返回 false 即取消，不保留已交付的 RGBA 帧。
    pub fn each_frame(&self, f: impl FnMut(usize, RgbaImage) -> bool) -> Result<(), Error> {
        match self.format {
            AnimFormat::Raster(format) => anim::each_frame(&self.data, format, &self.options, f),
            AnimFormat::Jxl => extra::jxl_each_frame(&self.data, &self.options, f),
        }
    }
}

impl Content {
    /// 取指定帧；未缓存的帧会解码，调用方必须放到后台线程。
    pub fn image_at(&self, index: usize) -> Result<Arc<RgbaImage>, Error> {
        match self {
            Content::Still { image, .. } if index == 0 => Ok(image.clone()),
            Content::Animation { frames, delays, source, .. } if index < delays.len() => {
                if let Some(frame) = frames.get(index) {
                    return Ok(frame.image.clone());
                }
                let mut found = None;
                if let Some(source) = source {
                    source.each_frame(|i, image| {
                        if i == index {
                            found = Some(Arc::new(image));
                        }
                        i < index
                    })?;
                }
                found.ok_or_else(|| Error::Corrupt("animation frame unavailable".into()))
            }
            _ => Err(Error::Corrupt("frame index out of range".into())),
        }
    }

    /// 动图的总帧数
    pub fn frame_count(&self) -> usize {
        match self {
            Content::Still { .. } => 1,
            Content::Animation { delays, .. } => delays.len(),
        }
    }
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
            Content::Animation { frames, source, .. } => frames.iter().map(|f| f.image.len()).sum::<usize>() + source.as_ref().map_or(0, |s| s.data.len()),
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
    /// Source bytes are bounded separately from pixel/decoder buffers.
    pub max_source: u64,
    /// Budget for known pixel/decoder buffers, not a process RSS limit.
    pub max_alloc: u64,
    /// 动图缓存的帧最多占多少内存（字节）
    pub anim_cache: usize,
    /// 生成逐级缩小的图
    pub pyramid: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { max_source: 512 << 20, max_alloc: 4 << 30, anim_cache: 256 << 20, pyramid: true }
    }
}

impl Options {
    pub(crate) fn image_limits(&self) -> image::Limits {
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(self.max_alloc);
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits
    }

    pub(crate) fn check_working(&self, w: u32, h: u32, bytes_per_pixel: u64) -> Result<u64, Error> {
        check_size(w, h)?;
        let bytes = u64::from(w).checked_mul(u64::from(h)).and_then(|n| n.checked_mul(bytes_per_pixel));
        match bytes {
            Some(n) if n <= self.max_alloc && n <= usize::MAX as u64 => Ok(n),
            _ => Err(Error::TooLarge { width: w, height: h }),
        }
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
            jpeg_quality: None,
            exif: None,
            icc: None,
        },
        content: Content::Still { image: Arc::new(img), levels },
    }
}

pub fn decode_file(path: &Path, opts: &Options) -> Result<Decoded, Error> {
    let file = std::fs::File::open(path).map_err(io_error)?;
    let size = file.metadata().map_err(io_error)?.len();
    if size > opts.max_source {
        return Err(Error::TooLarge { width: 0, height: 0 });
    }
    // Bound the read as well as metadata: a file can grow while it is open.
    let mut data = Vec::new();
    data.try_reserve_exact(usize::try_from(size).map_err(|_| Error::TooLarge { width: 0, height: 0 })?)
        .map_err(|_| Error::TooLarge { width: 0, height: 0 })?;
    file.take(opts.max_source.saturating_add(1)).read_to_end(&mut data).map_err(io_error)?;
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    decode_bytes(&data, &ext, opts)
}

pub fn decode_bytes(data: &[u8], ext: &str, opts: &Options) -> Result<Decoded, Error> {
    if data.len() as u64 > opts.max_source {
        return Err(Error::TooLarge { width: 0, height: 0 });
    }
    let kind = sniff::sniff(data, ext);
    let mut d = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_kind(data, kind, opts)))
        .unwrap_or_else(|_| Err(Error::Corrupt("decoder panicked".into())))?;
    d.info.file_size = data.len() as u64;
    if opts.pyramid
        && let Content::Still { image, levels } = &mut d.content
    {
        let mut bytes = image.len() as u64;
        let (mut w, mut h) = image.dimensions();
        while w.max(h) > 512 {
            (w, h) = (w.div_ceil(2), h.div_ceil(2));
            bytes += u64::from(w) * u64::from(h) * 4;
        }
        if bytes > opts.max_alloc {
            return Err(Error::TooLarge { width: image.width(), height: image.height() });
        }
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
            fallback(raster::decode(data, f, opts), || system(data, kind, opts))
        }
        Kind::Svg => extra::svg(data, opts),
        Kind::Jxl => extra::jxl(data, opts),
        Kind::Psd => fallback(extra::psd(data, opts), || system(data, kind, opts)),
        // RAW：先用内嵌的 JPEG 预览（快、各平台都能看），没有时交给系统解码
        Kind::Raw => fallback(extra::raw_preview(data, opts), || system(data, kind, opts)),
        _ => system(data, kind, opts),
    }
}

fn fallback(result: Result<Decoded, Error>, next: impl FnOnce() -> Result<Decoded, Error>) -> Result<Decoded, Error> {
    match result {
        Err(e @ Error::TooLarge { .. }) => Err(e),
        Err(original) => next().map_err(|e| if matches!(e, Error::TooLarge { .. } | Error::MissingCodec(_)) { e } else { original }),
        ok => ok,
    }
}

/// 系统解码组件
fn system(data: &[u8], kind: Kind, opts: &Options) -> Result<Decoded, Error> {
    #[cfg(windows)]
    {
        wic::decode(data, kind, opts)
    }
    #[cfg(not(windows))]
    {
        let _ = (data, kind, opts);
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
