//! 设置：JSON，原子写入并保留 `.bak`，损坏时自动回退。
//! 位置：Windows `%APPDATA%\LiteView\`、macOS `~/Library/Application Support/LiteView/`、
//! Linux `~/.config/liteview/`；绿色版存程序旁 `config`，MSIX 使用包的 LocalFolder。

use std::path::{Path, PathBuf};

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
    /// 关闭窗口（看完就关）
    Close,
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
    /// 排序「跟随资源管理器」（只有 Windows）：从资源管理器打开时按那个窗口的先后，其它情况按文件名；这时不看 sort / sort_desc。
    /// 单独一项而不是 SortKey 的新值：退回旧版本时 sort 仍认得，不会被重置
    pub sort_explorer: bool,
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
            sort_explorer: true,
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
                match Self::parse(&text) {
                    Ok((s, dropped)) => {
                        if !dropped.is_empty() {
                            log::warn!("设置文件 {} 里这几项认不出，用默认值：{}", candidate.display(), dropped.join("、"));
                        }
                        return s;
                    }
                    Err(e) => log::warn!("设置文件 {} 损坏：{e}", candidate.display()),
                }
            }
        }
        Self::default()
    }

    /// 读设置文件。某一项认不出（新版本加的选项、手改错的值）时只有这一项用默认值，其余照用；
    /// 以前整个文件作废、全部回到默认（.bak 是同样的内容，也救不回来）。返回认不出的那几项（如 `window.size`）
    pub fn parse(text: &str) -> Result<(Self, Vec<String>), serde_json::Error> {
        let user: serde_json::Value = serde_json::from_str(text)?;
        let (mut s, dropped) = match serde_json::from_value::<Self>(user.clone()) {
            Ok(s) => (s, Vec::new()),
            Err(_) => {
                let mut root = serde_json::to_value(Self::default())?;
                let mut dropped = Vec::new();
                merge_valid(&mut root, String::new(), &user, &mut dropped);
                (serde_json::from_value(root)?, dropped)
            }
        };
        // 1.10.0 以前的设置文件没有「跟随资源管理器」：还是默认排序（文件名、升序）的换成跟随，自己选过别的排序的照旧
        if user.get("sort_explorer").is_none() {
            s.sort_explorer = s.sort == SortKey::Name && !s.sort_desc;
        }
        Ok((s, dropped))
    }

    /// 浏览顺序。跟随资源管理器只在 Windows 上有；找不到资源管理器窗口时按文件名升序
    pub fn order(&self) -> crate::viewer::folder::Order {
        if cfg!(windows) && self.sort_explorer {
            crate::viewer::folder::Order { key: SortKey::Name, desc: false, explorer: true }
        } else {
            crate::viewer::folder::Order { key: self.sort, desc: self.sort_desc, explorer: false }
        }
    }

    /// 存设置。多窗口时几个 LiteView 共用这个文件：只把这个窗口改过的项（和 `base`，也就是上次读进来或存过的比）
    /// 写进文件现在的内容，别的窗口存过的照留。以前整个覆盖：后关的窗口把先关的窗口改的设置（包括关掉多窗口）又改了回去。
    /// 文件读不出来时整个写
    pub fn save(&self, base: &Settings) {
        self.save_to(&file(), base);
    }

    fn save_to(&self, p: &Path, base: &Settings) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // 读、合并、写回之间别的窗口不插进来
        let _lock = save_lock(p);
        let on_disk = std::fs::read_to_string(p).ok().and_then(|t| Self::parse(&t).ok()).map(|(s, _)| s);
        let out = match on_disk {
            Some(disk) if disk != *base => merge(&disk, base, self),
            _ => self.clone(),
        };
        let Ok(text) = serde_json::to_string_pretty(&out) else { return };
        if p.exists() {
            let _ = std::fs::copy(p, p.with_extension("json.bak"));
        }
        if let Err(e) = crate::fsutil::atomic_write(p, |w| w.write_all(text.as_bytes())) {
            log::warn!("保存设置失败：{e}");
        }
    }
}

fn file() -> PathBuf {
    config_dir().join("settings.json")
}

/// 文件里现在的 disk 加上这个窗口改过的项（mine 和 base 不一样的）。对象一层层比，`window.pos` 和 `window.borderless`
/// 各算一项；数组、数值整个算一项
fn merge(disk: &Settings, base: &Settings, mine: &Settings) -> Settings {
    let (Ok(mut d), Ok(b), Ok(m)) = (serde_json::to_value(disk), serde_json::to_value(base), serde_json::to_value(mine)) else { return mine.clone() };
    apply_changes(&mut d, &b, &m);
    serde_json::from_value(d).unwrap_or_else(|_| mine.clone())
}

fn apply_changes(disk: &mut serde_json::Value, base: &serde_json::Value, mine: &serde_json::Value) {
    if base == mine {
        return;
    }
    match (disk.as_object_mut(), base.as_object(), mine.as_object()) {
        (Some(d), Some(b), Some(m)) => {
            for (key, value) in m {
                if let (Some(dv), Some(bv)) = (d.get_mut(key), b.get(key)) {
                    apply_changes(dv, bv, value);
                }
            }
        }
        _ => *disk = mine.clone(),
    }
}

/// 几个 LiteView 同时存设置时一个一个来（多窗口时一起关掉）。最多等 2 秒，等不到照样存
#[cfg(windows)]
fn save_lock(p: &Path) -> impl Sized {
    // 按文件路径起名（FNV-1a，各版本算出来一样）：绿色版、安装版、商店版、测试各锁各的文件
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in p.to_string_lossy().to_lowercase().bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    crate::platform::windows::NamedLock::take(&format!("LiteView-settings-{h:016x}"), std::time::Duration::from_secs(2))
}

#[cfg(not(windows))]
fn save_lock(p: &Path) -> Option<std::fs::File> {
    let f = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(p.with_file_name(".settings.lock")).ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match f.try_lock() {
            // 文件关掉时锁就放了
            Ok(()) => return Some(f),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(_) => return None,
        }
    }
}

/// 把用户的各项逐个放进默认设置（JSON 指针 `at` 处），放进去整个设置还能读才留下；对象往里一层层放（`window` 里一项错了，
/// 别的窗口设置照留）。默认设置里没有的项不管（读的时候本来就忽略）
fn merge_valid(root: &mut serde_json::Value, at: String, user: &serde_json::Value, dropped: &mut Vec<String>) {
    let Some(fields) = user.as_object() else { return };
    for (key, value) in fields {
        let path = format!("{at}/{}", key.replace('~', "~0").replace('/', "~1"));
        let Some(slot) = root.pointer(&path) else { continue };
        if slot.is_object() && value.is_object() {
            merge_valid(root, path, value, dropped);
            continue;
        }
        let mut trial = root.clone();
        if let Some(t) = trial.pointer_mut(&path) {
            *t = value.clone();
        }
        if serde_json::from_value::<Settings>(trial.clone()).is_ok() {
            *root = trial;
        } else {
            dropped.push(path.trim_start_matches('/').replace('/', "."));
        }
    }
}

/// 配置目录（环境变量 `LITEVIEW_CONFIG_DIR` 可覆盖，测试用）
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("LITEVIEW_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    // 单元测试里关设置、退出都会存设置：写到临时目录，不碰用户真正的设置文件
    if cfg!(test) {
        return std::env::temp_dir().join(format!("liteview-test-config-{}", std::process::id()));
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

    #[test]
    fn one_unrecognised_value_only_resets_that_value() {
        // 新版本写的值（旧版本不认识）、写错的类型、越界的数：各自回到默认，别的照用
        let text = r#"{
            "language": "En", "double_click": "SomethingNewer", "jpeg_quality": 700, "wrap_around": false,
            "window": {"pos": [10.0, 20.0], "size": "big", "maximized": true, "borderless": true},
            "print": {"printer": "PDF", "orientation": "Diagonal"}, "future_option": 1
        }"#;
        let (s, mut dropped) = Settings::parse(text).unwrap();
        let d = Settings::default();
        assert_eq!(s.language, Language::En);
        assert!(!s.wrap_around);
        assert_eq!(s.double_click, d.double_click);
        assert_eq!(s.jpeg_quality, d.jpeg_quality);
        assert_eq!(s.window, WindowState { pos: Some([10.0, 20.0]), size: None, maximized: true, borderless: true, always_on_top: false, toolbar_outside: false });
        assert_eq!((s.print.printer.as_str(), s.print.orientation), ("PDF", d.print.orientation));
        dropped.sort();
        assert_eq!(dropped, ["double_click", "jpeg_quality", "print.orientation", "window.size"]);
        // 好的文件不走逐项那条路；坏的 JSON 还是报错（再去读 .bak）
        assert_eq!(Settings::parse(r#"{"wrap_around": false}"#).unwrap().1, Vec::<String>::new());
        assert!(Settings::parse("{ not json").is_err());
    }

    #[test]
    fn old_files_follow_explorer_only_if_the_sort_was_never_changed() {
        let follows = |text: &str| Settings::parse(text).unwrap().0.sort_explorer;
        // 1.9.x 写的文件：默认的文件名升序 → 跟随；改过排序或降序 → 照旧
        assert!(follows(r#"{"sort": "Name", "sort_desc": false}"#));
        assert!(follows("{}"));
        assert!(!follows(r#"{"sort": "Modified", "sort_desc": false}"#));
        assert!(!follows(r#"{"sort": "Name", "sort_desc": true}"#));
        // 逐项读的那条路（别的项有错）也一样
        assert!(!follows(r#"{"sort": "Size", "double_click": "SomethingNewer"}"#));
        assert!(follows(r#"{"sort": "Name", "jpeg_quality": 700}"#));
        // 新版本写的文件：用户在新版本里选了文件名，就是文件名
        assert!(!follows(r#"{"sort": "Name", "sort_desc": false, "sort_explorer": false}"#));
        assert!(follows(r#"{"sort": "Size", "sort_explorer": true}"#));
        assert!(Settings::default().sort_explorer);
    }

    #[test]
    fn order_follows_explorer_only_on_windows() {
        let s = Settings { sort: SortKey::Size, sort_desc: true, sort_explorer: true, ..Settings::default() };
        let o = s.order();
        assert_eq!(o.explorer, cfg!(windows));
        // 跟随时找不到窗口按文件名升序；其它系统照用 sort / sort_desc
        assert_eq!((o.key, o.desc), if cfg!(windows) { (SortKey::Name, false) } else { (SortKey::Size, true) });
        let fixed = Settings { sort_explorer: false, ..s }.order();
        assert_eq!((fixed.key, fixed.desc, fixed.explorer), (SortKey::Size, true, false));
    }

    /// 测试用的设置文件（每个测试一个目录）
    fn temp_file(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("liteview-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    fn read(p: &Path) -> Settings {
        Settings::parse(&std::fs::read_to_string(p).unwrap()).unwrap().0
    }

    #[test]
    fn each_window_only_writes_what_it_changed() {
        let p = temp_file("merge");
        // 还没有文件：整个写
        let start = Settings { single_instance: false, ..Settings::default() };
        start.save_to(&p, &Settings::default());
        assert_eq!(read(&p), start);
        // 两个窗口都从这份设置开始。B 关掉多窗口、换成深色、开了无边框，先关
        let b = Settings { single_instance: true, theme: ThemeMode::Dark, window: WindowState { borderless: true, ..start.window }, ..start.clone() };
        b.save_to(&p, &start);
        // A 只挪了窗口，后关：B 改的都还在，窗口位置和大小用 A 的
        let a = Settings { window: WindowState { pos: Some([40.0, 30.0]), size: Some([900.0, 600.0]), ..start.window }, ..start.clone() };
        a.save_to(&p, &start);
        let saved = read(&p);
        assert!(saved.single_instance);
        assert_eq!(saved.theme, ThemeMode::Dark);
        assert_eq!(saved.window, WindowState { pos: Some([40.0, 30.0]), size: Some([900.0, 600.0]), borderless: true, ..start.window });
        // 两个窗口改了同一项：后存的算；A 上次存过的不再算 A 改的
        let a2 = Settings { theme: ThemeMode::Light, ..a.clone() };
        a2.save_to(&p, &a);
        let saved = read(&p);
        assert_eq!(saved.theme, ThemeMode::Light);
        assert!(saved.single_instance && saved.window.borderless);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn an_unreadable_file_is_written_whole() {
        let p = temp_file("broken");
        std::fs::write(&p, "{ not json").unwrap();
        let mine = Settings { jpeg_quality: 70, theme: ThemeMode::Dark, ..Settings::default() };
        // 和 base 只差主题，但文件读不出来：整个写，JPEG 质量也在
        mine.save_to(&p, &Settings { theme: ThemeMode::Light, ..mine.clone() });
        assert_eq!(read(&p), mine);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
