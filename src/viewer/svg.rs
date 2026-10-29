//! 放大看 SVG：显示用的是长边 2048 的栅格，放大超过 1:1 就发虚。缩放、平移停下一会儿后在后台按屏幕的像素
//! 重画看得见的那块（只画那块，内存与窗口大小有关，与放大倍数无关），1:1 叠在上面；整块都盖住时底下的栅格不画。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use egui::{ColorImage, Mesh, Rect, TextureHandle, TextureOptions, pos2};

use super::texture::quad_mesh;
use super::view::Transform;
use crate::decode::extra;

/// 停下这么久才重画（拖动、滚轮缩放中途不画）
const SETTLE: Duration = Duration::from_millis(120);

/// 栅格上的一块（像素）和放大倍数：重画的就是它
#[derive(Clone, Copy, Debug, PartialEq)]
struct Region {
    zoom: f32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

struct Overlay {
    region: Region,
    tex: TextureHandle,
}

struct Job {
    region: Region,
    rx: Receiver<(Option<Arc<resvg::usvg::Tree>>, Option<ColorImage>)>,
}

#[derive(Default)]
pub struct SharpSvg {
    path: Option<PathBuf>,
    tree: Option<Arc<resvg::usvg::Tree>>,
    overlay: Option<Overlay>,
    job: Option<Job>,
    /// 上一帧看得见的那块、从什么时候起没变
    last: Option<Region>,
    since: Option<Instant>,
    /// 这块画不出来（读不了、解析不了、太大）：看得见的那块变了之前不再试
    failed: Option<Region>,
}

/// 屏幕上看得见的那块在栅格上的范围（向外取整）；size = 栅格大小，rect = 图片在屏幕上的矩形
fn visible(clip: Rect, rect: Rect, xf: Transform, size: (u32, u32), zoom: f32) -> Option<Region> {
    let r = clip.intersect(rect);
    if !r.is_positive() {
        return None;
    }
    let corners = [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()].map(|p| xf.unmap(rect, p));
    let (u0, u1) = corners.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c.0), b.max(c.0)));
    let (v0, v1) = corners.iter().fold((f32::MAX, f32::MIN), |(a, b), c| (a.min(c.1), b.max(c.1)));
    let (w, h) = (size.0 as f32, size.1 as f32);
    let x0 = (u0.clamp(0.0, 1.0) * w).floor() as u32;
    let y0 = (v0.clamp(0.0, 1.0) * h).floor() as u32;
    let x1 = ((u1.clamp(0.0, 1.0) * w).ceil() as u32).clamp(x0 + 1, size.0.max(x0 + 1));
    let y1 = ((v1.clamp(0.0, 1.0) * h).ceil() as u32).clamp(y0 + 1, size.1.max(y0 + 1));
    Some(Region { zoom, x0, y0, x1, y1 })
}

impl SharpSvg {
    /// 换了图或图重新载入：放掉
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// path 这张 SVG 显示成 rect、缩放 zoom（屏幕像素 / 栅格像素，> 1）、栅格 size，clip 是画得到的范围；
    /// settled = 缩放不在动画中。有对得上的重画结果时给出要叠在上面的网格，以及它是否盖住了看得见的整块
    /// （是的话底下的栅格不用画：透明的 SVG 会从边缘透出发虚的那层）
    #[allow(clippy::too_many_arguments)]
    pub fn overlay(&mut self, ctx: &egui::Context, clip: Rect, path: &Path, rect: Rect, xf: Transform, size: (u32, u32), zoom: f32, settled: bool) -> Option<(Mesh, bool)> {
        if self.path.as_deref() != Some(path) {
            self.reset();
            self.path = Some(path.to_path_buf());
        }
        self.poll(ctx);
        let want = visible(clip, rect, xf, size, zoom)?;
        // 看得见的那块变了：等它停下 SETTLE 再重画
        if self.last != Some(want) {
            self.last = Some(want);
            self.since = Some(Instant::now());
        }
        let waited = self.since.map_or(Duration::ZERO, |t| t.elapsed());
        let fresh = self.overlay.as_ref().is_some_and(|o| o.region == want) || self.job.as_ref().is_some_and(|j| j.region == want) || self.failed == Some(want);
        if settled && !fresh && self.job.is_none() {
            if waited >= SETTLE {
                self.start(ctx, want);
            } else {
                ctx.request_repaint_after(SETTLE - waited);
            }
        }
        let o = self.overlay.as_ref().filter(|o| (o.region.zoom - zoom).abs() < 1e-5)?;
        let (w, h) = (size.0 as f32, size.1 as f32);
        let g = o.region;
        let src = [g.x0 as f32 / w, g.y0 as f32 / h, g.x1 as f32 / w, g.y1 as f32 / h];
        let mesh = quad_mesh(o.tex.id(), rect, xf, src, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        Some((mesh, g.x0 <= want.x0 && g.y0 <= want.y0 && g.x1 >= want.x1 && g.y1 >= want.y1))
    }

    /// 后台画好了：换上（第一次还带回解析好的 SVG，留着下次用）
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(j) = &self.job else { return };
        let Ok((tree, img)) = j.rx.try_recv() else { return };
        let region = j.region;
        self.job = None;
        if let Some(t) = tree {
            self.tree = Some(t);
        }
        match img {
            Some(img) => self.overlay = Some(Overlay { region, tex: ctx.load_texture("svg-sharp", img, TextureOptions::NEAREST) }),
            None => self.failed = Some(region),
        }
    }

    fn start(&mut self, ctx: &egui::Context, r: Region) {
        let Some(path) = self.path.clone() else { return };
        let (tree, ctx) = (self.tree.clone(), ctx.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let parsed = tree.is_none();
            let tree = tree.or_else(|| std::fs::read(&path).ok().and_then(|d| extra::parse_svg(&d).ok()).map(Arc::new));
            let img = tree.as_ref().and_then(|t| {
                let (w, h) = (((r.x1 - r.x0) as f32 * r.zoom).ceil() as u32, ((r.y1 - r.y0) as f32 * r.zoom).ceil() as u32);
                // 直接画进 egui 的像素（预乘透明度 RGBA，排列相同）
                let mut px = vec![egui::Color32::TRANSPARENT; w as usize * h as usize];
                extra::render_svg_region_into(t, r.zoom, r.x0 as f32, r.y0 as f32, bytemuck::cast_slice_mut(&mut px), w, h).then(|| ColorImage::new([w as usize, h as usize], px))
            });
            let _ = tx.send((if parsed { tree } else { None }, img));
            ctx.request_repaint();
        });
        self.job = Some(Job { region: r, rx });
    }

    /// 不再放大（缩小到 1:1 以下）：放掉重画的纹理，解析好的 SVG 留着
    pub fn drop_overlay(&mut self) {
        self.overlay = None;
        self.last = None;
    }
}
