//! 自绘的扁平控件：图标按钮、浮动工具栏、分隔线、强调色按钮、分段按钮。

use egui::{Color32, CornerRadius, Frame, Margin, Response, Sense, Shadow, Stroke, Ui, Vec2, vec2};

use super::icons::{self, Icon};
use super::theme::{Palette, palette};

pub const BUTTON: f32 = 32.0;
pub const ICON: f32 = 20.0;

/// 32×32 的图标按钮；悬停提示「名称 + 快捷键」
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, shortcut: &str, enabled: bool) -> Response {
    icon_button_ex(ui, icon, tip, shortcut, enabled, false, None)
}

pub fn icon_button_ex(ui: &mut Ui, icon: Icon, tip: &str, shortcut: &str, enabled: bool, selected: bool, tint: Option<Color32>) -> Response {
    let p = palette(ui.ctx());
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BUTTON), sense);
    let label = tip.to_owned();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
    if ui.is_rect_visible(rect) {
        let bg = if selected {
            p.accent.gamma_multiply(0.18)
        } else if enabled && resp.is_pointer_button_down_on() {
            p.pressed
        } else if enabled && resp.hovered() {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
        let color = if !enabled {
            p.text_weak.gamma_multiply(0.5)
        } else if selected {
            p.accent
        } else {
            tint.unwrap_or(p.text)
        };
        icons::paint(ui, icon, egui::Rect::from_center_size(rect.center(), Vec2::splat(ICON)), color);
    }
    let text = if shortcut.is_empty() { tip.to_owned() } else { format!("{tip}  {shortcut}") };
    if enabled { resp.on_hover_text(text) } else { resp.on_disabled_hover_text(text) }
}

/// 浮动工具栏（胶囊形、淡淡的阴影）
pub fn floating_frame(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.floating)
        .stroke(Stroke::new(1.0, p.divider))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(6, 6))
        .shadow(Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }) })
}

pub fn separator(ui: &mut Ui) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, BUTTON), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range().shrink(6.0), Stroke::new(1.0, p.divider));
}

/// 强调色按钮（空状态的「打开图片」、确认框的主按钮）
pub fn accent_button(ui: &mut Ui, text: &str, danger: bool) -> Response {
    let p = palette(ui.ctx());
    let fill = if danger { p.danger } else { p.accent };
    let fg = if danger { Color32::WHITE } else { p.on_accent };
    ui.add(egui::Button::new(egui::RichText::new(text).color(fg)).fill(fill).corner_radius(CornerRadius::same(4)).min_size(vec2(88.0, 32.0)))
}

/// 普通按钮（次要操作）
pub fn plain_button(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui.ctx());
    ui.add(egui::Button::new(text).fill(p.hover).corner_radius(CornerRadius::same(4)).min_size(vec2(88.0, 32.0)))
}

/// 勾选框：16×16 圆角方框，勾选时强调色底 + 白色对勾
pub fn checkbox(ui: &mut Ui, value: &mut bool, text: &str) -> Response {
    let p = palette(ui.ctx());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::proportional(14.0), p.text);
    let size = vec2(16.0 + 8.0 + galley.size().x, 24.0f32.max(galley.size().y));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    let label = text.to_owned();
    let checked = *value;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, &label));
    let bx = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 8.0), Vec2::splat(16.0));
    if checked {
        ui.painter().rect_filled(bx, CornerRadius::same(4), p.accent);
        let pts = [bx.lerp_inside(vec2(0.22, 0.52)), bx.lerp_inside(vec2(0.43, 0.72)), bx.lerp_inside(vec2(0.78, 0.3))];
        ui.painter().line(pts.to_vec(), Stroke::new(1.8, p.on_accent));
    } else {
        let stroke = if resp.hovered() { p.text_weak } else { p.text_weak.gamma_multiply(0.7) };
        ui.painter().rect_stroke(bx, CornerRadius::same(4), Stroke::new(1.2, stroke), egui::StrokeKind::Inside);
    }
    ui.painter().galley(egui::pos2(bx.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    resp
}

/// 分段按钮：返回新选中的序号
pub fn segmented(ui: &mut Ui, items: &[&str], selected: usize) -> Option<usize> {
    let p = palette(ui.ctx());
    let mut out = None;
    Frame::new().fill(p.hover).corner_radius(CornerRadius::same(6)).inner_margin(Margin::same(2)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            for (i, text) in items.iter().enumerate() {
                let on = i == selected;
                let mut b = egui::Button::new(egui::RichText::new(*text).color(if on { p.text } else { p.text_weak }))
                    .corner_radius(CornerRadius::same(4))
                    .min_size(vec2(0.0, 26.0));
                b = b.fill(if on { p.surface } else { Color32::TRANSPARENT });
                if ui.add(b).clicked() && !on {
                    out = Some(i);
                }
            }
        });
    });
    out
}
