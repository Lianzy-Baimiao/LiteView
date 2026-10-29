//! 可分离的高质量缩放（Catmull-Rom / Lanczos3），在预乘透明度的像素上算（透明边缘不发黑），多线程。
//! 可以只算输出的一块区域，结果与整张算完全一致：编辑时缩小显示的图只重算改动影响到的部分。

use image::RgbaImage;
use rayon::prelude::*;

use super::IRect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// 缩小显示（与看图一致）
    CatmullRom,
    /// 调整大小
    Lanczos3,
}

impl Filter {
    fn support(self) -> f32 {
        match self {
            Filter::CatmullRom => 2.0,
            Filter::Lanczos3 => 3.0,
        }
    }

    fn eval(self, x: f32) -> f32 {
        let x = x.abs();
        match self {
            Filter::CatmullRom => {
                if x < 1.0 {
                    1.5 * x * x * x - 2.5 * x * x + 1.0
                } else if x < 2.0 {
                    -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
                } else {
                    0.0
                }
            }
            Filter::Lanczos3 => {
                if x < 1e-6 {
                    1.0
                } else if x < 3.0 {
                    let px = std::f32::consts::PI * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                } else {
                    0.0
                }
            }
        }
    }
}

/// 一个方向上每个输出像素用到哪些源像素、各占多少
#[derive(Clone, Debug)]
pub struct Axis {
    /// 每个输出像素：第一个源像素、权重
    taps: Vec<(u32, Vec<f32>)>,
}

impl Axis {
    pub fn new(src: u32, dst: u32, filter: Filter) -> Self {
        let (src, dst) = (src.max(1), dst.max(1));
        let ratio = src as f32 / dst as f32;
        let scale = ratio.max(1.0);
        let support = filter.support() * scale;
        let taps = (0..dst)
            .map(|i| {
                let center = (i as f32 + 0.5) * ratio;
                let left = ((center - support).floor() as i64).clamp(0, i64::from(src) - 1) as u32;
                let right = ((center + support).ceil() as i64).clamp(i64::from(left) + 1, i64::from(src)) as u32;
                let mut w: Vec<f32> = (left..right).map(|j| filter.eval((j as f32 + 0.5 - center) / scale)).collect();
                let sum: f32 = w.iter().sum();
                if sum.abs() < 1e-6 {
                    // 不会发生：退回最近的像素
                    w.iter_mut().for_each(|v| *v = 0.0);
                    let k = ((center as u32).clamp(left, right - 1) - left) as usize;
                    w[k] = 1.0;
                } else {
                    w.iter_mut().for_each(|v| *v /= sum);
                }
                (left, w)
            })
            .collect();
        Self { taps }
    }

    pub fn len(&self) -> u32 {
        self.taps.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.taps.is_empty()
    }

    fn span(&self, i: u32) -> (u32, u32) {
        let (l, w) = &self.taps[i as usize];
        (*l, *l + w.len() as u32)
    }

    /// 源像素 [a, b) 变了时受影响的输出像素范围
    pub fn affected(&self, a: u32, b: u32) -> (u32, u32) {
        let hits: Vec<u32> = (0..self.len()).filter(|&i| {
            let (l, r) = self.span(i);
            l < b && a < r
        }).collect();
        match (hits.first(), hits.last()) {
            (Some(&f), Some(&l)) => (f, l + 1),
            _ => (0, 0),
        }
    }
}

/// 源图 rect 变了时，输出上要重算的范围
pub fn affected(ax: &Axis, ay: &Axis, src: IRect) -> IRect {
    let (x0, x1) = ax.affected(src.x0, src.x1);
    let (y0, y1) = ay.affected(src.y0, src.y1);
    IRect::new(x0, y0, x1, y1)
}

/// 算输出的 out 这块；premultiplied = 结果是预乘透明度的（给显卡），否则是普通 RGBA
pub fn render(src: &RgbaImage, ax: &Axis, ay: &Axis, out: IRect, premultiplied: bool) -> Vec<u8> {
    let mut dst = vec![0u8; out.width() as usize * out.height() as usize * 4];
    render_into(&mut dst, src, ax, ay, out, premultiplied);
    dst
}

/// 同 `render`，写进 dst（out 那么大）
pub fn render_into(dst: &mut [u8], src: &RgbaImage, ax: &Axis, ay: &Axis, out: IRect, premultiplied: bool) {
    let (ow, oh) = (out.width() as usize, out.height() as usize);
    if ow == 0 || oh == 0 {
        return;
    }
    // 竖直方向要用到的源行
    let sy0 = ay.span(out.y0).0;
    let sy1 = (out.y0..out.y1).map(|y| ay.span(y).1).max().unwrap_or(sy0);
    let rows = (sy1 - sy0) as usize;
    let sw = src.width() as usize;
    let raw = src.as_raw();
    // 水平方向：每个源行算出这块的各列（预乘透明度）
    let mut mid = vec![0f32; rows * ow * 4];
    mid.par_chunks_mut(ow * 4).enumerate().for_each(|(r, line)| {
        let row = &raw[(sy0 as usize + r) * sw * 4..][..sw * 4];
        for (k, x) in (out.x0..out.x1).enumerate() {
            let (l, w) = &ax.taps[x as usize];
            let mut acc = [0f32; 4];
            for (j, &wt) in w.iter().enumerate() {
                let p = &row[(*l as usize + j) * 4..][..4];
                let a = f32::from(p[3]) / 255.0;
                acc[0] += f32::from(p[0]) * a * wt;
                acc[1] += f32::from(p[1]) * a * wt;
                acc[2] += f32::from(p[2]) * a * wt;
                acc[3] += f32::from(p[3]) * wt;
            }
            line[k * 4..k * 4 + 4].copy_from_slice(&acc);
        }
    });
    // 竖直方向
    debug_assert_eq!(dst.len(), ow * oh * 4);
    dst.par_chunks_mut(ow * 4).enumerate().for_each(|(k, line)| {
        let (l, w) = &ay.taps[(out.y0 as usize) + k];
        for x in 0..ow {
            let mut acc = [0f32; 4];
            for (j, &wt) in w.iter().enumerate() {
                let m = &mid[((*l - sy0) as usize + j) * ow * 4 + x * 4..][..4];
                for c in 0..4 {
                    acc[c] += m[c] * wt;
                }
            }
            let a = acc[3].clamp(0.0, 255.0);
            let px = &mut line[x * 4..x * 4 + 4];
            if premultiplied {
                for c in 0..3 {
                    px[c] = acc[c].clamp(0.0, a).round() as u8;
                }
            } else if a < 0.5 {
                px.copy_from_slice(&[0, 0, 0, 0]);
                continue;
            } else {
                for c in 0..3 {
                    px[c] = (acc[c] * 255.0 / a).clamp(0.0, 255.0).round() as u8;
                }
            }
            px[3] = a.round() as u8;
        }
    });
}

/// 整张图缩放到 w × h（分成几条并行算）
pub fn resize(src: &RgbaImage, w: u32, h: u32, filter: Filter, premultiplied: bool) -> Vec<u8> {
    let (w, h) = (w.max(1), h.max(1));
    let mut dst = vec![0u8; w as usize * h as usize * 4];
    resize_into(&mut dst, src, w, h, filter, premultiplied);
    dst
}

/// 同 `resize`，写进 dst（w × h × 4 字节）
pub fn resize_into(dst: &mut [u8], src: &RgbaImage, w: u32, h: u32, filter: Filter, premultiplied: bool) {
    render_all_into(dst, src, &Axis::new(src.width(), w, filter), &Axis::new(src.height(), h, filter), premultiplied);
}

/// 一条一条算、直接写进 dst（每条里面并行）。以前各条同时算再拼起来：16 线程时三十多条的中间结果同时在内存里，
/// 再加拼接、转换各一份结果，4800 万像素缩到九成时要多几百 MB。每个输出行怎么算与分条无关，结果不变
pub fn render_all_into(dst: &mut [u8], src: &RgbaImage, ax: &Axis, ay: &Axis, premultiplied: bool) {
    render_banded_into(dst, src, ax, ay, IRect::new(0, 0, ax.len(), ay.len()), premultiplied);
}

/// 输出的 out 这块一条条算、写进 dst（out 那么大）。一次算一大块时中间结果是源行数 × 输出宽：
/// 编辑时整张图翻转，4800 万像素在 90% 下要约 660 MB；一条条算只有约 8 MB。结果与 `render_into` 一次算的相同
pub fn render_banded_into(dst: &mut [u8], src: &RgbaImage, ax: &Axis, ay: &Axis, out: IRect, premultiplied: bool) {
    let (ow, oh) = (out.width(), out.height());
    if ow == 0 || oh == 0 {
        return;
    }
    // 每条的中间结果（源行数 × 输出宽 × 4 个 f32）不超过约 8 MB
    let src_per_row = f64::from(src.height()) / f64::from(ay.len().max(1));
    let rows = (8.0 * 1048576.0 / (f64::from(ow) * 16.0)).max(8.0);
    let band = ((rows / (src_per_row + 1.0)) as u32).clamp(8, oh.max(8));
    for (i, part) in dst.chunks_mut(band as usize * ow as usize * 4).enumerate() {
        let y0 = out.y0 + i as u32 * band;
        render_into(part, src, ax, ay, IRect::new(out.x0, y0, out.x1, (y0 + band).min(out.y1)), premultiplied);
    }
}

/// 调整大小：普通 RGBA 结果
pub fn resize_image(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    RgbaImage::from_raw(w, h, resize(src, w, h, Filter::Lanczos3, false)).expect("buffer size")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(w: u32, h: u32) -> RgbaImage {
        let mut s = 99u32;
        RgbaImage::from_fn(w, h, |_, _| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let v = s.to_le_bytes();
            image::Rgba([v[0], v[1], v[2], v[3] | 0x80])
        })
    }

    /// 输出宽 2000 时每条约 37 行、分二十多条一条条算：与整张一次算出来的逐字节相同
    #[test]
    fn banded_resize_matches_one_pass() {
        let src = noise(3000, 2000);
        let (w, h) = (2000, 900);
        let (ax, ay) = (Axis::new(3000, w, Filter::CatmullRom), Axis::new(2000, h, Filter::CatmullRom));
        let one = render(&src, &ax, &ay, IRect::new(0, 0, w, h), true);
        assert_eq!(resize(&src, w, h, Filter::CatmullRom, true), one);
        let mut pixels = vec![egui::Color32::TRANSPARENT; (w * h) as usize];
        resize_into(bytemuck::cast_slice_mut(&mut pixels), &src, w, h, Filter::CatmullRom, true);
        assert_eq!(bytemuck::cast_slice::<_, u8>(&pixels), &one[..]);
    }

    /// 只算一块（编辑时改了一部分）也一条条算：十来条，与这块一次算出来的逐字节相同
    #[test]
    fn banded_part_matches_one_pass() {
        let src = noise(3000, 2000);
        let (ax, ay) = (Axis::new(3000, 2000, Filter::CatmullRom), Axis::new(2000, 900, Filter::CatmullRom));
        let part = IRect::new(100, 50, 1900, 870);
        let one = render(&src, &ax, &ay, part, true);
        let mut banded = vec![0u8; one.len()];
        render_banded_into(&mut banded, &src, &ax, &ay, part, true);
        assert_eq!(banded, one);
    }

    #[test]
    fn partial_render_matches_full() {
        let src = noise(500, 300);
        let (w, h) = (137, 83);
        let full = resize(&src, w, h, Filter::CatmullRom, true);
        let (ax, ay) = (Axis::new(500, w, Filter::CatmullRom), Axis::new(300, h, Filter::CatmullRom));
        let r = IRect::new(20, 30, 90, 61);
        let part = render(&src, &ax, &ay, r, true);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let a = ((y - r.y0) * r.width() + (x - r.x0)) as usize * 4;
                let b = (y * w + x) as usize * 4;
                assert_eq!(part[a..a + 4], full[b..b + 4], "({x}, {y})");
            }
        }
    }

    #[test]
    fn affected_region_is_conservative() {
        let src = noise(400, 400);
        let (w, h) = (90, 70);
        let before = resize(&src, w, h, Filter::CatmullRom, true);
        let mut changed = src.clone();
        let edit = IRect::new(100, 150, 140, 170);
        for y in edit.y0..edit.y1 {
            for x in edit.x0..edit.x1 {
                changed.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
            }
        }
        let after = resize(&changed, w, h, Filter::CatmullRom, true);
        let (ax, ay) = (Axis::new(400, w, Filter::CatmullRom), Axis::new(400, h, Filter::CatmullRom));
        let out = affected(&ax, &ay, edit);
        assert!(!out.is_empty());
        for y in 0..h {
            for x in 0..w {
                if !out.contains(x, y) {
                    let i = (y * w + x) as usize * 4;
                    assert_eq!(before[i..i + 4], after[i..i + 4], "({x}, {y}) outside {out:?}");
                }
            }
        }
    }

    #[test]
    fn keeps_solid_colors_and_identity() {
        let src = RgbaImage::from_pixel(64, 48, image::Rgba([12, 200, 99, 255]));
        let small = resize_image(&src, 17, 9);
        assert!(small.pixels().all(|p| p.0 == [12, 200, 99, 255]));
        let n = noise(40, 30);
        assert_eq!(resize_image(&n, 40, 30), n);
        let big = resize_image(&src, 200, 150);
        assert!(big.pixels().all(|p| p.0 == [12, 200, 99, 255]));
    }
}
