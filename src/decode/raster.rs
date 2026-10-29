//! 静态图：image crate 解码 → 按 EXIF 方向转正 → 8 位 RGBA；HDR / EXR 从线性转到 sRGB 显示。

use std::io::Cursor;
use std::sync::Arc;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use rayon::prelude::*;

use super::{Content, Decoded, Error, Info, Options, format_name, img_error};

pub fn decode(data: &[u8], format: ImageFormat, opts: &Options) -> Result<Decoded, Error> {
    let mut reader = ImageReader::new(Cursor::new(data));
    reader.set_format(format);
    reader.limits(opts.image_limits());
    let mut decoder = reader.into_decoder().map_err(img_error)?;
    let (w, h) = decoder.dimensions();
    // 按以前的路估（原始像素、转好方向的一份、RGBA 同时在内存里）：现在常见格式只要一份，这里估多了，
    // 但它只拦特别大的图，先不放宽
    opts.check_working(w, h, u64::from(decoder.color_type().bytes_per_pixel()) * 2 + 4)?;
    let exif = decoder.exif_metadata().ok().flatten();
    let icc = if format == ImageFormat::Tiff { super::tiff_meta::icc(data, opts)? } else { decoder.icc_profile().ok().flatten() };
    let orientation = match decoder.orientation() {
        Ok(Orientation::NoTransforms) | Err(_) => exif.as_deref().and_then(Orientation::from_exif_chunk).unwrap_or(Orientation::NoTransforms),
        Ok(o) => o,
    };
    let color = decoder.color_type();
    let channels = color.channel_count();
    let rgba = if let Some(layout) = layout(color) {
        // 8 / 16 位灰度、RGB、RGBA（大多数 JPEG、PNG、TIFF）：直接解到最后的 RGBA 缓冲区开头，原地转成 8 位 RGBA，
        // 方向也原地转（转 90° 时原地转置）。以前原始像素、转好方向的和 RGBA 同时在内存里
        let n = w as usize * h as usize;
        let mut buf = vec![0u8; n * layout.bpp.max(4)];
        decoder.read_image(&mut buf[..n * layout.bpp]).map_err(img_error)?;
        to_rgba(buf, w, h, layout, orientation)
    } else {
        let mut img = DynamicImage::from_decoder(decoder).map_err(img_error)?;
        img.apply_orientation(orientation);
        to_rgba8(img, format)
    };
    Ok(Decoded {
        info: Info {
            format: format_name(format),
            width: rgba.width(),
            height: rgba.height(),
            bits: color.bits_per_pixel() / u16::from(channels.max(1)),
            channels,
            has_alpha: color.has_alpha(),
            file_size: 0,
            jpeg_quality: (format == ImageFormat::Jpeg).then(|| super::jpeg_quality::estimate(data)).flatten(),
            exif: exif.map(Arc::new),
            icc: icc.map(Arc::new),
        },
        content: Content::Still { image: Arc::new(rgba), levels: Vec::new() },
    })
}

/// 能在一个缓冲区里原地转成 8 位 RGBA 的像素格式
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// 每像素几个字节
    bpp: usize,
    /// 每个通道 16 位
    wide: bool,
    /// 通道数：1 灰度、2 灰度 + 透明、3 RGB、4 RGBA
    channels: usize,
}

fn layout(c: image::ColorType) -> Option<Layout> {
    use image::ColorType::{L8, L16, La8, La16, Rgb8, Rgb16, Rgba8, Rgba16};
    let wide = matches!(c, L16 | La16 | Rgb16 | Rgba16);
    matches!(c, L8 | La8 | Rgb8 | Rgba8 | L16 | La16 | Rgb16 | Rgba16).then(|| Layout { bpp: usize::from(c.bytes_per_pixel()), wide, channels: usize::from(c.channel_count()) })
}

/// buf 开头是解码器给的 w × h 像素（layout），转成 8 位 RGBA 并按 EXIF 方向转正，都在这一个缓冲区里
fn to_rgba(mut buf: Vec<u8>, w: u32, h: u32, layout: Layout, o: Orientation) -> RgbaImage {
    let n = w as usize * h as usize;
    convert_in_place(&mut buf, n, layout);
    if buf.len() > n * 4 {
        // 16 位 RGB / RGBA 转完变短了：放掉后面那段
        buf.truncate(n * 4);
        buf.shrink_to_fit();
    }
    orient(buf, w, h, o)
}

/// 原地转成 8 位 RGBA：变长的（灰度、RGB、16 位灰度）从后往前，变短的（16 位 RGB / RGBA）从前往后，写的位置总不在还没读的前面。
/// 16 位转 8 位与 image 的 `into_rgba8` 一样四舍五入：(c + 128) / 257
fn convert_in_place(buf: &mut [u8], n: usize, l: Layout) {
    /// 一个像素里第 k 个 16 位通道转成 8 位
    fn s16(p: &[u8], k: usize) -> u8 {
        ((u32::from(u16::from_ne_bytes([p[k * 2], p[k * 2 + 1]])) + 128) / 257) as u8
    }
    match (l.wide, l.channels) {
        (false, 4) => {}
        (false, 1) => convert_batches(buf, n, 1, |p| [p[0], p[0], p[0], 255]),
        (false, 2) => convert_batches(buf, n, 2, |p| [p[0], p[0], p[0], p[1]]),
        (false, _) => convert_batches(buf, n, 3, |p| [p[0], p[1], p[2], 255]),
        (true, 1) => convert_batches(buf, n, 2, |p| {
            let v = s16(p, 0);
            [v, v, v, 255]
        }),
        (true, 2) => convert_batches(buf, n, 4, |p| {
            let v = s16(p, 0);
            [v, v, v, s16(p, 1)]
        }),
        (true, 3) => convert_batches(buf, n, 6, |p| [s16(p, 0), s16(p, 1), s16(p, 2), 255]),
        (true, _) => convert_batches(buf, n, 8, |p| [s16(p, 0), s16(p, 1), s16(p, 2), s16(p, 3)]),
    }
}

/// 每像素 s 字节原地转成 4 字节（px 转一个像素）。一批批多线程做，每批读的和写的不重叠：
/// 变长（s < 4）从后往前，[lo, hi) 写 [4·lo, 4·hi)、读 [s·lo, s·hi)，lo ≥ s·hi / 4 即可；
/// 变短（s > 4）从前往后，hi ≤ s·lo / 4 即可，开头的一段先一个个做。前面（后面）的批次读完了才轮到这批写
fn convert_batches(buf: &mut [u8], n: usize, s: usize, px: impl Fn(&[u8]) -> [u8; 4] + Sync) {
    use std::cmp::Ordering;
    /// 少于这么多像素的部分一个个做
    const MIN: usize = 1 << 16;
    /// 每个线程任务的像素数
    const CH: usize = 1 << 12;
    let batch = |out: &mut [u8], src: &[u8]| {
        out.par_chunks_mut(4 * CH).zip(src.par_chunks(s * CH)).for_each(|(o, p)| {
            for (o, p) in o.as_chunks_mut::<4>().0.iter_mut().zip(p.chunks_exact(s)) {
                *o = px(p);
            }
        });
    };
    let one = |buf: &mut [u8], i: usize| {
        let v = px(&buf[s * i..s * i + s]);
        buf[4 * i..4 * i + 4].copy_from_slice(&v);
    };
    match s.cmp(&4) {
        // 16 位灰度 + 透明：读写同一处，像素之间不相干
        Ordering::Equal => buf[..n * 4].par_chunks_mut(4 * CH).for_each(|c| {
            for p in c.as_chunks_mut::<4>().0 {
                *p = px(p);
            }
        }),
        Ordering::Less => {
            let mut hi = n;
            while hi > MIN {
                let lo = (s * hi).div_ceil(4);
                let (left, right) = buf.split_at_mut(4 * lo);
                batch(&mut right[..4 * (hi - lo)], &left[s * lo..s * hi]);
                hi = lo;
            }
            for i in (0..hi).rev() {
                one(buf, i);
            }
        }
        Ordering::Greater => {
            let first = n.min(MIN);
            for i in 0..first {
                one(buf, i);
            }
            let mut lo = first;
            while lo < n {
                let hi = (s * lo / 4).min(n);
                let (left, right) = buf.split_at_mut(s * lo);
                batch(&mut left[4 * lo..4 * hi], &right[..s * (hi - lo)]);
                lo = hi;
            }
        }
    }
}

/// 按 EXIF 方向转正，都在原来的缓冲区里：翻转、转 180° 直接做；转 90° 先原地转置，再翻转
fn orient(mut buf: Vec<u8>, w: u32, h: u32, o: Orientation) -> RgbaImage {
    let (wu, hu) = (w as usize, h as usize);
    let px = buf.as_chunks_mut::<4>().0;
    let (w, h) = match o {
        Orientation::NoTransforms => (w, h),
        Orientation::FlipHorizontal => {
            flip_h(px, wu);
            (w, h)
        }
        Orientation::FlipVertical => {
            flip_v(px, wu);
            (w, h)
        }
        Orientation::Rotate180 => {
            reverse(px);
            (w, h)
        }
        // 转置 = 顺时针 90° 再左右翻转（EXIF 5）
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH => {
            transpose_in_place(px, hu, wu);
            match o {
                Orientation::Rotate90 => flip_h(px, hu),
                Orientation::Rotate270 => flip_v(px, hu),
                Orientation::Rotate270FlipH => reverse(px),
                _ => {}
            }
            (h, w)
        }
    };
    RgbaImage::from_raw(w, h, buf).expect("buffer size")
}

/// 整个倒过来（转 180°）：前一半和后一半对换，分给几个线程
fn reverse(px: &mut [[u8; 4]]) {
    const CHUNK: usize = 1 << 16;
    let half = px.len() / 2;
    let (front, back) = px.split_at_mut(half);
    let back_len = back.len();
    let back = &mut back[back_len - half..];
    front.par_chunks_mut(CHUNK).zip(back.par_rchunks_mut(CHUNK)).for_each(|(a, b)| {
        let k = a.len();
        for i in 0..k {
            std::mem::swap(&mut a[i], &mut b[k - 1 - i]);
        }
    });
}

/// 每行左右翻转（一行 w 个像素）
fn flip_h(px: &mut [[u8; 4]], w: usize) {
    if w > 0 {
        px.par_chunks_mut(w).for_each(<[[u8; 4]]>::reverse);
    }
}

/// 上下翻转：第 y 行和倒数第 y 行互换
fn flip_v(px: &mut [[u8; 4]], w: usize) {
    if w == 0 {
        return;
    }
    let rows = px.len() / w;
    let (top, bottom) = px.split_at_mut(rows / 2 * w);
    let bottom = &mut bottom[(rows % 2) * w..];
    top.par_chunks_mut(w).zip(bottom.par_chunks_mut(w).rev()).for_each(|(a, b)| a.swap_with_slice(b));
}

/// 列内重排时新的第 r 行取原来的哪一行：每列从 start 开始，往下一行调用一次 next（累加，不做除法）
trait Rows: Sync {
    type State: Copy + Default;
    fn start(&self, col: usize) -> Self::State;
    fn next(&self, s: &mut Self::State) -> usize;
}

/// 整个缓冲区的指针，给几个线程各写不重叠的几列
#[derive(Clone, Copy)]
struct Shared(*mut [u8; 4]);
// SAFETY: 只在 permute_columns 里用，各线程碰的列不重叠（见那里）
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    /// SAFETY: [at, at + len) 在缓冲区里，且这段时间没有别的线程碰它
    unsafe fn seg<'a>(self, at: usize, len: usize) -> &'a mut [[u8; 4]] {
        unsafe { std::slice::from_raw_parts_mut(self.0.add(at), len) }
    }
}

/// 列内重排：16 列一块，一行行连着读进临时空间，再按 rows 给的行取出写回。各块之间不相干，分给几个线程做
fn permute_columns<R: Rows>(px: &mut [[u8; 4]], m: usize, n: usize, rows: &R) {
    const TILE: usize = 16;
    debug_assert_eq!(px.len(), m * n);
    let base = Shared(px.as_mut_ptr());
    (0..n.div_ceil(TILE)).into_par_iter().for_each_init(
        || vec![[0u8; 4]; m * TILE],
        |tile, t| {
            let c0 = t * TILE;
            let tw = TILE.min(n - c0);
            // SAFETY: 第 t 块只碰每行的 [c0, c0 + tw) 这几列，块与块不重叠；px 的 &mut 在这期间没有别的用处
            for r in 0..m {
                tile[r * tw..r * tw + tw].copy_from_slice(unsafe { base.seg(r * n + c0, tw) });
            }
            let mut state: [R::State; TILE] = std::array::from_fn(|k| if k < tw { rows.start(c0 + k) } else { R::State::default() });
            for r in 0..m {
                let out = unsafe { base.seg(r * n + c0, tw) };
                for (k, p) in out.iter_mut().enumerate() {
                    *p = tile[rows.next(&mut state[k]) * tw + k];
                }
            }
        },
    );
}

/// 第 1 步：第 j 列新的第 r 行取原来的第 (r + j / b) mod m 行
struct Rotate {
    m: usize,
    b: usize,
}

impl Rows for Rotate {
    type State = usize;
    fn start(&self, j: usize) -> usize {
        (j / self.b) % self.m
    }
    fn next(&self, s: &mut usize) -> usize {
        let at = *s;
        *s = if at + 1 == self.m { 0 } else { at + 1 };
        at
    }
}

/// 第 3 步：第 c 列新的第 r 行（线性位置 p = r·n + c）取第 2 步后原来的 (p mod m, p / m) 所在的行 (p mod m − (p / m) / b) mod m。
/// p 每往下一行加 n，p mod m、p / m、(p / m) / b 跟着累加
struct Gather {
    m: usize,
    b: usize,
    /// n / m、n % m
    nq: usize,
    nr: usize,
}

impl Rows for Gather {
    /// (p mod m, (p / m) mod b, ((p / m) / b) mod m)
    type State = (usize, usize, usize);
    fn start(&self, c: usize) -> Self::State {
        let q = c / self.m;
        (c % self.m, q % self.b, (q / self.b) % self.m)
    }
    fn next(&self, (rem, qr, qbm): &mut Self::State) -> usize {
        let m = self.m;
        let at = if *rem >= *qbm { *rem - *qbm } else { *rem + m - *qbm };
        let mut dq = self.nq;
        *rem += self.nr;
        if *rem >= m {
            *rem -= m;
            dq += 1;
        }
        *qr += dq;
        while *qr >= self.b {
            *qr -= self.b;
            *qbm = if *qbm + 1 == m { 0 } else { *qbm + 1 };
        }
        at
    }
}

/// m 行 n 列的像素原地转置成 n 行 m 列，只多用几列、几行大小的临时空间。
/// 分三步，每步只在一列里或一行里挪（Catanzaro, Keller, Garland, "A Decomposition for In-place Matrix Transposition", PPoPP 2014）。
/// 设 g = gcd(m, n)，b = n / g：
/// 1. 第 j 列转动：新的第 r 行取原来的第 (r + j / b) mod m 行（g = 1 时不用转）；
/// 2. 每行内：原来在 (i, j) 的元素（现在在第 (i − j / b) mod m 行）放到第 (j·m + i) mod n 列；
/// 3. 每列内：转置后线性位置 p = r·n + c 的元素是原来的 (p mod m, p / m)，取第 2 步后它所在的行
fn transpose_in_place(px: &mut [[u8; 4]], m: usize, n: usize) {
    debug_assert_eq!(px.len(), m * n);
    if m <= 1 || n <= 1 {
        return;
    }
    let b = n / gcd(m, n);
    if b < n {
        permute_columns(px, m, n, &Rotate { m, b });
    }
    let mm = m % n;
    px.par_chunks_mut(n).enumerate().for_each_init(
        || vec![[0u8; 4]; n],
        |tmp, (r, row)| {
            // 第 j 列的元素原来在第 i = (r + j / b) mod m 行，放到 (j·m + i) mod n 列：i、i mod n、j·m mod n 都累加着算
            let (mut i, mut i_n, mut jm, mut t) = (r, r % n, 0usize, 0usize);
            for p in row.iter() {
                let at = jm + i_n;
                tmp[if at >= n { at - n } else { at }] = *p;
                jm += mm;
                if jm >= n {
                    jm -= n;
                }
                t += 1;
                if t == b {
                    t = 0;
                    i += 1;
                    i_n += 1;
                    if i_n == n {
                        i_n = 0;
                    }
                    if i == m {
                        i = 0;
                        i_n = 0;
                    }
                }
            }
            row.copy_from_slice(tmp);
        },
    );
    permute_columns(px, m, n, &Gather { m, b, nq: n / m, nr: n % m });
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn to_rgba8(img: DynamicImage, format: ImageFormat) -> RgbaImage {
    let hdr = matches!(format, ImageFormat::Hdr | ImageFormat::OpenExr);
    match img {
        DynamicImage::ImageRgb32F(f) if hdr => tonemap(f.width(), f.height(), f.as_raw(), 3),
        DynamicImage::ImageRgba32F(f) if hdr => tonemap(f.width(), f.height(), f.as_raw(), 4),
        other => other.into_rgba8(),
    }
}

/// 线性 → sRGB；1.0 为白，更亮的部分截断
fn tonemap(w: u32, h: u32, src: &[f32], ch: usize) -> RgbaImage {
    let mut out = vec![0u8; w as usize * h as usize * 4];
    out.par_chunks_mut(4).zip(src.par_chunks(ch)).for_each(|(o, s)| {
        for i in 0..3 {
            o[i] = (linear_to_srgb(s[i]) * 255.0 + 0.5) as u8;
        }
        o[3] = if ch == 4 { (s[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8 } else { 255 };
    });
    RgbaImage::from_raw(w, h, out).expect("buffer size")
}

fn linear_to_srgb(v: f32) -> f32 {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// 逐级缩小一半，直到长边 ≤ 512
pub fn pyramid(img: &RgbaImage) -> Vec<Arc<RgbaImage>> {
    let mut out: Vec<Arc<RgbaImage>> = Vec::new();
    loop {
        let src: &RgbaImage = out.last().map_or(img, |l| l);
        if src.width().max(src.height()) <= 512 {
            break;
        }
        let half = Arc::new(halve(src));
        out.push(half);
    }
    out
}

/// 2×2 平均缩小一半（按不透明度加权，透明像素的颜色不会渗进来）；奇数边重复最后一行 / 列
pub fn halve(src: &RgbaImage) -> RgbaImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    let s = src.as_raw();
    let mut out = vec![0u8; nw * nh * 4];
    out.par_chunks_mut(nw * 4).enumerate().for_each(|(y, row)| {
        let (y0, y1) = ((y * 2).min(h - 1), (y * 2 + 1).min(h - 1));
        for x in 0..nw {
            let (x0, x1) = ((x * 2).min(w - 1), (x * 2 + 1).min(w - 1));
            let idx = [(y0 * w + x0) * 4, (y0 * w + x1) * 4, (y1 * w + x0) * 4, (y1 * w + x1) * 4];
            let a: u32 = idx.iter().map(|&i| u32::from(s[i + 3])).sum();
            let o = &mut row[x * 4..x * 4 + 4];
            if a == 0 {
                o.fill(0);
                continue;
            }
            for c in 0..3 {
                let sum: u32 = idx.iter().map(|&i| u32::from(s[i + c]) * u32::from(s[i + 3])).sum();
                o[c] = ((sum + a / 2) / a) as u8;
            }
            o[3] = ((a + 2) / 4) as u8;
        }
    });
    RgbaImage::from_raw(nw as u32, nh as u32, out).expect("buffer size")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在一个缓冲区里转 RGBA、转方向，与以前的「解成原始格式 → apply_orientation → into_rgba8」逐像素相同：
    /// 8 / 16 位的灰度、灰度 + 透明、RGB、RGBA，8 种方向，宽高互质、有公因数、只有一行 / 一列
    #[test]
    fn in_place_conversion_matches_the_old_conversion() {
        use image::{GrayAlphaImage, GrayImage, ImageBuffer, Luma, LumaA, Rgb, RgbImage, Rgba};
        let v8 = |x: u32, y: u32, k: u32| ((x * 37 + y * 101 + k * 59) % 251) as u8;
        let v16 = |x: u32, y: u32, k: u32| ((x * 9_371 + y * 22_307 + k * 15_013) % 65_521) as u16;
        // 400 × 301：超过 65536 像素，转换分成几批多线程做
        for (w, h) in [(5u32, 3u32), (6, 4), (12, 8), (7, 7), (1, 9), (9, 1), (40, 17), (400, 301)] {
            let images = [
                DynamicImage::ImageLuma8(GrayImage::from_fn(w, h, |x, y| Luma([v8(x, y, 0)]))),
                DynamicImage::ImageLumaA8(GrayAlphaImage::from_fn(w, h, |x, y| LumaA([v8(x, y, 0), v8(x, y, 1)]))),
                DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| Rgb([v8(x, y, 0), v8(x, y, 1), v8(x, y, 2)]))),
                DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| Rgba([v8(x, y, 0), v8(x, y, 1), v8(x, y, 2), v8(x, y, 3)]))),
                DynamicImage::ImageLuma16(ImageBuffer::from_fn(w, h, |x, y| Luma([v16(x, y, 0)]))),
                DynamicImage::ImageLumaA16(ImageBuffer::from_fn(w, h, |x, y| LumaA([v16(x, y, 0), v16(x, y, 1)]))),
                DynamicImage::ImageRgb16(ImageBuffer::from_fn(w, h, |x, y| Rgb([v16(x, y, 0), v16(x, y, 1), v16(x, y, 2)]))),
                DynamicImage::ImageRgba16(ImageBuffer::from_fn(w, h, |x, y| Rgba([v16(x, y, 0), v16(x, y, 1), v16(x, y, 2), v16(x, y, 3)]))),
            ];
            for src in images {
                let l = layout(src.color()).expect("in-place layout");
                for e in 1..=8 {
                    let o = Orientation::from_exif(e).unwrap();
                    let mut old = src.clone();
                    old.apply_orientation(o);
                    let n = (w * h) as usize;
                    let mut buf = vec![0u8; n * l.bpp.max(4)];
                    buf[..src.as_bytes().len()].copy_from_slice(src.as_bytes());
                    assert_eq!(to_rgba(buf, w, h, l, o), old.into_rgba8(), "{w}x{h} {:?} {o:?}", src.color());
                }
            }
        }
        assert_eq!(layout(image::ColorType::Rgb32F), None);
    }

    /// 原地转置与直接按下标搬的结果相同：所有 1 ~ 20 的宽高，以及多块、多批的大小
    #[test]
    fn in_place_transpose_matches_a_copy() {
        let mut sizes: Vec<(usize, usize)> = (1..=20).flat_map(|m| (1..=20).map(move |n| (m, n))).collect();
        sizes.extend([(37, 100), (100, 37), (64, 96), (96, 64), (3, 1000), (1000, 3), (250, 250), (333, 1024)]);
        for (m, n) in sizes {
            let src: Vec<[u8; 4]> = (0..m * n).map(|i| (i as u32).to_le_bytes()).collect();
            let mut px = src.clone();
            transpose_in_place(&mut px, m, n);
            for r in 0..n {
                for c in 0..m {
                    assert_eq!(px[r * m + c], src[c * n + r], "{m}x{n} at ({r}, {c})");
                }
            }
        }
    }

    /// 分块多线程倒过来与 slice::reverse 相同（奇数长度，好几块）
    #[test]
    fn parallel_reverse_matches_reverse() {
        for len in [0usize, 1, 2, 65_536 * 2 + 1, 200_003] {
            let src: Vec<[u8; 4]> = (0..len).map(|i| (i as u32).to_le_bytes()).collect();
            let (mut a, mut b) = (src.clone(), src);
            reverse(&mut a);
            b.reverse();
            assert!(a == b, "len {len}");
        }
    }

    /// 整个解码：JPEG 带 EXIF 方向 1 ~ 8，结果与以前的路（apply_orientation → into_rgba8）逐像素相同
    #[test]
    fn jpeg_with_every_exif_orientation_decodes_like_before() {
        let src = image::RgbImage::from_fn(40, 24, |x, y| image::Rgb([(x * 6) as u8, (y * 10) as u8, ((x * y) % 256) as u8]));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&src).unwrap();
        for e in 1u8..=8 {
            let exif = [&b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0"[..], &[e], &[0u8; 6]].concat();
            let mut app1 = vec![0xFF, 0xE1];
            app1.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
            app1.extend_from_slice(&exif);
            let data = [&jpeg[..2], &app1, &jpeg[2..]].concat();
            let d = decode(&data, ImageFormat::Jpeg, &Options::default()).unwrap();
            let Content::Still { image, .. } = d.content else { panic!("still") };
            let mut old = DynamicImage::from_decoder(ImageReader::with_format(Cursor::new(&data), ImageFormat::Jpeg).into_decoder().unwrap()).unwrap();
            old.apply_orientation(Orientation::from_exif(e).unwrap());
            assert_eq!(*image, old.into_rgba8(), "orientation {e}");
            assert_eq!(image.dimensions(), if e >= 5 { (24, 40) } else { (40, 24) });
        }
    }

    /// 整个解码：16 位的 PNG、TIFF 走新路，结果与 image 自己转的一样
    #[test]
    fn sixteen_bit_png_and_tiff_decode_like_before() {
        use image::{ImageBuffer, Luma, Rgb, Rgba};
        let rgb: ImageBuffer<Rgb<u16>, Vec<u16>> = ImageBuffer::from_fn(9, 5, |x, y| Rgb([(x * 7000) as u16, (y * 13000) as u16, 65535 - (x * y * 900) as u16]));
        let rgba: ImageBuffer<Rgba<u16>, Vec<u16>> = ImageBuffer::from_fn(9, 5, |x, y| Rgba([(x * 300) as u16, 40000, (y * 9000) as u16, (x * 7000 + 129) as u16]));
        let grey: ImageBuffer<Luma<u16>, Vec<u16>> = ImageBuffer::from_fn(9, 5, |x, y| Luma([(x * 5000 + y * 1000 + 127) as u16]));
        for src in [DynamicImage::ImageRgb16(rgb), DynamicImage::ImageRgba16(rgba), DynamicImage::ImageLuma16(grey)] {
            for format in [ImageFormat::Png, ImageFormat::Tiff] {
                let mut data = Vec::new();
                src.write_to(&mut Cursor::new(&mut data), format).unwrap();
                let d = decode(&data, format, &Options::default()).unwrap();
                let Content::Still { image, .. } = d.content else { panic!("still") };
                assert_eq!(*image, src.to_rgba8(), "{format:?} {:?}", src.color());
                assert_eq!(d.info.bits, 16);
            }
        }
    }

    /// 整个解码：8 位 RGB 的 PNG 走新路，结果与 image 自己转的一样
    #[test]
    fn rgb_png_decodes_to_the_same_pixels() {
        let src = image::RgbImage::from_fn(7, 4, |x, y| image::Rgb([(x * 30) as u8, (y * 60) as u8, (x * y) as u8]));
        let mut png = Vec::new();
        src.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let d = decode(&png, ImageFormat::Png, &Options::default()).unwrap();
        let Content::Still { image, .. } = d.content else { panic!("still") };
        assert_eq!(*image, DynamicImage::ImageRgb8(src).into_rgba8());
        assert_eq!((d.info.channels, d.info.has_alpha), (3, false));
    }

    #[test]
    fn halve_weights_alpha_and_handles_odd_sizes() {
        let mut img = RgbaImage::new(3, 3);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, image::Rgba([0, 255, 0, 0])); // 透明，颜色不应渗入
        let h = halve(&img);
        assert_eq!(h.dimensions(), (2, 2));
        let p = h.get_pixel(0, 0).0;
        assert_eq!(&p[..3], &[255, 0, 0]);
        assert_eq!(p[3], 64);
        assert_eq!(h.get_pixel(1, 1).0, [0, 0, 0, 0]);
    }

    #[test]
    fn pyramid_stops_at_512() {
        let levels = pyramid(&RgbaImage::new(2000, 300));
        let sizes: Vec<_> = levels.iter().map(|l| l.dimensions()).collect();
        assert_eq!(sizes, [(1000, 150), (500, 75)]);
    }

    #[test]
    fn exif_orientation_applied() {
        // 2×1 JPEG，EXIF Orientation = 6（顺时针 90°）→ 显示为 1×2
        let mut jpeg = Vec::new();
        let img = image::RgbImage::from_pixel(2, 1, image::Rgb([200, 10, 10]));
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&img).unwrap();
        let exif: &[u8] = b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
        let mut app1 = vec![0xFF, 0xE1];
        app1.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        app1.extend_from_slice(exif);
        let with_exif = [&jpeg[..2], &app1, &jpeg[2..]].concat();
        let d = super::super::decode_bytes(&with_exif, "jpg", &Options::default()).unwrap();
        assert_eq!((d.info.width, d.info.height), (1, 2));
    }
}
