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
    /// 画布的右键菜单；返回菜单是否开着（或等着窗口变大后打开）。窗口放得下就完整显示，放不下才用滚轮滚动
    /// （egui 的菜单画不到窗口外面）。无边框的小窗口放不下时窗口先透明地变大（`update_grow`），下一帧再在右键的地方打开
    pub(crate) fn context_menu(&mut self, resp: &egui::Response) -> bool {
        let ctx = resp.ctx.clone();
        let max_h = (ctx.content_rect().height() - 16.0).max(80.0);
        if resp.secondary_clicked()
            && let Some(q) = resp.interact_pointer_pos()
        {
            self.menu_at = Some(q - self.canvas.min);
            self.menu_pending = true;
        }
        let mut open = None;
        if self.menu_pending
            && let Some(at) = self.menu_at
        {
            if self.overlay_ready(&ctx, super::frameless::Need::Menu(at)) {
                self.menu_pending = false;
                open = Some(egui::containers::SetOpenCommand::Bool(true));
            } else {
                ctx.request_repaint();
                return true;
            }
        } else if resp.clicked() {
            open = Some(egui::containers::SetOpenCommand::Bool(false));
        }
        // 每次打开都从第一项开始
        let fresh = open == Some(egui::containers::SetOpenCommand::Bool(true));
        let anchor = self.canvas.min + self.menu_at.unwrap_or_default();
        let shown = egui::Popup::menu(resp).open_memory(open).at_position(anchor).show(|ui| {
            // 滚动条单独占一列（默认的浮动滚动条压在文字和快捷键上），菜单跟着变宽
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            // 菜单第一次打开时 egui 按 400 高排版、之后一直用这个高度；按窗口能放下的高度排，内容矮时再缩回去
            let mut area = egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h);
            if fresh {
                area = area.vertical_scroll_offset(0.0);
            }
            area.show(ui, |ui| self.menu_items(ui)).content_size
        });
        match shown {
            Some(r) => {
                // 记下菜单不滚动时要多高，下次按它算窗口要不要变大
                self.menu_size = egui::vec2(r.response.rect.width(), r.inner.y + (r.response.rect.height() - r.inner.y.min(max_h)).max(0.0));
                true
            }
            None => false,
        }
    }

    /// 常用的放在第一层；同类的收进子菜单（鼠标指上去才出来），窗口矮时也少滚动
    fn menu_items(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(230.0);
        // 菜单项不折行：留出滚动条后宁可变宽
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        let t = tr();
        let p = theme::palette(ui.ctx());
        let has_img = self.has_image();
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
        let save_label = if self.is_animated() { t.export_frame } else { t.save_as };
        if item(ui, save_label, format!("{c}Shift+S"), has_img) {
            ui.close();
            self.save_as();
        }
        #[cfg(windows)]
        if item(ui, t.print, format!("{c}P"), has_img) {
            ui.close();
            self.open_print();
        }
        ui.menu_button(t.rotate_flip, |ui| {
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
        });
        ui.menu_button(t.file_menu, |ui| {
            if item(ui, t.copy_path, format!("{c}Shift+C"), has_file) {
                self.copy_path(ui.ctx());
                ui.close();
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
            if crate::platform::has_wallpaper() && item(ui, t.set_wallpaper, String::new(), has_img) {
                ui.close();
                self.set_wallpaper();
            }
        });
        ui.separator();
        let del = egui::Button::new(RichText::new(t.delete).color(if has_file { p.danger } else { p.text_weak })).shortcut_text("Del");
        if ui.add_enabled(has_file, del).clicked() {
            ui.close();
            self.request_delete(false);
        }
        ui.separator();
        ui.menu_button(t.window_menu, |ui| {
            if super::frameless::SUPPORTED {
                let mut on = self.settings.window.borderless;
                if ui.checkbox(&mut on, format!("{}    Ctrl+B", t.borderless)).clicked() {
                    ui.close();
                    self.toggle_borderless();
                }
                // 默认关：要多建一个小窗口（多一份显卡设备和界面）
                if ui.checkbox(&mut self.settings.window.toolbar_outside, t.toolbar_outside).on_hover_text(t.toolbar_outside_tip).clicked() {
                    ui.close();
                }
            }
            if ui.checkbox(&mut self.settings.window.always_on_top, t.always_on_top).clicked() {
                ui.close();
            }
        });
        if item(ui, t.info, "I".into(), has_img) {
            ui.close();
            self.info_open = true;
        }
        if item(ui, t.settings, format!("{c},"), true) {
            ui.close();
            self.open_settings(None);
        }
    }

    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        // 只改方向标记的格式（按文件内容认出的格式，不看扩展名）
        let tag_only = self.current_info().is_some_and(|i| matches!(i.format, "JPEG" | "TIFF"));
        if self.dialog.is_none() {
            return;
        }
        // 小窗口要先变大：这一帧先不画（不然先露出被裁掉的一角）
        if !self.overlay_ready(ctx, super::frameless::Need::Centered(super::frameless::DIALOG_ROOM)) {
            ctx.request_repaint();
            return;
        }
        let shell = self.modal(egui::Id::new("dialog"), ctx);
        let Some(d) = &mut self.dialog else { return };
        let t = tr();
        let p = theme::palette(ctx);
        let mut choice = None;
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(egui::CornerRadius::same(8));
        let modal = shell.frame(frame).show(ctx, |ui| {
            ui.set_width(380.0);
            let (title, body, primary, secondary, danger, cancel) = match d {
                Dialog::Delete { forever: true, .. } => (t.delete_title, t.delete_body_forever, t.delete_forever_btn, None, true, t.cancel),
                Dialog::Delete { .. } => (t.delete_title, trash_body(), t.delete_btn, None, true, t.cancel),
                Dialog::SaveRotation { .. } => (t.rotate_save_title, t.rotate_save_body, t.save, Some(t.dont_save), false, t.cancel),
                #[cfg(windows)]
                Dialog::AskDefault => (t.ask_title, t.ask_body, t.a_choose_formats, Some(t.ask_never), false, t.ask_later),
                Dialog::ExitEdit { .. } => (t.exit_edit_title, t.exit_edit_body, t.save, Some(t.dont_save), false, t.cancel),
                Dialog::Overwrite { .. } => (t.overwrite_title, t.overwrite_body, t.overwrite_btn, Some(t.save_as), false, t.cancel),
                Dialog::ExpandCanvas { .. } => (t.expand_title, t.expand_body, t.expand_btn, Some(t.keep_size), false, t.cancel),
            };
            ui.label(RichText::new(title).size(16.0).color(p.text));
            ui.add_space(6.0);
            ui.label(RichText::new(body).color(p.text_weak));
            if matches!(d, Dialog::SaveRotation { .. }) && tag_only {
                ui.label(RichText::new(t.rotate_save_tag).color(p.text_weak).size(12.0));
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
                    if self.save_rotation_blocking() {
                        self.resume(after, ctx);
                    }
                } else {
                    self.save_rotation();
                    self.resume(after, ctx);
                }
            }
            (Dialog::SaveRotation { after }, Choice::Secondary) => self.resume(after, ctx),
            #[cfg(windows)]
            (Dialog::AskDefault, Choice::Primary) => {
                self.settings.ask_default_app = false;
                self.open_settings(Some(super::settings_ui::Page::Assoc));
                self.settings.save();
            }
            #[cfg(windows)]
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
