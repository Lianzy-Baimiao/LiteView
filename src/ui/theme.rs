//! 设计变量（PRD 7.2）与 egui 样式：扁平、无渐变，阴影只给浮在内容上面的东西。

use std::sync::RwLock;

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

/// 正在用的浅色 / 深色设计变量（强调色可换）
static ACTIVE: RwLock<(&'static Palette, &'static Palette)> = RwLock::new((&LIGHT, &DARK));

/// 当前主题的设计变量
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    let (light, dark) = ACTIVE.read().map_or((&LIGHT, &DARK), |g| *g);
    if ctx.theme() == Theme::Dark { dark } else { light }
}

/// 相对亮度（WCAG）
fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// 换强调色：调深 / 调浅到在面板上对比度 ≥ 4.5:1（文字、链接用得上），按钮上的字选黑或白里对比度高的
fn with_accent(base: &Palette, rgb: [u8; 3]) -> Palette {
    let mut c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let toward = if base.dark { Color32::WHITE } else { Color32::BLACK };
    for _ in 0..20 {
        if contrast(c, base.surface) >= 4.5 {
            break;
        }
        c = c.lerp_to_gamma(toward, 0.1);
    }
    let on = if contrast(Color32::WHITE, c) >= contrast(Color32::BLACK, c) { Color32::WHITE } else { Color32::BLACK };
    Palette { accent: c, on_accent: on, ..*base }
}

/// 设置强调色（None = 默认的蓝色）；之后要再调用一次 install
pub fn set_accent(rgb: Option<[u8; 3]>) {
    let pair: (&'static Palette, &'static Palette) = match rgb {
        None => (&LIGHT, &DARK),
        // 每次换色泄漏两个很小的结构，换得很少
        Some(c) => (Box::leak(Box::new(with_accent(&LIGHT, c))), Box::leak(Box::new(with_accent(&DARK, c)))),
    };
    if let Ok(mut g) = ACTIVE.write() {
        *g = pair;
    }
}

pub fn install(ctx: &egui::Context) {
    let (light, dark) = ACTIVE.read().map_or((&LIGHT, &DARK), |g| *g);
    for (theme, p) in [(Theme::Light, light), (Theme::Dark, dark)] {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_contrast_meets_wcag_aa() {
        for p in [&LIGHT, &DARK] {
            for bg in [p.canvas, p.surface] {
                assert!(contrast(p.text, bg) >= 4.5 && contrast(p.text_weak, bg) >= 4.5, "dark={}", p.dark);
            }
            assert!(contrast(p.accent, p.surface) >= 4.5);
            assert!(contrast(p.on_accent, p.accent) >= 4.5);
        }
    }

    #[test]
    fn custom_accent_is_adjusted_for_contrast() {
        // 很浅的黄色：浅色主题下要调深
        let light = with_accent(&LIGHT, [0xFF, 0xE0, 0x66]);
        assert!(contrast(light.accent, LIGHT.surface) >= 4.5);
        assert!(contrast(light.on_accent, light.accent) >= 3.0);
        // 很深的蓝色：深色主题下要调浅
        let dark = with_accent(&DARK, [0x10, 0x20, 0x80]);
        assert!(contrast(dark.accent, DARK.surface) >= 4.5);
        // Windows 默认蓝在浅色主题下不用改
        assert_eq!(with_accent(&LIGHT, [0x00, 0x78, 0xD4]).accent, Color32::from_rgb(0x00, 0x78, 0xD4));
    }
}
