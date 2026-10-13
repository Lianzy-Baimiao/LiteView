//! 设计变量（PRD 7.2）与 egui 样式：扁平、无渐变，阴影只给浮在内容上面的东西。

use egui::{Color32, CornerRadius, Margin, Shadow, Stroke, Theme, Visuals};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub canvas: Color32,
    pub surface: Color32,
    pub floating: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
    /// 强调色按钮上的文字
    pub on_accent: Color32,
    pub danger: Color32,
    pub checker: [Color32; 2],
}

pub const LIGHT: Palette = Palette {
    dark: false,
    canvas: Color32::from_rgb(0xF3, 0xF3, 0xF3),
    surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    floating: Color32::from_rgba_premultiplied(235, 235, 235, 235),
    hover: Color32::from_rgba_premultiplied(0, 0, 0, 15),
    pressed: Color32::from_rgba_premultiplied(0, 0, 0, 26),
    divider: Color32::from_rgb(0xE5, 0xE5, 0xE5),
    text: Color32::from_rgb(0x1A, 0x1A, 0x1A),
    text_weak: Color32::from_rgb(0x61, 0x61, 0x61),
    accent: Color32::from_rgb(0x0A, 0x6C, 0xFF),
    on_accent: Color32::WHITE,
    danger: Color32::from_rgb(0xD1, 0x34, 0x38),
    checker: [Color32::from_rgb(0xFF, 0xFF, 0xFF), Color32::from_rgb(0xE6, 0xE6, 0xE6)],
};

pub const DARK: Palette = Palette {
    dark: true,
    canvas: Color32::from_rgb(0x1C, 0x1C, 0x1C),
    surface: Color32::from_rgb(0x2B, 0x2B, 0x2B),
    floating: Color32::from_rgba_premultiplied(40, 40, 40, 235),
    hover: Color32::from_rgba_premultiplied(20, 20, 20, 20),
    pressed: Color32::from_rgba_premultiplied(31, 31, 31, 31),
    divider: Color32::from_rgb(0x3A, 0x3A, 0x3A),
    text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
    text_weak: Color32::from_rgb(0xA8, 0xA8, 0xA8),
    accent: Color32::from_rgb(0x4C, 0xA0, 0xFF),
    on_accent: Color32::BLACK,
    danger: Color32::from_rgb(0xFF, 0x6B, 0x6B),
    checker: [Color32::from_rgb(0x3A, 0x3A, 0x3A), Color32::from_rgb(0x2E, 0x2E, 0x2E)],
};

/// 当前主题的设计变量
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    if ctx.theme() == Theme::Dark { &DARK } else { &LIGHT }
}

pub fn install(ctx: &egui::Context) {
    for (theme, p) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        ctx.style_mut_of(theme, |s| {
            s.visuals = visuals(p);
            s.spacing.button_padding = egui::vec2(10.0, 6.0);
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.menu_margin = Margin::same(6);
            s.spacing.interact_size.y = 28.0;
            s.animation_time = 0.12;
            s.interaction.selectable_labels = false;
            use egui::{FontFamily::Proportional, FontId, TextStyle};
            s.text_styles = [
                (TextStyle::Small, FontId::new(12.0, Proportional)),
                (TextStyle::Body, FontId::new(14.0, Proportional)),
                (TextStyle::Button, FontId::new(14.0, Proportional)),
                (TextStyle::Heading, FontId::new(20.0, Proportional)),
                (TextStyle::Monospace, FontId::new(13.0, egui::FontFamily::Monospace)),
            ]
            .into();
        });
    }
}

fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let r4 = CornerRadius::same(4);
    let shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 36 }) };
    v.panel_fill = p.canvas;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.divider);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.extreme_bg_color = p.canvas;
    v.faint_bg_color = p.canvas;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.override_text_color = None;
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.divider);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r4;
    for (s, fill) in [(&mut w.inactive, Color32::TRANSPARENT), (&mut w.hovered, p.hover), (&mut w.active, p.pressed), (&mut w.open, p.hover)] {
        s.bg_fill = fill;
        s.weak_bg_fill = fill;
        s.bg_stroke = Stroke::NONE;
        s.fg_stroke = Stroke::new(1.0, p.text);
        s.corner_radius = r4;
        s.expansion = 0.0;
    }
    v
}
