//! 只读文件结构、不解码像素，得到动图每一帧的延时（帧数 = 延时的个数）。
//! 帧太多、缓存放不下时用：先知道总帧数和每帧多长，后面的帧边播边解。延时的规则与解码时一样：≤ 10 ms 按 100 ms。

use std::time::Duration;

use image::ImageFormat;

fn clamp_ms(ms: f64) -> Duration {
    Duration::from_secs_f64(if ms <= 10.0 { 100.0 } else { ms } / 1000.0)
}

pub fn frame_delays(data: &[u8], format: ImageFormat) -> Option<Vec<Duration>> {
    match format {
        ImageFormat::Gif => gif(data),
        ImageFormat::Png => apng(data),
        ImageFormat::WebP => webp(data),
        _ => None,
    }
}

fn gif(d: &[u8]) -> Option<Vec<Duration>> {
    if !d.starts_with(b"GIF") || d.len() < 13 {
        return None;
    }
    let mut i = 13;
    if d[10] & 0x80 != 0 {
        i += 3 << ((d[10] & 7) + 1);
    }
    let skip_sub = |mut i: usize| -> Option<usize> {
        loop {
            let n = *d.get(i)? as usize;
            i += 1 + n;
            if n == 0 {
                return Some(i);
            }
        }
    };
    let (mut out, mut delay) = (Vec::new(), 0u16);
    loop {
        match *d.get(i)? {
            0x21 => {
                if d.get(i + 1) == Some(&0xF9) && d.get(i + 2) == Some(&4) {
                    delay = u16::from_le_bytes([*d.get(i + 4)?, *d.get(i + 5)?]);
                }
                i = skip_sub(i + 2)?;
            }
            0x2C => {
                let flags = *d.get(i + 9)?;
                i += 10;
                if flags & 0x80 != 0 {
                    i += 3 << ((flags & 7) + 1);
                }
                i = skip_sub(i + 1)?;
                out.push(clamp_ms(f64::from(delay) * 10.0));
                delay = 0;
            }
            // 结尾（文件被截断时也用已经读到的帧）
            _ => return Some(out),
        }
    }
}

fn apng(d: &[u8]) -> Option<Vec<Duration>> {
    if !d.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let (mut i, mut out) = (8, Vec::new());
    while i + 8 <= d.len() {
        let len = u32::from_be_bytes(d[i..i + 4].try_into().ok()?) as usize;
        let ty = &d[i + 4..i + 8];
        let body = d.get(i + 8..i + 8 + len)?;
        if ty == b"fcTL" && body.len() >= 26 {
            let num = u16::from_be_bytes([body[20], body[21]]);
            let den = match u16::from_be_bytes([body[22], body[23]]) {
                0 => 100,
                v => v,
            };
            out.push(clamp_ms(f64::from(num) * 1000.0 / f64::from(den)));
        }
        if ty == b"IEND" {
            break;
        }
        i += 12 + len;
    }
    Some(out)
}

fn webp(d: &[u8]) -> Option<Vec<Duration>> {
    if d.len() < 12 || &d[..4] != b"RIFF" || &d[8..12] != b"WEBP" {
        return None;
    }
    let (mut i, mut out) = (12, Vec::new());
    while i + 8 <= d.len() {
        let ty = &d[i..i + 4];
        let len = u32::from_le_bytes(d[i + 4..i + 8].try_into().ok()?) as usize;
        let body = d.get(i + 8..i + 8 + len)?;
        if ty == b"ANMF" {
            let b = body.get(..16)?;
            out.push(clamp_ms(f64::from(u32::from_le_bytes([b[12], b[13], b[14], 0]))));
        }
        i += 8 + len + (len & 1);
    }
    Some(out)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_delays_match_full_decode() {
        use image::codecs::gif::{GifEncoder, Repeat};
        let mut data = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut data);
            enc.set_repeat(Repeat::Infinite).unwrap();
            for (i, ms) in [0u32, 40, 70, 120].into_iter().enumerate() {
                let img = image::RgbaImage::from_pixel(6, 5, image::Rgba([i as u8 * 50, 10, 200, 255]));
                enc.encode_frame(image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(ms, 1))).unwrap();
            }
        }
        let d = gif(&data).unwrap();
        let full = super::super::decode_bytes(&data, "gif", &super::super::Options::default()).unwrap();
        let super::super::Content::Animation { frames, .. } = full.content else { panic!() };
        assert_eq!(d, frames.iter().map(|f| f.delay).collect::<Vec<_>>());
        assert_eq!(d[0], Duration::from_millis(100));
        assert_eq!(d[2], Duration::from_millis(70));
    }

    #[test]
    fn malformed_chunk_lengths_do_not_invent_frames_or_panic() {
        let bad_png = [&b"\x89PNG\r\n\x1a\n"[..], &u32::MAX.to_be_bytes(), b"fcTL"].concat();
        let bad_webp = [&b"RIFF\x14\0\0\0WEBPANMF"[..], &0u32.to_le_bytes(), &[0u8; 16]].concat();
        assert!(apng(&bad_png).is_none());
        assert!(webp(&bad_webp).is_none());
        for n in 0..bad_png.len() {
            let _ = apng(&bad_png[..n]);
        }
        for n in 0..bad_webp.len() {
            let _ = webp(&bad_webp[..n]);
        }
    }

    #[test]
    fn apng_and_webp_chunks() {
        // 两个 fcTL：1/25 秒、0（按 100 ms）
        let fctl = |num: u16, den: u16| {
            let mut body = vec![0u8; 26];
            body[20..22].copy_from_slice(&num.to_be_bytes());
            body[22..24].copy_from_slice(&den.to_be_bytes());
            [&26u32.to_be_bytes()[..], b"fcTL", &body, &[0; 4]].concat()
        };
        let png = [&b"\x89PNG\r\n\x1a\n"[..], &fctl(1, 25), &fctl(0, 0), &0u32.to_be_bytes(), b"IEND", &[0; 4]].concat();
        assert_eq!(apng(&png).unwrap(), vec![Duration::from_millis(40), Duration::from_millis(100)]);
        // 一个 ANMF：250 ms
        let mut anmf = vec![0u8; 16];
        anmf[12..15].copy_from_slice(&250u32.to_le_bytes()[..3]);
        let body = [&b"WEBP"[..], b"ANMF", &16u32.to_le_bytes(), &anmf].concat();
        let webp_data = [&b"RIFF"[..], &(body.len() as u32).to_le_bytes(), &body].concat();
        assert_eq!(webp(&webp_data).unwrap(), vec![Duration::from_millis(250)]);
        assert!(frame_delays(b"not an image", ImageFormat::Gif).is_none());
    }
}
