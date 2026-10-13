//! 动图：GIF、APNG、动态 WebP。image crate 给出的每一帧已经合成好（处置方式、局部帧、透明色）。
//! 帧延时 ≤ 10 ms 按 100 ms（与主流浏览器一致）；缓存的帧超过上限时只保留前面的帧。

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::metadata::LoopCount;
use image::{AnimationDecoder, ImageDecoder, ImageFormat};

use super::{Content, Decoded, Error, Frame, Info, Options, check_size, img_error};

/// 不是动图时返回 None（交给静态图解码）
pub fn decode(data: &[u8], format: ImageFormat, opts: &Options) -> Option<Result<Decoded, Error>> {
    let cur = || Cursor::new(data);
    match format {
        ImageFormat::Gif => Some(gif(data, opts)),
        ImageFormat::Png => {
            let d = PngDecoder::new(cur()).ok()?;
            if !d.is_apng().unwrap_or(false) {
                return None;
            }
            let (w, h) = d.dimensions();
            let apng = d.apng().ok()?;
            let plays = plays_of(apng.loop_count());
            Some(collect(apng.into_frames(), w, h, "APNG", plays, opts))
        }
        ImageFormat::WebP => {
            let d = WebPDecoder::new(cur()).ok()?;
            if !d.has_animation() {
                return None;
            }
            let (w, h) = d.dimensions();
            let plays = plays_of(d.loop_count());
            Some(collect(d.into_frames(), w, h, "WebP", plays, opts))
        }
        _ => None,
    }
}

fn gif(data: &[u8], opts: &Options) -> Result<Decoded, Error> {
    let d = GifDecoder::new(Cursor::new(data)).map_err(img_error)?;
    let (w, h) = d.dimensions();
    collect(d.into_frames(), w, h, "GIF", gif_plays(data), opts)
}

fn collect(frames: image::Frames<'_>, w: u32, h: u32, format: &'static str, plays: Option<u32>, opts: &Options) -> Result<Decoded, Error> {
    check_size(w, h)?;
    let mut out: Vec<Frame> = Vec::new();
    let (mut bytes, mut truncated) = (0usize, false);
    for f in frames {
        let f = match f {
            Ok(f) => f,
            Err(e) if out.is_empty() => return Err(img_error(e)),
            // 后面的帧坏了：显示前面能解出来的
            Err(_) => break,
        };
        let size = w as usize * h as usize * 4;
        if !out.is_empty() && bytes + size > opts.anim_cache {
            truncated = true;
            break;
        }
        bytes += size;
        let delay = delay_of(f.delay());
        out.push(Frame { image: Arc::new(f.into_buffer()), delay });
    }
    if out.is_empty() {
        return Err(Error::Corrupt("no frames".into()));
    }
    let info = Info { format, width: w, height: h, bits: 8, channels: 4, has_alpha: true, file_size: 0, exif: None };
    let content = if out.len() == 1 {
        let image = out.pop().expect("one frame").image;
        Content::Still { image, levels: Vec::new() }
    } else {
        Content::Animation { frames: out, plays, truncated }
    };
    Ok(Decoded { info, content })
}

fn delay_of(d: image::Delay) -> Duration {
    let (n, den) = d.numer_denom_ms();
    let ms = if den == 0 { 0.0 } else { f64::from(n) / f64::from(den) };
    Duration::from_secs_f64(if ms <= 10.0 { 100.0 } else { ms } / 1000.0)
}

/// APNG / WebP：文件里写的就是总播放次数，0 = 无限
fn plays_of(l: LoopCount) -> Option<u32> {
    match l {
        LoopCount::Infinite => None,
        LoopCount::Finite(n) => Some(n.get()),
    }
}

/// GIF：NETSCAPE2.0 扩展里的循环次数 n 表示「再重复 n 次」，0 = 无限；没有这个扩展时只播一次
fn gif_plays(data: &[u8]) -> Option<u32> {
    let head = &data[..data.len().min(1 << 20)];
    for tag in [&b"NETSCAPE2.0"[..], b"ANIMEXTS1.0"] {
        if let Some(i) = head.windows(tag.len()).position(|w| w == tag) {
            let sub = &head[i + tag.len()..];
            if sub.len() >= 4 && sub[0] >= 3 && sub[1] == 1 {
                let n = u16::from_le_bytes([sub[2], sub[3]]);
                return if n == 0 { None } else { Some(u32::from(n) + 1) };
            }
        }
    }
    Some(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Rgba, RgbaImage};

    fn make_gif(repeat: Option<Repeat>, frames: usize, delay_ms: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut buf);
            if let Some(r) = repeat {
                enc.set_repeat(r).unwrap();
            }
            for i in 0..frames {
                let img = RgbaImage::from_pixel(4, 3, Rgba([i as u8 * 60, 0, 0, 255]));
                let f = image::Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(delay_ms, 1));
                enc.encode_frame(f).unwrap();
            }
        }
        buf
    }

    #[test]
    fn loop_counts() {
        assert_eq!(gif_plays(&make_gif(Some(Repeat::Infinite), 2, 50)), None);
        assert_eq!(gif_plays(&make_gif(Some(Repeat::Finite(2)), 2, 50)), Some(3));
        assert_eq!(gif_plays(&make_gif(None, 2, 50)), Some(1));
    }

    #[test]
    fn frames_and_delay_clamp() {
        let data = make_gif(Some(Repeat::Infinite), 3, 0);
        let d = super::super::decode_bytes(&data, "gif", &Options::default()).unwrap();
        let Content::Animation { frames, plays, truncated } = d.content else { panic!("not animated") };
        assert_eq!((frames.len(), plays, truncated), (3, None, false));
        assert_eq!(frames[0].delay, Duration::from_millis(100));
        assert_eq!(frames[2].image.get_pixel(0, 0).0[0], 120);
    }

    #[test]
    fn single_frame_gif_is_still() {
        let d = super::super::decode_bytes(&make_gif(None, 1, 0), "gif", &Options::default()).unwrap();
        assert!(!d.is_animated());
    }

    #[test]
    fn cache_limit_truncates() {
        let data = make_gif(Some(Repeat::Infinite), 5, 40);
        let opts = Options { anim_cache: 4 * 3 * 4 * 2, ..Options::default() };
        let d = super::super::decode_bytes(&data, "gif", &opts).unwrap();
        let Content::Animation { frames, truncated, .. } = d.content else { panic!() };
        assert_eq!((frames.len(), truncated), (2, true));
        assert_eq!(frames[1].delay, Duration::from_millis(40));
    }
}
