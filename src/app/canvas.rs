//! 画布：背景、棋盘格、图片；拖动平移（带惯性）、滚轮缩放、双击切换；空状态 / 出错 / 加载中。

use std::time::Instant;

use egui::{Align2, Color32, CursorIcon, FontId, Rect, Sense, Vec2, vec2};

use super::{App, Tex};
use crate::decode::Error;
use crate::i18n::tr;
use crate::settings::{Background, DoubleClick, FitMode, WheelAction};
use crate::ui::{icons, theme, widgets};
use crate::viewer::texture::paint_checker;
use crate::viewer::view::{View, WHEEL_STEP, snap};

/// 微软商店：HEIF 图像扩展、AV1 视频扩展
const STORE_HEIF: &str = "ms-windows-store://pdp/?ProductId=9PMMSR1CGPWG";
const STORE_AV1: &str = "ms-windows-store://pdp/?ProductId=9MVZQVXJBQ9V";

impl App {
    pub(crate) fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let rect = ui.max_rect();
        let ppp = ctx.pixels_per_point();
        let bg = match self.settings.background {
            Background::Black => Color32::BLACK,
            Background::White => Color32::WHITE,
            _ => p.canvas,
        };
        ui.painter().rect_filled(rect, 0.0, bg);
        if self.settings.background == Background::Checker {
            paint_checker(&ctx, ui.painter(), rect, p.checker);
        }
        let resized = self.canvas.size() != rect.size();
        self.canvas = rect;
        if let Some(s) = self.shown {
            let size = self.view.shown_size(s.width, s.height);
            if self.pending_reset || (resized && self.view.fitted) {
                let mode = if self.pending_reset { self.settings.fit_mode } else { FitMode::Fit };
                self.view.reset(mode, rect.size(), ppp, size);
                self.pending_reset = false;
            }
            if self.view.tick(rect) {
                ctx.request_repaint();
            }
            if self.fling.length() > 0.5 && !ctx.input(|i| i.pointer.any_down()) {
                self.view.offset += self.fling * ctx.input(|i| i.stable_dt).min(0.05);
                self.fling *= 0.88;
                ctx.request_repaint();
            }
            self.view.clamp(rect, ppp, size);
            let img_rect = self.view.image_rect(rect, ppp, size);
            if s.has_alpha && self.settings.background != Background::Checker {
                paint_checker(&ctx, ui.painter(), img_rect, p.checker);
            }
            self.paint_image(&ctx, ui.painter(), img_rect);
        }
        let resp = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
        self.pointer(&ctx, &resp);
        self.context_menu(&resp);
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

    fn paint_image(&mut self, ctx: &egui::Context, painter: &egui::Painter, img_rect: Rect) {
        let (xf, zoom, settled) = (self.view.xf, self.view.zoom, !self.view.animating());
        let nearest = self.settings.nearest_above as f32 / 100.0;
        match &mut self.tex {
            Tex::None => {}
            Tex::Still(t) => t.paint(ctx, painter, img_rect, zoom, xf, nearest, settled),
            Tex::Anim { decoded, tex } => {
                let crate::decode::Content::Animation { frames, .. } = &decoded.content else { return };
                let index = match &mut self.play {
                    Some(play) => {
                        if let Some(wait) = play.tick(Instant::now(), |i| frames[i].delay) {
                            ctx.request_repaint_after(wait);
                        }
                        play.index.min(frames.len() - 1)
                    }
                    None => 0,
                };
                tex.paint(painter, img_rect, xf, index, &frames[index].image, zoom >= nearest);
            }
        }
        if !self.first_paint_logged {
            self.first_paint_logged = true;
            log::info!(
                "启动 {:.0} ms：第一张图显示，内存 {:.0} MB",
                crate::platform::ms_since_process_start(),
                crate::platform::memory_mb()
            );
        }
    }

    fn pointer(&mut self, ctx: &egui::Context, resp: &egui::Response) {
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
                DoubleClick::ToggleFit => {
                    let fit = View::fit_zoom(FitMode::Fit, rect.size(), ppp, size);
                    if (self.view.zoom - 1.0).abs() < 1e-3 && fit < 1.0 {
                        self.view.reset(FitMode::Fit, rect.size(), ppp, size);
                    } else if let Some(pos) = resp.interact_pointer_pos() {
                        self.view.zoom_to(1.0, pos, self.settings.smooth_zoom, rect);
                    }
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
                let fit = View::fit_zoom(FitMode::Fit, rect.size(), ppp, size);
                let from = self.view.target_zoom();
                let to = snap(from, from * factor, &[1.0, fit]);
                self.view.zoom_to(to, anchor, self.settings.smooth_zoom, rect);
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
        if resp.hovered() && (s_pts.x > rect.width() + 0.5 || s_pts.y > rect.height() + 0.5) {
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
