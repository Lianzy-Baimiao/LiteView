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
/// 窗口跟着图片时的最小窗口（点）：放得下细标题栏的三个按钮；比它小的图周围才有背景
pub(crate) const HUG_MIN: egui::Vec2 = egui::vec2(160.0, 100.0);
/// 有标题栏时的最小窗口（与 `app::viewport` 相同）
const NORMAL_MIN: egui::Vec2 = egui::vec2(520.0, 360.0);
/// 打开新图时最多占工作区的这么多（与「窗口适应图片大小」相同）
const HUG_SCREEN: f32 = 0.9;

/// 窗口跟着图片：想要的客户区大小 want（物理像素，已限在最小 / 工作区之间），让 anchor（窗口里的相对位置，0..1）
/// 在屏幕上不动，再整个挪进工作区。返回 (左, 上, 右, 下)
pub(crate) fn hug_rect(cur: (i32, i32, i32, i32), want: (i32, i32), anchor: (f32, f32), work: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (w, h) = (want.0.clamp(1, (work.2 - work.0).max(1)), want.1.clamp(1, (work.3 - work.1).max(1)));
    let ax = cur.0 as f32 + (cur.2 - cur.0) as f32 * anchor.0;
    let ay = cur.1 as f32 + (cur.3 - cur.1) as f32 * anchor.1;
    let l = ((ax - w as f32 * anchor.0).round() as i32).clamp(work.0, work.2 - w);
    let t = ((ay - h as f32 * anchor.1).round() as i32).clamp(work.1, work.3 - h);
    (l, t, l + w, t + h)
}

/// 浮动工具栏的小窗口放在哪（物理像素）：图片窗口下面居中、隔 gap；下面放不下就放上面；都放不下时贴着工作区
#[cfg(windows)]
pub(crate) fn dock_rect(main: (i32, i32, i32, i32), size: (i32, i32), work: (i32, i32, i32, i32), gap: i32) -> (i32, i32, i32, i32) {
    let (w, h) = (size.0.min(work.2 - work.0), size.1.min(work.3 - work.1));
    let l = ((main.0 + main.2) / 2 - w / 2).clamp(work.0, work.2 - w);
    let t = if main.3 + gap + h <= work.3 { main.3 + gap } else { (main.1 - gap - h).clamp(work.1, work.3 - h) };
    (l, t, l + w, t + h)
}

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
            // 无边框时比缩小停下的大小再小一点：winit 的最小值另加几像素边框，Windows 改大小时会照它限住，图片旁边就露出一条背景
            ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(if decorated { NORMAL_MIN } else { HUG_MIN * 0.75 }));
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

    /// 「适应窗口」按的大小：窗口跟着图片时按工作区的九成（不然只会适应现在这个窗口，越缩越小）
    pub(crate) fn fit_area(&self) -> egui::Vec2 {
        self.hug_area.unwrap_or(self.canvas.size())
    }

    /// 每帧最后：无边框时窗口正好包着图片。滚轮缩放时鼠标下的点不动，换图时窗口中心不动；放不下时最大到工作区，
    /// 再放大就在窗口里平移。最大化、全屏、拖动 / 改大小中、开着菜单 / 对话框 / 图片信息时不动窗口
    pub(crate) fn hug(&mut self, ctx: &egui::Context) {
        // 移动 / 改大小窗口时什么都不动（改大小时宽高比要一直锁着）
        if self.window_op {
            return;
        }
        #[cfg(windows)]
        if self.hug_now(ctx) {
            return;
        }
        let _ = ctx;
        self.hug_area = None;
        self.hug_image = None;
        // 用户关掉了无边框：下次打开时按那时的缩放，不回到以前的
        if !self.settings.window.borderless {
            self.hug_zoom = None;
        }
        #[cfg(windows)]
        crate::platform::windows::set_resize_aspect(0.0);
    }

    #[cfg(windows)]
    fn hug_now(&mut self, ctx: &egui::Context) -> bool {
        use crate::platform::windows as win;
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let (Some(hwnd), Some(s)) = (self.hwnd, self.shown) else { return false };
        if !self.frameless() || self.fullscreen || maximized || self.menu_open || self.modal_open() || self.info_open {
            return false;
        }
        let (Some(work), Some(cur)) = (win::work_area(hwnd), win::window_rect(hwnd)) else { return false };
        let ppp = ctx.pixels_per_point();
        let work_size = egui::vec2((work.2 - work.0) as f32, (work.3 - work.1) as f32);
        self.hug_area = Some(work_size * HUG_SCREEN / ppp);
        self.view.settle(self.canvas);
        let path = self.cur.as_ref().map(|c| c.path.clone());
        // 编辑、最大化、全屏、图片信息之后回来，还是同一张图：用原来的缩放（不然每来回一次窗口就小一点）
        if self.hug_image.is_none()
            && let Some((p, z)) = self.hug_zoom.clone()
            && path.as_ref() == Some(&p)
        {
            self.view.zoom_to(z, self.canvas.center(), false, self.canvas);
        }
        let size = self.view.shown_size(s.width, s.height).max(egui::Vec2::splat(1.0));
        // 缩小到窗口最小时就不再缩（否则窗口停在最小、图片四周露出背景）；本来就比最小还小的图按 100% 显示
        let min_px = HUG_MIN * ppp;
        let floor = (min_px.x / size.x).max(min_px.y / size.y).min(1.0);
        if self.view.zoom < floor - 1e-4 {
            self.view.zoom_to(floor, self.canvas.center(), false, self.canvas);
        }
        let shown = size * self.view.zoom;
        let want = (shown.x.round().max(min_px.x.round()) as i32, shown.y.round().max(min_px.y.round()) as i32);
        // 同一张图：鼠标在窗口里时按鼠标的位置缩放；换了图或鼠标不在窗口里时按窗口中心。
        // 用系统的鼠标位置：窗口在不动的鼠标下面变了大小时，egui 记的鼠标位置还是旧的，连着滚几格窗口会跑偏
        let same = self.hug_image.is_some() && self.hug_image == path;
        let anchor = match win::cursor_pos() {
            Some((x, y)) if same && x >= cur.0 && x < cur.2 && y >= cur.1 && y < cur.3 => {
                ((x - cur.0) as f32 / (cur.2 - cur.0) as f32, (y - cur.1) as f32 / (cur.3 - cur.1) as f32)
            }
            _ => (0.5, 0.5),
        };
        self.hug_zoom = path.clone().map(|p| (p, self.view.zoom));
        self.hug_image = path;
        // 大小没变就不动窗口（用户把它拖到一半出了屏幕也不拉回来）；要变大小时才按锚点放、挪进工作区
        let w = (want.0.min(work.2 - work.0), want.1.min(work.3 - work.1));
        if (w.0 - (cur.2 - cur.0)).abs() > 1 || (w.1 - (cur.3 - cur.1)).abs() > 1 {
            self.window_rect = Some(hug_rect(cur, want, anchor, work));
        }
        // 拖边缘改大小时保持图片的宽高比；窗口已经被工作区限住（图片比屏幕大）时不锁
        let clamped = shown.x > work_size.x + 0.5 || shown.y > work_size.y + 0.5;
        win::set_resize_aspect(if clamped { 0.0 } else { size.x / size.y });
        true
    }

    /// 这一帧要的窗口位置和大小（物理像素），由窗口循环在画完之后一次改好
    #[cfg(windows)]
    pub(crate) fn take_window_rect(&mut self) -> Option<(i32, i32, i32, i32)> {
        self.window_rect.take()
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
            // 改大小时图片一直适应窗口（窗口保持图片的宽高比），松开后窗口正好包着图片
            self.view.fitted = true;
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

    #[cfg(windows)]
    #[test]
    fn the_toolbar_window_goes_under_the_image_or_above_it_when_there_is_no_room() {
        let work = (0, 0, 1920, 1040);
        // 下面放得下：居中、隔 6
        assert_eq!(dock_rect((800, 300, 1100, 500), (600, 56), work, 6), (650, 506, 1250, 562));
        // 下面放不下：放上面
        assert_eq!(dock_rect((800, 900, 1100, 1030), (600, 56), work, 6), (650, 838, 1250, 894));
        // 左边会出工作区：贴着左边
        assert_eq!(dock_rect((0, 300, 200, 433), (600, 56), work, 6), (0, 439, 600, 495));
    }

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
    fn the_hugging_window_keeps_the_anchor_still_and_stays_inside_the_work_area() {
        let work = (0, 0, 3840, 2160);
        // 按中心放大：中心不动
        assert_eq!(hug_rect((1000, 800, 1400, 1100), (800, 600), (0.5, 0.5), work), (800, 650, 1600, 1250));
        // 鼠标在左上角：左上角不动
        assert_eq!(hug_rect((1000, 800, 1400, 1100), (200, 150), (0.0, 0.0), work), (1000, 800, 1200, 950));
        // 比工作区大：限在工作区里
        assert_eq!(hug_rect((1000, 800, 1400, 1100), (5000, 3000), (0.5, 0.5), work), (0, 0, 3840, 2160));
        // 会超出右边：整个挪回工作区
        assert_eq!(hug_rect((3500, 100, 3800, 300), (600, 400), (0.5, 0.5), work), (3240, 0, 3840, 400));
        // 第二块屏（工作区不从 0 开始）、大小不变：原样
        assert_eq!(hug_rect((3900, 50, 4300, 350), (400, 300), (0.3, 0.7), (3840, 0, 6400, 1560)), (3900, 50, 4300, 350));
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
