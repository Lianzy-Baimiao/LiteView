//! 编辑模式的画布：图片、选区与控制点、浮动选区、打码画笔的圆圈光标；右键菜单；光标形状。

use egui::{Color32, CursorIcon, Painter, Pos2, Rect, Sense, Shape, Stroke, Vec2, vec2};

use super::pointer::{handles, hit_handle};
use super::{Drag, Editor, Tool};
use crate::app::App;
use crate::i18n::tr;
use crate::ui::theme;
use crate::viewer::texture::paint_checker;
use crate::viewer::view::{View, WHEEL_STEP, snap};

/// 黑白两色的虚线框：在任何背景上都看得见
pub(crate) fn dashed_rect(painter: &Painter, r: Rect) {
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    painter.add(Shape::line(pts.to_vec(), Stroke::new(1.0, Color32::WHITE)));
    painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, Color32::BLACK), 4.0, 4.0));
}

pub(crate) fn paint_handles(painter: &Painter, r: Rect) {
    for h in handles(r) {
        let b = Rect::from_center_size(h, Vec2::splat(7.0));
        painter.rect_filled(b, 1.0, Color32::WHITE);
        painter.rect_stroke(b, 1.0, Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
    }
}

/// 黑白双色描边的圆圈
fn ring(painter: &Painter, c: Pos2, r: f32) {
    painter.circle_stroke(c, r + 0.75, Stroke::new(1.5, Color32::from_black_alpha(200)));
    painter.circle_stroke(c, r - 0.25, Stroke::new(1.0, Color32::WHITE));
}

/// 浮动选区的纹理（超过显卡上限时先缩小）
fn floating_texture(ctx: &egui::Context, img: &image::RgbaImage) -> egui::TextureHandle {
    let max = ctx.input(|i| i.max_texture_side) as u32;
    let mut small: Option<image::RgbaImage> = None;
    while let Some(s) = small.as_ref().or(Some(img)).filter(|s| s.width() > max || s.height() > max) {
        small = Some(crate::decode::halve(s));
    }
    let src = small.as_ref().unwrap_or(img);
    ctx.load_texture("edit-floating", crate::viewer::texture::region(src, 0, 0, src.width(), src.height()), egui::TextureOptions::LINEAR)
}

impl App {
    /// 当前工具的圆圈直径（图片像素）；不是画笔类工具时 None
    fn brush_diameter(&self, ed: &Editor) -> Option<f32> {
        let s = &self.settings;
        Some(match ed.tool {
            Tool::Redact if s.redact_mode == crate::settings::RedactMode::Brush => ed.redact_params(s).2 as f32,
            Tool::Pencil => s.pencil_size as f32,
            Tool::Brush => s.paint_size as f32,
            Tool::Eraser => s.eraser_size as f32,
            _ => return None,
        })
    }

    pub(crate) fn edit_canvas(&mut self, ui: &mut egui::Ui, ed: &mut Editor) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, p.canvas);
        let dims = (ed.doc.width(), ed.doc.height());
        if dims != ed.dims {
            ed.dims = dims;
            self.shown = Some(crate::app::Shown { width: dims.0, height: dims.1, has_alpha: true });
            if self.view.fitted {
                self.pending_reset = true;
            }
        }
        let size = vec2(dims.0 as f32, dims.1 as f32);
        let img_rect = self.update_view(&ctx, rect, size, crate::settings::FitMode::Fit);
        paint_checker(&ctx, ui.painter(), img_rect, p.checker);
        ed.tex.sync(&mut ed.doc);
        let nearest = self.settings.nearest_above as f32 / 100.0;
        let settled = !self.view.animating();
        ed.tex.paint(&ctx, ui.painter(), &ed.doc.image, img_rect, self.view.zoom, nearest, settled);
        let resp = ui.interact(rect, ui.id().with("edit-canvas"), Sense::click_and_drag());
        self.edit_wheel(&ctx, &resp, size);
        self.edit_pointer(&ctx, &resp, ed);
        self.edit_overlay(&ctx, ui.painter(), ed);
        self.edit_cursor(&ctx, &resp, ed);
        if matches!(ed.tool, Tool::Select | Tool::Redact) && ed.drag.is_none() {
            resp.context_menu(|ui| self.edit_menu(ui, ed));
        }
        if !self.first_paint_logged {
            self.first_paint_logged = true;
        }
    }

    /// 滚轮缩放（编辑时滚轮总是缩放）
    fn edit_wheel(&mut self, ctx: &egui::Context, resp: &egui::Response, size: Vec2) {
        if !resp.hovered() {
            return;
        }
        let mut factor = 1.0f32;
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::MouseWheel { unit, delta, .. } => {
                        let lines = match unit {
                            egui::MouseWheelUnit::Point => delta.y / 50.0,
                            egui::MouseWheelUnit::Line => delta.y,
                            egui::MouseWheelUnit::Page => delta.y * 10.0,
                        };
                        factor *= WHEEL_STEP.powf(lines);
                    }
                    egui::Event::Zoom(z) => factor *= z,
                    _ => {}
                }
            }
        });
        if (factor - 1.0).abs() > 1e-4 {
            let anchor = ctx.input(|i| i.pointer.hover_pos()).unwrap_or(self.canvas.center());
            let fit = View::fit_zoom(crate::settings::FitMode::Fit, self.canvas.size(), ctx.pixels_per_point(), size);
            let from = self.view.target_zoom();
            let to = snap(from, from * factor, &[1.0, fit]);
            self.view.zoom_to(to, anchor, self.settings.smooth_zoom, self.canvas);
            ctx.request_repaint();
        }
    }

    /// 叠加层：浮动选区、选区、框选打码的框、圆圈光标、调整大小时的提示
    fn edit_overlay(&mut self, ctx: &egui::Context, painter: &Painter, ed: &mut Editor) {
        if let Some(f) = &mut ed.floating
            && f.tex.is_none()
        {
            f.tex = Some(floating_texture(ctx, &f.image));
        }
        let to_screen = |app: &Self, r: Rect| Rect::from_min_max(app.image_to_screen(ed, r.min), app.image_to_screen(ed, r.max));
        if let Some(f) = &ed.floating
            && let Some(tex) = &f.tex
        {
            let sr = to_screen(self, f.rect);
            painter.image(tex.id(), sr, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            dashed_rect(painter, sr);
            paint_handles(painter, sr);
        } else if let Some(s) = ed.sel {
            let sr = to_screen(self, super::ops::irect_to_rect(s));
            dashed_rect(painter, sr);
            if ed.tool == Tool::Select {
                paint_handles(painter, sr);
            }
        }
        if let Some(Drag::RedactRect(a, _)) = &ed.drag
            && let Some(c) = ed.cursor
        {
            let r = super::pointer::select_rect(*a, c, false, ed.doc.width(), ed.doc.height());
            dashed_rect(painter, to_screen(self, super::ops::irect_to_rect(r)));
        }
        self.shape_overlay(ctx, painter, ed);
        self.text_ui(ctx, painter, ed);
        let k = self.view.zoom / ctx.pixels_per_point();
        if let (Some(d), Some(c)) = (self.brush_diameter(ed), ed.cursor) {
            let r = (d * k / 2.0).max(1.0);
            let center = self.image_to_screen(ed, c);
            ring(painter, center, r);
            if r < 4.0 {
                painter.line_segment([center - vec2(6.0, 0.0), center + vec2(6.0, 0.0)], Stroke::new(1.0, Color32::from_black_alpha(160)));
                painter.line_segment([center - vec2(0.0, 6.0), center + vec2(0.0, 6.0)], Stroke::new(1.0, Color32::from_black_alpha(160)));
            }
        }
        if let (Some(at), Some(d)) = (ed.size_hint, self.brush_diameter(ed)) {
            let age = at.elapsed().as_secs_f32();
            if age < 0.8 {
                let c = self.canvas.center();
                ring(painter, c, (d * k / 2.0).max(1.0));
                let p = theme::palette(ctx);
                painter.text(c, egui::Align2::CENTER_CENTER, format!("{d:.0} px"), egui::FontId::proportional(14.0), p.text);
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            } else {
                ed.size_hint = None;
            }
        }
        if ed.saving {
            let p = theme::palette(ctx);
            let c = self.canvas.center();
            painter.text(c, egui::Align2::CENTER_CENTER, tr().saving, egui::FontId::proportional(15.0), p.text);
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn edit_cursor(&mut self, ctx: &egui::Context, resp: &egui::Response, ed: &Editor) {
        if !resp.hovered() && ed.drag.is_none() {
            return;
        }
        let pos = ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
        let icon = match &ed.drag {
            Some(Drag::Pan) => CursorIcon::Grabbing,
            Some(Drag::Move { .. }) => CursorIcon::Move,
            Some(Drag::Handle { index, .. }) => handle_cursor(*index),
            Some(Drag::ShapeEdit { handle: None, .. } | Drag::TextMove { .. }) => CursorIcon::Move,
            Some(Drag::TextWidth { .. }) => CursorIcon::ResizeHorizontal,
            _ if ctx.input(|i| i.key_down(egui::Key::Space)) => CursorIcon::Grab,
            _ if self.brush_diameter(ed).is_some() => CursorIcon::None,
            _ if ed.tool == Tool::Shape => match self.shape_hit(ed, pos) {
                Some(super::paint::ShapeHit::Handle(i)) if !matches!(self.settings.shape, crate::settings::ShapeKind::Line | crate::settings::ShapeKind::Arrow) => handle_cursor(i),
                Some(super::paint::ShapeHit::Handle(_)) => CursorIcon::Crosshair,
                Some(super::paint::ShapeHit::Body) => CursorIcon::Move,
                None => CursorIcon::Crosshair,
            },
            _ => match ed.tool {
                Tool::Select => {
                    let float = ed.floating.as_ref().map(|f| f.rect).or(ed.sel.map(super::ops::irect_to_rect));
                    match float {
                        Some(r) => {
                            let sr = Rect::from_min_max(self.image_to_screen(ed, r.min), self.image_to_screen(ed, r.max));
                            match hit_handle(sr, pos) {
                                Some(i) => handle_cursor(i),
                                None if sr.contains(pos) => CursorIcon::Move,
                                None => CursorIcon::Crosshair,
                            }
                        }
                        None => CursorIcon::Crosshair,
                    }
                }
                Tool::Text => CursorIcon::Text,
                _ => CursorIcon::Crosshair,
            },
        };
        ctx.set_cursor_icon(icon);
    }

    /// 选择、打码工具下的右键菜单
    fn edit_menu(&mut self, ui: &mut egui::Ui, ed: &mut Editor) {
        ui.set_min_width(200.0);
        let t = tr();
        let c = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
        let has_sel = ed.sel.is_some() || ed.floating.is_some();
        let item = |ui: &mut egui::Ui, text: &str, sc: String, on: bool| ui.add_enabled(on, egui::Button::new(text).shortcut_text(sc)).clicked();
        if item(ui, t.copy_image, format!("{c}C"), true) {
            self.edit_copy(ed);
            ui.close();
        }
        if item(ui, t.cut, format!("{c}X"), has_sel) {
            self.edit_cut(ed);
            ui.close();
        }
        if item(ui, t.paste, format!("{c}V"), true) {
            self.edit_paste();
            ui.close();
        }
        ui.separator();
        if item(ui, t.t_crop, format!("{c}Shift+X"), has_sel) {
            self.edit_crop(ed);
            ui.close();
        }
        if item(ui, t.p_select_all, format!("{c}A"), true) {
            self.edit_select_all(ed);
            ui.close();
        }
        if item(ui, t.delete_sel, "Del".into(), has_sel) {
            self.edit_delete(ed);
            ui.close();
        }
        ui.separator();
        if item(ui, t.undo, format!("{c}Z"), ed.doc.can_undo() || ed.floating.is_some()) {
            self.edit_undo(ed);
            ui.close();
        }
    }

    /// [ ] 调整当前工具的大小（约 10%），在画布中央短暂显示
    pub(crate) fn adjust_size(&mut self, ed: &mut Editor, up: bool) {
        let step = |v: u32, lo: u32, hi: u32| {
            let d = (v as f32 * 0.1).round().max(1.0) as u32;
            if up { (v + d).min(hi) } else { v.saturating_sub(d).max(lo) }
        };
        let s = &mut self.settings;
        match ed.tool {
            Tool::Redact => {
                let cur = ed.redact_params(s).2;
                s.brush_size = Some(step(cur, 4, 1000));
            }
            Tool::Pencil => s.pencil_size = step(s.pencil_size, 1, 100),
            Tool::Brush => s.paint_size = step(s.paint_size, 1, 100),
            Tool::Eraser => s.eraser_size = step(s.eraser_size, 2, 400),
            Tool::Shape => s.shape_width = step(s.shape_width, 1, 100),
            Tool::Text => s.text_size = step(s.text_size, 6, 400),
            _ => return,
        }
        ed.size_hint = Some(std::time::Instant::now());
    }
}

fn handle_cursor(i: usize) -> CursorIcon {
    match i {
        0 | 4 => CursorIcon::ResizeNwSe,
        2 | 6 => CursorIcon::ResizeNeSw,
        1 | 5 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeHorizontal,
    }
}
