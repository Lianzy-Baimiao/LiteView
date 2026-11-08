//! 键盘与拖入文件（查看模式的快捷键见 PRD 第 8 节）。

use egui::{Key, Modifiers};

use super::App;

impl App {
    pub(crate) fn keyboard(&mut self, ctx: &egui::Context) {
        let dropped: Option<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).next());
        if let Some(p) = dropped {
            self.open_path(p, ctx);
        }
        if self.modal_open() || ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::I)) {
            self.info_open = !self.info_open;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) {
            self.start_rename();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Comma)) {
            self.open_settings(None);
            return;
        }
        let (cmd, shift, none) = (Modifiers::COMMAND, Modifiers::SHIFT, Modifiers::NONE);
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let k = |m: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(m, key));
        let anim = self.is_animated();

        // 翻页：刚按下的翻一张，按住交给 flip（系统的按键重复也吃掉，免得别处再处理）
        for (keys, d) in [([Key::ArrowRight, Key::PageDown], 1), ([Key::ArrowLeft, Key::PageUp], -1)] {
            let fresh = ctx.input_mut(|i| {
                let mut fresh = false;
                i.events.retain(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat, modifiers, .. } if keys.contains(key) && modifiers.matches_logically(none) => {
                        fresh |= !repeat;
                        false
                    }
                    _ => true,
                });
                fresh
            });
            if fresh {
                self.flip.pressed = Some(d);
            }
            if ctx.input(|i| i.modifiers.matches_logically(none) && keys.iter().any(|k| i.key_down(*k))) {
                self.flip.held = Some(d);
            }
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
            self.zoom_fit_shown();
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
        if super::frameless::SUPPORTED && k(cmd, Key::B) {
            self.toggle_borderless();
        }
        // Windows 上 egui 把 Shift+Del 转成「剪切」事件
        let cut = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Cut)) && i.modifiers.shift && !i.modifiers.command);
        if cut || k(shift, Key::Delete) {
            self.request_delete(true);
        } else if k(none, Key::Delete) {
            self.request_delete(false);
        }
        if k(none, Key::E) {
            self.enter_edit(super::edit::Tool::Select);
            return;
        }
        if k(none, Key::M) {
            self.enter_edit(super::edit::Tool::Redact);
            return;
        }
        if k(cmd, Key::O) {
            self.open_dialog(ctx);
        }
        #[cfg(windows)]
        if k(cmd, Key::P) {
            self.open_print();
            return;
        }
        if k(cmd_shift, Key::S) {
            self.save_as();
        } else if k(cmd, Key::S) {
            if self.needs_rotation_prompt() {
                self.save_rotation();
            } else if self.rotation_unsaved() && self.current_info().is_some() {
                // 动图等：存不回原文件，说一声（以前会只剩第一帧）
                self.show_toast(crate::i18n::tr().rotate_cannot_save);
            }
        }
        // 菜单开着时 Esc 留给菜单（工具栏上的菜单在这之后才画，先吃掉这个键它就关不掉了）
        if !super::popup_open(ctx) && k(none, Key::Escape) {
            if self.settings_page.is_some() {
                // 设置在单独的窗口里、图片窗口在最前面时：先关设置，不关程序
                self.close_settings();
            } else if self.info_open {
                self.info_open = false;
            } else if self.fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        // Ctrl+W：直接关窗口（全屏时也是）；编辑模式里是「调整大小」（与画图一致），在 edit/keys.rs
        if k(cmd, Key::W) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers, RawInput, ViewportCommand, ViewportId};

    use crate::app::App;
    use crate::settings::Settings;
    use crate::viewer::loader::Loader;

    /// 按一下 key，返回这一帧发出的窗口命令
    fn press(app: &mut App, ctx: &egui::Context, modifiers: Modifiers, key: Key) -> Vec<ViewportCommand> {
        let events = vec![
            Event::ModifiersChanged(modifiers),
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
            Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
        ];
        let mut out = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| app.keyboard(ui.ctx()));
        out.viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()
    }

    #[test]
    fn ctrl_w_closes_the_window_even_in_full_screen_where_esc_only_leaves_it() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        assert!(press(&mut app, &ctx, Modifiers::COMMAND, Key::W).contains(&ViewportCommand::Close));
        app.fullscreen = true;
        assert!(press(&mut app, &ctx, Modifiers::COMMAND, Key::W).contains(&ViewportCommand::Close));
        let esc = press(&mut app, &ctx, Modifiers::NONE, Key::Escape);
        assert!(esc.contains(&ViewportCommand::Fullscreen(false)) && !esc.contains(&ViewportCommand::Close));
        // 没按 Ctrl 的 W 不关
        app.fullscreen = false;
        assert!(!press(&mut app, &ctx, Modifiers::NONE, Key::W).contains(&ViewportCommand::Close));
    }

    #[test]
    fn ctrl_comma_opens_settings_once_and_then_only_brings_it_to_the_front() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        press(&mut app, &ctx, Modifiers::COMMAND, Key::Comma);
        assert_eq!(app.settings_page, Some(crate::app::settings_ui::Page::General));
        assert!(std::mem::take(&mut app.settings_focus));
        // 换到别的页之后再按：不回到第一页，只拿到最前面
        app.settings_page = Some(crate::app::settings_ui::Page::Keys);
        app.open_settings(None);
        assert_eq!(app.settings_page, Some(crate::app::settings_ui::Page::Keys));
        assert!(app.settings_focus);
        // Windows：设置在单独的窗口里，图片窗口的快捷键照用（I 打开图片信息）
        if cfg!(windows) {
            assert!(!app.modal_open());
            press(&mut app, &ctx, Modifiers::NONE, Key::I);
            assert!(app.info_open);
        } else {
            assert!(app.modal_open());
        }
        // Esc 先关设置，不关程序
        if cfg!(windows) {
            let cmds = press(&mut app, &ctx, Modifiers::NONE, Key::Escape);
            assert!(app.settings_page.is_none() && !cmds.contains(&ViewportCommand::Close));
            app.open_settings(None);
        }
        app.settings_window_failed = true;
        assert!(app.modal_open(), "drawn in the image window when its own window can't be made");
    }

    #[test]
    fn zooming_shows_the_percentage_but_rotating_does_not() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        app.shown = Some(crate::app::Shown { width: 400, height: 300, has_alpha: false, translucent: false });
        app.rotate(true);
        assert!(app.zoom_osd.is_none());
        press(&mut app, &ctx, Modifiers::COMMAND, Key::Num1);
        assert!(app.zoom_osd.is_some());
        app.zoom_osd = None;
        press(&mut app, &ctx, Modifiers::COMMAND, Key::Num0);
        assert!(app.zoom_osd.is_some());
        assert_eq!(crate::app::chrome::zoom_text(1.0), "100%");
        assert_eq!(crate::app::chrome::zoom_text(0.25), "25%");
        assert_eq!(crate::app::chrome::zoom_text(0.005), "0.5%");
        assert_eq!(crate::app::chrome::zoom_text(12.5), "1250%");
    }

    #[test]
    fn a_rotated_animation_is_never_offered_for_saving_back() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        let open = |data: &[u8]| Some(Ok(std::sync::Arc::new(crate::decode::decode_bytes(data, "png", &Default::default()).unwrap())));
        let png = crate::save::encode(&image::RgbaImage::new(4, 2), crate::save::Format::Png, 90).unwrap();
        app.cur = Some(crate::app::Current { path: "/d/a.png".into(), loaded: open(&png), preview: None, since: std::time::Instant::now(), clipboard: false });
        app.view.xf.rotate(true);
        assert!(app.needs_rotation_prompt());
        // APNG：以前保存旋转会只剩第一帧
        app.cur.as_mut().unwrap().loaded = open(include_bytes!("../../tests/fixtures/animation.png"));
        assert!(!app.needs_rotation_prompt());
        press(&mut app, &ctx, Modifiers::COMMAND, Key::S);
        assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some(crate::i18n::tr().rotate_cannot_save));
    }
}
