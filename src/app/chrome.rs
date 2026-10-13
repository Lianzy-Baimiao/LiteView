//! 浮动工具栏（鼠标移动时淡入，静止 2 秒后淡出）、动图播放条、左右切换按钮、轻提示。

use std::time::{Duration, Instant};

use egui::containers::menu::MenuButton;
use egui::{Align2, Area, CornerRadius, Id, Order, Rect, Sense, Stroke, Vec2, pos2, vec2};

use super::App;
use crate::i18n::tr;
use crate::ui::icons::{self, Icon};
use crate::ui::{theme, widgets};
use crate::viewer::playback::SPEEDS;

const HIDE_AFTER: f32 = 2.0;
const BAR_H: f32 = 44.0;

impl App {
    pub(crate) fn chrome(&mut self, ctx: &egui::Context) {
        let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_pressed());
        if moved {
            self.last_move = Instant::now();
        }
        let inside = ctx.input(|i| i.pointer.has_pointer());
        let idle = self.last_move.elapsed().as_secs_f32();
        let over = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| self.bar_rect.expand(8.0).contains(p));
        let show = self.cur.is_some() && inside && (idle < HIDE_AFTER || over);
        if show && !over {
            ctx.request_repaint_after(Duration::from_secs_f32((HIDE_AFTER - idle).max(0.0) + 0.05));
        }
        let o = ctx.animate_bool_with_time(Id::new("toolbar-shown"), show, 0.15);
        let p = theme::palette(ctx);
        if o > 0.0 {
            let r = Area::new(Id::new("toolbar"))
                .order(Order::Foreground)
                .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -16.0))
                .interactable(o > 0.5)
                .show(ctx, |ui| {
                    ui.set_opacity(o);
                    widgets::floating_frame(p).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            self.toolbar_contents(ui);
                        });
                    });
                });
            self.bar_rect = r.response.rect;
        }
        let paused = self.play.as_ref().is_some_and(|pl| !pl.playing);
        let oa = ctx.animate_bool_with_time(Id::new("animbar-shown"), self.is_animated() && (show || paused && inside), 0.15);
        if oa > 0.0 {
            Area::new(Id::new("animbar"))
                .order(Order::Foreground)
                .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -16.0 - BAR_H - 8.0))
                .interactable(oa > 0.5)
                .show(ctx, |ui| {
                    ui.set_opacity(oa);
                    widgets::floating_frame(p).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            self.anim_contents(ui);
                        });
                    });
                });
        }
        self.edge_arrows(ctx);
        self.toast_ui(ctx);
    }

    fn toolbar_contents(&mut self, ui: &mut egui::Ui) {
        let t = tr();
        let many = self.folder.len() > 1;
        let has = self.shown.is_some();
        if widgets::icon_button(ui, Icon::Prev, t.prev, "←", many).clicked() {
            self.navigate(-1);
        }
        let pos = if self.folder.is_empty() { String::new() } else { format!("{} / {}", self.folder.index + 1, self.folder.len()) };
        ui.add_sized([76.0, 32.0], egui::Label::new(egui::RichText::new(pos).size(13.0)));
        if widgets::icon_button(ui, Icon::Next, t.next, "→", many).clicked() {
            self.navigate(1);
        }
        widgets::separator(ui);
        if widgets::icon_button(ui, Icon::ZoomOut, t.zoom_out, "Ctrl+-", has).clicked() {
            self.zoom_step(false);
        }
        let pct = format!("{:.0}%", self.view.zoom * 100.0);
        let button = egui::Button::new(egui::RichText::new(pct).size(13.0)).min_size(vec2(58.0, 32.0)).frame_when_inactive(false);
        MenuButton::from_button(button).ui(ui, |ui| {
            for z in [0.25f32, 0.5, 1.0, 2.0, 4.0] {
                if ui.button(format!("{:.0}%", z * 100.0)).clicked() {
                    self.zoom_set(z);
                    ui.close();
                }
            }
        });
        if widgets::icon_button(ui, Icon::ZoomIn, t.zoom_in, "Ctrl+=", has).clicked() {
            self.zoom_step(true);
        }
        if widgets::icon_button(ui, Icon::Fit, t.fit, "Ctrl+0", has).clicked() {
            self.zoom_fit();
        }
        if widgets::icon_button(ui, Icon::Actual, t.actual, "Ctrl+1", has).clicked() {
            self.zoom_actual();
        }
        widgets::separator(ui);
        if widgets::icon_button(ui, Icon::RotateCw, t.rotate_cw, "Ctrl+R", has).clicked() {
            self.rotate(true);
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
    }

    fn anim_contents(&mut self, ui: &mut egui::Ui) {
        let t = tr();
        let delay = self.decoded().and_then(|d| match &d.content {
            crate::decode::Content::Animation { frames, .. } => self.play.as_ref().map(|p| frames[p.index.min(frames.len() - 1)].delay),
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

    fn edge_arrows(&mut self, ctx: &egui::Context) {
        if self.folder.len() < 2 || self.dialog.is_some() {
            return;
        }
        let pos = ctx.input(|i| i.pointer.hover_pos());
        let r = self.canvas;
        let zone = (r.width() * 0.15).clamp(60.0, 160.0);
        let p = theme::palette(ctx);
        for (left, icon, d, tip) in [(true, Icon::Prev, -1, tr().prev), (false, Icon::Next, 1, tr().next)] {
            let near = pos.is_some_and(|q| if left { q.x < r.left() + zone } else { q.x > r.right() - zone });
            let a = ctx.animate_bool_with_time(Id::new(("edge", left)), near, 0.15);
            if a <= 0.0 {
                continue;
            }
            let center = pos2(if left { r.left() + 36.0 } else { r.right() - 36.0 }, r.center().y);
            Area::new(Id::new(("edge-area", left)))
                .order(Order::Foreground)
                .fixed_pos(center - Vec2::splat(20.0))
                .interactable(a > 0.5)
                .show(ctx, |ui| {
                    ui.set_opacity(a);
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
                    let fill = if resp.hovered() { p.surface } else { p.floating };
                    ui.painter().circle(rect.center(), 20.0, fill, Stroke::new(1.0, p.divider));
                    icons::paint(ui, icon, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), p.text);
                    if resp.on_hover_text(tip).clicked() {
                        self.navigate(d);
                    }
                });
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
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
        let y = if self.is_animated() { -16.0 - 2.0 * (BAR_H + 8.0) - 4.0 } else { -16.0 - BAR_H - 12.0 };
        Area::new(Id::new("toast")).order(Order::Tooltip).anchor(Align2::CENTER_BOTTOM, vec2(0.0, y)).interactable(false).show(ctx, |ui| {
            ui.set_opacity(o);
            egui::Frame::new().fill(p.text).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::symmetric(14, 7)).show(ui, |ui| {
                ui.label(egui::RichText::new(text).color(p.canvas));
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
    if (resp.dragged() || resp.clicked()) && n > 1
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let f = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        return Some((f * (n - 1) as f32).round() as usize);
    }
    None
}
