//! 实时预览的一笔（画笔方式）或一个框（框选方式）：马赛克、模糊、涂抹、刷子、铅笔、橡皮擦。
//!
//! 都以落笔前的图为准计算：同一笔反复涂同一处结果不变（覆盖程度取最大值，不累加）；新的一笔可以
//! 叠在旧的效果上。每帧只重算这一帧涂到的范围；马赛克 / 模糊 / 涂抹的结果按 256×256 分块缓存。
//! 马赛克的方块、涂抹的笔触从整张图的左上角起划分，不管怎么涂、画笔与框选混着用，位置都一致。

use std::collections::HashMap;

use image::RgbaImage;
use rayon::prelude::*;

use super::history::Saved;
use super::{IRect, TILE, tile_rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// 方块边长（像素）
    Mosaic(u32),
    /// 模糊程度（像素，约等于高斯模糊的 2σ）
    Blur(u32),
    /// 涂抹的笔触宽度（像素）
    Smear(u32),
    /// 颜色（RGBA，A = 不透明度）
    Paint([u8; 4]),
    /// 擦成透明
    Erase,
}

impl Effect {
    pub fn is_redact(&self) -> bool {
        matches!(self, Effect::Mosaic(_) | Effect::Blur(_) | Effect::Smear(_))
    }
}

/// 打码的默认值（PRD 6.8.1）：短边 S → (方块, 模糊程度, 涂抹笔触宽度, 圆圈直径)
pub fn redact_defaults(w: u32, h: u32) -> (u32, u32, u32, u32) {
    let s = w.min(h);
    ((s / 80).max(8), (s / 60).max(8), (s / 40).max(12), (s / 20).max(24))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tip {
    /// 圆头、抗锯齿
    Round,
    /// 硬边（铅笔）
    Hard,
}

enum Shape {
    /// 画笔：每个像素的覆盖程度（按块存）
    Brush { radius: f32, tip: Tip, mask: HashMap<(u32, u32), Vec<u8>>, last: Option<(f32, f32)> },
    /// 框选：框里全部覆盖（涂抹时框外还有一圈不规则的边）
    Rect(IRect),
}

pub struct Stroke {
    pub effect: Effect,
    shape: Shape,
    saved: Saved,
    /// 马赛克 / 模糊 / 涂抹的结果（按块）
    cache: HashMap<(u32, u32), Vec<u8>>,
    /// 涂到过的范围
    pub bounds: IRect,
    /// 涂抹的走向（弧度，0 = 横着）：画笔拖出半个笔头宽后按这一段的方向定下来，之后不变；框选总是横着
    angle: f32,
    /// 画笔落笔的地方（还没定下走向时）
    aim_from: Option<(f32, f32)>,
    /// 走向已经定下
    aimed: bool,
}

/// 拖动方向 (dx, dy) → 涂抹的走向：来回一样（折到 −90° ~ 90°），离水平不到 4° 的算横着（与框选一样）
fn smear_angle(dx: f32, dy: f32) -> f32 {
    let mut a = dy.atan2(dx);
    if a > std::f32::consts::FRAC_PI_2 {
        a -= std::f32::consts::PI;
    } else if a <= -std::f32::consts::FRAC_PI_2 {
        a += std::f32::consts::PI;
    }
    if a.abs() < 4f32.to_radians() { 0.0 } else { a }
}

impl Stroke {
    /// 画笔：diameter 是圆圈直径（图片像素）
    pub fn brush(effect: Effect, diameter: f32, tip: Tip) -> Self {
        Self::new(effect, Shape::Brush { radius: (diameter / 2.0).max(0.5), tip, mask: HashMap::new(), last: None })
    }

    pub fn rect(effect: Effect) -> Self {
        Self::new(effect, Shape::Rect(IRect::EMPTY))
    }

    fn new(effect: Effect, shape: Shape) -> Self {
        Self { effect, shape, saved: Saved::default(), cache: HashMap::new(), bounds: IRect::EMPTY, angle: 0.0, aim_from: None, aimed: false }
    }

    /// 画笔从上一个点涂到 (x, y)（图片像素坐标）；返回这次改到的范围
    pub fn line_to(&mut self, img: &mut RgbaImage, x: f32, y: f32) -> IRect {
        let (w, h) = img.dimensions();
        let Shape::Brush { radius, tip, mask, last } = &mut self.shape else { return IRect::EMPTY };
        let (radius, tip) = (*radius, *tip);
        let (x, y) = snap(x, y, radius, tip);
        let (px, py) = last.unwrap_or((x, y));
        *last = Some((x, y));
        let r = radius + 1.0;
        let bbox = IRect::around(px.min(x) - r, py.min(y) - r, px.max(x) + r, py.max(y) + r, w, h);
        if bbox.is_empty() {
            return IRect::EMPTY;
        }
        stamp(mask, bbox, (px, py), (x, y), radius, tip, w, h);
        self.bounds = self.bounds.union(&bbox);
        // 涂抹：从落笔处拖出半个笔头宽，按这一段的方向定下走向；之前横着涂的那一小块按新走向重算
        if let Effect::Smear(_) = self.effect
            && !self.aimed
        {
            let from = *self.aim_from.get_or_insert((px, py));
            let (dx, dy) = (x - from.0, y - from.1);
            if dx.hypot(dy) >= radius.max(3.0) {
                self.aim_from = None;
                self.aimed = true;
                let angle = smear_angle(dx, dy);
                if angle != self.angle {
                    self.angle = angle;
                    self.cache.clear();
                    let all = self.bounds;
                    self.compose(img, all);
                    return all;
                }
            }
        }
        self.compose(img, bbox);
        bbox
    }

    /// 框选：框改成 r；返回要重画的几块（新旧两个框不重合的部分；涂抹时还有两个框外那圈不规则的边）
    pub fn set_rect(&mut self, img: &mut RgbaImage, r: IRect) -> Vec<IRect> {
        let full = IRect::full(img.width(), img.height());
        let r = r.intersect(&full);
        let Shape::Rect(old) = self.shape else { return Vec::new() };
        if old == r {
            return Vec::new();
        }
        // 涂到的范围：涂抹时框外还有一圈；两个框都盖住的部分不用重画
        let reach = if let Effect::Smear(s) = self.effect { super::smear::Ragged::reach(s) } else { 0 };
        let (was, now) = (grow(old, reach, full), grow(r, reach, full));
        let keep = old.intersect(&r);
        let mut parts = was.subtract(&keep);
        parts.extend(now.subtract(&was));
        self.shape = Shape::Rect(r);
        for p in &parts {
            self.compose(img, *p);
        }
        self.bounds = self.bounds.union(&now);
        parts
    }

    /// 完成：返回改之前的块（交给撤销历史）
    pub fn finish(self) -> Saved {
        self.saved
    }

    /// 取消这一笔 / 这一框：恢复原样；返回涉及的范围
    pub fn cancel(self, img: &mut RgbaImage) -> IRect {
        self.saved.restore(img)
    }

    /// 重算 rect 里的像素 = 落笔前的像素按覆盖程度混合效果
    fn compose(&mut self, img: &mut RgbaImage, rect: IRect) {
        let (w, h) = img.dimensions();
        self.saved.touch(img, rect);
        if self.effect.is_redact() {
            // 这里用到的块都已存下，没存的块就是落笔前的样子，可以直接读
            let missing: Vec<(u32, u32)> = rect.tiles().filter(|t| !self.cache.contains_key(t)).collect();
            let (saved, effect, angle) = (&self.saved, self.effect, self.angle);
            let img_ref: &RgbaImage = img;
            let done: Vec<_> = missing.into_par_iter().map(|t| (t, effect_tile(saved, img_ref, effect, angle, tile_rect(t.0, t.1, w, h)))).collect();
            self.cache.extend(done);
        }
        let (saved, cache, shape, effect) = (&self.saved, &self.cache, &self.shape, self.effect);
        // 框选涂抹：框外那圈不规则的边
        let ragged = match (shape, effect) {
            (Shape::Rect(r), Effect::Smear(s)) if !r.is_empty() => Some(super::smear::Ragged::new(*r, s)),
            _ => None,
        };
        let stride = w as usize * 4;
        let raw: &mut [u8] = img;
        raw.par_chunks_mut(stride).enumerate().skip(rect.y0 as usize).take(rect.height() as usize).for_each(|(y, row)| {
            let y = y as u32;
            let tips = ragged.as_ref().map(|g| g.tips(y));
            let mut x = rect.x0;
            while x < rect.x1 {
                let t = (x / TILE, y / TILE);
                let tr = tile_rect(t.0, t.1, w, h);
                let end = tr.x1.min(rect.x1);
                let orig = &saved.tiles[&t];
                let row_off = ((y - tr.y0) * tr.width()) as usize;
                let mask = match shape {
                    Shape::Brush { mask, .. } => mask.get(&t),
                    Shape::Rect(_) => None,
                };
                let fx = cache.get(&t);
                for xx in x..end {
                    let li = row_off + (xx - tr.x0) as usize;
                    let cov = match shape {
                        Shape::Brush { .. } => mask.map_or(0, |m| m[li]),
                        Shape::Rect(r) => {
                            if r.contains(xx, y) {
                                255
                            } else if let (Some(g), Some(t)) = (&ragged, tips) {
                                g.cov(xx, y, t)
                            } else {
                                0
                            }
                        }
                    };
                    let o = [orig[li * 4], orig[li * 4 + 1], orig[li * 4 + 2], orig[li * 4 + 3]];
                    let out = if cov == 0 {
                        o
                    } else {
                        match effect {
                            Effect::Mosaic(_) | Effect::Blur(_) | Effect::Smear(_) => {
                                let f = fx.expect("effect tile");
                                blend(o, [f[li * 4], f[li * 4 + 1], f[li * 4 + 2], f[li * 4 + 3]], cov)
                            }
                            Effect::Paint(c) => blend(o, [c[0], c[1], c[2], 255], mul(cov, c[3])),
                            Effect::Erase => blend(o, [0, 0, 0, 0], cov),
                        }
                    };
                    row[xx as usize * 4..xx as usize * 4 + 4].copy_from_slice(&out);
                }
                x = end;
            }
        });
    }
}

fn mul(a: u8, b: u8) -> u8 {
    ((u32::from(a) * u32::from(b) + 127) / 255) as u8
}

/// r 四周各扩大 d 像素，裁到 full 之内（空矩形还是空的）
fn grow(r: IRect, d: u32, full: IRect) -> IRect {
    if r.is_empty() {
        return IRect::EMPTY;
    }
    IRect::new(r.x0.saturating_sub(d), r.y0.saturating_sub(d), r.x1 + d, r.y1 + d).intersect(&full)
}

/// 铅笔（硬边）对齐到像素：奇数宽对齐像素中心，偶数宽对齐像素角，笔迹是整齐的方块
fn snap(x: f32, y: f32, radius: f32, tip: Tip) -> (f32, f32) {
    if tip == Tip::Round {
        return (x, y);
    }
    let d = (radius * 2.0).round() as i64;
    if d % 2 == 1 { (x.floor() + 0.5, y.floor() + 0.5) } else { (x.round(), y.round()) }
}

/// 把线段 a → b 涂进覆盖程度蒙版（取最大值）
#[allow(clippy::too_many_arguments)]
fn stamp(mask: &mut HashMap<(u32, u32), Vec<u8>>, bbox: IRect, a: (f32, f32), b: (f32, f32), radius: f32, tip: Tip, w: u32, h: u32) {
    for t in bbox.tiles() {
        mask.entry(t).or_insert_with(|| vec![0; tile_rect(t.0, t.1, w, h).area() as usize]);
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let tiles: Vec<((u32, u32), &mut Vec<u8>)> = mask.iter_mut().filter(|(t, _)| tile_rect(t.0, t.1, w, h).intersects(&bbox)).map(|(t, m)| (*t, m)).collect();
    tiles.into_par_iter().for_each(|(t, m)| {
        let tr = tile_rect(t.0, t.1, w, h);
        let part = tr.intersect(&bbox);
        for y in part.y0..part.y1 {
            for x in part.x0..part.x1 {
                // 像素中心到线段的距离
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let k = if len2 > 0.0 { (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let (qx, qy) = (a.0 + k * dx - px, a.1 + k * dy - py);
                let d = (qx * qx + qy * qy).sqrt();
                let cov = match tip {
                    Tip::Round => ((radius + 0.5 - d).clamp(0.0, 1.0) * 255.0).round() as u8,
                    Tip::Hard => {
                        if d < radius {
                            255
                        } else {
                            0
                        }
                    }
                };
                let i = ((y - tr.y0) * tr.width() + (x - tr.x0)) as usize;
                if cov > m[i] {
                    m[i] = cov;
                }
            }
        }
    });
}

/// 预乘透明度后按 t（0 ~ 255）混合两个像素：透明边缘不发灰
pub fn blend(a: [u8; 4], b: [u8; 4], t: u8) -> [u8; 4] {
    match t {
        0 => return a,
        255 => return b,
        _ => {}
    }
    let (t, s) = (u32::from(t), 255 - u32::from(t));
    if a[3] == 255 && b[3] == 255 {
        let m = |i: usize| ((u32::from(a[i]) * s + u32::from(b[i]) * t + 127) / 255) as u8;
        return [m(0), m(1), m(2), 255];
    }
    let (aa, ba) = (u32::from(a[3]), u32::from(b[3]));
    let alpha = aa * s + ba * t; // 0 ~ 255²
    if alpha == 0 {
        return [0, 0, 0, 0];
    }
    let c = |i: usize| ((u32::from(a[i]) * aa * s + u32::from(b[i]) * ba * t + alpha / 2) / alpha).min(255) as u8;
    [c(0), c(1), c(2), ((alpha + 127) / 255) as u8]
}

/// 一块的马赛克 / 模糊 / 涂抹结果（从落笔前的图计算）；angle 是涂抹的走向
fn effect_tile(saved: &Saved, img: &RgbaImage, effect: Effect, angle: f32, tr: IRect) -> Vec<u8> {
    match effect {
        Effect::Mosaic(block) => mosaic_tile(saved, img, block.max(1), tr),
        Effect::Blur(strength) => blur_tile(saved, img, strength, tr),
        Effect::Smear(strength) => super::smear::tile_angled(saved, img, strength, tr, angle),
        _ => Vec::new(),
    }
}

/// 马赛克：每个方块取平均色（按透明度加权）；方块从整张图的左上角起划分
fn mosaic_tile(saved: &Saved, img: &RgbaImage, block: u32, tr: IRect) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let src = IRect::new(tr.x0 / block * block, tr.y0 / block * block, tr.x1.div_ceil(block) * block, tr.y1.div_ceil(block) * block)
        .intersect(&IRect::full(w, h));
    let data = saved.region(img, src);
    let sw = src.width() as usize;
    let mut out = vec![0u8; tr.area() as usize * 4];
    let mut by = src.y0;
    while by < src.y1 {
        let by1 = (by + block).min(src.y1);
        let mut bx = src.x0;
        while bx < src.x1 {
            let bx1 = (bx + block).min(src.x1);
            let mut sum = [0u64; 4];
            for y in by..by1 {
                for x in bx..bx1 {
                    let i = ((y - src.y0) as usize * sw + (x - src.x0) as usize) * 4;
                    let a = u64::from(data[i + 3]);
                    for c in 0..3 {
                        sum[c] += u64::from(data[i + c]) * a;
                    }
                    sum[3] += a;
                }
            }
            let n = u64::from(bx1 - bx) * u64::from(by1 - by);
            let px = if sum[3] == 0 {
                [0, 0, 0, 0]
            } else {
                let c = |i: usize| ((sum[i] + sum[3] / 2) / sum[3]).min(255) as u8;
                [c(0), c(1), c(2), ((sum[3] + n / 2) / n) as u8]
            };
            // 写进这一块里与方块重合的部分
            let part = IRect::new(bx, by, bx1, by1).intersect(&tr);
            for y in part.y0..part.y1 {
                for x in part.x0..part.x1 {
                    let o = (((y - tr.y0) * tr.width() + (x - tr.x0)) * 4) as usize;
                    out[o..o + 4].copy_from_slice(&px);
                }
            }
            bx = bx1;
        }
        by = by1;
    }
    out
}

/// 三次方框模糊近似高斯模糊（σ = 程度 / 2）的半径
fn box_radii(strength: u32) -> [usize; 3] {
    let sigma = (strength.max(1) as f32 / 2.0).max(0.5);
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - 3.0 * (wl * wl) as f32 - 12.0 * wl as f32 - 9.0) / (-4.0 * wl as f32 - 4.0)).round() as i32;
    let r = |i: i32| (((if i < m { wl } else { wu }) - 1) / 2).max(0) as usize;
    [r(0), r(1), r(2)]
}

/// 模糊：在预乘透明度的像素上做三次方框模糊；四周多取一圈，结果与整张图一起模糊完全一致
fn blur_tile(saved: &Saved, img: &RgbaImage, strength: u32, tr: IRect) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let radii = box_radii(strength);
    let pad: u32 = radii.iter().sum::<usize>() as u32;
    let src = IRect::new(tr.x0.saturating_sub(pad), tr.y0.saturating_sub(pad), (tr.x1 + pad).min(w), (tr.y1 + pad).min(h));
    let data = saved.region(img, src);
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let mut buf: Vec<u32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = u32::from(p[3]);
            [u32::from(p[0]) * a, u32::from(p[1]) * a, u32::from(p[2]) * a, a * 255]
        })
        .collect();
    for &r in &radii {
        box_rows(&mut buf, sw, sh, r);
    }
    let mut t = transpose(&buf, sw, sh);
    for &r in &radii {
        box_rows(&mut t, sh, sw, r);
    }
    let buf = transpose(&t, sh, sw);
    let mut out = vec![0u8; tr.area() as usize * 4];
    for y in tr.y0..tr.y1 {
        for x in tr.x0..tr.x1 {
            let i = ((y - src.y0) as usize * sw + (x - src.x0) as usize) * 4;
            let a = buf[i + 3];
            let o = (((y - tr.y0) * tr.width() + (x - tr.x0)) * 4) as usize;
            if a == 0 {
                out[o..o + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            for c in 0..3 {
                out[o + c] = ((buf[i + c] * 255 + a / 2) / a).min(255) as u8;
            }
            out[o + 3] = ((a + 127) / 255) as u8;
        }
    }
    out
}

/// 每行做一次半径 r 的方框平均（边缘按最近的像素延伸）
fn box_rows(buf: &mut [u32], w: usize, h: usize, r: usize) {
    if r == 0 || w == 0 {
        return;
    }
    let n = (2 * r + 1) as u32;
    buf.par_chunks_mut(w * 4).take(h).for_each(|row| {
        let src = row.to_vec();
        let at = |i: isize, c: usize| src[(i.clamp(0, w as isize - 1) as usize) * 4 + c];
        for c in 0..4 {
            let mut sum: u32 = (-(r as isize)..=r as isize).map(|i| at(i, c)).sum();
            for x in 0..w {
                row[x * 4 + c] = (sum + n / 2) / n;
                sum += at(x as isize + r as isize + 1, c);
                sum -= at(x as isize - r as isize, c);
            }
        }
    });
}

fn transpose(buf: &[u32], w: usize, h: usize) -> Vec<u32> {
    let mut out = vec![0u32; buf.len()];
    for y in 0..h {
        for x in 0..w {
            let (s, d) = ((y * w + x) * 4, (x * h + y) * 4);
            out[d..d + 4].copy_from_slice(&buf[s..s + 4]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(w: u32, h: u32) -> RgbaImage {
        let mut s = 12345u32;
        RgbaImage::from_fn(w, h, |_, _| {
            s = s.wrapping_mul(1103515245).wrapping_add(12345);
            let v = s.to_le_bytes();
            image::Rgba([v[0], v[1], v[2], 255])
        })
    }

    #[test]
    fn defaults_follow_prd_examples() {
        assert_eq!(redact_defaults(1920, 1080), (13, 18, 27, 54));
        assert_eq!(redact_defaults(4000, 3000), (37, 50, 75, 150));
        assert_eq!(redact_defaults(100, 50), (8, 8, 12, 24));
    }

    #[test]
    fn mosaic_blocks_align_to_image_grid() {
        let orig = noise(700, 500);
        // 框选
        let mut a = orig.clone();
        let mut s = Stroke::rect(Effect::Mosaic(13));
        s.set_rect(&mut a, IRect::new(100, 100, 400, 300));
        // 画笔涂同一块的中间，再框一次旁边：方块位置、颜色都与框选一致
        let mut b = orig.clone();
        let mut s2 = Stroke::brush(Effect::Mosaic(13), 120.0, Tip::Round);
        s2.line_to(&mut b, 150.0, 200.0);
        s2.line_to(&mut b, 350.0, 200.0);
        for y in 160..240 {
            for x in 150..350 {
                assert_eq!(a.get_pixel(x, y), b.get_pixel(x, y), "({x}, {y})");
            }
        }
        // 方块边界在 13 的倍数上
        assert_eq!(a.get_pixel(104, 104), a.get_pixel(116, 116));
        assert_ne!(a.get_pixel(116, 116), a.get_pixel(117, 117));
        // 框外不变
        assert_eq!(a.get_pixel(99, 99), orig.get_pixel(99, 99));
    }

    #[test]
    fn same_stroke_does_not_darken() {
        let orig = RgbaImage::from_pixel(300, 300, image::Rgba([255, 255, 255, 255]));
        let mut once = orig.clone();
        let mut s = Stroke::brush(Effect::Paint([255, 0, 0, 128]), 20.0, Tip::Round);
        s.line_to(&mut once, 50.0, 50.0);
        s.line_to(&mut once, 250.0, 50.0);
        let mut many = orig.clone();
        let mut s2 = Stroke::brush(Effect::Paint([255, 0, 0, 128]), 20.0, Tip::Round);
        for _ in 0..5 {
            s2.line_to(&mut many, 50.0, 50.0);
            s2.line_to(&mut many, 250.0, 50.0);
        }
        assert_eq!(once, many);
        // 新的一笔叠在旧的上面会更深
        let before = many.get_pixel(150, 50).0;
        let mut s3 = Stroke::brush(Effect::Paint([255, 0, 0, 128]), 20.0, Tip::Round);
        s3.line_to(&mut many, 150.0, 50.0);
        assert!(many.get_pixel(150, 50).0[1] < before[1]);
    }

    /// 斜着拖的涂抹画笔：拖出半个笔头宽后按这段的方向定下走向，开头先横着涂的那一小块也按新走向重算；
    /// 笔道中间就是整张按这个走向涂抹的结果。横着拖（差不到 4°）与框选一样是横的；来回方向一样
    #[test]
    fn smear_brush_follows_the_drag_direction() {
        let (w, h, s) = (480u32, 360u32, 20u32);
        let orig = noise(w, h);
        let mut img = orig.clone();
        let mut st = Stroke::brush(Effect::Smear(s), 60.0, Tip::Round);
        for i in 0..=20 {
            let t = i as f32 * 10.0;
            st.line_to(&mut img, 100.0 + t * 0.866, 100.0 + t * 0.5);
        }
        assert!(st.aimed && (st.angle - 30f32.to_radians()).abs() < 1e-3, "{}", st.angle.to_degrees());
        let field = super::super::smear::tile_angled(&Saved::default(), &orig, s, IRect::full(w, h), st.angle);
        for i in 0..=20 {
            let t = i as f32 * 10.0;
            let (x, y) = ((100.0 + t * 0.866) as u32, (100.0 + t * 0.5) as u32);
            let k = ((y * w + x) * 4) as usize;
            assert_eq!(img.get_pixel(x, y).0, [field[k], field[k + 1], field[k + 2], field[k + 3]], "({x}, {y})");
        }
        assert_eq!(smear_angle(100.0, 3.0), 0.0);
        assert!((smear_angle(-86.6, -50.0) - smear_angle(86.6, 50.0)).abs() < 1e-5);
        assert!((smear_angle(0.0, -40.0) - smear_angle(0.0, 40.0)).abs() < 1e-5);
    }

    #[test]
    fn tiled_blur_matches_whole_image_blur() {
        let orig = noise(600, 400);
        let mut img = orig.clone();
        let mut s = Stroke::rect(Effect::Blur(20));
        s.set_rect(&mut img, IRect::full(600, 400));
        // 整张一起算（一个大块）
        let whole = blur_tile(&Saved::default(), &orig, 20, IRect::full(600, 400));
        assert_eq!(img.as_raw(), &whole);
        // 模糊后相邻像素差别变小
        let d = |m: &RgbaImage| (0..599).map(|x| (i32::from(m.get_pixel(x, 200).0[0]) - i32::from(m.get_pixel(x + 1, 200).0[0])).abs()).sum::<i32>();
        assert!(d(&img) * 5 < d(&orig));
    }

    #[test]
    fn rect_preview_moves_and_cancels() {
        let orig = noise(300, 300);
        let mut img = orig.clone();
        let mut s = Stroke::rect(Effect::Mosaic(10));
        s.set_rect(&mut img, IRect::new(0, 0, 200, 200));
        let parts = s.set_rect(&mut img, IRect::new(0, 0, 100, 100));
        assert!(!parts.is_empty());
        // 缩回去的部分恢复原样
        assert_eq!(img.get_pixel(150, 150), orig.get_pixel(150, 150));
        s.cancel(&mut img);
        assert_eq!(img, orig);
    }

    #[test]
    fn erase_and_pencil() {
        let mut img = RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 255]));
        let mut s = Stroke::brush(Effect::Erase, 10.0, Tip::Round);
        s.line_to(&mut img, 32.0, 32.0);
        assert_eq!(img.get_pixel(32, 32).0[3], 0);
        let mut p = Stroke::brush(Effect::Paint([0, 0, 0, 255]), 1.0, Tip::Hard);
        p.line_to(&mut img, 2.2, 2.7);
        p.line_to(&mut img, 12.9, 2.1);
        // 1 像素宽的横线
        assert!((2..=12).all(|x| img.get_pixel(x, 2).0 == [0, 0, 0, 255]));
        assert_eq!(img.get_pixel(5, 1).0, [10, 20, 30, 255]);
        assert_eq!(img.get_pixel(5, 3).0, [10, 20, 30, 255]);
    }

    #[test]
    fn blend_premultiplied() {
        assert_eq!(blend([255, 0, 0, 255], [0, 0, 255, 255], 255), [0, 0, 255, 255]);
        // 与全透明混合时颜色不变暗
        let half = blend([200, 100, 50, 255], [0, 0, 0, 0], 128);
        assert_eq!(&half[..3], &[200, 100, 50]);
        assert!((126..=128).contains(&half[3]));
    }
}
