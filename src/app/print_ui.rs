//! 打印对话框（Windows）：左边纸张预览，右边设置。打印机、纸张、可打印范围都在后台读（网络打印机可能很慢），
//! 驱动的「打印机属性」在主线程弹出；点「打印」才提交，由后台线程输出。几何见 `print::layout`，方案见 docs/PRINTING-PLAN.md。
//! 开发脚本可设 `LITEVIEW_PRINT_OUTPUT`：打印到文件的打印机（Microsoft Print to PDF 等）直接写到这里，不弹对话框。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, pos2, vec2};
use image::RgbaImage;

use super::settings_ui::choose;
use super::{App, Tex};
use crate::i18n::{fill, tr};
use crate::platform::print::{self as backend, Caps, DevMode, Outcome, Paper, Printer};
use crate::print::layout::{self, Device, Placement, Size};
use crate::settings::{PrintOrientation, PrintSettings, PrintSize};
use crate::ui::{theme, widgets};
use crate::viewer::view::Transform;

/// 后台做的一件事（线程 panic 也算失败，不会一直等）
enum Bg<T> {
    Idle,
    Busy(Receiver<Result<T, String>>),
    Done(Result<T, String>),
}

impl<T: Send + 'static> Bg<T> {
    fn spawn(ctx: &egui::Context, f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Self {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("print".into()).spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => Self::Busy(rx),
            Err(e) => Self::Done(Err(e.to_string())),
        }
    }

    fn poll(&mut self) {
        let next = match self {
            Self::Busy(rx) => match rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(tr().err_corrupt.to_owned())),
            },
            _ => None,
        };
        if let Some(r) = next {
            *self = Self::Done(r);
        }
    }
}

impl<T> Bg<T> {
    fn ok(&self) -> Option<&T> {
        match self {
            Self::Done(Ok(v)) => Some(v),
            _ => None,
        }
    }

    fn err(&self) -> Option<&str> {
        match self {
            Self::Done(Err(e)) => Some(e),
            _ => None,
        }
    }

    fn busy(&self) -> bool {
        matches!(self, Self::Busy(_))
    }

    fn idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// 打印机接口的错误给用户看
fn describe(e: &windows::core::Error) -> String {
    if backend::spooler_stopped(e) {
        return tr().pr_spooler.to_owned();
    }
    let m = e.message();
    if m.trim().is_empty() { format!("0x{:08X}", e.code().0) } else { m.trim().to_owned() }
}

/// 毫米：整数不带小数
fn mm(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 { format!("{}", v.round()) } else { format!("{v:.1}") }
}

/// 只限制用户输入的值：按比例算出来的另一边（极宽 / 极高的图可能不到 1 mm）照原样显示，不被改写
fn mm_field(v: &mut f32) -> egui::DragValue<'_> {
    egui::DragValue::new(v).range(1.0..=5000.0).clamp_existing_to_range(false).speed(0.5).fixed_decimals(1).suffix(" mm")
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::palette(ui.ctx()).text_weak).size(12.0));
}

/// 打印什么
enum Source {
    /// 查看：当前图（index = 动图的帧）、显示方向
    View { index: usize },
    /// 编辑：编辑画布（方向已经在像素里）
    Edit,
}

/// 这一帧之后对话框怎么样
pub(crate) enum Next {
    Stay,
    /// 关掉；带一句轻提示
    Close(Option<String>),
}

/// 这一帧点了什么
#[derive(Default)]
struct Act {
    print: bool,
    cancel: bool,
    close: bool,
    properties: bool,
}

pub(crate) struct PrintDialog {
    source: Source,
    /// 查看时打开对话框那一刻的图（换了图就关掉，免得打错）
    path: Option<PathBuf>,
    /// 显示方向的像素尺寸（排版用）
    px: [u32; 2],
    xf: Transform,
    /// 打印队列里的名字
    title: String,
    /// 动图：打印第几帧（从 1 开始）
    frame: Option<usize>,
    /// 要打印的像素（原图 / 当前帧 / 编辑画布）
    pixels: Bg<Arc<RgbaImage>>,
    dpi: Bg<Option<[f32; 2]>>,
    preview_job: Bg<egui::ColorImage>,
    preview: Option<egui::TextureHandle>,
    printers: Bg<Vec<Printer>>,
    printer: Option<String>,
    caps: HashMap<String, Bg<Caps>>,
    /// 当前打印机 / 纸张 / 方向下的几何（驱动核对过的设置一起）
    geo: Bg<(Device, DevMode)>,
    geo_key: Option<(String, i16, bool, u32)>,
    /// 「打印机属性」改过一次加一（几何要重读）
    rev: u32,
    /// 本次的打印设置（关掉时存回设置）
    s: PrintSettings,
    copies: u32,
    job: Option<(Bg<Outcome>, Arc<AtomicBool>)>,
    /// 点了打印、原图还在解：等它（打印到哪个文件）
    queued: Option<Option<PathBuf>>,
    /// 上一次的结果（取消、失败）
    note: Option<(String, bool)>,
    /// 上一帧结束时有输入框在输入
    typing: bool,
}

impl App {
    /// 打印当前图（右键菜单、Ctrl+P）
    pub(crate) fn open_print(&mut self) {
        let (Some(cur), Some(shown)) = (self.cur.as_ref(), self.shown) else { return };
        if !self.has_image() || self.print.is_some() {
            return;
        }
        let path = (!cur.clipboard).then(|| cur.path.clone());
        let index = self.play.as_ref().map_or(0, |p| p.index);
        let frame = self.is_animated().then_some(index + 1);
        if let Some(p) = &mut self.play {
            p.playing = false;
        }
        let size = self.view.shown_size(shown.width, shown.height);
        let px = [size.x as u32, size.y as u32];
        let d = self.new_print(Source::View { index }, path, px, self.view.xf, frame, Bg::Idle);
        self.print = Some(Box::new(d));
    }

    /// 编辑中 Ctrl+P：先定下进行中的笔画、选区、形状、文字，打印当前合成结果
    pub(crate) fn open_print_edit(&mut self, ed: &mut super::edit::Editor) {
        if self.print.is_some() {
            return;
        }
        self.edit_finalize(ed);
        let img = ed.doc.image.clone();
        let px = [img.width(), img.height()];
        let path = self.cur.as_ref().filter(|c| !c.clipboard).map(|c| c.path.clone());
        let d = self.new_print(Source::Edit, path, px, Transform::default(), None, Bg::Done(Ok(img)));
        self.print = Some(Box::new(d));
    }

    fn new_print(&self, source: Source, path: Option<PathBuf>, px: [u32; 2], xf: Transform, frame: Option<usize>, pixels: Bg<Arc<RgbaImage>>) -> PrintDialog {
        let ctx = &self.ctx;
        let title = path.as_deref().map_or_else(|| tr().clipboard_image.to_owned(), crate::natsort::file_name);
        let dpi = match path.clone() {
            Some(p) => Bg::spawn(ctx, move || Ok(crate::print::dpi::read(&p))),
            None => Bg::Done(Ok(None)),
        };
        let printers = Bg::spawn(ctx, || backend::printers().map_err(|e| describe(&e)));
        let path = if matches!(source, Source::View { .. }) { self.cur.as_ref().map(|c| c.path.clone()) } else { path };
        PrintDialog {
            source,
            path,
            px,
            xf,
            title,
            frame,
            pixels,
            dpi,
            preview_job: Bg::Idle,
            preview: None,
            printers,
            printer: None,
            caps: HashMap::new(),
            geo: Bg::Idle,
            geo_key: None,
            rev: 0,
            s: self.settings.print.clone(),
            copies: 1,
            job: None,
            queued: None,
            note: None,
            typing: false,
        }
    }

    /// 每帧：对话框开着时画它；查看时要原图像素就解
    pub(crate) fn print_ui(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.print.take() else { return };
        if let Source::View { index } = d.source {
            // 打开后换了图（文件夹监听、单实例转来的图）：关掉，免得打错
            if self.cur.as_ref().map(|c| &c.path) != d.path.as_ref() && d.pixels.idle() {
                return;
            }
            if d.pixels.idle() {
                match (self.decoded().cloned(), self.cur.as_ref().and_then(|c| c.loaded.as_ref()).is_some_and(|l| l.is_err())) {
                    (Some(decoded), _) => d.pixels = Bg::spawn(ctx, move || decoded.content.image_at(index).map_err(|_| tr().err_corrupt.to_owned())),
                    (None, true) => d.pixels = Bg::Done(Err(tr().err_corrupt.to_owned())),
                    (None, false) => self.want_full(),
                }
            }
        }
        let tex = &self.tex;
        let fallback = |painter: &egui::Painter, rect: Rect, xf: Transform| match tex {
            Tex::Still(t) => t.paint_resident(painter, rect, xf),
            Tex::Anim { tex, .. } => tex.paint_resident(painter, rect, xf),
            Tex::None => {}
        };
        match d.show(ctx, &mut self.print_devmodes, self.hwnd, &fallback) {
            Next::Stay => self.print = Some(d),
            Next::Close(toast) => {
                if self.settings.print != d.s {
                    self.settings.print = d.s.clone();
                    self.settings.save();
                }
                if let Some(t) = toast {
                    self.show_toast(t);
                }
            }
        }
    }
}

type Fallback<'a> = &'a dyn Fn(&egui::Painter, Rect, Transform);

impl PrintDialog {
    fn poll(&mut self, ctx: &egui::Context) {
        self.pixels.poll();
        self.dpi.poll();
        self.printers.poll();
        self.caps.values_mut().for_each(Bg::poll);
        self.geo.poll();
        // 预览：长边约 1024 像素，与看图同样缩小；方向在画的时候按 xf 转
        if self.preview_job.idle()
            && let Some(img) = self.pixels.ok()
        {
            let img = img.clone();
            self.preview_job = Bg::spawn(ctx, move || {
                let s = (1024.0 / img.width().max(img.height()) as f32).min(1.0);
                let (w, h) = (((img.width() as f32 * s).round() as u32).max(1), ((img.height() as f32 * s).round() as u32).max(1));
                let px = crate::editor::resample::resize(&img, w, h, crate::editor::resample::Filter::CatmullRom, true);
                Ok(egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &px))
            });
        }
        self.preview_job.poll();
        if self.preview.is_none() && self.preview_job.ok().is_some() {
            // 交给显卡后不再留着（留一个空的「完成」，不再重算）
            if let Bg::Done(Ok(img)) = std::mem::replace(&mut self.preview_job, Bg::Done(Err(String::new()))) {
                self.preview = Some(ctx.load_texture("print-preview", img, egui::TextureOptions::LINEAR));
            }
        }
        // 原图解不出来：排着的打印取消
        if self.queued.is_some()
            && let Some(e) = self.pixels.err()
        {
            self.note = Some((fill(tr().pr_failed, &[&e]), true));
            self.queued = None;
        }
    }

    /// 打印任务的结果：交出去了就关掉对话框
    fn poll_job(&mut self) -> Option<Next> {
        let (job, _) = self.job.as_mut()?;
        job.poll();
        let r = match job {
            Bg::Done(r) => r.clone(),
            _ => return None,
        };
        self.job = None;
        let printer = self.printer.as_deref().unwrap_or_default();
        match r {
            Ok(Outcome::Sent) => {
                log::info!("打印：已交给 {printer}（{} 份）", self.copies);
                return Some(Next::Close(Some(tr().pr_sent.to_owned())));
            }
            Ok(Outcome::Cancelled) => {
                log::info!("打印：已取消（{printer}）");
                self.note = Some((tr().pr_cancelled.to_owned(), false));
            }
            Err(e) => {
                log::warn!("打印失败（{printer}）：{e}");
                self.note = Some((fill(tr().pr_failed, &[&e]), true));
            }
        }
        None
    }

    /// 画对话框、处理操作。devmodes：本次运行里「打印机属性」改过的设置（按打印机）
    fn show(&mut self, ctx: &egui::Context, devmodes: &mut HashMap<String, DevMode>, owner: Option<isize>, fallback: Fallback) -> Next {
        // 输入框里正在输入：回车只是确认这个数、Esc 只是退出输入。回车时输入框在本帧画的时候才放开焦点，
        // Esc 时 egui 在帧开始就清掉焦点，所以还要看上一帧结束时的状态
        let typing = self.typing || ctx.egui_wants_keyboard_input();
        self.poll(ctx);
        if let Some(next) = self.poll_job() {
            return next;
        }
        // 打印机：上次用的，否则默认打印机
        if self.printer.is_none()
            && let Some(list) = self.printers.ok()
        {
            let want = list.iter().find(|p| p.name == self.s.printer).or_else(|| list.iter().find(|p| p.default)).or(list.first());
            self.printer = want.map(|p| p.name.clone());
        }
        let printer = self.printer.clone();
        if let Some(name) = &printer
            && !self.caps.contains_key(name)
        {
            let n = name.clone();
            self.caps.insert(name.clone(), Bg::spawn(ctx, move || backend::caps(&n).map_err(|e| describe(&e))));
        }
        let caps = printer.as_ref().and_then(|n| self.caps.get(n)).and_then(Bg::ok);
        let papers: Vec<Paper> = caps.map(|c| c.papers.clone()).unwrap_or_default();
        // 纸张：上次用的（这台打印机有的话），否则打印机默认的
        let want = if self.s.paper != 0 { self.s.paper } else { caps.map_or(0, |c| c.defaults.paper()) };
        let paper = papers.iter().find(|p| p.id == want).or_else(|| papers.iter().find(|p| Some(p.id) == caps.map(|c| c.defaults.paper()))).or(papers.first()).cloned();
        let landscape = layout::landscape(orientation(self.s.orientation), self.px);
        let (max_copies, to_file) = caps.map_or((1, false), |c| (c.max_copies, c.to_file));
        // 几何：打印机 / 纸张 / 方向变了才重读
        if let (Some(name), Some(c)) = (&printer, caps) {
            let key = (name.clone(), paper.as_ref().map_or(0, |p| p.id), landscape, self.rev);
            if self.geo_key.as_ref() != Some(&key) {
                let mut dm = devmodes.get(name).unwrap_or(&c.defaults).clone();
                dm.set(key.1, landscape, 1);
                let n = name.clone();
                self.geo = Bg::spawn(ctx, move || backend::device(&n, &dm).map_err(|e| describe(&e)));
                self.geo_key = Some(key);
            }
        }
        let have_caps = caps.is_some();
        let paper_mm = paper.as_ref().map_or([210.0, 297.0], |p| p.mm);
        let dev = match self.geo.ok() {
            Some((d, _)) if !self.geo.busy() => *d,
            _ if landscape => Device::nominal(vec2(paper_mm[1], paper_mm[0])),
            _ => Device::nominal(vec2(paper_mm[0], paper_mm[1])),
        };
        let fit = layout::place(&dev, self.s.margin_mm, self.px, Size::Fit);
        // 上次存的是指定尺寸但没有长边：从适合页面的大小开始
        if self.s.size == PrintSize::Custom && self.s.long_mm <= 0.0 {
            self.s.long_mm = fit.map_or(100.0, |f| f.rect.width().max(f.rect.height()));
        }
        let dpi = self.dpi.ok().copied().flatten();
        let size = match (self.s.size, dpi) {
            (PrintSize::Custom, _) => Size::LongSide(self.s.long_mm),
            (PrintSize::Actual, Some(d)) => Size::Dpi(d),
            _ => Size::Fit,
        };
        let placement = layout::place(&dev, self.s.margin_mm, self.px, size);
        let busy = self.queued.is_some() || self.job.is_some();
        let geo_ready = matches!(&self.geo, Bg::Done(Ok(_))) && self.geo_key.is_some();
        let can_print = have_caps && geo_ready && placement.is_some() && !busy && self.pixels.err().is_none();
        let (status, error) = self.status(placement.is_some()).unwrap_or_default();
        let mut act = Act::default();
        let screen = ctx.content_rect().size();
        let (w, h) = ((screen.x - 24.0).clamp(480.0, 980.0), (screen.y - 24.0).clamp(320.0, 680.0));
        let side = if w < 640.0 { 230.0 } else { 280.0 };
        let p = theme::palette(ctx);
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(16)).corner_radius(egui::CornerRadius::same(8));
        let printers = self.printers.ok().cloned().unwrap_or_default();
        let fit_long = fit.map_or(100.0, |f| f.rect.width().max(f.rect.height()));
        let hardware_margins = dev.offset_px != [0, 0] || dev.printable_px != dev.paper_px;
        let ctl = Ctl { printers: &printers, papers: &papers, paper: paper.as_ref(), dpi, placement, have_caps, fit_long, hardware_margins };
        let modal = egui::Modal::new(egui::Id::new("print")).frame(frame).show(ctx, |ui| {
            ui.set_width(w - 32.0);
            ui.set_height(h - 32.0);
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(w - 32.0 - side - 16.0, h - 32.0), egui::Sense::hover());
                self.paint_preview(ui, rect, &dev, placement, paper.as_ref(), fallback);
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width(side);
                    ui.label(RichText::new(tr().pr_title).size(18.0).color(p.text));
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical().max_height(h - 32.0 - 30.0 - 60.0).show(ui, |ui| {
                        if ui.add_enabled_ui(!busy, |ui| self.controls(ui, &ctl)).inner {
                            act.properties = true;
                        }
                    });
                    ui.add_space(6.0);
                    if !status.is_empty() {
                        ui.label(RichText::new(&status).size(12.0).color(if error { p.danger } else { p.text_weak }));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                        if ui.add_enabled_ui(can_print, |ui| widgets::accent_button(ui, tr().pr_print, false)).inner.clicked() {
                            act.print = true;
                        }
                        if widgets::plain_button(ui, tr().cancel).clicked() {
                            act.cancel = busy;
                            act.close = !busy;
                        }
                    });
                });
            });
        });
        self.typing = ctx.egui_wants_keyboard_input();
        if can_print && !typing && !self.typing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            act.print = true;
        }
        if modal.should_close() && !ctx.any_popup_open() && !typing {
            act.cancel |= busy;
            act.close |= !busy;
        }
        if act.close {
            return Next::Close(None);
        }
        if act.cancel {
            if let Some((_, flag)) = &self.job {
                flag.store(true, Ordering::Relaxed);
            } else if self.queued.take().is_some() {
                self.note = Some((tr().pr_cancelled.to_owned(), false));
            }
        }
        if act.properties {
            self.properties(owner, devmodes, paper.as_ref(), landscape);
        }
        if act.print && can_print {
            self.note = None;
            match self.output(to_file) {
                Some(out) => self.queued = Some(out),
                None => self.note = Some((tr().pr_cancelled.to_owned(), false)),
            }
        }
        if self.queued.is_some() && self.pixels.ok().is_some() {
            self.start_job(ctx, placement, max_copies);
        }
        Next::Stay
    }

    /// 驱动的「打印机属性」（主线程，模态）；改了纸张 / 方向就跟着改
    fn properties(&mut self, owner: Option<isize>, devmodes: &mut HashMap<String, DevMode>, paper: Option<&Paper>, landscape: bool) {
        let Some(name) = self.printer.clone() else { return };
        let Some(c) = self.caps.get(&name).and_then(Bg::ok) else { return };
        let mut dm = devmodes.get(&name).unwrap_or(&c.defaults).clone();
        let id = paper.map_or(0, |p| p.id);
        dm.set(id, landscape, 1);
        let Some(out) = backend::properties(owner, &name, &dm) else { return };
        if out.paper() != id && out.paper() != 0 {
            self.s.paper = out.paper();
        }
        if out.landscape() != landscape {
            self.s.orientation = if out.landscape() { PrintOrientation::Landscape } else { PrintOrientation::Portrait };
        }
        devmodes.insert(name, out);
        self.rev += 1;
    }

    /// 打印到文件的打印机先问存到哪里：Some(None) = 不是打印到文件，None = 取消了
    fn output(&self, to_file: bool) -> Option<Option<PathBuf>> {
        if !to_file {
            return Some(None);
        }
        if let Some(p) = std::env::var_os("LITEVIEW_PRINT_OUTPUT") {
            return Some(Some(PathBuf::from(p)));
        }
        let lower = self.printer.as_deref().unwrap_or_default().to_ascii_lowercase();
        let ext = if lower.contains("xps") { "oxps" } else if lower.contains("pdf") { "pdf" } else { "prn" };
        let stem = Path::new(&self.title).file_stem().map_or_else(|| "print".to_owned(), |s| s.to_string_lossy().into_owned());
        let mut d = rfd::FileDialog::new().set_file_name(format!("{stem}.{ext}")).add_filter(tr().pr_file_filter, &[ext]);
        if let Some(dir) = self.path.as_deref().and_then(Path::parent) {
            d = d.set_directory(dir);
        }
        d.save_file().map(Some)
    }

    /// 原图像素到了：交给后台线程打印
    fn start_job(&mut self, ctx: &egui::Context, placement: Option<Placement>, max_copies: u32) {
        let (Some(out), Some(placement), Some(name), Some((_, dm)), Some(image)) = (self.queued.clone(), placement, self.printer.clone(), self.geo.ok(), self.pixels.ok()) else {
            return;
        };
        let job = backend::Job {
            printer: name,
            devmode: dm.clone(),
            title: self.title.clone(),
            output: out,
            copies: self.copies,
            max_copies,
            image: image.clone(),
            transform: self.xf,
            placement,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        self.job = Some((Bg::spawn(ctx, move || backend::run(job, &flag).map_err(|e| describe(&e))), cancel));
        self.queued = None;
    }

    /// 状态行（文字、是不是错误）
    fn status(&self, placed: bool) -> Option<(String, bool)> {
        let t = tr();
        let caps = self.printer.as_ref().and_then(|n| self.caps.get(n));
        if self.queued.is_some() {
            return Some((t.pr_preparing.to_owned(), false));
        }
        if self.job.is_some() {
            return Some((t.pr_sending.to_owned(), false));
        }
        if self.printers.busy() {
            return Some((t.pr_loading.to_owned(), false));
        }
        if let Some(e) = self.printers.err() {
            return Some((e.to_owned(), true));
        }
        if self.printers.ok().is_some_and(|l| l.is_empty()) {
            return Some((t.pr_none.to_owned(), true));
        }
        if let Some(e) = caps.and_then(Bg::err).or(self.geo.err()) {
            return Some((fill(t.pr_failed, &[&e]), true));
        }
        if caps.is_some_and(Bg::busy) || self.geo.busy() {
            return Some((t.pr_reading.to_owned(), false));
        }
        if let Some(e) = self.pixels.err() {
            return Some((fill(t.pr_failed, &[&e]), true));
        }
        if !placed {
            return Some((t.pr_bad_margin.to_owned(), true));
        }
        self.note.clone()
    }

    /// 纸张预览：白纸、灰色的印不到的边、图片（超出实际打印范围的部分不画，外框标红）
    fn paint_preview(&self, ui: &Ui, rect: Rect, dev: &Device, placement: Option<Placement>, paper: Option<&Paper>, fallback: Fallback) {
        let p = theme::palette(ui.ctx());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, egui::CornerRadius::same(6), p.canvas);
        let size = dev.paper_mm();
        let room = rect.shrink(16.0);
        let label_h = 22.0;
        let s = (room.width() / size.x).min((room.height() - label_h) / size.y).max(0.01);
        let page = Rect::from_center_size(pos2(room.center().x, room.center().y - label_h / 2.0), size * s);
        let map = |r: Rect| Rect::from_min_max(page.min + r.min.to_vec2() * s, page.min + r.max.to_vec2() * s);
        painter.rect_filled(page.translate(vec2(2.0, 3.0)), egui::CornerRadius::ZERO, Color32::from_black_alpha(40));
        painter.rect_filled(page, egui::CornerRadius::ZERO, Color32::WHITE);
        // 印不到的边
        let printable = map(dev.printable_mm()).intersect(page);
        let grey = Color32::from_gray(222);
        for band in [
            Rect::from_min_max(page.min, pos2(page.max.x, printable.min.y)),
            Rect::from_min_max(pos2(page.min.x, printable.max.y), page.max),
            Rect::from_min_max(pos2(page.min.x, printable.min.y), pos2(printable.min.x, printable.max.y)),
            Rect::from_min_max(pos2(printable.max.x, printable.min.y), pos2(page.max.x, printable.max.y)),
        ] {
            if band.width() > 0.0 && band.height() > 0.0 {
                painter.rect_filled(band, egui::CornerRadius::ZERO, grey);
            }
        }
        if let Some(pl) = placement {
            let img = map(pl.rect);
            let clipped = painter.with_clip_rect(map(pl.area).intersect(page));
            match (&self.preview, &self.source) {
                (Some(t), _) => crate::viewer::texture::quad(&clipped, t.id(), img, self.xf, [0.0, 0.0, 1.0, 1.0], Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))),
                (None, Source::View { .. }) => fallback(&clipped, img, self.xf),
                (None, Source::Edit) => {}
            }
            if pl.overflows() {
                painter.rect_stroke(img, egui::CornerRadius::ZERO, Stroke::new(1.5, p.danger), StrokeKind::Outside);
            }
            if self.s.margin_mm > 0.0 {
                let a = map(pl.area);
                let pts = [a.left_top(), a.right_top(), a.right_bottom(), a.left_bottom(), a.left_top()];
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, Color32::from_gray(150)), 4.0, 3.0));
            }
        }
        let name = paper.map_or_else(String::new, |p| format!("{} · ", p.name));
        let text = format!("{name}{} × {} mm", mm(size.x), mm(size.y));
        painter.text(pos2(page.center().x, page.max.y + 8.0), Align2::CENTER_TOP, text, FontId::proportional(12.0), p.text_weak);
    }

    /// 右边的设置；返回 true = 点了「打印机属性」
    fn controls(&mut self, ui: &mut Ui, c: &Ctl) -> bool {
        let t = tr();
        let p = theme::palette(ui.ctx());
        let mut properties = false;
        ui.spacing_mut().item_spacing.y = 6.0;
        let width = ui.available_width();
        caption(ui, t.pr_printer);
        let shown = |p: &Printer| if p.default { format!("{}{}", p.name, t.pr_default) } else { p.name.clone() };
        let current = c.printers.iter().find(|p| Some(&p.name) == self.printer.as_ref()).map(shown).unwrap_or_default();
        egui::ComboBox::from_id_salt("print-printer").width(width).selected_text(current).truncate().show_ui(ui, |ui| {
            for pr in c.printers {
                if ui.selectable_label(Some(&pr.name) == self.printer.as_ref(), shown(pr)).clicked() {
                    self.printer = Some(pr.name.clone());
                    self.s.printer = pr.name.clone();
                    // 换了打印机：纸张按它的默认
                    self.s.paper = 0;
                }
            }
        });
        caption(ui, t.pr_paper);
        ui.add_enabled_ui(c.have_caps, |ui| {
            let current = c.paper.map_or_else(String::new, |p| p.name.clone());
            egui::ComboBox::from_id_salt("print-paper").width(width).selected_text(current).truncate().show_ui(ui, |ui| {
                for pa in c.papers {
                    let r = ui.selectable_label(Some(pa.id) == c.paper.map(|p| p.id), &pa.name).on_hover_text(format!("{} × {} mm", mm(pa.mm[0]), mm(pa.mm[1])));
                    if r.clicked() {
                        self.s.paper = pa.id;
                    }
                }
            });
        });
        caption(ui, t.pr_orientation);
        choose(ui, &mut self.s.orientation, &[(PrintOrientation::Auto, t.pr_auto), (PrintOrientation::Portrait, t.pr_portrait), (PrintOrientation::Landscape, t.pr_landscape)]);
        caption(ui, t.pr_size);
        choose(ui, &mut self.s.size, &[(PrintSize::Fit, t.pr_fit), (PrintSize::Custom, t.pr_custom), (PrintSize::Actual, t.pr_actual)]);
        // 刚切到指定尺寸：从适合页面的大小开始（同一帧里就要有值，不能等下一帧）
        if self.s.size == PrintSize::Custom && self.s.long_mm <= 0.0 {
            self.s.long_mm = c.fit_long;
        }
        let [iw, ih] = [self.px[0].max(1) as f32, self.px[1].max(1) as f32];
        match self.s.size {
            PrintSize::Custom => {
                // 宽高锁定比例，存的是长边
                let wide = iw >= ih;
                let (mut w, mut h) = if wide { (self.s.long_mm, self.s.long_mm * ih / iw) } else { (self.s.long_mm * iw / ih, self.s.long_mm) };
                ui.horizontal(|ui| {
                    ui.label(t.pr_width);
                    if ui.add(mm_field(&mut w)).changed() {
                        self.s.long_mm = if wide { w } else { w * ih / iw };
                    }
                    ui.label(t.pr_height);
                    if ui.add(mm_field(&mut h)).changed() {
                        self.s.long_mm = if wide { h * iw / ih } else { h };
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for (name, b) in [(t.pr_5in, [89.0, 127.0]), (t.pr_6in, [102.0, 152.0]), ("A4", [210.0, 297.0])] {
                        if ui.small_button(name).on_hover_text(format!("{} × {} mm", mm(b[0]), mm(b[1]))).clicked() {
                            self.s.long_mm = layout::fit_long_side(b, self.px);
                        }
                    }
                });
            }
            PrintSize::Actual => match (self.dpi.busy(), c.dpi) {
                (true, _) => {}
                (_, Some(d)) => caption(ui, &fill(t.pr_actual_dpi, &[&d[0].round()])),
                (_, None) => {
                    ui.label(RichText::new(t.pr_no_dpi).size(12.0).color(p.danger));
                }
            },
            PrintSize::Fit => {}
        }
        caption(ui, t.pr_copies);
        ui.add(egui::DragValue::new(&mut self.copies).range(1..=99).speed(0.1));
        egui::CollapsingHeader::new(t.pr_more).id_salt("print-more").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(t.pr_margin);
                ui.add(egui::DragValue::new(&mut self.s.margin_mm).range(0.0..=50.0).speed(0.5).fixed_decimals(1).suffix(" mm"));
            });
            if ui.add_enabled_ui(c.have_caps, |ui| widgets::plain_button(ui, t.pr_properties)).inner.clicked() {
                properties = true;
            }
        });
        if let Some(pl) = c.placement {
            ui.add_space(4.0);
            let ppi = pl.ppi(self.px);
            caption(ui, &fill(t.pr_info, &[&mm(pl.rect.width()), &mm(pl.rect.height()), &ppi.round()]));
            if pl.overflows() {
                let over = pl.over.iter().fold(0.0f32, |a, &b| a.max(b));
                ui.label(RichText::new(fill(t.pr_over, &[&mm(over)])).size(12.0).color(p.danger));
                if ui.small_button(t.pr_use_fit).clicked() {
                    self.s.size = PrintSize::Fit;
                }
            } else if ppi < 150.0 {
                ui.label(RichText::new(t.pr_low_res).size(12.0).color(p.danger));
            }
        }
        if let Some(f) = self.frame {
            caption(ui, &fill(t.pr_frame, &[&f]));
        }
        if c.hardware_margins {
            caption(ui, t.pr_unprintable);
        }
        properties
    }
}

/// 右边设置要用的（这一帧算好的）
struct Ctl<'a> {
    printers: &'a [Printer],
    papers: &'a [Paper],
    paper: Option<&'a Paper>,
    dpi: Option<[f32; 2]>,
    placement: Option<Placement>,
    have_caps: bool,
    /// 适合页面时的长边（切到指定尺寸时的初始值）
    fit_long: f32,
    /// 打印机有印不到的边
    hardware_margins: bool,
}

fn orientation(o: PrintOrientation) -> Option<bool> {
    match o {
        PrintOrientation::Auto => None,
        PrintOrientation::Portrait => Some(false),
        PrintOrientation::Landscape => Some(true),
    }
}
