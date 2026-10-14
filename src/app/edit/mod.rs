//! 编辑模式（PRD 6.7、6.8、7.4）：在同一个窗口里切换，保持缩放与位置；顶部是编辑工具栏与属性栏，底部是状态栏。
//! `session` 进入 / 退出，`ops` 撤销、选区、裁剪等操作，`keys` 快捷键，`canvas` 画布与叠加层，
//! `pointer` 鼠标，`bars` 工具栏，`props` 属性栏与状态栏，`texture` 显示，`save` 保存，`dialogs` 对话框。

mod bars;
mod canvas;
mod colors;
mod dialogs;
mod keys;
mod ops;
mod paint;
mod pointer;
mod props;
mod save;
mod session;
mod text;
mod texture;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

use crate::editor::IRect;
use crate::editor::history::{Doc, Saved};
use crate::editor::stroke::Stroke;
use crate::settings::Settings;

pub(crate) use save::{SaveDone, SaveThen};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tool {
    Select,
    Pencil,
    Brush,
    Eraser,
    Fill,
    Picker,
    Text,
    Shape,
    Redact,
}

/// 浮动选区：粘贴进来的，或从图上拿起来移动的
pub(crate) struct Floating {
    pub image: Arc<RgbaImage>,
    /// 在图上的位置（图片像素，可以超出图片）
    pub rect: egui::Rect,
    /// 拿起时清空原处之前的块（粘贴的为空）；放下时与贴上去的一起记成一步
    pub saved: Saved,
    pub tex: Option<egui::TextureHandle>,
}

pub(crate) enum Drag {
    /// 拖出新选区：起点（图片像素）
    Select(egui::Pos2),
    /// 移动浮动选区：按下时的鼠标位置与选区位置
    Move {
        grab: egui::Pos2,
        from: egui::Rect,
    },
    /// 拉控制点缩放（0 ~ 7 从左上角起顺时针）
    Handle {
        index: usize,
        from: egui::Rect,
    },
    Pan,
    /// 涂抹：画笔打码、铅笔、刷子、橡皮擦
    Stroke(Stroke),
    /// 框选打码：起点与这一框
    RedactRect(egui::Pos2, Stroke),
    /// 画形状：起点
    Shape(egui::Pos2),
    /// 调整还没确认的形状：handle = 拉哪个控制点（None = 移动）；按下时的鼠标位置与形状
    ShapeEdit {
        handle: Option<usize>,
        grab: egui::Pos2,
        a: egui::Pos2,
        b: egui::Pos2,
    },
    /// 拖动文本框
    TextMove {
        grab: egui::Pos2,
        from: egui::Pos2,
    },
    /// 拉宽文本框
    TextWidth {
        from: f32,
        grab: f32,
    },
}

/// 编辑的图来自哪里
pub(crate) struct Source {
    pub path: PathBuf,
    pub clipboard: bool,
    pub format: &'static str,
    pub file_size: u64,
    pub jpeg_quality: Option<u8>,
    pub exif: Option<Arc<Vec<u8>>>,
    pub icc: Option<Arc<Vec<u8>>>,
    /// 原图有透明通道：删除选区、拿起选区后原处变透明（否则填辅色）
    pub has_alpha: bool,
}

pub(crate) struct Editor {
    pub doc: Doc,
    pub tex: texture::EditTextures,
    pub tool: Tool,
    /// 取色后回到的工具
    pub prev_tool: Tool,
    pub sel: Option<IRect>,
    pub floating: Option<Floating>,
    pub drag: Option<Drag>,
    pub src: Source,
    /// 光标所在的图片像素
    pub cursor: Option<egui::Pos2>,
    /// 结果已经复制到剪贴板时的版本号（剪贴板来的图退出时不再询问）
    pub copied_rev: Option<u64>,
    pub saving: bool,
    /// 保存过（退出时不再恢复进入前的旋转）
    pub saved_once: bool,
    /// 这次编辑已经确认过覆盖
    pub overwrite_ok: bool,
    /// 调整圆圈大小时在画布中央显示一会儿
    pub size_hint: Option<Instant>,
    /// 进入编辑前的显示变换（不保存就退出时恢复）
    pub xf_before: crate::viewer::view::Transform,
    /// 图的尺寸（变了且处于「适应窗口」时重新适应）
    pub dims: (u32, u32),
    /// 编辑结束：退出后做的事
    pub leave: bool,
    pub after: Option<super::After>,
    pub resize: Option<dialogs::ResizeState>,
    pub save_options: Option<save::SaveJob>,
    pub shape: Option<paint::PendingShape>,
    pub text: Option<paint::PendingText>,
    /// 这一帧开始时有菜单开着（点外面关菜单的那一下不在画布上画）
    pub popup_was_open: bool,
}

impl Editor {
    pub fn new(doc: Doc, src: Source, tool: Tool, xf_before: crate::viewer::view::Transform) -> Self {
        let dims = (doc.width(), doc.height());
        Self {
            doc,
            tex: texture::EditTextures::default(),
            tool,
            prev_tool: tool,
            sel: None,
            floating: None,
            drag: None,
            src,
            cursor: None,
            copied_rev: None,
            saving: false,
            saved_once: false,
            overwrite_ok: false,
            size_hint: None,
            xf_before,
            dims,
            leave: false,
            after: None,
            resize: None,
            save_options: None,
            shape: None,
            text: None,
            popup_was_open: false,
        }
    }

    /// 有没保存的修改（浮动的粘贴也算）
    pub fn modified(&self) -> bool {
        self.doc.modified() || self.floating.is_some()
    }

    /// 删除选区、拿起选区后原处的颜色
    pub fn clear_color(&self, s: &Settings) -> [u8; 4] {
        if self.src.has_alpha { [0, 0, 0, 0] } else { s.color2 }
    }

    /// 打码的方块、模糊程度、圆圈直径：用户改过的值，否则按图片大小算
    pub fn redact_params(&self, s: &Settings) -> (u32, u32, u32) {
        let (b, r, d) = crate::editor::stroke::redact_defaults(self.doc.width(), self.doc.height());
        (s.mosaic_block.unwrap_or(b), s.blur_radius.unwrap_or(r), s.brush_size.unwrap_or(d))
    }
}
