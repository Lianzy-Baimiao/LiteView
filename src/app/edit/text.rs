//! 文字（PRD 6.7.3）：点击放置文本框，可拖动、拉宽；中文输入法、多行；字体、字号、粗体 / 斜体 / 下划线 / 删除线、
//! 颜色、背景填辅色。确认前可改，点框外或切换工具后变成像素（与画图一致）。
//! 写进图里的文字，那块没被别的改动盖过时，用文字工具点它能拿起来接着改、拖到别处，放下时与拿起记成一步。
//! 光标、选中、输入法用 egui 的文本框（文字本身透明）；看得见的文字按同样的换行用 resvg 画，与写进图里的完全一样。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, FontFamily, FontId, Pos2, Rect, Vec2, pos2, vec2};
use image::RgbaImage;

use super::{Drag, Editor, Tool};
use crate::app::App;
use crate::editor::IRect;
use crate::editor::history::{Saved, read_tile, write_tile};

/// 还没确认的文本框
pub(crate) struct PendingText {
    /// 左上角（图片像素）
    pub pos: Pos2,
    /// 宽度（图片像素，到这里自动换行）
    pub width: f32,
    pub text: String,
    /// 右键放置的：用辅色
    pub swap: bool,
    /// 上一帧的高度（图片像素）
    pub height: f32,
    /// 按当前换行生成的 SVG、外框，与画好的图
    svg: String,
    layout: Option<Layout>,
    raster: Option<Arc<RgbaImage>>,
    tex: Option<egui::TextureHandle>,
    pub focus: bool,
    /// 从图上拿起来的：拿起时改回原样的块（放下时与写上去的一起记成一步，撤销时写回去）
    lifted: Option<Saved>,
}

impl PendingText {
    pub fn new(pos: Pos2, width: f32, swap: bool) -> Self {
        Self { pos, width, text: String::new(), swap, height: 0.0, svg: String::new(), layout: None, raster: None, tex: None, focus: true, lifted: None }
    }
}

/// 写进图里的文字：写之前、写之后那一块的像素，与重新编辑要用的内容和样式
pub(crate) struct PlacedText {
    rect: IRect,
    /// 点它能拿起来的范围：真的写了字（或背景）的地方，四周宽一点
    hit: IRect,
    under: Vec<u8>,
    over: Vec<u8>,
    pos: Pos2,
    width: f32,
    height: f32,
    text: String,
    style: TextStyle,
}

struct TextStyle {
    /// 设置里的字体（空 = 默认中文字体）
    font: String,
    size: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: [u8; 4],
    background: Option<[u8; 4]>,
}

/// 记着的文字像素最多占这么多，超出时丢最早的
const PLACED_BYTES: usize = 64 << 20;

impl Editor {
    /// p（图片像素）处最上面一个还能拿起来的文字：写上去之后那一块没被改过
    pub(crate) fn text_at(&self, p: Pos2) -> Option<usize> {
        let (x, y) = (p.x.floor(), p.y.floor());
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (w, h) = (self.doc.width(), self.doc.height());
        self.texts.iter().rposition(|t| {
            t.hit.contains(x as u32, y as u32) && t.rect.x1 <= w && t.rect.y1 <= h && read_tile(&self.doc.image, t.rect) == t.over
        })
    }
}

/// rect 这块写之前、写之后（按行连续的 RGBA）不一样的像素的范围
fn changed_bounds(before: &[u8], after: &[u8], rect: IRect) -> Option<IRect> {
    let w = rect.width() as usize;
    let mut out: Option<IRect> = None;
    for (y, (a, b)) in before.chunks_exact(w * 4).zip(after.chunks_exact(w * 4)).enumerate() {
        let diff = |i: &usize| a[i * 4..i * 4 + 4] != b[i * 4..i * 4 + 4];
        let (Some(x0), Some(x1)) = ((0..w).find(diff), (0..w).rfind(diff)) else { continue };
        let row = IRect::new(rect.x0 + x0 as u32, rect.y0 + y as u32, rect.x0 + x1 as u32 + 1, rect.y0 + y as u32 + 1);
        out = Some(out.map_or(row, |o| o.union(&row)));
    }
    out
}

/// 没选过字体时用的：中文字体
pub(crate) fn default_family(db: &resvg::usvg::fontdb::Database) -> String {
    for f in ["Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", "Source Han Sans SC", "WenQuanYi Micro Hei"] {
        if db.faces().any(|face| face.families.iter().any(|(n, _)| n == f)) {
            return f.to_owned();
        }
    }
    "sans-serif".to_owned()
}

/// 系统里所有字体的名字（排序、去重）
pub(crate) fn families() -> Option<&'static [String]> {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(l) = LIST.get() {
        return Some(l);
    }
    let db = crate::decode::extra::fontdb_ready()?;
    let mut v: Vec<String> = db.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup();
    Some(LIST.get_or_init(|| v))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 文本框用的 egui 字体：与 resvg 选同一个字形文件（粗体 / 斜体按字体的对应字形），文字宽度一致、光标位置对得上。
/// 第一次用到时加入 egui，下一帧才能用；还不能用时返回界面字体
fn egui_family(ctx: &egui::Context, family: &str, bold: bool, italic: bool) -> FontFamily {
    use resvg::usvg::fontdb::{Family, Query, Source, Style, Weight};
    static LOADED: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    static DEFS: OnceLock<Mutex<egui::FontDefinitions>> = OnceLock::new();
    let name = format!("text:{family}:{bold}:{italic}");
    let fam = FontFamily::Name(name.clone().into());
    let loaded = LOADED.get_or_init(Default::default);
    if let Some(&ok) = loaded.lock().expect("lock").get(&name) {
        let bound = ctx.fonts(|f| f.definitions().families.contains_key(&fam));
        return if ok && bound { fam } else { FontFamily::Proportional };
    }
    let Some(db) = crate::decode::extra::fontdb_ready() else { return FontFamily::Proportional };
    let q = Query {
        families: &[Family::Name(family), Family::SansSerif],
        weight: if bold { Weight::BOLD } else { Weight::NORMAL },
        style: if italic { Style::Italic } else { Style::Normal },
        ..Query::default()
    };
    let face = db.query(&q).and_then(|id| db.face(id)).and_then(|f| {
        let bytes: &'static [u8] = match &f.source {
            Source::File(p) | Source::SharedFile(p, _) => crate::ui::fonts::map_cached(p)?,
            Source::Binary(b) => Box::leak((**b).as_ref().to_vec().into_boxed_slice()),
        };
        Some((bytes, f.index))
    });
    let ok = face.is_some();
    loaded.lock().expect("lock").insert(name.clone(), ok);
    if let Some((bytes, index)) = face {
        let mut defs = DEFS.get_or_init(|| Mutex::new(ctx.fonts(|f| f.definitions().clone()))).lock().expect("lock");
        let mut data = egui::FontData::from_static(bytes);
        data.index = index;
        defs.font_data.insert(name.clone(), data.into());
        // 缺的字（如中文、符号）再用界面字体
        let mut list = vec![name.clone()];
        list.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
        defs.families.insert(fam, list);
        ctx.set_fonts(defs.clone());
    }
    FontFamily::Proportional
}

/// 文本框的样子：每行（文字、基线左端，图片像素、相对文本框左上角）与外框
pub(crate) struct Layout {
    rows: Vec<(String, f32, f32)>,
    /// 文本框宽、高，四周留的边（斜体、字形可能伸出框外）
    w: f32,
    h: f32,
    m: f32,
}

/// 按文本框里的换行生成 SVG（每行一个 text，位置与 egui 排版一致）；单位是图片像素
fn build_svg(l: &Layout, family: &str, size: f32, s: &crate::settings::Settings, color: [u8; 4], bg: Option<[u8; 4]>) -> String {
    let (w, h, m) = (l.w, l.h, l.m);
    let hex = |c: [u8; 4]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let mut deco = Vec::new();
    if s.text_underline {
        deco.push("underline");
    }
    if s.text_strike {
        deco.push("line-through");
    }
    let (sw, sh) = (w + 2.0 * m, h + 2.0 * m);
    let mut out = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{sw}" height="{sh}" viewBox="0 0 {sw} {sh}">"#);
    if let Some(b) = bg {
        out += &format!(r#"<rect x="{m}" y="{m}" width="{w}" height="{h}" fill="{}"/>"#, hex(b));
    }
    for (text, x, y) in &l.rows {
        let (x, y) = (x + m, y + m);
        out += &format!(
            r#"<text x="{x}" y="{y}" font-family="{}" font-size="{size}" font-weight="{}" font-style="{}" text-decoration="{}" fill="{}" fill-opacity="{}" xml:space="preserve">{}</text>"#,
            xml_escape(family),
            if s.text_bold { "bold" } else { "normal" },
            if s.text_italic { "italic" } else { "normal" },
            if deco.is_empty() { "none".to_owned() } else { deco.join(" ") },
            hex(color),
            f32::from(color[3]) / 255.0,
            xml_escape(text)
        );
    }
    out + "</svg>"
}

/// 文本框里排好的每一行（图片像素）
fn layout_rows(g: &egui::Galley, off: Vec2, k: f32) -> Vec<(String, f32, f32)> {
    g.rows
        .iter()
        .filter_map(|row| {
            let first = row.row.glyphs.first()?;
            let text: String = row.row.glyphs.iter().map(|g| g.chr).collect();
            (!text.trim().is_empty()).then(|| (text, (off.x + row.pos.x + first.pos.x) / k, (off.y + row.pos.y + first.pos.y) / k))
        })
        .collect()
}

/// 画 SVG（图片像素，1:1）
fn render_svg(svg: &str, w: u32, h: u32) -> Option<RgbaImage> {
    let db = crate::decode::extra::fontdb();
    let opt = resvg::usvg::Options { fontdb: db, ..Default::default() };
    let tree = resvg::usvg::Tree::from_str(svg, &opt).ok()?;
    let mut pix = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pix.as_mut());
    let mut out = RgbaImage::new(w.max(1), h.max(1));
    for (o, p) in out.pixels_mut().zip(pix.pixels()) {
        let c = p.demultiply();
        *o = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Some(out)
}

impl App {
    /// 当前用的字体（没选过时用中文字体）
    fn text_font(&self) -> String {
        if !self.settings.text_font.is_empty() {
            return self.settings.text_font.clone();
        }
        crate::decode::extra::fontdb_ready().map_or_else(|| "sans-serif".into(), |db| default_family(&db))
    }

    /// 文本框、看得见的文字、外框与拉宽的控制点
    pub(crate) fn text_ui(&mut self, ctx: &egui::Context, painter: &egui::Painter, ed: &mut Editor) {
        if ed.text.is_none() {
            self.text_hover(painter, ed);
            return;
        }
        let ppp = ctx.pixels_per_point();
        let k = self.view.zoom / ppp;
        let origin = self.image_to_screen(ed, Pos2::ZERO);
        let family = self.text_font();
        let s = &self.settings;
        let size = s.text_size as f32;
        let font = FontId::new((size * k).clamp(1.0, 512.0), egui_family(ctx, &family, s.text_bold, s.text_italic));
        let p = crate::ui::theme::palette(ctx);
        let Some(t) = &mut ed.text else { return };
        let at = origin + t.pos.to_vec2() * k;
        let width = (t.width * k).max(8.0);
        let mut galley = None;
        egui::Area::new(egui::Id::new("edit-text")).order(egui::Order::Foreground).fixed_pos(at).constrain(false).show(ctx, |ui| {
            let v = ui.visuals_mut();
            v.selection.bg_fill = p.accent.gamma_multiply(0.35);
            v.text_cursor.stroke = egui::Stroke::new(2.0, p.accent);
            // Area 给的可用宽度是上一帧的大小：不定死的话框只会变窄，拉宽、放大后字还按原来的宽度换行
            ui.set_width(width);
            let out = egui::TextEdit::multiline(&mut t.text)
                .font(font.clone())
                .text_color(Color32::TRANSPARENT)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .desired_width(width)
                .desired_rows(1)
                .show(ui);
            // 松开鼠标之后再要焦点（按下的那一下是在画布上，松开时 egui 会把焦点收回）
            if t.focus && !ui.input(|i| i.pointer.any_down()) {
                if out.response.response.has_focus() {
                    t.focus = false;
                } else {
                    out.response.response.request_focus();
                    ui.ctx().request_repaint();
                }
            }
            galley = Some((out.galley.clone(), out.galley_pos - at));
        });
        let Some((g, off)) = galley else { return };
        t.height = (g.size().y.max(font.size) + off.y) / k;
        let m = (size * 0.5).ceil();
        let layout = Layout { rows: layout_rows(&g, off, k), w: t.width, h: t.height, m };
        let (c1, c2) = if t.swap { (s.color2, s.color1) } else { (s.color1, s.color2) };
        let svg = build_svg(&layout, &family, size, s, c1, s.text_background.then_some(c2));
        let ready = crate::decode::extra::fontdb_ready().is_some();
        if (svg != t.svg || t.raster.is_none()) && ready {
            let (w, h) = ((layout.w + 2.0 * m).ceil() as u32, (layout.h + 2.0 * m).ceil() as u32);
            t.raster = render_svg(&svg, w, h).map(Arc::new);
            t.tex = t.raster.as_ref().map(|r| ctx.load_texture("edit-text", crate::viewer::texture::region(r, 0, 0, r.width(), r.height()), egui::TextureOptions::LINEAR));
            t.svg = svg;
        }
        t.layout = Some(layout);
        let screen = Rect::from_min_size(at, vec2(width, t.height * k));
        if let (Some(tex), Some(r)) = (&t.tex, &t.raster) {
            let rr = Rect::from_min_size(at - Vec2::splat(m * k), vec2(r.width() as f32, r.height() as f32) * k);
            painter.image(tex.id(), rr, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            // 系统字体还没加载完：先用界面字体显示
            painter.galley_with_override_text_color(at + off, g, Color32::from_rgba_unmultiplied(c1[0], c1[1], c1[2], c1[3]));
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let frame = screen.expand(3.0);
        super::canvas::dashed_rect(painter, frame);
        // 左右两侧拉宽（整条边都能拉，控制点只是标出来）
        for x in [frame.left(), frame.right()] {
            let hr = Rect::from_center_size(pos2(x, frame.center().y), Vec2::splat(8.0));
            painter.rect_filled(hr, 1.0, Color32::WHITE);
            painter.rect_stroke(hr, 1.0, egui::Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
        }
    }

    /// 文字工具下鼠标停在还能拿起来的文字上：画出它的框，看得出能拖
    fn text_hover(&self, painter: &egui::Painter, ed: &Editor) {
        if ed.tool != Tool::Text || ed.drag.is_some() {
            return;
        }
        let Some(t) = ed.cursor.and_then(|c| ed.text_at(c)).map(|i| &ed.texts[i]) else { return };
        let h = t.hit;
        let a = self.image_to_screen(ed, pos2(h.x0 as f32, h.y0 as f32));
        let b = self.image_to_screen(ed, pos2(h.x1 as f32, h.y1 as f32));
        super::canvas::dashed_rect(painter, Rect::from_min_max(a, b));
    }

    /// 确认文字：写进图里（记成一步；从图上拿起来的与拿起记成同一步）；没有文字时直接去掉
    pub(crate) fn commit_text(&mut self, ed: &mut Editor) {
        let Some(mut t) = ed.text.take() else { return };
        let mut saved = t.lifted.take().unwrap_or_default();
        let raster = match &t.layout {
            Some(l) if !t.text.trim().is_empty() => match &t.raster {
                Some(r) if !t.svg.is_empty() => Some(r.clone()),
                // 系统字体没加载完就确认了：现在画（会等一下字体）
                _ => self.render_text(l, t.swap).map(Arc::new),
            },
            _ => None,
        };
        let (Some(l), Some(raster)) = (&t.layout, raster) else {
            // 文字删光了：拿起来的就是删掉（记成一步）；还没排过版（拿起的同一帧）或画不出来：放回原处
            if t.text.trim().is_empty() {
                ed.doc.commit(saved, false);
            } else {
                let d = saved.restore(ed.doc.image_mut());
                ed.doc.mark(d);
            }
            return;
        };
        let (x, y) = ((t.pos.x - l.m).round() as i64, (t.pos.y - l.m).round() as i64);
        let (w, h) = (i64::from(ed.doc.width()), i64::from(ed.doc.height()));
        let c = |v: i64, m: i64| v.clamp(0, m) as u32;
        let dest = IRect::new(c(x, w), c(y, h), c(x + i64::from(raster.width()), w), c(y + i64::from(raster.height()), h));
        saved.touch(&ed.doc.image, dest);
        let under = read_tile(&ed.doc.image, dest);
        let changed = crate::editor::ops::composite(ed.doc.image_mut(), &raster, x, y);
        ed.doc.mark(changed);
        ed.doc.commit(saved, false);
        let s = &self.settings;
        let (c1, c2) = if t.swap { (s.color2, s.color1) } else { (s.color1, s.color2) };
        let style = TextStyle {
            font: s.text_font.clone(), size: s.text_size, bold: s.text_bold, italic: s.text_italic,
            underline: s.text_underline, strike: s.text_strike, color: c1, background: s.text_background.then_some(c2),
        };
        self.remember_color(c1);
        let over = read_tile(&ed.doc.image, dest);
        if let Some(r) = changed_bounds(&under, &over, dest) {
            // 点得到的范围：真的写了字（或背景）的地方，四周再宽一点
            let (iw, ih) = (ed.doc.width(), ed.doc.height());
            let hit = IRect::new(r.x0.saturating_sub(4), r.y0.saturating_sub(4), (r.x1 + 4).min(iw), (r.y1 + 4).min(ih));
            ed.texts.retain(|p| p.rect != dest || p.over != over);
            ed.texts.push(PlacedText { rect: dest, hit, under, over, pos: t.pos, width: t.width, height: t.height, text: t.text, style });
        }
        while ed.texts.len() > 1 && ed.texts.iter().map(|p| p.under.len() + p.over.len()).sum::<usize>() > PLACED_BYTES {
            ed.texts.remove(0);
        }
    }

    /// 按文本框现在的排版和属性栏的样式画文字（图片像素）
    fn render_text(&self, l: &Layout, swap: bool) -> Option<RgbaImage> {
        let family = self.text_font();
        let s = &self.settings;
        let (c1, c2) = if swap { (s.color2, s.color1) } else { (s.color1, s.color2) };
        let svg = build_svg(l, &family, s.text_size as f32, s, c1, s.text_background.then_some(c2));
        render_svg(&svg, (l.w + 2.0 * l.m).ceil() as u32, (l.h + 2.0 * l.m).ceil() as u32)
    }

    /// 把写进图里的第 i 个文字拿起来：那块改回写之前的样子，变回文本框，样式放回属性栏
    fn lift_text(&mut self, ed: &mut Editor, i: usize) {
        let p = &ed.texts[i];
        let mut saved = Saved::default();
        saved.touch(&ed.doc.image, p.rect);
        write_tile(ed.doc.image_mut(), p.rect, &p.under);
        ed.doc.mark(p.rect);
        let st = &p.style;
        let s = &mut self.settings;
        (s.text_font, s.text_size, s.text_bold, s.text_italic) = (st.font.clone(), st.size, st.bold, st.italic);
        (s.text_underline, s.text_strike, s.color1) = (st.underline, st.strike, st.color);
        s.text_background = st.background.is_some();
        if let Some(b) = st.background {
            s.color2 = b;
        }
        let mut t = PendingText::new(p.pos, p.width, false);
        (t.text, t.height, t.lifted) = (p.text.clone(), p.height, Some(saved));
        ed.text = Some(t);
    }

    /// 不要还没确认的文字；从图上拿起来的写回原处
    pub(crate) fn drop_text(&mut self, ed: &mut Editor) {
        if let Some(saved) = ed.text.take().and_then(|t| t.lifted) {
            let d = saved.restore(ed.doc.image_mut());
            ed.doc.mark(d);
        }
    }

    /// 文字工具按下：点在文本框的边上拖动 / 拉宽；点在别处确认现在的文字；
    /// 没有文本框时，点在还能拿起来的文字上把它拿起来（可以接着拖），否则放一个新的
    pub(crate) fn text_press(&mut self, ed: &mut Editor, p: Pos2, screen: Pos2, secondary: bool) {
        if let Some(d) = self.text_hit(ed, screen, p) {
            ed.drag = Some(d);
            return;
        }
        if ed.text.is_some() {
            self.edit_finalize(ed);
            return;
        }
        self.edit_finalize(ed);
        crate::decode::extra::preload_fontdb();
        if !secondary && let Some(i) = ed.text_at(p) {
            self.lift_text(ed, i);
            let from = ed.texts[i].pos;
            ed.drag = Some(Drag::TextMove { grab: p, from });
            return;
        }
        let size = self.settings.text_size as f32;
        let width = (size * 12.0).min((ed.doc.width() as f32 - p.x).max(size * 2.0));
        ed.text = Some(PendingText::new(p.round(), width.round(), secondary));
    }

    /// 文本框四周（屏幕坐标）：左右两条边（连同四个角）拉宽，上下边拖动；框里面是输入框自己的（放光标、选字）
    pub(crate) fn text_hit(&self, ed: &Editor, screen: Pos2, p: Pos2) -> Option<Drag> {
        const EDGE: f32 = 12.0;
        let t = ed.text.as_ref()?;
        let k = self.view.zoom / self.ctx.pixels_per_point();
        let r = Rect::from_min_size(self.image_to_screen(ed, t.pos), vec2((t.width * k).max(8.0), (t.height * k).max(8.0)));
        if r.contains(screen) || !r.expand(EDGE).contains(screen) {
            return None;
        }
        if screen.x > r.right() {
            Some(Drag::TextWidth { from: t.width, grab: p.x, left: None })
        } else if screen.x < r.left() {
            Some(Drag::TextWidth { from: t.width, grab: p.x, left: Some(t.pos.x) })
        } else {
            Some(Drag::TextMove { grab: p, from: t.pos })
        }
    }

    pub(crate) fn text_drag(&mut self, ed: &mut Editor, p: Pos2) {
        let Some(t) = &mut ed.text else { return };
        let min = t.height.clamp(8.0, 40.0);
        match &ed.drag {
            Some(Drag::TextMove { grab, from }) => t.pos = (*from + (p - *grab)).round(),
            Some(Drag::TextWidth { from, grab, left: None }) => t.width = (from + p.x - grab).round().max(min),
            // 拉左边：右边不动
            Some(Drag::TextWidth { from, grab, left: Some(x0) }) => {
                let right = x0 + from;
                t.pos.x = (x0 + p.x - grab).round().min(right - min);
                t.width = right - t.pos.x;
            }
            _ => {}
        }
    }

    pub(crate) fn text_props(&mut self, ui: &mut egui::Ui, _ed: &mut Editor) {
        let t = crate::i18n::tr();
        crate::decode::extra::preload_fontdb();
        let cur = self.text_font();
        let s = &mut self.settings;
        ui.label(egui::RichText::new(t.p_font).color(crate::ui::theme::palette(ui.ctx()).text_weak));
        let list = families();
        ui.add_enabled_ui(list.is_some(), |ui| {
            egui::ComboBox::from_id_salt("text-font").selected_text(cur.as_str()).width(170.0).height(360.0).show_ui(ui, |ui| {
                for f in list.unwrap_or_default() {
                    if ui.selectable_label(*f == cur, f).clicked() {
                        s.text_font = f.clone();
                    }
                }
            });
        });
        ui.add_space(10.0);
        super::props::slider_row(ui, t.p_size, &mut s.text_size, 6, 400, true, " px");
        let p = crate::ui::theme::palette(ui.ctx());
        for (icon, tip, on) in [
            (crate::ui::icons::Icon::Bold, t.p_bold, &mut s.text_bold),
            (crate::ui::icons::Icon::Italic, t.p_italic, &mut s.text_italic),
            (crate::ui::icons::Icon::Underline, t.p_underline, &mut s.text_underline),
            (crate::ui::icons::Icon::Strike, t.p_strike, &mut s.text_strike),
        ] {
            if crate::ui::widgets::icon_button_ex(ui, icon, tip, "", true, *on, None).clicked() {
                *on = !*on;
            }
        }
        let _ = p;
        ui.add_space(10.0);
        crate::ui::widgets::checkbox(ui, &mut s.text_background, t.p_text_bg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::viewer::loader::Loader;

    /// 画一帧还没确认的文本框，返回文字排成几行
    fn rows(app: &mut App, ed: &mut Editor, ctx: &egui::Context) -> usize {
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))), ..Default::default() };
        let _ = ctx.run_ui(input, |ui| {
            app.canvas = ui.max_rect();
            app.text_ui(ui.ctx(), ui.painter(), ed);
        });
        ed.text.as_ref().and_then(|t| t.layout.as_ref()).map_or(0, |l| l.rows.len())
    }

    /// 400 × 300 的透明图上放一个文本框（左上角 10, 10，宽 300）
    fn editor_with_text(text: &str, settings: Settings) -> (egui::Context, App, Editor) {
        let ctx = egui::Context::default();
        let app = App::new(Settings { ask_default_app: false, ..settings }, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        let doc = crate::editor::history::Doc::new(RgbaImage::new(400, 300), 100, 1 << 20);
        let src = super::super::Source {
            path: "a.png".into(), clipboard: false, format: "PNG", file_size: 0,
            jpeg_quality: None, exif: None, canvas_icc: None, canvas_alpha: false,
        };
        let mut ed = Editor::new(doc, src, Tool::Text, Default::default());
        let mut t = PendingText::new(pos2(10.0, 10.0), 300.0, false);
        t.text = text.into();
        t.focus = false;
        ed.text = Some(t);
        (ctx, app, ed)
    }

    #[test]
    fn the_text_rewraps_when_the_box_gets_wider_or_the_view_zooms_in() {
        let (ctx, mut app, mut ed) = editor_with_text("LiteView box", Settings { text_size: 20, ..Default::default() });
        let mut rows_at = |width: f32, zoom: f32| {
            if let Some(t) = &mut ed.text {
                t.width = width;
            }
            app.view.zoom = zoom;
            rows(&mut app, &mut ed, &ctx)
        };
        let wide = rows_at(300.0, 1.0);
        assert_eq!(wide, 1);
        assert!(rows_at(60.0, 1.0) > wide);
        // 拉回原来的宽度：按新宽度重排（以前停在拉窄时的宽度）
        assert_eq!(rows_at(300.0, 1.0), wide);
        // 放大看：框和字一起变大，换行不变
        assert_eq!(rows_at(300.0, 2.0), wide);
    }

    /// 按下时落在文本框哪一块（屏幕坐标相对文本框）
    #[test]
    fn the_whole_side_of_the_text_box_widens_it_and_the_top_and_bottom_move_it() {
        let (ctx, mut app, mut ed) = editor_with_text("LiteView\nbox\nthree", Settings { text_size: 20, ..Default::default() });
        rows(&mut app, &mut ed, &ctx);
        let t = ed.text.as_ref().unwrap();
        let k = app.view.zoom / ctx.pixels_per_point();
        let r = Rect::from_min_size(app.image_to_screen(&ed, t.pos), vec2(t.width * k, t.height * k));
        assert!(r.height() > 50.0, "{r:?}");
        let hit = |s: Pos2| app.text_hit(&ed, s, app.screen_to_image(&ed, s));
        // 右边整条边、上下两个角都能拉宽（以前只有边中间 7 像素的圆点）
        for y in [r.top() + 2.0, r.center().y, r.bottom() - 2.0, r.top() - 6.0, r.bottom() + 6.0] {
            assert!(matches!(hit(pos2(r.right() + 6.0, y)), Some(Drag::TextWidth { left: None, .. })), "right at {y}");
            assert!(matches!(hit(pos2(r.left() - 6.0, y)), Some(Drag::TextWidth { left: Some(_), .. })), "left at {y}");
        }
        assert!(matches!(hit(pos2(r.center().x, r.top() - 6.0)), Some(Drag::TextMove { .. })));
        assert!(matches!(hit(pos2(r.center().x, r.bottom() + 6.0)), Some(Drag::TextMove { .. })));
        // 框里面放光标；离得远是确认
        assert!(hit(r.center()).is_none());
        assert!(hit(pos2(r.right() + 30.0, r.center().y)).is_none());
    }

    #[test]
    fn written_text_can_be_picked_up_and_moved_until_something_covers_it() {
        let settings = Settings { text_size: 20, text_background: true, color2: [200, 0, 0, 255], ..Default::default() };
        let (ctx, mut app, mut ed) = editor_with_text("Hello", settings);
        let blank = ed.doc.image.as_ref().clone();
        rows(&mut app, &mut ed, &ctx);
        app.commit_text(&mut ed);
        let written = ed.doc.image.as_ref().clone();
        assert!(written != blank);
        // 改了属性栏之后点文字：拿起来，那块恢复成写之前的样子，属性栏回到这段文字的样式
        (app.settings.text_size, app.settings.color2) = (40, [0, 0, 255, 255]);
        let at = pos2(50.0, 20.0);
        assert!(ed.text_at(at).is_some());
        app.text_press(&mut ed, at, Pos2::ZERO, false);
        assert!(ed.text.as_ref().is_some_and(|t| t.text == "Hello"));
        assert!(*ed.doc.image == blank);
        assert_eq!((app.settings.text_size, app.settings.color2), (20, [200, 0, 0, 255]));
        // 接着拖到右边 100 像素、松开、确认：原处空了，新位置的能再拿起来；撤销一次回到原处
        assert!(matches!(ed.drag, Some(Drag::TextMove { .. })));
        app.text_drag(&mut ed, at + vec2(100.0, 0.0));
        ed.drag = None;
        rows(&mut app, &mut ed, &ctx);
        app.commit_text(&mut ed);
        assert!(ed.text_at(pos2(20.0, 15.0)).is_none());
        assert!(ed.text_at(at + vec2(100.0, 0.0)).is_some());
        assert!(ed.doc.undo());
        assert!(*ed.doc.image == written);
        assert!(ed.text_at(at).is_some());
        // 拿起来之后撤销：放回原处
        app.text_press(&mut ed, at, Pos2::ZERO, false);
        ed.drag = None;
        app.edit_undo(&mut ed);
        assert!(ed.text.is_none());
        assert!(*ed.doc.image == written);
        // 那块被改过（例如画了一笔）：不能再拿起来，点那里放新的文本框
        let mut saved = Saved::default();
        saved.touch(&ed.doc.image, IRect::new(50, 20, 51, 21));
        ed.doc.image_mut().put_pixel(50, 20, image::Rgba([0, 255, 0, 255]));
        ed.doc.commit(saved, false);
        assert!(ed.text_at(at).is_none());
        app.text_press(&mut ed, at, Pos2::ZERO, false);
        assert!(ed.text.as_ref().is_some_and(|t| t.text.is_empty()));
    }

    /// 能点到的范围只算真的改了的像素（没有背景的短字，框右边的空白处点下去是放新的文本框）
    #[test]
    fn the_pick_up_area_is_only_the_changed_pixels() {
        let rect = IRect::new(100, 10, 110, 14);
        let before = vec![0u8; 10 * 4 * 4];
        let mut after = before.clone();
        after[(10 + 2) * 4] = 9; // (102, 11)
        after[(3 * 10 + 5) * 4 + 3] = 9; // (105, 13)
        assert_eq!(changed_bounds(&before, &after, rect), Some(IRect::new(102, 11, 106, 14)));
        assert_eq!(changed_bounds(&before, &before, rect), None);
    }
}
