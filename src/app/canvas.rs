//! 画布：背景、棋盘格、图片；拖动平移（带惯性）、滚轮缩放、双击切换；空状态 / 出错 / 加载中。

use std::time::{Duration, Instant};

use egui::{Align2, Color32, CursorIcon, FontId, Rect, Sense, Vec2, vec2};

use super::frameless::SeeThrough;
use super::{App, Tex};
use crate::decode::Error;
use crate::i18n::tr;
use crate::settings::{Background, DoubleClick, FitMode, WheelAction};
use crate::ui::{icons, theme, widgets};
use crate::viewer::loader::Target;
use crate::viewer::texture::paint_checker;
use crate::viewer::view::{View, WHEEL_STEP, snap};

/// 微软商店：HEIF 图像扩展、AV1 视频扩展、VP9 视频扩展（WebM 表情包）
const STORE_HEIF: &str = "ms-windows-store://pdp/?ProductId=9PMMSR1CGPWG";
const STORE_AV1: &str = "ms-windows-store://pdp/?ProductId=9MVZQVXJBQ9V";
const STORE_VP9: &str = "ms-windows-store://pdp/?ProductId=9N4D0MSMP0PT";

impl App {
    /// 预读的缩略图按新打开时的显示方式缩小（画布 = 查看时的整个窗口），切过去时就是要显示的样子。
    /// 在收取解码结果之前调用，第一帧就排上的预读也知道窗口大小
    pub(crate) fn update_target(&mut self, ui: &egui::Ui) {
        let ctx = ui.ctx();
        let max_side = ctx.input(|i| i.max_texture_side) as u32;
        self.loader.set_target(Target { fit: self.settings.fit_mode, canvas: ui.max_rect().size(), ppp: ctx.pixels_per_point(), max_side });
    }

    pub(crate) fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        // 窗口为了菜单、对话框、图片信息变大着：画布只占图片那块（在屏幕上没动），其余透明
        let full = ui.max_rect();
        let rect = self.grown_box.unwrap_or(full);
        let bg = match self.settings.background {
            Background::Black => Color32::BLACK,
            Background::White => Color32::WHITE,
            _ => p.canvas,
        };
        // 无边框看有透明像素的图：不画背景和棋盘格，透明的地方透出桌面（窗口循环清成透明色）
        let wanted = self.see_through_wanted(&ctx);
        let can = self.see_through_avail == SeeThrough::Yes;
        // 变大着的窗口图片那块以外都透明（不能透明的窗口照旧整个铺背景）
        let outside_clear = self.grown.is_some() && can;
        self.see_through = can && (wanted || self.grown.is_some());
        let image_clear = wanted && can;
        // 等别的提示（Ctrl+B 的「无边框」说明）显示完再提示
        if wanted && self.see_through_avail == SeeThrough::Restart && !self.see_through_hint && self.toast.is_none() {
            self.see_through_hint = true;
            self.show_toast(tr().see_through_restart);
        }
        if !image_clear {
            let fill = if outside_clear { rect } else { full };
            ui.painter().rect_filled(fill, 0.0, bg);
            if self.settings.background == Background::Checker {
                paint_checker(&ctx, ui.painter(), fill, p.checker);
            }
        }
        // 变大着时点图片那块外面（透明的地方）：关掉图片信息（菜单、对话框各自会关）
        if self.grown_box.is_some() && ui.interact(full, ui.id().with("outside"), Sense::click()).clicked() {
            self.info_open = false;
        }
        if let Some(s) = self.shown {
            let size = self.view.shown_size(s.width, s.height);
            let img_rect = self.update_view(&ctx, rect, size, self.settings.fit_mode);
            let painter = ui.painter_at(rect);
            if s.has_alpha && self.settings.background != Background::Checker && !image_clear {
                paint_checker(&ctx, &painter, img_rect, p.checker);
            }
            self.paint_image(&ctx, &painter, img_rect);
        } else {
            self.canvas = rect;
        }
        // 开着对话框：只压暗图片那块（egui 的对话框压暗整个窗口，变大着时那是一大块灰）
        if self.grown_box.is_some() && self.modal_open() {
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(100));
        }
        let resp = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
        self.pointer(&ctx, &resp);
        self.menu_open = self.context_menu(&resp);
        // 图片信息开着：点一下图片区域（不拖动）就关掉
        if self.info_open && resp.clicked() && !self.window_op {
            self.info_open = false;
        }
        match &self.cur {
            None => self.empty_state(ui, rect),
            Some(c) => match &c.loaded {
                None if self.shown.is_none() && c.since.elapsed().as_millis() > 300 => {
                    let name = crate::natsort::file_name(&c.path);
                    ui.put(Rect::from_center_size(rect.center(), Vec2::splat(28.0)), egui::Spinner::new().size(28.0));
                    ui.painter().text(rect.center() + vec2(0.0, 32.0), Align2::CENTER_TOP, name, FontId::proportional(13.0), p.text_weak);
                }
                None if self.shown.is_none() => ctx.request_repaint_after(std::time::Duration::from_millis(320)),
                Some(Err(e)) => {
                    let e = e.clone();
                    self.error_state(ui, rect, &e);
                }
                _ => {}
            },
        }
    }

    /// 画布变成 rect、图片（旋转后）尺寸 size：按需重新适应，推进缩放动画与惯性，限制平移；返回图片在屏幕上的矩形。
    /// fit_mode = 新图片时的显示方式（编辑时总是适应窗口）
    pub(crate) fn update_view(&mut self, ctx: &egui::Context, rect: Rect, size: Vec2, fit_mode: FitMode) -> Rect {
        let ppp = ctx.pixels_per_point();
        // 拖到缩放比例不同的显示器上：窗口按逻辑大小保持不变、物理像素变了，zoom（按物理像素算）也要重新适应
        let dpi_changed = self.canvas_ppp != ppp && self.canvas_ppp > 0.0;
        let resized = self.canvas.size() != rect.size() || self.canvas_ppp != ppp;
        let old_ppp = self.canvas_ppp;
        self.canvas = rect;
        self.canvas_ppp = ppp;
        // 无边框（窗口就是图片）换到缩放不同的屏：按逻辑大小不变换算缩放（最多 100%、不超过那块屏的九成），
        // 不按系统给的窗口重新适应。不然 1:1 的图到 150% 的屏上还是 100%、窗口变小，回到 125% 的屏时
        // 按变小的窗口适应成 83%，来回一次窗口小一圈
        #[cfg(windows)]
        if dpi_changed && !self.pending_reset && self.frameless() {
            let logical = self.dpi_zoom.filter(|d| (d.1 - self.view.zoom).abs() < 1e-5).map_or(self.view.zoom / old_ppp, |d| d.0);
            self.view.zoom = self.dpi_zoom_for(logical * ppp, size, ppp);
            self.dpi_zoom = Some((logical, self.view.zoom));
            log::debug!("换屏：缩放 {old_ppp} → {ppp}，图片缩放 {:.4}（逻辑 {logical:.4}），画布 {:?}，fitted {}", self.view.zoom, rect.size(), self.view.fitted);
            return self.finish_view(ctx, rect, size);
        }
        let _ = (dpi_changed, old_ppp);
        if self.pending_reset || (resized && self.view.fitted) {
            let mode = if self.pending_reset { fit_mode } else { FitMode::Fit };
            // 无边框时新图按工作区的九成适应，窗口再跟着它变（只按现在的窗口适应的话，窗口会越换越小）
            let area = if self.pending_reset { self.fit_area() } else { rect.size() };
            self.view.reset(mode, area, ppp, size);
            self.pending_reset = false;
        }
        // 记下现在的缩放折成逻辑点是多少（换屏时按它算；用户改了缩放就跟着改）
        if self.dpi_zoom.is_none_or(|d| (d.1 - self.view.zoom).abs() >= 1e-5) {
            self.dpi_zoom = Some((self.view.zoom / ppp, self.view.zoom));
        }
        self.finish_view(ctx, rect, size)
    }

    /// 缩放动画、惯性、限制平移；返回图片在屏幕上的矩形
    fn finish_view(&mut self, ctx: &egui::Context, rect: Rect, size: Vec2) -> Rect {
        let ppp = ctx.pixels_per_point();
        if self.view.tick(rect) {
            ctx.request_repaint();
        }
        if self.fling.length() > 0.5 && !ctx.input(|i| i.pointer.any_down()) {
            self.view.offset += self.fling * ctx.input(|i| i.stable_dt).min(0.05);
            self.fling *= 0.88;
            ctx.request_repaint();
        }
        self.view.clamp(rect, ppp, size);
        self.view.image_rect(rect, ppp, size)
    }

    /// 正在看的是 SVG（原图或缩略图里记着格式）
    fn svg_format(&self) -> bool {
        let Some(c) = &self.cur else { return false };
        let format = match (&c.loaded, &c.preview) {
            (Some(Ok(d)), _) => d.info.format,
            (_, Some(p)) => p.info.format,
            _ => return false,
        };
        format == "SVG"
    }

    fn paint_image(&mut self, ctx: &egui::Context, painter: &egui::Painter, img_rect: Rect) {
        let (xf, zoom, settled) = (self.view.xf, self.view.zoom, !self.view.animating());
        let nearest = self.settings.nearest_above as f32 / 100.0;
        // 只有缩略图、这个比例下不够清楚：要原图（先拉伸缩略图，原图解完换上）
        if let Tex::Still(t) = &self.tex
            && t.wants_source(zoom)
        {
            self.want_full();
        }
        // 放大看 SVG：看得见的那块按屏幕重画后叠上去；盖住整块时底下发虚的栅格不画
        let svg = match (&self.tex, &self.cur, self.shown) {
            (Tex::Still(_), Some(c), Some(s)) if zoom > 1.0 && !c.clipboard && self.svg_format() => Some((c.path.clone(), (s.width, s.height))),
            _ => None,
        };
        let sharp = match &svg {
            Some((path, size)) => self.sharp_svg.overlay(ctx, painter.clip_rect(), path, img_rect, xf, *size, zoom, settled),
            None => {
                self.sharp_svg.drop_overlay();
                None
            }
        };
        let covered = sharp.as_ref().is_some_and(|s| s.1);
        match &mut self.tex {
            Tex::None => {}
            Tex::Still(_) if covered => {}
            Tex::Still(t) => t.paint(ctx, painter, img_rect, zoom, xf, nearest, settled),
            Tex::Anim { decoded, tex, frames } => {
                let crate::decode::Content::Animation { delays, .. } = &decoded.content else { return };
                let result = (|| {
                    if let Some(play) = &mut self.play {
                        if frames.request(play.index, play.playing)?
                            && let Some(wait) = play.tick(Instant::now(), |i| delays[i])
                        {
                            // egui 会从延时里减去预计的一帧时间（提前醒），醒来时离换帧还差一截，就要多画一次旧帧；
                            // 先加回去，到点才醒
                            let early = Duration::try_from_secs_f32(ctx.input(|i| i.predicted_dt)).unwrap_or_default();
                            ctx.request_repaint_after(wait + early);
                        }
                        if !frames.request(play.index, play.playing)? {
                            play.buffering();
                        }
                    }
                    Ok::<_, crate::decode::Error>(())
                })();
                let (index, image) = frames.displayed();
                tex.paint(painter, img_rect, xf, index, image, zoom >= nearest);
                if let Err(e) = result {
                    if let Some(play) = &mut self.play {
                        play.playing = false;
                    }
                    log::warn!("动图解码失败：{e:?}");
                    // 只提示一次；后续保留最后一张已解出的画面。
                    self.show_toast(tr().err_corrupt);
                }
            }
        }
        if let Some((mesh, _)) = sharp {
            painter.add(mesh);
        }
        if !self.first_paint_logged {
            self.first_paint_logged = true;
            log::info!("启动 {:.0} ms：第一张图显示，内存 {:.0} MB", crate::platform::ms_since_process_start(), crate::platform::memory_mb());
        }
    }

    fn pointer(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        // 这次按下已经交给系统移动 / 改大小窗口：松开之前不平移、不算点击
        if self.window_op {
            return;
        }
        let moves = self.drag_moves_window(ctx, self.shown.map(|s| self.view.shown_size(s.width, s.height)));
        if resp.drag_started_by(egui::PointerButton::Primary) {
            log::debug!("开始拖：{}，缩放 {:.4}，画布 {:?}，fitted {}", if moves { "移动窗口" } else { "平移" }, self.view.zoom, self.canvas.size(), self.view.fitted);
            if moves {
                self.start_window_drag(ctx);
                return;
            }
        }
        let ppp = ctx.pixels_per_point();
        let rect = self.canvas;
        let Some(s) = self.shown else { return };
        let size = self.view.shown_size(s.width, s.height);
        if resp.dragged_by(egui::PointerButton::Primary) {
            self.view.offset += resp.drag_delta();
            self.view.fitted = false;
            self.fling = if self.settings.drag_inertia { ctx.input(|i| i.pointer.velocity()) } else { Vec2::ZERO };
        }
        if resp.drag_stopped() && ctx.input(|i| i.pointer.velocity().length()) < 50.0 {
            self.fling = Vec2::ZERO;
        }
        if resp.double_clicked() {
            match self.settings.double_click {
                DoubleClick::Fullscreen => self.toggle_fullscreen(ctx),
                // 和 Esc、Ctrl+W 一样走关闭窗口（旋转了没保存时先问）
                DoubleClick::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                DoubleClick::ToggleFit => {
                    let fit = View::fit_zoom(FitMode::Fit, rect.size(), ppp, size);
                    if (self.view.zoom - 1.0).abs() < 1e-3 && fit < 1.0 {
                        self.view.reset(FitMode::Fit, rect.size(), ppp, size);
                    } else if let Some(pos) = resp.interact_pointer_pos() {
                        self.view.zoom_to(1.0, pos, self.settings.smooth_zoom, rect);
                    }
                    self.show_zoom();
                }
            }
        }
        if resp.hovered() {
            let (mut factor, mut nav) = (1.0f32, 0.0f32);
            let navigate = self.settings.wheel == WheelAction::Navigate;
            ctx.input(|i| {
                for e in &i.events {
                    match e {
                        egui::Event::MouseWheel { unit, delta, modifiers, .. } => {
                            let lines = match unit {
                                egui::MouseWheelUnit::Point => delta.y / 50.0,
                                egui::MouseWheelUnit::Line => delta.y,
                                egui::MouseWheelUnit::Page => delta.y * 10.0,
                            };
                            if navigate && !modifiers.command {
                                nav += lines;
                            } else {
                                factor *= WHEEL_STEP.powf(lines);
                            }
                        }
                        egui::Event::Zoom(z) => factor *= z,
                        _ => {}
                    }
                }
            });
            if (factor - 1.0).abs() > 1e-4 {
                let anchor = ctx.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                let fit = View::fit_zoom(FitMode::Fit, self.fit_area(), ppp, size);
                let from = self.view.target_zoom();
                let to = snap(from, from * factor, &[1.0, fit]);
                self.view.zoom_to(to, anchor, self.settings.smooth_zoom, rect);
                self.show_zoom();
                ctx.request_repaint();
            }
            self.wheel_acc += nav;
            if self.wheel_acc.abs() >= 1.0 {
                let d = if self.wheel_acc < 0.0 { 1 } else { -1 };
                self.wheel_acc = 0.0;
                self.navigate(d);
            }
        }
        let back_fwd = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::PointerButton { button: egui::PointerButton::Extra1, pressed: true, .. } => Some(-1),
                egui::Event::PointerButton { button: egui::PointerButton::Extra2, pressed: true, .. } => Some(1),
                _ => None,
            })
        });
        if let Some(d) = back_fwd {
            self.navigate(d);
        }
        let s_pts = size * self.view.zoom / ppp;
        if resp.hovered() && (moves || s_pts.x > rect.width() + 0.5 || s_pts.y > rect.height() + 0.5) {
            ctx.set_cursor_icon(if resp.dragged() { CursorIcon::Grabbing } else { CursorIcon::Grab });
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = theme::palette(ui.ctx());
        let t = tr();
        let block = Rect::from_center_size(rect.center(), vec2(320.0, 170.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(block).layout(egui::Layout::top_down(egui::Align::Center)));
        let (icon_rect, _) = child.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
        let tex = crate::ui::icons::texture(child.ctx(), icons::Icon::Image, 64.0);
        child.painter().image(tex.id(), icon_rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), p.text_weak.gamma_multiply(0.6));
        child.add_space(16.0);
        if widgets::accent_button(&mut child, t.open, false).clicked() {
            self.open_dialog(ui.ctx());
        }
        child.add_space(10.0);
        child.label(egui::RichText::new(t.empty_hint).color(p.text_weak).size(13.0));
    }

    fn error_state(&mut self, ui: &mut egui::Ui, rect: Rect, e: &Error) {
        let p = theme::palette(ui.ctx());
        let t = tr();
        let reason = match e {
            Error::Unsupported => t.err_unsupported,
            Error::Corrupt(_) => t.err_corrupt,
            Error::MissingCodec(crate::decode::Kind::Heif) => t.err_codec_heic,
            Error::MissingCodec(crate::decode::Kind::Webm) => t.err_codec_vp9,
            Error::MissingCodec(_) => t.err_codec,
            Error::Denied => t.err_denied,
            Error::NotFound => t.err_not_found,
            Error::TooLarge { .. } => t.err_too_large,
        };
        let block = Rect::from_center_size(rect.center(), vec2(440.0, 200.0));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(block).layout(egui::Layout::top_down(egui::Align::Center)));
        let (icon_rect, _) = c.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
        icons::paint(&c, icons::Icon::ImageOff, icon_rect, p.text_weak);
        c.add_space(12.0);
        c.label(egui::RichText::new(t.cannot_open).size(16.0).color(p.text));
        c.add_space(4.0);
        c.label(egui::RichText::new(reason).color(p.text_weak));
        if let Error::TooLarge { width, height } = e
            && *width > 0
        {
            c.label(egui::RichText::new(format!("{width} × {height}")).color(p.text_weak).size(12.0));
        }
        c.add_space(14.0);
        let path = self.cur.as_ref().map(|c| c.path.clone());
        c.horizontal(|ui| {
            let store = match e {
                Error::MissingCodec(crate::decode::Kind::Heif) if cfg!(windows) => Some(STORE_HEIF),
                Error::MissingCodec(crate::decode::Kind::Avif) if cfg!(windows) => Some(STORE_AV1),
                Error::MissingCodec(crate::decode::Kind::Webm) if cfg!(windows) => Some(STORE_VP9),
                _ => None,
            };
            let width = if store.is_some() { 3.0 } else { 2.0 } * 104.0;
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            if let Some(url) = store
                && widgets::accent_button(ui, t.get_from_store, false).clicked()
            {
                crate::platform::open_url(url);
            }
            if let Some(path) = &path {
                if crate::platform::has_open_with() && widgets::plain_button(ui, t.open_other).clicked() {
                    crate::platform::open_with(path);
                }
                if widgets::plain_button(ui, folder_label()).clicked() {
                    crate::platform::show_in_folder(path);
                }
            }
        });
    }
}

pub(crate) fn folder_label() -> &'static str {
    let t = tr();
    if cfg!(target_os = "macos") {
        t.show_in_folder_mac
    } else if cfg!(windows) {
        t.show_in_folder
    } else {
        t.show_in_folder_linux
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::viewer::loader::Loader;

    /// 在缩放比例为 ppp 的显示器上画一帧（窗口逻辑大小不变），返回此时的 zoom
    fn frame(app: &mut App, ctx: &egui::Context, ppp: f32, zoom: Option<f32>) -> f32 {
        let mut input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 700.0))), ..Default::default() };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let _ = ctx.run_ui(input, |ui| {
            let rect = ui.max_rect();
            if let Some(z) = zoom {
                app.view.zoom_to(z, rect.center(), false, rect);
            }
            app.update_view(ui.ctx(), rect, vec2(4000.0, 3000.0), FitMode::Fit);
        });
        app.view.zoom
    }

    #[test]
    fn moving_to_a_screen_with_another_scale_refits_a_fitted_image() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        app.pending_reset = true;
        // 1000×700 点：150% 时 1500×1050 物理像素，100% 时 1000×700
        assert_eq!(frame(&mut app, &ctx, 1.5, None), View::fit_zoom(FitMode::Fit, vec2(1000.0, 700.0), 1.5, vec2(4000.0, 3000.0)));
        assert_eq!(frame(&mut app, &ctx, 1.0, None), View::fit_zoom(FitMode::Fit, vec2(1000.0, 700.0), 1.0, vec2(4000.0, 3000.0)));
        assert_eq!(frame(&mut app, &ctx, 1.5, None), View::fit_zoom(FitMode::Fit, vec2(1000.0, 700.0), 1.5, vec2(4000.0, 3000.0)));
        // 手动缩放过的（例如 100%）：zoom 按物理像素算，换显示器后照旧
        assert_eq!(frame(&mut app, &ctx, 1.5, Some(1.0)), 1.0);
        assert_eq!(frame(&mut app, &ctx, 1.0, None), 1.0);
    }
}

#[cfg(all(test, windows))]
mod drag_tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, RawInput, ViewportCommand, ViewportId, pos2};
    use crate::settings::Settings;
    use crate::viewer::loader::Loader;

    /// 用真实 egui 按下、移动事件驱动画布入口，检查发出的窗口命令及图片偏移。
    struct Canvas {
        app: App,
        ctx: egui::Context,
        input: RawInput,
        modifiers: Modifiers,
    }

    impl Canvas {
        fn new(width: u32, height: u32, zoom: f32) -> Self {
            let ctx = egui::Context::default();
            let settings = Settings { ask_default_app: false, drag_inertia: false, ..Default::default() };
            let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
            app.shown = Some(crate::app::Shown { width, height, has_alpha: false, translucent: false });
            app.view.zoom = zoom;
            let mut input = RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0))), time: Some(0.0), ..Default::default() };
            input.viewports.entry(ViewportId::ROOT).or_default().outer_rect = Some(Rect::from_min_size(pos2(100.0, 100.0), vec2(1220.0, 840.0)));
            Self { app, ctx, input, modifiers: Modifiers::NONE }
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<ViewportCommand> {
            *self.input.time.as_mut().unwrap() += 0.02;
            let mut input = self.input.clone();
            input.events = vec![Event::ModifiersChanged(self.modifiers)];
            input.events.extend(events);
            let mut out = self.ctx.run_ui(input, |ui| {
                self.app.canvas = ui.max_rect();
                let resp = ui.interact(ui.max_rect(), ui.id().with("canvas"), Sense::click_and_drag());
                self.app.pointer(ui.ctx(), &resp);
            });
            out.viewport_output.remove(&ViewportId::ROOT).unwrap().commands
        }

        fn button(&mut self, pressed: bool) -> Vec<ViewportCommand> {
            self.frame(vec![Event::PointerButton { pos: pos2(300.0, 300.0), button: PointerButton::Primary, pressed, modifiers: self.modifiers }])
        }

        fn drag(&mut self) -> Vec<ViewportCommand> {
            assert!(!self.frame(vec![Event::PointerMoved(pos2(300.0, 300.0))]).contains(&ViewportCommand::StartDrag));
            assert!(!self.button(true).contains(&ViewportCommand::StartDrag));
            self.frame(vec![Event::PointerMoved(pos2(380.0, 360.0))])
        }
    }

    #[test]
    fn dragging_uses_visible_size_including_dpi_and_rotation_not_zoom_percentage() {
        for (w, h, zoom, ppp, rotation, moves) in [
            (6000, 4000, 0.5, 1.0, 0, false), // 不到 100% 仍要平移
            (300, 200, 2.0, 1.0, 0, true),   // 超过 100% 仍完整可见
            (1200, 800, 1.0, 1.0, 0, true), // 刚好放下
            (1201, 800, 1.0, 1.0, 0, false),
            (1800, 1200, 1.0, 1.5, 0, true), // 150% DPI 下刚好放下
            (1800, 1200, 1.0, 1.0, 0, false),
            (600, 1100, 1.0, 1.0, 0, false), // 长图只有竖向超出也须平移
            (600, 1100, 1.0, 1.0, 1, true),  // 旋转后完整可见
        ] {
            let mut c = Canvas::new(w, h, zoom);
            c.input.viewports.get_mut(&ViewportId::ROOT).unwrap().native_pixels_per_point = Some(ppp);
            c.app.view.xf.rotation = rotation;
            let cmds = c.drag();
            assert_eq!(cmds.contains(&ViewportCommand::StartDrag), moves, "{w}×{h}, zoom {zoom}, DPI {ppp}, rotation {rotation}");
            assert_eq!(c.app.window_op, moves);
            assert_eq!(c.app.view.offset, if moves { Vec2::ZERO } else { vec2(80.0, 60.0) });
        }
    }

    #[test]
    fn shift_drag_moves_the_window_once_without_panning_or_inertia() {
        let mut c = Canvas::new(6000, 4000, 1.0);
        c.modifiers = Modifiers::SHIFT;
        c.app.fling = vec2(200.0, 100.0);
        let cmds = c.drag();
        assert!(cmds.contains(&ViewportCommand::StartDrag));
        assert!(cmds.contains(&ViewportCommand::OuterPosition(pos2(180.0, 160.0))));
        assert_eq!(c.app.view.offset, Vec2::ZERO);
        assert_eq!(c.app.fling, Vec2::ZERO);
        // 松开 Shift 继续拖也不会重新启动系统拖窗或改成平移。
        c.modifiers = Modifiers::NONE;
        assert!(!c.frame(vec![Event::PointerMoved(pos2(450.0, 400.0))]).contains(&ViewportCommand::StartDrag));
        assert_eq!(c.app.view.offset, Vec2::ZERO);
    }

    #[test]
    fn starting_a_pan_then_pressing_shift_does_not_switch_to_window_drag() {
        let mut c = Canvas::new(6000, 4000, 0.5);
        assert!(!c.drag().contains(&ViewportCommand::StartDrag));
        c.modifiers = Modifiers::SHIFT;
        assert!(!c.frame(vec![Event::PointerMoved(pos2(450.0, 400.0))]).contains(&ViewportCommand::StartDrag));
        assert_eq!(c.app.view.offset, vec2(150.0, 100.0));
    }

    #[test]
    fn maximized_normal_windows_fullscreen_and_dialogs_do_not_start_window_drag() {
        for borderless in [false, true] {
            let mut c = Canvas::new(300, 200, 1.0);
            c.app.settings.window.borderless = borderless;
            c.modifiers = Modifiers::SHIFT;
            c.input.viewports.get_mut(&ViewportId::ROOT).unwrap().maximized = Some(true);
            let cmds = c.drag();
            assert_eq!(cmds.contains(&ViewportCommand::StartDrag), borderless);
            assert!(!cmds.iter().any(|c| matches!(c, ViewportCommand::OuterPosition(_))));

            let mut c = Canvas::new(300, 200, 1.0);
            c.app.settings.window.borderless = borderless;
            c.app.fullscreen = true;
            c.modifiers = Modifiers::SHIFT;
            assert!(!c.drag().contains(&ViewportCommand::StartDrag));
        }
        // 设置画在图片窗口里时（其它系统；Windows 上单独的设置窗口建不起来时）
        let mut c = Canvas::new(300, 200, 1.0);
        c.app.settings_page = Some(crate::app::settings_ui::Page::General);
        c.app.settings_window_failed = true;
        assert!(!c.drag().contains(&ViewportCommand::StartDrag));
        // Windows：设置在单独的窗口里，图片窗口照常能拖
        #[cfg(windows)]
        {
            let mut c = Canvas::new(300, 200, 1.0);
            c.app.settings_page = Some(crate::app::settings_ui::Page::General);
            assert!(c.drag().contains(&ViewportCommand::StartDrag));
        }
        let mut c = Canvas::new(300, 200, 1.0);
        c.app.menu_open = true;
        assert!(!c.drag().contains(&ViewportCommand::StartDrag));
        let mut c = Canvas::new(300, 200, 1.0);
        c.app.shown = None;
        assert!(!c.drag().contains(&ViewportCommand::StartDrag));
    }

    #[test]
    fn right_click_opens_the_menu_where_it_was_clicked() {
        let mut c = Canvas::new(300, 200, 1.0);
        let at = pos2(260.0, 180.0);
        let frame = |c: &mut Canvas, events: Vec<Event>| {
            *c.input.time.as_mut().unwrap() += 0.02;
            let mut input = c.input.clone();
            input.events = events;
            let _ = c.ctx.run_ui(input, |ui| c.app.canvas_ui(ui));
        };
        frame(&mut c, vec![Event::PointerMoved(at)]);
        frame(&mut c, vec![Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE }]);
        frame(&mut c, vec![Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
        frame(&mut c, vec![]);
        assert!(c.app.menu_open, "menu open");
        assert!(!c.app.menu_pending);
        assert!(c.ctx.any_popup_open());
        // 左键点一下关掉
        frame(&mut c, vec![Event::PointerButton { pos: pos2(40.0, 40.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        frame(&mut c, vec![Event::PointerButton { pos: pos2(40.0, 40.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        frame(&mut c, vec![]);
        assert!(!c.app.menu_open);
    }

    #[test]
    fn double_click_still_changes_zoom_without_dragging_the_window() {
        let mut c = Canvas::new(300, 200, 2.0);
        c.app.settings.double_click = DoubleClick::ToggleFit;
        c.app.settings.smooth_zoom = false;
        c.frame(vec![Event::PointerMoved(pos2(300.0, 300.0))]);
        for _ in 0..2 {
            assert!(!c.button(true).contains(&ViewportCommand::StartDrag));
            assert!(!c.button(false).contains(&ViewportCommand::StartDrag));
        }
        assert_eq!(c.app.view.zoom, 1.0);
        assert!(!c.app.window_op);
    }

    /// 画布那一帧：透出桌面了没有、有没有提示「重新打开」
    fn see_through_after_frame(c: &mut Canvas) -> bool {
        let input = c.input.clone();
        let _ = c.ctx.run_ui(input, |ui| c.app.canvas_ui(ui));
        c.app.see_through
    }

    #[test]
    fn borderless_shows_the_desktop_only_for_images_with_transparent_pixels() {
        let mut c = Canvas::new(300, 200, 1.0);
        c.app.settings.window.borderless = true;
        c.app.see_through_avail = SeeThrough::Yes;
        c.app.shown = Some(crate::app::Shown { width: 300, height: 200, has_alpha: true, translucent: true });
        assert!(see_through_after_frame(&mut c));
        // 有透明通道、没有透明像素；普通窗口；最大化；开着设置
        c.app.shown = Some(crate::app::Shown { width: 300, height: 200, has_alpha: true, translucent: false });
        assert!(!see_through_after_frame(&mut c));
        c.app.shown = Some(crate::app::Shown { width: 300, height: 200, has_alpha: true, translucent: true });
        c.app.settings.window.borderless = false;
        assert!(!see_through_after_frame(&mut c));
        c.app.settings.window.borderless = true;
        c.input.viewports.get_mut(&ViewportId::ROOT).unwrap().maximized = Some(true);
        assert!(!see_through_after_frame(&mut c));
        c.input.viewports.get_mut(&ViewportId::ROOT).unwrap().maximized = Some(false);
        c.app.settings_page = Some(crate::app::settings_ui::Page::General);
        c.app.settings_window_failed = true;
        assert!(!see_through_after_frame(&mut c));
        c.app.settings_page = None;
        c.app.settings_window_failed = false;
        assert!(see_through_after_frame(&mut c));
        // 启动时不是无边框：不透，提示一次重新打开
        c.app.see_through_avail = SeeThrough::Restart;
        assert!(!see_through_after_frame(&mut c));
        assert!(c.app.toast.take().is_some_and(|t| t.0 == crate::i18n::tr().see_through_restart));
        assert!(!see_through_after_frame(&mut c));
        assert!(c.app.toast.is_none());
        // 系统合成用不了：不透，也不提示
        let mut c = Canvas::new(300, 200, 1.0);
        c.app.settings.window.borderless = true;
        c.app.shown = Some(crate::app::Shown { width: 300, height: 200, has_alpha: true, translucent: true });
        assert!(!see_through_after_frame(&mut c));
        assert!(c.app.toast.is_none());
    }

    #[test]
    fn double_click_can_close_the_window() {
        for (setting, closes) in [(DoubleClick::Close, true), (DoubleClick::ToggleFit, false), (DoubleClick::Fullscreen, false)] {
            let mut c = Canvas::new(300, 200, 1.0);
            c.app.settings.double_click = setting;
            c.frame(vec![Event::PointerMoved(pos2(300.0, 300.0))]);
            let mut cmds = Vec::new();
            for _ in 0..2 {
                cmds.extend(c.button(true));
                cmds.extend(c.button(false));
            }
            assert_eq!(cmds.contains(&ViewportCommand::Close), closes, "{setting:?}");
            assert!(!cmds.contains(&ViewportCommand::StartDrag), "{setting:?}");
        }
        // 单击不关
        let mut c = Canvas::new(300, 200, 1.0);
        c.app.settings.double_click = DoubleClick::Close;
        c.frame(vec![Event::PointerMoved(pos2(300.0, 300.0))]);
        let mut cmds = c.button(true);
        cmds.extend(c.button(false));
        assert!(!cmds.contains(&ViewportCommand::Close));
    }
}
