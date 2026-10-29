//! 设置：JSON，原子写入并保留 `.bak`，损坏时自动回退。
//! 位置：Windows `%APPDATA%\LiteView\`、macOS `~/Library/Application Support/LiteView/`、
//! Linux `~/.config/liteview/`；绿色版存程序旁 `config`，MSIX 使用包的 LocalFolder。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Language {
    #[default]
    Auto,
    ZhCn,
    En,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// 打开图片时的显示方式
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FitMode {
    /// 大图缩小到完整显示，小图按 100%
    #[default]
    Fit,
    Actual,
    FitWidth,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WheelAction {
    #[default]
    Zoom,
    /// 滚轮切换上一张 / 下一张，Ctrl+滚轮缩放
    Navigate,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Background {
    #[default]
    Theme,
    Black,
    White,
    Checker,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Size,
    Type,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DoubleClick {
    #[default]
    ToggleFit,
    Fullscreen,
}

/// 省电模式：连续画面（拖动、缩放、动图）最多画多快；静止时本来就不画
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FrameCap {
    /// 高刷新率屏降到不低于 60 的整数分频（144 Hz → 72，160 Hz → 80），帧间隔均匀
    #[default]
    Auto,
    /// 最多 60 帧
    Hz60,
    /// 跟显示器刷新率
    Unlimited,
}

/// 打码方式：画笔涂抹 / 框选
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RedactMode {
    #[default]
    Brush,
    Rect,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RedactEffect {
    #[default]
    Mosaic,
    Blur,
    /// 油画笔涂抹
    Smear,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ShapeKind {
    Line,
    Arrow,
    #[default]
    Rect,
    RoundRect,
    Ellipse,
}

/// 打印方向
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PrintOrientation {
    /// 图比较宽就横向
    #[default]
    Auto,
    Portrait,
    Landscape,
}

/// 打印尺寸
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PrintSize {
    /// 保持比例，在能印到的范围里尽量大
    #[default]
    Fit,
    /// 指定尺寸（长边毫米，宽高锁定比例）
    Custom,
    /// 按图片记录的分辨率
    Actual,
}

/// 上次的打印设置（份数每次从 1 开始；驱动属性里改的只在本次运行内保留）
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct PrintSettings {
    /// 空 = 系统默认打印机
    pub printer: String,
    /// 纸张编号（DMPAPER_*）；0 = 打印机的默认纸张
    pub paper: i16,
    pub orientation: PrintOrientation,
    pub size: PrintSize,
    /// 指定尺寸的长边（毫米）；0 = 从适合页面的大小开始
    pub long_mm: f32,
    /// 四边边距（毫米，叠在打印机印不到的边上）
    pub margin_mm: f32,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self { printer: String::new(), paper: 0, orientation: PrintOrientation::Auto, size: PrintSize::Fit, long_mm: 0.0, margin_mm: 0.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub struct WindowState {
    pub pos: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
    /// 无边框（只有 Windows；编辑时临时恢复标题栏）。旧设置文件没有这一项，按关
    #[serde(default)]
    pub borderless: bool,
    /// 窗口置顶
    #[serde(default)]
    pub always_on_top: bool,
    /// 无边框、窗口比工具栏窄时把工具栏放到图片下面的小窗口里（默认关：多一个窗口和显卡设备）
    #[serde(default)]
    pub toolbar_outside: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub language: Language,
    pub theme: ThemeMode,
    pub single_instance: bool,
    pub confirm_delete: bool,
    pub wrap_around: bool,
    pub ask_default_app: bool,
    pub fit_mode: FitMode,
    pub wheel: WheelAction,
    pub smooth_zoom: bool,
    /// 放大超过这个比例（%）改用最近邻，看清每个像素
    pub nearest_above: u32,
    pub background: Background,
    pub double_click: DoubleClick,
    pub preload: u32,
    pub cache_mb: u32,
    pub lock_zoom: bool,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub drag_inertia: bool,
    pub anim_autoplay: bool,
    pub anim_always_loop: bool,
    pub anim_cache_mb: u32,
    /// 默认播放速度
    pub anim_speed: f32,
    /// 打开图片时窗口适应图片大小（不超过屏幕）
    pub fit_window: bool,
    pub frame_cap: FrameCap,
    /// 强调色；None = 跟随系统
    pub accent: Option<[u8; 3]>,
    pub confirm_overwrite: bool,
    pub jpeg_quality: u8,
    pub webp_lossless: bool,
    pub webp_quality: u8,
    pub window: WindowState,
    /// 文件关联要注册的扩展名
    pub assoc_exts: Vec<String>,
    // 编辑
    pub undo_steps: u32,
    pub undo_mb: u32,
    /// 保存时保留 EXIF（打过码的图默认不保留）
    pub keep_metadata: bool,
    pub redact_mode: RedactMode,
    pub redact_effect: RedactEffect,
    /// 用户改过的打码参数（None = 按图片大小自动算）
    pub mosaic_block: Option<u32>,
    pub blur_radius: Option<u32>,
    /// 涂抹的笔触宽度
    pub smear_width: Option<u32>,
    pub brush_size: Option<u32>,
    /// 主色 / 辅色（RGBA）
    pub color1: [u8; 4],
    pub color2: [u8; 4],
    /// 最近用过的自定义颜色
    pub recent_colors: Vec<[u8; 4]>,
    pub pencil_size: u32,
    pub paint_size: u32,
    pub paint_opacity: u32,
    /// 刷子是荧光笔（半透明、同一笔不叠加）
    pub highlighter: bool,
    pub eraser_size: u32,
    /// 橡皮擦擦成透明（否则擦成辅色）
    pub eraser_transparent: bool,
    pub fill_tolerance: u32,
    pub shape: ShapeKind,
    pub shape_outline: bool,
    pub shape_fill: bool,
    pub shape_width: u32,
    pub text_size: u32,
    pub text_font: String,
    pub text_bold: bool,
    pub text_italic: bool,
    pub text_underline: bool,
    pub text_strike: bool,
    /// 文字背景填辅色
    pub text_background: bool,
    pub print: PrintSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::Auto,
            theme: ThemeMode::System,
            single_instance: true,
            confirm_delete: true,
            wrap_around: true,
            ask_default_app: true,
            fit_mode: FitMode::Fit,
            wheel: WheelAction::Zoom,
            smooth_zoom: true,
            nearest_above: 400,
            background: Background::Theme,
            double_click: DoubleClick::ToggleFit,
            preload: 1,
            cache_mb: 512,
            lock_zoom: false,
            sort: SortKey::Name,
            sort_desc: false,
            drag_inertia: true,
            anim_autoplay: true,
            anim_always_loop: false,
            anim_cache_mb: 256,
            anim_speed: 1.0,
            fit_window: false,
            frame_cap: FrameCap::Auto,
            accent: None,
            confirm_overwrite: true,
            jpeg_quality: 90,
            webp_lossless: true,
            webp_quality: 90,
            window: WindowState::default(),
            assoc_exts: crate::platform::assoc::default_exts(),
            undo_steps: 100,
            undo_mb: 512,
            keep_metadata: true,
            redact_mode: RedactMode::Brush,
            redact_effect: RedactEffect::Mosaic,
            mosaic_block: None,
            blur_radius: None,
            smear_width: None,
            brush_size: None,
            color1: [0, 0, 0, 255],
            color2: [255, 255, 255, 255],
            recent_colors: Vec::new(),
            pencil_size: 1,
            paint_size: 8,
            paint_opacity: 100,
            highlighter: false,
            eraser_size: 20,
            eraser_transparent: false,
            fill_tolerance: 32,
            shape: ShapeKind::Rect,
            shape_outline: true,
            shape_fill: false,
            shape_width: 4,
            text_size: 24,
            text_font: String::new(),
            text_bold: false,
            text_italic: false,
            text_underline: false,
            text_strike: false,
            text_background: false,
            print: PrintSettings::default(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let p = file();
        for candidate in [p.clone(), p.with_extension("json.bak")] {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                match serde_json::from_str(&text) {
                    Ok(s) => return s,
                    Err(e) => log::warn!("设置文件 {} 损坏：{e}", candidate.display()),
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let p = file();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        if p.exists() {
            let _ = std::fs::copy(&p, p.with_extension("json.bak"));
        }
        if let Err(e) = crate::fsutil::atomic_write(&p, |w| w.write_all(text.as_bytes())) {
            log::warn!("保存设置失败：{e}");
        }
    }
}

fn file() -> PathBuf {
    config_dir().join("settings.json")
}

/// 配置目录（环境变量 `LITEVIEW_CONFIG_DIR` 可覆盖，测试用）
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("LITEVIEW_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    #[cfg(windows)]
    if crate::platform::package::is_packaged() {
        if let Some(dir) = crate::platform::package::local_data_dir() {
            return dir;
        }
        // Never write beside the read-only executable, even if a marker was added.
        return std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Packages")
            .join(&crate::platform::package::identity().expect("package identity is cached").family)
            .join("LocalState");
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()))
        && dir.join("LiteView.portable").exists()
    {
        return dir.join("config");
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("LiteView")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/LiteView")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("liteview")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerant_parse() {
        // 1.5.1 及以前的窗口设置没有无边框、置顶：照常读进来，按关
        let old: Settings = serde_json::from_str(r#"{"window": {"pos": [10.0, 20.0], "size": [800.0, 600.0], "maximized": true}}"#).unwrap();
        assert_eq!(old.window, WindowState { pos: Some([10.0, 20.0]), size: Some([800.0, 600.0]), maximized: true, borderless: false, always_on_top: false, toolbar_outside: false });
        let s: Settings = serde_json::from_str(r#"{"wrap_around": false, "unknown": 1}"#).unwrap();
        assert!(!s.wrap_around);
        assert_eq!(s.jpeg_quality, 90);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
