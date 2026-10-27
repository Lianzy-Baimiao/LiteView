//! 编辑模式的鼠标：按下、拖动、松开。每帧把这一帧里的每个移动点都用上（快速移动时笔迹也连成一片）。

use egui::{Event, PointerButton, Pos2, Rect, pos2, vec2};

use super::{Drag, Editor, Tool};
use crate::app::App;
use crate::editor::IRect;
use crate::editor::stroke::{Effect, Stroke, Tip};
use crate::settings::{RedactEffect, RedactMode};

/// 8 个控制点（左上起顺时针）
pub(crate) fn handles(r: Rect) -> [Pos2; 8] {
    let c = r.center();
    [r.left_top(), pos2(c.x, r.top()), r.right_top(), pos2(r.right(), c.y), r.right_bottom(), pos2(c.x, r.bottom()), r.left_bottom(), pos2(r.left(), c.y)]
}

/// 屏幕上 p 附近（6 像素以内）的控制点
pub(crate) fn hit_handle(screen: Rect, p: Pos2) -> Option<usize> {
    handles(screen).iter().position(|h| (*h - p).length() <= 6.0)
}

/// 从 a 拖到 b 的选区（Shift：正方形），取整并限制在图里
pub(crate) fn select_rect(a: Pos2, b: Pos2, square: bool, w: u32, h: u32) -> IRect {
    let mut b = b;
    if square {
        let s = (b.x - a.x).abs().max((b.y - a.y).abs());
        b = pos2(a.x + s * (b.x - a.x).signum(), a.y + s * (b.y - a.y).signum());
    }
    let c = |v: f32, m: u32| v.round().clamp(0.0, m as f32) as u32;
    IRect::new(c(a.x.min(b.x), w), c(a.y.min(b.y), h), c(a.x.max(b.x), w), c(a.y.max(b.y), h))
}

/// 拉控制点 index 到 p（Shift：拉四个角时保持宽高比）
pub(crate) fn resize_rect(from: Rect, index: usize, p: Pos2, keep: bool) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (from.min.x, from.min.y, from.max.x, from.max.y);
    let (px, py) = (p.x.round(), p.y.round());
    match index {
        0 => (x0, y0) = (px, py),
        1 => y0 = py,
        2 => (x1, y0) = (px, py),
        3 => x1 = px,
        4 => (x1, y1) = (px, py),
        5 => y1 = py,
        6 => (x0, y1) = (px, py),
        _ => x0 = px,
    }
    let mut r = Rect::from_two_pos(pos2(x0, y0), pos2(x1, y1));
    if keep && index.is_multiple_of(2) && from.height() > 0.0 {
        let ratio = from.width() / from.height();
        let anchor = handles(from)[(index + 4) % 8];
        let w = r.width().max(r.height() * ratio);
        let dir = vec2(if px >= anchor.x { 1.0 } else { -1.0 }, if py >= anchor.y { 1.0 } else { -1.0 });
        r = Rect::from_two_pos(anchor, anchor + vec2(w * dir.x, (w / ratio).round() * dir.y));
    }
    r.max.x = r.max.x.max(r.min.x + 1.0);
    r.max.y = r.max.y.max(r.min.y + 1.0);
    r
}
impl App {
    pub(crate) fn edit_pointer(&mut self, ctx: &egui::Context, resp: &egui::Response, ed: &mut Editor) {
        let hover = ctx.input(|i| i.pointer.hover_pos());
        ed.cursor = hover.filter(|_| resp.hovered()).map(|p| self.screen_to_image(ed, p));
        let space = ctx.input(|i| i.key_down(egui::Key::Space)) && !ctx.egui_wants_keyboard_input();
        let shift = ctx.input(|i| i.modifiers.shift);
        let blocked = ed.popup_was_open || ed.saving;
        let events: Vec<Event> = ctx.input(|i| i.events.clone());
        for e in events {
            match e {
                Event::PointerButton { pos, button, pressed: true, .. } if ed.drag.is_none() && resp.hovered() && !blocked => {
                    let p = self.screen_to_image(ed, pos);
                    match button {
                        PointerButton::Middle => ed.drag = Some(Drag::Pan),
                        PointerButton::Primary if space => ed.drag = Some(Drag::Pan),
                        PointerButton::Primary => self.press(ed, p, pos, false),
                        PointerButton::Secondary if !matches!(ed.tool, Tool::Select | Tool::Redact) => self.press(ed, p, pos, true),
                        _ => {}
                    }
                }
                Event::PointerMoved(pos) if ed.drag.is_some() => {
                    let p = self.screen_to_image(ed, pos);
                    self.drag_to(ed, p, shift);
                }
                Event::PointerButton { pressed: false, .. } if ed.drag.is_some() => self.release(ed),
                _ => {}
            }
        }
        if matches!(ed.drag, Some(Drag::Pan)) {
            self.view.offset += ctx.input(|i| i.pointer.delta());
            self.view.fitted = false;
        }
        // 松开时鼠标在窗口外：没有收到松开事件
        if ed.drag.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            self.release(ed);
        }
    }

    /// 按下：screen 是屏幕坐标（判断控制点用），secondary = 右键（用辅色画）
    fn press(&mut self, ed: &mut Editor, p: Pos2, screen: Pos2, secondary: bool) {
        match ed.tool {
            Tool::Select => {
                if let Some(f) = &ed.floating {
                    let sr = Rect::from_min_max(self.image_to_screen(ed, f.rect.min), self.image_to_screen(ed, f.rect.max));
                    if let Some(index) = hit_handle(sr, screen) {
                        ed.drag = Some(Drag::Handle { index, from: f.rect });
                        return;
                    }
                    if f.rect.contains(p) {
                        ed.drag = Some(Drag::Move { grab: p, from: f.rect });
                        return;
                    }
                    self.commit_floating(ed);
                }
                if let Some(s) = ed.sel {
                    let r = super::ops::irect_to_rect(s);
                    let sr = Rect::from_min_max(self.image_to_screen(ed, r.min), self.image_to_screen(ed, r.max));
                    let handle = hit_handle(sr, screen);
                    if handle.is_some() || r.contains(p) {
                        self.lift(ed);
                        ed.drag = Some(match handle {
                            Some(index) => Drag::Handle { index, from: r },
                            None => Drag::Move { grab: p, from: r },
                        });
                        return;
                    }
                }
                ed.sel = None;
                ed.drag = Some(Drag::Select(p));
            }
            Tool::Redact => {
                self.edit_finalize(ed);
                let (block, blur, smear, size) = ed.redact_params(&self.settings);
                let effect = match self.settings.redact_effect {
                    RedactEffect::Mosaic => Effect::Mosaic(block),
                    RedactEffect::Blur => Effect::Blur(blur),
                    RedactEffect::Smear => Effect::Smear(smear),
                };
                match self.settings.redact_mode {
                    RedactMode::Brush => {
                        let mut s = Stroke::brush(effect, size as f32, Tip::Round);
                        let r = s.line_to(ed.doc.image_mut(), p.x, p.y);
                        ed.doc.mark(r);
                        ed.drag = Some(Drag::Stroke(s));
                    }
                    RedactMode::Rect => ed.drag = Some(Drag::RedactRect(p, Stroke::rect(effect))),
                }
            }
            Tool::Pencil | Tool::Brush | Tool::Eraser => {
                self.edit_finalize(ed);
                let color = if secondary { self.settings.color2 } else { self.settings.color1 };
                if ed.tool != Tool::Eraser {
                    self.remember_color(color);
                }
                let s = &self.settings;
                let (effect, size, tip) = match ed.tool {
                    Tool::Pencil => (Effect::Paint(color), s.pencil_size as f32, Tip::Hard),
                    Tool::Brush if s.highlighter => (Effect::Paint([color[0], color[1], color[2], 96]), s.paint_size as f32, Tip::Round),
                    Tool::Brush => (Effect::Paint([color[0], color[1], color[2], (s.paint_opacity.min(100) * 255 / 100) as u8]), s.paint_size as f32, Tip::Round),
                    _ if s.eraser_transparent && ed.src.canvas_alpha => (Effect::Erase, s.eraser_size as f32, Tip::Hard),
                    _ => (Effect::Paint(s.color2), s.eraser_size as f32, Tip::Hard),
                };
                let mut st = Stroke::brush(effect, size, tip);
                let r = st.line_to(ed.doc.image_mut(), p.x, p.y);
                ed.doc.mark(r);
                ed.drag = Some(Drag::Stroke(st));
            }
            Tool::Fill => self.fill_at(ed, p, secondary),
            Tool::Picker => self.pick_at(ed, p, secondary),
            Tool::Shape => self.shape_press(ed, p, screen, secondary),
            Tool::Text => self.text_press(ed, p, screen, secondary),
        }
    }
    fn drag_to(&mut self, ed: &mut Editor, p: Pos2, shift: bool) {
        let (w, h) = (ed.doc.width(), ed.doc.height());
        if matches!(ed.drag, Some(Drag::Shape(_) | Drag::ShapeEdit { .. })) {
            self.shape_drag(ed, p, shift);
            return;
        }
        if matches!(ed.drag, Some(Drag::TextMove { .. } | Drag::TextWidth { .. })) {
            self.text_drag(ed, p);
            return;
        }
        match &mut ed.drag {
            Some(Drag::Select(a)) => {
                let r = select_rect(*a, p, shift, w, h);
                ed.sel = (!r.is_empty()).then_some(r);
            }
            Some(Drag::Move { grab, from }) => {
                let d = (p - *grab).round();
                if let Some(f) = &mut ed.floating {
                    f.rect = from.translate(d);
                }
            }
            Some(Drag::Handle { index, from }) => {
                let r = resize_rect(*from, *index, p, shift);
                if let Some(f) = &mut ed.floating {
                    f.rect = r;
                }
            }
            Some(Drag::Stroke(s)) => {
                let r = s.line_to(std::sync::Arc::make_mut(&mut ed.doc.image), p.x, p.y);
                ed.doc.mark(r);
            }
            Some(Drag::RedactRect(a, s)) => {
                let r = select_rect(*a, p, shift, w, h);
                for part in s.set_rect(std::sync::Arc::make_mut(&mut ed.doc.image), r) {
                    ed.doc.mark(part);
                }
            }
            Some(Drag::Shape(_) | Drag::ShapeEdit { .. } | Drag::TextMove { .. } | Drag::TextWidth { .. } | Drag::Pan) | None => {}
        }
    }

    fn release(&mut self, ed: &mut Editor) {
        match ed.drag.take() {
            Some(d @ (Drag::Stroke(_) | Drag::RedactRect(..))) => {
                ed.drag = Some(d);
                self.commit_stroke(ed);
            }
            // 只点了一下：不留选区
            Some(Drag::Select(_)) => ed.sel = ed.sel.filter(|s| s.width() > 1 && s.height() > 1),
            Some(Drag::Shape(_)) => self.shape_release(ed),
            // 拖完文本框接着输入
            Some(Drag::TextMove { .. } | Drag::TextWidth { .. }) => {
                if let Some(t) = &mut ed.text {
                    t.focus = true;
                }
            }
            _ => {}
        }
    }

    /// 颜料桶：主色（右键辅色）填充相近颜色的连通区域
    fn fill_at(&mut self, ed: &mut Editor, p: Pos2, secondary: bool) {
        self.edit_finalize(ed);
        let (x, y) = (p.x.floor(), p.y.floor());
        if x < 0.0 || y < 0.0 || x >= ed.doc.width() as f32 || y >= ed.doc.height() as f32 {
            return;
        }
        let color = if secondary { self.settings.color2 } else { self.settings.color1 };
        self.remember_color(color);
        let tol = self.settings.fill_tolerance.min(255) as u8;
        let (bits, r) = crate::editor::ops::flood(&ed.doc.image, x as u32, y as u32, tol);
        let mut saved = crate::editor::history::Saved::default();
        saved.touch(&ed.doc.image, r);
        crate::editor::ops::fill_bits(ed.doc.image_mut(), &bits, r, color);
        ed.doc.mark(r);
        ed.doc.commit(saved, false);
    }

    /// 取色器：左键取主色、右键取辅色，然后回到之前的工具
    fn pick_at(&mut self, ed: &mut Editor, p: Pos2, secondary: bool) {
        let (x, y) = (p.x.floor(), p.y.floor());
        if x < 0.0 || y < 0.0 || x >= ed.doc.width() as f32 || y >= ed.doc.height() as f32 {
            return;
        }
        let mut c = ed.doc.image.get_pixel(x as u32, y as u32).0;
        c[3] = 255;
        if secondary {
            self.settings.color2 = c;
        } else {
            self.settings.color1 = c;
        }
        let back = if ed.prev_tool == Tool::Picker { Tool::Pencil } else { ed.prev_tool };
        self.switch_tool(ed, back);
    }
}
