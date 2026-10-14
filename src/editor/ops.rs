//! 像素操作：裁剪 / 取出一块、填充、贴图（src-over）、旋转翻转、调整大小、颜料桶。

use image::RgbaImage;
use rayon::prelude::*;

use super::IRect;

/// 取出 r 这块
pub fn extract(img: &RgbaImage, r: IRect) -> RgbaImage {
    image::imageops::crop_imm(img, r.x0, r.y0, r.width(), r.height()).to_image()
}

/// 把 r 填成 color
pub fn fill_rect(img: &mut RgbaImage, r: IRect, color: [u8; 4]) {
    let r = r.intersect(&IRect::full(img.width(), img.height()));
    let w = img.width() as usize;
    let raw: &mut [u8] = img;
    raw.par_chunks_mut(w * 4).skip(r.y0 as usize).take(r.height() as usize).for_each(|row| {
        for p in row[r.x0 as usize * 4..r.x1 as usize * 4].as_chunks_mut::<4>().0 {
            p.copy_from_slice(&color);
        }
    });
}

/// 把 src 叠到 img 上（左上角在 (x, y)，可以有一部分在图外）；返回改到的范围
pub fn composite(img: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64) -> IRect {
    let (w, h) = (i64::from(img.width()), i64::from(img.height()));
    let (sw, sh) = (i64::from(src.width()), i64::from(src.height()));
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + sw).min(w), (y + sh).min(h));
    if x1 <= x0 || y1 <= y0 {
        return IRect::EMPTY;
    }
    let iw = img.width() as usize;
    let raw: &mut [u8] = img;
    raw.par_chunks_mut(iw * 4).enumerate().skip(y0 as usize).take((y1 - y0) as usize).for_each(|(yy, row)| {
        let sy = (yy as i64 - y) as u32;
        for xx in x0..x1 {
            let s = src.get_pixel((xx - x) as u32, sy).0;
            let d = &mut row[xx as usize * 4..xx as usize * 4 + 4];
            let out = over(s, [d[0], d[1], d[2], d[3]]);
            d.copy_from_slice(&out);
        }
    });
    IRect::new(x0 as u32, y0 as u32, x1 as u32, y1 as u32)
}

/// s 盖在 d 上（普通 RGBA）
pub fn over(s: [u8; 4], d: [u8; 4]) -> [u8; 4] {
    match s[3] {
        255 => return s,
        0 => return d,
        _ => {}
    }
    let (sa, da) = (u32::from(s[3]), u32::from(d[3]));
    let a = sa * 255 + da * (255 - sa); // 0 ~ 255²
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let c = |i: usize| ((u32::from(s[i]) * sa * 255 + u32::from(d[i]) * da * (255 - sa) + a / 2) / a).min(255) as u8;
    [c(0), c(1), c(2), ((a + 127) / 255) as u8]
}

pub fn rotate90(img: &RgbaImage) -> RgbaImage {
    image::imageops::rotate90(img)
}

pub fn rotate180(img: &RgbaImage) -> RgbaImage {
    image::imageops::rotate180(img)
}

pub fn rotate270(img: &RgbaImage) -> RgbaImage {
    image::imageops::rotate270(img)
}

pub fn flip_h(img: &RgbaImage) -> RgbaImage {
    image::imageops::flip_horizontal(img)
}

pub fn flip_v(img: &RgbaImage) -> RgbaImage {
    image::imageops::flip_vertical(img)
}

/// 扩大画布：原图放在左上角，新的部分填 fill
pub fn expand(img: &RgbaImage, w: u32, h: u32, fill: [u8; 4]) -> RgbaImage {
    let mut out = RgbaImage::from_pixel(w.max(img.width()), h.max(img.height()), image::Rgba(fill));
    image::imageops::replace(&mut out, img, 0, 0);
    out
}

/// 颜料桶：从 (x, y) 开始，与起点颜色相差（各通道差的最大值）不超过 tolerance 的连通区域（四邻接）。
/// 返回每个像素是否在区域里（按位存）与区域的范围
pub fn flood(img: &RgbaImage, x: u32, y: u32, tolerance: u8) -> (Vec<u64>, IRect) {
    let (w, h) = img.dimensions();
    let n = w as usize * h as usize;
    let mut bits = vec![0u64; n.div_ceil(64)];
    if x >= w || y >= h {
        return (bits, IRect::EMPTY);
    }
    let raw = img.as_raw();
    let seed = img.get_pixel(x, y).0;
    let t = i16::from(tolerance);
    let close = |i: usize| {
        let p = &raw[i * 4..i * 4 + 4];
        (0..4).all(|c| (i16::from(p[c]) - i16::from(seed[c])).abs() <= t)
    };
    let get = |b: &[u64], i: usize| b[i / 64] >> (i % 64) & 1 == 1;
    let (mut minx, mut miny, mut maxx, mut maxy) = (x, y, x, y);
    let mut stack = vec![(x, y)];
    while let Some((sx, sy)) = stack.pop() {
        let row = sy as usize * w as usize;
        if get(&bits, row + sx as usize) {
            continue;
        }
        let (mut l, mut r) = (sx, sx);
        while l > 0 && !get(&bits, row + l as usize - 1) && close(row + l as usize - 1) {
            l -= 1;
        }
        while r + 1 < w && !get(&bits, row + r as usize + 1) && close(row + r as usize + 1) {
            r += 1;
        }
        for i in l..=r {
            let k = row + i as usize;
            bits[k / 64] |= 1 << (k % 64);
        }
        (minx, maxx, miny, maxy) = (minx.min(l), maxx.max(r), miny.min(sy), maxy.max(sy));
        // 上下两行：每段连续的相近像素放一个种子
        for ny in [sy.checked_sub(1), (sy + 1 < h).then_some(sy + 1)].into_iter().flatten() {
            let nrow = ny as usize * w as usize;
            let mut i = l;
            while i <= r {
                let k = nrow + i as usize;
                if !get(&bits, k) && close(k) {
                    stack.push((i, ny));
                    while i <= r && !get(&bits, nrow + i as usize) && close(nrow + i as usize) {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    (bits, IRect::new(minx, miny, maxx + 1, maxy + 1))
}

/// 把 flood 选中的像素填成 color（不抗锯齿）
pub fn fill_bits(img: &mut RgbaImage, bits: &[u64], r: IRect, color: [u8; 4]) {
    let w = img.width() as usize;
    let raw: &mut [u8] = img;
    raw.par_chunks_mut(w * 4).enumerate().skip(r.y0 as usize).take(r.height() as usize).for_each(|(y, row)| {
        for x in r.x0 as usize..r.x1 as usize {
            let k = y * w + x;
            if bits[k / 64] >> (k % 64) & 1 == 1 {
                row[x * 4..x * 4 + 4].copy_from_slice(&color);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_fills_connected_region_only() {
        // 左右两块白色被一条黑线隔开
        let mut img = RgbaImage::from_pixel(20, 10, image::Rgba([255, 255, 255, 255]));
        for y in 0..10 {
            img.put_pixel(10, y, image::Rgba([0, 0, 0, 255]));
        }
        img.put_pixel(3, 3, image::Rgba([240, 240, 240, 255]));
        let (bits, r) = flood(&img, 0, 0, 32);
        assert_eq!(r, IRect::new(0, 0, 10, 10));
        let mut out = img.clone();
        fill_bits(&mut out, &bits, r, [255, 0, 0, 255]);
        assert_eq!(out.get_pixel(3, 3).0, [255, 0, 0, 255], "within tolerance");
        assert_eq!(out.get_pixel(15, 5).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(10, 5).0, [0, 0, 0, 255]);
        let (_, strict) = flood(&img, 0, 0, 0);
        assert_eq!(strict, IRect::new(0, 0, 10, 10));
        // 螺旋形的区域也能填满
        let mut maze = RgbaImage::from_pixel(9, 9, image::Rgba([0, 0, 0, 255]));
        for (x, y) in [(1, 1), (2, 1), (3, 1), (3, 2), (3, 3), (2, 3), (1, 3), (1, 4), (1, 5), (2, 5), (3, 5), (4, 5), (5, 5), (5, 4), (5, 3), (5, 2), (5, 1)] {
            maze.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
        let (bits, _) = flood(&maze, 1, 1, 0);
        let count: u32 = bits.iter().map(|b| b.count_ones()).sum();
        assert_eq!(count, 17);
    }

    #[test]
    fn composite_and_over() {
        let mut img = RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 255, 255]));
        let src = RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 128]));
        let r = composite(&mut img, &src, 8, -2);
        assert_eq!(r, IRect::new(8, 0, 10, 2));
        let p = img.get_pixel(9, 0).0;
        assert!(p[0] > 120 && p[2] > 120 && p[3] == 255, "{p:?}");
        assert_eq!(img.get_pixel(7, 0).0, [0, 0, 255, 255]);
        assert_eq!(over([1, 2, 3, 0], [9, 9, 9, 9]), [9, 9, 9, 9]);
        let e = expand(&RgbaImage::from_pixel(2, 2, image::Rgba([1, 1, 1, 255])), 4, 3, [0, 0, 0, 0]);
        assert_eq!((e.dimensions(), e.get_pixel(1, 1).0, e.get_pixel(3, 2).0), ((4, 3), [1, 1, 1, 255], [0, 0, 0, 0]));
    }
}
