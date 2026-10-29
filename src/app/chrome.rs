//! 浮动工具栏（鼠标移到窗口底部时淡入，移开后淡出）、动图播放条、左右切换按钮、轻提示。

use std::time::Duration;

use egui::containers::menu::MenuButton;
use egui::{Align2, Area, CornerRadius, Id, Order, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::App;
use crate::i18n::tr;
use crate::ui::icons::{self, Icon};
use crate::ui::{theme, widgets};
use crate::viewer::playback::SPEEDS;

const BAR_H: f32 = 44.0;
/// 工具栏距窗口底边
const BAR_GAP: f32 = 16.0;
/// 底部感应区比工具栏（动图时连同播放条）再高出这么多
const ZONE_MARGIN: f32 = 32.0;
/// 鼠标离开浮动工具栏的小窗口后过这么久才收起（从图片移过去要经过中间的缝）
const DOCK_GRACE: std::time::Duration = std::time::Duration::from_millis(500);
/// 工具栏两边至少留这么多（点），窗口再窄就放到下面的小窗口里
const DOCK_ROOM: f32 = 16.0;

impl App {
    pub(crate) fn chrome(&mut self, ctx: &egui::Context) {
        // 开着对话框、右键菜单时都不出来：重新出现的浮层会排到它们上面，盖住按钮、抢走点击
        let blocked = self.modal_open() || self.menu_open;
        let inside = !blocked && ctx.input(|i| i.pointer.has_pointer());
        // 无边框、窗口比工具栏还窄时，工具栏和播放条放到图片下面的小窗口里（`dock_ui`，窗口循环去画），不在窗口里截断
        // 按这个窗口宽度下会画的那种工具栏（窄时没有「适应窗口」「原始大小」，动图另有播放条）比，放得下就留在窗口里
        // 右键菜单「窗口 → 工具栏放不下时放到图片下面」开着才这样（默认关）
        self.docked = self.frameless() && self.settings.window.toolbar_outside && self.canvas.width() < self.toolbar_w[self.bar_kind()] + DOCK_ROOM;
        if !self.docked && !ctx.input(|i| i.pointer.primary_down()) {
            self.flip.button = None;
        }
        // 只在鼠标移到窗口底部时显示；它的弹出菜单（比例预设、播放速度）开着时不收起。图片信息面板开着时放在剩下的地方
        let free = self.free_rect(ctx);
        let bars = if self.is_animated() { 2.0 } else { 1.0 };
        let zone = BAR_GAP + bars * (BAR_H + 8.0) + ZONE_MARGIN;
        let near = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|q| free.contains(q) && q.y >= free.bottom() - zone);
        // 小窗口：鼠标在它上面、它的弹出菜单开着、刚离开图片底部或小窗口不到半秒（中间隔着一条缝）时都不收起
        if self.docked && (near || self.dock_hover) {
            self.dock_seen = Some(std::time::Instant::now());
        }
        let on_dock = self.docked && (self.dock_hover || self.dock_popup || self.dock_seen.is_some_and(|t| t.elapsed() < DOCK_GRACE));
        let show = self.cur.is_some() && !blocked && (near || on_dock || (self.bar_shown && ctx.any_popup_open()));
        self.bar_shown = show;
        if self.docked {
            if show {
                // 鼠标在小窗口上时这个窗口收不到移动，过一会儿再看一次
                ctx.request_repaint_after(DOCK_GRACE / 4);
            }
            return;
        }
        let o = ctx.animate_bool_with_time(Id::new("toolbar-shown"), show, 0.15);
        let p = theme::palette(ctx);
        let dx = free.center().x - ctx.content_rect().center().x;
        if o > 0.0 {
            let bar = Area::new(Id::new("toolbar")).order(Order::Foreground).anchor(Align2::CENTER_BOTTOM, vec2(dx, -BAR_GAP)).interactable(o > 0.5).show(ctx, |ui| {
                ui.set_opacity(o);
                widgets::floating_frame(p).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        self.toolbar_contents(ui);
                    });
                });
            });
            self.toolbar_w[self.bar_kind()] = bar.response.rect.width();
        }
        let paused = self.play.as_ref().is_some_and(|pl| !pl.playing);
        let oa = ctx.animate_bool_with_time(Id::new("animbar-shown"), self.is_animated() && (show || paused && inside), 0.15);
        if oa > 0.0 {
            let anim = Area::new(Id::new("animbar")).order(Order::Foreground).anchor(Align2::CENTER_BOTTOM, vec2(dx, -BAR_GAP - BAR_H - 8.0)).interactable(oa > 0.5).show(
                ctx,
                |ui| {
                    ui.set_opacity(oa);
                    widgets::floating_frame(p).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            self.anim_contents(ui);
                        });
                    });
                },
            );
            // 动图：播放条和工具栏取宽的那个
            let k = self.bar_kind();
            self.toolbar_w[k] = self.toolbar_w[k].max(anim.response.rect.width());
        }
        self.edge_arrows(ctx);
    }

    /// 浮动工具栏的小窗口里画什么（窗口循环在它自己的 egui 里调）：动图的播放条和工具栏。返回工具栏占的地方
    /// （提示、弹出菜单另外占的不算：小窗口按工具栏放，它们只让窗口往右、往下变大，工具栏不跟着跑）
    #[cfg(windows)]
    pub(crate) fn dock_ui(&mut self, ctx: &egui::Context) -> egui::Rect {
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.flip.button = None;
        }
        let p = theme::palette(ctx);
        let kind = self.bar_kind();
        let area = Area::new(Id::new("dock")).order(Order::Foreground).fixed_pos(egui::Pos2::ZERO).show(ctx, |ui| {
            egui::Frame::new().fill(p.floating).inner_margin(egui::Margin::symmetric(6, 6)).show(ui, |ui| {
                ui.vertical(|ui| {
                    if self.is_animated() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            self.anim_contents(ui);
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        self.toolbar_contents(ui);
                    });
                });
            });
        });
        // 和窗口里那条比：那条多 1 点的边框
        self.toolbar_w[kind] = area.response.rect.width() + 2.0;
        self.dock_popup = ctx.any_popup_open();
        area.response.rect
    }

    /// 工具栏的样子：0 / 1 = 窄（没有「适应窗口」「原始大小」），2 / 3 = 宽；单数 = 动图（多一条播放条）
    pub(crate) fn bar_kind(&self) -> usize {
        usize::from(self.canvas.width() >= 640.0) * 2 + usize::from(self.is_animated())
    }

    /// 窗口循环问：小窗口还要不要留着（关掉了这个选项或无边框时连它的显卡设备一起放掉）
    #[cfg(windows)]
    pub(crate) fn dock_allowed(&self) -> bool {
        self.settings.window.borderless && self.settings.window.toolbar_outside
    }

    /// 窗口循环问：要不要显示浮动工具栏的小窗口
    #[cfg(windows)]
    pub(crate) fn dock_wanted(&self) -> bool {
        self.docked && self.bar_shown && !self.window_op && !self.fullscreen
    }

    /// 窗口循环告诉界面鼠标在不在小窗口上
    #[cfg(windows)]
    pub(crate) fn set_dock_hover(&mut self, on: bool) {
        self.dock_hover = on;
    }

    fn toolbar_contents(&mut self, ui: &mut egui::Ui) {
        let t = tr();
        let many = self.folder.len() > 1;
        let has = self.shown.is_some();
        let prev = widgets::icon_button(ui, Icon::Prev, t.prev, "←", many);
        self.flip_button(&prev, -1, many);
        let pos = if self.folder.is_empty() { String::new() } else { format!("{} / {}", self.folder.index + 1, self.folder.len()) };
        ui.add_sized([76.0, 32.0], egui::Label::new(egui::RichText::new(pos).size(13.0)));
        let next = widgets::icon_button(ui, Icon::Next, t.next, "→", many);
        self.flip_button(&next, 1, many);
        widgets::separator(ui);
        if widgets::icon_button(ui, Icon::ZoomOut, t.zoom_out, "Ctrl+-", has).clicked() {
            self.zoom_step(false);
        }
        let pct = format!("{:.0}%", self.view.zoom * 100.0);
        let button = egui::Button::new(egui::RichText::new(pct).size(13.0)).min_size(vec2(58.0, 32.0)).frame_when_inactive(false);
        let menu = MenuButton::from_button(button).ui(ui, |ui| {
            // 输入比例（回车生效），或选预设
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.zoom_text).desired_width(56.0).hint_text("100"));
                ui.label("%");
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Ok(v) = self.zoom_text.trim().trim_end_matches('%').parse::<f32>()
                        && v > 0.0
                    {
                        self.zoom_set(v / 100.0);
                    }
                    ui.close();
                }
            });
            ui.separator();
            for z in [0.25f32, 0.5, 1.0, 2.0, 4.0] {
                if ui.button(format!("{:.0}%", z * 100.0)).clicked() {
                    self.zoom_set(z);
                    ui.close();
                }
            }
        });
        if menu.0.clicked() {
            self.zoom_text = format!("{:.0}", self.view.zoom * 100.0);
        }
        if widgets::icon_button(ui, Icon::ZoomIn, t.zoom_in, "Ctrl+=", has).clicked() {
            self.zoom_step(true);
        }
        // 窗口窄时不放「适应窗口」「原始大小」按钮（Ctrl+0 / Ctrl+1、双击还在）
        if self.canvas.width() >= 640.0 {
            if widgets::icon_button(ui, Icon::Fit, t.fit, "Ctrl+0", has).clicked() {
                self.zoom_fit();
            }
            if widgets::icon_button(ui, Icon::Actual, t.actual, "Ctrl+1", has).clicked() {
                self.zoom_actual();
            }
        }
        // 锁定缩放：窗口窄时也留着（没有快捷键）
        if widgets::icon_button_ex(ui, Icon::Lock, t.lock_zoom, "", true, self.settings.lock_zoom, None).clicked() {
            self.settings.lock_zoom = !self.settings.lock_zoom;
            self.settings.save();
        }
        widgets::separator(ui);
        if widgets::icon_button(ui, Icon::RotateCw, t.rotate_cw, "Ctrl+R", has).clicked() {
            self.rotate(true);
        }
        // 只显示着缩略图时也能编辑：点了先解码原图，到了再进入
        let editable = has && self.has_image() && !self.is_animated();
        let tip = if self.is_animated() { t.anim_no_edit } else { t.edit };
        if widgets::icon_button(ui, Icon::Edit, tip, "E", editable).clicked() {
            self.enter_edit(super::edit::Tool::Select);
        }
        let deletable = self.cur.as_ref().is_some_and(|c| !c.clipboard);
        let del = widgets::icon_button_ex(ui, Icon::Delete, t.delete, "Del", deletable, false, None);
        if del.hovered() && deletable {
            // 悬停变红
            let r = Rect::from_center_size(del.rect.center(), Vec2::splat(widgets::ICON));
            icons::paint(ui, Icon::Delete, r, theme::palette(ui.ctx()).danger);
        }
        if del.clicked() {
            self.request_delete(false);
        }
        if widgets::icon_button_ex(ui, Icon::Info, t.info, "I", has, self.info_open, None).clicked() {
            self.info_open = !self.info_open;
        }
    }

    fn anim_contents(&mut self, ui: &mut egui::Ui) {
        let t = tr();
        let delay = self.decoded().and_then(|d| match &d.content {
            crate::decode::Content::Animation { delays, .. } => self.play.as_ref().and_then(|p| delays.get(p.index).copied()),
            _ => None,
        });
        let Some(play) = &mut self.play else { return };
        let (icon, tip) = if play.playing { (Icon::Pause, t.pause) } else { (Icon::Play, t.play) };
        if widgets::icon_button(ui, icon, tip, "Space", true).clicked() {
            play.toggle();
        }
        if widgets::icon_button(ui, Icon::FramePrev, t.frame_prev, ",", true).clicked() {
            play.step(-1);
        }
        if widgets::icon_button(ui, Icon::FrameNext, t.frame_next, ".", true).clicked() {
            play.step(1);
        }
        let n = play.count();
        if let Some(i) = scrubber(ui, play.index, n) {
            play.seek(i);
            play.playing = false;
        }
        ui.add_sized([84.0, 32.0], egui::Label::new(egui::RichText::new(format!("{} / {}", play.index + 1, n)).size(13.0)));
        if let Some(d) = delay {
            ui.add_sized([56.0, 32.0], egui::Label::new(egui::RichText::new(format!("{} ms", d.as_millis())).size(13.0)));
        }
        let button = egui::Button::new(egui::RichText::new(format!("{}×", play.speed)).size(13.0)).min_size(vec2(48.0, 32.0)).frame_when_inactive(false);
        MenuButton::from_button(button).ui(ui, |ui| {
            for s in SPEEDS {
                if ui.button(format!("{s}×")).clicked() {
                    play.set_speed(s);
                    ui.close();
                }
            }
        });
    }

    /// 翻页按钮：按下就翻一张，按住连续翻（`flip`）；键盘 / 读屏软件的「点击」也翻一张
    fn flip_button(&mut self, resp: &egui::Response, d: isize, enabled: bool) {
        if !enabled {
            return;
        }
        let (pressed, down, released, on) = resp.ctx.input(|i| {
            let p = &i.pointer;
            (p.primary_pressed(), p.primary_down(), p.primary_released(), p.latest_pos().is_some_and(|q| resp.interact_rect.contains(q)))
        });
        if pressed && resp.is_pointer_button_down_on() {
            self.flip.pressed = Some(d);
            self.flip.button = Some(resp.id);
        }
        if down && on && self.flip.button == Some(resp.id) {
            self.flip.held = Some(d);
        } else if resp.clicked() && !down && (pressed || !released) {
            // 同一帧里按下又松开，或者键盘 / 读屏软件的点击（鼠标的普通点击在按下时已经翻过）
            self.flip.pressed = Some(d);
        }
    }

    /// 画布去掉图片信息面板占的地方（工具栏、左右按钮、提示放在这里面）
    pub(crate) fn free_rect(&self, ctx: &egui::Context) -> Rect {
        let mut r = self.canvas;
        r.min.x = (r.min.x + self.info_width(ctx)).min(r.max.x);
        r
    }

    fn edge_arrows(&mut self, ctx: &egui::Context) {
        if self.folder.len() < 2 || self.modal_open() || self.menu_open {
            return;
        }
        let pos = ctx.input(|i| i.pointer.hover_pos());
        let r = self.free_rect(ctx);
        let zone = (r.width() * 0.15).clamp(60.0, 160.0);
        let p = theme::palette(ctx);
        for (left, icon, d, tip) in [(true, Icon::Prev, -1, tr().prev), (false, Icon::Next, 1, tr().next)] {
            // 在图片信息面板上时不算靠近左边
            let near = pos.is_some_and(|q| q.x >= r.left() && if left { q.x < r.left() + zone } else { q.x > r.right() - zone });
            let a = ctx.animate_bool_with_time(Id::new(("edge", left)), near, 0.15);
            if a <= 0.0 {
                continue;
            }
            let center = pos2(if left { r.left() + 36.0 } else { r.right() - 36.0 }, r.center().y);
            Area::new(Id::new(("edge-area", left))).order(Order::Foreground).fixed_pos(center - Vec2::splat(20.0)).interactable(a > 0.5).show(ctx, |ui| {
                ui.set_opacity(a);
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
                let fill = if resp.hovered() { p.surface } else { p.floating };
                ui.painter().circle(rect.center(), 20.0, fill, Stroke::new(1.0, p.divider));
                icons::paint(ui, icon, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), p.text);
                let resp = resp.on_hover_text(tip);
                self.flip_button(&resp, d, true);
            });
        }
    }

    pub(crate) fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = &self.toast else { return };
        let age = at.elapsed().as_secs_f32();
        if age > 2.0 {
            self.toast = None;
            return;
        }
        let o = if age > 1.7 { (2.0 - age) / 0.3 } else { 1.0 };
        ctx.request_repaint_after(Duration::from_secs_f32(if age < 1.7 { 1.7 - age } else { 0.03 }));
        let p = theme::palette(ctx);
        let text = text.clone();
        let y = if self.editor.is_some() {
            // 编辑时在状态栏上方
            -28.0 - 16.0
        } else if self.is_animated() {
            -16.0 - 2.0 * (BAR_H + 8.0) - 4.0
        } else {
            -16.0 - BAR_H - 12.0
        };
        let dx = self.free_rect(ctx).center().x - ctx.content_rect().center().x;
        Area::new(Id::new("toast")).order(Order::Tooltip).anchor(Align2::CENTER_BOTTOM, vec2(dx, y)).interactable(false).show(ctx, |ui| {
            ui.set_opacity(o);
            egui::Frame::new().fill(p.text).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::symmetric(14, 7)).show(ui, |ui| {
                ui.label(egui::RichText::new(text).color(p.canvas));
            });
        });
    }

    /// 翻页提示（已回到第一张、已经是最后一张、只有这一张）：在图片区域正中，字大，停一会儿再淡出；不挡鼠标
    pub(crate) fn notice_ui(&mut self, ctx: &egui::Context) {
        const SHOW: f32 = 1.6;
        const FADE: f32 = 0.3;
        let Some((text, at)) = &self.notice else { return };
        let age = at.elapsed().as_secs_f32();
        if age > SHOW {
            self.notice = None;
            return;
        }
        let o = (age / 0.12).min(1.0).min((SHOW - age) / FADE);
        ctx.request_repaint_after(Duration::from_secs_f32(if (0.12..SHOW - FADE).contains(&age) { SHOW - FADE - age } else { 0.016 }));
        let p = theme::palette(ctx);
        let text = text.clone();
        let center = if self.canvas.is_positive() { self.free_rect(ctx).center() } else { ctx.content_rect().center() };
        Area::new(Id::new("notice")).order(Order::Tooltip).pivot(Align2::CENTER_CENTER).fixed_pos(center).interactable(false).show(ctx, |ui| {
            ui.set_opacity(o);
            egui::Frame::new()
                .fill(p.text.gamma_multiply(0.92))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(egui::Margin::symmetric(28, 16))
                .shadow(egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: egui::Color32::from_black_alpha(60) })
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(text).size(22.0).color(p.canvas));
                });
        });
    }
}

/// 帧进度条：返回拖到的帧
fn scrubber(ui: &mut egui::Ui, index: usize, n: usize) -> Option<usize> {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(200.0, 32.0), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), vec2(rect.width() - 14.0, 4.0));
    let frac = if n > 1 { index as f32 / (n - 1) as f32 } else { 0.0 };
    let x = track.left() + frac * track.width();
    ui.painter().rect_filled(track, CornerRadius::same(2), p.divider);
    ui.painter().rect_filled(Rect::from_min_max(track.min, pos2(x, track.max.y)), CornerRadius::same(2), p.accent);
    let r = if resp.hovered() || resp.dragged() { 7.0 } else { 6.0 };
    ui.painter().circle_filled(pos2(x, rect.center().y), r, p.accent);
    if (resp.dragged() || resp.clicked())
        && n > 1
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let f = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        return Some((f * (n - 1) as f32).round() as usize);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::viewer::loader::Loader;

    /// 鼠标停在窗口底部画一帧，返回工具栏是否显示
    fn hover_bottom(app: &mut App, ctx: &egui::Context) -> bool {
        let mut input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 700.0))), ..Default::default() };
        input.events.push(egui::Event::PointerMoved(pos2(500.0, 690.0)));
        let _ = ctx.run_ui(input, |ui| {
            app.canvas = ui.max_rect();
            app.chrome(ui.ctx());
        });
        app.bar_shown
    }

    #[test]
    fn the_bottom_toolbar_stays_hidden_while_a_dialog_is_open() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        app.cur = Some(crate::app::Current { path: "a.png".into(), loaded: None, preview: None, since: std::time::Instant::now(), clipboard: false });
        assert!(hover_bottom(&mut app, &ctx));
        app.settings_page = Some(Default::default());
        assert!(!hover_bottom(&mut app, &ctx));
        app.settings_page = None;
        assert!(hover_bottom(&mut app, &ctx));
        // 右键菜单开着时也不出来（菜单靠近底部时会被它盖住）
        app.menu_open = true;
        assert!(!hover_bottom(&mut app, &ctx));
        app.menu_open = false;
        assert!(hover_bottom(&mut app, &ctx));
    }
}
