//! 打码的「涂抹」效果：像用油画笔横着抹过去。
//!
//! 整张图从上到下分成一条条横的笔触（宽 s 像素），笔触的上下边是平缓的波浪线；每条笔触再按 2.5 s
//! 一段分段，相邻笔触的段错开。每段只取原图的平均色（和马赛克方块一样，原图的细节一点不留），段与段
//! 之间平滑过渡，再叠上与图片内容无关的刷毛纹理。笔触按整张图的坐标划分：画笔、框选混着用，分块算，
//! 结果都一样。
//!
//! 框选时（[`Ragged`]）框里全部涂满，框外留一圈不规则的边：左右是一条条长短不一的圆头笔触，
//! 上下是起伏的笔触边缘，最多伸出约一个笔触宽度。

use image::RgbaImage;
use rayon::prelude::*;

use super::IRect;
use super::history::Saved;

/// 每段的长度（笔触宽度的倍数）
const SEG: f32 = 2.5;

fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ 0x27D4_EB2F;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// 第 k 条笔触的第 n 种随机数用的种子
fn key(k: i32, n: u32) -> u32 {
    (k as u32).wrapping_mul(0x9E37_79B9) ^ n.wrapping_mul(0x632B_E5AB)
}

/// 0 ~ 1 的伪随机数
fn rand(seed: u32, i: u32) -> f32 {
    (hash(seed, i) >> 8) as f32 / 16_777_216.0
}

/// 平滑起伏的一维噪声（0 ~ 1），t 每变 1 起伏一次
fn noise(seed: u32, t: f32) -> f32 {
    let i = t.floor();
    let f = smooth(t - i);
    let i = i as i32 as u32;
    let (a, b) = (rand(seed, i), rand(seed, i.wrapping_add(1)));
    a + (b - a) * f
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// 第 k 条笔触的上边在 x 处的 y（第 k 条笔触在 edge(k) 与 edge(k + 1) 之间）：围绕 k·s 起伏
/// 不超过 s / 6，相邻的边不会交叉
fn edge(s: f32, k: i32, x: f32) -> f32 {
    let w = (noise(key(k, 1), x / (s * 9.0)) - 0.5) * 0.24 + (noise(key(k, 2), x / (s * 2.2)) - 0.5) * 0.08;
    (k as f32 + w) * s
}

/// (x, y) 在第几条笔触里，以及在这条笔触里从上往下的位置（0 ~ 1）
fn band_at(s: f32, x: f32, y: f32) -> (i32, f32) {
    let mut k = (y / s).floor() as i32;
    if y < edge(s, k, x) {
        k -= 1;
    } else if y >= edge(s, k + 1, x) {
        k += 1;
    }
    let (top, bottom) = (edge(s, k, x), edge(s, k + 1, x));
    (k, ((y - top) / (bottom - top)).clamp(0.0, 1.0))
}

/// 第 k 条笔触的段错开多少像素
fn shift(s: f32, k: i32) -> f32 {
    rand(key(k, 3), 0) * SEG * s
}

/// 宽 w 的图上第 k 条笔触用到的第一段、最后一段：图边不到半段的零头并进旁边那段
fn seg_range(s: f32, k: i32, w: u32) -> (i32, i32) {
    let (l, sh) = (SEG * s, shift(s, k));
    let mut last = ((w as f32 + sh) / l).ceil() as i32 - 1;
    let mut first = 0;
    if l - sh < l / 2.0 && first < last {
        first = 1;
    }
    if w as f32 - (last as f32 * l - sh) < l / 2.0 && last > first {
        last -= 1;
    }
    (first, last)
}

/// 高 h 的图上的最后一条笔触（取色用）：底边不到半条的零头并进上面那条
fn band_last(si: u32, h: u32) -> i32 {
    let mut last = h.div_ceil(si) as i32 - 1;
    if last > 0 && 2 * (h - last as u32 * si) < si {
        last -= 1;
    }
    last.max(0)
}

/// 第 k 条笔触取色的行（第一条、最后一条延伸到图边）
fn band_rows(si: u32, k: i32, last: i32, h: u32) -> (u32, u32) {
    let y0 = if k <= 0 { 0 } else { (k as u32 * si).min(h) };
    let y1 = if k >= last { h } else { ((k as u32 + 1) * si).min(h) };
    (y0, y1)
}

/// 一条笔触在一块里要用到的各段
struct Row {
    shift: f32,
    /// 这条笔触的第一段、最后一段
    first: i32,
    last: i32,
    /// cells[0] 是第几段
    j0: i32,
    /// 各段的平均色（预乘透明度，0 ~ 255）
    cells: Vec<[f32; 4]>,
}

impl Row {
    /// 第 j 段取色的列（第一段、最后一段延伸到图边）
    fn cols(&self, s: f32, j: i32, w: u32) -> (u32, u32) {
        let l = SEG * s;
        let at = |j: i32| (j as f32 * l - self.shift).round().clamp(0.0, w as f32) as u32;
        let x0 = if j <= self.first { 0 } else { at(j) };
        let x1 = if j >= self.last { w } else { at(j + 1) };
        (x0, x1.max(x0))
    }

    /// x 处的颜色（预乘透明度）：相邻两段的平均色之间平滑过渡
    fn at(&self, l: f32, x: f32) -> [f32; 4] {
        let u = (x + self.shift) / l - 0.5;
        let j = u.floor();
        let t = smooth(u - j);
        let cell = |j: i32| self.cells[(j.clamp(self.first, self.last) - self.j0) as usize];
        let (a, b) = (cell(j as i32), cell(j as i32 + 1));
        mix(a, b, t)
    }
}

/// 一块要用到的笔触（取色用的笔触，图外的笔触用最近的那条）
struct Palette {
    k0: i32,
    rows: Vec<Row>,
}

impl Palette {
    /// tr 这一块、第 k0 ~ k1 条笔触
    fn new(saved: &Saved, img: &RgbaImage, s: f32, tr: IRect, k0: i32, k1: i32) -> Self {
        let (w, h) = img.dimensions();
        let (si, l) = (s as u32, SEG * s);
        let lastk = band_last(si, h);
        let (c0, c1) = (k0.clamp(0, lastk), k1.clamp(0, lastk));
        let mut rows: Vec<Row> = (c0..=c1)
            .map(|k| {
                let shift = shift(s, k);
                let (first, last) = seg_range(s, k, w);
                // 这一块的像素落在哪几段之间
                let j = |x: f32| ((x + shift) / l - 0.5).floor() as i32;
                let (j0, j1) = (j(tr.x0 as f32 + 0.5).clamp(first, last), (j(tr.x1 as f32 - 0.5) + 1).clamp(first, last));
                Row { shift, first, last, j0, cells: vec![[0.0; 4]; (j1 - j0 + 1) as usize] }
            })
            .collect();
        // 一次读出要用到的原图，再按段求平均
        let (y0, y1) = (band_rows(si, c0, lastk, h).0, band_rows(si, c1, lastk, h).1);
        let (x0, x1) = rows.iter().fold((w, 0), |(a, b), r| (a.min(r.cols(s, r.j0, w).0), b.max(r.cols(s, r.j0 + r.cells.len() as i32 - 1, w).1)));
        let src = IRect::new(x0, y0, x1, y1);
        let data = saved.region(img, src);
        let sw = src.width() as usize;
        for (k, row) in (c0..).zip(rows.iter_mut()) {
            let (ry0, ry1) = band_rows(si, k, lastk, h);
            let cells: Vec<[f32; 4]> = (0..row.cells.len() as i32)
                .map(|i| {
                    let (cx0, cx1) = row.cols(s, row.j0 + i, w);
                    let mut sum = [0u64; 4];
                    for y in ry0..ry1 {
                        let off = ((y - src.y0) as usize * sw + (cx0 - src.x0) as usize) * 4;
                        for p in data[off..off + (cx1 - cx0) as usize * 4].as_chunks::<4>().0 {
                            let a = u64::from(p[3]);
                            sum[0] += u64::from(p[0]) * a;
                            sum[1] += u64::from(p[1]) * a;
                            sum[2] += u64::from(p[2]) * a;
                            sum[3] += a;
                        }
                    }
                    let n = (u64::from(cx1 - cx0) * u64::from(ry1 - ry0)).max(1) as f64;
                    let c = |ch: usize, d: f64| (sum[ch] as f64 / n / d) as f32;
                    [c(0, 255.0), c(1, 255.0), c(2, 255.0), c(3, 1.0)]
                })
                .collect();
            row.cells = cells;
        }
        Self { k0: c0, rows }
    }

    /// 第 k 条笔触（几何上的）用哪条取色
    fn at(&self, k: i32) -> &Row {
        &self.rows[(k.clamp(self.k0, self.k0 + self.rows.len() as i32 - 1) - self.k0) as usize]
    }
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// 刷毛纹理的强度（亮度增减的倍数，0 ~ 255）
const TEXTURE: f32 = 160.0;

/// 刷毛纹理（与图片内容无关）：亮度增减，约 -0.18 ~ 0.12。v 是在笔触里从上往下的位置，d 是离笔触边多远
fn texture(s: f32, bristles: f32, k: i32, v: f32, x: f32, d: f32) -> f32 {
    // 一根根刷毛横着走：每根深浅不同，顺着走向慢慢变
    let b = (v * bristles).floor().min(bristles - 1.0) as u32;
    let streak = (rand(key(k, 4), b) - 0.5) * 0.10 + (noise(hash(key(k, 5), b), x / (s * 1.7)) - 0.5) * 0.08;
    // 笔触上沿略亮、下沿略暗；两笔交界处颜料堆得厚，暗一些
    let ridge = (0.5 - v) * 0.05;
    let seam = if d < 2.0 { -(2.0 - d) * 0.03 } else { 0.0 };
    streak + ridge + seam
}

/// 预乘透明度的颜色 → 像素，亮度加上 light
fn finish(c: [f32; 4], light: f32) -> [u8; 4] {
    if c[3] < 0.5 {
        return [0, 0, 0, 0];
    }
    let k = 255.0 / c[3];
    let ch = |v: f32| (v * k + light * TEXTURE).round().clamp(0.0, 255.0) as u8;
    [ch(c[0]), ch(c[1]), ch(c[2]), c[3].round().min(255.0) as u8]
}

/// 一块（tr）的涂抹结果（从落笔前的图计算）；strength 是笔触宽度（像素）
pub fn tile(saved: &Saved, img: &RgbaImage, strength: u32, tr: IRect) -> Vec<u8> {
    let s = strength.max(1) as f32;
    let (l, tw) = (SEG * s, tr.width() as usize);
    // 这一块的像素在哪几条笔触里：边起伏不到 s / 6，上下各多算一条，抗锯齿再用到相邻的一条
    let (k0, k1) = ((tr.y0 as f32 / s).floor() as i32 - 2, (tr.y1 as f32 / s).floor() as i32 + 2);
    // 每条边在每一列的 y
    let edges: Vec<f32> = (k0..=k1).flat_map(|k| (tr.x0..tr.x1).map(move |x| edge(s, k, x as f32 + 0.5))).collect();
    let e = |k: i32, i: usize| edges[(k - k0) as usize * tw + i];
    let pal = Palette::new(saved, img, s, tr, k0, k1);
    let bristles = (s / 3.0).round().max(3.0);
    let mut out = vec![0u8; tr.area() as usize * 4];
    for y in tr.y0..tr.y1 {
        let py = y as f32 + 0.5;
        for x in tr.x0..tr.x1 {
            let (i, px) = ((x - tr.x0) as usize, x as f32 + 0.5);
            let mut k = (py / s).floor() as i32;
            if py < e(k, i) {
                k -= 1;
            } else if py >= e(k + 1, i) {
                k += 1;
            }
            let (top, bottom) = (e(k, i), e(k + 1, i));
            let (dt, db) = (py - top, bottom - py);
            let mut c = pal.at(k).at(l, px);
            // 笔触交界处抗锯齿：离边不到半个像素时与相邻的笔触混合
            if dt < 0.5 {
                c = mix(pal.at(k - 1).at(l, px), c, 0.5 + dt);
            } else if db < 0.5 {
                c = mix(pal.at(k + 1).at(l, px), c, 0.5 + db);
            }
            let v = (dt / (bottom - top)).clamp(0.0, 1.0);
            let o = ((y - tr.y0) as usize * tw + i) * 4;
            out[o..o + 4].copy_from_slice(&finish(c, texture(s, bristles, k, v, px, dt.min(db))));
        }
    }
    out
}

/// 转过 angle 的笔触坐标：u 顺着笔触，v 垂直于它；分条、分段、取色都在这里与横着的一样做。
/// 取色的条与段只与整张图有关（与分块无关）
struct Frame {
    s: f32,
    l: f32,
    sin: f32,
    cos: f32,
    /// 整张图（像素边）四个角的 (u, v)，按顺序
    corners: [(f32, f32); 4],
    vlo: f32,
    vhi: f32,
    /// 取色的第一条、最后一条（图边不到半条的零头并进旁边那条）
    kfirst: i32,
    klast: i32,
}

impl Frame {
    fn new(w: u32, h: u32, s: f32, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        let uv = |x: f32, y: f32| (x * cos + y * sin, -x * sin + y * cos);
        let corners = [(0.0, 0.0), (w as f32, 0.0), (w as f32, h as f32), (0.0, h as f32)].map(|(x, y)| uv(x, y));
        let (vlo, vhi) = corners.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c.1), b.max(c.1)));
        let (mut kfirst, mut klast) = ((vlo / s).floor() as i32, (vhi / s).ceil() as i32 - 1);
        if (kfirst + 1) as f32 * s - vlo < s / 2.0 && kfirst < klast {
            kfirst += 1;
        }
        if vhi - klast as f32 * s < s / 2.0 && klast > kfirst {
            klast -= 1;
        }
        Self { s, l: SEG * s, sin, cos, corners, vlo, vhi, kfirst, klast }
    }

    fn uv(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.cos + y * self.sin, -x * self.sin + y * self.cos)
    }

    fn xy(&self, u: f32, v: f32) -> (f32, f32) {
        (u * self.cos - v * self.sin, u * self.sin + v * self.cos)
    }

    /// v 落在取色的第几条
    fn color_band(&self, v: f32) -> i32 {
        ((v / self.s).floor() as i32).clamp(self.kfirst, self.klast)
    }

    /// 取色的第 k 条的 v 范围（第一条、最后一条延伸到图边）
    fn band_v(&self, k: i32) -> (f32, f32) {
        (if k <= self.kfirst { self.vlo } else { k as f32 * self.s }, if k >= self.klast { self.vhi } else { (k + 1) as f32 * self.s })
    }

    /// 图在 v ∈ [va, vb] 这一条里的 u 范围
    fn u_extent(&self, va: f32, vb: f32) -> (f32, f32) {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let mut add = |u: f32| {
            lo = lo.min(u);
            hi = hi.max(u);
        };
        for i in 0..4 {
            let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
            if (va..=vb).contains(&p.1) {
                add(p.0);
            }
            for c in [va, vb] {
                if (p.1 - c) * (q.1 - c) <= 0.0 && p.1 != q.1 {
                    add(p.0 + (c - p.1) / (q.1 - p.1) * (q.0 - p.0));
                }
            }
        }
        if lo > hi { (0.0, 0.0) } else { (lo, hi) }
    }

    /// 取色的第 k 条：段错开多少、第一段、最后一段（图边不到半段的零头并进旁边那段）、这一条的 u 范围
    fn segs(&self, k: i32) -> (f32, i32, i32, f32, f32) {
        let (l, sh) = (self.l, shift(self.s, k));
        let (va, vb) = self.band_v(k);
        let (ulo, uhi) = self.u_extent(va, vb);
        let (mut first, mut last) = (((ulo + sh) / l).floor() as i32, ((uhi + sh) / l).ceil() as i32 - 1);
        if (first + 1) as f32 * l - sh - ulo < l / 2.0 && first < last {
            first += 1;
        }
        if uhi - (last as f32 * l - sh) < l / 2.0 && last > first {
            last -= 1;
        }
        (sh, first, last.max(first), ulo, uhi)
    }

    /// 原图像素 (x, y) 的中心算进哪一条的哪一段（与 tile_angled 里求平均时一样）
    #[cfg(test)]
    fn cell(&self, x: u32, y: u32) -> (i32, i32) {
        let (u, v) = self.uv(x as f32 + 0.5, y as f32 + 0.5);
        let k = self.color_band(v);
        let (sh, first, last, _, _) = self.segs(k);
        (k, (((u + sh) / self.l).floor() as i32).clamp(first, last))
    }
}

/// 涂抹沿 angle（弧度，0 = 横着；y 朝下，正的往右下）方向走：像素中心转到笔触坐标（[`Frame`]）里，与横着的一样分条、
/// 分段；每段的平均色取中心落在这一段里的原图像素（转过的方格），原图的细节同样一点不留。angle = 0 就是 [`tile`]
pub fn tile_angled(saved: &Saved, img: &RgbaImage, strength: u32, tr: IRect, angle: f32) -> Vec<u8> {
    if angle == 0.0 {
        return tile(saved, img, strength, tr);
    }
    let (w, h) = img.dimensions();
    let s = strength.max(1) as f32;
    let f = Frame::new(w, h, s, angle);
    let l = f.l;
    // 这一块（像素边）在笔触坐标里的范围
    let tc = [(tr.x0, tr.y0), (tr.x1, tr.y0), (tr.x1, tr.y1), (tr.x0, tr.y1)].map(|(x, y)| f.uv(x as f32, y as f32));
    let (tumin, tumax) = tc.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c.0), b.max(c.0)));
    let (tvmin, tvmax) = tc.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c.1), b.max(c.1)));
    // 要用到的条（几何上的条边起伏不到 s / 6，上下各多算两条）和它们取色的段
    let (c0, c1) = (((tvmin / s).floor() as i32 - 2).clamp(f.kfirst, f.klast), ((tvmax / s).floor() as i32 + 2).clamp(f.kfirst, f.klast));
    let segs: Vec<_> = (c0..=c1).map(|k| f.segs(k)).collect();
    let mut rows: Vec<Row> = segs
        .iter()
        .map(|&(shift, first, last, _, _)| {
            let j = |u: f32| ((u + shift) / l - 0.5).floor() as i32;
            let (j0, j1) = (j(tumin).clamp(first, last), (j(tumax) + 1).clamp(first, last));
            Row { shift, first, last, j0, cells: vec![[0.0; 4]; (j1 - j0 + 1) as usize] }
        })
        .collect();
    // 这些段在笔触坐标里的范围 → 原图上要读的那块
    let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (i, (row, &(sh, first, last, ulo, uhi))) in rows.iter().zip(&segs).enumerate() {
        let k = c0 + i as i32;
        let j1 = row.j0 + row.cells.len() as i32 - 1;
        umin = umin.min(if row.j0 <= first { ulo } else { row.j0 as f32 * l - sh });
        umax = umax.max(if j1 >= last { uhi } else { (j1 + 1) as f32 * l - sh });
        let (va, vb) = f.band_v(k);
        (vmin, vmax) = (vmin.min(va), vmax.max(vb));
    }
    let bc = [(umin, vmin), (umax, vmin), (umax, vmax), (umin, vmax)].map(|(u, v)| f.xy(u, v));
    let (bx0, bx1) = bc.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c.0), b.max(c.0)));
    let (by0, by1) = bc.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c.1), b.max(c.1)));
    let clampx = |v: f32| (v.max(0.0) as u32).min(w);
    let clampy = |v: f32| (v.max(0.0) as u32).min(h);
    let src = IRect::new(clampx(bx0.floor() - 1.0), clampy(by0.floor() - 1.0), clampx(bx1.ceil() + 1.0), clampy(by1.ceil() + 1.0));
    let data = saved.region(img, src);
    let sw = src.width() as usize;
    type Sums = Vec<Vec<([u64; 4], u64)>>;
    let zero = || -> Sums { rows.iter().map(|r| vec![([0; 4], 0); r.cells.len()]).collect() };
    // 每一行只走 [umin, umax] × [vmin, vmax] 那个（转过的）框落在这一行的那一段（多留一个像素；里面还逐个判断条和段）
    let span = |py: f32| -> (u32, u32) {
        let (mut lo, mut hi) = (f32::MIN, f32::MAX);
        let mut within = |a: f32, b0: f32, b1: f32| {
            // b0 ≤ a·px ≤ b1
            if a.abs() > 1e-6 {
                let (p, q) = ((b0 / a).min(b1 / a), (b0 / a).max(b1 / a));
                (lo, hi) = (lo.max(p), hi.min(q));
            }
        };
        within(f.cos, umin - py * f.sin, umax - py * f.sin);
        within(-f.sin, vmin - py * f.cos, vmax - py * f.cos);
        let x0 = if lo == f32::MIN { src.x0 } else { ((lo - 1.5).floor().max(0.0) as u32).clamp(src.x0, src.x1) };
        let x1 = if hi == f32::MAX { src.x1 } else { ((hi + 1.5).ceil().max(0.0) as u32).clamp(x0, src.x1) };
        (x0, x1)
    };
    // 整数相加，与分几份、什么顺序加无关：分给几个线程按行加，再合起来
    let sums: Sums = (src.y0..src.y1)
        .into_par_iter()
        .fold(zero, |mut sums, y| {
            let py = y as f32 + 0.5;
            let (x0, x1) = span(py);
            for x in x0..x1 {
                let (u, v) = f.uv(x as f32 + 0.5, py);
                let k = f.color_band(v);
                if k < c0 || k > c1 {
                    continue;
                }
                let ri = (k - c0) as usize;
                let row = &rows[ri];
                let j = (((u + row.shift) / l).floor() as i32).clamp(row.first, row.last);
                if j < row.j0 || j >= row.j0 + row.cells.len() as i32 {
                    continue;
                }
                let p = &data[((y - src.y0) as usize * sw + (x - src.x0) as usize) * 4..][..4];
                let (sum, n) = &mut sums[ri][(j - row.j0) as usize];
                let a = u64::from(p[3]);
                sum[0] += u64::from(p[0]) * a;
                sum[1] += u64::from(p[1]) * a;
                sum[2] += u64::from(p[2]) * a;
                sum[3] += a;
                *n += 1;
            }
            sums
        })
        .reduce(zero, |mut a, b| {
            for (ra, rb) in a.iter_mut().zip(b) {
                for ((sa, na), (sb, nb)) in ra.iter_mut().zip(rb) {
                    for c in 0..4 {
                        sa[c] += sb[c];
                    }
                    *na += nb;
                }
            }
            a
        });
    for (row, sums) in rows.iter_mut().zip(&sums) {
        for (cell, (sum, n)) in row.cells.iter_mut().zip(sums) {
            let n = (*n).max(1) as f64;
            let c = |ch: usize, d: f64| (sum[ch] as f64 / n / d) as f32;
            *cell = [c(0, 255.0), c(1, 255.0), c(2, 255.0), c(3, 1.0)];
        }
    }
    let pal = |k: i32| &rows[(k.clamp(c0, c1) - c0) as usize];
    let bristles = (s / 3.0).round().max(3.0);
    let tw = tr.width() as usize;
    let mut out = vec![0u8; tr.area() as usize * 4];
    out.par_chunks_mut(tw * 4).enumerate().for_each(|(i, line)| {
        let y = tr.y0 + i as u32;
        for x in tr.x0..tr.x1 {
            let (u, v) = f.uv(x as f32 + 0.5, y as f32 + 0.5);
            let mut k = (v / s).floor() as i32;
            let (mut top, mut bottom) = (edge(s, k, u), edge(s, k + 1, u));
            if v < top {
                k -= 1;
                (top, bottom) = (edge(s, k, u), top);
            } else if v >= bottom {
                k += 1;
                (top, bottom) = (bottom, edge(s, k + 1, u));
            }
            let (dt, db) = (v - top, bottom - v);
            let mut c = pal(k).at(l, u);
            if dt < 0.5 {
                c = mix(pal(k - 1).at(l, u), c, 0.5 + dt);
            } else if db < 0.5 {
                c = mix(pal(k + 1).at(l, u), c, 0.5 + db);
            }
            let vr = (dt / (bottom - top)).clamp(0.0, 1.0);
            let o = (x - tr.x0) as usize * 4;
            line[o..o + 4].copy_from_slice(&finish(c, texture(s, bristles, k, vr, u, dt.min(db))));
        }
    });
    out
}

/// 框选的涂抹：框里全部涂满，框外一圈不规则的边
pub struct Ragged {
    r: IRect,
    s: f32,
}

impl Ragged {
    pub fn new(r: IRect, strength: u32) -> Self {
        Self { r, s: strength.max(1) as f32 }
    }

    /// 框外最多涂出去多远（像素）
    pub fn reach(strength: u32) -> u32 {
        (strength.max(1) as f32 * 0.9).ceil() as u32 + 2
    }

    /// y 这一行左、右两头的笔触伸出框外多长：每条笔触一头圆、长短不一，刷毛参差
    pub fn tips(&self, y: u32) -> (f32, f32) {
        let (s, py) = (self.s, y as f32 + 0.5);
        let tip = |x: u32, side: u32| {
            let (k, v) = band_at(s, x as f32, py);
            let len = s * (0.15 + 0.55 * rand(key(k, 6), side));
            let round = (1.0 - (2.0 * v - 1.0).powi(2)).max(0.0).sqrt();
            let hair = rand(hash(key(k, 7), side), (v * s / 1.5) as u32) * 0.12 * s;
            len * (0.25 + 0.75 * round) + hair
        };
        (tip(self.r.x0, 0), tip(self.r.x1, 1))
    }

    /// x 这一列上、下两边涂出框外多远：笔触边缘缓缓起伏
    fn margins(&self, x: u32) -> (f32, f32) {
        let (s, px) = (self.s, x as f32 + 0.5);
        let m = |side: i32| s * (0.06 + 0.22 * noise(key(side, 8), px / (s * 3.0)) + 0.08 * noise(key(side, 9), px / (s * 0.7)));
        (m(0), m(1))
    }

    /// 框外 (x, y) 的覆盖程度（0 ~ 255）；tips 是这一行的 [`Ragged::tips`]
    pub fn cov(&self, x: u32, y: u32, tips: (f32, f32)) -> u8 {
        let r = self.r;
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        // 离涂到的范围有多远（负数 = 在里面），按像素中心算，半个像素内抗锯齿
        let dx = (r.x0 as f32 - tips.0 - px).max(px - r.x1 as f32 - tips.1);
        let dy = if y < r.y0 || y >= r.y1 {
            let (top, bottom) = self.margins(x);
            (r.y0 as f32 - top - py).max(py - r.y1 as f32 - bottom)
        } else {
            f32::NEG_INFINITY
        };
        ((0.5 - dx.max(dy)).clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::super::stroke::{Effect, Stroke};
    use super::super::{TILE, tile_rect};
    use super::*;

    fn noise_img(w: u32, h: u32) -> RgbaImage {
        let mut s = 777u32;
        RgbaImage::from_fn(w, h, |_, _| {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            let v = s.to_le_bytes();
            image::Rgba([v[1], v[2], v[3], 255])
        })
    }

    #[test]
    fn edges_never_cross_and_bands_are_found() {
        for s in [8.0, 27.0, 128.0] {
            for k in -2..40 {
                for x in (0..3000).step_by(7) {
                    let x = x as f32;
                    assert!(edge(s, k + 1, x) - edge(s, k, x) > s * 0.6, "s {s} k {k} x {x}");
                    let y = edge(s, k, x) + 0.3;
                    assert_eq!(band_at(s, x, y).0, k);
                }
            }
        }
    }

    #[test]
    fn tiles_match_one_whole_image_tile() {
        let img = noise_img(700, 530);
        let whole = tile(&Saved::default(), &img, 27, IRect::full(700, 530));
        for ty in 0..530u32.div_ceil(TILE) {
            for tx in 0..700u32.div_ceil(TILE) {
                let tr = tile_rect(tx, ty, 700, 530);
                let part = tile(&Saved::default(), &img, 27, tr);
                for y in tr.y0..tr.y1 {
                    let a = ((y - tr.y0) * tr.width()) as usize * 4;
                    let b = (y as usize * 700 + tr.x0 as usize) * 4;
                    assert_eq!(part[a..a + tr.width() as usize * 4], whole[b..b + tr.width() as usize * 4], "tile ({tx}, {ty}) row {y}");
                }
            }
        }
    }

    /// 斜着涂抹：分块算与整张一次算逐字节相同（几种走向）；取色的方格转过了，分块时要读的范围不能漏
    #[test]
    fn angled_tiles_match_one_whole_image_tile() {
        let img = noise_img(700, 530);
        for angle in [0.35f32, -0.9, std::f32::consts::FRAC_PI_4, 1.4] {
            let whole = tile_angled(&Saved::default(), &img, 27, IRect::full(700, 530), angle);
            for ty in 0..530u32.div_ceil(TILE) {
                for tx in 0..700u32.div_ceil(TILE) {
                    let tr = tile_rect(tx, ty, 700, 530);
                    let part = tile_angled(&Saved::default(), &img, 27, tr, angle);
                    for y in tr.y0..tr.y1 {
                        let a = ((y - tr.y0) * tr.width()) as usize * 4;
                        let b = (y as usize * 700 + tr.x0 as usize) * 4;
                        assert_eq!(part[a..a + tr.width() as usize * 4], whole[b..b + tr.width() as usize * 4], "angle {angle} tile ({tx}, {ty}) row {y}");
                    }
                }
            }
        }
    }

    /// 斜着涂抹也只留下每段的平均色：同一个（转过的）方格里的像素随便换位置，结果完全一样
    #[test]
    fn angled_only_cell_averages_survive() {
        let (w, h, s, angle) = (400u32, 300u32, 27u32, 0.6f32);
        let img = noise_img(w, h);
        let f = Frame::new(w, h, s as f32, angle);
        let target = f.cell(200, 150);
        let members: Vec<(u32, u32)> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| f.cell(x, y) == target).collect();
        assert!(members.len() > 200, "{}", members.len());
        let mut moved = img.clone();
        for i in 0..members.len() / 2 {
            let (a, b) = (members[i], members[members.len() - 1 - i]);
            let (pa, pb) = (*moved.get_pixel(a.0, a.1), *moved.get_pixel(b.0, b.1));
            moved.put_pixel(a.0, a.1, pb);
            moved.put_pixel(b.0, b.1, pa);
        }
        assert_ne!(img, moved);
        let full = IRect::full(w, h);
        assert_eq!(tile_angled(&Saved::default(), &img, s, full, angle), tile_angled(&Saved::default(), &moved, s, full, angle));
    }

    /// 45° 涂抹：顺着右下几乎没有起伏，垂直于它（右上）只剩刷毛纹理和笔触交界
    #[test]
    fn angled_smear_runs_along_the_angle() {
        let img = noise_img(512, 512);
        let out = RgbaImage::from_raw(512, 512, tile_angled(&Saved::default(), &img, 16, IRect::full(512, 512), std::f32::consts::FRAC_PI_4)).unwrap();
        let grad = |m: &RgbaImage, dy: i32| -> u64 {
            let mut sum = 0;
            for y in 1..511u32 {
                for x in 0..511u32 {
                    let y2 = (y as i32 + dy) as u32;
                    sum += u64::from(m.get_pixel(x, y).0[1].abs_diff(m.get_pixel(x + 1, y2).0[1]));
                }
            }
            sum
        };
        let (along, across, orig) = (grad(&out, 1), grad(&out, -1), grad(&img, 1));
        eprintln!("SMEAR along {along} across {across} original {orig}");
        assert!(along * 3 < across, "along {along} across {across}");
        assert!(along * 20 < orig, "along {along} original {orig}");
    }

    #[test]
    fn only_segment_averages_survive() {
        // 同一段里的像素随便换位置，结果完全一样：原图细节不会从涂抹里漏出来
        let (w, h, s) = (400, 300, 27u32);
        let img = noise_img(w, h);
        let mut moved = img.clone();
        let k = 4;
        let row = Row { shift: shift(s as f32, k), first: 0, last: 0, j0: 0, cells: Vec::new() };
        let (first, last) = seg_range(s as f32, k, w);
        let row = Row { first, last, ..row };
        let (x0, x1) = row.cols(s as f32, first + 2, w);
        let (y0, y1) = band_rows(s, k, band_last(s, h), h);
        for i in 0..(x1 - x0) {
            let (a, b) = ((x0 + i, y0 + i % (y1 - y0)), (x1 - 1 - i, y1 - 1 - (i * 7) % (y1 - y0)));
            let (pa, pb) = (*moved.get_pixel(a.0, a.1), *moved.get_pixel(b.0, b.1));
            moved.put_pixel(a.0, a.1, pb);
            moved.put_pixel(b.0, b.1, pa);
        }
        assert_ne!(img, moved);
        let full = IRect::full(w, h);
        assert_eq!(tile(&Saved::default(), &img, s, full), tile(&Saved::default(), &moved, s, full));
    }

    #[test]
    fn smear_destroys_fine_detail() {
        let img = noise_img(512, 512);
        let out = RgbaImage::from_raw(512, 512, tile(&Saved::default(), &img, 16, IRect::full(512, 512))).unwrap();
        let grad = |m: &RgbaImage, dx: u32, dy: u32| -> u64 {
            let mut sum = 0;
            for y in 0..512 - dy {
                for x in 0..512 - dx {
                    sum += u64::from(m.get_pixel(x, y).0[1].abs_diff(m.get_pixel(x + dx, y + dy).0[1]));
                }
            }
            sum
        };
        // 横向几乎没有起伏，竖向只剩刷毛纹理和笔触交界
        assert!(grad(&out, 1, 0) * 30 < grad(&img, 1, 0), "{} vs {}", grad(&out, 1, 0), grad(&img, 1, 0));
        assert!(grad(&out, 0, 1) * 6 < grad(&img, 0, 1), "{} vs {}", grad(&out, 0, 1), grad(&img, 0, 1));
    }

    #[test]
    fn rect_fills_inside_and_leaves_a_ragged_edge() {
        let (w, h, s) = (480, 360, 27u32);
        let orig = noise_img(w, h);
        let field = tile(&Saved::default(), &orig, s, IRect::full(w, h));
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [field[i], field[i + 1], field[i + 2], field[i + 3]]
        };
        let r = IRect::new(120, 100, 360, 200);
        let mut img = orig.clone();
        let mut st = Stroke::rect(Effect::Smear(s));
        st.set_rect(&mut img, r);
        let reach = Ragged::reach(s);
        for y in 0..h {
            for x in 0..w {
                let p = img.get_pixel(x, y).0;
                if r.contains(x, y) {
                    assert_eq!(p, at(x, y), "inside ({x}, {y})");
                } else if x + reach < r.x0 || x >= r.x1 + reach || y + reach < r.y0 || y >= r.y1 + reach {
                    assert_eq!(p, orig.get_pixel(x, y).0, "far outside ({x}, {y})");
                }
            }
        }
        // 左边：每行涂出去的长短不一样，但都不超过 reach
        let spill: Vec<u32> = (r.y0..r.y1).map(|y| (0..r.x0).rev().take_while(|&x| img.get_pixel(x, y) != orig.get_pixel(x, y)).count() as u32).collect();
        assert!(spill.iter().all(|&d| d < reach));
        assert!(spill.iter().max().unwrap() - spill.iter().min().unwrap() >= s / 3, "{spill:?}");
        // 上边也涂出去一点，起伏不平
        let top: Vec<u32> = (r.x0..r.x1).map(|x| (0..r.y0).rev().take_while(|&y| img.get_pixel(x, y) != orig.get_pixel(x, y)).count() as u32).collect();
        assert!(top.iter().all(|&d| d >= 1 && d < reach), "{top:?}");
        assert!(top.iter().max().unwrap() > top.iter().min().unwrap());
    }

    #[test]
    fn dragging_the_rect_matches_drawing_it_fresh_and_cancels() {
        let (w, h, s) = (480, 360, 20u32);
        let orig = noise_img(w, h);
        let mut fresh = orig.clone();
        let mut a = Stroke::rect(Effect::Smear(s));
        a.set_rect(&mut fresh, IRect::new(150, 120, 300, 260));
        let mut dragged = orig.clone();
        let mut b = Stroke::rect(Effect::Smear(s));
        for r in [IRect::new(150, 120, 170, 130), IRect::new(150, 120, 420, 330), IRect::new(60, 40, 200, 150), IRect::new(150, 120, 300, 260)] {
            b.set_rect(&mut dragged, r);
        }
        assert_eq!(fresh, dragged);
        b.cancel(&mut dragged);
        assert_eq!(dragged, orig);
    }

    #[test]
    fn brush_and_rect_agree_inside() {
        let (w, h, s) = (480, 360, 27u32);
        let orig = noise_img(w, h);
        let mut a = orig.clone();
        Stroke::rect(Effect::Smear(s)).set_rect(&mut a, IRect::new(100, 100, 380, 260));
        let mut b = orig.clone();
        let mut st = Stroke::brush(Effect::Smear(s), 120.0, super::super::stroke::Tip::Round);
        st.line_to(&mut b, 150.0, 180.0);
        st.line_to(&mut b, 330.0, 180.0);
        for y in 140..220 {
            for x in 150..330 {
                assert_eq!(a.get_pixel(x, y), b.get_pixel(x, y), "({x}, {y})");
            }
        }
    }
}
