//! 缩放与平移。zoom = 每个图片像素占多少个屏幕物理像素（1.0 = 100%）；
//! offset = 图片中心相对画布中心的偏移（逻辑像素）。旋转 / 翻转只改显示。

use std::time::Instant;

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::settings::FitMode;

pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;
/// 滚轮每格的缩放倍数
pub const WHEEL_STEP: f32 = 1.15;
const ANIM_SECS: f32 = 0.1;

/// 显示变换：先在原图上顺时针转 rotation 个 90°，再在屏幕上水平 / 垂直翻转
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Transform {
    pub rotation: u8,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Transform {
    /// 原图上的归一化坐标 (u, v) → 屏幕坐标；rect 是旋转后图片在屏幕上的矩形
    pub fn map(&self, rect: Rect, u: f32, v: f32) -> Pos2 {
        let (mut s, mut t) = (u, v);
        for _ in 0..self.rotation % 4 {
            (s, t) = (1.0 - t, s);
        }
        if self.flip_h {
            s = 1.0 - s;
        }
        if self.flip_v {
            t = 1.0 - t;
        }
        pos2(rect.left() + s * rect.width(), rect.top() + t * rect.height())
    }

    /// 屏幕上顺时针 / 逆时针转 90°（转之前的翻转方向随之互换）
    pub fn rotate(&mut self, clockwise: bool) {
        self.rotation = (self.rotation + if clockwise { 1 } else { 3 }) % 4;
        std::mem::swap(&mut self.flip_h, &mut self.flip_v);
    }

    pub fn is_identity(&self) -> bool {
        // 水平 + 垂直翻转 = 转 180°
        let r = (self.rotation + if self.flip_h && self.flip_v { 2 } else { 0 }) % 4;
        r == 0 && self.flip_h == self.flip_v
    }
}

#[derive(Clone, Copy, Debug)]
struct Anim {
    from: f32,
    to: f32,
    start: Instant,
    anchor: Pos2,
}

#[derive(Clone, Debug)]
pub struct View {
    pub zoom: f32,
    pub offset: Vec2,
    pub xf: Transform,
    /// 处于「适应窗口」：窗口大小变化时跟着重新适应
    pub fitted: bool,
    anim: Option<Anim>,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, offset: Vec2::ZERO, xf: Transform::default(), fitted: true, anim: None }
    }
}

impl View {
    /// 旋转后的显示尺寸（图片像素）
    pub fn shown_size(&self, w: u32, h: u32) -> Vec2 {
        if self.xf.rotation % 2 == 1 { vec2(h as f32, w as f32) } else { vec2(w as f32, h as f32) }
    }

    /// 图片在屏幕上的矩形（逻辑像素），左上角对齐到物理像素
    pub fn image_rect(&self, canvas: Rect, ppp: f32, size: Vec2) -> Rect {
        let s = size * self.zoom / ppp;
        let min = canvas.center() + self.offset - s / 2.0;
        Rect::from_min_size(pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp), s)
    }

    /// 屏幕坐标 → 显示图上的像素坐标
    pub fn to_image(&self, canvas: Rect, ppp: f32, size: Vec2, pos: Pos2) -> Pos2 {
        let r = self.image_rect(canvas, ppp, size);
        ((pos - r.min) * ppp / self.zoom).to_pos2()
    }

    pub fn fit_zoom(mode: FitMode, canvas: Vec2, ppp: f32, size: Vec2) -> f32 {
        let avail = canvas * ppp;
        let z = match mode {
            FitMode::Fit => (avail.x / size.x).min(avail.y / size.y),
            FitMode::FitWidth => avail.x / size.x,
            FitMode::Actual => 1.0,
        };
        z.min(1.0).clamp(MIN_ZOOM, MAX_ZOOM)
    }

    /// 按显示方式重新显示（新图片、「适应窗口」）；长图「适应宽度」时从顶部开始
    pub fn reset(&mut self, mode: FitMode, canvas: Vec2, ppp: f32, size: Vec2) {
        self.anim = None;
        self.zoom = Self::fit_zoom(mode, canvas, ppp, size);
        self.offset = Vec2::ZERO;
        self.fitted = mode == FitMode::Fit;
        let h = size.y * self.zoom / ppp;
        if mode == FitMode::FitWidth && h > canvas.y {
            self.offset.y = (h - canvas.y) / 2.0;
        }
    }

    /// 把缩放设为 z，anchor（屏幕坐标）下的像素位置不变
    fn apply_zoom(&mut self, z: f32, anchor: Pos2, canvas: Rect) {
        let z = z.clamp(MIN_ZOOM, MAX_ZOOM);
        let center = canvas.center() + self.offset;
        let new_center = anchor + (center - anchor) * (z / self.zoom);
        self.offset = new_center - canvas.center();
        self.zoom = z;
    }

    pub fn animating(&self) -> bool {
        self.anim.is_some()
    }

    pub fn target_zoom(&self) -> f32 {
        self.anim.map_or(self.zoom, |a| a.to)
    }

    /// 缩放到 target；smooth 时有 100 ms 过渡
    pub fn zoom_to(&mut self, target: f32, anchor: Pos2, smooth: bool, canvas: Rect) {
        let target = target.clamp(MIN_ZOOM, MAX_ZOOM);
        self.fitted = false;
        if smooth {
            self.anim = Some(Anim { from: self.zoom, to: target, start: Instant::now(), anchor });
        } else {
            self.anim = None;
            self.apply_zoom(target, anchor, canvas);
        }
    }

    /// 推进缩放动画；返回是否还要继续重绘
    pub fn tick(&mut self, canvas: Rect) -> bool {
        let Some(a) = self.anim else { return false };
        let t = (a.start.elapsed().as_secs_f32() / ANIM_SECS).min(1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        let z = if t >= 1.0 { a.to } else { (a.from.ln() + (a.to.ln() - a.from.ln()) * e).exp() };
        self.apply_zoom(z, a.anchor, canvas);
        if t >= 1.0 {
            self.anim = None;
        }
        t < 1.0
    }

    /// 平移不能把图片完全拖出可视区域；比窗口小的方向保持居中
    pub fn clamp(&mut self, canvas: Rect, ppp: f32, size: Vec2) {
        let s = size * self.zoom / ppp;
        let lim = |img: f32, can: f32| if img <= can { 0.0 } else { (img - can) / 2.0 };
        let (lx, ly) = (lim(s.x, canvas.width()), lim(s.y, canvas.height()));
        self.offset.x = self.offset.x.clamp(-lx, lx);
        self.offset.y = self.offset.y.clamp(-ly, ly);
    }
}

/// 缩放经过某个吸附点（100%、适应窗口）时停在那里；下一次再越过
pub fn snap(from: f32, to: f32, points: &[f32]) -> f32 {
    let (lo, hi) = if from < to { (from, to) } else { (to, from) };
    points
        .iter()
        .copied()
        .filter(|&p| p > lo * 1.0001 && p < hi / 1.0001)
        .min_by(|a, b| (a - from).abs().total_cmp(&(b - from).abs()))
        .unwrap_or(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
    }

    #[test]
    fn zoom_keeps_anchor_pixel() {
        let size = vec2(4000.0, 3000.0);
        let mut v = View::default();
        v.reset(FitMode::Actual, canvas().size(), 1.0, size);
        let anchor = pos2(100.0, 50.0);
        let before = v.to_image(canvas(), 1.0, size, anchor);
        v.zoom_to(2.0, anchor, false, canvas());
        let after = v.to_image(canvas(), 1.0, size, anchor);
        assert!((before - after).length() < 0.6, "{before:?} vs {after:?}");
    }

    #[test]
    fn fit_and_clamp() {
        let size = vec2(1600.0, 600.0);
        assert_eq!(View::fit_zoom(FitMode::Fit, canvas().size(), 1.0, size), 0.5);
        assert_eq!(View::fit_zoom(FitMode::Fit, canvas().size(), 2.0, vec2(100.0, 100.0)), 1.0);
        let mut v = View { zoom: 1.0, offset: vec2(5000.0, 40.0), ..View::default() };
        v.clamp(canvas(), 1.0, size);
        assert_eq!(v.offset, vec2(400.0, 0.0));
        let mut w = View::default();
        w.reset(FitMode::FitWidth, canvas().size(), 1.0, vec2(800.0, 3000.0));
        assert_eq!(w.image_rect(canvas(), 1.0, vec2(800.0, 3000.0)).top(), 0.0);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap(0.9, 1.035, &[1.0]), 1.0);
        assert_eq!(snap(1.0, 1.15, &[1.0]), 1.15);
        assert_eq!(snap(2.0, 0.3, &[1.0, 0.5]), 1.0);
    }

    #[test]
    fn transform() {
        let r = Rect::from_min_size(Pos2::ZERO, vec2(2.0, 4.0));
        let mut xf = Transform::default();
        xf.rotate(true);
        assert_eq!(xf.map(r, 0.0, 0.0), pos2(2.0, 0.0));
        let mut f = Transform { flip_h: true, ..Default::default() };
        f.rotate(true);
        assert!(f.flip_v && !f.flip_h);
        let both = Transform { rotation: 2, flip_h: true, flip_v: true };
        assert!(both.is_identity());
        assert!(!Transform { rotation: 1, ..Default::default() }.is_identity());
    }
}
