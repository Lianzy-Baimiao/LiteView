//! 图片信息面板（I）：文件、尺寸、格式、EXIF；GPS 默认折叠。从右侧滑出，盖在图片上。
//! 重命名（F2）：只选中主文件名，扩展名不变。

use std::path::{Path, PathBuf};

use egui::{Align2, Area, Id, Order, RichText, Stroke, vec2};

use super::App;
use crate::decode::Decoded;
use crate::i18n::{fill, tr};
use crate::ui::{icons::Icon, theme, widgets};

pub(crate) struct InfoCache {
    path: PathBuf,
    rows: Vec<(&'static str, String)>,
    exif: Vec<(&'static str, String)>,
    gps: Option<String>,
}

pub(crate) struct Rename {
    pub path: PathBuf,
    pub text: String,
    pub focus: bool,
}

pub(crate) fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1048576.0 { format!("{:.1} MB", b / 1048576.0) } else { format!("{:.0} KB", (b / 1024.0).max(1.0)) }
}

fn build(path: &Path, d: &Decoded, clipboard: bool) -> InfoCache {
    let t = tr();
    let i = &d.info;
    let mut rows = Vec::new();
    if !clipboard {
        rows.push((t.i_name, crate::natsort::file_name(path)));
        rows.push((t.i_folder, path.parent().map(|p| p.display().to_string()).unwrap_or_default()));
    }
    let mp = f64::from(i.width) * f64::from(i.height) / 1e6;
    rows.push((t.i_dims, format!("{} × {}（{mp:.1} MP）", i.width, i.height)));
    if i.file_size > 0 {
        rows.push((t.i_size, size_text(i.file_size)));
    }
    rows.push((t.i_format, format!("{}，{} bit × {}", i.format, i.bits, i.channels)));
    if let crate::decode::Content::Animation { delays, .. } = &d.content {
        let ms: u128 = delays.iter().map(|d| d.as_millis()).sum();
        rows.push((t.i_frames, fill(t.i_frames_fmt, &[&delays.len(), &format!("{:.2}", ms as f64 / 1000.0)])));
    }
    if let Ok(m) = std::fs::metadata(path).and_then(|m| m.modified()) {
        rows.push((t.i_modified, chrono::DateTime::<chrono::Local>::from(m).format("%Y-%m-%d %H:%M:%S").to_string()));
    }
    let (exif, gps) = i.exif.as_deref().map(|e| parse_exif(e)).unwrap_or_default();
    InfoCache { path: path.to_path_buf(), rows, exif, gps }
}

fn parse_exif(raw: &[u8]) -> (Vec<(&'static str, String)>, Option<String>) {
    use exif::{In, Tag};
    let t = tr();
    let data = raw.strip_prefix(b"Exif\0\0").unwrap_or(raw).to_vec();
    let Ok(e) = exif::Reader::new().read_raw(data) else { return Default::default() };
    let get = |tag: Tag| e.get_field(tag, In::PRIMARY).map(|f| f.display_value().with_unit(&e).to_string().trim_matches('"').trim().to_owned());
    let mut out = Vec::new();
    let make = get(Tag::Make).unwrap_or_default();
    let model = get(Tag::Model).unwrap_or_default();
    let camera = if model.to_lowercase().starts_with(&make.to_lowercase()) { model } else { format!("{make} {model}").trim().to_owned() };
    for (label, value) in [
        (t.i_camera, Some(camera).filter(|s| !s.is_empty())),
        (t.i_lens, get(Tag::LensModel)),
        (t.i_aperture, get(Tag::FNumber)),
        (t.i_shutter, get(Tag::ExposureTime)),
        (t.i_iso, get(Tag::PhotographicSensitivity)),
        (t.i_focal, get(Tag::FocalLength)),
        (t.i_taken, get(Tag::DateTimeOriginal)),
    ] {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            out.push((label, v));
        }
    }
    let gps = match (get(Tag::GPSLatitude), get(Tag::GPSLongitude)) {
        (Some(la), Some(lo)) => Some(format!("{la} {}，{lo} {}", get(Tag::GPSLatitudeRef).unwrap_or_default(), get(Tag::GPSLongitudeRef).unwrap_or_default())),
        _ => None,
    };
    (out, gps)
}

impl App {
    pub(crate) fn info_panel(&mut self, ctx: &egui::Context) {
        let a = ctx.animate_bool_with_time(Id::new("info-open"), self.info_open, 0.15);
        if a <= 0.0 {
            return;
        }
        let Some(cur) = &self.cur else { return };
        let Some(d) = cur.loaded.as_ref().and_then(|l| l.as_ref().ok()).cloned() else { return };
        if self.info.as_ref().is_none_or(|c| c.path != cur.path) {
            self.info = Some(build(&cur.path, &d, cur.clipboard));
        }
        let p = theme::palette(ctx);
        let w = 300.0;
        let r = self.canvas;
        let mut close = false;
        Area::new(Id::new("info")).order(Order::Foreground).anchor(Align2::RIGHT_TOP, vec2(w * (1.0 - a), 0.0)).show(ctx, |ui| {
            egui::Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.divider)).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.set_width(w - 32.0);
                ui.set_min_height(r.height() - 32.0);
                let t = tr();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t.info).size(16.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = widgets::icon_button(ui, Icon::Close, t.close, "I", true).clicked();
                    });
                });
                ui.add_space(8.0);
                let Some(c) = &self.info else { return };
                let row = |ui: &mut egui::Ui, k: &str, v: &str| {
                    ui.label(RichText::new(k).color(p.text_weak).size(12.0));
                    ui.add(egui::Label::new(RichText::new(v).color(p.text)).wrap());
                    ui.add_space(6.0);
                };
                for (k, v) in &c.rows {
                    row(ui, k, v);
                }
                if !c.exif.is_empty() || c.gps.is_some() {
                    ui.separator();
                    ui.add_space(6.0);
                }
                for (k, v) in &c.exif {
                    row(ui, k, v);
                }
                if let Some(g) = &c.gps {
                    widgets::checkbox(ui, &mut self.show_gps, t.i_gps_show);
                    if self.show_gps {
                        row(ui, t.i_gps, g);
                    }
                }
            });
        });
        if close {
            self.info_open = false;
        }
    }

    pub(crate) fn start_rename(&mut self) {
        let Some(c) = self.cur.as_ref().filter(|c| !c.clipboard) else { return };
        let name = crate::natsort::file_name(&c.path);
        self.rename = Some(Rename { path: c.path.clone(), text: name, focus: true });
    }

    pub(crate) fn rename_ui(&mut self, ctx: &egui::Context) {
        let Some(r) = &mut self.rename else { return };
        let t = tr();
        let mut done = None;
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(20)).corner_radius(egui::CornerRadius::same(8));
        let modal = egui::Modal::new(Id::new("rename")).frame(frame).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(t.rename).size(16.0));
            ui.add_space(10.0);
            let out = egui::TextEdit::singleline(&mut r.text).desired_width(380.0).show(ui);
            if r.focus {
                r.focus = false;
                out.response.request_focus();
                // 只选中主文件名
                let stem = Path::new(&r.text).file_stem().map_or(0, |s| s.to_string_lossy().chars().count());
                let mut st = out.state;
                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(stem))));
                st.store(ui.ctx(), out.response.id);
            }
            if out.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::accent_button(ui, t.rename, false).clicked() {
                    done = Some(true);
                }
                if widgets::plain_button(ui, t.cancel).clicked() {
                    done = Some(false);
                }
            });
        });
        if done.is_none() && modal.should_close() {
            done = Some(false);
        }
        let Some(ok) = done else { return };
        let Some(r) = self.rename.take() else { return };
        if ok {
            self.do_rename(&r.path, r.text.trim());
        }
    }

    fn do_rename(&mut self, from: &Path, name: &str) {
        let bad = name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']);
        if bad || Some(name) == from.file_name().and_then(|n| n.to_str()) {
            return;
        }
        let to = from.with_file_name(name);
        if to.exists() {
            self.show_toast(fill(tr().failed, &[&tr().name_exists]));
            return;
        }
        if let Err(e) = std::fs::rename(from, &to) {
            self.show_toast(fill(tr().failed, &[&e]));
            return;
        }
        self.loader.forget(from);
        if let Some(f) = self.folder.files.iter_mut().find(|p| p.as_path() == from) {
            *f = to.clone();
        }
        if let Some(c) = self.cur.as_mut().filter(|c| c.path == from) {
            c.path = to.clone();
            if let Some(l) = c.loaded.clone() {
                self.loader.insert(&to, l);
            }
        }
        self.info = None;
        self.title.clear();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn exif_fields() {
        // TIFF 头 + IFD0 一项：Make = "Canon"
        let raw = b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x0F\0\x02\0\0\0\x06\0\0\0\x1A\0\0\0\0Canon\0";
        let (rows, gps) = super::parse_exif(raw);
        assert!(rows.iter().any(|(_, v)| v == "Canon"), "{rows:?}");
        assert!(gps.is_none());
        assert_eq!(super::size_text(2 * 1048576 + 300_000), "2.3 MB");
    }
}
