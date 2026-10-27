//! 窗口与交互。`state` 打开 / 切换图片，`canvas` 画图与鼠标，`input` 键盘，`chrome` 浮动工具栏，
//! `menu` 右键菜单与确认框，`actions` 复制、粘贴、删除、另存为等。

mod actions;
#[cfg(windows)]
mod assoc_ui;
mod canvas;
mod chrome;
pub(crate) mod edit;
mod flip;
#[cfg(windows)]
mod host;
pub(crate) mod info;
mod input;
mod menu;
mod pacing;
#[cfg(windows)]
mod print_ui;
mod settings_ui;
mod state;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use crate::decode::Options;
use crate::settings::Settings;
use crate::ui::theme;
use crate::viewer::folder::Folder;
use crate::viewer::loader::{Loaded, Loader};
use crate::viewer::playback::Playback;
use crate::viewer::texture::{AnimTexture, StillTextures};
use crate::viewer::view::View;

/// 后台任务（复制、粘贴、删除、保存）回报给界面
pub(crate) enum Notice {
    Toast(String),
    OpenPath(PathBuf),
    Pasted(Box<crate::decode::Decoded>),
    Saved(PathBuf),
    /// 另一个实例发来的「打开这个文件」
    Remote(crate::single::Request),
    /// 编辑模式：保存完成
    EditSaved(Box<edit::SaveDone>),
    /// 编辑模式：要粘贴的图（剪贴板 / 拖进来的文件）
    EditPaste(Box<image::RgbaImage>),
}

pub(crate) struct Current {
    pub path: PathBuf,
    /// 完整解码；预读的图在原图解完前是 None，先显示 preview
    pub loaded: Option<Loaded>,
    pub preview: Option<std::sync::Arc<crate::viewer::loader::Preview>>,
    pub since: Instant,
    /// 来自剪贴板（没有源文件，保存时走「另存为」）
    pub clipboard: bool,
}

pub(crate) enum Tex {
    None,
    Still(StillTextures),
    Anim { decoded: std::sync::Arc<crate::decode::Decoded>, tex: AnimTexture, frames: crate::viewer::animation::AnimationFrames },
}

/// 正在显示的图（新图解码完成前继续显示旧图）
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shown {
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
}

/// 弹出确认框后继续做的事
#[derive(Clone, Debug)]
pub(crate) enum After {
    Step(isize),
    Go(usize),
    Open(PathBuf),
    Close,
}

pub(crate) enum Dialog {
    Delete {
        path: PathBuf,
        forever: bool,
        dont_ask: bool,
    },
    SaveRotation {
        after: After,
    },
    /// 首次启动：设为默认看图软件？（Windows 文件关联）
    #[cfg(windows)]
    AskDefault,
    /// 编辑有未保存的修改：保存 / 不保存 / 取消；after = 之后做的事
    ExitEdit {
        after: Option<After>,
    },
    /// 第一次覆盖原图
    Overwrite {
        then: edit::SaveThen,
        dont_ask: bool,
    },
    /// 粘贴的图比画布大
    ExpandCanvas {
        image: std::sync::Arc<image::RgbaImage>,
    },
}

pub struct App {
    pub(crate) settings: Settings,
    pub(crate) loader: Loader,
    pub(crate) folder: Folder,
    pub(crate) cur: Option<Current>,
    pub(crate) view: View,
    pub(crate) tex: Tex,
    pub(crate) shown: Option<Shown>,
    pub(crate) wheel_acc: f32,
    pub(crate) play: Option<Playback>,
    pub(crate) pending_reset: bool,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) tx: Sender<Notice>,
    rx: Receiver<Notice>,
    pub(crate) fling: egui::Vec2,
    pub(crate) canvas: egui::Rect,
    /// 上一次画画布时的缩放比例（物理像素 / 逻辑像素）
    pub(crate) canvas_ppp: f32,
    pub(crate) fullscreen: bool,
    pub(crate) allow_close: bool,
    pub(crate) paste_seq: u32,
    titlebar_dark: Option<bool>,
    title: String,
    pub(crate) first_paint_logged: bool,
    /// 已经运行的界面帧数（第二帧开始时第一帧已经画到窗口上）
    frames: u32,
    script: crate::devtools::Script,
    pub(crate) ctx: egui::Context,
    /// 浮动工具栏上一帧是否显示（它的弹出菜单开着时不收起）
    pub(crate) bar_shown: bool,
    /// 画布的右键菜单开着（浮动工具栏、左右按钮不出来，免得盖住菜单）
    pub(crate) menu_open: bool,
    /// 按住翻页
    pub(crate) flip: flip::Flip,
    /// 窗口中间的翻页提示（已回到第一张、已经是最后一张……）
    pub(crate) notice: Option<(String, Instant)>,
    /// 设置窗口开着时是哪一页
    pub(crate) settings_page: Option<settings_ui::Page>,
    pub(crate) info_open: bool,
    pub(crate) info: Option<info::InfoCache>,
    pub(crate) show_gps: bool,
    pub(crate) rename: Option<info::Rename>,
    /// 监听当前文件夹
    pub(crate) watch: Option<crate::viewer::watch::Watch>,
    pub(crate) watch_dir: Option<PathBuf>,
    /// 刚由本程序保存的文件（文件夹监听不再重复载入）
    pub(crate) self_saved: Vec<(PathBuf, Instant)>,
    /// 编辑模式（None = 查看）
    pub(crate) editor: Option<Box<edit::Editor>>,
    /// 图片载入后进入编辑（`--edit`、`--mosaic`、资源管理器右键）
    pub(crate) pending_edit: Option<edit::Tool>,
    /// 新图片载入：窗口适应图片大小（设置里开了时）
    pub(crate) pending_window_fit: bool,
    /// 缩放比例输入框里的文字
    pub(crate) zoom_text: String,
    /// 省电模式：连续画面的帧率上限
    pacer: pacing::Pacer,
    /// 当前图要了原图（放大、编辑、复制要用），还没放掉
    pub(crate) full_requested: bool,
    /// 回到适应窗口、可以换回缩略图的时刻（停够一会儿再放掉原图）
    pub(crate) settle_since: Option<Instant>,
    /// 只显示着缩略图时点的复制 / 另存为 / 设为背景：原图解完再做
    pub(crate) pending_action: Option<PendingAction>,
    /// 打印对话框（开着时）
    #[cfg(windows)]
    pub(crate) print: Option<Box<print_ui::PrintDialog>>,
    /// 本次运行里「打印机属性」改过的设置（按打印机）
    #[cfg(windows)]
    pub(crate) print_devmodes: std::collections::HashMap<String, crate::platform::print::DevMode>,
    /// 主窗口句柄（驱动对话框的 owner）
    #[cfg(windows)]
    pub(crate) hwnd: Option<isize>,
}

/// 要原图像素的操作（只显示着缩略图时先解码）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingAction {
    Copy,
    SaveAs,
    Wallpaper,
}

pub type RunResult = Result<(), Box<dyn std::error::Error>>;

pub fn run() -> RunResult {
    crate::logger::init();
    let args = crate::cli::parse(std::env::args_os().skip(1));
    let settings = Settings::load();
    crate::i18n::set_language(settings.language);
    crate::startup::install_panic_hook();
    #[cfg(windows)]
    crate::startup::crash::install();
    if args.diagnostics {
        let result = args.path.as_deref().ok_or_else(|| std::io::Error::other("--diagnostics requires an output file"))
            .and_then(crate::devtools::write_diagnostics);
        return result.map_err(Into::into);
    }
    // 给安装程序用：注册 / 删除文件关联后退出
    #[cfg(windows)]
    if args.register || args.unregister {
        if args.unregister {
            crate::platform::assoc::unregister();
        } else if let Err(e) = crate::platform::assoc::register(&settings.assoc_exts) {
            log::warn!("注册文件关联失败：{e}");
            return Err(std::io::Error::other(format!("File association registration failed: {e}")).into());
        }
        return Ok(());
    }
    // 另一个实例正在启动时先等它（画出第一帧后才接收转发；卡住就判失败、换接口）
    let mut guard = crate::startup::Guard::load();
    // 单实例：交给已经开着的窗口
    if settings.single_instance
        && let Some(p) = &args.path
        && crate::single::send(&crate::single::request(p, args.edit, args.mosaic))
    {
        return Ok(());
    }
    // 先开始解码第一张图，与创建窗口、初始化显卡同时进行
    let mut loader = Loader::new(2, decode_options(&settings), settings.cache_mb as usize * 1048576);
    if let Some(p) = &args.path {
        loader.request(p);
    }
    let parts = Some((settings, loader, args));
    #[cfg(windows)]
    return run_windows(parts, &mut guard);
    #[cfg(not(windows))]
    return run_eframe(parts, &mut guard);
}

/// Windows：自己的窗口循环 + Direct3D 11（硬件不行用 WARP）。驱动在建设备时崩溃 / 卡住由 Guard 记下，下次先用 WARP
#[cfg(windows)]
fn run_windows(mut parts: Option<(Settings, Loader, crate::cli::Args)>, guard: &mut crate::startup::Guard) -> RunResult {
    let renderers = match crate::gpu::forced() {
        Some(name) => vec![name],
        None => crate::startup::order(crate::gpu::preferred(), guard.failed()),
    };
    let Some((settings, ..)) = parts.as_ref() else { return Ok(()) };
    let (builder, centered) = (viewport(settings), settings.window.pos.is_none());
    log::info!("启动 {:.0} ms：创建窗口", crate::platform::ms_since_process_start());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host::run(builder, centered, renderers, guard, |ctx| create(&mut parts, ctx))));
    let created = parts.is_none();
    let error = match result {
        Ok(host::Outcome::Closed) => return Ok(()),
        Ok(host::Outcome::NoRenderer(e)) => {
            let e = if e.is_empty() { crate::i18n::tr().gpu_no_adapter.to_owned() } else { e };
            guard.gave_up(&e);
            crate::startup::fatal_dialog(&e);
            return Err(std::io::Error::other(e).into());
        }
        Ok(host::Outcome::Failed(e)) => e,
        Err(payload) => {
            let text = crate::startup::last_panic(payload.as_ref());
            // 界面建好之后的 panic：换接口解决不了，提示后照常退出
            crate::startup::record_exit(&text, !created);
            crate::startup::fatal_dialog(&text);
            std::panic::resume_unwind(payload);
        }
    };
    log::warn!("运行中出错：{error}");
    crate::startup::append_error_log(&error);
    crate::startup::record_exit(&error, !crate::startup::confirmed());
    crate::startup::fatal_dialog(&error);
    Err(std::io::Error::other(error).into())
}

/// 其它系统：eframe + wgpu。窗口还没显示时显卡接口失败（返回错误或 panic）：换下一个接口在本进程重试
#[cfg(not(windows))]
fn run_eframe(mut parts: Option<(Settings, Loader, crate::cli::Args)>, guard: &mut crate::startup::Guard) -> RunResult {
    let names = match crate::gpu::forced() {
        Some(_) => vec!["WGPU_BACKEND"],
        None => crate::startup::order(crate::gpu::preferred(), guard.failed()),
    };
    let mut last_error = None;
    for name in names {
        // 先记下再枚举：隐式层 / 驱动在枚举时就崩溃，下次也能跳过这个接口
        guard.begin(name);
        if crate::gpu::forced().is_none() && !crate::gpu::has_adapter(name) {
            continue;
        }
        let Some((settings, ..)) = parts.as_ref() else { break };
        let options = eframe::NativeOptions {
            viewport: viewport(settings),
            centered: settings.window.pos.is_none(),
            wgpu_options: crate::gpu::configuration(name),
            ..Default::default()
        };
        log::info!("启动 {:.0} ms：创建窗口", crate::platform::ms_since_process_start());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            eframe::run_native("LiteView", options, Box::new(|cc| Ok(Box::new(EframeApp(create(&mut parts, &cc.egui_ctx))) as Box<dyn eframe::App>)))
        }));
        let created = parts.is_none();
        let error = match result {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => e.to_string(),
            Err(payload) => {
                let text = crate::startup::last_panic(payload.as_ref());
                if created {
                    // 界面建好之后的 panic：换接口解决不了，提示后照常退出
                    crate::startup::record_exit(&text, false);
                    crate::startup::fatal_dialog(&text);
                    std::panic::resume_unwind(payload);
                }
                text
            }
        };
        log::warn!("用 {name} 启动失败：{error}");
        crate::startup::append_error_log(&format!("{name}: {error}"));
        if created {
            crate::startup::record_exit(&error, !crate::startup::confirmed());
            crate::startup::fatal_dialog(&error);
            return Err(std::io::Error::other(error).into());
        }
        guard.fail(name, &error);
        last_error = Some(error);
    }
    let error = last_error.unwrap_or_else(|| crate::i18n::tr().gpu_no_adapter.to_owned());
    guard.gave_up(&error);
    crate::startup::fatal_dialog(&error);
    Err(std::io::Error::other(error).into())
}

fn viewport(settings: &Settings) -> egui::ViewportBuilder {
    let w = settings.window;
    let mut vp = egui::ViewportBuilder::default()
        .with_title(crate::i18n::tr().app_name)
        .with_inner_size(w.size.unwrap_or([1100.0, 720.0]))
        .with_min_inner_size([520.0, 360.0])
        .with_drag_and_drop(true)
        .with_maximized(w.maximized)
        .with_icon(app_icon());
    if let Some(pos) = w.pos {
        vp = vp.with_position(pos);
    }
    vp
}

/// 显卡和窗口都准备好之后建界面（每次启动只会成功一次；之前失败的尝试不会走到这里）
fn create(parts: &mut Option<(Settings, Loader, crate::cli::Args)>, ctx: &egui::Context) -> App {
    let (settings, loader, args) = parts.take().expect("app parts are taken once");
    no_ui_zoom_keys(ctx);
    crate::ui::fonts::install(ctx);
    theme::set_accent(settings.accent.or_else(crate::platform::system_accent));
    theme::install(ctx);
    ctx.set_theme(match settings.theme {
        crate::settings::ThemeMode::System => egui::ThemePreference::System,
        crate::settings::ThemeMode::Light => egui::ThemePreference::Light,
        crate::settings::ThemeMode::Dark => egui::ThemePreference::Dark,
    });
    loader.set_context(ctx.clone());
    App::new(settings, loader, args, ctx)
}

/// Ctrl+= / Ctrl+- / Ctrl+0 只缩放图片：设置、对话框、改名框里我们不处理这几个键时，egui 会拿去缩放整个界面
/// （界面大小跟着系统的显示缩放）
fn no_ui_zoom_keys(ctx: &egui::Context) {
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

/// 命令行 / 另一个实例要求打开后进入的工具
fn edit_tool(edit: bool, mosaic: bool) -> Option<edit::Tool> {
    if mosaic {
        Some(edit::Tool::Redact)
    } else if edit {
        Some(edit::Tool::Select)
    } else {
        None
    }
}

pub(crate) fn decode_options(s: &Settings) -> Options {
    Options { anim_cache: s.anim_cache_mb as usize * 1048576, ..Options::default() }
}

fn app_icon() -> egui::IconData {
    let img = crate::ui::icons::rasterize_color(include_str!("../../assets/app.svg"), 64);
    egui::IconData { rgba: img, width: 64, height: 64 }
}

impl App {
    fn new(settings: Settings, loader: Loader, args: crate::cli::Args, ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        let mut app = Self {
            settings,
            loader,
            folder: Folder::empty(),
            cur: None,
            view: View::default(),
            tex: Tex::None,
            shown: None,
            wheel_acc: 0.0,
            play: None,
            pending_reset: false,
            toast: None,
            dialog: None,
            tx,
            rx,
            fling: egui::Vec2::ZERO,
            canvas: egui::Rect::NOTHING,
            canvas_ppp: 0.0,
            fullscreen: false,
            allow_close: false,
            paste_seq: 0,
            titlebar_dark: None,
            title: String::new(),
            first_paint_logged: false,
            frames: 0,
            script: crate::devtools::Script::from_env(),
            ctx: ctx.clone(),
            bar_shown: false,
            menu_open: false,
            flip: Default::default(),
            notice: None,
            settings_page: None,
            info_open: false,
            info: None,
            show_gps: false,
            rename: None,
            watch: None,
            watch_dir: None,
            self_saved: Vec::new(),
            editor: None,
            pending_edit: None,
            pending_window_fit: false,
            zoom_text: String::new(),
            pacer: Default::default(),
            full_requested: false,
            settle_since: None,
            pending_action: None,
            #[cfg(windows)]
            print: None,
            #[cfg(windows)]
            print_devmodes: Default::default(),
            #[cfg(windows)]
            hwnd: None,
        };
        app.pending_edit = edit_tool(args.edit, args.mosaic);
        if let Some(p) = args.path {
            app.open_path(p, ctx);
        }
        // 首次启动询问一次是否设为默认看图软件
        #[cfg(windows)]
        if app.settings.ask_default_app && !crate::platform::assoc::is_registered() {
            app.dialog = Some(Dialog::AskDefault);
        }
        app
    }

    /// 第二帧开始时第一帧已经画到窗口上：确认这次启动成功，再接收其它实例转来的图片。
    /// 第一帧卡在驱动里时不开管道，之后双击图片会另开一个进程（按启动记录换接口），不会被卡住的进程吞掉。
    fn track_startup(&mut self, ctx: &egui::Context) {
        self.frames = self.frames.saturating_add(1);
        match self.frames {
            1 => ctx.request_repaint(),
            2 => {
                crate::startup::confirm();
                if self.settings.single_instance {
                    let (tx, ctx2) = (self.tx.clone(), ctx.clone());
                    crate::single::serve(move |r| {
                        let _ = tx.send(Notice::Remote(r));
                        ctx2.request_repaint();
                    });
                }
            }
            _ => {}
        }
    }

    fn poll_notices(&mut self, ctx: &egui::Context) {
        while let Ok(n) = self.rx.try_recv() {
            match n {
                Notice::Toast(t) => self.show_toast(t),
                Notice::OpenPath(p) => self.open_path(p, ctx),
                Notice::Pasted(d) => self.show_pasted(*d),
                Notice::Remote(r) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.pending_edit = edit_tool(r.edit, r.mosaic);
                    if self.editor.is_some() {
                        // 编辑中：先离开编辑（有修改时询问），再打开
                        self.with_editor(|app, ed| app.edit_request_exit(ed, Some(After::Open(r.path))));
                    } else {
                        self.open_path(r.path, ctx);
                    }
                }
                Notice::EditSaved(done) => self.with_editor(|app, ed| app.edit_saved(ed, *done)),
                Notice::EditPaste(img) => self.with_editor(|app, ed| app.edit_paste_image(ed, *img)),
                Notice::Saved(p) => {
                    self.loader.forget(&p);
                    self.self_saved.push((p.clone(), Instant::now()));
                    self.show_toast(crate::i18n::tr().saved.to_owned());
                    // 保存的是正在看的图：重新载入（旋转已写进文件，载入后显示方向复位）
                    if self.cur.as_ref().is_some_and(|c| c.path == p) {
                        self.reload_current();
                    }
                }
            }
        }
    }

    pub(crate) fn show_toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// 窗口中间、比轻提示醒目的翻页提示
    pub(crate) fn show_notice(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// `--edit` / `--mosaic`：图片载入后进入编辑；打不开就算了
    fn start_pending_edit(&mut self) {
        let Some(tool) = self.pending_edit else { return };
        if self.editor.is_some() || self.dialog.is_some() {
            return;
        }
        match self.cur.as_ref().and_then(|c| c.loaded.as_ref()) {
            Some(Ok(_)) => {
                self.pending_edit = None;
                self.enter_edit(tool);
            }
            Some(Err(_)) => self.pending_edit = None,
            None if self.cur.is_none() => self.pending_edit = None,
            // 只显示着缩略图：要原图
            None => self.want_full(),
        }
    }

    /// 复制、另存为、设为桌面背景等原图解完再做（只显示着缩略图时点的）
    fn start_pending_action(&mut self) {
        let Some(action) = self.pending_action else { return };
        if self.editor.is_some() || self.dialog.is_some() {
            return;
        }
        match self.cur.as_ref().and_then(|c| c.loaded.as_ref()) {
            Some(Ok(_)) => {
                self.pending_action = None;
                match action {
                    PendingAction::Copy => self.copy_image(),
                    PendingAction::SaveAs => self.save_as(),
                    PendingAction::Wallpaper => self.set_wallpaper(),
                }
            }
            Some(Err(_)) => self.pending_action = None,
            None if self.cur.is_none() => self.pending_action = None,
            None => self.want_full(),
        }
    }

    fn update_window(&mut self, ctx: &egui::Context, hwnd: Option<isize>) {
        let t = crate::i18n::tr();
        // 编辑中有未保存的修改时文件名前加 *
        let star = if self.editor.as_ref().is_some_and(|e| e.modified()) { "*" } else { "" };
        let title = match &self.cur {
            Some(c) if c.clipboard => format!("{star}{} - {}", t.clipboard_image, t.app_name),
            Some(c) => format!("{star}{} - {}", crate::natsort::file_name(&c.path), t.app_name),
            None => t.app_name.to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        let dark = ctx.theme() == egui::Theme::Dark;
        if self.titlebar_dark != Some(dark) {
            self.titlebar_dark = Some(dark);
            style_titlebar(hwnd, theme::palette(ctx));
        }
        let scale = ctx.input(|i| i.viewport().native_pixels_per_point).unwrap_or(1.0);
        window_icon(hwnd, scale);
        // 「窗口适应图片大小」：按图片的原始大小（不超过屏幕的九成）改窗口大小
        if std::mem::take(&mut self.pending_window_fit)
            && let Some(s) = self.shown
        {
            let (maximized, full, monitor) =
                ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false), i.viewport().monitor_size));
            if !maximized && !full {
                let size = self.view.shown_size(s.width, s.height) / ctx.pixels_per_point();
                let max = monitor.map_or(egui::vec2(1600.0, 1000.0), |m| m * 0.9);
                let want = size.min(max).max(egui::vec2(520.0, 360.0));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
            }
        }
        ctx.input(|i| {
            let vp = i.viewport();
            self.fullscreen = vp.fullscreen.unwrap_or(false);
            let maximized = vp.maximized.unwrap_or(false);
            self.settings.window.maximized = maximized;
            if !maximized && !self.fullscreen {
                if let Some(r) = vp.outer_rect {
                    self.settings.window.pos = Some([r.min.x, r.min.y]);
                }
                if let Some(r) = vp.inner_rect {
                    self.settings.window.size = Some([r.width(), r.height()]);
                }
            }
        });
    }
}

/// 窗口所在显示器的刷新率（只有 Windows 取得到）
#[allow(unused_variables)]
fn refresh_hz(hwnd: Option<isize>) -> Option<f32> {
    #[cfg(windows)]
    if let Some(h) = hwnd {
        return crate::platform::windows::refresh_hz(h);
    }
    None
}

/// Windows 标题栏 / 任务栏图标按显示器缩放画准确尺寸（其它系统用启动时设的 64 px 图标）
#[allow(unused_variables)]
fn window_icon(hwnd: Option<isize>, scale: f32) {
    #[cfg(windows)]
    if let Some(h) = hwnd {
        crate::platform::windows::ensure_window_icon(h, scale, |px| crate::ui::icons::rasterize_color(include_str!("../../assets/app.svg"), px));
    }
}

#[allow(unused_variables)]
fn style_titlebar(hwnd: Option<isize>, p: &theme::Palette) {
    #[cfg(windows)]
    if let Some(h) = hwnd {
        let c = p.canvas.to_array();
        let t = p.text.to_array();
        crate::platform::windows::style_titlebar(h, p.dark, [c[0], c[1], c[2]], [t[0], t[1], t[2]]);
    }
}

/// eframe 的外壳（Windows 以外）
#[cfg(not(windows))]
struct EframeApp(App);

#[cfg(not(windows))]
impl eframe::App for EframeApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.inject_input(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.frame(ui, None);
    }

    fn on_exit(&mut self) {
        self.0.exit();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.0.background()
    }
}

impl App {
    /// 每帧交给 egui 之前（开发脚本注入输入）
    pub(crate) fn inject_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.script.inject(ctx, raw);
    }

    /// 一帧界面。hwnd：Windows 的窗口句柄（标题栏颜色、图标、刷新率用）
    pub(crate) fn frame(&mut self, ui: &mut egui::Ui, hwnd: Option<isize>) {
        let ctx = ui.ctx().clone();
        #[cfg(windows)]
        {
            self.hwnd = hwnd;
        }
        self.track_startup(&ctx);
        if self.editor.is_none() {
            self.update_target(ui);
        }
        self.poll_notices(&ctx);
        self.poll_loader(&ctx);
        self.poll_watch(&ctx);
        self.update_window(&ctx, hwnd);
        self.start_pending_edit();
        self.start_pending_action();
        if self.editor.is_some() {
            if let Some(ed) = &mut self.editor {
                ed.popup_was_open = ctx.any_popup_open();
            }
            self.edit_frame(ui);
        } else {
            self.canvas_ui(ui);
            self.keyboard(&ctx);
            self.chrome(&ctx);
            self.info_panel(&ctx);
            self.flip_tick(&ctx);
        }
        self.toast_ui(&ctx);
        self.notice_ui(&ctx);
        self.dialogs(&ctx);
        self.settings_ui(&ctx);
        self.rename_ui(&ctx);
        #[cfg(windows)]
        self.print_ui(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.editor.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.dialog.is_none() {
                    self.with_editor(|app, ed| app.edit_request_exit(ed, Some(After::Close)));
                }
            } else if self.needs_rotation_prompt() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialog = Some(Dialog::SaveRotation { after: After::Close });
            }
        }
        let load_state = match self.cur.as_ref().map(|c| (c.loaded.as_ref(), c.preview.is_some())) {
            Some((Some(Ok(_)), _) | (None, true)) => crate::devtools::LoadState::Shown,
            Some((Some(Err(_)), _)) => crate::devtools::LoadState::Error,
            _ => crate::devtools::LoadState::Pending,
        };
        self.script.after_frame(&ctx, self.first_paint_logged, load_state);
        self.pacer.pace(self.settings.frame_cap, || refresh_hz(hwnd));
    }

    /// 窗口关闭：存设置
    pub(crate) fn exit(&mut self) {
        log::debug!("退出：共画了 {} 帧", self.ctx.cumulative_frame_nr());
        self.settings.save();
    }

    /// 清屏颜色（gamma 空间）
    pub(crate) fn background(&self) -> [f32; 4] {
        let p = if self.titlebar_dark == Some(true) { &theme::DARK } else { &theme::LIGHT };
        p.canvas.to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    /// 按下 key（带 Ctrl / Cmd）后再画一帧，返回 egui 的整体界面缩放
    fn zoom_after(ctx: &egui::Context, key: egui::Key) -> f32 {
        let press = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
        let _ = ctx.run_ui(egui::RawInput { events: vec![press], ..Default::default() }, |_| {});
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        ctx.zoom_factor()
    }

    #[test]
    fn ctrl_plus_minus_zero_do_not_zoom_the_whole_ui() {
        // 不关的话 egui 会拿这几个键缩放整个界面（设置、对话框有焦点时我们不处理它们）
        let plain = egui::Context::default();
        assert!(zoom_after(&plain, egui::Key::Equals) > 1.0);
        let ctx = egui::Context::default();
        super::no_ui_zoom_keys(&ctx);
        for key in [egui::Key::Equals, egui::Key::Plus, egui::Key::Minus, egui::Key::Num0] {
            assert_eq!(zoom_after(&ctx, key), 1.0, "{key:?}");
        }
    }
}
