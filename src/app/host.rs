//! Windows 的窗口循环：winit + egui-winit + 自己的 Direct3D 11 画法（`platform::d3d11`），不用 eframe / wgpu
//! （wgpu 的 Vulkan / DX12 光初始化就要约 100 MB）。照 eframe 0.36.2 单窗口的做法：画出第一帧才显示窗口、
//! 按 egui 的请求安排重绘、处理 ViewportCommand、读屏（AccessKit）、截图、改变大小时同步重画。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{ViewportBuilder, ViewportId, ViewportInfo};
use egui_winit::winit;
use egui_winit::{ActionRequested, accesskit_winit};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use super::App;
use crate::platform::d3d11::{Driver, Painter};

/// 最小化时连续重绘的间隔（看不见，只让逻辑往前走）
const HIDDEN_REPAINT: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub enum UserEvent {
    RequestRepaint { when: Instant, pass: u64 },
    AccessKit(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        Self::AccessKit(e)
    }
}

pub enum Outcome {
    /// 正常关闭
    Closed,
    /// 窗口还没显示，所有画法都失败了（已记进 Guard）
    NoRenderer(String),
    /// 界面建好之后出错（显卡设备丢失等）
    Failed(String),
}

struct Running {
    window: Arc<Window>,
    painter: Painter,
    state: egui_winit::State,
    ctx: egui::Context,
    app: App,
    info: ViewportInfo,
    actions: Vec<ActionRequested>,
    pending: egui::TexturesDelta,
    screenshot: Option<egui::Event>,
    shown: bool,
    beginning: Instant,
    /// 用的哪种画法（浮动工具栏的小窗口用同一种）
    driver: Driver,
    dock: Option<Dock>,
    /// 小窗口建不起来：不再试
    dock_failed: bool,
    /// 上一帧透出桌面（DWM 的圆角、细边框、阴影已经关了）
    see_through: bool,
    /// 单独的设置窗口（开着设置时）
    settings_win: Option<SettingsWin>,
    /// 窗口为菜单、对话框临时变大 / 变回去：上一帧不拉伸、按比例放好了，等这一轮消息处理完再改窗口（画刷的改动先生效）
    held: Held,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Held {
    None,
    /// 画面已经按比例放好，还没改窗口
    Pending((i32, i32, i32, i32)),
    /// 窗口改了：按新大小画出一帧后回到拉伸铺满
    Applied,
}

/// 浮动工具栏的小窗口（无边框、窗口比工具栏还窄时放在图片下面）：没有标题栏，不进任务栏，点它不抢焦点，
/// 跟着图片窗口最小化。自己的 Direct3D 交换链和 egui（字体是同一份映射的文件），第一次要用时才建
struct Dock {
    window: Window,
    painter: Painter,
    state: egui_winit::State,
    ctx: egui::Context,
    hwnd: isize,
    /// 工具栏的大小、连同提示 / 弹出菜单的大小（物理像素），上次放的位置
    bar: (i32, i32),
    size: (i32, i32),
    rect: Option<(i32, i32, i32, i32)>,
    dirty: bool,
    next: Option<Instant>,
    /// 还没交给显卡的纹理变化（egui 第一帧量大小时什么都不画，字体纹理也在那一帧，不能丢）
    pending: egui::TexturesDelta,
    /// 上一帧里有输入框有焦点（比例弹出菜单）。点输入框时系统一般会把这个小窗口放到最前面，按键直接到它这里；
    /// 图片窗口还在最前面时，图片窗口收到的按键转给它
    typing: bool,
    /// 点输入框时系统还是把它放到了最前面；从什么时候起它在最前面、却没有输入框有焦点（打完了）
    stray_since: Option<Instant>,
}

/// 小窗口里排版用的大小（点）：工具栏和它的弹出菜单都放得下；实际窗口只有画出来的那么大
const DOCK_LAYOUT: egui::Vec2 = egui::vec2(900.0, 420.0);
/// 小窗口离图片窗口的距离（物理像素）
const DOCK_GAP: i32 = 6;
/// 小窗口在最前面、却没有在打字这么久，就把最前面还给图片窗口
const DOCK_REFOCUS: std::time::Duration = std::time::Duration::from_millis(150);

impl Dock {
    fn new(event_loop: &ActiveEventLoop, owner: &Window, driver: Driver, main: &egui::Context) -> Result<Self, String> {
        use winit::platform::windows::WindowAttributesExtWindows;
        let owner_hwnd = hwnd(owner).ok_or("no HWND")?;
        let pos = owner.outer_position().unwrap_or_default();
        let attrs = Window::default_attributes()
            .with_title("")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_position(pos)
            .with_inner_size(winit::dpi::PhysicalSize::new(400u32, 60u32))
            .with_skip_taskbar(true)
            .with_undecorated_shadow(false)
            .with_owner_window(owner_hwnd.0 as isize);
        let window = event_loop.create_window(attrs).map_err(|e| e.to_string())?;
        let h = hwnd(&window).ok_or("no HWND")?;
        crate::platform::windows::style_dock(h.0 as isize);
        let painter = Painter::new(h, driver, [400, 60]).map_err(|e| e.to_string())?;
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        crate::ui::fonts::install(&ctx);
        crate::ui::theme::install(&ctx);
        ctx.set_theme(main.theme());
        let state = egui_winit::State::new(ctx.clone(), ViewportId::ROOT, event_loop, Some(window.scale_factor() as f32), event_loop.system_theme(), Some(painter.max_texture_side));
        Ok(Self { window, painter, state, ctx, hwnd: h.0 as isize, bar: (0, 0), size: (0, 0), rect: None, dirty: true, next: None, pending: Default::default(), typing: false, stray_since: None })
    }

    /// 画一帧；返回这一帧有没有鼠标 / 键盘输入（有的话图片窗口也要跟着重画）
    fn paint(&mut self, app: &mut App) -> Result<bool, String> {
        let mut raw = self.state.take_egui_input(&self.window);
        let input = !raw.events.is_empty();
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, DOCK_LAYOUT));
        self.ctx.set_theme(app.ctx.theme());
        let ctx = self.ctx.clone();
        let mut bar = egui::Rect::NOTHING;
        let out = self.ctx.run_ui(raw, |_| bar = app.dock_ui(&ctx));
        // 有输入框有焦点时 egui 给出输入法的位置
        self.typing = out.platform_output.ime.is_some();
        self.state.handle_platform_output(&self.window, out.platform_output);
        self.pending.append(out.textures_delta);
        let used = out.shapes.iter().map(|s| s.shape.visual_bounding_rect().intersect(s.clip_rect)).filter(|r| r.is_positive()).fold(egui::Rect::NOTHING, |a, b| a.union(b));
        let ppp = out.pixels_per_point;
        self.bar = if bar.is_positive() { ((bar.max.x * ppp).ceil() as i32, (bar.max.y * ppp).ceil() as i32) } else { (0, 0) };
        self.size = if used.is_positive() { ((used.max.x * ppp).ceil() as i32, (used.max.y * ppp).ceil() as i32) } else { (0, 0) };
        self.next = out.viewport_output.get(&ViewportId::ROOT).and_then(|v| Instant::now().checked_add(v.repaint_delay));
        if self.size.0 > 0 && self.size.1 > 0 {
            self.painter.resize([self.size.0 as u32, self.size.1 as u32]).map_err(|e| e.0)?;
            let prims = self.ctx.tessellate(out.shapes, ppp);
            let p = crate::ui::theme::palette(&self.ctx);
            let delta = std::mem::take(&mut self.pending);
            self.painter.paint(p.floating.to_normalized_gamma_f32(), ppp, &prims, &delta, false).map_err(|e| e.0)?;
        }
        self.dirty = false;
        Ok(input)
    }
}

/// 单独的设置窗口：有标题栏、能拖开、改大小，跟着图片窗口（owner：不进任务栏，在图片窗口上面，一起最小化）。
/// 自己的 Direct3D 交换链和 egui（字体是同一份映射的文件），打开设置时建，关掉就放掉（约 20 MB 内存、20 MB 显存）
struct SettingsWin {
    window: Window,
    painter: Painter,
    state: egui_winit::State,
    ctx: egui::Context,
    hwnd: isize,
    dirty: bool,
    next: Option<Instant>,
    pending: egui::TexturesDelta,
    shown: bool,
    title: String,
    /// 上次跟着图片窗口换主题时的深浅、强调色
    look: Option<(bool, Option<[u8; 3]>)>,
}

/// 设置窗口的大小（点）
const SETTINGS_SIZE: egui::Vec2 = egui::vec2(720.0, 500.0);
const SETTINGS_MIN: egui::Vec2 = egui::vec2(520.0, 360.0);

impl SettingsWin {
    fn new(event_loop: &ActiveEventLoop, owner: &Window, driver: Driver, proxy: &EventLoopProxy<UserEvent>) -> Result<Self, String> {
        use winit::platform::windows::WindowAttributesExtWindows;
        let owner_hwnd = hwnd(owner).ok_or("no HWND")?;
        // 放在图片窗口中间，整个在它那块屏的工作区里
        let scale = owner.scale_factor() as f32;
        let (w, h) = ((SETTINGS_SIZE.x * scale).round() as i32, (SETTINGS_SIZE.y * scale).round() as i32);
        let o = owner.outer_position().unwrap_or_default();
        let os = owner.outer_size();
        let (mut x, mut y) = (o.x + os.width as i32 / 2 - w / 2, o.y + os.height as i32 / 2 - h / 2);
        if let Some(work) = crate::platform::windows::work_area(owner_hwnd.0 as isize) {
            x = x.clamp(work.0, (work.2 - w).max(work.0));
            y = y.clamp(work.1, (work.3 - h).max(work.1));
        }
        let title = settings_title();
        let attrs = Window::default_attributes()
            .with_title(title.clone())
            .with_visible(false)
            .with_position(winit::dpi::PhysicalPosition::new(x, y))
            .with_inner_size(winit::dpi::LogicalSize::new(SETTINGS_SIZE.x, SETTINGS_SIZE.y))
            .with_min_inner_size(winit::dpi::LogicalSize::new(SETTINGS_MIN.x, SETTINGS_MIN.y))
            .with_owner_window(owner_hwnd.0 as isize);
        let window = event_loop.create_window(attrs).map_err(|e| e.to_string())?;
        let h = hwnd(&window).ok_or("no HWND")?;
        let size = window.inner_size();
        let painter = Painter::new(h, driver, [size.width, size.height]).map_err(|e| e.to_string())?;
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        crate::ui::fonts::install(&ctx);
        crate::ui::theme::install(&ctx);
        let mut state = egui_winit::State::new(ctx.clone(), ViewportId::ROOT, event_loop, Some(window.scale_factor() as f32), event_loop.system_theme(), Some(painter.max_texture_side));
        state.init_accesskit(event_loop, &window, proxy.clone());
        Ok(Self { window, painter, state, ctx, hwnd: h.0 as isize, dirty: true, next: None, pending: Default::default(), shown: false, title, look: None })
    }

    /// 画一帧；返回 true = 要关掉（按了 Esc）
    fn paint(&mut self, app: &mut App) -> Result<bool, String> {
        // 跟着图片窗口的深浅色和强调色
        let main = app.ctx.clone();
        let look = (main.theme() == egui::Theme::Dark, app.settings.accent);
        if self.look != Some(look) {
            if self.look.is_some_and(|l| l.1 != look.1) {
                crate::ui::theme::install(&self.ctx);
            }
            self.ctx.set_theme(main.theme());
            super::style_titlebar(Some(self.hwnd), crate::ui::theme::palette(&main));
            self.look = Some(look);
        }
        let title = settings_title();
        if title != self.title {
            self.window.set_title(&title);
            self.title = title;
        }
        super::window_icon(Some(self.hwnd), self.window.scale_factor() as f32);
        let raw = self.state.take_egui_input(&self.window);
        let ctx = self.ctx.clone();
        let mut close = false;
        let out = self.ctx.run_ui(raw, |_| close = app.settings_window_ui(&ctx));
        self.state.handle_platform_output(&self.window, out.platform_output);
        self.pending.append(out.textures_delta);
        self.next = out.viewport_output.get(&ViewportId::ROOT).and_then(|v| Instant::now().checked_add(v.repaint_delay));
        let size = self.window.inner_size();
        if size.width > 0 && size.height > 0 {
            self.painter.resize([size.width, size.height]).map_err(|e| e.0)?;
            let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
            let delta = std::mem::take(&mut self.pending);
            let clear = self.ctx.global_style().visuals.window_fill.to_normalized_gamma_f32();
            let shot = app.take_settings_shot();
            let image = self.painter.paint(clear, out.pixels_per_point, &prims, &delta, shot.is_some()).map_err(|e| e.0)?;
            if let (Some(path), Some(img)) = (shot, image) {
                crate::devtools::save_shot(&path, &img);
            }
            if !self.shown {
                // 画好第一帧才显示：用户刚在图片窗口里按的，系统让它到最前面
                self.shown = true;
                self.window.set_visible(true);
                crate::platform::windows::bring_to_front(self.hwnd);
                log::debug!("设置窗口：{:?}，排版 {:?} 点", crate::platform::windows::window_rect(self.hwnd), self.ctx.content_rect().size());
            }
        }
        self.dirty = false;
        Ok(close)
    }
}

/// 「LiteView 设置」（不以 LiteView 结尾：检查脚本按这个找图片窗口）
fn settings_title() -> String {
    let t = crate::i18n::tr();
    format!("{} {}", t.app_name, t.settings)
}

struct Host<'a, F: FnOnce(&egui::Context) -> App> {
    builder: ViewportBuilder,
    centered: bool,
    renderers: Vec<&'static str>,
    guard: &'a mut crate::startup::Guard,
    create: Option<F>,
    proxy: EventLoopProxy<UserEvent>,
    running: Option<Running>,
    next_repaint: Option<Instant>,
    close: bool,
    outcome: Option<Outcome>,
}

/// 建窗口、按顺序试 renderers（"d3d11" / "warp"），然后跑到窗口关闭
pub fn run(builder: ViewportBuilder, centered: bool, renderers: Vec<&'static str>, guard: &mut crate::startup::Guard, create: impl FnOnce(&egui::Context) -> App) -> Outcome {
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => return Outcome::NoRenderer(format!("event loop: {e}")),
    };
    let proxy = event_loop.create_proxy();
    let mut host = Host { builder, centered, renderers, guard, create: Some(create), proxy, running: None, next_repaint: None, close: false, outcome: None };
    if let Err(e) = event_loop.run_app(&mut host) {
        return host.outcome.unwrap_or(Outcome::Failed(format!("event loop: {e}")));
    }
    host.outcome.unwrap_or(Outcome::Closed)
}

fn hwnd(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(w) => Some(windows::Win32::Foundation::HWND(w.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// 与 `egui_winit::create_window` 相同；no_gdi_layer = 窗口不要 GDI 那层（WS_EX_NOREDIRECTIONBITMAP，能透明的窗口用）
fn create_window(ctx: &egui::Context, event_loop: &ActiveEventLoop, builder: &ViewportBuilder, no_gdi_layer: bool) -> Result<Window, winit::error::OsError> {
    use winit::platform::windows::WindowAttributesExtWindows;
    let attrs = egui_winit::create_winit_window_attributes(ctx, builder.clone()).with_no_redirection_bitmap(no_gdi_layer);
    let window = event_loop.create_window(egui_winit::apply_monitor_to_window_attributes(attrs, builder, event_loop))?;
    egui_winit::apply_viewport_builder_to_window(ctx, &window, builder);
    Ok(window)
}

/// 能透明的窗口：无边框时的阴影按 egui-winit 处理 Decorations 的规则（无边框、不全屏时有），透出桌面时不要
/// （图片不是方的，方阴影不对）。没变时 winit 什么都不做
fn sync_shadow(r: &Running) {
    if r.painter.composited() {
        use winit::platform::windows::WindowExtWindows;
        r.window.set_undecorated_shadow(!r.window.is_decorated() && r.window.fullscreen().is_none() && !r.see_through);
    }
}

/// 所有显示器里最大的（逻辑点）：初始窗口不超过它（与 eframe 相同）
fn largest_monitor_points(event_loop: &ActiveEventLoop) -> egui::Vec2 {
    let max = event_loop.available_monitors().fold(egui::Vec2::ZERO, |m, mon| {
        let s = mon.size().to_logical::<f32>(mon.scale_factor());
        m.max(egui::vec2(s.width, s.height))
    });
    if max == egui::Vec2::ZERO { egui::Vec2::splat(16000.0) } else { max }
}

impl<F: FnOnce(&egui::Context) -> App> Host<'_, F> {
    /// 建窗口（先不显示）和画法、egui 状态，再建界面。返回失败原因时界面没有建
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        ctx.options_mut(|o| o.max_passes = 2.try_into().expect("nonzero"));
        let mut builder = self.builder.clone().with_visible(false);
        if let Some(size) = builder.inner_size {
            builder = builder.with_inner_size(size.min(largest_monitor_points(event_loop)));
        }
        if self.centered
            && let Some(mon) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next())
        {
            let m = mon.size().to_logical::<f32>(mon.scale_factor());
            let inner = builder.inner_size.unwrap_or(egui::vec2(800.0, 600.0));
            if m.width > 0.0 && m.height > 0.0 {
                builder = builder.with_position([(m.width - inner.x) / 2.0, (m.height - inner.y) / 2.0]);
            }
        }
        // 无边框启动：画法挂到系统合成（Windows.UI.Composition）上，看有透明像素的图时能透出桌面。窗口建成没有 GDI 那层的
        // （有的话显示过一次系统标题栏，也就是进过编辑，那层就不透明了），只能在建窗口时决定；普通窗口照旧
        let borderless = builder.decorations == Some(false);
        let composited = borderless && crate::platform::d3d11::composition_available();
        let window = Arc::new(create_window(&ctx, event_loop, &builder, composited).map_err(|e| format!("create window: {e}"))?);
        let hwnd = hwnd(&window).ok_or("no HWND")?;
        crate::platform::windows::use_system_dpi_rect(hwnd.0 as isize);
        crate::platform::windows::keep_resize_aspect(hwnd.0 as isize);
        let mut last = String::new();
        let mut painter = None;
        for name in self.renderers.clone() {
            // 先记下再建设备：驱动在建设备 / 交换链时崩溃，下次启动也能跳过它
            self.guard.begin(name);
            let driver = if name == "warp" { Driver::Warp } else { Driver::Hardware };
            let size = window.inner_size();
            let make: fn(_, _, _) -> _ = if composited { Painter::new_composited } else { Painter::new };
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| make(hwnd, driver, [size.width, size.height])));
            let err = match made {
                Ok(Ok(p)) => {
                    log::info!("显卡接口：{}（{}）", crate::gpu::display_name(name), p.adapter);
                    painter = Some((p, driver));
                    break;
                }
                Ok(Err(e)) => e.to_string(),
                Err(payload) => crate::startup::last_panic(payload.as_ref()),
            };
            log::warn!("用 {name} 启动失败：{err}");
            crate::startup::append_error_log(&format!("{name}: {err}"));
            self.guard.fail(name, &err);
            last = err;
        }
        let Some((painter, driver)) = painter else { return Err(last) };
        let mut info = ViewportInfo::default();
        egui_winit::update_viewport_info(&mut info, &ctx, &window, true);
        let ppp = egui_winit::pixels_per_point(&ctx, &window);
        ctx.input_mut(|i| {
            i.raw.viewports.insert(ViewportId::ROOT, info.clone());
            i.pixels_per_point = ppp;
        });
        let proxy = Mutex::new(self.proxy.clone());
        ctx.set_request_repaint_callback(move |r| {
            let e = UserEvent::RequestRepaint { when: Instant::now() + r.delay, pass: r.current_cumulative_pass_nr };
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(e);
            }
        });
        let mut state = egui_winit::State::new(ctx.clone(), ViewportId::ROOT, event_loop, Some(window.scale_factor() as f32), event_loop.system_theme(), Some(painter.max_texture_side));
        state.init_accesskit(event_loop, &window, self.proxy.clone());
        let create = self.create.take().ok_or("app already created")?;
        let mut app = create(&ctx);
        use super::frameless::SeeThrough;
        app.see_through_avail = match (borderless, painter.composited()) {
            (true, true) => SeeThrough::Yes,
            (true, false) => SeeThrough::No,
            (false, _) => SeeThrough::Restart,
        };
        if borderless && !painter.composited() {
            log::warn!("无边框窗口透不出桌面（系统合成用不了）");
        }
        self.running = Some(Running {
            window,
            painter,
            state,
            ctx,
            app,
            info,
            actions: Vec::new(),
            pending: Default::default(),
            screenshot: None,
            shown: false,
            beginning: Instant::now(),
            driver,
            dock: None,
            dock_failed: false,
            see_through: false,
            settings_win: None,
            held: Held::None,
        });
        self.next_repaint = Some(Instant::now());
        Ok(())
    }

    /// 跑一帧界面并画出来（与 eframe 的 run_ui_and_paint 同样的顺序）。返回 Err 时要退出
    fn paint(&mut self) -> Result<(), String> {
        let Some(r) = &mut self.running else { return Ok(()) };
        egui_winit::update_viewport_info(&mut r.info, &r.ctx, &r.window, false);
        let mut raw = r.state.take_egui_input(&r.window);
        raw.time = Some(r.beginning.elapsed().as_secs_f64());
        raw.viewports.insert(ViewportId::ROOT, r.info.clone());
        if let Some(shot) = r.screenshot.take() {
            raw.events.push(shot);
        }
        r.app.inject_input(&r.ctx, &mut raw);
        let close_requested = raw.viewport().close_requested();
        let hwnd = hwnd(&r.window).map(|h| h.0 as isize);
        let app = &mut r.app;
        let started = Instant::now();
        let out = r.ctx.run_ui(raw, |ui| app.frame(ui, hwnd));
        r.info.events.clear();
        // 下一帧按这一帧要的时间排（帧里所有重绘请求取最早的）；之前排下的时间已经由这一帧画过了
        let delay = out.viewport_output.get(&ViewportId::ROOT).map_or(Duration::MAX, |v| v.repaint_delay);
        self.next_repaint = started.checked_add(delay).map(|t| if r.window.is_minimized() == Some(true) { t.max(started + HIDDEN_REPAINT) } else { t });
        if close_requested && !out.viewport_output.get(&ViewportId::ROOT).is_some_and(|v| v.commands.contains(&egui::ViewportCommand::CancelClose)) {
            self.close = true;
        }
        r.state.handle_platform_output(&r.window, out.platform_output);
        r.pending.append(out.textures_delta);
        let size = r.window.inner_size();
        // 最小化时窗口是 0×0：只让逻辑往前走，不画（纹理变化攒着，下次一起交）
        if size.width > 0 && size.height > 0 {
            r.painter.resize([size.width, size.height]).map_err(|e| e.0)?;
            let prims = r.ctx.tessellate(out.shapes, out.pixels_per_point);
            let mut shots = Vec::new();
            r.actions.retain(|a| match a {
                ActionRequested::Screenshot(u) => {
                    shots.push(u.clone());
                    false
                }
                _ => true,
            });
            let delta = std::mem::take(&mut r.pending);
            // 透出桌面时清成全透明（画布没画背景）；DWM 的圆角、细边框跟着换
            let see_through = r.app.see_through && r.painter.composited();
            if see_through != r.see_through {
                r.see_through = see_through;
                if let Some(h) = hwnd {
                    crate::platform::windows::set_see_through(h, see_through);
                }
            }
            sync_shadow(r);
            let clear = if see_through { [0.0; 4] } else { r.app.background() };
            let image = r.painter.paint(clear, out.pixels_per_point, &prims, &delta, !shots.is_empty()).map_err(|e| e.0)?;
            // 窗口变好大小后按新大小画出了一帧：回到拉伸铺满（缓冲区和窗口一样大时两种一样）
            if r.held == Held::Applied {
                r.painter.hold_content(None);
                r.held = Held::None;
            }
            if let (Some(image), Some(user_data)) = (image, shots.pop()) {
                r.screenshot = Some(egui::Event::Screenshot { viewport_id: ViewportId::ROOT, user_data, image: Arc::new(image) });
                r.ctx.request_repaint();
            }
            if let Some(d) = &mut r.dock {
                d.dirty = true;
            }
            if !r.shown {
                // 画好第一帧才显示（之前显示会白屏闪一下；驱动卡住时也不会出现一个空窗口）
                r.shown = true;
                r.window.set_visible(true);
            }
        }
        for a in r.actions.drain(..) {
            match a {
                ActionRequested::Cut => r.state.egui_input_mut().events.push(egui::Event::Cut),
                ActionRequested::Copy => r.state.egui_input_mut().events.push(egui::Event::Copy),
                ActionRequested::Paste => {
                    if let Some(text) = r.state.clipboard_text().map(|t| t.replace("\r\n", "\n")).filter(|t| !t.is_empty()) {
                        r.state.egui_input_mut().events.push(egui::Event::Paste(text));
                    }
                }
                ActionRequested::Screenshot(_) => {}
            }
        }
        if let Some(v) = out.viewport_output.get(&ViewportId::ROOT) {
            egui_winit::process_viewport_commands(&r.ctx, &mut r.info, v.commands.clone(), &r.window, &mut r.actions);
        }
        // 这一帧刚换的标题栏也算上（egui-winit 处理 Decorations 时会重设阴影）
        sync_shadow(r);
        // 无边框时窗口包着图片：画完这一帧再一次改好位置和大小（Resized 会马上按新大小再画）。
        // 为菜单、对话框临时变大 / 变回去：先让上一帧不拉伸、留在原来的地方，等画刷的改动生效（about_to_wait）再改窗口，
        // 不然新大小的帧画出来之前，DWM 会把上一帧拉伸铺满整个窗口闪一下
        if let (Some((rect, ratio)), Some(h)) = (r.app.take_window_rect(), hwnd) {
            match ratio {
                Some(ratio) if r.painter.composited() => {
                    r.painter.hold_content(Some(ratio));
                    r.held = Held::Pending(rect);
                }
                _ => crate::platform::windows::set_window_rect(h, rect),
            }
        }
        Ok(())
    }

    /// 浮动工具栏的小窗口：界面要时建（只建一次）、画、放到图片窗口下面；鼠标在不在它上面告诉界面
    fn dock_tick(&mut self, event_loop: &ActiveEventLoop) {
        let Some(r) = &mut self.running else { return };
        if !r.app.dock_allowed() {
            if r.dock.take().is_some() {
                r.app.set_dock_hover(false);
            }
            r.dock_failed = false;
            return;
        }
        let wanted = r.shown && r.app.dock_wanted() && r.window.is_minimized() != Some(true);
        if !wanted {
            if let Some(d) = &mut r.dock {
                // 藏起来后它不再画，输入框的焦点也不会再变：按键还给图片窗口
                d.typing = false;
                d.stray_since = None;
                if d.rect.take().is_some() {
                    let front = crate::platform::windows::is_foreground(d.hwnd);
                    crate::platform::windows::hide(d.hwnd);
                    if front && let Some(main) = hwnd(&r.window) {
                        crate::platform::windows::bring_to_front(main.0 as isize);
                    }
                }
            }
            r.app.set_dock_hover(false);
            return;
        }
        if r.dock.is_none() && !r.dock_failed {
            match Dock::new(event_loop, &r.window, r.driver, &r.ctx) {
                Ok(d) => r.dock = Some(d),
                Err(e) => {
                    log::warn!("浮动工具栏的小窗口建不起来：{e}");
                    r.dock_failed = true;
                }
            }
        }
        let Some(d) = &mut r.dock else { return };
        if d.dirty || d.next.is_some_and(|t| Instant::now() >= t) {
            match d.paint(&mut r.app) {
                Ok(true) => r.window.request_redraw(),
                Ok(false) => {}
                Err(e) => log::warn!("浮动工具栏画不出来：{e}"),
            }
        }
        if let Some(t) = d.next {
            self.next_repaint = Some(self.next_repaint.map_or(t, |n| n.min(t)));
        }
        use crate::platform::windows as win;
        let main = hwnd(&r.window).map(|h| h.0 as isize);
        // 点输入框时它被放到了最前面（WS_EX_NOACTIVATE 也挡不住）。打完（回车、选了预设、点了外面）还在最前面：
        // 等一小会儿（点的那一下先画完）再把最前面还给图片窗口，不然快捷键都到了它这里
        if !d.typing && win::is_foreground(d.hwnd) {
            let since = *d.stray_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= DOCK_REFOCUS {
                d.stray_since = None;
                if let Some(m) = main {
                    win::bring_to_front(m);
                }
            } else {
                let t = since + DOCK_REFOCUS;
                self.next_repaint = Some(self.next_repaint.map_or(t, |n| n.min(t)));
            }
        } else {
            d.stray_since = None;
        }
        if let (Some(m), true) = (main, d.bar.0 > 0 && d.size.0 > 0)
            && let (Some(mr), Some(work)) = (win::window_rect(m), win::work_area(m))
        {
            // 按工具栏放；提示、弹出菜单只让窗口往右、往下变大
            let at = super::frameless::dock_rect(mr, d.bar, work, DOCK_GAP);
            let rect = (at.0, at.1, at.0 + d.size.0.max(d.bar.0), at.1 + d.size.1.max(d.bar.1));
            if d.rect != Some(rect) {
                win::show_at(d.hwnd, rect);
                d.rect = Some(rect);
            }
            let on = win::cursor_pos().is_some_and(|(x, y)| x >= rect.0 && x < rect.2 && y >= rect.1 && y < rect.3);
            r.app.set_dock_hover(on);
        }
    }

    /// 单独的设置窗口：开着设置时建（建不起来就让界面画在图片窗口里）、画；关掉设置时放掉
    fn settings_tick(&mut self, event_loop: &ActiveEventLoop) {
        let Some(r) = &mut self.running else { return };
        let wanted = r.app.settings_page.is_some() && !r.app.settings_window_failed;
        if !wanted {
            if r.settings_win.take().is_some() {
                // 关掉后回到图片窗口
                if let Some(main) = hwnd(&r.window) {
                    crate::platform::windows::bring_to_front(main.0 as isize);
                }
            }
            return;
        }
        if r.settings_win.is_none() {
            match SettingsWin::new(event_loop, &r.window, r.driver, &self.proxy) {
                Ok(w) => r.settings_win = Some(w),
                Err(e) => {
                    log::warn!("设置窗口建不起来，画在图片窗口里：{e}");
                    r.app.settings_window_failed = true;
                    r.window.request_redraw();
                    return;
                }
            }
        }
        let Some(w) = &mut r.settings_win else { return };
        if std::mem::take(&mut r.app.settings_focus) && w.shown {
            crate::platform::windows::bring_to_front(w.hwnd);
        }
        // 开发脚本要设置窗口的截图：画一帧时截
        if r.app.settings_shot_pending() {
            w.dirty = true;
        }
        if w.dirty || w.next.is_some_and(|t| Instant::now() >= t) {
            match w.paint(&mut r.app) {
                Ok(true) => {
                    r.app.close_settings();
                    r.window.request_redraw();
                }
                Ok(false) => {}
                Err(e) => log::warn!("设置窗口画不出来：{e}"),
            }
        }
        if let Some(t) = r.settings_win.as_ref().and_then(|w| w.next) {
            self.next_repaint = Some(self.next_repaint.map_or(t, |n| n.min(t)));
        }
    }

    fn paint_or_exit(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(e) = self.paint() {
            log::error!("画图失败：{e}");
            self.outcome = Some(Outcome::Failed(e));
            event_loop.exit();
        } else if self.close {
            event_loop.exit();
        }
    }
}

impl<F: FnOnce(&egui::Context) -> App> ApplicationHandler<UserEvent> for Host<'_, F> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() || self.outcome.is_some() {
            return;
        }
        if let Err(e) = self.start(event_loop) {
            self.outcome = Some(Outcome::NoRenderer(e));
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // 设置窗口：交给它自己的 egui；关闭按钮 = 关掉设置；改大小时同步重画（拖动改大小时系统的循环里不走 about_to_wait）
        if let Some(r) = self.running.as_mut()
            && let Some(w) = r.settings_win.as_mut().filter(|w| w.window.id() == id)
        {
            match event {
                WindowEvent::CloseRequested => {
                    r.app.close_settings();
                    r.window.request_redraw();
                }
                WindowEvent::Resized(s) if s.width > 0 && s.height > 0 => {
                    let _ = w.state.on_window_event(&w.window, &event);
                    if let Err(e) = w.paint(&mut r.app) {
                        log::warn!("设置窗口画不出来：{e}");
                    }
                }
                _ => {
                    if w.state.on_window_event(&w.window, &event).repaint || matches!(event, WindowEvent::RedrawRequested) {
                        w.dirty = true;
                    }
                }
            }
            return;
        }
        // 浮动工具栏的小窗口：交给它自己的 egui，画留到 about_to_wait
        if let Some(d) = self.running.as_mut().and_then(|r| r.dock.as_mut()).filter(|d| d.window.id() == id) {
            if !matches!(event, WindowEvent::CloseRequested) && (d.state.on_window_event(&d.window, &event).repaint || matches!(event, WindowEvent::RedrawRequested)) {
                d.dirty = true;
            }
            return;
        }
        let Some(r) = &mut self.running else { return };
        // 小窗口的输入框有焦点：按键、输入法转给它，图片窗口不处理（否则打的字会变成快捷键：Delete 删文件、方向键翻页）。
        // 修饰键两边都要知道
        if let Some(d) = r.dock.as_mut().filter(|d| d.typing) {
            let modifiers = matches!(event, WindowEvent::ModifiersChanged(_));
            if modifiers || matches!(event, WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_)) {
                let _ = d.state.on_window_event(&d.window, &event);
                d.dirty = true;
                if !modifiers {
                    return;
                }
            }
        }
        let mut paint_now = false;
        match &event {
            WindowEvent::RedrawRequested => {
                self.paint_or_exit(event_loop);
                return;
            }
            // 用户拖动改变大小时要在这里同步画，否则系统合成器会拉伸旧画面、闪烁（eframe 同样）
            WindowEvent::Resized(s) if s.width > 0 && s.height > 0 => {
                if let Err(e) = r.painter.resize([s.width, s.height]) {
                    self.outcome = Some(Outcome::Failed(e.0));
                    event_loop.exit();
                    return;
                }
                paint_now = true;
            }
            WindowEvent::CloseRequested => {
                if self.close {
                    event_loop.exit();
                    return;
                }
                r.info.events.push(egui::ViewportEvent::Close);
                r.ctx.request_repaint();
            }
            _ => {}
        }
        let Running { window, state, .. } = r;
        let response = state.on_window_event(window, &event);
        if self.close {
            event_loop.exit();
        } else if paint_now {
            self.paint_or_exit(event_loop);
        } else if response.repaint {
            window.request_redraw();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let Some(r) = &mut self.running else { return };
        match event {
            UserEvent::RequestRepaint { when, pass } => {
                let now = r.ctx.cumulative_pass_nr_for(ViewportId::ROOT);
                // 只管帧外的请求（后台线程等）。帧里的请求已经算进那一帧的 repaint_delay（见 paint）；它们的回调
                // 要等 Present 等完垂直同步才送到，这时再按它排就会多画一帧（30 ms 一帧的动图每帧画两次）
                if now == pass {
                    let minimized = r.window.is_minimized() == Some(true);
                    let when = if minimized { when.max(Instant::now() + HIDDEN_REPAINT) } else { when };
                    self.next_repaint = Some(self.next_repaint.map_or(when, |t| t.min(when)));
                }
            }
            // 读屏软件读设置窗口
            UserEvent::AccessKit(e) if r.settings_win.as_ref().is_some_and(|w| w.window.id() == e.window_id) => {
                let Some(w) = &mut r.settings_win else { return };
                match e.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => w.ctx.enable_accesskit(),
                    accesskit_winit::WindowEvent::ActionRequested(req) => w.state.on_accesskit_action_request(req),
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => w.ctx.disable_accesskit(),
                }
                w.dirty = true;
            }
            UserEvent::AccessKit(e) => match e.window_event {
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    r.ctx.enable_accesskit();
                    self.next_repaint = Some(Instant::now());
                }
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    r.state.on_accesskit_action_request(req);
                    r.window.request_redraw();
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => r.ctx.disable_accesskit(),
            },
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 画刷的改动随这一轮消息提交了：现在改窗口大小（之后的 Resized 按新大小画）
        if let Some(r) = &mut self.running
            && let Held::Pending(rect) = r.held
        {
            r.held = Held::Applied;
            if let Some(h) = hwnd(&r.window) {
                crate::platform::windows::set_window_rect(h.0 as isize, rect);
            }
        }
        self.dock_tick(event_loop);
        self.settings_tick(event_loop);
        if let Some(t) = self.next_repaint
            && Instant::now() >= t
        {
            self.next_repaint = None;
            if let Some(r) = &self.running {
                // 最小化 / 还没显示的窗口收不到 RedrawRequested：直接跑这一帧
                if r.window.is_minimized() == Some(true) || !r.shown {
                    self.paint_or_exit(event_loop);
                } else {
                    r.window.request_redraw();
                }
            }
        }
        event_loop.set_control_flow(match self.next_repaint {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(r) = &mut self.running {
            r.app.exit();
        }
    }
}
