//! 画图工具里要「确认」才写进图的：形状（画完可以拉控制点调整）、文字（输入时可改）。

use super::Editor;
use crate::app::App;

/// 画完还没确认的形状
pub(crate) struct PendingShape;

/// 还没确认的文本框
pub(crate) struct PendingText;

impl App {
    pub(crate) fn commit_shape(&mut self, ed: &mut Editor) {
        ed.shape = None;
    }

    pub(crate) fn commit_text(&mut self, ed: &mut Editor) {
        ed.text = None;
    }

    pub(crate) fn shape_press(&mut self, ed: &mut Editor, p: egui::Pos2, _screen: egui::Pos2, _secondary: bool) {
        self.edit_finalize(ed);
        ed.drag = Some(super::Drag::Shape(p));
    }

    pub(crate) fn shape_drag(&mut self, _ed: &mut Editor, _p: egui::Pos2, _shift: bool) {}

    pub(crate) fn shape_release(&mut self, _ed: &mut Editor) {}

    pub(crate) fn text_press(&mut self, ed: &mut Editor, _p: egui::Pos2) {
        self.edit_finalize(ed);
    }

    pub(crate) fn shape_props(&mut self, ui: &mut egui::Ui, _ed: &mut Editor) {
        super::bars::shape_menu(ui, &mut self.settings.shape);
    }

    pub(crate) fn text_props(&mut self, _ui: &mut egui::Ui, _ed: &mut Editor) {}
}
