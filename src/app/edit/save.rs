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
    pub keep_meta: bool,
    pub then: SaveThen,
}

pub(crate) struct SaveDone {
    pub dest: PathBuf,
    pub rev: u64,
    pub then: SaveThen,
    pub result: Result<Arc<Decoded>, String>,
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
        let job = SaveJob { dest: ed.src.path.clone(), format, quality: self.settings.jpeg_quality, keep_meta, then };
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
        let job = SaveJob { dest, format, quality: self.settings.jpeg_quality, keep_meta, then };
        if format == Format::Jpeg || (ed.src.exif.is_some() && matches!(format, Format::Jpeg | Format::Png | Format::WebP)) {
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
        let meta = save::Meta { exif, icc: ed.src.icc.as_deref().cloned() };
        let rev = ed.doc.rev;
        let opts = crate::app::decode_options(&self.settings);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = save::write_with(&job.dest, &img, job.format, job.quality, &meta).map(|()| {
                // 读回保存的文件（JPEG 的压缩、元数据都与文件一致）
                let d = crate::decode::decode_file(&job.dest, &opts).unwrap_or_else(|_| crate::decode::from_rgba((*img).clone(), job.format.name()));
                Arc::new(d)
            });
            let _ = tx.send(Notice::EditSaved(Box::new(SaveDone { dest: job.dest, rev, then: job.then, result: r })));
            ctx.request_repaint();
        });
    }

    /// 保存完成
    pub(crate) fn edit_saved(&mut self, ed: &mut Editor, done: SaveDone) {
        ed.saving = false;
        let d = match done.result {
            Ok(d) => d,
            Err(e) => {
                ed.after = None;
                self.show_toast(fill(tr().failed, &[&e]));
                return;
            }
        };
        ed.doc.mark_saved(done.rev);
        ed.saved_once = true;
        ed.overwrite_ok = true;
        self.self_saved.push((done.dest.clone(), std::time::Instant::now()));
        self.loader.insert(&done.dest, Ok(d.clone()));
        if ed.src.clipboard || done.dest != ed.src.path {
            ed.src.path = done.dest.clone();
            ed.src.clipboard = false;
            self.folder = crate::viewer::folder::Folder::open(&done.dest, self.settings.sort, self.settings.sort_desc, Some(self.ctx.clone()));
        }
        ed.src.format = d.info.format;
        ed.src.file_size = d.info.file_size;
        ed.src.exif = d.info.exif.clone();
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
