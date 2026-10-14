//! 右键菜单（PRD 6.5）与确认框（只用在不可撤销的操作上，按钮写清动作）。

use egui::RichText;

use super::{App, Dialog};
use crate::i18n::tr;
use crate::ui::{theme, widgets};

enum Choice {
    Primary,
    Secondary,
    Cancel,
}

impl App {
    pub(crate) fn context_menu(&mut self, resp: &egui::Response) {
        resp.context_menu(|ui| {
            ui.set_min_width(230.0);
            let t = tr();
            let p = theme::palette(ui.ctx());
            let has_img = self.decoded().is_some();
            let has_file = self.cur.as_ref().is_some_and(|c| !c.clipboard);
            let c = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
            let item = |ui: &mut egui::Ui, text: &str, sc: String, on: bool| ui.add_enabled(on, egui::Button::new(text).shortcut_text(sc)).clicked();
            // 动图只看，不能编辑、打码
            let editable = has_img && !self.is_animated();
            let edit = ui.add_enabled(editable, egui::Button::new(t.edit).shortcut_text("E"));
            if has_img && !editable {
                edit.clone().on_disabled_hover_text(t.anim_no_edit);
            }
            if edit.clicked() {
                ui.close();
                self.enter_edit(super::edit::Tool::Select);
            }
            if item(ui, t.mosaic, "M".into(), editable) {
                ui.close();
                self.enter_edit(super::edit::Tool::Redact);
            }
            ui.separator();
            if item(ui, t.copy_image, format!("{c}C"), has_img) {
                self.copy_image();
                ui.close();
            }
            if item(ui, t.copy_path, format!("{c}Shift+C"), has_file) {
                self.copy_path(ui.ctx());
                ui.close();
            }
            ui.separator();
            if item(ui, t.rotate_cw, format!("{c}R"), has_img) {
                self.rotate(true);
                ui.close();
            }
            if item(ui, t.rotate_ccw, format!("{c}Shift+R"), has_img) {
                self.rotate(false);
                ui.close();
            }
            if item(ui, t.flip_h, String::new(), has_img) {
                self.view.xf.flip_h = !self.view.xf.flip_h;
                ui.close();
            }
            if item(ui, t.flip_v, String::new(), has_img) {
                self.view.xf.flip_v = !self.view.xf.flip_v;
                ui.close();
            }
            ui.separator();
            let save_label = if self.is_animated() { t.export_frame } else { t.save_as };
            if item(ui, save_label, format!("{c}Shift+S"), has_img) {
                ui.close();
                self.save_as();
            }
            if item(ui, t.rename, "F2".into(), has_file) {
                ui.close();
                self.start_rename();
            }
            if item(ui, super::canvas::folder_label(), String::new(), has_file) {
                if let Some(cur) = &self.cur {
                    crate::platform::show_in_folder(&cur.path);
                }
                ui.close();
            }
            if crate::platform::has_open_with() && item(ui, t.open_with, String::new(), has_file) {
                if let Some(cur) = &self.cur {
                    crate::platform::open_with(&cur.path);
                }
                ui.close();
            }
            ui.separator();
            let del = egui::Button::new(RichText::new(t.delete).color(if has_file { p.danger } else { p.text_weak })).shortcut_text("Del");
            if ui.add_enabled(has_file, del).clicked() {
                ui.close();
                self.request_delete(false);
            }
            ui.separator();
            if item(ui, t.info, "I".into(), has_img) {
                ui.close();
                self.info_open = true;
            }
            if item(ui, t.settings, format!("{c},"), true) {
                ui.close();
                self.settings_page = Some(Default::default());
            }
        });
    }

    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        let is_jpeg = self.cur.as_ref().is_some_and(|c| {
            c.path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "jpe" | "jfif"))
        });
        let Some(d) = &mut self.dialog else { return };
        let t = tr();
        let p = theme::palette(ctx);
        let mut choice = None;
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(egui::CornerRadius::same(8));
        let modal = egui::Modal::new(egui::Id::new("dialog")).frame(frame).show(ctx, |ui| {
            ui.set_width(380.0);
            let (title, body, primary, secondary, danger, cancel) = match d {
                Dialog::Delete { forever: true, .. } => (t.delete_title, t.delete_body_forever, t.delete_forever_btn, None, true, t.cancel),
                Dialog::Delete { .. } => (t.delete_title, trash_body(), t.delete_btn, None, true, t.cancel),
                Dialog::SaveRotation { .. } => (t.rotate_save_title, t.rotate_save_body, t.save, Some(t.dont_save), false, t.cancel),
                Dialog::AskDefault => (t.ask_title, t.ask_body, t.a_set_default, Some(t.ask_never), false, t.ask_later),
                Dialog::ExitEdit { .. } => (t.exit_edit_title, t.exit_edit_body, t.save, Some(t.dont_save), false, t.cancel),
                Dialog::Overwrite { .. } => (t.overwrite_title, t.overwrite_body, t.overwrite_btn, Some(t.save_as), false, t.cancel),
                Dialog::ExpandCanvas { .. } => (t.expand_title, t.expand_body, t.expand_btn, Some(t.keep_size), false, t.cancel),
            };
            ui.label(RichText::new(title).size(16.0).color(p.text));
            ui.add_space(6.0);
            ui.label(RichText::new(body).color(p.text_weak));
            if matches!(d, Dialog::SaveRotation { .. }) && is_jpeg {
                ui.label(RichText::new(t.rotate_save_jpeg).color(p.text_weak).size(12.0));
            }
            if let Dialog::Delete { forever: false, dont_ask, .. } | Dialog::Overwrite { dont_ask, .. } = d {
                ui.add_space(10.0);
                widgets::checkbox(ui, dont_ask, t.dont_ask);
            }
            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::accent_button(ui, primary, danger).clicked() {
                    choice = Some(Choice::Primary);
                }
                if let Some(s) = secondary
                    && widgets::plain_button(ui, s).clicked()
                {
                    choice = Some(Choice::Secondary);
                }
                if widgets::plain_button(ui, cancel).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(Choice::Cancel);
        }
        // 回车 = 主按钮（删除这类不可恢复的操作除外，要点按钮）
        if choice.is_none() && !matches!(self.dialog, Some(Dialog::Delete { .. })) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            choice = Some(Choice::Primary);
        }
        let Some(choice) = choice else { return };
        let Some(d) = self.dialog.take() else { return };
        use super::edit::SaveThen;
        match (d, choice) {
            (Dialog::ExitEdit { .. }, Choice::Cancel) => self.pending_edit = None,
            (Dialog::Overwrite { .. }, Choice::Cancel) => self.with_editor(|_, ed| ed.after = None),
            (_, Choice::Cancel) => {}
            (Dialog::ExitEdit { after }, Choice::Primary) => self.with_editor(|app, ed| {
                ed.after = after;
                app.edit_save(ed, SaveThen::Exit);
            }),
            (Dialog::ExitEdit { after }, Choice::Secondary) => self.with_editor(|_, ed| {
                ed.leave = true;
                ed.after = after;
            }),
            (Dialog::Overwrite { then, dont_ask }, Choice::Primary) => {
                if dont_ask {
                    self.settings.confirm_overwrite = false;
                }
                self.with_editor(|app, ed| {
                    ed.overwrite_ok = true;
                    app.edit_save(ed, then);
                });
            }
            (Dialog::Overwrite { then, .. }, Choice::Secondary) => self.with_editor(|app, ed| app.edit_save_as(ed, then)),
            (Dialog::ExpandCanvas { image }, Choice::Primary) => self.with_editor(|app, ed| app.expand_and_paste(ed, image)),
            (Dialog::ExpandCanvas { image }, Choice::Secondary) => self.with_editor(|app, ed| app.place_floating(ed, image, true)),
            (Dialog::Delete { path, forever, dont_ask }, _) => {
                if dont_ask && !forever {
                    self.settings.confirm_delete = false;
                }
                self.do_delete(path, forever);
            }
            (Dialog::SaveRotation { after }, Choice::Primary) => {
                if matches!(after, super::After::Close) {
                    self.save_rotation_blocking();
                } else {
                    self.save_rotation();
                }
                self.resume(after, ctx);
            }
            (Dialog::SaveRotation { after }, Choice::Secondary) => self.resume(after, ctx),
            (Dialog::AskDefault, Choice::Primary) => {
                self.settings.ask_default_app = false;
                #[cfg(windows)]
                match crate::platform::assoc::register(&self.settings.assoc_exts) {
                    Ok(()) => crate::platform::assoc::open_default_apps(),
                    Err(e) => self.show_toast(crate::i18n::fill(t.failed, &[&e])),
                }
                self.settings.save();
            }
            (Dialog::AskDefault, Choice::Secondary) => {
                self.settings.ask_default_app = false;
                self.settings.save();
            }
        }
    }
}

pub(crate) fn trash_body() -> &'static str {
    if cfg!(target_os = "macos") { tr().delete_body_trash_mac } else { tr().delete_body_trash }
}
