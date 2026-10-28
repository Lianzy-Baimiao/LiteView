//! 无边框模式（Windows，`docs/BORDERLESS-PLAN.md`）：去掉系统标题栏，鼠标到顶部时淡入一条细标题栏；
//! 图片整张在窗口里时拖图片移动窗口（Shift + 拖总是移动），拖窗口边缘改大小。移动、改大小都交给系统
//! （`StartDrag` / `BeginResize`），贴边分屏、拖到别的屏和有标题栏时一样。编辑时临时恢复系统标题栏。

use egui::{Align2, Area, Color32, CursorIcon, FontId, Id, Order, Pos2, Rect, ResizeDirection, Sense, Stroke, StrokeKind, ViewportCommand, pos2, vec2};

use super::App;
use crate::i18n::tr;
use crate::ui::theme;

/// 只在 Windows 上做；其它系统保留系统标题栏，菜单和 Ctrl+B 都没有
pub(crate) const SUPPORTED: bool = cfg!(windows);
/// 细标题栏的高度、离顶边多近时出来
const TITLE_H: f32 = 32.0;
const ZONE: f32 = 40.0;
/// 右边三个按钮的宽度（与系统标题栏相同）
const BUTTON_W: f32 = 46.0;
/// 边缘改大小的范围：四边 / 四角
const EDGE: f32 = 6.0;
const CORNER: f32 = 12.0;

/// 鼠标在窗口的哪条边、哪个角上（四角优先）
pub(crate) fn edge_at(r: Rect, q: Pos2) -> Option<ResizeDirection> {
    if !r.contains(q) {
        return None;
    }
    let (l, t, rt, b) = (q.x - r.left(), q.y - r.top(), r.right() - q.x, r.bottom() - q.y);
    let (cl, ct, cr, cb) = (l < CORNER, t < CORNER, rt < CORNER, b < CORNER);
    Some(match () {
        _ if ct && cl => ResizeDirection::NorthWest,
        _ if ct && cr => ResizeDirection::NorthEast,
        _ if cb && cl => ResizeDirection::SouthWest,
        _ if cb && cr => ResizeDirection::SouthEast,
        _ if t < EDGE => ResizeDirection::North,
        _ if b < EDGE => ResizeDirection::South,
        _ if l < EDGE => ResizeDirection::West,
        _ if rt < EDGE => ResizeDirection::East,
        _ => return None,
    })
}

/// 拖图片时移动窗口还是平移图片：无边框时，图片整张在窗口里（没有可平移的方向）或按住 Shift 就移动窗口
pub(crate) fn drag_moves_window(borderless: bool, pannable: bool, shift: bool) -> bool {
    borderless && (shift || !pannable)
}

fn cursor(d: ResizeDirection) -> CursorIcon {
    match d {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

impl App {
    /// 这一帧窗口是不是无边框（编辑时恢复标题栏）
    pub(crate) fn frameless(&self) -> bool {
        SUPPORTED && self.settings.window.borderless && self.editor.is_none()
    }

    /// 每帧：标题栏、置顶跟设置和编辑状态走（全屏时不动标题栏，退出全屏后再换）
    pub(crate) fn sync_frame(&mut self, ctx: &egui::Context) {
        let decorated = !self.frameless();
        if decorated != self.decorated && !self.fullscreen {
            ctx.send_viewport_cmd(ViewportCommand::Decorations(decorated));
            self.decorated = decorated;
        }
        let top = self.settings.window.always_on_top;
        if top != self.on_top {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
            self.on_top = top;
        }
    }

    /// 交给系统移动窗口。拖过 egui 的拖动门槛才开始，这时鼠标已经离按下的地方几个像素：先把窗口挪过去，
    /// 免得图片在鼠标下面滑开（最大化时由系统先还原，不挪）
    pub(crate) fn start_window_drag(&mut self, ctx: &egui::Context) {
        let (moved, outer, maximized) = ctx.input(|i| {
            let moved = i.pointer.interact_pos().zip(i.pointer.press_origin()).map(|(now, from)| now - from);
            (moved, i.viewport().outer_rect, i.viewport().maximized.unwrap_or(false))
        });
        if let (Some(d), Some(r), false) = (moved, outer, maximized) {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(r.min + d));
        }
        self.window_op = true;
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }

    pub(crate) fn toggle_borderless(&mut self) {
        self.settings.window.borderless = !self.settings.window.borderless;
        if self.settings.window.borderless {
            self.show_toast(tr().borderless_tip);
        }
    }

    /// 拖图片时移动窗口（在按下后开始拖的那一帧决定，拖到一半不换）
    pub(crate) fn drag_moves_window(&self, ctx: &egui::Context, size: Option<egui::Vec2>) -> bool {
        let pannable = size.is_some_and(|s| {
            let s = s * self.view.zoom / ctx.pixels_per_point();
            s.x > self.canvas.width() + 0.5 || s.y > self.canvas.height() + 0.5
        });
        drag_moves_window(self.frameless(), pannable, ctx.input(|i| i.modifiers.shift))
    }

    /// 无边框、没有最大化 / 全屏时：鼠标在边缘上显示改大小的箭头，按下就交给系统改大小
    pub(crate) fn resize_edges(&mut self, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if !self.frameless() || self.fullscreen || maximized || self.modal_open() {
            return;
        }
        let Some(q) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let Some(d) = edge_at(ctx.content_rect(), q) else { return };
        ctx.set_cursor_icon(cursor(d));
        if !self.window_op && ctx.input(|i| i.pointer.primary_pressed()) {
            self.window_op = true;
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(d));
        }
    }

    /// 细标题栏：鼠标到顶部时淡入。左边文件名，右边最小化、最大化 / 还原、关闭；拖它移动窗口，双击最大化 / 还原
    pub(crate) fn titlebar(&mut self, ctx: &egui::Context) {
        if !self.frameless() || self.fullscreen {
            self.titlebar_shown = false;
            return;
        }
        // 开着对话框、右键菜单时不出来（和底部工具栏一样）
        let blocked = self.modal_open() || self.menu_open;
        let full = ctx.content_rect();
        let near = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|q| q.y < full.top() + ZONE);
        let show = !blocked && (near || self.titlebar_shown && self.window_op);
        self.titlebar_shown = show;
        let o = ctx.animate_bool_with_time(Id::new("titlebar-shown"), show, 0.15);
        if o <= 0.0 {
            return;
        }
        let p = theme::palette(ctx);
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let title = self.title.clone();
        Area::new(Id::new("titlebar")).order(Order::Foreground).fixed_pos(full.min).interactable(o > 0.5).show(ctx, |ui| {
            ui.set_opacity(o);
            let (rect, resp) = ui.allocate_exact_size(vec2(full.width(), TITLE_H), Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, p.floating);
            ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, p.divider));
            if resp.double_clicked() {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            } else if resp.drag_started_by(egui::PointerButton::Primary) {
                self.start_window_drag(ctx);
            }
            let t = tr();
            let middle = if maximized { (Glyph::Restore, t.restore_window) } else { (Glyph::Maximize, t.maximize) };
            let buttons = [(Glyph::Minimize, t.minimize), middle, (Glyph::Close, t.close)];
            let left = rect.right() - BUTTON_W * buttons.len() as f32;
            for (k, (glyph, tip)) in buttons.into_iter().enumerate() {
                let r = Rect::from_min_size(pos2(left + BUTTON_W * k as f32, rect.top()), vec2(BUTTON_W, TITLE_H));
                if caption_button(ui, r, glyph, tip, p).clicked() {
                    ctx.send_viewport_cmd(match glyph {
                        Glyph::Minimize => ViewportCommand::Minimized(true),
                        Glyph::Maximize | Glyph::Restore => ViewportCommand::Maximized(!maximized),
                        Glyph::Close => ViewportCommand::Close,
                    });
                }
            }
            let clip = Rect::from_min_max(rect.min, pos2(left - 8.0, rect.bottom()));
            ui.painter().with_clip_rect(clip).text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, title, FontId::proportional(13.0), p.text);
        });
    }
}

#[derive(Clone, Copy)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// 标题栏按钮：画线条图标（与系统标题栏的一样），关闭按钮悬停时红底白字
fn caption_button(ui: &mut egui::Ui, r: Rect, glyph: Glyph, tip: &str, p: &theme::Palette) -> egui::Response {
    let resp = ui.interact(r, Id::new(("caption", glyph as u8)), Sense::click()).on_hover_text(tip);
    let close = matches!(glyph, Glyph::Close);
    let fill = match (resp.is_pointer_button_down_on(), resp.hovered(), close) {
        (_, true, true) | (true, _, true) => Color32::from_rgb(0xC4, 0x2B, 0x1C),
        (true, _, false) => p.pressed,
        (false, true, false) => p.hover,
        _ => Color32::TRANSPARENT,
    };
    let painter = ui.painter();
    painter.rect_filled(r, 0.0, fill);
    let color = if close && fill != Color32::TRANSPARENT { Color32::WHITE } else { p.text };
    let s = Stroke::new(1.0, color);
    let c = r.center();
    let h = 5.0;
    match glyph {
        Glyph::Minimize => {
            painter.line_segment([pos2(c.x - h, c.y), pos2(c.x + h, c.y)], s);
        }
        Glyph::Maximize => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(2.0 * h, 2.0 * h)), 1.0, s, StrokeKind::Inside);
        }
        Glyph::Restore => {
            // 后面的方框只露出上边和右边
            let front = Rect::from_center_size(c + vec2(-1.5, 1.5), vec2(2.0 * h - 2.0, 2.0 * h - 2.0));
            painter.line_segment([front.left_top() + vec2(2.5, -2.5), front.right_top() + vec2(2.5, -2.5)], s);
            painter.line_segment([front.right_top() + vec2(2.5, -2.5), front.right_bottom() + vec2(2.5, -2.5)], s);
            painter.rect_stroke(front, 1.0, s, StrokeKind::Inside);
        }
        Glyph::Close => {
            painter.line_segment([c - vec2(h, h), c + vec2(h, h)], s);
            painter.line_segment([c + vec2(-h, h), c + vec2(h, -h)], s);
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::viewer::loader::Loader;

    #[test]
    fn corners_win_over_edges_and_the_inside_is_not_an_edge() {
        let r = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        assert_eq!(edge_at(r, pos2(3.0, 10.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(edge_at(r, pos2(795.0, 3.0)), Some(ResizeDirection::NorthEast));
        assert_eq!(edge_at(r, pos2(5.0, 595.0)), Some(ResizeDirection::SouthWest));
        assert_eq!(edge_at(r, pos2(790.0, 590.0)), Some(ResizeDirection::SouthEast));
        assert_eq!(edge_at(r, pos2(400.0, 2.0)), Some(ResizeDirection::North));
        assert_eq!(edge_at(r, pos2(400.0, 597.0)), Some(ResizeDirection::South));
        assert_eq!(edge_at(r, pos2(1.0, 300.0)), Some(ResizeDirection::West));
        assert_eq!(edge_at(r, pos2(799.0, 300.0)), Some(ResizeDirection::East));
        // 离边 6 点以上不算边；四角范围（12 点）只在角上
        assert_eq!(edge_at(r, pos2(400.0, 8.0)), None);
        assert_eq!(edge_at(r, pos2(20.0, 300.0)), None);
        assert_eq!(edge_at(r, pos2(400.0, 300.0)), None);
        assert_eq!(edge_at(r, pos2(900.0, 300.0)), None);
    }

    #[test]
    fn dragging_the_image_moves_the_window_only_when_borderless_and_nothing_to_pan() {
        assert!(drag_moves_window(true, false, false));
        assert!(!drag_moves_window(true, true, false));
        // Shift + 拖总是移动窗口；有标题栏时一直是平移
        assert!(drag_moves_window(true, true, true));
        assert!(!drag_moves_window(false, false, false));
        assert!(!drag_moves_window(false, true, true));
    }

    #[cfg(windows)]
    #[test]
    fn ctrl_b_turns_borderless_on_and_hides_the_title_bar() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        let press = egui::Event::Key { key: egui::Key::B, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
        let out = ctx.run_ui(egui::RawInput { events: vec![press], ..Default::default() }, |ui| {
            app.keyboard(ui.ctx());
            app.sync_frame(ui.ctx());
        });
        assert!(app.settings.window.borderless);
        let cmds = &out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(cmds.contains(&ViewportCommand::Decorations(false)), "{cmds:?}");
        // 编辑时恢复标题栏的判断
        assert!(app.frameless());
    }
}
