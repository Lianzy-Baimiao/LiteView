//! 启动时加载系统字体，不内嵌（中文字体内嵌要多 10 MB 以上）。
//! 字体文件用内存映射，只有用到的字形才读进内存。找不到时用 egui 自带的字体（没有中文）。

use egui::{FontData, FontDefinitions, FontFamily};

/// (路径, 字体集里的序号)
fn candidates() -> Vec<(std::path::PathBuf, u32)> {
    let mut v = Vec::new();
    #[cfg(windows)]
    {
        let dir = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        // 西文优先用原生 UI 字体；中文回退到微软雅黑 UI，避免西文也走中文字体。
        v.push((dir.join("segoeui.ttf"), 0));
        v.push((dir.join("msyh.ttc"), 1));
        v.push((dir.join("malgun.ttf"), 0));
        v.push((dir.join("seguisym.ttf"), 0));
    }
    #[cfg(target_os = "macos")]
    {
        for p in ["/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/STHeiti Light.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc"] {
            v.push((p.into(), 0));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Noto Sans CJK 的字体集里序号 2 是简体中文
        for p in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
        ] {
            v.push((p.into(), if p.ends_with(".ttc") { 2 } else { 0 }));
        }
        v.push(("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc".into(), 0));
        v.push(("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf".into(), 0));
    }
    v
}

fn map(path: &std::path::Path) -> Option<&'static [u8]> {
    let f = std::fs::File::open(path).ok()?;
    // SAFETY: 系统字体文件在运行期间不会被改写；映射一直保留到进程结束
    let m: &'static memmap2::Mmap = Box::leak(Box::new(unsafe { memmap2::Mmap::map(&f) }.ok()?));
    Some(&m[..])
}

/// 映射字体文件（同一个文件只映射一次；文字工具选字体时用）
pub fn map_cached(path: &std::path::Path) -> Option<&'static [u8]> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static MAPS: OnceLock<Mutex<HashMap<std::path::PathBuf, &'static [u8]>>> = OnceLock::new();
    let mut m = MAPS.get_or_init(Default::default).lock().ok()?;
    if let Some(b) = m.get(path) {
        return Some(b);
    }
    let b = map(path)?;
    m.insert(path.to_path_buf(), b);
    Some(b)
}

fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let mut names = Vec::new();
    for (i, (path, index)) in candidates().into_iter().enumerate() {
        let Some(bytes) = map(&path) else { continue };
        let name = format!("system-{i}");
        let mut data = FontData::from_static(bytes);
        data.index = index;
        // 雅黑的小字号 hinting 在分数缩放下会撑粗笔画；保留轮廓的灰度抗锯齿。
        #[cfg(windows)]
        if path.ends_with("msyh.ttc") {
            data.tweak.hinting = Some(false);
        }
        defs.font_data.insert(name.clone(), data.into());
        names.push(name);
    }
    if names.is_empty() {
        log::warn!("没有找到系统中文字体");
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = defs.families.entry(family.clone()).or_default();
        let at = if family == FontFamily::Proportional { 0 } else { list.len().min(1) };
        for (k, n) in names.iter().enumerate() {
            list.insert(at + k, n.clone());
        }
    }
    defs
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn ink_and_soft_edges(defs: FontDefinitions) -> (f64, f64) {
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Light);
        ctx.set_pixels_per_point(1.25);
        ctx.set_fonts(defs);
        let mut result = (0.0, 0.0);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            result = ui.ctx().fonts_mut(|f| {
                let galley = f.layout_no_wrap("打开图片 文件关联 常用格式 默认应用".into(), egui::FontId::proportional(14.0), egui::Color32::BLACK);
                let atlas = f.image();
                let (mut coverage, mut soft, mut ink) = (0.0, 0, 0);
                for row in &galley.rows {
                    for glyph in &row.glyphs {
                        let uv = glyph.uv_rect;
                        for y in uv.min[1]..uv.max[1] {
                            for x in uv.min[0]..uv.max[0] {
                                let alpha = atlas[(x as usize, y as usize)].a();
                                if alpha > 0 {
                                    coverage += f64::from(alpha) / 255.0;
                                    ink += 1;
                                    if alpha < 255 {
                                        soft += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                (coverage, f64::from(soft) / f64::from(ink))
            });
        });
        result
    }

    #[test]
    fn chinese_body_text_avoids_heavy_grid_fitting_at_125_percent() {
        let Some((i, _)) = candidates().iter().enumerate().find(|(_, (p, _))| p.ends_with("msyh.ttc") && p.is_file()) else { return };
        let actual = definitions();
        let mut baseline = actual.clone();
        std::sync::Arc::make_mut(baseline.font_data.get_mut(&format!("system-{i}")).unwrap()).tweak.hinting = Some(true);
        let (ink, edges) = ink_and_soft_edges(actual);
        let (baseline_ink, baseline_edges) = ink_and_soft_edges(baseline);
        assert!(edges > baseline_edges + 0.05, "soft edges: {edges:.3}, baseline: {baseline_edges:.3}");
        assert!(ink < baseline_ink && ink > baseline_ink * 0.8, "ink: {ink:.1}, baseline: {baseline_ink:.1}");
    }
}
