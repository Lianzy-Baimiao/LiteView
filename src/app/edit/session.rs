//! 进入 / 退出编辑；每帧把编辑器从 App 里拿出来用（避免同时借用），用完放回。

use egui::{Align, Frame, Layout};

use super::{Editor, Source, Tool};
use crate::app::{After, App, Dialog, Shown, Tex};
use crate::decode::Content;
use crate::editor::history::Doc;
use crate::i18n::tr;
use crate::ui::theme;

impl App {
    /// 进入编辑（E）或直接进入打码（M）；已经在编辑时只换工具
    pub(crate) fn enter_edit(&mut self, tool: Tool) {
        if self.editor.is_some() {
            self.with_editor(|app, ed| app.switch_tool(ed, tool));
            return;
        }
        let t = tr();
        let Some(d) = self.decoded().cloned() else {
            // 只显示着缩略图：解码原图，到了再进入
            if self.cur.as_ref().is_some_and(|c| c.preview.is_some()) {
                self.pending_edit = Some(tool);
                self.want_full();
            }
            return;
        };
        let Content::Still { image, .. } = &d.content else {
            self.show_toast(t.anim_no_edit);
            return;
        };
        if self.dialog.is_some() || self.rename.is_some() {
            return;
        }
        if d.info.bits > 8 {
            self.show_toast(t.edit_8bit);
        } else if u64::from(d.info.width) * u64::from(d.info.height) > 64_000_000 {
            self.show_toast(t.edit_large);
        }
        let xf = self.view.xf;
        let img = crate::save::apply_transform(image, xf);
        let s = &self.settings;
        let doc = Doc::new(img, s.undo_steps as usize, s.undo_mb as usize * 1048576);
        let Some(cur) = self.cur.as_ref() else { return };
        let src = Source {
            path: cur.path.clone(),
            clipboard: cur.clipboard,
            format: d.info.format,
            file_size: d.info.file_size,
            jpeg_quality: d.info.jpeg_quality,
            exif: d.info.exif.clone(),
            canvas_icc: d.info.icc.clone(),
            canvas_alpha: d.info.has_alpha,
        };
        self.view.xf = Default::default();
        self.shown = Some(Shown { width: doc.width(), height: doc.height(), has_alpha: true });
        // 看图用的纹理先放掉，退出时再建
        self.tex = Tex::None;
        self.play = None;
        self.info_open = false;
        self.pending_edit = None;
        self.editor = Some(Box::new(Editor::new(doc, src, tool, xf)));
    }

    /// 退出编辑，回到查看：显示最近保存的结果；没保存过就显示原图并恢复进入前的旋转
    pub(crate) fn exit_edit(&mut self, ed: Box<Editor>) {
        // 剪贴板来的图、结果已经复制回剪贴板：退出后显示复制出去的结果（没有文件可回退）
        if ed.src.clipboard
            && ed.copied_rev == Some(ed.doc.rev)
            && ed.doc.modified()
            && let Some(c) = &mut self.cur
        {
            c.loaded = Some(Ok(std::sync::Arc::new(crate::decode::from_rgba((*ed.doc.image).clone(), "PNG"))));
        }
        let Some(l) = self.cur.as_ref().and_then(|c| c.loaded.clone()) else {
            self.tex = Tex::None;
            self.shown = None;
            return;
        };
        let pending = self.pending_reset;
        self.on_loaded(l);
        if !ed.saved_once {
            self.view.xf = ed.xf_before;
        }
        // 尺寸没变时保持缩放与位置
        if let Some(s) = self.shown
            && self.view.shown_size(s.width, s.height) == egui::vec2(ed.doc.width() as f32, ed.doc.height() as f32)
        {
            self.pending_reset = pending;
        }
    }

    /// 编辑模式的一帧：工具栏、属性栏、状态栏、画布、快捷键
    pub(crate) fn edit_frame(&mut self, ui: &mut egui::Ui) {
        let Some(mut ed) = self.editor.take() else { return };
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let bar = Frame::new().fill(p.surface).inner_margin(egui::Margin::symmetric(8, 0));
        egui::Panel::top("edit-toolbar").exact_size(48.0).resizable(false).frame(bar).show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| self.edit_toolbar(ui, &mut ed));
        });
        egui::Panel::top("edit-props").exact_size(40.0).resizable(false).frame(bar).show(ui, |ui| {
            // 窗口窄时属性栏可以横向滚动（滚轮 / 拖动），不显示滚动条
            egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.set_min_height(40.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| self.edit_props(ui, &mut ed));
            });
        });
        egui::Panel::bottom("edit-status").exact_size(28.0).resizable(false).frame(bar).show(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| self.edit_status(ui, &mut ed));
        });
        egui::CentralPanel::no_frame().show(ui, |ui| self.edit_canvas(ui, &mut ed));
        self.edit_keys(&ctx, &mut ed);
        self.edit_dialogs(&ctx, &mut ed);
        self.finish_edit_frame(ed);
    }

    /// 放回编辑器；要退出时退出并接着做之后的事
    pub(crate) fn finish_edit_frame(&mut self, mut ed: Box<Editor>) {
        if ed.leave {
            let after = ed.after.take();
            self.exit_edit(ed);
            if let Some(a) = after {
                self.after_edit(a);
            }
        } else {
            self.editor = Some(ed);
        }
    }

    /// 在编辑器拿出来的情况下做一件事（处理通知、确认框时用）
    pub(crate) fn with_editor(&mut self, f: impl FnOnce(&mut Self, &mut Editor)) {
        let Some(mut ed) = self.editor.take() else { return };
        f(self, &mut ed);
        self.finish_edit_frame(ed);
    }

    /// 退出编辑之后继续：切换图片、打开文件、关闭窗口
    fn after_edit(&mut self, a: After) {
        match a {
            After::Step(d) => {
                self.navigate(d);
            }
            After::Go(i) => self.go(i),
            After::Open(p) => self.open_path(p, &self.ctx.clone()),
            // 进入编辑前旋转过、又没保存：先问要不要保存旋转
            After::Close if self.needs_rotation_prompt() => self.dialog = Some(Dialog::SaveRotation { after: After::Close }),
            After::Close => {
                self.allow_close = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// 要离开编辑（取消、Esc、关闭窗口、打开别的图）：有没保存的修改时先询问
    pub(crate) fn edit_request_exit(&mut self, ed: &mut Editor, after: Option<After>) {
        if ed.saving {
            return;
        }
        self.edit_finalize(ed);
        let copied = ed.src.clipboard && ed.copied_rev == Some(ed.doc.rev);
        if !ed.modified() || copied {
            ed.leave = true;
            ed.after = after;
        } else {
            self.dialog = Some(Dialog::ExitEdit { after });
        }
    }
}
