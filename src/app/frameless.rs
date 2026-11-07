//! 无边框模式（Windows，`docs/BORDERLESS-PLAN.md`）：去掉系统标题栏，鼠标到顶部时淡入一条细标题栏；
//! 图片整张在窗口里时拖图片移动窗口（普通未最大化窗口也支持，Shift + 拖强制移动），拖窗口边缘改大小。移动、改大小都交给系统
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
#[cfg(windows)]
const HUG_SCREEN: f32 = 0.9;

/// 窗口跟着图片：想要的客户区大小 want（物理像素，已限在最小 / 工作区之间），让 anchor（窗口里的相对位置，0..1）
/// 在屏幕上不动，再整个挪进工作区。返回 (左, 上, 右, 下)
#[cfg(windows)]
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

/// 屏幕上的矩形，物理像素 (左, 上, 右, 下)
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) type Px = (i32, i32, i32, i32);

/// 窗口要改到的外框（物理像素）；比例见 `take_window_rect`
pub(crate) type WindowMove = ((i32, i32, i32, i32), Option<(f32, f32)>);

/// 无边框的小窗口为了菜单、对话框、图片信息临时变大着（都是物理像素）
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Grown {
    /// 变大之前的外框：变回去就是它
    pub home: (i32, i32, i32, i32),
    /// 图片那块在屏幕上的位置（变大之前的客户区），变大期间不动
    pub image: (i32, i32, i32, i32),
    /// 要变到的外框
    pub asked: (i32, i32, i32, i32),
    /// 已经在变回去
    pub shrinking: bool,
    /// 用不着了有几帧（关掉的那一帧还画着菜单，再等一帧才变回去）
    pub idle: u8,
    /// 等窗口变到位等了几帧（系统没按要的大小改时，过一会儿照样画）
    pub waited: u8,
}

/// 菜单 / 对话框 / 图片信息要多大地方
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Need {
    /// 放在图片中间的对话框（点）
    Centered(egui::Vec2),
    /// 右键菜单：右键按在哪（离图片左上角多远，点）
    Menu(egui::Vec2),
    /// 图片信息：面板贴着图片左边
    Info,
}

/// 确认框、改名框要的地方（点，含阴影）
pub(crate) const DIALOG_ROOM: egui::Vec2 = egui::vec2(460.0, 300.0);
/// 打印对话框：窗口小时它跟着缩小，最小 480 × 320，再小就要变大窗口
pub(crate) const PRINT_ROOM: egui::Vec2 = egui::vec2(1004.0, 704.0);
/// 设置（只在单独的设置窗口建不起来时画在图片窗口里）
pub(crate) const SETTINGS_ROOM: egui::Vec2 = egui::vec2(804.0, 484.0);
/// 图片信息面板：宽、放得下常见各行的高
#[cfg_attr(not(windows), allow(dead_code))]
const INFO_ROOM: egui::Vec2 = egui::vec2(300.0, 560.0);
/// 子菜单往旁边伸出去的宽度
#[cfg_attr(not(windows), allow(dead_code))]
const SUBMENU_W: f32 = 260.0;
/// 等窗口变到位最多等几帧
#[cfg_attr(not(windows), allow(dead_code))]
const GROW_PATIENCE: u8 = 10;

/// 客户区要盖住的地方（屏幕物理像素）；图片那块已经放得下时 None。image 是图片那块在屏幕上的位置，
/// menu 是右键菜单的大小（点，量过的）。都限在工作区里，再和图片那块合起来（图片不动）
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn grow_target(need: Need, image: (i32, i32, i32, i32), ppp: f32, work: (i32, i32, i32, i32), menu: egui::Vec2) -> Option<(i32, i32, i32, i32)> {
    let px = |v: f32| (v * ppp).round() as i32;
    let (iw, ih) = (image.2 - image.0, image.3 - image.1);
    let (cx, cy) = ((image.0 + image.2) / 2, (image.1 + image.3) / 2);
    let want = match need {
        Need::Centered(room) => {
            if iw >= px(room.x) && ih >= px(room.y) {
                return None;
            }
            let (w, h) = (px(room.x), px(room.y));
            (cx - w / 2, cy - h / 2, cx - w / 2 + w, cy - h / 2 + h)
        }
        Need::Menu(at) => {
            let (w, h, sub) = (px(menu.x + 16.0), px(menu.y + 16.0), px(SUBMENU_W));
            if iw >= w + sub && ih >= h {
                return None;
            }
            // 右键的地方往右（右边放不下就往左）留出菜单和子菜单，往下（下边放不下就往上）留出菜单
            let (x, y) = (image.0 + px(at.x), image.1 + px(at.y));
            let (l, r) = if x + w + sub <= work.2 { (x, x + w + sub) } else { (x - w - sub, x) };
            let (t, b) = if y + h <= work.3 { (y, y + h) } else { (y - h, y) };
            (l, t, r, b)
        }
        Need::Info => {
            let (w, h) = (px(INFO_ROOM.x), px(INFO_ROOM.y));
            if iw >= w + px(60.0) && ih >= px(320.0) {
                return None;
            }
            (image.0 - w, cy - h / 2, image.2, cy - h / 2 + h)
        }
    };
    Some(union(clamp_into(want, work), image))
}

/// 整个挪进工作区（比工作区大时贴着左上）
#[cfg_attr(not(windows), allow(dead_code))]
fn clamp_into(r: (i32, i32, i32, i32), work: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (w, h) = ((r.2 - r.0).min(work.2 - work.0), (r.3 - r.1).min(work.3 - work.1));
    let l = r.0.clamp(work.0, (work.2 - w).max(work.0));
    let t = r.1.clamp(work.1, (work.3 - h).max(work.1));
    (l, t, l + w, t + h)
}

/// a 盖住了 b（容 2 像素：有阴影时客户区上边少 1 像素）
#[cfg_attr(not(windows), allow(dead_code))]
fn contains(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    a.0 <= b.0 + 2 && a.1 <= b.1 + 2 && a.2 >= b.2 - 2 && a.3 >= b.3 - 2
}

#[cfg_attr(not(windows), allow(dead_code))]
fn union(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}

/// 图片那块在窗口里的位置（点）。窗口还没变大（客户区就是图片那块）时 None
#[cfg_attr(not(windows), allow(dead_code))]
fn box_in(client: (i32, i32, i32, i32), image: (i32, i32, i32, i32), ppp: f32) -> Option<Rect> {
    let same = |a: i32, b: i32| (a - b).abs() <= 2;
    if same(client.2 - client.0, image.2 - image.0) && same(client.3 - client.1, image.3 - image.1) {
        return None;
    }
    let p = |x: i32, y: i32| pos2(x as f32 / ppp, y as f32 / ppp);
    Some(Rect::from_min_max(p(image.0 - client.0, image.1 - client.1), p(image.2 - client.0, image.3 - client.1)))
}

/// 窗口从 old（客户区，旧画面就是这么大）改成 new 时，旧画面不拉伸、留在屏幕上原来的地方要的对齐比例：
/// 偏移 = 比例 × (new − old) 的大小差 = old 在 new 里的位置
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn hold_ratio(old: (i32, i32, i32, i32), new: (i32, i32, i32, i32)) -> (f32, f32) {
    let axis = |o0: i32, o1: i32, n0: i32, n1: i32| {
        let diff = (n1 - n0) - (o1 - o0);
        if diff == 0 { 0.0 } else { ((o0 - n0) as f32 / diff as f32).clamp(0.0, 1.0) }
    };
    (axis(old.0, old.2, new.0, new.2), axis(old.1, old.3, new.1, new.3))
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

/// 窗口能不能透出桌面（`d3d11::Painter::new_composited`：要在窗口第一次显示画面前就挂到系统合成上）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum SeeThrough {
    /// 不能：不是 Windows，或系统合成用不了
    #[default]
    No,
    /// 启动时不是无边框：重新打开后才能
    Restart,
    Yes,
}

impl App {
    /// 这一帧窗口是不是无边框（编辑时恢复标题栏）
    pub(crate) fn frameless(&self) -> bool {
        SUPPORTED && self.settings.window.borderless && self.editor.is_none()
    }

    /// 这一帧图片透明的地方该不该透出桌面：无边框、没有最大化 / 全屏、没开对话框，正在看的图真有透明的像素
    pub(crate) fn see_through_wanted(&self, ctx: &egui::Context) -> bool {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        self.frameless() && !self.fullscreen && !maximized && !self.modal_open() && self.shown.is_some_and(|s| s.translucent)
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
        self.fling = egui::Vec2::ZERO;
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
        if !self.frameless() || self.fullscreen || maximized || self.menu_open || self.menu_pending || self.modal_open() || self.info_open || self.grown.is_some() {
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
            self.window_rect = Some((hug_rect(cur, want, anchor, work), None));
        }
        // 拖边缘改大小时保持图片的宽高比；窗口已经被工作区限住（图片比屏幕大）时不锁
        let clamped = shown.x > work_size.x + 0.5 || shown.y > work_size.y + 0.5;
        win::set_resize_aspect(if clamped { 0.0 } else { size.x / size.y });
        true
    }

    /// 换到缩放不同的屏后想要的缩放 z（物理像素 / 图片像素）：最多 100%，图片不超过这块屏工作区的九成
    #[cfg(windows)]
    pub(crate) fn dpi_zoom_for(&self, z: f32, size: egui::Vec2, ppp: f32) -> f32 {
        let fit = self.hwnd.and_then(crate::platform::windows::work_area).map_or(1.0, |w| {
            let area = egui::vec2((w.2 - w.0) as f32, (w.3 - w.1) as f32) * HUG_SCREEN / ppp;
            crate::viewer::view::View::fit_zoom(crate::settings::FitMode::Fit, area, ppp, size).max(1e-3)
        });
        z.min(fit).min(1.0)
    }

    /// 这一帧要的窗口位置和大小（物理像素），由窗口循环在画完之后一次改好。
    /// 带比例的（窗口为了菜单、对话框临时变大 / 变回去）：上一帧不拉伸、按比例放（`Painter::hold_content`），等画面挂好了再改窗口
    #[cfg(windows)]
    pub(crate) fn take_window_rect(&mut self) -> Option<WindowMove> {
        self.window_rect.take()
    }

    /// 每帧开头：窗口为了菜单、对话框变大着时，图片那块在窗口里的位置（点）。窗口还没变大或已经变回去时 None
    #[cfg(windows)]
    pub(crate) fn sync_grown_box(&mut self, ctx: &egui::Context) {
        self.grown_box = None;
        let (Some(g), Some(hwnd)) = (self.grown, self.hwnd) else { return };
        let Some(client) = crate::platform::windows::client_rect(hwnd) else { return };
        match box_in(client, g.image, ctx.pixels_per_point()) {
            // 窗口被系统挪了（Win + 方向键等）：不再管它，下一帧起照常包着图片
            Some(r) if !ctx.content_rect().expand(2.0).contains_rect(r) => self.grown = None,
            r => self.grown_box = r,
        }
    }

    /// 这一帧的菜单 / 对话框 / 图片信息要多大地方
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn overlay_need(&self, ctx: &egui::Context) -> Option<Need> {
        if self.dialog.is_some() || self.rename.is_some() {
            return Some(Need::Centered(DIALOG_ROOM));
        }
        if self.printing() {
            return Some(Need::Centered(PRINT_ROOM));
        }
        if self.settings_modal() {
            return Some(Need::Centered(SETTINGS_ROOM));
        }
        if self.menu_open || self.menu_pending {
            return self.menu_at.map(Need::Menu);
        }
        // 面板滑出去的那 0.15 秒也算
        if self.info_open || ctx.animate_bool_with_time(Id::new("info-open"), self.info_open, 0.15) > 0.0 {
            return Some(Need::Info);
        }
        None
    }

    /// 这个菜单 / 对话框现在能画了吗：窗口不用变大，或已经变到位。还没到时这一帧先不画（不然会先被裁掉一下），等窗口变大
    pub(crate) fn overlay_ready(&self, ctx: &egui::Context, need: Need) -> bool {
        #[cfg(windows)]
        {
            let Some((client, target)) = self.grow_plan(ctx, need) else { return true };
            let Some(t) = target else { return true };
            match self.grown {
                Some(g) if g.shrinking => false,
                Some(g) => contains(client, t) || g.waited > GROW_PATIENCE,
                None => false,
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (ctx, need);
            true
        }
    }

    /// (现在的客户区, 要变到的大小)：不是无边框窗口等不用变大的情况 None；放得下时 target 是 None
    /// 窗口能为菜单、对话框临时变大：无边框，没有最大化、全屏
    #[cfg(windows)]
    fn grow_allowed(&self, ctx: &egui::Context) -> bool {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        self.frameless() && !self.fullscreen && !maximized
    }

    #[cfg(windows)]
    fn grow_plan(&self, ctx: &egui::Context, need: Need) -> Option<(Px, Option<Px>)> {
        use crate::platform::windows as win;
        if !self.grow_allowed(ctx) {
            return None;
        }
        let hwnd = self.hwnd?;
        let (client, work) = (win::client_rect(hwnd)?, win::work_area(hwnd)?);
        let image = self.grown.map_or(client, |g| g.image);
        Some((client, grow_target(need, image, ctx.pixels_per_point(), work, self.menu_size)))
    }

    /// 每帧最后：无边框窗口小、放不下这一帧的菜单 / 对话框 / 图片信息时，窗口透明地变大（图片留在屏幕上原来的地方），
    /// 关掉之后变回去。变大期间关掉 DWM 的阴影和圆角（`see_through`），不包着图片
    #[cfg(windows)]
    pub(crate) fn update_grow(&mut self, ctx: &egui::Context) {
        let Some(hwnd) = self.hwnd else { return };
        if !self.grow_allowed(ctx) {
            // 进了编辑：先回到原来的大小（在那里恢复标题栏）；全屏、最大化交给系统
            if let Some(g) = self.grown.take()
                && self.editor.is_some()
                && !g.shrinking
            {
                self.window_rect = Some((g.home, None));
            }
            return;
        }
        let Some(client) = crate::platform::windows::client_rect(hwnd) else { return };
        let target = self.overlay_need(ctx).and_then(|n| self.grow_plan(ctx, n)).and_then(|p| p.1);
        let can_see_through = self.see_through_avail == SeeThrough::Yes;
        match (self.grown.as_mut(), target) {
            (None, None) => {}
            (None, Some(t)) => {
                let Some(outer) = crate::platform::windows::window_rect(hwnd) else { return };
                log::debug!("窗口为菜单 / 对话框变大：图片 {client:?} → 窗口 {t:?}（原来 {outer:?}）");
                self.grown = Some(Grown { home: outer, image: client, asked: t, shrinking: false, idle: 0, waited: 0 });
                self.window_rect = Some((t, Some(hold_ratio(client, t))));
                // 这一帧就关掉阴影：窗口变大时外框和客户区一样大
                self.see_through |= can_see_through;
                ctx.request_repaint();
            }
            (Some(g), _) if g.shrinking => {
                let (iw, ih) = (g.image.2 - g.image.0, g.image.3 - g.image.1);
                g.waited = g.waited.saturating_add(1);
                if ((client.2 - client.0) - iw).abs() <= 2 && ((client.3 - client.1) - ih).abs() <= 2 || g.waited > GROW_PATIENCE * 4 {
                    self.grown = None;
                }
                ctx.request_repaint();
            }
            (Some(g), Some(t)) => {
                g.idle = 0;
                if contains(client, t) {
                    g.waited = 0;
                } else if contains(g.asked, t) {
                    g.waited = g.waited.saturating_add(1);
                    ctx.request_repaint();
                } else {
                    // 换了一个更大的（开着图片信息时又按了 F2）：在已经要的基础上再变大
                    let r = union(g.asked, t);
                    g.asked = r;
                    g.waited = 0;
                    self.window_rect = Some((r, Some(hold_ratio(client, r))));
                    ctx.request_repaint();
                }
            }
            (Some(g), None) => {
                // 关掉的那一帧还画着菜单 / 对话框：再等一帧，用一张干净的画面变回去
                g.idle = g.idle.saturating_add(1);
                if g.idle >= 2 {
                    log::debug!("窗口变回去：{:?}", g.home);
                    g.shrinking = true;
                    g.waited = 0;
                    self.window_rect = Some((g.home, Some(hold_ratio(client, g.home))));
                }
                ctx.request_repaint();
            }
        }
    }

    pub(crate) fn toggle_borderless(&mut self) {
        self.settings.window.borderless = !self.settings.window.borderless;
        if self.settings.window.borderless {
            self.show_toast(tr().borderless_tip);
        }
    }

    /// 拖图片时移动窗口（在按下后开始拖的那一帧决定，拖到一半不换）
    pub(crate) fn drag_moves_window(&self, ctx: &egui::Context, size: Option<egui::Vec2>) -> bool {
        // 窗口为了菜单、图片信息变大着：只平移，不挪窗口
        if !SUPPORTED || self.fullscreen || self.editor.is_some() || self.modal_open() || self.menu_open || self.grown.is_some() {
            return false;
        }
        let (maximized, shift) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.modifiers.shift));
        let borderless = self.frameless();
        // 普通窗口最大化后不从画布拖出还原；无边框保留原有行为。
        if maximized && !borderless {
            return false;
        }
        // 普通窗口只在有图片时启用；无边框空窗口仍能拖动。
        let Some(size) = size else { return borderless };
        let shown = size * self.view.zoom / ctx.pixels_per_point();
        let pannable = shown.x > self.canvas.width() + 0.5 || shown.y > self.canvas.height() + 0.5;
        shift || !pannable
    }

    /// 无边框、没有最大化 / 全屏时：鼠标在边缘上显示改大小的箭头，按下就交给系统改大小
    pub(crate) fn resize_edges(&mut self, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if !self.frameless() || self.fullscreen || maximized || self.modal_open() || self.grown.is_some() {
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
        // 开着对话框、右键菜单时不出来（和底部工具栏一样）；窗口为它们变大着时顶边离图片很远，也不出来
        let blocked = self.modal_open() || self.menu_open || self.grown.is_some();
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
    #[cfg(windows)]
    use crate::settings::Settings;
    #[cfg(windows)]
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

    #[cfg(windows)]
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
    fn a_small_window_grows_around_the_image_for_dialogs_menus_and_the_info_panel() {
        let work = (3840, 0, 6400, 1560);
        let image = (4500, 600, 4700, 725); // 200 × 125 像素（125% 时 160 × 100 点）
        let ppp = 1.25;
        // 确认框：以图片为中心，图片还在里面
        let r = grow_target(Need::Centered(DIALOG_ROOM), image, ppp, work, egui::Vec2::ZERO).unwrap();
        assert_eq!((r.2 - r.0, r.3 - r.1), (575, 375));
        assert_eq!(((r.0 + r.2) / 2, (r.1 + r.3) / 2), (4600, 662));
        assert!(contains(r, image));
        // 已经放得下：不变
        assert_eq!(grow_target(Need::Centered(DIALOG_ROOM), (4000, 100, 5000, 900), ppp, work, egui::Vec2::ZERO), None);
        // 右键菜单：从右键的地方往右下留出菜单和子菜单
        let menu = egui::vec2(280.0, 560.0);
        let r = grow_target(Need::Menu(egui::vec2(40.0, 30.0)), image, ppp, work, menu).unwrap();
        assert_eq!((r.0, r.1), (4500, 600));
        assert_eq!((r.2, r.3), (4550 + 370 + 325, 638 + 720));
        // 靠着屏幕下边：往上留
        let low = (4500, 1400, 4700, 1525);
        let r = grow_target(Need::Menu(egui::vec2(40.0, 30.0)), low, ppp, work, menu).unwrap();
        assert!(r.1 < low.1 && r.3 == low.3, "{r:?}");
        // 图片信息：面板贴着图片左边；图片靠着屏幕左边时挪进屏幕、盖住图片一点
        let r = grow_target(Need::Info, image, ppp, work, menu).unwrap();
        assert_eq!((r.0, r.2), (4500 - 375, 4700));
        let left = (3850, 600, 4050, 725);
        let r = grow_target(Need::Info, left, ppp, work, menu).unwrap();
        assert_eq!(r.0, 3840);
        assert!(contains(r, left));
    }

    #[test]
    fn the_old_frame_stays_where_it_was_while_the_window_grows_and_shrinks() {
        // 变大：旧画面（图片那块）在新窗口里的偏移 = 比例 × 大小差
        let (old, new) = ((4500, 600, 4700, 725), (4400, 500, 4975, 875));
        let (rx, ry) = hold_ratio(old, new);
        assert!((rx * 375.0 - 100.0).abs() < 1e-3 && (ry * 250.0 - 100.0).abs() < 1e-3, "{rx} {ry}");
        // 变回去：大窗口的画面里，图片那块落在新窗口上
        let (rx, ry) = hold_ratio(new, old);
        assert!((rx * -375.0 + 100.0).abs() < 1e-3 && (ry * -250.0 + 100.0).abs() < 1e-3, "{rx} {ry}");
        // 大小没变
        assert_eq!(hold_ratio(old, old), (0.0, 0.0));
        // 图片那块在变大的窗口里的位置（点）
        let b = box_in(new, old, 1.25).unwrap();
        assert_eq!((b.min, b.max), (pos2(80.0, 80.0), pos2(240.0, 180.0)));
        assert_eq!(box_in(old, old, 1.25), None);
        assert_eq!(clamp_into((6300, 1500, 6500, 1700), (3840, 0, 6400, 1560)), (6200, 1360, 6400, 1560));
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
