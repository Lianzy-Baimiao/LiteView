//! 编辑时的显示。缩小显示：按准确的显示尺寸高质量缩小（与看图一致，没有摩尔纹），改动时只重算受影响的部分；
//! 缩放变了先把旧的拉伸着显示，停下后在后台重算。放大显示：分块纹理（第一次用到时才上传），只上传改动的部分。

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use egui::{Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions, pos2, vec2};
use image::RgbaImage;

use crate::editor::IRect;
use crate::editor::history::Doc;
use crate::editor::resample::{self, Axis, Filter};
use crate::viewer::texture::{Tile, paint_tiles, update_tiles, upload};
use crate::viewer::view::Transform;

/// 缩小显示的图：zoom 下准确尺寸
struct Exact {
    zoom: f32,
    size: [u32; 2],
    ax: Axis,
    ay: Axis,
    tex: TextureHandle,
}

struct Job {
    zoom: f32,
    rx: Receiver<(Axis, Axis, ColorImage)>,
    /// 后台算的期间又改过的范围（算完后补上）
    dirty: IRect,
}

#[derive(Default)]
pub struct EditTextures {
    tiles: Vec<Tile>,
    nearest: bool,
    /// 分块纹理还没更新的范围
    stale: IRect,
    exact: Option<Exact>,
    job: Option<Job>,
}

/// zoom 下准确的显示尺寸（与看图一致）
fn target(img: &RgbaImage, zoom: f32) -> (u32, u32) {
    (((img.width() as f32 * zoom).round() as u32).max(1), ((img.height() as f32 * zoom).round() as u32).max(1))
}

fn compute(img: &RgbaImage, zoom: f32) -> (Axis, Axis, ColorImage) {
    let (w, h) = target(img, zoom);
    let (ax, ay) = (Axis::new(img.width(), w, Filter::CatmullRom), Axis::new(img.height(), h, Filter::CatmullRom));
    let ci = render_part(img, &ax, &ay, IRect::new(0, 0, w, h));
    (ax, ay, ci)
}

/// 输出的 out 这块：一条条算、直接写进结果的像素（不另外留一份 RGBA 再转；4800 万像素缩到九成时这一份约 150 MB）
fn render_part(img: &RgbaImage, ax: &Axis, ay: &Axis, out: IRect) -> ColorImage {
    let (w, h) = (out.width() as usize, out.height() as usize);
    let mut pixels = vec![Color32::TRANSPARENT; w * h];
    resample::render_banded_into(bytemuck::cast_slice_mut(&mut pixels), img, ax, ay, out, true);
    ColorImage::new([w, h], pixels)
}

impl Exact {
    /// 图上 dirty 变了：只重算受影响的部分
    fn patch(&mut self, img: &RgbaImage, dirty: IRect) {
        let out = resample::affected(&self.ax, &self.ay, dirty);
        if out.is_empty() {
            return;
        }
        let ci = render_part(img, &self.ax, &self.ay, out);
        self.tex.set_partial([out.x0 as usize, out.y0 as usize], ci, TextureOptions::LINEAR);
    }
}

impl EditTextures {
    /// 每帧画之前：取走文档改过的范围，更新已有的纹理
    pub fn sync(&mut self, doc: &mut Doc) {
        let (dirty, resized) = doc.take_dirty();
        if resized {
            *self = Self::default();
            return;
        }
        if dirty.is_empty() {
            return;
        }
        self.stale = self.stale.union(&dirty);
        if let Some(e) = &mut self.exact {
            e.patch(&doc.image, dirty);
        }
        if let Some(j) = &mut self.job {
            j.dirty = j.dirty.union(&dirty);
        }
    }

    /// 画到 rect（图片在屏幕上的矩形）；settled = 缩放没有在动画中
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, ctx: &egui::Context, painter: &Painter, img: &Arc<RgbaImage>, rect: Rect, zoom: f32, nearest_above: f32, settled: bool) {
        let max = ctx.input(|i| i.max_texture_side) as u32;
        let (tw, th) = target(img, zoom);
        let fits = zoom < 0.999 && tw <= max && th <= max && u64::from(tw) * u64::from(th) <= 40_000_000;
        if fits {
            self.poll(ctx, img);
            let same = |z: f32| (z - zoom).abs() < 1e-5;
            if self.exact.is_none() {
                // 刚进入编辑或尺寸刚变：同步算一次，避免先显示别的
                let (ax, ay, ci) = compute(img, zoom);
                let tex = ctx.load_texture("edit-exact", ci, TextureOptions::LINEAR);
                self.exact = Some(Exact { zoom, size: [tw, th], ax, ay, tex });
            } else if settled && !self.exact.as_ref().is_some_and(|e| same(e.zoom)) && !self.job.as_ref().is_some_and(|j| same(j.zoom)) {
                let (tx, rx) = channel();
                let (src, ctx2) = (img.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(compute(&src, zoom));
                    ctx2.request_repaint();
                });
                self.job = Some(Job { zoom, rx, dirty: IRect::EMPTY });
            }
            if let Some(e) = &self.exact {
                // 准确尺寸时 1:1 显示；缩放在变时拉伸旧的
                let r = if same(e.zoom) { Rect::from_min_size(rect.min, vec2(e.size[0] as f32, e.size[1] as f32) / ctx.pixels_per_point()) } else { rect };
                crate::viewer::texture::quad(painter, e.tex.id(), r, Transform::default(), [0.0, 0.0, 1.0, 1.0], Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
                return;
            }
        }
        let nearest = zoom >= nearest_above;
        if self.tiles.is_empty() || self.nearest != nearest {
            self.tiles = upload(ctx, "edit", img, ctx.input(|i| i.max_texture_side).max(2048), nearest);
            self.nearest = nearest;
            self.stale = IRect::EMPTY;
        } else if !self.stale.is_empty() {
            let s = std::mem::take(&mut self.stale);
            update_tiles(&mut self.tiles, img, s.x0, s.y0, s.x1, s.y1, nearest);
        }
        paint_tiles(painter, &self.tiles, img.dimensions(), rect, Transform::default());
    }

    /// 后台算好了：换上，并补上算的期间的改动
    fn poll(&mut self, ctx: &egui::Context, img: &RgbaImage) {
        let Some(j) = &self.job else { return };
        let Ok((ax, ay, ci)) = j.rx.try_recv() else { return };
        let (zoom, dirty) = (j.zoom, j.dirty);
        self.job = None;
        let size = [ci.size[0] as u32, ci.size[1] as u32];
        let tex = ctx.load_texture("edit-exact", ci, TextureOptions::LINEAR);
        let mut e = Exact { zoom, size, ax, ay, tex };
        if !dirty.is_empty() {
            e.patch(img, dirty);
        }
        self.exact = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 直接写进 ColorImage 的像素，与以前「算出 RGBA → from_rgba_premultiplied」的结果相同（整张和改了一块）
    #[test]
    fn writes_the_same_pixels_as_the_rgba_copy() {
        let img = RgbaImage::from_fn(900, 700, |x, y| image::Rgba([(x * 7) as u8, (y * 3) as u8, ((x ^ y) * 5) as u8, 128 + (x % 128) as u8]));
        let (ax, ay, ci) = compute(&img, 0.37);
        let full = IRect::new(0, 0, ax.len(), ay.len());
        let old = ColorImage::from_rgba_premultiplied(ci.size, &resample::render(&img, &ax, &ay, full, true));
        assert_eq!(ci.pixels, old.pixels);
        let part = IRect::new(13, 40, 300, 233);
        let old = ColorImage::from_rgba_premultiplied([part.width() as usize, part.height() as usize], &resample::render(&img, &ax, &ay, part, true));
        assert_eq!(render_part(&img, &ax, &ay, part).pixels, old.pixels);
    }
}
