//! 属性栏（随当前工具变化）与状态栏（光标位置、选区、图片尺寸、文件大小、缩放）。

use egui::{Align, Layout, RichText, Ui};

use super::{Editor, Tool};
use crate::app::App;
use crate::i18n::{fill, tr};
use crate::settings::{RedactEffect, RedactMode};
use crate::ui::{theme, widgets};

/// 「名称  滑块  数值」
pub(crate) fn slider_row(ui: &mut Ui, label: &str, value: &mut u32, lo: u32, hi: u32, log: bool, unit: &str) -> bool {
    let p = theme::palette(ui.ctx());
    ui.label(RichText::new(label).color(p.text_weak));
    let mut v = *value as f32;
    let r = if log { widgets::slider_log(ui, &mut v, lo as f32, hi as f32, 140.0) } else { widgets::slider(ui, &mut v, lo as f32..=hi as f32, 140.0) };
    let changed = r.changed() && v as u32 != *value;
    if changed {
        *value = v as u32;
    }
    ui.add_sized([56.0, 24.0], egui::Label::new(RichText::new(format!("{}{unit}", *value)).size(13.0)));
    ui.add_space(8.0);
    changed
}

fn seg<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, items: &[(T, &str)]) {
    let names: Vec<&str> = items.iter().map(|(_, n)| *n).collect();
    let cur = items.iter().position(|(v, _)| v == value).unwrap_or(0);
    if let Some(i) = widgets::segmented(ui, &names, cur) {
        *value = items[i].0;
    }
    ui.add_space(10.0);
}

impl App {
    pub(crate) fn edit_props(&mut self, ui: &mut Ui, ed: &mut Editor) {
        let t = tr();
        let p = theme::palette(ui.ctx());
        match ed.tool {
            Tool::Redact => {
                let (block, blur, smear, size) = ed.redact_params(&self.settings);
                let s = &mut self.settings;
                seg(ui, &mut s.redact_mode, &[(RedactMode::Brush, t.p_brush), (RedactMode::Rect, t.p_rect)]);
                seg(ui, &mut s.redact_effect, &[(RedactEffect::Mosaic, t.p_mosaic), (RedactEffect::Blur, t.p_blur), (RedactEffect::Smear, t.p_smear)]);
                if s.redact_mode == RedactMode::Brush {
                    let mut v = size;
                    if slider_row(ui, t.p_size, &mut v, 4, 1000, true, " px") {
                        s.brush_size = Some(v);
                    }
                }
                let (mut v, slot) = match s.redact_effect {
                    RedactEffect::Mosaic => (block, &mut s.mosaic_block),
                    RedactEffect::Blur => (blur, &mut s.blur_radius),
                    RedactEffect::Smear => (smear, &mut s.smear_width),
                };
                if slider_row(ui, t.p_level, &mut v, 8, 128, false, "") {
                    *slot = Some(v);
                }
            }
            Tool::Select => {
                ui.label(RichText::new(t.p_select_hint).color(p.text_weak).size(13.0));
                ui.add_space(12.0);
                if ui.link(t.p_select_all).clicked() {
                    self.edit_select_all(ed);
                }
                if ed.sel.is_some() && ui.link(t.t_crop).clicked() {
                    self.edit_crop(ed);
                }
            }
            Tool::Pencil => {
                slider_row(ui, t.p_width, &mut self.settings.pencil_size, 1, 100, true, " px");
            }
            Tool::Brush => {
                let s = &mut self.settings;
                slider_row(ui, t.p_width, &mut s.paint_size, 1, 100, true, " px");
                if !s.highlighter {
                    slider_row(ui, t.p_opacity, &mut s.paint_opacity, 1, 100, false, "%");
                }
                widgets::checkbox(ui, &mut s.highlighter, t.p_highlighter);
            }
            Tool::Eraser => {
                let s = &mut self.settings;
                slider_row(ui, t.p_size, &mut s.eraser_size, 2, 400, true, " px");
                if ed.src.canvas_alpha {
                    ui.label(RichText::new(t.p_erase_to).color(p.text_weak));
                    seg(ui, &mut s.eraser_transparent, &[(true, t.p_transparent), (false, t.p_secondary)]);
                }
            }
            Tool::Fill => {
                slider_row(ui, t.p_tolerance, &mut self.settings.fill_tolerance, 0, 255, false, "");
            }
            Tool::Picker => {
                ui.label(RichText::new(t.p_picker_hint).color(p.text_weak).size(13.0));
            }
            Tool::Shape => self.shape_props(ui, ed),
            Tool::Text => self.text_props(ui, ed),
        }
    }

    pub(crate) fn edit_status(&mut self, ui: &mut Ui, ed: &mut Editor) {
        let t = tr();
        let p = theme::palette(ui.ctx());
        let txt = |ui: &mut Ui, s: String| {
            ui.label(RichText::new(s).size(12.0).color(p.text_weak));
            ui.add_space(14.0);
        };
        let (w, h) = (ed.doc.width(), ed.doc.height());
        if let Some(c) = ed.cursor.filter(|c| c.x >= 0.0 && c.y >= 0.0 && c.x < w as f32 && c.y < h as f32) {
            txt(ui, format!("{}, {} px", c.x.floor(), c.y.floor()));
        }
        let sel = ed.floating.as_ref().map(|f| (f.rect.width().round() as u32, f.rect.height().round() as u32)).or(ed.sel.map(|s| (s.width(), s.height())));
        if let Some((sw, sh)) = sel {
            txt(ui, fill(t.st_sel, &[&sw, &sh]));
        }
        txt(ui, format!("{w} × {h}"));
        if ed.src.file_size > 0 {
            txt(ui, crate::app::info::size_text(ed.src.file_size));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let small = |s: &str| egui::Button::new(RichText::new(s).size(14.0)).frame_when_inactive(false).min_size(egui::vec2(24.0, 22.0));
            if ui.add(small("+")).on_hover_text(t.zoom_in).clicked() {
                self.zoom_step(true);
            }
            ui.add_sized([48.0, 22.0], egui::Label::new(RichText::new(format!("{:.0}%", self.view.zoom * 100.0)).size(12.0)));
            if ui.add(small("−")).on_hover_text(t.zoom_out).clicked() {
                self.zoom_step(false);
            }
        });
    }
}
