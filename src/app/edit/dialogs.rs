//! 编辑模式的对话框：调整大小（百分比 / 像素、保持宽高比）、保存选项（JPEG 质量、是否保留 EXIF）。

use egui::RichText;

use super::Editor;
use crate::app::App;
use crate::i18n::tr;
use crate::ui::{theme, widgets};

pub(crate) struct ResizeState {
    pub percent: bool,
    pub w: String,
    pub h: String,
    pub keep: bool,
}

/// 调整后的尺寸（不合理时 None）
fn resize_target(s: &ResizeState, w0: u32, h0: u32) -> Option<(u32, u32)> {
    let (a, b) = (s.w.trim().parse::<f32>().ok()?, s.h.trim().parse::<f32>().ok()?);
    let (w, h) = if s.percent { (w0 as f32 * a / 100.0, h0 as f32 * b / 100.0) } else { (a, b) };
    let (w, h) = (w.round(), h.round());
    let ok = w >= 1.0 && h >= 1.0 && w <= crate::decode::MAX_SIDE as f32 && h <= crate::decode::MAX_SIDE as f32 && w * h <= 300e6;
    ok.then_some((w as u32, h as u32))
}

impl App {
    pub(crate) fn open_resize(&mut self, ed: &mut Editor) {
        self.edit_finalize(ed);
        ed.resize = Some(ResizeState { percent: true, w: "100".into(), h: "100".into(), keep: true });
    }

    pub(crate) fn edit_dialogs(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        if ed.resize.is_some() {
            self.resize_dialog(ctx, ed);
        }
        if ed.save_options.is_some() {
            self.save_options_dialog(ctx, ed);
        }
    }

    fn resize_dialog(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        let t = tr();
        let (w0, h0) = (ed.doc.width(), ed.doc.height());
        let Some(st) = &mut ed.resize else { return };
        let mut apply = None;
        let mut close = false;
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(egui::CornerRadius::same(8));
        let modal = egui::Modal::new(egui::Id::new("resize")).frame(frame).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(RichText::new(t.resize_title).size(16.0));
            ui.add_space(10.0);
            let mut mode = usize::from(!st.percent);
            if let Some(m) = widgets::segmented(ui, &[t.by_percent, t.by_pixels], mode) {
                mode = m;
                st.percent = mode == 0;
                (st.w, st.h) = if st.percent { ("100".into(), "100".into()) } else { (w0.to_string(), h0.to_string()) };
            }
            ui.add_space(10.0);
            let unit = if st.percent { "%" } else { "px" };
            let mut edited = None;
            for (i, label) in [(0, t.r_width), (1, t.r_height)] {
                ui.horizontal(|ui| {
                    ui.add_sized([40.0, 24.0], egui::Label::new(label));
                    let field = if i == 0 { &mut st.w } else { &mut st.h };
                    let r = ui.add(egui::TextEdit::singleline(field).desired_width(110.0));
                    if r.changed() {
                        edited = Some(i);
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        apply = resize_target(st, w0, h0);
                    }
                    ui.label(unit);
                });
            }
            if st.keep
                && let Some(i) = edited
            {
                let src = if i == 0 { &st.w } else { &st.h };
                if let Ok(v) = src.trim().parse::<f32>() {
                    let other = if st.percent {
                        v
                    } else if i == 0 {
                        (v * h0 as f32 / w0 as f32).round()
                    } else {
                        (v * w0 as f32 / h0 as f32).round()
                    };
                    let s = format!("{other}");
                    if i == 0 { st.h = s } else { st.w = s }
                }
            }
            ui.add_space(6.0);
            widgets::checkbox(ui, &mut st.keep, t.keep_ratio);
            let target = resize_target(st, w0, h0);
            ui.label(RichText::new(target.map_or("—".into(), |(w, h)| format!("{w} × {h}"))).size(12.0).color(theme::palette(ctx).text_weak));
            ui.add_space(14.0);
            if target.is_some() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                apply = target;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled_ui(target.is_some(), |ui| widgets::accent_button(ui, t.t_resize, false)).inner.clicked() {
                    apply = target;
                }
                if widgets::plain_button(ui, t.cancel).clicked() {
                    close = true;
                }
            });
        });
        if let Some((w, h)) = apply {
            ed.resize = None;
            self.edit_resize(ed, w, h);
        } else if close || modal.should_close() {
            ed.resize = None;
        }
    }

    fn save_options_dialog(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        let t = tr();
        let p = theme::palette(ctx);
        let redacted = ed.doc.redacted();
        let has_exif = ed.src.exif.is_some();
        let Some(job) = &mut ed.save_options else { return };
        let (mut go, mut close) = (false, false);
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(egui::CornerRadius::same(8));
        let modal = egui::Modal::new(egui::Id::new("save-options")).frame(frame).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(t.save_options).size(16.0));
            ui.label(RichText::new(crate::natsort::file_name(&job.dest)).size(12.0).color(p.text_weak));
            ui.add_space(12.0);
            if job.format == crate::save::Format::WebP {
                widgets::checkbox(ui, &mut job.webp_lossless, t.webp_lossless);
            }
            if job.format == crate::save::Format::Jpeg || (job.format == crate::save::Format::WebP && !job.webp_lossless) {
                ui.horizontal(|ui| {
                    ui.label(if job.format == crate::save::Format::WebP { t.webp_quality } else { t.s_jpeg_quality });
                    let mut q = f32::from(job.quality);
                    widgets::slider(ui, &mut q, 1.0..=100.0, 180.0);
                    job.quality = q as u8;
                    ui.label(job.quality.to_string());
                });
                ui.add_space(6.0);
            }
            if has_exif && job.format.supports_exif() {
                widgets::checkbox(ui, &mut job.keep_meta, t.keep_meta);
                if redacted {
                    ui.label(RichText::new(t.keep_meta_redacted).size(12.0).color(p.danger));
                }
            }
            if has_exif && !job.format.supports_exif() {
                ui.label(RichText::new(t.save_drops_exif).size(12.0).color(p.text_weak));
            }
            if ed.src.canvas_icc.is_some() && !job.format.supports_icc() {
                ui.label(RichText::new(t.save_drops_icc).size(12.0).color(p.danger));
            }
            if !job.format.full_alpha() {
                ui.label(RichText::new(if job.format == crate::save::Format::Gif { t.save_binary_alpha } else { t.save_flattens_alpha }).size(12.0).color(p.text_weak));
            }
            ui.add_space(16.0);
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                go = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::accent_button(ui, t.save, false).clicked() {
                    go = true;
                }
                if widgets::plain_button(ui, t.cancel).clicked() {
                    close = true;
                }
            });
        });
        if go {
            if let Some(job) = ed.save_options.take() {
                if job.format == crate::save::Format::WebP {
                    self.settings.webp_quality = job.quality;
                    self.settings.webp_lossless = job.webp_lossless;
                } else if job.format == crate::save::Format::Jpeg {
                    self.settings.jpeg_quality = job.quality;
                }
                self.edit_start_save(ed, job);
            }
        } else if close || modal.should_close() {
            ed.save_options = None;
            ed.after = None;
        }
    }
}
