//! 启动时加载系统字体，不内嵌（中文字体内嵌要多 10 MB 以上）。
//! 字体文件用内存映射，只有用到的字形才读进内存。找不到时用 egui 自带的字体（没有中文）。

use egui::{FontData, FontDefinitions, FontFamily};

/// (路径, 字体集里的序号)
fn candidates() -> Vec<(std::path::PathBuf, u32)> {
    let mut v = Vec::new();
    #[cfg(windows)]
    {
        let dir = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        // 微软雅黑 UI（序号 1）的西文部分就是 Segoe UI 风格，行距比「微软雅黑」紧凑
        v.push((dir.join("msyh.ttc"), 1));
        v.push((dir.join("segoeui.ttf"), 0));
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

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut names = Vec::new();
    for (i, (path, index)) in candidates().into_iter().enumerate() {
        let Some(bytes) = map(&path) else { continue };
        let name = format!("system-{i}");
        let mut data = FontData::from_static(bytes);
        data.index = index;
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
    ctx.set_fonts(defs);
}
