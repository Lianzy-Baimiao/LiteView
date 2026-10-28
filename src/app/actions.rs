//! 复制、粘贴、删除、另存为、保存旋转、打开文件、缩放与旋转。耗时的都在后台线程做。

use std::sync::Arc;

use super::{App, Dialog, Notice, PendingAction};
use crate::i18n::{fill, tr};
use crate::save::{self, Format};
use crate::settings::FitMode;
use crate::viewer::view::{View, WHEEL_STEP, snap};

impl App {
    /// 复制一张图到剪贴板（后台），完成后轻提示
    pub(crate) fn copy_rgba(&self, img: image::RgbaImage) {
        self.spawn(move || {
            Some(Notice::Toast(match crate::clipboard::set_image(img) {
                Ok(()) => tr().copied.to_owned(),
                Err(e) => fill(tr().failed, &[&e]),
            }))
        });
    }

    /// 编辑时 Ctrl+V：剪贴板里的图片 / 复制的图片文件 → 浮动选区
    pub(crate) fn edit_paste(&mut self) {
        let opts = super::decode_options(&self.settings);
        self.spawn(move || {
            let file = |p: &std::path::Path| crate::decode::decode_file(p, &opts).ok().map(|d| Notice::EditPaste(Box::new(d.first_image().as_ref().clone())));
            Some(match crate::clipboard::get() {
                crate::clipboard::Content::Image(img) => Notice::EditPaste(Box::new(img)),
                crate::clipboard::Content::Files(files) => files.iter().find_map(|p| file(p)).unwrap_or(Notice::Toast(tr().nothing_to_paste.to_owned())),
                crate::clipboard::Content::Path(p) => file(&p).unwrap_or(Notice::Toast(tr().nothing_to_paste.to_owned())),
                crate::clipboard::Content::Nothing => Notice::Toast(tr().nothing_to_paste.to_owned()),
            })
        });
    }

    /// 编辑时拖进来的图片文件 → 浮动选区
    pub(crate) fn paste_file(&mut self, p: std::path::PathBuf) {
        let opts = super::decode_options(&self.settings);
        self.spawn(move || {
            Some(match crate::decode::decode_file(&p, &opts) {
                Ok(d) => Notice::EditPaste(Box::new(d.first_image().as_ref().clone())),
                Err(_) => Notice::Toast(tr().cannot_open.to_owned()),
            })
        });
    }

    pub(crate) fn spawn(&self, f: impl FnOnce() -> Option<Notice> + Send + 'static) {
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

    /// 在调用时固定帧号；真正取像素在后台，拖动进度条后不会错导出缓存末帧。
    fn displayed_image(&self) -> Option<(Arc<crate::decode::Decoded>, usize, crate::viewer::view::Transform)> {
        Some((self.decoded()?.clone(), self.play.as_ref().map_or(0, |p| p.index), self.view.xf))
    }

    /// 只显示着缩略图：先解码原图，到了再做这件事；返回 true = 已推迟
    fn defer_until_full(&mut self, action: PendingAction) -> bool {
        if self.decoded().is_some() || !self.cur.as_ref().is_some_and(|c| c.preview.is_some()) {
            return false;
        }
        self.pending_action = Some(action);
        self.want_full();
        true
    }

    pub(crate) fn copy_image(&mut self) {
        if self.defer_until_full(PendingAction::Copy) {
            return;
        }
        let Some((decoded, index, xf)) = self.displayed_image() else { return };
        self.spawn(move || {
            let img = match decoded.content.image_at(index) {
                Ok(img) => save::apply_transform(&img, xf),
                Err(e) => {
                    log::warn!("取动图帧失败：{e:?}");
                    return Some(Notice::Toast(tr().err_corrupt.to_owned()));
                }
            };
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

    /// 另存为；动图是导出当前帧（默认 PNG，文件名带帧号）
    pub(crate) fn save_as(&mut self) {
        if self.defer_until_full(PendingAction::SaveAs) {
            return;
        }
        let Some((decoded, index, xf)) = self.displayed_image() else { return };
        let stem = self.cur.as_ref().filter(|c| !c.clipboard).and_then(|c| c.path.file_stem()).map_or("image".into(), |s| s.to_string_lossy().into_owned());
        let name = match &self.play {
            Some(p) if self.is_animated() => format!("{stem}-{}.png", p.index + 1),
            _ => format!("{stem}.png"),
        };
        let mut d = rfd::FileDialog::new().set_file_name(name);
        for f in Format::ALL {
            d = d.add_filter(f.name(), f.exts());
        }
        if let Some(dir) = self.cur.as_ref().filter(|c| !c.clipboard).and_then(|c| c.path.parent()) {
            d = d.set_directory(dir);
        }
        let Some(dest) = d.save_file() else { return };
        let fmt = Format::from_path(&dest).unwrap_or(Format::Png);
        let q = if fmt == Format::WebP { self.settings.webp_quality } else { self.settings.jpeg_quality };
        let webp_lossless = self.settings.webp_lossless;
        self.spawn(move || {
            let img = match decoded.content.image_at(index) {
                Ok(img) => save::apply_transform(&img, xf),
                Err(e) => {
                    log::warn!("取动图帧失败：{e:?}");
                    return Some(Notice::Toast(tr().err_corrupt.to_owned()));
                }
            };
            Some(match save::write_with_mode(&dest, &img, fmt, q, &save::Meta::default(), webp_lossless) {
                Ok(()) => Notice::Saved(dest),
                Err(e) => Notice::Toast(fill(tr().failed, &[&e])),
            })
        });
    }

    /// 设为桌面背景：按显示方向存一份到配置目录（原图被移走、删除后背景还在；HEIC 等格式也能用）
    pub(crate) fn set_wallpaper(&mut self) {
        if self.defer_until_full(PendingAction::Wallpaper) {
            return;
        }
        let Some((decoded, index, xf)) = self.displayed_image() else { return };
        self.spawn(move || {
            let img = match decoded.content.image_at(index) {
                Ok(img) => save::apply_transform(&img, xf),
                Err(e) => {
                    log::warn!("取动图帧失败：{e:?}");
                    return Some(Notice::Toast(tr().err_corrupt.to_owned()));
                }
            };
            let dir = crate::settings::config_dir();
            let _ = std::fs::create_dir_all(&dir);
            let opaque = img.pixels().all(|p| p.0[3] == 255);
            let (path, fmt) = if opaque { (dir.join("wallpaper.jpg"), Format::Jpeg) } else { (dir.join("wallpaper.png"), Format::Png) };
            let r = save::write(&path, &img, fmt, 95).and_then(|()| crate::platform::set_wallpaper(&path));
            Some(Notice::Toast(match r {
                Ok(()) => tr().wallpaper_set.to_owned(),
                Err(e) => fill(tr().failed, &[&e]),
            }))
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
        View::fit_zoom(FitMode::Fit, self.fit_area(), self.ctx.pixels_per_point(), self.view.shown_size(s.width, s.height))
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
            self.view.reset(FitMode::Fit, self.fit_area(), self.ctx.pixels_per_point(), size);
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
