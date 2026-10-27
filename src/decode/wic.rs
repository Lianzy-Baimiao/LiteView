//! Windows 图像组件（WIC）兜底：HEIC / AVIF（装了系统扩展时）、RAW、JPEG XR 等。
//! 每个解码线程各自初始化 COM（MTA）并缓存一个 WIC 工厂。

use std::cell::RefCell;
use std::sync::Arc;

use image::RgbaImage;
use rayon::prelude::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};

use super::{Content, Decoded, Error, Info, Kind, Options};

thread_local! {
    static FACTORY: RefCell<Option<IWICImagingFactory>> = const { RefCell::new(None) };
}

fn factory() -> Option<IWICImagingFactory> {
    FACTORY.with(|f| {
        if f.borrow().is_none() {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                *f.borrow_mut() = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok();
            }
        }
        f.borrow().clone()
    })
}

/// WIC 的错误归类。系统扩展认得容器、但不支持里面的编码或结构（VVC 编码、只有图像序列）时是「不支持」，
/// 不能说文件坏了；普通 HEIC 的常见截断、损坏仍按原逻辑归类。
fn classify(e: &windows::core::Error, kind: Kind, sequence_only: bool) -> Error {
    use windows::Win32::Foundation::{E_OUTOFMEMORY, WINCODEC_ERR_COMPONENTNOTFOUND, WINCODEC_ERR_UNSUPPORTEDOPERATION, WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT};
    match e.code() {
        E_OUTOFMEMORY => Error::TooLarge { width: 0, height: 0 },
        // 图像序列的失败码与截断文件相同（MF_E_INVALID_FILE_FORMAT），只能靠 ftyp 品牌区分
        _ if sequence_only => Error::Unsupported,
        WINCODEC_ERR_COMPONENTNOTFOUND if matches!(kind, Kind::Heif | Kind::Avif) => Error::MissingCodec(kind),
        WINCODEC_ERR_COMPONENTNOTFOUND | WINCODEC_ERR_UNSUPPORTEDOPERATION | WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT => Error::Unsupported,
        _ => Error::Corrupt(e.message()),
    }
}

pub fn decode(data: &[u8], kind: Kind, opts: &Options) -> Result<Decoded, Error> {
    let fac = factory().ok_or(Error::Unsupported)?;
    let sequence_only = matches!(kind, Kind::Heif | Kind::Avif) && super::sniff::image_sequence_only(data);
    let codec_error = |e: windows::core::Error| classify(&e, kind, sequence_only);
    unsafe {
        let stream = fac.CreateStream().map_err(|_| Error::Unsupported)?;
        stream.InitializeFromMemory(data).map_err(|_| Error::Unsupported)?;
        let decoder = fac.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).map_err(codec_error)?;
        let container = decoder.GetContainerFormat().unwrap_or_default();
        let frame = decoder.GetFrame(0).map_err(codec_error)?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).map_err(|e| Error::Corrupt(e.message()))?;
        // WIC codecs have no allocator-limit API; reserve conversion headroom.
        opts.check_working(w, h, 16)?;
        let conv = fac.CreateFormatConverter().map_err(|_| Error::Unsupported)?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).map_err(codec_error)?;
        conv.GetSize(&mut w, &mut h).map_err(|e| Error::Corrupt(e.message()))?;
        opts.check_working(w, h, 16)?;
        let stride = w * 4;
        let mut buf = vec![0u8; stride as usize * h as usize];
        conv.CopyPixels(std::ptr::null(), stride, &mut buf).map_err(codec_error)?;
        let has_alpha = buf.par_chunks(4).any(|p| p[3] != 255);
        let image = RgbaImage::from_raw(w, h, buf).ok_or(Error::Corrupt("size".into()))?;
        Ok(Decoded {
            info: Info {
                format: name(container, kind),
                width: w,
                height: h,
                bits: 8,
                channels: if has_alpha { 4 } else { 3 },
                has_alpha,
                file_size: 0,
                jpeg_quality: None,
                exif: None,
                icc: None,
            },
            content: Content::Still { image: Arc::new(image), levels: Vec::new() },
        })
    }
}

fn name(container: windows::core::GUID, kind: Kind) -> &'static str {
    match kind {
        Kind::Avif => return "AVIF",
        Kind::Heif => return "HEIF",
        Kind::Raw => return "RAW",
        Kind::Jxl => return "JPEG XL",
        Kind::Svg => return "SVG",
        Kind::Psd => return "PSD",
        _ => {}
    }
    [
        (GUID_ContainerFormatWmp, "JPEG XR"),
        (GUID_ContainerFormatDds, "DDS"),
        (GUID_ContainerFormatTiff, "TIFF"),
        (GUID_ContainerFormatJpeg, "JPEG"),
        (GUID_ContainerFormatPng, "PNG"),
        (GUID_ContainerFormatBmp, "BMP"),
        (GUID_ContainerFormatIco, "ICO"),
        (GUID_ContainerFormatGif, "GIF"),
        (GUID_ContainerFormatWebp, "WebP"),
        (GUID_ContainerFormatHeif, "HEIF"),
        (GUID_ContainerFormatRaw, "RAW"),
        (GUID_ContainerFormatAdng, "DNG"),
    ]
    .iter()
    .find(|(g, _)| *g == container)
    .map_or("Image", |(_, n)| n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{E_INVALIDARG, E_OUTOFMEMORY, WINCODEC_ERR_COMPONENTNOTFOUND, WINCODEC_ERR_UNSUPPORTEDOPERATION, WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT};
    use windows::core::{Error as WinError, HRESULT};

    /// MF_E_INVALID_FILE_FORMAT：HEIF 扩展对纯图像序列和截断的 HEIC 都返回它
    const INVALID_FILE_FORMAT: HRESULT = HRESULT(0xC00D36BE_u32 as i32);

    fn class(code: HRESULT, kind: Kind, sequence_only: bool) -> Error {
        classify(&WinError::from_hresult(code), kind, sequence_only)
    }

    #[test]
    fn unsupported_content_is_not_reported_as_damage() {
        // 实测：VVC 编码的 .heif 在 GetFrame 返回 UNSUPPORTEDOPERATION；纯 msf1 序列返回 INVALID_FILE_FORMAT
        assert_eq!(class(WINCODEC_ERR_UNSUPPORTEDOPERATION, Kind::Heif, false), Error::Unsupported);
        assert_eq!(class(WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT, Kind::Unknown, false), Error::Unsupported);
        assert_eq!(class(INVALID_FILE_FORMAT, Kind::Heif, true), Error::Unsupported);
        // 纯序列装了扩展也解不了，不提示去安装
        assert_eq!(class(WINCODEC_ERR_COMPONENTNOTFOUND, Kind::Heif, true), Error::Unsupported);
        // 截断、损坏的普通 HEIC 仍是「已损坏」；缺扩展仍提示安装
        assert!(matches!(class(INVALID_FILE_FORMAT, Kind::Heif, false), Error::Corrupt(_)));
        assert!(matches!(class(E_INVALIDARG, Kind::Heif, false), Error::Corrupt(_)));
        assert_eq!(class(WINCODEC_ERR_COMPONENTNOTFOUND, Kind::Heif, false), Error::MissingCodec(Kind::Heif));
        assert_eq!(class(WINCODEC_ERR_COMPONENTNOTFOUND, Kind::Avif, false), Error::MissingCodec(Kind::Avif));
        assert_eq!(class(WINCODEC_ERR_COMPONENTNOTFOUND, Kind::Raw, false), Error::Unsupported);
        assert_eq!(class(E_OUTOFMEMORY, Kind::Heif, true), Error::TooLarge { width: 0, height: 0 });
    }

    /// HEIC 的方向写在容器的 irot / imir 里（iPhone 照片也是），系统 HEIF 扩展解出来时已经转正；
    /// 这里不能再按 EXIF 转一次。没装 HEIF + HEVC 扩展的机器（CI）解不了，跳过
    #[test]
    fn heic_rotation_is_applied_once_by_the_system_decoder() {
        let data = include_bytes!("../../tests/fixtures/rotated.heic");
        let d = match super::super::decode_bytes(data, "heic", &Options::default()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skipped: {e:?}");
                return;
            }
        };
        let Content::Still { image, .. } = &d.content else { panic!("still image") };
        // 横的 300×200（左上红、右上蓝）带 irot 顺时针转 90°：竖的 200×300，红在右上、蓝在右下
        assert_eq!(image.dimensions(), (200, 300));
        let near = |x: u32, y: u32, want: [u8; 3]| image.get_pixel(x, y).0[..3].iter().zip(want).all(|(&c, w)| c.abs_diff(w) < 40);
        assert!(near(170, 30, [220, 0, 0]), "red top-right: {:?}", image.get_pixel(170, 30));
        assert!(near(170, 270, [0, 0, 220]), "blue bottom-right: {:?}", image.get_pixel(170, 270));
        assert!(near(30, 30, [230, 230, 230]));
    }
}
