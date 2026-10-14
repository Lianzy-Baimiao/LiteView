//! 形状：画完保留 8 个控制点（直线 / 箭头是两个端点），可拉伸、移动、改颜色和线宽，直到切换工具或点空白处。
//! 预览按屏幕分辨率用同一套画法画出来（所见即所得）。文字工具在 `text`。

use egui::{Color32, Pos2, Rect, Vec2, pos2};
use resvg::tiny_skia::{Pixmap, Transform};

use super::{Drag, Editor};
use crate::app::App;
use crate::editor::shape::{Shape, constrain};
use crate::settings::{Settings, ShapeKind};

pub(crate) use super::text::PendingText;

/// 画完还没确认的形状（颜色、线宽、种类随属性栏变化）
pub(crate) struct PendingShape {
    pub a: Pos2,
    pub b: Pos2,
    /// 右键画的：主色辅色互换
    pub swap: bool,
    preview: Option<(String, egui::TextureHandle, Rect)>,
}

pub(crate) enum ShapeHit {
    Handle(usize),
    Body,
}

impl PendingShape {
    pub fn new(a: Pos2, swap: bool) -> Self {
        Self { a, b: a, swap, preview: None }
    }

    /// 按当前设置得到的形状。只有填充时用主色（盖住证件号这类用途）；轮廓 + 填充时轮廓主色、填充辅色
    pub fn shape(&self, s: &Settings) -> Shape {
        let (c1, c2) = if self.swap { (s.color2, s.color1) } else { (s.color1, s.color2) };
        let line = matches!(s.shape, ShapeKind::Line | ShapeKind::Arrow);
        let (outline, fill) = match (s.shape_outline || line, s.shape_fill && !line) {
            (true, true) => (Some(c1), Some(c2)),
            (false, true) => (None, Some(c1)),
            _ => (Some(c1), None),
        };
        Shape { kind: s.shape, a: (self.a.x, self.a.y), b: (self.b.x, self.b.y), outline, fill, width: s.shape_width as f32 }
    }
}

/// 拉控制点：直线 / 箭头的 0、1 是两个端点，其它形状按外框的 8 个点
fn apply_handle(a: Pos2, b: Pos2, line: bool, index: usize, p: Pos2, shift: bool) -> (Pos2, Pos2) {
    let p = p.round();
    if line {
        return if index == 0 { (p, b) } else { (a, p) };
    }
    let r = super::pointer::resize_rect(Rect::from_two_pos(a, b), index, p, shift);
    (r.min, r.max)
}

impl App {
    pub(crate) fn shape_hit(&self, ed: &Editor, screen: Pos2) -> Option<ShapeHit> {
        let sh = ed.shape.as_ref()?;
        let line = matches!(self.settings.shape, ShapeKind::Line | ShapeKind::Arrow);
        let (a, b) = (self.image_to_screen(ed, sh.a), self.image_to_screen(ed, sh.b));
        if line {
            if let Some(i) = [a, b].iter().position(|h| (*h - screen).length() <= 7.0) {
                return Some(ShapeHit::Handle(i));
            }
            // 离线段近
            let (d, len2) = (b - a, (b - a).length_sq());
            let t = if len2 > 0.0 { ((screen - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
            return ((a + d * t - screen).length() <= 6.0 + self.settings.shape_width as f32 * self.view.zoom / 2.0).then_some(ShapeHit::Body);
        }
        // 与 shape_overlay 画的外框一致
        let sr = Rect::from_two_pos(a, b).expand(self.settings.shape_width as f32 * self.view.zoom / self.ctx.pixels_per_point() / 2.0 + 4.0);
        if let Some(i) = super::pointer::hit_handle(sr, screen) {
            return Some(ShapeHit::Handle(i));
        }
        sr.contains(screen).then_some(ShapeHit::Body)
    }

    pub(crate) fn shape_press(&mut self, ed: &mut Editor, p: Pos2, screen: Pos2, secondary: bool) {
        match self.shape_hit(ed, screen) {
            Some(ShapeHit::Handle(index)) => {
                let sh = ed.shape.as_ref().expect("hit implies shape");
                ed.drag = Some(Drag::ShapeEdit { handle: Some(index), grab: p, a: sh.a, b: sh.b });
            }
            Some(ShapeHit::Body) => {
                let sh = ed.shape.as_ref().expect("hit implies shape");
                ed.drag = Some(Drag::ShapeEdit { handle: None, grab: p, a: sh.a, b: sh.b });
            }
            None => {
                self.edit_finalize(ed);
                ed.shape = Some(PendingShape::new(p.round(), secondary));
                ed.drag = Some(Drag::Shape(p.round()));
            }
        }
    }
    pub(crate) fn shape_drag(&mut self, ed: &mut Editor, p: Pos2, shift: bool) {
        let kind = self.settings.shape;
        let line = matches!(kind, ShapeKind::Line | ShapeKind::Arrow);
        let Some(sh) = &mut ed.shape else { return };
        match &ed.drag {
            Some(Drag::Shape(start)) => {
                let b = if shift { constrain(kind, (start.x, start.y), (p.x, p.y)) } else { (p.x, p.y) };
                sh.a = *start;
                sh.b = pos2(b.0, b.1).round();
            }
            Some(Drag::ShapeEdit { handle: Some(i), grab, a, b }) => {
                // 控制点画在外框上（比形状大一圈）：按拖动的距离移动对应的角，不跳到鼠标处
                let start = if line { if *i == 0 { *a } else { *b } } else { super::pointer::handles(Rect::from_two_pos(*a, *b))[*i] };
                let (na, nb) = apply_handle(*a, *b, line, *i, start + (p - *grab), shift);
                (sh.a, sh.b) = (na, nb);
            }
            Some(Drag::ShapeEdit { handle: None, grab, a, b }) => {
                let d = (p - *grab).round();
                (sh.a, sh.b) = (*a + d, *b + d);
            }
            _ => {}
        }
    }

    /// 只点了一下、没拖出形状：不留
    pub(crate) fn shape_release(&mut self, ed: &mut Editor) {
        if ed.shape.as_ref().is_some_and(|s| (s.b - s.a).length() < 2.0) {
            ed.shape = None;
        }
    }

    pub(crate) fn commit_shape(&mut self, ed: &mut Editor) {
        let Some(sh) = ed.shape.take() else { return };
        let shape = sh.shape(&self.settings);
        let (x0, y0, x1, y1) = shape.bounds();
        let r = crate::editor::IRect::around(x0, y0, x1, y1, ed.doc.width(), ed.doc.height());
        let mut saved = crate::editor::history::Saved::default();
        saved.touch(&ed.doc.image, r);
        let changed = shape.composite(ed.doc.image_mut());
        ed.doc.mark(changed);
        ed.doc.commit(saved, false);
        let c = if sh.swap { self.settings.color2 } else { self.settings.color1 };
        self.remember_color(c);
    }

    /// 画还没确认的形状（按屏幕分辨率，与写进图里的画法相同）与控制点
    pub(crate) fn shape_overlay(&mut self, ctx: &egui::Context, painter: &egui::Painter, ed: &mut Editor) {
        let Some(sh) = &ed.shape else { return };
        let shape = sh.shape(&self.settings);
        let ppp = ctx.pixels_per_point();
        let k = self.view.zoom;
        let (x0, y0, x1, y1) = shape.bounds();
        let (s0, s1) = (self.image_to_screen(ed, pos2(x0, y0)), self.image_to_screen(ed, pos2(x1, y1)));
        let vis = Rect::from_min_max(s0, s1).intersect(self.canvas);
        let (a, b) = (self.image_to_screen(ed, sh.a), self.image_to_screen(ed, sh.b));
        if vis.is_positive() {
            // 屏幕物理像素对齐
            let min = pos2((vis.min.x * ppp).floor(), (vis.min.y * ppp).floor());
            let (w, h) = (((vis.max.x * ppp).ceil() - min.x).max(1.0) as u32, ((vis.max.y * ppp).ceil() - min.y).max(1.0) as u32);
            let origin = self.image_to_screen(ed, Pos2::ZERO) * ppp;
            let key = format!("{shape:?}{k}{min:?}{origin:?}{w}{h}");
            let cached = ed.shape.as_ref().and_then(|s| s.preview.as_ref()).filter(|(kk, _, _)| *kk == key).map(|(_, t, r)| (t.clone(), *r));
            let (tex, r) = match cached {
                Some(c) => c,
                None => {
                    let Some(mut pix) = Pixmap::new(w, h) else { return };
                    shape.draw(&mut pix, Transform::from_row(k, 0.0, 0.0, k, origin.x - min.x, origin.y - min.y));
                    let ci = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pix.data());
                    let tex = ctx.load_texture("edit-shape", ci, egui::TextureOptions::NEAREST);
                    let r = Rect::from_min_size(min / ppp, Vec2::new(w as f32, h as f32) / ppp);
                    if let Some(s) = &mut ed.shape {
                        s.preview = Some((key, tex.clone(), r));
                    }
                    (tex, r)
                }
            };
            painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }
        if matches!(self.settings.shape, ShapeKind::Line | ShapeKind::Arrow) {
            for p in [a, b] {
                let r = Rect::from_center_size(p, Vec2::splat(7.0));
                painter.rect_filled(r, 1.0, Color32::WHITE);
                painter.rect_stroke(r, 1.0, egui::Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
            }
        } else {
            // 外框放在轮廓外面一点，不挡住轮廓
            let r = Rect::from_two_pos(a, b).expand(self.settings.shape_width as f32 * self.view.zoom / ppp / 2.0 + 4.0);
            super::canvas::dashed_rect(painter, r);
            super::canvas::paint_handles(painter, r);
        }
    }

    pub(crate) fn shape_props(&mut self, ui: &mut egui::Ui, _ed: &mut Editor) {
        let t = crate::i18n::tr();
        let s = &mut self.settings;
        super::bars::shape_menu(ui, &mut s.shape);
        ui.add_space(10.0);
        if !matches!(s.shape, ShapeKind::Line | ShapeKind::Arrow) {
            crate::ui::widgets::checkbox(ui, &mut s.shape_outline, t.p_outline);
            ui.add_space(6.0);
            crate::ui::widgets::checkbox(ui, &mut s.shape_fill, t.p_fill);
            ui.add_space(10.0);
            if !s.shape_outline && !s.shape_fill {
                s.shape_outline = true;
            }
        }
        if s.shape_outline || matches!(s.shape, ShapeKind::Line | ShapeKind::Arrow) {
            super::props::slider_row(ui, t.p_width, &mut s.shape_width, 1, 100, true, " px");
        }
    }
}