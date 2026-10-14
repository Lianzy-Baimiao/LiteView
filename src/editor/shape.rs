//! 形状（PRD 6.7.3）：直线、箭头、矩形、圆角矩形、椭圆；轮廓 / 填充、粗细，抗锯齿。
//! 预览与写进图里用同一套画法（tiny-skia），所见即所得。

use image::RgbaImage;
use resvg::tiny_skia::{self as sk, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Transform};

use super::IRect;
use crate::settings::ShapeKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    /// 起点、终点（图片像素）
    pub a: (f32, f32),
    pub b: (f32, f32),
    pub outline: Option<[u8; 4]>,
    pub fill: Option<[u8; 4]>,
    pub width: f32,
}

impl Shape {
    fn is_line(&self) -> bool {
        matches!(self.kind, ShapeKind::Line | ShapeKind::Arrow)
    }

    /// 箭头的大小随线宽变化
    fn head(&self) -> f32 {
        (self.width * 3.0).max(10.0)
    }

    /// 画出来会涉及的范围（图片像素，含线宽、箭头）
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let pad = self.width / 2.0 + if self.kind == ShapeKind::Arrow { self.head() } else { 0.0 } + 2.0;
        (self.a.0.min(self.b.0) - pad, self.a.1.min(self.b.1) - pad, self.a.0.max(self.b.0) + pad, self.a.1.max(self.b.1) + pad)
    }

    fn path(&self) -> Option<sk::Path> {
        let (x0, y0, x1, y1) = (self.a.0.min(self.b.0), self.a.1.min(self.b.1), self.a.0.max(self.b.0), self.a.1.max(self.b.1));
        let mut pb = PathBuilder::new();
        match self.kind {
            ShapeKind::Line | ShapeKind::Arrow => {
                pb.move_to(self.a.0, self.a.1);
                pb.line_to(self.b.0, self.b.1);
            }
            ShapeKind::Rect => pb.push_rect(sk::Rect::from_ltrb(x0, y0, x1.max(x0 + 0.5), y1.max(y0 + 0.5))?),
            ShapeKind::Ellipse => pb.push_oval(sk::Rect::from_ltrb(x0, y0, x1.max(x0 + 0.5), y1.max(y0 + 0.5))?),
            ShapeKind::RoundRect => {
                let r = ((x1 - x0).min(y1 - y0) * 0.2).min(self.width * 4.0 + 8.0).max(0.0);
                let k = r * 0.552_284_8;
                pb.move_to(x0 + r, y0);
                pb.line_to(x1 - r, y0);
                pb.cubic_to(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r);
                pb.line_to(x1, y1 - r);
                pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
                pb.line_to(x0 + r, y1);
                pb.cubic_to(x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r);
                pb.line_to(x0, y0 + r);
                pb.cubic_to(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0);
                pb.close();
            }
        }
        pb.finish()
    }

    /// 箭头的三角形
    fn arrow_head(&self) -> Option<sk::Path> {
        let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1.0 {
            return None;
        }
        let (ux, uy) = (dx / len, dy / len);
        let h = self.head().min(len);
        let (bx, by) = (self.b.0 - ux * h, self.b.1 - uy * h);
        let w = h * 0.55;
        let mut pb = PathBuilder::new();
        pb.move_to(self.b.0, self.b.1);
        pb.line_to(bx - uy * w, by + ux * w);
        pb.line_to(bx + uy * w, by - ux * w);
        pb.close();
        pb.finish()
    }

    /// 画到 pixmap 上；t 把图片像素坐标变成 pixmap 坐标
    pub fn draw(&self, pix: &mut Pixmap, t: Transform) {
        let paint = |c: [u8; 4]| {
            let mut p = Paint::default();
            p.set_color_rgba8(c[0], c[1], c[2], c[3]);
            p.anti_alias = true;
            p
        };
        let Some(path) = self.path() else { return };
        if let Some(f) = self.fill.filter(|_| !self.is_line()) {
            pix.fill_path(&path, &paint(f), FillRule::Winding, t, None);
        }
        let Some(c) = self.outline.or(self.is_line().then_some(self.fill).flatten()) else { return };
        let mut stroke = sk::Stroke { width: self.width.max(1.0), line_join: LineJoin::Miter, ..Default::default() };
        if self.is_line() {
            stroke.line_cap = if self.kind == ShapeKind::Arrow { LineCap::Butt } else { LineCap::Round };
        }
        if self.kind == ShapeKind::Arrow
            && let Some(head) = self.arrow_head()
        {
            // 线画到箭头的底边，箭头盖住线头
            let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
            let len = (dx * dx + dy * dy).sqrt();
            let k = ((len - self.head() * 0.8).max(0.0)) / len;
            let mut pb = PathBuilder::new();
            pb.move_to(self.a.0, self.a.1);
            pb.line_to(self.a.0 + dx * k, self.a.1 + dy * k);
            if let Some(shaft) = pb.finish() {
                pix.stroke_path(&shaft, &paint(c), &stroke, t, None);
            }
            pix.fill_path(&head, &paint(c), FillRule::Winding, t, None);
        } else {
            pix.stroke_path(&path, &paint(c), &stroke, t, None);
        }
    }

    /// 写进图里；返回改到的范围
    pub fn composite(&self, img: &mut RgbaImage) -> IRect {
        let (w, h) = img.dimensions();
        let (x0, y0, x1, y1) = self.bounds();
        let r = IRect::around(x0, y0, x1, y1, w, h);
        let Some(mut pix) = Pixmap::new(r.width().max(1), r.height().max(1)) else { return IRect::EMPTY };
        self.draw(&mut pix, Transform::from_translate(-(r.x0 as f32), -(r.y0 as f32)));
        for (i, p) in pix.pixels().iter().enumerate() {
            if p.alpha() == 0 {
                continue;
            }
            let c = p.demultiply();
            let (x, y) = (r.x0 + i as u32 % r.width(), r.y0 + i as u32 / r.width());
            let d = img.get_pixel(x, y).0;
            img.put_pixel(x, y, image::Rgba(super::ops::over([c.red(), c.green(), c.blue(), c.alpha()], d)));
        }
        r
    }
}

/// Shift：矩形 / 椭圆变成正方形 / 正圆，直线吸附到 45° 的倍数
pub fn constrain(kind: ShapeKind, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    if matches!(kind, ShapeKind::Line | ShapeKind::Arrow) {
        let len = (dx * dx + dy * dy).sqrt();
        let ang = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
        return (a.0 + len * ang.cos(), a.1 + len * ang.sin());
    }
    let s = dx.abs().max(dy.abs());
    (a.0 + s * dx.signum(), a.1 + s * dy.signum())
}
#[cfg(test)]
mod tests {
    use super::*;

    fn white() -> RgbaImage {
        RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]))
    }

    #[test]
    fn rectangle_outline_and_fill() {
        let mut img = white();
        let s = Shape { kind: ShapeKind::Rect, a: (20.0, 20.0), b: (80.0, 80.0), outline: Some([255, 0, 0, 255]), fill: None, width: 4.0 };
        let r = s.composite(&mut img);
        assert!(r.contains(18, 18) && r.contains(81, 81));
        assert_eq!(img.get_pixel(50, 20).0, [255, 0, 0, 255], "edge");
        assert_eq!(img.get_pixel(50, 50).0, [255, 255, 255, 255], "inside stays");
        assert_eq!(img.get_pixel(50, 5).0, [255, 255, 255, 255], "outside stays");
        let mut img = white();
        let e = Shape { kind: ShapeKind::Ellipse, a: (10.0, 30.0), b: (90.0, 70.0), outline: None, fill: Some([0, 0, 255, 255]), width: 2.0 };
        e.composite(&mut img);
        assert_eq!(img.get_pixel(50, 50).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(12, 32).0, [255, 255, 255, 255], "corner outside ellipse");
    }

    #[test]
    fn arrow_has_head_at_end() {
        let mut img = white();
        let s = Shape { kind: ShapeKind::Arrow, a: (10.0, 50.0), b: (90.0, 50.0), outline: Some([0, 0, 0, 255]), fill: None, width: 3.0 };
        s.composite(&mut img);
        // 箭头底边比线宽，线头被箭头盖住（(82, 53) 在箭头里、在 3 像素宽的线外）
        assert_eq!(img.get_pixel(82, 53).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(30, 53).0, [255, 255, 255, 255]);
        assert_eq!(img.get_pixel(30, 50).0, [0, 0, 0, 255]);
    }

    #[test]
    fn shift_constrains() {
        assert_eq!(constrain(ShapeKind::Rect, (0.0, 0.0), (10.0, -4.0)), (10.0, -10.0));
        let p = constrain(ShapeKind::Line, (0.0, 0.0), (10.0, 1.0));
        assert!((p.1).abs() < 1e-4 && (p.0 - 101f32.sqrt()).abs() < 1e-3);
        let q = constrain(ShapeKind::Arrow, (0.0, 0.0), (10.0, 9.0));
        assert!((q.0 - q.1).abs() < 1e-3);
    }
}
