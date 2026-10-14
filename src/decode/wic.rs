//! Windows 图像组件（WIC）兜底：HEIC / AVIF（装了系统扩展时）、RAW、JPEG XR 等。
//! 每个解码线程各自初始化 COM（MTA）并缓存一个 WIC 工厂。

use std::cell::RefCell;
use std::sync::Arc;

use image::RgbaImage;
use rayon::prelude::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};

use super::{Content, Decoded, Error, Info, Kind, check_size};

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

pub fn decode(data: &[u8], kind: Kind) -> Result<Decoded, Error> {
    let fac = factory().ok_or(Error::Unsupported)?;
    let missing = || match kind {
        Kind::Heif | Kind::Avif => Error::MissingCodec(kind),
        _ => Error::Unsupported,
    };
    unsafe {
        let stream = fac.CreateStream().map_err(|_| Error::Unsupported)?;
        stream.InitializeFromMemory(data).map_err(|_| Error::Unsupported)?;
        let decoder = fac
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .map_err(|_| missing())?;
        let container = decoder.GetContainerFormat().unwrap_or_default();
        let frame = decoder.GetFrame(0).map_err(|_| missing())?;
        let conv = fac.CreateFormatConverter().map_err(|_| Error::Unsupported)?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .map_err(|_| missing())?;
        let (mut w, mut h) = (0u32, 0u32);
        conv.GetSize(&mut w, &mut h).map_err(|e| Error::Corrupt(e.message()))?;
        check_size(w, h)?;
        let stride = w * 4;
        let mut buf = vec![0u8; stride as usize * h as usize];
        conv.CopyPixels(std::ptr::null(), stride, &mut buf).map_err(|_| missing())?;
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
