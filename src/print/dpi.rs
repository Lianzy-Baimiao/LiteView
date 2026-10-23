//! 图片文件里记录的分辨率（「实际大小」打印用）：JPEG 的 JFIF / EXIF、PNG 的 pHYs / eXIf、TIFF、BMP、
//! WebP 的 EXIF、HEIF 的 EXIF。只读文件头附近，不解码像素；没有记录（或单位不明）时返回 None。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 读文件头最多这么多字节（APP0 / APP1、pHYs、BMP 头都在前面）
const HEAD: u64 = 256 * 1024;

pub fn read(path: &Path) -> Option<[f32; 2]> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    (&mut f).take(HEAD).read_to_end(&mut head).ok()?;
    let dpi = if head.starts_with(&[0xFF, 0xD8]) {
        jpeg(&head)
    } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(&head)
    } else if head.starts_with(b"BM") {
        bmp(&head)
    } else if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        // TIFF 的 IFD 可以在文件任何地方（扫描仪常写在最后）：从头按偏移去读
        f.seek(SeekFrom::Start(0)).ok()?;
        tiff(&mut f)
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        webp(&mut f)
    } else if head.len() >= 12 && &head[4..8] == b"ftyp" {
        heif(&mut f)
    } else {
        None
    };
    dpi.filter(|d| d.iter().all(|v| v.is_finite() && (1.0..=100_000.0).contains(v)))
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// 每米多少像素 → dpi（0 = 没写）
fn per_meter(x: u32, y: u32) -> Option<[f32; 2]> {
    (x > 0 && y > 0).then_some([x as f32 * 0.0254, y as f32 * 0.0254])
}

/// JFIF 写了单位（英寸 / 厘米）就用它；只有比例（单位 0）时看 EXIF
fn jpeg(b: &[u8]) -> Option<[f32; 2]> {
    let (mut i, mut jfif, mut exif) = (2, None, None);
    while i + 4 <= b.len() && b[i] == 0xFF {
        let marker = b[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = be16(b, i + 2)? as usize;
        let seg = b.get(i + 4..i + 2 + len)?;
        if marker == 0xE0 && seg.starts_with(b"JFIF\0") && seg.len() >= 12 {
            let (x, y) = (f32::from(be16(seg, 8)?), f32::from(be16(seg, 10)?));
            jfif = match seg[7] {
                1 => Some([x, y]),
                2 => Some([x * 2.54, y * 2.54]),
                _ => None,
            }
            .filter(|d| d[0] > 0.0 && d[1] > 0.0);
        } else if marker == 0xE1 && seg.starts_with(b"Exif\0\0") && exif.is_none() {
            exif = tiff(&mut std::io::Cursor::new(&seg[6..]));
        }
        i += 2 + len;
    }
    jfif.or(exif)
}

fn png(b: &[u8]) -> Option<[f32; 2]> {
    let (mut i, mut exif) = (8, None);
    while i + 12 <= b.len() {
        let len = be32(b, i)? as usize;
        let kind = &b[i + 4..i + 8];
        let data = b.get(i + 8..i + 8 + len)?;
        match kind {
            b"pHYs" if len >= 9 && data[8] == 1 => return per_meter(be32(data, 0)?, be32(data, 4)?),
            b"pHYs" => return exif,
            b"eXIf" => exif = tiff(&mut std::io::Cursor::new(data)),
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    exif
}

fn bmp(b: &[u8]) -> Option<[f32; 2]> {
    // BITMAPINFOHEADER 及以后的版本：biXPelsPerMeter / biYPelsPerMeter 在文件第 38 / 42 字节
    (le32(b, 14)? >= 40).then_some(())?;
    per_meter(le32(b, 38)?, le32(b, 42)?)
}

/// TIFF 结构（TIFF 文件本身、各种格式里的 EXIF）第一个 IFD 的 XResolution / YResolution / ResolutionUnit
fn tiff<R: Read + Seek>(r: &mut R) -> Option<[f32; 2]> {
    let start = r.stream_position().ok()?;
    let mut h = [0u8; 8];
    r.read_exact(&mut h).ok()?;
    let le = match &h[..4] {
        b"II*\0" => true,
        b"MM\0*" => false,
        _ => return None,
    };
    let u16_at = |b: &[u8]| if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) };
    let u32_at = |b: &[u8]| if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) };
    r.seek(SeekFrom::Start(start + u64::from(u32_at(&h[4..8])))).ok()?;
    let mut n = [0u8; 2];
    r.read_exact(&mut n).ok()?;
    let count = usize::from(u16_at(&n)).min(512);
    let mut entries = vec![0u8; count * 12];
    r.read_exact(&mut entries).ok()?;
    let (mut xr, mut yr, mut unit) = (None, None, 2u16);
    for e in entries.as_chunks::<12>().0 {
        match (u16_at(&e[0..2]), u16_at(&e[2..4])) {
            // RATIONAL：值在 offset 处
            (0x011A, 5) => xr = Some(u32_at(&e[8..12])),
            (0x011B, 5) => yr = Some(u32_at(&e[8..12])),
            (0x0128, 3) => unit = u16_at(&e[8..10]),
            _ => {}
        }
    }
    let mut rational = |off: u32| -> Option<f32> {
        r.seek(SeekFrom::Start(start + u64::from(off))).ok()?;
        let mut v = [0u8; 8];
        r.read_exact(&mut v).ok()?;
        let (num, den) = (u32_at(&v[0..4]), u32_at(&v[4..8]));
        (den > 0 && num > 0).then(|| num as f32 / den as f32)
    };
    let (x, y) = (rational(xr?)?, rational(yr?)?);
    match unit {
        2 => Some([x, y]),
        3 => Some([x * 2.54, y * 2.54]),
        _ => None,
    }
}

/// WebP：RIFF 块里的 EXIF（扩展格式里通常在图像数据之后，按块跳过去）
fn webp<R: Read + Seek>(r: &mut R) -> Option<[f32; 2]> {
    r.seek(SeekFrom::Start(12)).ok()?;
    for _ in 0..64 {
        let mut h = [0u8; 8];
        r.read_exact(&mut h).ok()?;
        let len = u64::from(le32(&h, 4)?);
        if &h[..4] == b"EXIF" {
            let mut data = vec![0u8; len.min(1 << 20) as usize];
            r.read_exact(&mut data).ok()?;
            let tiff_start = if data.starts_with(b"Exif\0\0") { 6 } else { 0 };
            return tiff(&mut std::io::Cursor::new(&data[tiff_start..]));
        }
        r.seek(SeekFrom::Current((len + (len & 1)) as i64)).ok()?;
    }
    None
}

/// HEIF / AVIF：交给 kamadak-exif 找 EXIF（它按盒子跳着读）
fn heif(f: &mut std::fs::File) -> Option<[f32; 2]> {
    use exif::{In, Tag, Value};
    f.seek(SeekFrom::Start(0)).ok()?;
    let e = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(f)).ok()?;
    let rational = |tag| match e.get_field(tag, In::PRIMARY).map(|f| &f.value) {
        Some(Value::Rational(v)) if !v.is_empty() && v[0].denom > 0 && v[0].num > 0 => Some(v[0].to_f64() as f32),
        _ => None,
    };
    let (x, y) = (rational(Tag::XResolution)?, rational(Tag::YResolution)?);
    match e.get_field(Tag::ResolutionUnit, In::PRIMARY).and_then(|f| f.value.get_uint(0)).unwrap_or(2) {
        2 => Some([x, y]),
        3 => Some([x * 2.54, y * 2.54]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 大端 TIFF：IFD0 里 XResolution、YResolution（RATIONAL）和 ResolutionUnit
    fn tiff_blob(x: (u32, u32), y: (u32, u32), unit: Option<u16>) -> Vec<u8> {
        let mut t = b"MM\0*".to_vec();
        t.extend_from_slice(&8u32.to_be_bytes());
        let n: u16 = if unit.is_some() { 3 } else { 2 };
        t.extend_from_slice(&n.to_be_bytes());
        let data = 8 + 2 + u32::from(n) * 12 + 4;
        let entry = |t: &mut Vec<u8>, tag: u16, kind: u16, value: [u8; 4]| {
            t.extend_from_slice(&tag.to_be_bytes());
            t.extend_from_slice(&kind.to_be_bytes());
            t.extend_from_slice(&1u32.to_be_bytes());
            t.extend_from_slice(&value);
        };
        entry(&mut t, 0x011A, 5, data.to_be_bytes());
        entry(&mut t, 0x011B, 5, (data + 8).to_be_bytes());
        if let Some(u) = unit {
            let b = u.to_be_bytes();
            entry(&mut t, 0x0128, 3, [b[0], b[1], 0, 0]);
        }
        t.extend_from_slice(&0u32.to_be_bytes());
        for (num, den) in [x, y] {
            t.extend_from_slice(&num.to_be_bytes());
            t.extend_from_slice(&den.to_be_bytes());
        }
        t
    }

    fn jpeg_with(jfif: Option<(u8, u16, u16)>, exif: Option<Vec<u8>>) -> Vec<u8> {
        let mut j = vec![0xFF, 0xD8];
        if let Some((unit, x, y)) = jfif {
            let mut seg = b"JFIF\0\x01\x02".to_vec();
            seg.push(unit);
            seg.extend_from_slice(&x.to_be_bytes());
            seg.extend_from_slice(&y.to_be_bytes());
            seg.extend_from_slice(&[0, 0]);
            j.extend_from_slice(&[0xFF, 0xE0]);
            j.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
            j.extend_from_slice(&seg);
        }
        if let Some(t) = exif {
            let mut seg = b"Exif\0\0".to_vec();
            seg.extend_from_slice(&t);
            j.extend_from_slice(&[0xFF, 0xE1]);
            j.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
            j.extend_from_slice(&seg);
        }
        j.extend_from_slice(&[0xFF, 0xDA, 0, 2, 0xFF, 0xD9]);
        j
    }

    fn png_with(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in chunks {
            p.extend_from_slice(&(data.len() as u32).to_be_bytes());
            p.extend_from_slice(*kind);
            p.extend_from_slice(data);
            p.extend_from_slice(&[0; 4]);
        }
        p
    }

    fn phys(x: u32, y: u32, unit: u8) -> Vec<u8> {
        let mut d = x.to_be_bytes().to_vec();
        d.extend_from_slice(&y.to_be_bytes());
        d.push(unit);
        d
    }

    /// 写到临时目录读一遍（每个样本自己一个目录：测试并行时不互相删）
    fn file(name: &str, bytes: &[u8]) -> Option<[f32; 2]> {
        let dir = std::env::temp_dir().join(format!("liteview-dpi-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        let r = read(&p);
        let _ = std::fs::remove_dir_all(&dir);
        r
    }

    fn near(a: Option<[f32; 2]>, b: [f32; 2]) -> bool {
        a.is_some_and(|a| (a[0] - b[0]).abs() < 0.05 && (a[1] - b[1]).abs() < 0.05)
    }

    #[test]
    fn jpeg_jfif_units_and_exif_fallback() {
        assert!(near(file("a.jpg", &jpeg_with(Some((1, 300, 300)), None)), [300.0, 300.0]));
        assert!(near(file("b.jpg", &jpeg_with(Some((2, 118, 118)), None)), [299.72, 299.72]));
        // 只有比例：看 EXIF
        let exif = tiff_blob((72, 1), (72, 1), Some(2));
        assert!(near(file("c.jpg", &jpeg_with(Some((0, 1, 1)), Some(exif))), [72.0, 72.0]));
        assert_eq!(file("d.jpg", &jpeg_with(Some((0, 1, 1)), None)), None);
        assert_eq!(file("e.jpg", &jpeg_with(None, None)), None);
        // EXIF 厘米、默认英寸、单位不明
        assert!(near(file("f.jpg", &jpeg_with(None, Some(tiff_blob((118, 1), (236, 2), Some(3))))), [299.72, 299.72]));
        assert!(near(file("g.jpg", &jpeg_with(None, Some(tiff_blob((600, 1), (300, 1), None)))), [600.0, 300.0]));
        assert_eq!(file("h.jpg", &jpeg_with(None, Some(tiff_blob((72, 1), (72, 1), Some(1))))), None);
    }

    #[test]
    fn png_tiff_bmp_and_missing() {
        // 11811 像素 / 米 ≈ 300 dpi
        assert!(near(file("a.png", &png_with(&[(b"IHDR", vec![0; 13]), (b"pHYs", phys(11811, 11811, 1)), (b"IDAT", vec![0; 4])])), [299.99, 299.99]));
        // 单位不明（只有比例）
        assert_eq!(file("b.png", &png_with(&[(b"pHYs", phys(1, 1, 0))])), None);
        assert!(near(file("c.png", &png_with(&[(b"eXIf", tiff_blob((96, 1), (96, 1), Some(2))), (b"IDAT", vec![0; 4])])), [96.0, 96.0]));
        assert_eq!(file("d.png", &png_with(&[(b"IHDR", vec![0; 13]), (b"IDAT", vec![0; 4])])), None);
        assert!(near(file("a.tif", &tiff_blob((400, 1), (400, 1), Some(2))), [400.0, 400.0]));
        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[38..42].copy_from_slice(&3780u32.to_le_bytes());
        bmp[42..46].copy_from_slice(&3780u32.to_le_bytes());
        assert!(near(file("a.bmp", &bmp), [96.01, 96.01]));
        bmp[38..42].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(file("b.bmp", &bmp), None);
        assert_eq!(file("x.gif", b"GIF89a\x01\0\x01\0"), None);
        // 截断、乱码不出错
        assert_eq!(file("t.jpg", &[0xFF, 0xD8, 0xFF, 0xE0, 0xFF]), None);
        assert_eq!(file("t.tif", b"II*\0\xFF\xFF\xFF\x7F"), None);
    }

    #[test]
    fn webp_exif_after_image_data() {
        let mut body = b"WEBP".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            body.extend_from_slice(kind);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        };
        chunk(b"VP8X", &[0x08, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        chunk(b"VP8L", &[1, 2, 3]);
        chunk(b"EXIF", &tiff_blob((240, 1), (240, 1), Some(2)));
        let mut riff = b"RIFF".to_vec();
        riff.extend_from_slice(&(body.len() as u32).to_le_bytes());
        riff.extend_from_slice(&body);
        assert!(near(file("a.webp", &riff), [240.0, 240.0]));
    }
}
