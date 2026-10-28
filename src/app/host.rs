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
}

/// 小窗口里排版用的大小（点）：工具栏和它的弹出菜单都放得下；实际窗口只有画出来的那么大
const DOCK_LAYOUT: egui::Vec2 = egui::vec2(900.0, 420.0);
/// 小窗口离图片窗口的距离（物理像素）
const DOCK_GAP: i32 = 6;

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
        Ok(Self { window, painter, state, ctx, hwnd: h.0 as isize, bar: (0, 0), size: (0, 0), rect: None, dirty: true, next: None, pending: Default::default() })
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
        let window = Arc::new(egui_winit::create_window(&ctx, event_loop, &builder).map_err(|e| format!("create window: {e}"))?);
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
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Painter::new(hwnd, driver, [size.width, size.height])));
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
        let app = create(&ctx);
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
            let clear = r.app.background();
            let image = r.painter.paint(clear, out.pixels_per_point, &prims, &delta, !shots.is_empty()).map_err(|e| e.0)?;
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
        // 无边框时窗口包着图片：画完这一帧再一次改好位置和大小（Resized 会马上按新大小再画）
        if let (Some(rect), Some(h)) = (r.app.take_window_rect(), hwnd) {
            crate::platform::windows::set_window_rect(h, rect);
        }
        Ok(())
    }

    /// 浮动工具栏的小窗口：界面要时建（只建一次）、画、放到图片窗口下面；鼠标在不在它上面告诉界面
    fn dock_tick(&mut self, event_loop: &ActiveEventLoop) {
        let Some(r) = &mut self.running else { return };
        let wanted = r.shown && r.app.dock_wanted() && r.window.is_minimized() != Some(true);
        if !wanted {
            if let Some(d) = &mut r.dock
                && d.rect.take().is_some()
            {
                crate::platform::windows::hide(d.hwnd);
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
        // 浮动工具栏的小窗口：交给它自己的 egui，画留到 about_to_wait
        if let Some(d) = self.running.as_mut().and_then(|r| r.dock.as_mut()).filter(|d| d.window.id() == id) {
            if !matches!(event, WindowEvent::CloseRequested) && (d.state.on_window_event(&d.window, &event).repaint || matches!(event, WindowEvent::RedrawRequested)) {
                d.dirty = true;
            }
            return;
        }
        let Some(r) = &mut self.running else { return };
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
        self.dock_tick(event_loop);
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
