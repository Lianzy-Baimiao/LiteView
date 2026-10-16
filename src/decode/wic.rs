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

pub fn decode(data: &[u8], kind: Kind, opts: &Options) -> Result<Decoded, Error> {
    let fac = factory().ok_or(Error::Unsupported)?;
    let codec_error = |e: windows::core::Error| match e.code() {
        windows::Win32::Foundation::WINCODEC_ERR_COMPONENTNOTFOUND if matches!(kind, Kind::Heif | Kind::Avif) => Error::MissingCodec(kind),
        windows::Win32::Foundation::WINCODEC_ERR_COMPONENTNOTFOUND => Error::Unsupported,
        windows::Win32::Foundation::E_OUTOFMEMORY => Error::TooLarge { width: 0, height: 0 },
        _ => Error::Corrupt(e.message()),
    };
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
