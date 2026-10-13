//! 设置：JSON，原子写入并保留 `.bak`，损坏时自动回退。
//! 位置：Windows `%APPDATA%\LiteView\`、macOS `~/Library/Application Support/LiteView/`、
//! Linux `~/.config/liteview/`；程序旁有 `LiteView.portable` 时存程序旁的 `config` 目录。

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

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub struct WindowState {
    pub pos: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
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
    pub confirm_overwrite: bool,
    pub jpeg_quality: u8,
    pub window: WindowState,
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
            confirm_overwrite: true,
            jpeg_quality: 90,
            window: WindowState::default(),
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
        let s: Settings = serde_json::from_str(r#"{"wrap_around": false, "unknown": 1}"#).unwrap();
        assert!(!s.wrap_around);
        assert_eq!(s.jpeg_quality, 90);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
