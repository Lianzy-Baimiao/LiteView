//! 设置窗口：左侧分类、右侧内容，修改立即生效（没有「应用」按钮）。Ctrl+, 或右键菜单打开。

use egui::{RichText, Ui};

use super::App;
use crate::i18n::{fill, tr};
use crate::settings::{Background, DoubleClick, FitMode, Language, RedactEffect, RedactMode, Settings, SortKey, ThemeMode, WheelAction};
use crate::ui::{theme, widgets};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Page {
    #[default]
    General,
    View,
    Anim,
    Edit,
    Assoc,
    Keys,
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

#[cfg(windows)]
fn assoc_page(ui: &mut Ui, s: &mut Settings) -> Option<String> {
    super::assoc_ui::show(ui, s)
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
        // 窗口小时设置窗口跟着缩小（右边内容可滚动）
        let screen = ctx.content_rect().size();
        let (w, h) = ((screen.x - 24.0).clamp(480.0, 700.0), (screen.y - 24.0).clamp(300.0, 460.0));
        let (nav, inner) = (if w < 620.0 { 120.0 } else { 150.0 }, h - 20.0);
        let modal = egui::Modal::new(egui::Id::new("settings")).frame(frame).show(ctx, |ui| {
            ui.set_width(w);
            ui.set_height(h);
            ui.horizontal_top(|ui| {
                egui::Frame::new().fill(p.canvas).inner_margin(egui::Margin::same(10)).show(ui, |ui| ui.vertical(|ui| {
                    ui.set_width(nav);
                    ui.set_min_height(inner);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let t = tr();
                    for (pg, name) in [
                        (Page::General, t.s_general),
                        (Page::View, t.s_view),
                        (Page::Anim, t.s_anim),
                        (Page::Edit, t.s_edit),
                        (Page::Assoc, t.s_assoc),
                        (Page::Keys, t.s_keys),
                        (Page::About, t.s_about),
                    ] {
                        let on = page == pg;
                        let b = egui::Button::new(RichText::new(name).color(if on { p.text } else { p.text_weak }))
                            .fill(if on { p.hover } else { egui::Color32::TRANSPARENT })
                            .min_size(egui::vec2(nav, 32.0));
                        if ui.add(b).clicked() {
                            page = pg;
                        }
                    }
                }));
                egui::Frame::new().inner_margin(egui::Margin::same(20)).show(ui, |ui| ui.vertical(|ui| {
                    ui.set_width(w - nav - 20.0 - 40.0);
                    egui::ScrollArea::both().max_height(h - 40.0).show(ui, |ui| self.settings_page_ui(ui, page));
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
            Page::Keys => t.s_keys,
            Page::About => t.s_about,
        }).size(20.0));
        ui.add_space(14.0);
        match page {
            Page::General => {
                row(ui, t.s_language, |ui| choose(ui, &mut s.language, &[(Language::Auto, t.s_follow_system), (Language::ZhCn, "简体中文"), (Language::En, "English")]));
                row(ui, t.s_theme, |ui| choose(ui, &mut s.theme, &[(ThemeMode::System, t.s_follow_system), (ThemeMode::Light, t.s_light), (ThemeMode::Dark, t.s_dark)]));
                row(ui, t.s_accent, |ui| {
                    let mut custom = s.accent.is_some();
                    choose(ui, &mut custom, &[(false, t.s_follow_system), (true, t.s_custom)]);
                    if custom != s.accent.is_some() {
                        s.accent = custom.then(|| crate::platform::system_accent().unwrap_or([0x0A, 0x6C, 0xFF]));
                    }
                    if let Some(a) = &mut s.accent {
                        let mut c = egui::Color32::from_rgb(a[0], a[1], a[2]);
                        if egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque).changed() {
                            *a = [c.r(), c.g(), c.b()];
                        }
                    }
                });
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
                widgets::checkbox(ui, &mut s.lock_zoom, t.s_lock_zoom);
                widgets::checkbox(ui, &mut s.fit_window, t.s_fit_window);
            }
            Page::Anim => {
                widgets::checkbox(ui, &mut s.anim_autoplay, t.s_autoplay);
                widgets::checkbox(ui, &mut s.anim_always_loop, t.s_always_loop);
                ui.add_space(6.0);
                row(ui, t.speed, |ui| {
                    let names: Vec<String> = crate::viewer::playback::SPEEDS.iter().map(|v| format!("{v}×")).collect();
                    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                    let cur = crate::viewer::playback::SPEEDS.iter().position(|v| (*v - s.anim_speed).abs() < 1e-3).unwrap_or(2);
                    if let Some(i) = widgets::segmented(ui, &refs, cur) {
                        s.anim_speed = crate::viewer::playback::SPEEDS[i];
                    }
                });
                row(ui, t.s_anim_cache, |ui| number(ui, &mut s.anim_cache_mb, 64.0..=2048.0));
            }
            Page::Edit => {
                widgets::checkbox(ui, &mut s.confirm_overwrite, t.s_confirm_overwrite);
                widgets::checkbox(ui, &mut s.keep_metadata, t.s_keep_meta);
                ui.add_space(6.0);
                let mut q = u32::from(s.jpeg_quality);
                row(ui, t.s_jpeg_quality, |ui| number(ui, &mut q, 1.0..=100.0));
                s.jpeg_quality = q as u8;
                widgets::checkbox(ui, &mut s.webp_lossless, t.webp_lossless);
                if !s.webp_lossless {
                    let mut q = u32::from(s.webp_quality);
                    row(ui, t.webp_quality, |ui| number(ui, &mut q, 1.0..=100.0));
                    s.webp_quality = q as u8;
                }
                row(ui, t.s_undo_steps, |ui| number(ui, &mut s.undo_steps, 50.0..=500.0));
                row(ui, t.s_undo_mb, |ui| number(ui, &mut s.undo_mb, 128.0..=4096.0));
                row(ui, t.s_redact_default, |ui| {
                    choose(ui, &mut s.redact_mode, &[(RedactMode::Brush, t.p_brush), (RedactMode::Rect, t.p_rect)]);
                    choose(ui, &mut s.redact_effect, &[(RedactEffect::Mosaic, t.p_mosaic), (RedactEffect::Blur, t.p_blur)]);
                });
            }
            Page::Assoc => {
                if let Some(m) = assoc_page(ui, s) {
                    toast = Some(m);
                }
            }
            Page::Keys => {
                let p = theme::palette(ui.ctx());
                egui::Grid::new("keys").num_columns(3).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
                    for (i, line) in t.keys_table.lines().enumerate() {
                        for cell in line.split('|') {
                            let text = RichText::new(cell).size(13.0);
                            ui.label(if i == 0 { text.color(p.text_weak) } else { text.color(p.text) });
                        }
                        ui.end_row();
                    }
                });
            }
            Page::About => {
                let p = theme::palette(ui.ctx());
                ui.label(RichText::new(format!("{}  {}", t.app_name, fill(t.s_version, &[&env!("CARGO_PKG_VERSION")]))).size(16.0));
                ui.add_space(6.0);
                ui.label(RichText::new(t.s_license).color(p.text_weak));
                // 反馈问题时能直接截图：用的哪个显卡接口、跳过了哪些、上次崩在哪里
                let gpu = crate::startup::info();
                if !gpu.backend.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(fill(t.s_gpu, &[&gpu.backend])).color(p.text_weak));
                }
                if !gpu.failed.is_empty() {
                    ui.label(RichText::new(fill(t.s_gpu_skipped, &[&gpu.failed.join(", ")])).color(p.text_weak));
                }
                if let Some(problem) = &gpu.last_failure {
                    ui.label(RichText::new(fill(t.s_last_problem, &[problem])).color(p.text_weak));
                }
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
        if s.accent != before.accent {
            theme::set_accent(s.accent.or_else(crate::platform::system_accent));
            theme::install(ctx);
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
