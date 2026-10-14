//! 编辑模式的快捷键（PRD 第 8 节）。文本框输入时单键快捷键全部失效。

use egui::{Event, Key, Modifiers};

use super::ops::Xform;
use super::{Drag, Editor, SaveThen, Tool};
use crate::app::App;

impl App {
    pub(crate) fn edit_keys(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        let dropped: Option<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).next());
        if let Some(p) = dropped {
            self.paste_file(p);
        }
        if self.dialog.is_some() || self.settings_page.is_some() || ed.resize.is_some() || ed.save_options.is_some() || ed.saving {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input() || ed.text.is_some();
        let (cmd, shift, none) = (Modifiers::COMMAND, Modifiers::SHIFT, Modifiers::NONE);
        let cmd_shift = cmd | shift;
        let k = |m: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(m, key));
        // 剪切 / 复制 / 粘贴（egui 把 Ctrl+X、Shift+Del 转成 Cut 事件，Ctrl+C 转成 Copy；Ctrl+V 看按键抬起）
        let (cut, copy, paste) = ctx.input(|i| {
            let cut = i.events.iter().find_map(|e| matches!(e, Event::Cut).then_some(i.modifiers));
            let copy = i.events.iter().any(|e| matches!(e, Event::Copy));
            let paste = i.events.iter().any(|e| matches!(e, Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command && !modifiers.shift));
            (cut, copy, paste)
        });
        if !typing {
            match cut {
                Some(m) if m.command && m.shift => self.edit_crop(ed),
                Some(_) => self.edit_cut(ed),
                None => {}
            }
            if copy {
                self.edit_copy(ed);
            }
            if paste {
                self.edit_paste();
            }
        }
        if k(cmd_shift, Key::Z) || k(cmd, Key::Y) {
            self.edit_redo(ed);
        } else if k(cmd, Key::Z) {
            self.edit_undo(ed);
        }
        if k(cmd, Key::Enter) {
            self.edit_save(ed, SaveThen::Exit);
            return;
        }
        if k(cmd_shift, Key::S) {
            self.edit_save_as(ed, SaveThen::Stay);
        } else if k(cmd, Key::S) {
            self.edit_save(ed, SaveThen::Stay);
        }
        if k(cmd, Key::W) {
            self.open_resize(ed);
        }
        if k(cmd_shift, Key::R) {
            self.edit_transform(ed, Xform::Ccw);
        } else if k(cmd, Key::R) {
            self.edit_transform(ed, Xform::Cw);
        }
        if k(cmd, Key::Num0) {
            self.zoom_fit();
        }
        if k(cmd, Key::Num1) {
            self.zoom_actual();
        }
        if k(cmd, Key::Equals) || k(cmd, Key::Plus) {
            self.zoom_step(true);
        }
        if k(cmd, Key::Minus) {
            self.zoom_step(false);
        }
        if k(none, Key::F11) {
            self.toggle_fullscreen(ctx);
        }
        if typing {
            return;
        }
        if k(cmd, Key::A) {
            self.edit_select_all(ed);
        }
        if k(none, Key::Delete) {
            self.edit_delete(ed);
        }
        if k(none, Key::Plus) || k(none, Key::Equals) {
            self.zoom_step(true);
        }
        if k(none, Key::Minus) {
            self.zoom_step(false);
        }
        for (key, tool) in [
            (Key::S, Tool::Select),
            (Key::P, Tool::Pencil),
            (Key::B, Tool::Brush),
            (Key::E, Tool::Eraser),
            (Key::G, Tool::Fill),
            (Key::I, Tool::Picker),
            (Key::T, Tool::Text),
            (Key::U, Tool::Shape),
            (Key::M, Tool::Redact),
        ] {
            if k(none, key) {
                self.switch_tool(ed, tool);
            }
        }
        if k(none, Key::C) {
            self.edit_crop(ed);
        }
        if k(none, Key::OpenBracket) {
            self.adjust_size(ed, false);
        }
        if k(none, Key::CloseBracket) {
            self.adjust_size(ed, true);
        }
        if k(none, Key::X) {
            std::mem::swap(&mut self.settings.color1, &mut self.settings.color2);
        }
        // 方向键微调浮动选区（Shift 每次 10 像素）
        for (key, d) in [(Key::ArrowLeft, egui::vec2(-1.0, 0.0)), (Key::ArrowRight, egui::vec2(1.0, 0.0)), (Key::ArrowUp, egui::vec2(0.0, -1.0)), (Key::ArrowDown, egui::vec2(0.0, 1.0))] {
            let step = if k(shift, key) {
                10.0
            } else if k(none, key) {
                1.0
            } else {
                continue;
            };
            if let Some(f) = &mut ed.floating {
                f.rect = f.rect.translate(d * step);
            }
        }
        if k(none, Key::Escape) && !ctx.any_popup_open() {
            self.edit_escape(ctx, ed);
        }
    }

    /// Esc 依次取消：拖动中的 → 浮动选区 / 形状 / 文字（确认下来）→ 选区；都没有时退出编辑
    fn edit_escape(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        if let Some(d) = ed.drag.take() {
            match d {
                Drag::Stroke(s) | Drag::RedactRect(_, s) => {
                    let r = s.cancel(ed.doc.image_mut());
                    ed.doc.mark(r);
                }
                Drag::Move { from, .. } | Drag::Handle { from, .. } => {
                    if let Some(f) = &mut ed.floating {
                        f.rect = from;
                    }
                }
                Drag::Select(_) | Drag::Pan | Drag::Shape(_) => {}
            }
        } else if ed.floating.is_some() || ed.shape.is_some() || ed.text.is_some() {
            self.edit_finalize(ed);
        } else if ed.sel.is_some() {
            ed.sel = None;
        } else if self.fullscreen {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        } else {
            self.edit_request_exit(ed, None);
        }
    }
}
