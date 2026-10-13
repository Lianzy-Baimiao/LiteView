//! 窗口与交互。`state` 打开 / 切换图片，`canvas` 画图与鼠标，`input` 键盘，`chrome` 浮动工具栏，
//! `menu` 右键菜单与确认框，`actions` 复制、粘贴、删除、另存为等。

mod actions;
mod canvas;
mod chrome;
mod input;
mod menu;
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
pub enum Notice {
    Toast(String),
    OpenPath(PathBuf),
    Pasted(Box<crate::decode::Decoded>),
    Removed(PathBuf),
    Saved(PathBuf),
}

pub(crate) struct Current {
    pub path: PathBuf,
    pub loaded: Option<Loaded>,
    pub since: Instant,
    /// 来自剪贴板（没有源文件，保存时走「另存为」）
    pub clipboard: bool,
}

pub(crate) enum Tex {
    None,
    Still(StillTextures),
    Anim { decoded: std::sync::Arc<crate::decode::Decoded>, tex: AnimTexture },
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
    Delete { path: PathBuf, forever: bool, dont_ask: bool },
    SaveRotation { after: After },
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
    pub(crate) last_move: Instant,
    pub(crate) fling: egui::Vec2,
    pub(crate) canvas: egui::Rect,
    pub(crate) fullscreen: bool,
    pub(crate) allow_close: bool,
    pub(crate) paste_seq: u32,
    titlebar_dark: Option<bool>,
    title: String,
    pub(crate) first_paint_logged: bool,
    script: crate::devtools::Script,
    pub(crate) ctx: egui::Context,
    /// 浮动工具栏的位置（鼠标在上面时不淡出）
    pub(crate) bar_rect: egui::Rect,
}

pub fn run() -> eframe::Result {
    crate::logger::init();
    let args = crate::cli::parse(std::env::args_os().skip(1));
    let settings = Settings::load();
    crate::i18n::set_language(settings.language);
    // 先开始解码第一张图，与创建窗口、初始化显卡同时进行
    let mut loader = Loader::new(2, decode_options(&settings), settings.cache_mb as usize * 1048576);
    if let Some(p) = &args.path {
        loader.request(p);
    }
    let w = settings.window;
    let mut vp = egui::ViewportBuilder::default()
        .with_title(crate::i18n::tr().app_name)
        .with_inner_size(w.size.unwrap_or([1100.0, 720.0]))
        .with_min_inner_size([360.0, 260.0])
        .with_drag_and_drop(true)
        .with_maximized(w.maximized)
        .with_icon(app_icon());
    if let Some(pos) = w.pos {
        vp = vp.with_position(pos);
    }
    let options = eframe::NativeOptions {
        viewport: vp,
        centered: w.pos.is_none(),
        wgpu_options: crate::gpu::configuration(),
        ..Default::default()
    };
    log::info!("启动 {:.0} ms：创建窗口", crate::platform::ms_since_process_start());
    eframe::run_native(
        "LiteView",
        options,
        Box::new(move |cc| {
            crate::ui::fonts::install(&cc.egui_ctx);
            theme::install(&cc.egui_ctx);
            cc.egui_ctx.set_theme(match settings.theme {
                crate::settings::ThemeMode::System => egui::ThemePreference::System,
                crate::settings::ThemeMode::Light => egui::ThemePreference::Light,
                crate::settings::ThemeMode::Dark => egui::ThemePreference::Dark,
            });
            loader.set_context(cc.egui_ctx.clone());
            Ok(Box::new(App::new(settings, loader, args, &cc.egui_ctx)))
        }),
    )
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
            last_move: Instant::now(),
            fling: egui::Vec2::ZERO,
            canvas: egui::Rect::NOTHING,
            fullscreen: false,
            allow_close: false,
            paste_seq: 0,
            titlebar_dark: None,
            title: String::new(),
            first_paint_logged: false,
            script: crate::devtools::Script::from_env(),
            ctx: ctx.clone(),
            bar_rect: egui::Rect::NOTHING,
        };
        if let Some(p) = args.path {
            app.open_path(p, ctx);
        }
        app
    }

    fn poll_notices(&mut self, ctx: &egui::Context) {
        while let Ok(n) = self.rx.try_recv() {
            match n {
                Notice::Toast(t) => self.show_toast(t),
                Notice::OpenPath(p) => self.open_path(p, ctx),
                Notice::Pasted(d) => self.show_pasted(*d),
                Notice::Removed(p) => self.loader.forget(&p),
                Notice::Saved(p) => {
                    self.loader.forget(&p);
                    self.show_toast(crate::i18n::tr().saved.to_owned());
                    // 保存的是正在看的图：重新载入（旋转已写进文件，载入后显示方向复位）
                    if let Some(c) = &mut self.cur
                        && c.path == p
                    {
                        c.loaded = None;
                        c.since = Instant::now();
                        if let Some(l) = self.loader.request(&p) {
                            self.on_loaded(l);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn show_toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    fn update_window(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let t = crate::i18n::tr();
        let title = match &self.cur {
            Some(c) if c.clipboard => format!("{} - {}", t.clipboard_image, t.app_name),
            Some(c) => format!("{} - {}", crate::natsort::file_name(&c.path), t.app_name),
            None => t.app_name.to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        let dark = ctx.theme() == egui::Theme::Dark;
        if self.titlebar_dark != Some(dark) {
            self.titlebar_dark = Some(dark);
            style_titlebar(frame, theme::palette(ctx));
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

#[allow(unused_variables)]
fn style_titlebar(frame: &eframe::Frame, p: &theme::Palette) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = frame.window_handle()
            && let RawWindowHandle::Win32(w) = h.as_raw()
        {
            let c = p.canvas.to_array();
            let t = p.text.to_array();
            crate::platform::windows::style_titlebar(w.hwnd.get(), p.dark, [c[0], c[1], c[2]], [t[0], t[1], t[2]]);
        }
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.script.inject(ctx, raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_notices(&ctx);
        self.poll_loader(&ctx);
        self.update_window(&ctx, frame);
        self.canvas_ui(ui);
        self.keyboard(&ctx);
        self.chrome(&ctx);
        self.dialogs(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.needs_rotation_prompt() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::SaveRotation { after: After::Close });
        }
        self.script.after_frame(&ctx, self.first_paint_logged);
    }

    fn on_exit(&mut self) {
        self.settings.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let p = if self.titlebar_dark == Some(true) { &theme::DARK } else { &theme::LIGHT };
        p.canvas.to_normalized_gamma_f32()
    }
}
