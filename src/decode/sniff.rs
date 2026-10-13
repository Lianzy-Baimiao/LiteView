//! 按文件内容（文件头）识别格式，不看扩展名；只有没有可靠文件头的格式（TGA、RAW）才参考扩展名。

use image::ImageFormat;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Raster(ImageFormat),
    Svg,
    Jxl,
    Heif,
    Avif,
    Raw,
    Psd,
    Unknown,
}

/// 相机 RAW 的扩展名（多数基于 TIFF，文件头与 TIFF 相同）
pub const RAW_EXTS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "raf", "orf", "rw2", "pef", "srw", "x3f", "3fr", "erf", "kdc",
    "mef", "mos", "mrw", "iiq", "rwl",
];

pub fn sniff(data: &[u8], ext: &str) -> Kind {
    let ext = ext.to_ascii_lowercase();
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let brand = &data[8..12];
        return match brand {
            b"avif" | b"avis" => Kind::Avif,
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => {
                // mif1 也可能是 AVIF：看兼容品牌里有没有 avif
                let head = &data[..data.len().min(64)];
                if head.windows(4).any(|w| w == b"avif") { Kind::Avif } else { Kind::Heif }
            }
            b"crx " => Kind::Raw,
            _ => Kind::Unknown,
        };
    }
    if data.starts_with(&[0xFF, 0x0A]) || data.starts_with(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' ']) {
        return Kind::Jxl;
    }
    if data.starts_with(b"8BPS") {
        return Kind::Psd;
    }
    if data.starts_with(b"FUJIFILMCCD-RAW") || data.starts_with(b"IIRO") || data.starts_with(b"IIRS") || data.starts_with(b"IIU\0") {
        return Kind::Raw;
    }
    let tiff_like = data.starts_with(b"II*\0") || data.starts_with(b"MM\0*");
    if tiff_like && RAW_EXTS.contains(&ext.as_str()) {
        return Kind::Raw;
    }
    if let Ok(f) = image::guess_format(data) {
        return Kind::Raster(f);
    }
    if is_svg(data) {
        return Kind::Svg;
    }
    if ext == "tga" || ext == "icb" || ext == "vda" || ext == "vst" {
        return Kind::Raster(ImageFormat::Tga);
    }
    if RAW_EXTS.contains(&ext.as_str()) {
        return Kind::Raw;
    }
    Kind::Unknown
}

fn is_svg(data: &[u8]) -> bool {
    let head = &data[..data.len().min(4096)];
    let head = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start();
    t.starts_with('<') && text.contains("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_content() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n0000", "jpg"), Kind::Raster(ImageFormat::Png));
        assert_eq!(sniff(b"\xFF\xD8\xFF\xE0", "png"), Kind::Raster(ImageFormat::Jpeg));
        assert_eq!(sniff(b"GIF89a", ""), Kind::Raster(ImageFormat::Gif));
        assert_eq!(sniff(b"\0\0\0\x1cftypheic\0\0\0\0mif1heic", "jpg"), Kind::Heif);
        assert_eq!(sniff(b"\0\0\0\x1cftypavif\0\0\0\0", ""), Kind::Avif);
        assert_eq!(sniff(b"\0\0\0\x1cftypmif1\0\0\0\0mif1avif", ""), Kind::Avif);
        assert_eq!(sniff(b"<?xml version=\"1.0\"?><svg xmlns=", ""), Kind::Svg);
        assert_eq!(sniff(b"II*\0\x08\0\0\0", "NEF"), Kind::Raw);
        assert_eq!(sniff(b"II*\0\x08\0\0\0", "tif"), Kind::Raster(ImageFormat::Tiff));
        assert_eq!(sniff(b"\xFF\x0A", ""), Kind::Jxl);
        assert_eq!(sniff(b"\0\0", "tga"), Kind::Raster(ImageFormat::Tga));
        assert_eq!(sniff(b"hello", "txt"), Kind::Unknown);
    }
}
