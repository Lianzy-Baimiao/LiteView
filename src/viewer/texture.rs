//! 图片纹理：超过显卡单张纹理上限的图分块上传（块之间重叠 1 像素，线性插值不出接缝）；
//! 缩小显示时用逐级缩小一半的图（没有摩尔纹）；放大超过阈值改用最近邻，看清每个像素。
//! 每一级在第一次用到时才上传。切到预读的图时先显示缩略图，原图解完后换上。

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use egui::{Color32, ColorImage, Mesh, Painter, Pos2, Rect, TextureHandle, TextureOptions, pos2};
use image::RgbaImage;
use rayon::prelude::*;

use super::view::Transform;

pub struct Tile {
    tex: TextureHandle,
    /// 这一块显示的区域（这一级的像素坐标）
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    /// 纹理左上角比显示区域多出的重叠像素
    pad_x: u32,
    pad_y: u32,
}

struct Level {
    image: Arc<RgbaImage>,
    tiles: Vec<Tile>,
    nearest: bool,
}

/// 按准确显示尺寸缩小好的图（1:1 显示，没有摩尔纹）
struct Exact {
    zoom: f32,
    tex: TextureHandle,
    size: [u32; 2],
}

pub struct StillTextures {
    name: String,
    /// 原图尺寸（只有预读缩略图时 levels 还是空的）
    size: (u32, u32),
    levels: Vec<Level>,
    exact: Option<Exact>,
    pending: Option<(f32, Receiver<ColorImage>)>,
}

/// 缩放 zoom 下按准确尺寸缩小后的大小
pub fn exact_size(w: u32, h: u32, zoom: f32) -> (u32, u32) {
    (((w as f32 * zoom).round() as u32).max(1), ((h as f32 * zoom).round() as u32).max(1))
}

/// 准确尺寸缩小用哪一级做源：不比目标小的最小一级（sizes 从原图开始逐级变小）
pub fn exact_source(sizes: &[(u32, u32)], tw: u32, th: u32) -> usize {
    sizes.iter().rposition(|&(w, h)| w >= tw && h >= th).unwrap_or(0)
}

/// 准确尺寸缩小的结果能不能放进一张纹理
pub fn exact_fits(tw: u32, th: u32, max_side: u32) -> bool {
    tw <= max_side && th <= max_side && u64::from(tw) * u64::from(th) <= 40_000_000
}

impl StillTextures {
    pub fn new(name: &str, image: Arc<RgbaImage>, smaller: &[Arc<RgbaImage>]) -> Self {
        let mut t = Self { name: name.to_owned(), size: image.dimensions(), levels: Vec::new(), exact: None, pending: None };
        t.upgrade(image, smaller);
        t
    }

    /// 缩略图（已在显卡上）：在它算好的缩放比例下就是准确缩小的结果；要原图时 `upgrade`
    pub fn preview(name: &str, p: &crate::viewer::loader::Preview) -> Self {
        let exact = Exact { zoom: p.zoom, tex: p.tex.clone(), size: p.size };
        Self { name: name.to_owned(), size: (p.info.width, p.info.height), levels: Vec::new(), exact: Some(exact), pending: None }
    }

    /// 只有缩略图、而这个比例下它不够（放大、缩小到别的比例、旋转后适应窗口的比例变了）：要原图
    pub fn wants_source(&self, zoom: f32) -> bool {
        self.levels.is_empty() && !self.exact.as_ref().is_some_and(|e| (e.zoom - zoom).abs() < 1e-5)
    }

    /// 换成原图（缩放、位置、已有的准确缩小结果都保留）
    pub fn upgrade(&mut self, image: Arc<RgbaImage>, smaller: &[Arc<RgbaImage>]) {
        self.size = image.dimensions();
        self.levels = std::iter::once(image).chain(smaller.iter().cloned()).map(|image| Level { image, tiles: Vec::new(), nearest: false }).collect();
    }

    /// 缩小显示：缩放停下后在后台按准确尺寸高质量缩小，完成后 1:1 显示；没有时返回 false
    fn paint_exact(&mut self, ctx: &egui::Context, painter: &Painter, rect: Rect, zoom: f32, xf: Transform, settled: bool) -> bool {
        let (tw, th) = exact_size(self.size.0, self.size.1, zoom);
        let same = |z: f32| (z - zoom).abs() < 1e-5;
        if let Some((z, rx)) = &self.pending
            && let Ok(img) = rx.try_recv()
        {
            if same(*z) {
                let tex = ctx.load_texture(format!("{}-exact", self.name), img, TextureOptions::NEAREST);
                self.exact = Some(Exact { zoom, tex, size: [tw, th] });
            }
            self.pending = None;
        }
        let ready = self.exact.as_ref().is_some_and(|e| same(e.zoom));
        let max = ctx.input(|i| i.max_texture_side) as u32;
        if !ready && settled && exact_fits(tw, th, max) && self.pending.is_none() && !self.levels.is_empty() {
            let sizes: Vec<_> = self.levels.iter().map(|l| l.image.dimensions()).collect();
            let src = self.levels[exact_source(&sizes, tw, th)].image.clone();
            let (tx, rx) = channel();
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(resample(&src, tw, th));
                ctx2.request_repaint();
            });
            self.pending = Some((zoom, rx));
        }
        let Some(e) = self.exact.as_ref().filter(|e| same(e.zoom)) else { return false };
        let (dw, dh) = if xf.rotation % 2 == 1 { (e.size[1], e.size[0]) } else { (e.size[0], e.size[1]) };
        let r = Rect::from_min_size(rect.min, egui::vec2(dw as f32, dh as f32) / ctx.pixels_per_point());
        quad(painter, e.tex.id(), r, xf, [0.0, 0.0, 1.0, 1.0], Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        true
    }

    /// 用已经在显卡上的纹理画到 rect（打印预览用；不上传、不改画布用的缩放缓存）：准确缩小的结果，否则最小的一级
    pub fn paint_resident(&self, painter: &Painter, rect: Rect, xf: Transform) {
        if let Some(e) = &self.exact {
            quad(painter, e.tex.id(), rect, xf, [0.0, 0.0, 1.0, 1.0], Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        } else if let Some(lvl) = self.levels.iter().rev().find(|l| !l.tiles.is_empty()) {
            paint_tiles(painter, &lvl.tiles, lvl.image.dimensions(), rect, xf);
        }
    }

    /// 缩放 zoom（屏幕物理像素 / 图片像素）下用哪一级：缩小不超过一半
    fn pick(&self, zoom: f32) -> usize {
        let mut i = 0;
        let mut z = zoom;
        while z <= 0.5 && i + 1 < self.levels.len() {
            z *= 2.0;
            i += 1;
        }
        i
    }

    /// 画到 rect（旋转后图片在屏幕上的矩形）；settled = 缩放没有在动画中
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, ctx: &egui::Context, painter: &Painter, rect: Rect, zoom: f32, xf: Transform, nearest_above: f32, settled: bool) {
        if zoom < 0.999 && self.paint_exact(ctx, painter, rect, zoom, xf, settled) {
            return;
        }
        if self.levels.is_empty() {
            // 原图还在解码：先把缩略图拉伸到当前大小
            if let Some(e) = &self.exact {
                quad(painter, e.tex.id(), rect, xf, [0.0, 0.0, 1.0, 1.0], Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
            }
            return;
        }
        let i = self.pick(zoom);
        let nearest = i == 0 && zoom >= nearest_above;
        let max_side = ctx.input(|inp| inp.max_texture_side).max(2048);
        let lvl = &mut self.levels[i];
        if lvl.tiles.is_empty() || lvl.nearest != nearest {
            lvl.tiles = upload(ctx, &self.name, &lvl.image, max_side, nearest);
            lvl.nearest = nearest;
        }
        paint_tiles(painter, &lvl.tiles, lvl.image.dimensions(), rect, xf);
    }
}

/// 画分块纹理：size 是这一级图的尺寸，rect 是旋转后图片在屏幕上的矩形
pub fn paint_tiles(painter: &Painter, tiles: &[Tile], size: (u32, u32), rect: Rect, xf: Transform) {
    let (lw, lh) = (size.0 as f32, size.1 as f32);
    let clip = painter.clip_rect();
    for t in tiles {
        let (u0, v0, u1, v1) = (t.x0 as f32 / lw, t.y0 as f32 / lh, t.x1 as f32 / lw, t.y1 as f32 / lh);
        let a = xf.map(rect, u0, v0);
        let b = xf.map(rect, u1, v1);
        if !Rect::from_two_pos(a, b).intersects(clip) {
            continue;
        }
        let [tw, th] = t.tex.size();
        let uv = Rect::from_min_max(
            pos2(t.pad_x as f32 / tw as f32, t.pad_y as f32 / th as f32),
            pos2((t.pad_x + t.x1 - t.x0) as f32 / tw as f32, (t.pad_y + t.y1 - t.y0) as f32 / th as f32),
        );
        quad(painter, t.tex.id(), rect, xf, [u0, v0, u1, v1], uv);
    }
}

/// 图上 [x0, x1) × [y0, y1) 变了：只重新上传各块纹理里对应的部分
pub fn update_tiles(tiles: &mut [Tile], img: &RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, nearest: bool) {
    let opts = options(nearest);
    for t in tiles {
        let [tw, th] = t.tex.size();
        // 这块纹理覆盖的图片范围（含重叠像素）
        let (tx0, ty0) = (t.x0 - t.pad_x, t.y0 - t.pad_y);
        let (tx1, ty1) = (tx0 + tw as u32, ty0 + th as u32);
        let (ax, ay, bx, by) = (x0.max(tx0), y0.max(ty0), x1.min(tx1), y1.min(ty1));
        if ax >= bx || ay >= by {
            continue;
        }
        t.tex.set_partial([(ax - tx0) as usize, (ay - ty0) as usize], region(img, ax, ay, bx - ax, by - ay), opts);
    }
}

/// 画一块：原图归一化区域 src = [u0, v0, u1, v1] 经 xf 映射到屏幕
pub fn quad(painter: &Painter, tex: egui::TextureId, rect: Rect, xf: Transform, src: [f32; 4], uv: Rect) {
    let [u0, v0, u1, v1] = src;
    let mut mesh = Mesh::with_texture(tex);
    let corners = [(u0, v0, uv.left_top()), (u1, v0, uv.right_top()), (u1, v1, uv.right_bottom()), (u0, v1, uv.left_bottom())];
    for (u, v, t) in corners {
        mesh.colored_vertex(xf.map(rect, u, v), Color32::WHITE);
        let last = mesh.vertices.len() - 1;
        mesh.vertices[last].uv = t;
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

fn options(nearest: bool) -> TextureOptions {
    let mut o = TextureOptions::LINEAR;
    if nearest {
        o.magnification = egui::TextureFilter::Nearest;
    }
    o
}

pub fn upload(ctx: &egui::Context, name: &str, img: &RgbaImage, max_side: usize, nearest: bool) -> Vec<Tile> {
    let (w, h) = img.dimensions();
    let opts = options(nearest);
    let max = max_side as u32;
    if w <= max && h <= max {
        let tex = ctx.load_texture(name, region(img, 0, 0, w, h), opts);
        return vec![Tile { tex, x0: 0, y0: 0, x1: w, y1: h, pad_x: 0, pad_y: 0 }];
    }
    // 每块最多 max_side（含两边各 1 像素重叠）
    let stride = max - 2;
    let mut tiles = Vec::new();
    for y0 in (0..h).step_by(stride as usize) {
        for x0 in (0..w).step_by(stride as usize) {
            let (x1, y1) = ((x0 + stride).min(w), (y0 + stride).min(h));
            let (tx0, ty0) = (x0.saturating_sub(1), y0.saturating_sub(1));
            let (tx1, ty1) = ((x1 + 1).min(w), (y1 + 1).min(h));
            let tex = ctx.load_texture(format!("{name}#{x0},{y0}"), region(img, tx0, ty0, tx1 - tx0, ty1 - ty0), opts);
            tiles.push(Tile { tex, x0, y0, x1, y1, pad_x: x0 - tx0, pad_y: y0 - ty0 });
        }
    }
    tiles
}

/// 高质量缩小到 w × h（与编辑时缩小显示用同一个算法，多线程）
pub(crate) fn resample(src: &RgbaImage, w: u32, h: u32) -> ColorImage {
    let out = crate::editor::resample::resize(src, w, h, crate::editor::resample::Filter::CatmullRom, true);
    ColorImage::from_rgba_premultiplied([w as usize, h as usize], &out)
}

/// 取一块区域转成 egui 的（预乘）颜色
pub fn region(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> ColorImage {
    let stride = img.width() as usize * 4;
    let raw = img.as_raw();
    let mut pixels = vec![Color32::TRANSPARENT; w as usize * h as usize];
    pixels.par_chunks_mut(w as usize).enumerate().for_each(|(row, out)| {
        let start = (y as usize + row) * stride + x as usize * 4;
        let src = &raw[start..start + w as usize * 4];
        for (o, p) in out.iter_mut().zip(src.as_chunks::<4>().0) {
            *o = if p[3] == 255 {
                Color32::from_rgb(p[0], p[1], p[2])
            } else {
                Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])
            };
        }
    });
    ColorImage::new([w as usize, h as usize], pixels)
}

/// 透明部分的棋盘格：每格 8 个逻辑像素，跟着图片移动
pub fn paint_checker(ctx: &egui::Context, painter: &Painter, rect: Rect, colors: [Color32; 2]) {
    let id = egui::Id::new(("checker", colors[0].to_array(), colors[1].to_array()));
    let tex = ctx.data(|d| d.get_temp::<TextureHandle>(id)).unwrap_or_else(|| {
        let img = ColorImage::new([2, 2], vec![colors[0], colors[1], colors[1], colors[0]]);
        let t = ctx.load_texture("checker", img, TextureOptions::NEAREST_REPEAT);
        ctx.data_mut(|d| d.insert_temp(id, t.clone()));
        t
    });
    let r = rect.intersect(painter.clip_rect());
    if !r.is_positive() {
        return;
    }
    let cell = 8.0 * 2.0;
    let uv = Rect::from_min_max(((r.min - rect.min) / cell).to_pos2(), ((r.max - rect.min) / cell).to_pos2());
    painter.image(tex.id(), r, uv, Color32::WHITE);
}

/// 动图：一张纹理，换帧时整张替换；超过纹理上限的帧先缩小
pub struct AnimTexture {
    tex: Option<TextureHandle>,
    shown: Option<usize>,
}

impl AnimTexture {
    pub fn new() -> Self {
        Self { tex: None, shown: None }
    }

    pub fn paint(&mut self, painter: &Painter, rect: Rect, xf: Transform, index: usize, frame: &RgbaImage, nearest: bool) {
        let ctx = painter.ctx();
        let opts = options(nearest);
        if self.shown != Some(index) || self.tex.is_none() {
            let started = std::time::Instant::now();
            let max = ctx.input(|i| i.max_texture_side).max(2048) as u32;
            let ci = if frame.width() > max || frame.height() > max {
                let mut small = crate::decode::halve(frame);
                while small.width() > max || small.height() > max {
                    small = crate::decode::halve(&small);
                }
                region(&small, 0, 0, small.width(), small.height())
            } else {
                region(frame, 0, 0, frame.width(), frame.height())
            };
            match &mut self.tex {
                Some(t) => t.set(ci, opts),
                None => self.tex = Some(ctx.load_texture("anim", ci, opts)),
            }
            self.shown = Some(index);
            log::debug!("动图换帧 {index}：转换 {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        if let Some(t) = &self.tex {
            quad(painter, t.id(), rect, xf, [0.0, 0.0, 1.0, 1.0], Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        }
    }

    /// 画当前这一帧（不上传；打印预览用）
    pub fn paint_resident(&self, painter: &Painter, rect: Rect, xf: Transform) {
        if let Some(t) = &self.tex {
            quad(painter, t.id(), rect, xf, [0.0, 0.0, 1.0, 1.0], Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        }
    }
}

impl Default for AnimTexture {
    fn default() -> Self {
        Self::new()
    }
}
