//! 其它自带解码器：SVG（resvg）、JPEG XL（jxl-oxide，含动画）、RAW（内嵌的 JPEG 预览）、PSD（合并后的图像）。

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use image::RgbaImage;

use super::{Content, Decoded, Error, Frame, Info, Options, check_size};

fn info(format: &'static str, w: u32, h: u32, has_alpha: bool) -> Info {
    Info { format, width: w, height: h, bits: 8, channels: if has_alpha { 4 } else { 3 }, has_alpha, file_size: 0, jpeg_quality: None, exif: None, icc: None }
}

fn still(format: &'static str, img: RgbaImage) -> Decoded {
    let has_alpha = img.pixels().any(|p| p.0[3] != 255);
    Decoded { info: info(format, img.width(), img.height(), has_alpha), content: Content::Still { image: Arc::new(img), levels: Vec::new() } }
}

/// SVG 的长边栅格化到这么多像素（小图标也放大到清楚）
const SVG_SIDE: f32 = 2048.0;

static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();

/// 系统字体只在第一次用到时加载（带文字的 SVG、文字工具；加载要几十毫秒）
pub fn fontdb() -> Arc<resvg::usvg::fontdb::Database> {
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// 已经加载好的系统字体（还没加载完时 None，不等待）
pub fn fontdb_ready() -> Option<Arc<resvg::usvg::fontdb::Database>> {
    DB.get().cloned()
}

/// 在后台开始加载系统字体（选中文字工具时调用）
pub fn preload_fontdb() {
    if DB.get().is_none() {
        std::thread::spawn(|| {
            let _ = fontdb();
        });
    }
}

/// 解析 SVG（有文字时带上系统字体）
pub fn parse_svg(data: &[u8]) -> Result<resvg::usvg::Tree, Error> {
    let mut opt = resvg::usvg::Options::default();
    if data.windows(5).any(|w| w == b"<text") {
        opt.fontdb = fontdb();
    }
    resvg::usvg::Tree::from_data(data, &opt).map_err(|e| Error::Corrupt(e.to_string()))
}

/// SVG 栅格化成多大（长边 2048）：显示、编辑用的就是这张
pub fn svg_raster_scale(tree: &resvg::usvg::Tree) -> f32 {
    let s = tree.size();
    SVG_SIDE / s.width().max(s.height()).max(1.0)
}

/// 放大看 SVG 时按屏幕重画看得见的那块：栅格（长边 2048）上从 (x0, y0) 起放大 zoom 倍，画进 out_w × out_h 的
/// 预乘透明度 RGBA（out 要先清成全透明；与 egui 的 Color32 排列相同）。out 大小不对时返回 false
pub fn render_svg_region_into(tree: &resvg::usvg::Tree, zoom: f32, x0: f32, y0: f32, out: &mut [u8], out_w: u32, out_h: u32) -> bool {
    let s = svg_raster_scale(tree) * zoom;
    let Some(mut pix) = resvg::tiny_skia::PixmapMut::from_bytes(out, out_w, out_h) else { return false };
    resvg::render(tree, resvg::tiny_skia::Transform::from_row(s, 0.0, 0.0, s, -x0 * zoom, -y0 * zoom), &mut pix);
    true
}

pub fn svg(data: &[u8], opts: &Options) -> Result<Decoded, Error> {
    let tree = parse_svg(data)?;
    let s = tree.size();
    let scale = svg_raster_scale(&tree);
    let (w, h) = ((s.width() * scale).round().max(1.0) as u32, (s.height() * scale).round().max(1.0) as u32);
    opts.check_working(w, h, 16)?;
    let mut pix = resvg::tiny_skia::Pixmap::new(w, h).ok_or(Error::TooLarge { width: w, height: h })?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    let mut out = vec![0u8; (w * h * 4) as usize];
    for (o, p) in out.as_chunks_mut::<4>().0.iter_mut().zip(pix.pixels()) {
        let c = p.demultiply();
        o.copy_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let mut d = still("SVG", RgbaImage::from_raw(w, h, out).expect("size"));
    d.info.has_alpha = true;
    Ok(d)
}

fn open_jxl(data: &[u8], opts: &Options) -> Result<jxl_oxide::JxlImage, Error> {
    let tracker = jxl_oxide::AllocTracker::with_limit((opts.max_alloc / 2).min(usize::MAX as u64) as usize);
    let mut img = jxl_oxide::JxlImage::builder().alloc_tracker(tracker).read(std::io::Cursor::new(data)).map_err(jxl_error)?;
    opts.check_working(img.width(), img.height(), 16)?;
    img.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(jxl_oxide::RenderingIntent::Relative));
    Ok(img)
}

fn jxl_error(e: Box<dyn std::error::Error + Send + Sync>) -> Error {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(e.as_ref());
    while let Some(error) = cause {
        if error.to_string().starts_with("failed to allocate ") {
            return Error::TooLarge { width: 0, height: 0 };
        }
        cause = error.source();
    }
    Error::Corrupt(e.to_string())
}

fn jxl_frame(img: &jxl_oxide::JxlImage, index: usize, opts: &Options) -> Result<RgbaImage, Error> {
    let render = img.render_frame(index).map_err(jxl_error)?;
    let mut stream = render.stream();
    let (w, h, ch) = (stream.width(), stream.height(), stream.channels() as usize);
    opts.check_working(w, h, ch as u64 + 4)?;
    let mut buf = vec![0u8; w as usize * h as usize * ch];
    stream.write_to_buffer(&mut buf);
    let rgba: Vec<u8> = match ch {
        1 => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        2 => buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        3 => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => buf.chunks_exact(ch).flat_map(|p| [p[0], p[1], p[2], p[3]]).collect(),
    };
    RgbaImage::from_raw(w, h, rgba).ok_or_else(|| Error::Corrupt("invalid JPEG XL frame size".into()))
}

pub(super) fn jxl_each_frame(data: &[u8], opts: &Options, mut f: impl FnMut(usize, RgbaImage) -> bool) -> Result<(), Error> {
    let img = open_jxl(data, opts)?;
    for i in 0..img.num_loaded_keyframes() {
        if !f(i, jxl_frame(&img, i, opts)?) {
            break;
        }
    }
    Ok(())
}

pub fn jxl(data: &[u8], opts: &Options) -> Result<Decoded, Error> {
    let img = open_jxl(data, opts)?;
    let anim = img.image_header().metadata.animation.as_ref();
    let tps = anim.map(|a| f64::from(a.tps_numerator) / f64::from(a.tps_denominator.max(1)));
    let delays: Vec<_> = (0..img.num_loaded_keyframes())
        .map(|i| {
            let ms = tps.filter(|t| *t > 0.0).map_or(0.0, |t| f64::from(img.frame_header(i).expect("loaded frame").duration) / t * 1000.0);
            Duration::from_secs_f64(if ms <= 10.0 { 0.1 } else { ms / 1000.0 })
        })
        .collect();
    let plays = anim.and_then(|a| (a.num_loops > 0).then_some(a.num_loops));
    let mut frames = Vec::new();
    let mut bytes = 0usize;
    let size = img.width() as usize * img.height() as usize * 4;
    let cache = opts.anim_cache.min((opts.max_alloc / 4).min(usize::MAX as u64) as usize);
    for (i, &delay) in delays.iter().enumerate() {
        if !frames.is_empty() && bytes + size > cache {
            break;
        }
        let image = Arc::new(jxl_frame(&img, i, opts)?);
        bytes += image.len();
        frames.push(Frame { image, delay });
    }
    let first = frames.first().ok_or(Error::Corrupt("no frames".into()))?.image.clone();
    let has_alpha = first.pixels().any(|p| p.0[3] != 255);
    let mut info = info("JPEG XL", first.width(), first.height(), has_alpha);
    info.bits = img.image_header().metadata.bit_depth.bits_per_sample() as u16;
    let content = if delays.len() == 1 {
        Content::Still { image: first, levels: Vec::new() }
    } else {
        let source = (frames.len() < delays.len()).then(|| Arc::new(super::AnimSource { data: data.into(), format: super::AnimFormat::Jxl, options: *opts }));
        Content::Animation { frames, delays, plays, source }
    };
    Ok(Decoded { info, content })
}

/// RAW：找出文件里最大的内嵌 JPEG 预览（相机都会存一张），按 RAW 的方向转正
pub fn raw_preview(data: &[u8], opts: &Options) -> Result<Decoded, Error> {
    let mut best: Option<(usize, u64)> = None;
    let mut i = 0;
    let mut tries = 0;
    while let Some(off) = data[i..].windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) {
        let at = i + off;
        i = at + 3;
        tries += 1;
        if tries > 4096 {
            break;
        }
        if let Some((w, h)) = jpeg_size(&data[at..])
            && best.is_none_or(|(_, area)| u64::from(w) * u64::from(h) > area)
        {
            best = Some((at, u64::from(w) * u64::from(h)));
        }
    }
    let (at, _) = best.ok_or(Error::Unsupported)?;
    let mut d = super::raster::decode(&data[at..], image::ImageFormat::Jpeg, opts)?;
    let preview_has_orientation = d.info.exif.as_deref().and_then(|e| image::metadata::Orientation::from_exif_chunk(e)).is_some();
    if !preview_has_orientation
        && let Some(o) = tiff_orientation(data).and_then(image::metadata::Orientation::from_exif)
        && let Content::Still { image, .. } = &mut d.content
    {
        let mut img = image::DynamicImage::ImageRgba8((**image).clone());
        img.apply_orientation(o);
        *image = Arc::new(img.into_rgba8());
        d.info.width = image.width();
        d.info.height = image.height();
    }
    d.info.format = "RAW";
    Ok(d)
}

/// 基线 / 渐进 JPEG 的尺寸（无损 JPEG 是 RAW 的传感器数据，不算预览）
fn jpeg_size(d: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < d.len() && d[i] == 0xFF {
        let m = d[i + 1];
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        if matches!(m, 0xC0..=0xC2) {
            let h = u16::from_be_bytes([d[i + 5], d[i + 6]]);
            let w = u16::from_be_bytes([d[i + 7], d[i + 8]]);
            return (w >= 160 && h >= 120).then_some((u32::from(w), u32::from(h)));
        }
        if matches!(m, 0xC3 | 0xDA | 0xD9) || len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

/// TIFF 结构的 RAW：IFD0 里的方向标记
fn tiff_orientation(d: &[u8]) -> Option<u8> {
    let be = match d.get(..2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let rd16 = |o: usize| d.get(o..o + 2).map(|b| if be { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) });
    let rd32 = |o: usize| d.get(o..o + 4).map(|b| if be { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) });
    let ifd = rd32(4)? as usize;
    let n = rd16(ifd)? as usize;
    (0..n.min(512)).map(|k| ifd + 2 + k * 12).find(|&e| rd16(e) == Some(0x0112)).and_then(|e| rd16(e + 8)).map(|v| v as u8)
}

/// PSD：文件末尾「合并后的图像」（8 位 RGB / 灰度，原始或 PackBits 压缩）
pub fn psd(data: &[u8], opts: &Options) -> Result<Decoded, Error> {
    let bad = || Error::Corrupt("psd".into());
    let be16 = |o: usize| data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or_else(bad);
    let be32 = |o: usize| data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(bad);
    if be16(4)? != 1 {
        return Err(Error::Unsupported); // PSB（大文档格式）
    }
    let (channels, h, w, depth, mode) = (be16(12)? as usize, be32(14)?, be32(18)?, be16(22)?, be16(24)?);
    check_size(w, h)?;
    if depth != 8 || !matches!(mode, 1 | 3) {
        return Err(Error::Unsupported);
    }
    if channels < if mode == 1 { 1 } else { 3 } || channels > 56 {
        return Err(bad());
    }
    opts.check_working(w, h, channels as u64 + 8)?;
    let mut o = 26usize;
    for _ in 0..3 {
        o += 4 + be32(o)? as usize; // 颜色模式数据、图像资源、图层信息
    }
    let comp = be16(o)?;
    o += 2;
    let (w, h) = (w as usize, h as usize);
    let mut planes = vec![vec![0u8; w * h]; channels];
    match comp {
        0 => {
            for (c, plane) in planes.iter_mut().enumerate() {
                let start = o + c * w * h;
                plane.copy_from_slice(data.get(start..start + w * h).ok_or_else(bad)?);
            }
        }
        1 => {
            let counts: Vec<usize> = (0..channels * h).map(|r| be16(o + r * 2).map(usize::from)).collect::<Result<_, _>>()?;
            let mut src = o + channels * h * 2;
            for (r, &n) in counts.iter().enumerate() {
                let row = data.get(src..src + n).ok_or_else(bad)?;
                let dst = &mut planes[r / h][(r % h) * w..(r % h + 1) * w];
                packbits(row, dst)?;
                src += n;
            }
        }
        _ => return Err(Error::Unsupported),
    }
    let gray = mode == 1;
    let has_alpha = channels > if gray { 1 } else { 3 };
    let mut out = vec![255u8; w * h * 4];
    for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let ch = |c: usize| planes.get(c).map_or(0, |p| p[i]);
        let (r, g, b) = if gray { (ch(0), ch(0), ch(0)) } else { (ch(0), ch(1), ch(2)) };
        px.copy_from_slice(&[r, g, b, if has_alpha { ch(if gray { 1 } else { 3 }) } else { 255 }]);
    }
    Ok(still("PSD", RgbaImage::from_raw(w as u32, h as u32, out).expect("size")))
}

fn packbits(src: &[u8], dst: &mut [u8]) -> Result<(), Error> {
    let bad = || Error::Corrupt("truncated PSD PackBits row".into());
    let (mut i, mut o) = (0, 0);
    while i < src.len() && o < dst.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            if len > dst.len() - o || len > src.len() - i {
                return Err(bad());
            }
            dst[o..o + len].copy_from_slice(&src[i..i + len]);
            i += n as usize + 1;
            o += len;
        } else if n != -128 {
            let len = (1 - n as isize) as usize;
            if len > dst.len() - o {
                return Err(bad());
            }
            let v = *src.get(i).ok_or_else(bad)?;
            dst[o..o + len].fill(v);
            i += 1;
            o += len;
        }
    }
    if o != dst.len() { return Err(bad()); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_renders_at_2048() {
        let d = svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#, &Options::default()).unwrap();
        assert_eq!((d.info.width, d.info.height), (2048, 1024));
        assert_eq!(d.first_image().get_pixel(5, 5).0, [255, 0, 0, 255]);
    }

    /// 放大重画的一块与按同样倍数整张画出来的对应部分相同；zoom = 1 时就是显示用的那张
    #[test]
    fn svg_region_matches_the_same_part_of_a_full_render() {
        let data = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><circle cx="17" cy="13" r="9" fill="#2a6" stroke="#000" stroke-width="0.7"/><path d="M2 28 L38 3" stroke="#c22" stroke-width="0.4"/></svg>"##;
        let tree = parse_svg(data).unwrap();
        let render = |zoom: f32, x0: f32, y0: f32, w: u32, h: u32| {
            let mut out = vec![0u8; (w * h * 4) as usize];
            assert!(render_svg_region_into(&tree, zoom, x0, y0, &mut out, w, h));
            out
        };
        let (zoom, full_w, full_h) = (1.5f32, 3072, 2304);
        let full = render(zoom, 0.0, 0.0, full_w, full_h);
        let (x0, y0, w, h) = (700u32, 400u32, 333u32, 210u32);
        let part = render(zoom, x0 as f32 / zoom, y0 as f32 / zoom, w, h);
        // 与整张画的错开 (dx, dy) 比：差超过 1 的像素数。只画一块时抗锯齿的边缘与整张画的有一点不同（tiny-skia），
        // 对齐时只有边缘上少数几个像素；错开一个像素，每条边都会差
        let differ = |dx: u32, dy: u32| {
            let (mut n, mut sum) = (0usize, 0u64);
            for y in 0..h {
                for x in 0..w {
                    let a = &part[((y * w + x) * 4) as usize..][..4];
                    let b = &full[(((y0 + dy + y) * full_w + x0 + dx + x) * 4) as usize..][..4];
                    n += usize::from(a.iter().zip(b).any(|(a, b)| a.abs_diff(*b) > 1));
                    sum += a.iter().zip(b).map(|(a, b)| u64::from(a.abs_diff(*b))).sum::<u64>();
                }
            }
            (n, sum)
        };
        // 本机：对齐时 459 个像素、差的总和 14245；错开一个像素 107143 / 118303
        let (aligned, (dx, dy)) = (differ(0, 0), (differ(1, 0), differ(0, 1)));
        assert!(aligned.0 * 100 < (w * h) as usize, "{aligned:?} of {} pixels differ", w * h);
        assert!(aligned.1 * 4 < dx.1 && aligned.1 * 4 < dy.1, "aligned {aligned:?}, off by one {dx:?} / {dy:?}");
        let base = svg(data, &Options::default()).unwrap();
        let one = render(1.0, 0.0, 0.0, 2048, 1536);
        // 圆里面一点（不透明，预乘与否一样）
        let px = |v: &[u8], x: u32, y: u32| v[((y * 2048 + x) * 4) as usize..][..4].to_vec();
        assert_eq!(px(&one, 870, 665)[3], 255);
        assert_eq!(px(&one, 870, 665), base.first_image().get_pixel(870, 665).0.to_vec());
    }

    #[test]
    fn packbits_and_psd() {
        let mut out = [0u8; 6];
        packbits(&[0x02, 1, 2, 3, 0xFE, 9], &mut out).unwrap();
        assert_eq!(out, [1, 2, 3, 9, 9, 9]);
        // 2×1 RGB，未压缩
        let mut psd_data = b"8BPS\0\x01\0\0\0\0\0\0\0\x03\0\0\0\x01\0\0\0\x02\0\x08\0\x03".to_vec();
        psd_data.extend_from_slice(&[0; 12]);
        psd_data.extend_from_slice(&[0, 0]);
        psd_data.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
        let d = psd(&psd_data, &Options::default()).unwrap();
        assert_eq!(d.first_image().get_pixel(1, 0).0, [20, 40, 60, 255]);
    }

    #[test]
    fn truncated_packbits_is_not_successfully_decoded_as_black() {
        for data in [&[][..], &[2, 1], &[255], &[2, 1, 2, 3]] {
            assert!(packbits(data, &mut [0; 2]).is_err());
        }
        assert!(packbits(&[128, 1, 4, 5], &mut [0; 2]).is_ok());
    }

    #[test]
    fn raw_preview_picks_largest_jpeg() {
        let jpeg = |w, h| {
            let mut v = Vec::new();
            image::codecs::jpeg::JpegEncoder::new(&mut v).encode_image(&image::RgbImage::new(w, h)).unwrap();
            v
        };
        let mut raw = b"II*\0\x08\0\0\0\0\0".to_vec();
        raw.extend(jpeg(160, 120));
        raw.extend(jpeg(640, 480));
        let d = raw_preview(&raw, &Options::default()).unwrap();
        assert_eq!((d.info.width, d.info.height, d.info.format), (640, 480, "RAW"));
    }
}
