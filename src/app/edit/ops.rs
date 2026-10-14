//! 编辑操作：放下浮动选区、撤销重做、复制剪切删除粘贴、裁剪、调整大小、旋转翻转、换工具。

use std::sync::Arc;

use egui::{Pos2, Rect, pos2, vec2};
use image::RgbaImage;

use super::{Drag, Editor, Floating, Tool};
use crate::app::{App, Dialog};
use crate::editor::history::Saved;
use crate::editor::{IRect, ops};
use crate::i18n::tr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Xform {
    Cw,
    Ccw,
    R180,
    FlipH,
    FlipV,
}

pub(crate) fn irect_to_rect(r: IRect) -> Rect {
    Rect::from_min_max(pos2(r.x0 as f32, r.y0 as f32), pos2(r.x1 as f32, r.y1 as f32))
}

/// 浮动选区放下时的尺寸
fn float_size(f: &Floating) -> (u32, u32) {
    (f.rect.width().round().max(1.0) as u32, f.rect.height().round().max(1.0) as u32)
}

/// 浮动选区按当前大小的像素
fn float_pixels(f: &Floating) -> Arc<RgbaImage> {
    let (w, h) = float_size(f);
    if (w, h) == f.image.dimensions() { f.image.clone() } else { Arc::new(crate::editor::resample::resize_image(&f.image, w, h)) }
}

impl App {
    /// 把进行中的东西定下来（保存、复制、裁剪、换工具之前）：涂抹、浮动选区、形状、文字
    pub(crate) fn edit_finalize(&mut self, ed: &mut Editor) {
        self.commit_stroke(ed);
        self.commit_floating(ed);
        self.commit_shape(ed);
        self.commit_text(ed);
    }

    /// 涂完一笔 / 框完一个框：记成一步
    pub(crate) fn commit_stroke(&mut self, ed: &mut Editor) {
        match ed.drag.take() {
            Some(Drag::Stroke(s) | Drag::RedactRect(_, s)) => {
                let redact = s.effect.is_redact();
                ed.doc.commit(s.finish(), redact);
            }
            other => ed.drag = other,
        }
    }

    /// 放下浮动选区（叠在图上），与拿起时清空原处一起记成一步
    pub(crate) fn commit_floating(&mut self, ed: &mut Editor) {
        let Some(f) = ed.floating.take() else { return };
        let img = float_pixels(&f);
        let (w, h) = (i64::from(ed.doc.width()), i64::from(ed.doc.height()));
        let (x, y) = (f.rect.min.x.round() as i64, f.rect.min.y.round() as i64);
        let c = |v: i64, m: i64| v.clamp(0, m) as u32;
        let dest = IRect::new(c(x, w), c(y, h), c(x + i64::from(img.width()), w), c(y + i64::from(img.height()), h));
        let mut saved = f.saved;
        saved.touch(&ed.doc.image, dest);
        let changed = ops::composite(ed.doc.image_mut(), &img, x, y);
        ed.doc.mark(changed);
        ed.doc.commit(saved, false);
        ed.sel = (!dest.is_empty()).then_some(dest);
    }

    /// 拿起选区里的像素（变成浮动选区），原处清空
    pub(crate) fn lift(&mut self, ed: &mut Editor) {
        let Some(r) = ed.sel else { return };
        let image = Arc::new(ops::extract(&ed.doc.image, r));
        let mut saved = Saved::default();
        saved.touch(&ed.doc.image, r);
        let color = ed.clear_color(&self.settings);
        ops::fill_rect(ed.doc.image_mut(), r, color);
        ed.doc.mark(r);
        ed.floating = Some(Floating { image, rect: irect_to_rect(r), saved, tex: None });
    }

    /// 撤销拿起 / 粘贴：恢复原样
    fn drop_floating(&mut self, ed: &mut Editor) {
        if let Some(f) = ed.floating.take() {
            let d = f.saved.restore(ed.doc.image_mut());
            ed.doc.mark(d);
        }
    }

    pub(crate) fn edit_undo(&mut self, ed: &mut Editor) {
        if ed.drag.is_some() {
            return;
        }
        if ed.floating.is_some() {
            self.drop_floating(ed);
            ed.sel = None;
            return;
        }
        if ed.shape.take().is_some() | ed.text.take().is_some() {
            return;
        }
        if ed.doc.undo() {
            self.after_history(ed);
        }
    }

    pub(crate) fn edit_redo(&mut self, ed: &mut Editor) {
        if ed.drag.is_some() {
            return;
        }
        self.edit_finalize(ed);
        if ed.doc.redo() {
            self.after_history(ed);
        }
    }

    fn after_history(&mut self, ed: &mut Editor) {
        let full = ed.doc.full();
        ed.sel = ed.sel.map(|s| s.intersect(&full)).filter(|s| !s.is_empty());
    }

    /// 复制：浮动选区 / 选区 / 整张图（整张时记下，剪贴板来的图退出时不再询问）
    pub(crate) fn edit_copy(&mut self, ed: &mut Editor) {
        self.commit_shape(ed);
        self.commit_text(ed);
        let img: RgbaImage = if let Some(f) = &ed.floating {
            (*float_pixels(f)).clone()
        } else if let Some(r) = ed.sel {
            ops::extract(&ed.doc.image, r)
        } else {
            ed.copied_rev = Some(ed.doc.rev);
            (*ed.doc.image).clone()
        };
        self.copy_rgba(img);
    }

    /// 整张图复制到剪贴板（「复制到剪贴板并退出」）
    pub(crate) fn edit_copy_all(&mut self, ed: &mut Editor) {
        ed.copied_rev = Some(ed.doc.rev);
        self.copy_rgba((*ed.doc.image).clone());
    }

    pub(crate) fn edit_cut(&mut self, ed: &mut Editor) {
        if ed.floating.is_some() || ed.sel.is_some() {
            self.edit_copy(ed);
            self.edit_delete(ed);
        }
    }

    /// 删除选区：有透明通道时变透明，否则填辅色
    pub(crate) fn edit_delete(&mut self, ed: &mut Editor) {
        if let Some(f) = ed.floating.take() {
            // 拿起的：原处清空记成一步；粘贴的：直接丢掉
            ed.doc.commit(f.saved, false);
            ed.sel = None;
            return;
        }
        let Some(r) = ed.sel.take() else { return };
        let mut saved = Saved::default();
        saved.touch(&ed.doc.image, r);
        let color = ed.clear_color(&self.settings);
        ops::fill_rect(ed.doc.image_mut(), r, color);
        ed.doc.mark(r);
        ed.doc.commit(saved, false);
    }

    /// 粘贴（剪贴板 / 拖进来的图）：放成浮动选区；比画布大时先问要不要扩大画布
    pub(crate) fn edit_paste_image(&mut self, ed: &mut Editor, img: RgbaImage) {
        self.edit_finalize(ed);
        if img.width() > ed.doc.width() || img.height() > ed.doc.height() {
            self.dialog = Some(Dialog::ExpandCanvas { image: Arc::new(img) });
            return;
        }
        self.place_floating(ed, Arc::new(img), false);
    }

    /// 放下粘贴的图：at_origin 时放在左上角，否则放在看得见的区域中间
    pub(crate) fn place_floating(&mut self, ed: &mut Editor, image: Arc<RgbaImage>, at_origin: bool) {
        let (w, h) = (image.width() as f32, image.height() as f32);
        let (dw, dh) = (ed.doc.width() as f32, ed.doc.height() as f32);
        let min = if at_origin {
            Pos2::ZERO
        } else {
            let c = self.screen_to_image(ed, self.canvas.center());
            pos2((c.x - w / 2.0).clamp(0.0, (dw - w).max(0.0)).round(), (c.y - h / 2.0).clamp(0.0, (dh - h).max(0.0)).round())
        };
        ed.floating = Some(Floating { image, rect: Rect::from_min_size(min, vec2(w, h)), saved: Saved::default(), tex: None });
        ed.sel = None;
        ed.tool = Tool::Select;
    }

    /// 扩大画布以放下粘贴的图（左上角不动）
    pub(crate) fn expand_and_paste(&mut self, ed: &mut Editor, image: Arc<RgbaImage>) {
        let (ow, oh) = (ed.doc.width() as f32, ed.doc.height() as f32);
        let (nw, nh) = (image.width().max(ed.doc.width()), image.height().max(ed.doc.height()));
        let fill = ed.clear_color(&self.settings);
        let big = ops::expand(&ed.doc.image, nw, nh, fill);
        ed.doc.replace(big);
        let k = self.view.zoom / self.ctx.pixels_per_point();
        self.view.offset += vec2(nw as f32 - ow, nh as f32 - oh) * k / 2.0;
        self.place_floating(ed, image, true);
    }

    /// 裁剪到选区（Ctrl+Shift+X）：保持选区在屏幕上的位置
    pub(crate) fn edit_crop(&mut self, ed: &mut Editor) {
        self.edit_finalize(ed);
        let Some(r) = ed.sel.take() else {
            self.show_toast(tr().need_selection);
            self.switch_tool(ed, Tool::Select);
            return;
        };
        if r == ed.doc.full() {
            return;
        }
        let k = self.view.zoom / self.ctx.pixels_per_point();
        let (cx, cy) = ((r.x0 + r.x1) as f32 / 2.0, (r.y0 + r.y1) as f32 / 2.0);
        self.view.offset += vec2(cx - ed.doc.width() as f32 / 2.0, cy - ed.doc.height() as f32 / 2.0) * k;
        let img = ops::extract(&ed.doc.image, r);
        ed.doc.replace(img);
    }

    /// 调整大小（高质量重采样）
    pub(crate) fn edit_resize(&mut self, ed: &mut Editor, w: u32, h: u32) {
        self.edit_finalize(ed);
        let (w, h) = (w.clamp(1, crate::decode::MAX_SIDE), h.clamp(1, crate::decode::MAX_SIDE));
        if (w, h) == (ed.doc.width(), ed.doc.height()) {
            return;
        }
        let img = crate::editor::resample::resize_image(&ed.doc.image, w, h);
        ed.doc.replace(img);
        ed.sel = None;
    }

    pub(crate) fn edit_transform(&mut self, ed: &mut Editor, x: Xform) {
        self.edit_finalize(ed);
        let src = &ed.doc.image;
        let img = match x {
            Xform::Cw => ops::rotate90(src),
            Xform::Ccw => ops::rotate270(src),
            Xform::R180 => ops::rotate180(src),
            Xform::FlipH => ops::flip_h(src),
            Xform::FlipV => ops::flip_v(src),
        };
        ed.doc.replace(img);
        ed.sel = None;
    }

    pub(crate) fn switch_tool(&mut self, ed: &mut Editor, tool: Tool) {
        if ed.tool == tool {
            return;
        }
        self.edit_finalize(ed);
        if tool != Tool::Select {
            ed.sel = None;
        }
        if tool == Tool::Picker {
            ed.prev_tool = ed.tool;
        }
        if tool == Tool::Text {
            // 系统字体在后台先加载好
            crate::decode::extra::preload_fontdb();
        }
        ed.tool = tool;
    }

    pub(crate) fn edit_select_all(&mut self, ed: &mut Editor) {
        self.edit_finalize(ed);
        ed.tool = Tool::Select;
        ed.sel = Some(ed.doc.full());
    }

    /// 屏幕坐标 → 图片像素坐标（编辑时没有旋转）
    pub(crate) fn screen_to_image(&self, ed: &Editor, p: Pos2) -> Pos2 {
        let size = vec2(ed.doc.width() as f32, ed.doc.height() as f32);
        self.view.to_image(self.canvas, self.ctx.pixels_per_point(), size, p)
    }

    /// 图片像素坐标 → 屏幕坐标
    pub(crate) fn image_to_screen(&self, ed: &Editor, p: Pos2) -> Pos2 {
        let ppp = self.ctx.pixels_per_point();
        let size = vec2(ed.doc.width() as f32, ed.doc.height() as f32);
        let r = self.view.image_rect(self.canvas, ppp, size);
        r.min + p.to_vec2() * self.view.zoom / ppp
    }
}
