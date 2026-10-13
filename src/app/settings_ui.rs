//! 设置窗口：左侧分类、右侧内容，修改立即生效（没有「应用」按钮）。Ctrl+, 或右键菜单打开。

use egui::{RichText, Ui};

use super::App;
use crate::i18n::{fill, tr};
use crate::settings::{Background, DoubleClick, FitMode, Language, Settings, SortKey, ThemeMode, WheelAction};
use crate::ui::{theme, widgets};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Page {
    #[default]
    General,
    View,
    Anim,
    Edit,
    Assoc,
    About,
}

/// 一行：左边说明，右边控件
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(170.0, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(170.0);
            ui.label(RichText::new(label).color(p.text));
        });
        add(ui);
    });
    ui.add_space(4.0);
}

/// 分段按钮选枚举值
fn choose<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, items: &[(T, &str)]) {
    let names: Vec<&str> = items.iter().map(|(_, n)| *n).collect();
    let cur = items.iter().position(|(v, _)| v == value).unwrap_or(0);
    if let Some(i) = widgets::segmented(ui, &names, cur) {
        *value = items[i].0;
    }
}

fn number(ui: &mut Ui, value: &mut u32, range: std::ops::RangeInclusive<f32>) {
    let mut v = *value as f32;
    widgets::slider(ui, &mut v, range, 200.0);
    *value = v as u32;
    ui.label(RichText::new(value.to_string()).size(13.0));
}

/// 文件关联页：按组勾选扩展名，「设为默认看图软件」= 注册 + 打开系统默认应用；返回轻提示
#[cfg(windows)]
fn assoc_page(ui: &mut Ui, s: &mut Settings) -> Option<String> {
    use crate::platform::assoc;
    let t = tr();
    let p = theme::palette(ui.ctx());
    let registered = assoc::is_registered();
    ui.label(RichText::new(if registered { t.a_registered } else { t.a_not_registered }).color(p.text_weak));
    ui.add_space(10.0);
    for (group, name) in assoc::GROUPS.iter().zip(t.a_groups.split('|')) {
        ui.label(RichText::new(name).size(12.0).color(p.text_weak));
        ui.horizontal_wrapped(|ui| {
            for ext in group.iter() {
                let mut on = s.assoc_exts.iter().any(|e| e == ext);
                let label = if registered && assoc::is_default(ext) { format!(".{ext}{}", t.a_is_default) } else { format!(".{ext}") };
                if widgets::checkbox(ui, &mut on, &label).changed() {
                    if on {
                        s.assoc_exts.push(ext.to_string());
                    } else {
                        s.assoc_exts.retain(|e| e != ext);
                    }
                }
                ui.add_space(10.0);
            }
        });
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        if ui.link(t.a_select_all).clicked() {
            s.assoc_exts = assoc::all_exts();
        }
        if ui.link(t.a_restore).clicked() {
            s.assoc_exts = assoc::default_exts();
        }
    });
    ui.add_space(12.0);
    ui.label(RichText::new(t.a_note).color(p.text_weak).size(12.0));
    ui.add_space(10.0);
    let mut msg = None;
    ui.horizontal(|ui| {
        if widgets::accent_button(ui, t.a_set_default, false).clicked() {
            match assoc::register(&s.assoc_exts) {
                Ok(()) => assoc::open_default_apps(),
                Err(e) => msg = Some(fill(t.failed, &[&e])),
            }
        }
        if registered && widgets::plain_button(ui, t.a_unregister).clicked() {
            assoc::unregister();
            msg = Some(t.a_removed.to_owned());
        }
    });
    msg
}

#[cfg(not(windows))]
fn assoc_page(ui: &mut Ui, _s: &mut Settings) -> Option<String> {
    ui.label(RichText::new(tr().a_windows_only).color(theme::palette(ui.ctx()).text_weak));
    None
}

impl App {
    pub(crate) fn settings_ui(&mut self, ctx: &egui::Context) {
        let Some(page) = self.settings_page else { return };
        let before = self.settings.clone();
        let mut page = page;
        let p = theme::palette(ctx);
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(0)).corner_radius(egui::CornerRadius::same(8));
        let modal = egui::Modal::new(egui::Id::new("settings")).frame(frame).show(ctx, |ui| {
            ui.set_width(700.0);
            ui.set_height(460.0);
            ui.horizontal_top(|ui| {
                egui::Frame::new().fill(p.canvas).inner_margin(egui::Margin::same(10)).show(ui, |ui| ui.vertical(|ui| {
                    ui.set_width(150.0);
                    ui.set_min_height(440.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let t = tr();
                    for (pg, name) in [
                        (Page::General, t.s_general),
                        (Page::View, t.s_view),
                        (Page::Anim, t.s_anim),
                        (Page::Edit, t.s_edit),
                        (Page::Assoc, t.s_assoc),
                        (Page::About, t.s_about),
                    ] {
                        let on = page == pg;
                        let b = egui::Button::new(RichText::new(name).color(if on { p.text } else { p.text_weak }))
                            .fill(if on { p.hover } else { egui::Color32::TRANSPARENT })
                            .min_size(egui::vec2(150.0, 32.0));
                        if ui.add(b).clicked() {
                            page = pg;
                        }
                    }
                }));
                egui::Frame::new().inner_margin(egui::Margin::same(20)).show(ui, |ui| ui.vertical(|ui| {
                    ui.set_width(500.0);
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| self.settings_page_ui(ui, page));
                }));
            });
        });
        self.settings_page = Some(page);
        if modal.should_close() && !ctx.any_popup_open() {
            self.settings_page = None;
            self.settings.save();
        }
        if self.settings != before {
            self.apply_settings(ctx, &before);
        }
    }

    fn settings_page_ui(&mut self, ui: &mut Ui, page: Page) {
        let t = tr();
        let mut toast = None;
        let s = &mut self.settings;
        ui.label(RichText::new(match page {
            Page::General => t.s_general,
            Page::View => t.s_view,
            Page::Anim => t.s_anim,
            Page::Edit => t.s_edit,
            Page::Assoc => t.s_assoc,
            Page::About => t.s_about,
        }).size(20.0));
        ui.add_space(14.0);
        match page {
            Page::General => {
                row(ui, t.s_language, |ui| choose(ui, &mut s.language, &[(Language::Auto, t.s_follow_system), (Language::ZhCn, "简体中文"), (Language::En, "English")]));
                row(ui, t.s_theme, |ui| choose(ui, &mut s.theme, &[(ThemeMode::System, t.s_follow_system), (ThemeMode::Light, t.s_light), (ThemeMode::Dark, t.s_dark)]));
                widgets::checkbox(ui, &mut s.confirm_delete, t.s_confirm_delete);
                widgets::checkbox(ui, &mut s.wrap_around, t.s_wrap);
            }
            Page::View => {
                row(ui, t.s_fit, |ui| choose(ui, &mut s.fit_mode, &[(FitMode::Fit, t.s_fit_fit), (FitMode::Actual, t.s_fit_actual), (FitMode::FitWidth, t.s_fit_width)]));
                row(ui, t.s_wheel, |ui| choose(ui, &mut s.wheel, &[(WheelAction::Zoom, t.s_wheel_zoom), (WheelAction::Navigate, t.s_wheel_nav)]));
                row(ui, t.s_double, |ui| choose(ui, &mut s.double_click, &[(DoubleClick::ToggleFit, t.s_double_fit), (DoubleClick::Fullscreen, t.s_double_full)]));
                row(ui, t.s_background, |ui| {
                    choose(ui, &mut s.background, &[(Background::Theme, t.s_bg_theme), (Background::Black, t.s_bg_black), (Background::White, t.s_bg_white), (Background::Checker, t.s_bg_checker)])
                });
                row(ui, t.s_sort, |ui| {
                    choose(ui, &mut s.sort, &[(SortKey::Name, t.s_sort_name), (SortKey::Modified, t.s_sort_time), (SortKey::Size, t.s_sort_size), (SortKey::Type, t.s_sort_type)])
                });
                row(ui, "", |ui| {
                    widgets::checkbox(ui, &mut s.sort_desc, t.s_sort_desc);
                });
                row(ui, t.s_preload, |ui| number(ui, &mut s.preload, 0.0..=3.0));
                row(ui, t.s_cache, |ui| number(ui, &mut s.cache_mb, 128.0..=4096.0));
                row(ui, t.s_nearest, |ui| number(ui, &mut s.nearest_above, 100.0..=1600.0));
                widgets::checkbox(ui, &mut s.smooth_zoom, t.s_smooth);
                widgets::checkbox(ui, &mut s.drag_inertia, t.s_inertia);
            }
            Page::Anim => {
                widgets::checkbox(ui, &mut s.anim_autoplay, t.s_autoplay);
                widgets::checkbox(ui, &mut s.anim_always_loop, t.s_always_loop);
                ui.add_space(6.0);
                row(ui, t.s_anim_cache, |ui| number(ui, &mut s.anim_cache_mb, 64.0..=2048.0));
            }
            Page::Edit => {
                widgets::checkbox(ui, &mut s.confirm_overwrite, t.s_confirm_overwrite);
                ui.add_space(6.0);
                let mut q = u32::from(s.jpeg_quality);
                row(ui, t.s_jpeg_quality, |ui| number(ui, &mut q, 1.0..=100.0));
                s.jpeg_quality = q as u8;
            }
            Page::Assoc => {
                if let Some(m) = assoc_page(ui, s) {
                    toast = Some(m);
                }
            }
            Page::About => {
                let p = theme::palette(ui.ctx());
                ui.label(RichText::new(format!("{}  {}", t.app_name, fill(t.s_version, &[&env!("CARGO_PKG_VERSION")]))).size(16.0));
                ui.add_space(6.0);
                ui.label(RichText::new(t.s_license).color(p.text_weak));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if widgets::plain_button(ui, t.s_homepage).clicked() {
                        crate::platform::open_url(env!("CARGO_PKG_REPOSITORY"));
                    }
                    if widgets::plain_button(ui, t.s_open_config).clicked() {
                        let dir = crate::settings::config_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        crate::platform::open_url(&dir.to_string_lossy());
                    }
                });
            }
        }
        self.settings_toast(toast);
    }

    fn settings_toast(&mut self, toast: Option<String>) {
        if let Some(m) = toast {
            self.show_toast(m);
        }
    }

    /// 让改过的设置立即生效
    pub(crate) fn apply_settings(&mut self, ctx: &egui::Context, before: &Settings) {
        let s = &self.settings;
        if s.theme != before.theme {
            ctx.set_theme(match s.theme {
                ThemeMode::System => egui::ThemePreference::System,
                ThemeMode::Light => egui::ThemePreference::Light,
                ThemeMode::Dark => egui::ThemePreference::Dark,
            });
        }
        if s.language != before.language {
            crate::i18n::set_language(s.language);
            self.title.clear();
        }
        if s.cache_mb != before.cache_mb {
            self.loader.set_budget(s.cache_mb as usize * 1048576);
        }
        if s.anim_cache_mb != before.anim_cache_mb {
            self.loader.set_options(super::decode_options(s));
        }
        if (s.sort, s.sort_desc) != (before.sort, before.sort_desc) && self.folder.len() > 1 {
            let cur = self.folder.current().map(std::path::PathBuf::from);
            crate::viewer::folder::sort_files(&mut self.folder.files, s.sort);
            if s.sort_desc {
                self.folder.files.reverse();
            }
            if let Some(i) = cur.and_then(|c| self.folder.files.iter().position(|p| *p == c)) {
                self.folder.index = i;
            }
        }
    }
}
