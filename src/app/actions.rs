//! 复制、粘贴、删除、另存为、保存旋转、打开文件、缩放与旋转。耗时的都在后台线程做。

use std::sync::Arc;

use super::{App, Dialog, Notice};
use crate::i18n::{fill, tr};
use crate::save::{self, Format};
use crate::settings::FitMode;
use crate::viewer::view::{View, WHEEL_STEP, snap};

impl App {
    fn spawn(&self, f: impl FnOnce() -> Option<Notice> + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            if let Some(n) = f() {
                let _ = tx.send(n);
                ctx.request_repaint();
            }
        });
    }

    pub(crate) fn open_dialog(&mut self, ctx: &egui::Context) {
        let mut exts: Vec<&str> = crate::viewer::folder::EXTS.to_vec();
        exts.extend_from_slice(crate::decode::sniff::RAW_EXTS);
        let mut d = rfd::FileDialog::new().add_filter(tr().image_files, &exts).add_filter(tr().all_files, &["*"]);
        if let Some(dir) = self.cur.as_ref().and_then(|c| (!c.clipboard).then(|| c.path.parent().map(|p| p.to_path_buf())).flatten()) {
            d = d.set_directory(dir);
        }
        if let Some(p) = d.pick_file() {
            self.open_path(p, ctx);
        }
    }

    /// 当前显示的图（动图取当前帧），按显示方向旋转好
    fn displayed_image(&self) -> Option<(Arc<image::RgbaImage>, crate::viewer::view::Transform)> {
        let d = self.decoded()?;
        let img = match &d.content {
            crate::decode::Content::Animation { frames, .. } => frames[self.play.as_ref().map_or(0, |p| p.index).min(frames.len() - 1)].image.clone(),
            crate::decode::Content::Still { image, .. } => image.clone(),
        };
        Some((img, self.view.xf))
    }

    pub(crate) fn copy_image(&mut self) {
        let Some((img, xf)) = self.displayed_image() else { return };
        self.spawn(move || {
            let img = save::apply_transform(&img, xf);
            Some(Notice::Toast(match crate::clipboard::set_image(img) {
                Ok(()) => tr().copied.to_owned(),
                Err(e) => fill(tr().failed, &[&e]),
            }))
        });
    }

    pub(crate) fn copy_path(&mut self, ctx: &egui::Context) {
        if let Some(c) = self.cur.as_ref().filter(|c| !c.clipboard) {
            ctx.copy_text(c.path.to_string_lossy().into_owned());
            self.show_toast(tr().copied);
        }
    }

    /// Ctrl+V：文件（资源管理器里复制的）→ 图片 → 文字里的路径
    pub(crate) fn paste(&mut self) {
        self.spawn(|| {
            Some(match crate::clipboard::get() {
                crate::clipboard::Content::Files(files) => match files.into_iter().find(|p| p.is_file()) {
                    Some(p) => Notice::OpenPath(p),
                    None => Notice::Toast(tr().nothing_to_paste.to_owned()),
                },
                crate::clipboard::Content::Image(img) => Notice::Pasted(Box::new(crate::decode::from_rgba(img, "PNG"))),
                crate::clipboard::Content::Path(p) => Notice::OpenPath(p),
                crate::clipboard::Content::Nothing => Notice::Toast(tr().nothing_to_paste.to_owned()),
            })
        });
    }

    pub(crate) fn request_delete(&mut self, forever: bool) {
        let Some(c) = self.cur.as_ref().filter(|c| !c.clipboard) else { return };
        let path = c.path.clone();
        if forever || self.settings.confirm_delete {
            self.dialog = Some(Dialog::Delete { path, forever, dont_ask: false });
        } else {
            self.do_delete(path, false);
        }
    }

    /// 先从列表里拿掉并显示下一张，再在后台删除
    pub(crate) fn do_delete(&mut self, path: std::path::PathBuf, forever: bool) {
        if self.folder.current() == Some(path.as_path()) {
            self.view.xf = Default::default();
            self.folder.remove_current();
            self.loader.forget(&path);
            self.show_current();
        }
        self.spawn(move || {
            let r = if forever { std::fs::remove_file(&path).map_err(|e| e.to_string()) } else { crate::platform::trash(&path) };
            Some(Notice::Toast(match r {
                Ok(()) if forever => tr().deleted.to_owned(),
                Ok(()) if cfg!(target_os = "macos") => tr().trashed_mac.to_owned(),
                Ok(()) => tr().trashed.to_owned(),
                Err(e) => fill(tr().failed, &[&e]),
            }))
        });
    }

    pub(crate) fn save_as(&mut self) {
        let Some((img, xf)) = self.displayed_image() else { return };
        let stem = self.cur.as_ref().filter(|c| !c.clipboard).and_then(|c| c.path.file_stem()).map_or("image".into(), |s| s.to_string_lossy().into_owned());
        let mut d = rfd::FileDialog::new().set_file_name(format!("{stem}.png"));
        for f in Format::ALL {
            d = d.add_filter(f.name(), f.exts());
        }
        if let Some(dir) = self.cur.as_ref().filter(|c| !c.clipboard).and_then(|c| c.path.parent()) {
            d = d.set_directory(dir);
        }
        let Some(dest) = d.save_file() else { return };
        let fmt = Format::from_path(&dest).unwrap_or(Format::Png);
        let q = self.settings.jpeg_quality;
        self.spawn(move || {
            let img = save::apply_transform(&img, xf);
            Some(match save::write(&dest, &img, fmt, q) {
                Ok(()) => Notice::Saved(dest),
                Err(e) => Notice::Toast(fill(tr().failed, &[&e])),
            })
        });
    }

    pub(crate) fn save_rotation(&mut self) {
        let Some(c) = self.cur.as_ref().filter(|c| !c.clipboard) else { return };
        let (path, xf, q) = (c.path.clone(), self.view.xf, self.settings.jpeg_quality);
        self.spawn(move || {
            Some(match save::save_rotation(&path, xf, q) {
                Ok(()) => Notice::Saved(path),
                Err(e) => Notice::Toast(fill(tr().failed, &[&e])),
            })
        });
    }

    /// 关闭窗口前保存：在界面线程上做完再退出
    pub(crate) fn save_rotation_blocking(&mut self) {
        if let Some(c) = self.cur.as_ref().filter(|c| !c.clipboard)
            && let Err(e) = save::save_rotation(&c.path, self.view.xf, self.settings.jpeg_quality)
        {
            log::warn!("保存旋转失败：{e}");
        }
    }

    pub(crate) fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!self.fullscreen));
    }

    fn fit_zoom(&self) -> f32 {
        let Some(s) = self.shown else { return 1.0 };
        View::fit_zoom(FitMode::Fit, self.canvas.size(), self.ctx.pixels_per_point(), self.view.shown_size(s.width, s.height))
    }

    pub(crate) fn zoom_step(&mut self, zoom_in: bool) {
        let from = self.view.target_zoom();
        let to = if zoom_in { from * WHEEL_STEP * WHEEL_STEP } else { from / WHEEL_STEP / WHEEL_STEP };
        let to = snap(from, to, &[1.0, self.fit_zoom()]);
        self.view.zoom_to(to, self.canvas.center(), self.settings.smooth_zoom, self.canvas);
    }

    pub(crate) fn zoom_set(&mut self, z: f32) {
        self.view.zoom_to(z, self.canvas.center(), self.settings.smooth_zoom, self.canvas);
    }

    pub(crate) fn zoom_fit(&mut self) {
        if let Some(s) = self.shown {
            let size = self.view.shown_size(s.width, s.height);
            self.view.reset(FitMode::Fit, self.canvas.size(), self.ctx.pixels_per_point(), size);
        }
    }

    pub(crate) fn zoom_actual(&mut self) {
        self.zoom_set(1.0);
    }

    pub(crate) fn rotate(&mut self, clockwise: bool) {
        if self.shown.is_none() {
            return;
        }
        self.view.xf.rotate(clockwise);
        if self.view.fitted {
            self.zoom_fit();
        }
    }
}
