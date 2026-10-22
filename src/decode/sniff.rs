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

/// HEIF / AVIF 只有图像序列（动画轨道）、没有静态主图：系统扩展解不了，失败时是「不支持」而不是文件坏了。
/// 只看 ftyp 品牌：有序列品牌（msf1、hevc、avis…），且没有任何静态图品牌（mif1、heic、avif…）。
#[cfg(any(windows, test))]
pub(crate) fn image_sequence_only(data: &[u8]) -> bool {
    const STILL: [[u8; 4]; 11] = [*b"mif1", *b"mif2", *b"mif3", *b"miaf", *b"heic", *b"heix", *b"heim", *b"heis", *b"avif", *b"avci", *b"vvic"];
    const SEQUENCE: [[u8; 4]; 8] = [*b"msf1", *b"hevc", *b"hevx", *b"hevm", *b"hevs", *b"avis", *b"avcs", *b"vvis"];
    let (mut sequence, mut still) = (false, false);
    for brand in ftyp_brands(data) {
        sequence |= SEQUENCE.contains(&brand);
        still |= STILL.contains(&brand);
    }
    sequence && !still
}

/// ftyp 盒里的主品牌和兼容品牌；不是完整的 ftyp 时为空
#[cfg(any(windows, test))]
fn ftyp_brands(data: &[u8]) -> Vec<[u8; 4]> {
    let size = data.get(..4).map_or(0, |s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize);
    if !(16..=4096).contains(&size) || !size.is_multiple_of(4) || size > data.len() || data.get(4..8) != Some(b"ftyp".as_slice()) {
        return Vec::new();
    }
    // 必须完整扫描才可断言没有静态品牌；超出 4 KiB 的盒不猜测。
    // 8..12 主品牌，12..16 次版本号，其后是兼容品牌。
    data[8..12].as_chunks::<4>().0.iter().chain(data[16..size].as_chunks::<4>().0).copied().collect()
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

    #[test]
    fn image_sequence_without_still_image_brand() {
        let ftyp = |brands: &[&str]| {
            let mut b = ((16 + 4 * (brands.len() - 1)) as u32).to_be_bytes().to_vec();
            b.extend_from_slice(b"ftyp");
            b.extend_from_slice(brands[0].as_bytes());
            b.extend_from_slice(&[0; 4]);
            for brand in &brands[1..] {
                b.extend_from_slice(brand.as_bytes());
            }
            b.extend_from_slice(b"\0\0\0\x08moov");
            b
        };
        // 只有图像序列（如 Nokia 一致性样本 C026）
        assert!(image_sequence_only(&ftyp(&["msf1", "msf1", "hevc", "iso8"])));
        assert!(image_sequence_only(&ftyp(&["avis", "avis", "msf1"])));
        // 序列之外还有静态主图（连拍、动画封面）或普通照片：照常解码
        assert!(!image_sequence_only(&ftyp(&["msf1", "msf1", "hevc", "heic", "mif1", "iso8"])));
        assert!(!image_sequence_only(&ftyp(&["heic", "mif1", "heic"])));
        assert!(!image_sequence_only(&ftyp(&["avis", "avif", "mif1", "msf1"])));
        // 不完整或越界的 ftyp 不当作序列
        assert!(!image_sequence_only(b"\0\0\0\x08ftypmsf1"));
        assert!(!image_sequence_only(b"\0\0\0\x18ftypmsf1"));
        assert!(!image_sequence_only(b"\xff\xff\xff\xffftypmsf1\0\0\0\0hevc"));
        assert!(!image_sequence_only(b""));
    }

    #[test]
    fn incomplete_brand_scan_does_not_assert_sequence_only() {
        for size in [17u32, 18, 19, 4100] {
            let mut data = size.to_be_bytes().to_vec();
            data.extend_from_slice(b"ftypmsf1\0\0\0\0");
            data.resize(size as usize, 0);
            if size == 4100 {
                data[4096..].copy_from_slice(b"heic");
            }
            assert!(!image_sequence_only(&data), "incomplete brand scan for size {size}");
        }
    }
}
