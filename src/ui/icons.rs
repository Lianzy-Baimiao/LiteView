//! 图标：Fluent UI System Icons（MIT，assets/icons），编进程序里。
//! 按当前缩放栅格化成白色蒙版纹理，画的时候按需要的颜色着色。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, ColorImage, TextureHandle, TextureOptions};

macro_rules! icons {
    ($($name:ident => $file:literal,)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum Icon { $($name,)* }
        impl Icon {
            fn svg(self) -> &'static str {
                match self { $(Icon::$name => include_str!(concat!("../../assets/icons/", $file, ".svg")),)* }
            }
        }
    };
}

icons! {
    Prev => "prev", Next => "next", ZoomIn => "zoom_in", ZoomOut => "zoom_out", Fit => "fit", Actual => "actual",
    RotateCw => "rotate_cw", RotateCcw => "rotate_ccw", Edit => "edit", Delete => "delete", Info => "info",
    Play => "play", Pause => "pause", FramePrev => "frame_prev", FrameNext => "frame_next", Open => "open",
    Image => "image", ImageOff => "image_off", Paste => "paste", Settings => "settings", Fullscreen => "fullscreen",
    Close => "close", Warning => "warning", Undo => "undo", Redo => "redo", Select => "select", Crop => "crop",
    Resize => "resize", Pencil => "pencil", Brush => "brush", Highlighter => "highlighter", Eraser => "eraser",
    Fill => "fill", Picker => "picker", Text => "text", Shapes => "shapes", Line => "line", Arrow => "arrow",
    Rect => "rect", RoundRect => "round_rect", Ellipse => "ellipse", Mosaic => "mosaic", Blur => "blur",
    Swap => "swap", Done => "done", FlipH => "flip_h", FlipV => "flip_v", Copy => "copy", Cut => "cut",
    Save => "save", More => "more", Dropdown => "dropdown", Bold => "bold", Italic => "italic",
    Underline => "underline", Strike => "strike", Lasso => "lasso", Lock => "lock",
}

type Cache = Arc<Mutex<HashMap<(Icon, u32), TextureHandle>>>;

/// 取图标纹理（size 为逻辑像素）
pub fn texture(ctx: &egui::Context, icon: Icon, size: f32) -> TextureHandle {
    let px = (size * ctx.pixels_per_point()).round().max(1.0) as u32;
    let cache: Cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::new("icon-cache")).clone());
    let mut map = cache.lock().expect("icon cache");
    map.entry((icon, px))
        .or_insert_with(|| ctx.load_texture(format!("icon-{icon:?}-{px}"), rasterize(icon.svg(), px), TextureOptions::LINEAR))
        .clone()
}

/// 把 SVG 画成白色蒙版（透明度 = 图形覆盖率）
pub fn rasterize(svg: &str, px: u32) -> ColorImage {
    let blank = || ColorImage::new([px as usize, px as usize], vec![Color32::TRANSPARENT; (px * px) as usize]);
    let Ok(tree) = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()) else { return blank() };
    let Some(mut pix) = resvg::tiny_skia::Pixmap::new(px, px) else { return blank() };
    let s = tree.size();
    let scale = px as f32 / s.width().max(s.height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    let pixels = pix.data().as_chunks::<4>().0.iter().map(|p| Color32::from_rgba_premultiplied(p[3], p[3], p[3], p[3])).collect();
    ColorImage::new([px as usize, px as usize], pixels)
}

/// 彩色 SVG（程序图标）→ RGBA（非预乘）
pub fn rasterize_color(svg: &str, px: u32) -> Vec<u8> {
    let mut out = vec![0u8; (px * px * 4) as usize];
    let Ok(tree) = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()) else { return out };
    let Some(mut pix) = resvg::tiny_skia::Pixmap::new(px, px) else { return out };
    let s = tree.size();
    let scale = px as f32 / s.width().max(s.height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    for (o, p) in out.as_chunks_mut::<4>().0.iter_mut().zip(pix.pixels()) {
        let c = p.demultiply();
        o.copy_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

/// 画一个着色的图标
pub fn paint(ui: &egui::Ui, icon: Icon, rect: egui::Rect, color: Color32) {
    let tex = texture(ui.ctx(), icon, rect.width());
    ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_rasterize() {
        for icon in [Icon::Prev, Icon::Mosaic, Icon::Lasso, Icon::Ellipse] {
            let img = rasterize(icon.svg(), 40);
            assert!(img.pixels.iter().any(|p| p.a() > 200), "{icon:?} is blank");
        }
    }
}
