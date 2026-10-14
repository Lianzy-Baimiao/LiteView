//! 主色 / 辅色（PRD 6.7.4）：两个色块，点击弹出调色板（20 个预设色 + 最近用过的 10 个 + 自定义）；X 交换。

use egui::{Color32, CornerRadius, Popup, PopupCloseBehavior, Sense, Stroke, Vec2, vec2};

use super::Editor;
use crate::app::App;
use crate::i18n::tr;
use crate::ui::icons::Icon;
use crate::ui::{theme, widgets};

/// 扁平风格的预设色：黑白灰 + 常用标注色
pub(crate) const PALETTE: [[u8; 3]; 20] = [
    [0x00, 0x00, 0x00], [0x5C, 0x5C, 0x5C], [0x9E, 0x9E, 0x9E], [0xD6, 0xD6, 0xD6], [0xFF, 0xFF, 0xFF],
    [0xE5, 0x48, 0x4D], [0xF7, 0x6B, 0x15], [0xFF, 0xC5, 0x3D], [0x30, 0xA4, 0x6C], [0x0A, 0x84, 0xFF],
    [0x8E, 0x4E, 0xC6], [0xD6, 0x40, 0x9F], [0x7A, 0x4A, 0x2A], [0xA3, 0xE6, 0x35], [0x12, 0xA5, 0x94],
    [0x05, 0xA2, 0xC2], [0x1E, 0x3A, 0x8A], [0xFF, 0xB3, 0xC1], [0xFF, 0xE8, 0x99], [0xB4, 0xE4, 0xFF],
];

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn swatch(ui: &mut egui::Ui, color: [u8; 4], size: f32, selected: bool) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    ui.painter().rect_filled(rect, CornerRadius::same(4), c32(color));
    let stroke = if selected { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, p.divider) };
    ui.painter().rect_stroke(rect, CornerRadius::same(4), stroke, egui::StrokeKind::Inside);
    resp
}

/// 解析 #RRGGBB / RRGGBB
fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

impl App {
    pub(crate) fn color_swatches(&mut self, ui: &mut egui::Ui, _ed: &mut Editor) {
        let t = tr();
        for primary in [true, false] {
            let color = if primary { self.settings.color1 } else { self.settings.color2 };
            let r = swatch(ui, color, 24.0, false).on_hover_text(if primary { t.color1 } else { t.color2 });
            Popup::menu(&r).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| self.palette_ui(ui, primary));
        }
        if widgets::icon_button(ui, Icon::Swap, t.swap_colors, "X", true).clicked() {
            std::mem::swap(&mut self.settings.color1, &mut self.settings.color2);
        }
    }

    /// 调色板：点预设色、最近用过的、或自定义（色相 / 饱和度 / 亮度、HEX）
    fn palette_ui(&mut self, ui: &mut egui::Ui, primary: bool) {
        let t = tr();
        let p = theme::palette(ui.ctx());
        let cur = if primary { self.settings.color1 } else { self.settings.color2 };
        let mut pick = None;
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for row in PALETTE.chunks(5) {
            ui.horizontal(|ui| {
                for c in row {
                    let c = [c[0], c[1], c[2], 255];
                    if swatch(ui, c, 26.0, c == cur).clicked() {
                        pick = Some(c);
                    }
                }
            });
        }
        if !self.settings.recent_colors.is_empty() {
            ui.label(egui::RichText::new(t.recent).size(12.0).color(p.text_weak));
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(5.0 * 32.0);
                for c in self.settings.recent_colors.clone() {
                    if swatch(ui, c, 26.0, c == cur).clicked() {
                        pick = Some(c);
                    }
                }
            });
        }
        ui.separator();
        ui.label(egui::RichText::new(t.custom_color).size(12.0).color(p.text_weak));
        let mut c = c32(cur);
        if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            pick = Some([r, g, b, 255]);
        }
        let id = egui::Id::new(("hex", primary));
        let mut hex = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| format!("#{:02X}{:02X}{:02X}", cur[0], cur[1], cur[2]));
        let r = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(90.0).font(egui::TextStyle::Monospace));
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id, hex.clone()));
            if let Some(c) = parse_hex(&hex) {
                pick = Some(c);
            }
        }
        if !r.has_focus() && !r.changed() {
            ui.data_mut(|d| d.remove::<String>(id));
        }
        if let Some(c) = pick {
            if primary {
                self.settings.color1 = c;
            } else {
                self.settings.color2 = c;
            }
        }
    }

    /// 真正画下去的自定义颜色记进「最近使用」（预设色不记；拖动取色时不会记一串）
    pub(crate) fn remember_color(&mut self, c: [u8; 4]) {
        if PALETTE.iter().any(|p| p[..] == c[..3]) {
            return;
        }
        let r = &mut self.settings.recent_colors;
        r.retain(|x| *x != c);
        r.insert(0, c);
        r.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex() {
        assert_eq!(super::parse_hex("#E5484D"), Some([0xE5, 0x48, 0x4D, 255]));
        assert_eq!(super::parse_hex("0a84ff"), Some([0x0A, 0x84, 0xFF, 255]));
        assert_eq!(super::parse_hex("#12"), None);
    }
}
