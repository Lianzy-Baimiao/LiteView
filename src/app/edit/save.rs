//! 保存（PRD 6.9）：Ctrl+S 覆盖原图（第一次先确认；原格式不能写入或来自剪贴板时改为另存为）；
//! 都在后台写临时文件再替换原文件。打过码的图默认去掉 EXIF；颜色配置文件保留。

use std::path::PathBuf;
use std::sync::Arc;

use super::Editor;
use crate::app::{App, Dialog, Notice};
use crate::decode::Decoded;
use crate::i18n::{fill, tr};
use crate::save::{self, Format};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SaveThen {
    Stay,
    /// 保存后回到查看
    Exit,
}

/// 要保存的内容与方式
pub(crate) struct SaveJob {
    pub dest: PathBuf,
    pub format: Format,
    pub quality: u8,
    pub webp_lossless: bool,
    pub keep_meta: bool,
    pub then: SaveThen,
}

pub(crate) struct SaveDone {
    pub dest: PathBuf,
    pub rev: u64,
    pub then: SaveThen,
    pub result: SaveResult,
}

type SaveResult = Result<Result<Arc<Decoded>, crate::decode::Error>, String>;

fn write_and_verify(img: &image::RgbaImage, job: &SaveJob, meta: &save::Meta, opts: &crate::decode::Options) -> SaveResult {
    save::write_with_mode(&job.dest, img, job.format, job.quality, meta, job.webp_lossless)?;
    Ok(crate::decode::decode_file(&job.dest, opts).map(Arc::new))
}

fn record_saved(ed: &mut Editor, decoded: &Decoded, rev: u64) {
    ed.doc.mark_saved(rev);
    ed.saved_once = true;
    ed.overwrite_ok = true;
    ed.src.format = decoded.info.format;
    ed.src.file_size = decoded.info.file_size;
    ed.src.jpeg_quality = decoded.info.jpeg_quality;
    ed.src.exif = decoded.info.exif.clone();
}

impl App {
    /// Ctrl+S /「完成」：覆盖原图
    pub(crate) fn edit_save(&mut self, ed: &mut Editor, then: SaveThen) {
        if ed.saving {
            return;
        }
        self.edit_finalize(ed);
        if !ed.modified() && !ed.src.clipboard {
            if then == SaveThen::Exit {
                ed.leave = true;
            }
            return;
        }
        let fmt = Format::from_path(&ed.src.path).filter(|_| !ed.src.clipboard && writable(ed.src.format));
        let Some(format) = fmt else {
            self.edit_save_as(ed, then);
            return;
        };
        if self.settings.confirm_overwrite && !ed.overwrite_ok {
            self.dialog = Some(Dialog::Overwrite { then, dont_ask: false });
            return;
        }
        let keep_meta = self.settings.keep_metadata && !ed.doc.redacted();
        let quality = if format == Format::WebP { self.settings.webp_quality } else { overwrite_quality(format, ed.src.jpeg_quality, self.settings.jpeg_quality) };
        let job = SaveJob { dest: ed.src.path.clone(), format, quality, webp_lossless: self.settings.webp_lossless, keep_meta, then };
        self.edit_start_save(ed, job);
    }

    /// 另存为：系统保存对话框，之后按格式问质量、元数据
    pub(crate) fn edit_save_as(&mut self, ed: &mut Editor, then: SaveThen) {
        if ed.saving {
            return;
        }
        self.edit_finalize(ed);
        let stem = if ed.src.clipboard { "image".to_owned() } else { ed.src.path.file_stem().map_or("image".into(), |s| s.to_string_lossy().into_owned()) };
        let first = Format::from_path(&ed.src.path).filter(|_| !ed.src.clipboard && writable(ed.src.format)).unwrap_or(Format::Png);
        let mut d = rfd::FileDialog::new().set_file_name(format!("{stem}.{}", first.exts()[0]));
        d = d.add_filter(first.name(), first.exts());
        for f in Format::ALL.into_iter().filter(|f| *f != first) {
            d = d.add_filter(f.name(), f.exts());
        }
        if !ed.src.clipboard
            && let Some(dir) = ed.src.path.parent()
        {
            d = d.set_directory(dir);
        }
        let Some(dest) = d.save_file() else {
            ed.after = None;
            return;
        };
        let format = Format::from_path(&dest).unwrap_or(Format::Png);
        let keep_meta = self.settings.keep_metadata && !ed.doc.redacted() && ed.src.exif.is_some();
        let quality = if format == Format::WebP { self.settings.webp_quality } else { self.settings.jpeg_quality };
        let job = SaveJob { dest, format, quality, webp_lossless: self.settings.webp_lossless, keep_meta, then };
        if matches!(format, Format::Jpeg | Format::WebP) || ed.src.exif.is_some() || (ed.src.canvas_icc.is_some() && !format.supports_icc()) || !format.full_alpha() {
            ed.save_options = Some(job);
        } else {
            self.edit_start_save(ed, job);
        }
    }

    /// 在后台编码、写入，再读回来给查看用
    pub(crate) fn edit_start_save(&mut self, ed: &mut Editor, job: SaveJob) {
        ed.saving = true;
        let img = ed.doc.image.clone();
        let exif = if job.keep_meta { ed.src.exif.as_deref().and_then(|e| save::clean_exif(e)) } else { None };
        let meta = save::Meta { exif, icc: ed.src.canvas_icc.as_deref().cloned() };
        let rev = ed.doc.rev;
        let opts = crate::app::decode_options(&self.settings);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = write_and_verify(&img, &job, &meta, &opts);
            let _ = tx.send(Notice::EditSaved(Box::new(SaveDone { dest: job.dest, rev, then: job.then, result: r })));
            ctx.request_repaint();
        });
    }

    /// 保存完成
    pub(crate) fn edit_saved(&mut self, ed: &mut Editor, done: SaveDone) {
        ed.saving = false;
        let d = match done.result {
            Ok(Ok(d)) => d,
            Ok(Err(e)) => {
                ed.after = None;
                self.loader.forget(&done.dest);
                log::warn!("Saved {}, but read-back verification failed: {e:?}", done.dest.display());
                self.show_toast(fill(tr().saved_unverified, &[&done.dest.display()]));
                return;
            }
            Err(e) => {
                ed.after = None;
                self.show_toast(fill(tr().failed, &[&e]));
                return;
            }
        };
        record_saved(ed, &d, done.rev);
        self.self_saved.push((done.dest.clone(), std::time::Instant::now()));
        self.loader.insert(&done.dest, Ok(d.clone()));
        if ed.src.clipboard || done.dest != ed.src.path {
            ed.src.path = done.dest.clone();
            ed.src.clipboard = false;
            self.folder = crate::viewer::folder::Folder::open(&done.dest, self.settings.sort, self.settings.sort_desc, Some(self.ctx.clone()));
        }
        // Preserve the canvas and its color/alpha semantics, including undo history.
        // cur.loaded stores the actual flattened/quantized file and its metadata.
        self.info = None;
        if let Some(c) = &mut self.cur {
            c.path = done.dest;
            c.clipboard = false;
            c.loaded = Some(Ok(d));
        }
        self.show_toast(tr().saved);
        if done.then == SaveThen::Exit {
            ed.leave = true;
        }
    }
}

/// 原格式能不能直接写回（HEIC、RAW、SVG、PSD 等只能另存为）
fn writable(format: &str) -> bool {
    matches!(format, "PNG" | "JPEG" | "WebP" | "BMP" | "GIF" | "TIFF")
}

/// 只影响覆盖保存；另存为继续采用用户选择的品质。
fn overwrite_quality(format: Format, original: Option<u8>, fallback: u8) -> u8 {
    if format == Format::Jpeg { original.unwrap_or(fallback) } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_jpeg_does_not_destroy_canvas_alpha_profile_or_undo() {
        let src = image::RgbaImage::from_pixel(8, 6, image::Rgba([20, 40, 60, 128]));
        let mut doc = crate::editor::history::Doc::new(src.clone(), 100, 1 << 20);
        doc.replace(image::RgbaImage::from_pixel(8, 6, image::Rgba([80, 40, 60, 128])));
        let profile = Arc::new(b"canvas-profile".to_vec());
        let source = super::super::Source {
            path: "input.png".into(), clipboard: false, format: "PNG", file_size: 0,
            jpeg_quality: None, exif: None, canvas_icc: Some(profile.clone()), canvas_alpha: true,
        };
        let mut ed = Editor::new(doc, source, super::super::Tool::Select, Default::default());
        let bytes = save::encode(&ed.doc.image, Format::Jpeg, 90).unwrap();
        let saved = crate::decode::decode_bytes(&bytes, "jpg", &Default::default()).unwrap();
        let rev = ed.doc.rev;
        record_saved(&mut ed, &saved, rev);
        assert!(!saved.info.has_alpha);
        assert!(saved.info.icc.is_none());
        assert!(ed.src.canvas_alpha);
        assert_eq!(ed.src.canvas_icc, Some(profile));
        assert_eq!(ed.doc.image.get_pixel(0, 0)[3], 128);
        let png = save::encode(&ed.doc.image, Format::Png, 90).unwrap();
        let png = crate::decode::decode_bytes(&png, "png", &Default::default()).unwrap();
        assert_eq!(png.first_image().as_ref(), ed.doc.image.as_ref());
        assert!(ed.doc.undo());
        assert_eq!(ed.doc.image.as_ref(), &src);
        assert!(ed.doc.modified());
    }

    #[test]
    fn successful_write_and_failed_verification_are_distinct() {
        let dir = std::env::temp_dir().join(format!("liteview-verify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let job = SaveJob { dest: dir.join("output.png"), format: Format::Png, quality: 90, webp_lossless: true, keep_meta: false, then: SaveThen::Stay };
        let image = image::RgbaImage::new(8, 6);
        let opts = crate::decode::Options { max_source: 0, ..Default::default() };
        let result = write_and_verify(&image, &job, &save::Meta::default(), &opts);
        assert!(matches!(result, Ok(Err(crate::decode::Error::TooLarge { .. }))));
        assert!(crate::decode::decode_file(&job.dest, &Default::default()).is_ok());
        std::fs::remove_file(&job.dest).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn jpeg_overwrite_preserves_detected_quality_instead_of_using_default() {
        let img = image::RgbaImage::from_fn(16, 16, |x, y| image::Rgba([(x * 15) as u8, (y * 15) as u8, 91, 255]));
        for original in [1, 35, 50, 75, 95, 100] {
            let bytes = save::encode(&img, Format::Jpeg, original).unwrap();
            let decoded = crate::decode::decode_bytes(&bytes, "jpg", &Default::default()).unwrap();
            let quality = overwrite_quality(Format::Jpeg, decoded.info.jpeg_quality, 90);
            let saved = save::encode(decoded.first_image(), Format::Jpeg, quality).unwrap();
            let reread = crate::decode::decode_bytes(&saved, "jpg", &Default::default()).unwrap();
            assert_eq!(reread.info.jpeg_quality, Some(original));
        }
        assert_eq!(overwrite_quality(Format::Jpeg, None, 83), 83);
        assert_eq!(overwrite_quality(Format::Png, Some(35), 83), 83);
        let png = save::encode(&img, Format::Png, 90).unwrap();
        assert_eq!(crate::decode::decode_bytes(&png, "png", &Default::default()).unwrap().info.jpeg_quality, None);
    }
}
