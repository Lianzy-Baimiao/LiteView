//! 编辑工具栏（只有图标，悬停提示「名称 + 快捷键」）：撤销重做 ｜ 选择 裁剪 调整大小 旋转 ｜ 画图工具 ｜ 打码 ｜ 颜色 ｜ 取消 完成。

use egui::containers::menu::MenuButton;
use egui::{Align, Layout, Popup, vec2};

use super::ops::Xform;
use super::{Editor, SaveThen, Tool};
use crate::app::App;
use crate::i18n::tr;
use crate::settings::ShapeKind;
use crate::ui::icons::Icon;
use crate::ui::widgets;

impl App {
    pub(crate) fn edit_toolbar(&mut self, ui: &mut egui::Ui, ed: &mut Editor) {
        let t = tr();
        let c = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
        ui.spacing_mut().item_spacing.x = 2.0;
        let on = !ed.saving;
        if widgets::icon_button(ui, Icon::Undo, t.undo, &format!("{c}Z"), on && (ed.doc.can_undo() || ed.floating.is_some())).clicked() {
            self.edit_undo(ed);
        }
        if widgets::icon_button(ui, Icon::Redo, t.redo, &format!("{c}Y"), on && ed.doc.can_redo()).clicked() {
            self.edit_redo(ed);
        }
        widgets::separator(ui);
        let tool = |app: &mut Self, ui: &mut egui::Ui, ed: &mut Editor, icon: Icon, tip: &str, key: &str, tl: Tool| {
            if widgets::icon_button_ex(ui, icon, tip, key, on, ed.tool == tl, None).clicked() {
                app.switch_tool(ed, tl);
            }
        };
        tool(self, ui, ed, Icon::Select, t.t_select, "S", Tool::Select);
        if widgets::icon_button(ui, Icon::Crop, t.t_crop, &format!("C  {c}Shift+X"), on).clicked() {
            self.edit_crop(ed);
        }
        if widgets::icon_button(ui, Icon::Resize, t.t_resize, &format!("{c}W"), on).clicked() {
            self.open_resize(ed);
        }
        let rot = widgets::icon_button(ui, Icon::RotateCw, t.t_rotate, "", on);
        Popup::menu(&rot).show(|ui| {
            ui.set_min_width(180.0);
            for (x, label, sc) in [
                (Xform::Cw, t.rotate_cw, format!("{c}R")),
                (Xform::Ccw, t.rotate_ccw, format!("{c}Shift+R")),
                (Xform::R180, t.rotate_180, String::new()),
                (Xform::FlipH, t.flip_h, String::new()),
                (Xform::FlipV, t.flip_v, String::new()),
            ] {
                if ui.add(egui::Button::new(label).shortcut_text(sc)).clicked() {
                    self.edit_transform(ed, x);
                    ui.close();
                }
            }
        });
        widgets::separator(ui);
        tool(self, ui, ed, Icon::Pencil, t.t_pencil, "P", Tool::Pencil);
        tool(self, ui, ed, Icon::Brush, t.t_brush, "B", Tool::Brush);
        tool(self, ui, ed, Icon::Eraser, t.t_eraser, "E", Tool::Eraser);
        tool(self, ui, ed, Icon::Fill, t.t_fill, "G", Tool::Fill);
        tool(self, ui, ed, Icon::Picker, t.t_picker, "I", Tool::Picker);
        tool(self, ui, ed, Icon::Text, t.t_text, "T", Tool::Text);
        let shape_icon = match self.settings.shape {
            ShapeKind::Line => Icon::Line,
            ShapeKind::Arrow => Icon::Arrow,
            ShapeKind::Rect => Icon::Rect,
            ShapeKind::RoundRect => Icon::RoundRect,
            ShapeKind::Ellipse => Icon::Ellipse,
        };
        tool(self, ui, ed, shape_icon, t.t_shape, "U", Tool::Shape);
        widgets::separator(ui);
        // 打码单独一组，选中时用强调色底
        if widgets::accent_icon_button(ui, Icon::Mosaic, t.t_redact, "M", on, ed.tool == Tool::Redact).clicked() {
            self.switch_tool(ed, Tool::Redact);
        }
        widgets::separator(ui);
        self.color_swatches(ui, ed);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.done_cancel(ui, ed));
    }

    /// 右边：完成（旁边的下拉：另存为、复制到剪贴板并退出）、取消
    fn done_cancel(&mut self, ui: &mut egui::Ui, ed: &mut Editor) {
        let t = tr();
        let on = !ed.saving;
        ui.spacing_mut().item_spacing.x = 2.0;
        let tex = crate::ui::icons::texture(ui.ctx(), Icon::Dropdown, 16.0);
        let img = egui::Image::new((tex.id(), vec2(16.0, 16.0))).tint(crate::ui::theme::palette(ui.ctx()).text);
        let more = ui.add_enabled(on, egui::Button::image(img).min_size(vec2(24.0, 32.0)).frame_when_inactive(false)).on_hover_text(tr().more);
        Popup::menu(&more).align(egui::RectAlign::BOTTOM_END).show(|ui| {
            ui.set_min_width(200.0);
            let c = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
            if ui.add(egui::Button::new(t.save_as).shortcut_text(format!("{c}Shift+S"))).clicked() {
                ui.close();
                self.edit_save_as(ed, SaveThen::Exit);
            }
            if ui.button(t.copy_and_exit).clicked() {
                ui.close();
                self.edit_finalize(ed);
                self.edit_copy_all(ed);
                ed.leave = true;
            }
        });
        let done = ui.add_enabled(on, egui::Button::new(egui::RichText::new(t.done).color(crate::ui::theme::palette(ui.ctx()).on_accent))
            .fill(crate::ui::theme::palette(ui.ctx()).accent)
            .corner_radius(egui::CornerRadius::same(4))
            .min_size(vec2(72.0, 32.0)));
        if done.on_hover_text(format!("{}  Ctrl+Enter", t.done_hint)).clicked() {
            self.edit_save(ed, SaveThen::Exit);
        }
        ui.add_space(6.0);
        if ui.add_enabled(on, egui::Button::new(t.cancel).min_size(vec2(64.0, 32.0))).clicked() {
            self.edit_request_exit(ed, None);
        }
    }
}

/// 形状下拉（属性栏里也用）
pub(crate) fn shape_menu(ui: &mut egui::Ui, kind: &mut ShapeKind) {
    let t = tr();
    let names = [(ShapeKind::Line, t.shape_line), (ShapeKind::Arrow, t.shape_arrow), (ShapeKind::Rect, t.shape_rect), (ShapeKind::RoundRect, t.shape_round), (ShapeKind::Ellipse, t.shape_ellipse)];
    let cur = names.iter().find(|(k, _)| k == kind).map_or("", |(_, n)| *n);
    MenuButton::new(cur).ui(ui, |ui| {
        for (k, n) in names {
            if ui.selectable_label(*kind == k, n).clicked() {
                *kind = k;
                ui.close();
            }
        }
    });
}
