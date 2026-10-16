use image::RgbaImage;

use super::Meta;

pub fn lossy(img: &RgbaImage, quality: u8, meta: &Meta) -> Result<Vec<u8>, String> {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > 16383 || h > 16383 {
        return Err("WebP dimensions must be between 1 and 16383 pixels".into());
    }
    let encoded = ::webp::Encoder::from_rgba(img.as_raw(), w, h)
        .encode_simple(false, f32::from(quality.clamp(1, 100)))
        .map_err(|e| format!("WebP encoding failed: {e:?}"))?;
    if meta.icc.is_none() && meta.exif.is_none() {
        return Ok(encoded.to_vec());
    }
    // Rebuild the extended RIFF container around libwebp's image chunks.
    // ICCP precedes image data; EXIF follows it. Never copy unknown metadata.
    let mut chunks = Vec::new();
    let mut at = 12usize;
    while at + 8 <= encoded.len() {
        let size = u32::from_le_bytes(encoded[at + 4..at + 8].try_into().unwrap()) as usize;
        let end = at.checked_add(8 + size + size % 2).filter(|&end| end <= encoded.len()).ok_or("Invalid WebP encoder output")?;
        if &encoded[at..at + 4] != b"VP8X" {
            chunks.extend_from_slice(&encoded[at..end]);
        }
        at = end;
    }
    if at != encoded.len() { return Err("Invalid WebP encoder output".into()); }
    let alpha = img.pixels().any(|p| p[3] != 255);
    let mut vp8x = [0u8; 10];
    vp8x[0] = (u8::from(meta.icc.is_some()) * 0x20) | (u8::from(alpha) * 0x10) | (u8::from(meta.exif.is_some()) * 0x08);
    vp8x[4..7].copy_from_slice(&(w - 1).to_le_bytes()[..3]);
    vp8x[7..10].copy_from_slice(&(h - 1).to_le_bytes()[..3]);
    let mut out = b"RIFF\0\0\0\0WEBP".to_vec();
    chunk(&mut out, b"VP8X", &vp8x)?;
    if let Some(icc) = &meta.icc { chunk(&mut out, b"ICCP", icc)?; }
    out.extend_from_slice(&chunks);
    if let Some(exif) = &meta.exif { chunk(&mut out, b"EXIF", exif)?; }
    let size = u32::try_from(out.len() - 8).map_err(|_| "WebP file exceeds RIFF limit")?;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Ok(out)
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) -> Result<(), String> {
    let size = u32::try_from(data.len()).map_err(|_| "WebP metadata too large")?;
    out.extend_from_slice(kind);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 { out.push(0); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{Options, decode_bytes};

    #[test]
    fn lossy_quality_changes_size_and_error_while_alpha_is_exact() {
        let src = RgbaImage::from_fn(128, 96, |x, y| image::Rgba([
            (x * 2) as u8, (y * 2) as u8, ((x * y + x * 7) % 256) as u8, if x < 16 { (x * 16) as u8 } else { 255 },
        ]));
        let low = lossy(&src, 10, &Meta::default()).unwrap();
        let high = lossy(&src, 95, &Meta::default()).unwrap();
        assert!(high.len() > low.len());
        let error = |bytes: &[u8]| {
            let decoded = decode_bytes(bytes, "webp", &Options::default()).unwrap();
            assert_eq!(decoded.first_image().dimensions(), src.dimensions());
            src.pixels().zip(decoded.first_image().pixels()).map(|(a,b)| {
                assert_eq!(a[3], b[3]);
                (0..3).map(|c| (i64::from(a[c]) - i64::from(b[c])).pow(2)).sum::<i64>()
            }).sum::<i64>()
        };
        assert!(error(&high) < error(&low));
    }

    #[test]
    fn lossy_metadata_roundtrip_and_invalid_dimensions() {
        for alpha in [255, 100] {
            let src = RgbaImage::from_pixel(8, 6, image::Rgba([20, 40, 60, alpha]));
            let meta = Meta { exif: Some(b"Exif\0\0test-data".to_vec()), icc: Some(b"test-icc-odd!".to_vec()) };
            let bytes = lossy(&src, 90, &meta).unwrap();
            let decoded = decode_bytes(&bytes, "webp", &Options::default()).unwrap();
            assert_eq!(decoded.info.icc.as_deref(), meta.icc.as_ref());
            assert_eq!(decoded.info.exif.as_deref(), meta.exif.as_ref());
            assert_eq!(decoded.first_image().get_pixel(0, 0)[3], alpha);
        }
        assert!(lossy(&RgbaImage::new(0, 0), 90, &Meta::default()).is_err());
        assert!(lossy(&RgbaImage::new(16384, 1), 90, &Meta::default()).is_err());
    }
}
