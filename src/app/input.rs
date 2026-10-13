//! 键盘与拖入文件（查看模式的快捷键见 PRD 第 8 节）。

use egui::{Key, Modifiers};

use super::App;

impl App {
    pub(crate) fn keyboard(&mut self, ctx: &egui::Context) {
        let dropped: Option<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).next());
        if let Some(p) = dropped {
            self.open_path(p, ctx);
        }
        if self.dialog.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let (cmd, shift, none) = (Modifiers::COMMAND, Modifiers::SHIFT, Modifiers::NONE);
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let k = |m: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(m, key));
        let anim = self.is_animated();

        if k(none, Key::ArrowRight) || k(none, Key::PageDown) {
            self.navigate(1);
        }
        if k(none, Key::ArrowLeft) || k(none, Key::PageUp) {
            self.navigate(-1);
        }
        if k(none, Key::Home) {
            self.go(0);
        }
        if k(none, Key::End) {
            self.go(self.folder.len().saturating_sub(1));
        }
        if anim {
            if k(none, Key::Space)
                && let Some(p) = &mut self.play {
                    p.toggle();
                }
            if k(none, Key::Comma)
                && let Some(p) = &mut self.play {
                    p.step(-1);
                }
            if k(none, Key::Period)
                && let Some(p) = &mut self.play {
                    p.step(1);
                }
        }
        if k(cmd_shift, Key::R) {
            self.rotate(false);
        } else if k(cmd, Key::R) {
            self.rotate(true);
        }
        if k(cmd, Key::Num0) {
            self.zoom_fit();
        }
        if k(cmd, Key::Num1) {
            self.zoom_actual();
        }
        if k(cmd, Key::Equals) || k(cmd, Key::Plus) || k(none, Key::Plus) || k(none, Key::Equals) {
            self.zoom_step(true);
        }
        if k(cmd, Key::Minus) || k(none, Key::Minus) {
            self.zoom_step(false);
        }
        if k(none, Key::F11) {
            self.toggle_fullscreen(ctx);
        }
        if k(shift, Key::Delete) {
            self.request_delete(true);
        } else if k(none, Key::Delete) {
            self.request_delete(false);
        }
        if k(cmd, Key::O) {
            self.open_dialog(ctx);
        }
        if k(cmd_shift, Key::S) {
            self.save_as();
        } else if k(cmd, Key::S) && self.needs_rotation_prompt() {
            self.save_rotation();
        }
        if k(none, Key::Escape) && !ctx.any_popup_open() {
            if self.fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // Ctrl+C 被 egui 转成 Copy 事件；Ctrl+V 只在剪贴板有文字时才变成 Paste 事件，所以看按键抬起
        let (copy, paste) = ctx.input(|i| {
            let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            let paste = i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command && !modifiers.shift)
            });
            (copy.then_some(i.modifiers.shift), paste)
        });
        match copy {
            Some(true) => self.copy_path(ctx),
            Some(false) => self.copy_image(),
            None => {}
        }
        if paste {
            self.paste();
        }
    }
}
