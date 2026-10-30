//! 文字（PRD 6.7.3）：点击放置文本框，可拖动、拉宽；中文输入法、多行；字体、字号、粗体 / 斜体 / 下划线 / 删除线、
//! 颜色、背景填辅色。确认前可改，点框外或切换工具后变成像素（与画图一致）。
//! 光标、选中、输入法用 egui 的文本框（文字本身透明）；看得见的文字按同样的换行用 resvg 画，与写进图里的完全一样。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, FontFamily, FontId, Pos2, Rect, Vec2, pos2, vec2};
use image::RgbaImage;

use super::{Drag, Editor};
use crate::app::App;
use crate::editor::IRect;

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
}

impl PendingText {
    pub fn new(pos: Pos2, width: f32, swap: bool) -> Self {
        Self { pos, width, text: String::new(), swap, height: 0.0, svg: String::new(), layout: None, raster: None, tex: None, focus: true }
    }
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
        super::canvas::dashed_rect(painter, screen.expand(3.0));
        let h = pos2(screen.right() + 3.0, screen.center().y);
        let hr = Rect::from_center_size(h, Vec2::splat(7.0));
        painter.rect_filled(hr, 1.0, Color32::WHITE);
        painter.rect_stroke(hr, 1.0, egui::Stroke::new(1.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
    }

    /// 确认文字：写进图里（记成一步）；没有文字时直接去掉
    pub(crate) fn commit_text(&mut self, ed: &mut Editor) {
        let Some(t) = ed.text.take() else { return };
        if t.text.trim().is_empty() {
            return;
        }
        let Some(l) = &t.layout else { return };
        let raster = match t.raster {
            Some(r) if !t.svg.is_empty() => r,
            // 系统字体没加载完就确认了：现在画（会等一下字体）
            _ => {
                let family = self.text_font();
                let s = &self.settings;
                let (c1, c2) = if t.swap { (s.color2, s.color1) } else { (s.color1, s.color2) };
                let svg = build_svg(l, &family, s.text_size as f32, s, c1, s.text_background.then_some(c2));
                match render_svg(&svg, (l.w + 2.0 * l.m).ceil() as u32, (l.h + 2.0 * l.m).ceil() as u32) {
                    Some(r) => Arc::new(r),
                    None => return,
                }
            }
        };
        let (x, y) = ((t.pos.x - l.m).round() as i64, (t.pos.y - l.m).round() as i64);
        let (w, h) = (i64::from(ed.doc.width()), i64::from(ed.doc.height()));
        let c = |v: i64, m: i64| v.clamp(0, m) as u32;
        let dest = IRect::new(c(x, w), c(y, h), c(x + i64::from(raster.width()), w), c(y + i64::from(raster.height()), h));
        let mut saved = crate::editor::history::Saved::default();
        saved.touch(&ed.doc.image, dest);
        let changed = crate::editor::ops::composite(ed.doc.image_mut(), &raster, x, y);
        ed.doc.mark(changed);
        ed.doc.commit(saved, false);
        let col = if t.swap { self.settings.color2 } else { self.settings.color1 };
        self.remember_color(col);
    }

    /// 文字工具按下：点在文本框的边上拖动 / 拉宽；点在别处确认现在的文字，没有时放一个新的
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
        let size = self.settings.text_size as f32;
        let width = (size * 12.0).min((ed.doc.width() as f32 - p.x).max(size * 2.0));
        ed.text = Some(PendingText::new(p.round(), width.round(), secondary));
    }

    /// 文本框外框附近：右边中间的点拉宽，其余拖动
    fn text_hit(&self, ed: &Editor, screen: Pos2, p: Pos2) -> Option<Drag> {
        let t = ed.text.as_ref()?;
        let k = self.view.zoom / self.ctx.pixels_per_point();
        let at = self.image_to_screen(ed, t.pos);
        let r = Rect::from_min_size(at, vec2((t.width * k).max(8.0), (t.height * k).max(8.0))).expand(3.0);
        if (pos2(r.right(), r.center().y) - screen).length() <= 7.0 {
            return Some(Drag::TextWidth { from: t.width, grab: p.x });
        }
        (r.expand(5.0).contains(screen) && !r.shrink(2.0).contains(screen)).then_some(Drag::TextMove { grab: p, from: t.pos })
    }

    pub(crate) fn text_drag(&mut self, ed: &mut Editor, p: Pos2) {
        let Some(t) = &mut ed.text else { return };
        match &ed.drag {
            Some(Drag::TextMove { grab, from }) => t.pos = (*from + (p - *grab)).round(),
            Some(Drag::TextWidth { from, grab }) => t.width = (from + p.x - grab).round().max(t.height.clamp(8.0, 40.0)),
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

    #[test]
    fn the_text_rewraps_when_the_box_gets_wider_or_the_view_zooms_in() {
        let ctx = egui::Context::default();
        let settings = Settings { ask_default_app: false, text_size: 20, ..Default::default() };
        let mut app = App::new(settings, Loader::new(0, crate::decode::Options::default(), 1 << 20), Default::default(), &ctx);
        let doc = crate::editor::history::Doc::new(RgbaImage::new(400, 300), 100, 1 << 20);
        let src = super::super::Source {
            path: "a.png".into(), clipboard: false, format: "PNG", file_size: 0,
            jpeg_quality: None, exif: None, canvas_icc: None, canvas_alpha: false,
        };
        let mut ed = Editor::new(doc, src, super::super::Tool::Text, Default::default());
        let mut t = PendingText::new(pos2(10.0, 10.0), 300.0, false);
        t.text = "LiteView box".into();
        t.focus = false;
        ed.text = Some(t);
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
}
