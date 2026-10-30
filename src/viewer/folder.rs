//! 文件夹浏览列表：打开一张图后在后台扫描同目录下所有支持的格式，扫描不阻塞显示第一张。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crate::settings::SortKey;

/// 浏览列表认为是图片的扩展名（打开时仍按文件内容识别格式）
pub const EXTS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "bmp", "dib", "webp", "ico", "cur", "tif", "tiff", "svg", "svgz", "avif",
    "heic", "heif", "hif", "jxl", "jxr", "wdp", "hdp", "tga", "dds", "qoi", "pnm", "pbm", "pgm", "ppm", "pam", "hdr", "exr", "ff",
    "psd",
];

/// 只有 Windows 能解的（WebM 表情包用系统的 VP9 解码器）
pub const WINDOWS_EXTS: &[&str] = &["webm"];

/// 这个系统上能打开的扩展名：EXTS、RAW，Windows 上再加 WINDOWS_EXTS
pub fn all_exts() -> Vec<&'static str> {
    let mut v: Vec<&str> = EXTS.to_vec();
    v.extend_from_slice(crate::decode::sniff::RAW_EXTS);
    if cfg!(windows) {
        v.extend_from_slice(WINDOWS_EXTS);
    }
    v
}

pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        let e = e.to_ascii_lowercase();
        EXTS.contains(&e.as_str()) || crate::decode::sniff::RAW_EXTS.contains(&e.as_str()) || (cfg!(windows) && WINDOWS_EXTS.contains(&e.as_str()))
    })
}

/// 扫描结果合并后的变化
#[derive(Debug, PartialEq, Eq)]
pub enum Polled {
    Nothing,
    Changed,
    /// 重新扫描时发现当前图已经不在了（被删除或移走）：index 指向原位置上的下一张
    CurrentGone,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Moved,
    /// 越过了末尾 / 开头，回到另一头
    Wrapped,
    AtEdge,
    Single,
}

pub struct Folder {
    pub files: Vec<PathBuf>,
    pub index: usize,
    /// 后台扫描；bool = 是重新扫描（文件夹有变化），不是刚打开
    scan: Option<(Receiver<Vec<PathBuf>>, bool)>,
}

impl Folder {
    pub fn empty() -> Self {
        Self { files: Vec::new(), index: 0, scan: None }
    }

    /// 只有这一张（扫描完成前、或从剪贴板打开）
    pub fn single(path: PathBuf) -> Self {
        Self { files: vec![path], index: 0, scan: None }
    }

    /// 打开一张图并在后台扫描它所在的文件夹；扫描完成时让界面重绘
    pub fn open(path: &Path, sort: SortKey, desc: bool, ctx: Option<egui::Context>) -> Self {
        let mut f = Self::single(path.to_path_buf());
        f.start_scan(sort, desc, ctx, false);
        f
    }

    /// 文件夹有变化：在后台重新扫描（正在扫描的作废）
    pub fn rescan(&mut self, sort: SortKey, desc: bool, ctx: Option<egui::Context>) {
        self.start_scan(sort, desc, ctx, true);
    }

    fn start_scan(&mut self, sort: SortKey, desc: bool, ctx: Option<egui::Context>, rescan: bool) {
        let Some(dir) = self.dir().map(Path::to_path_buf) else { return };
        let (tx, rx) = channel();
        self.scan = Some((rx, rescan));
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                let files = scan(&dir, sort, desc);
                if tx.send(files).is_ok()
                    && let Some(ctx) = ctx
                {
                    ctx.request_repaint();
                }
            })
            .ok();
    }

    /// 当前图所在的文件夹
    pub fn dir(&self) -> Option<&Path> {
        self.current()?.parent().filter(|d| !d.as_os_str().is_empty())
    }

    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// 扫描完成后合并到列表
    pub fn poll(&mut self) -> Polled {
        let Some((rx, rescan)) = &self.scan else { return Polled::Nothing };
        let rescan = *rescan;
        let files = match rx.try_recv() {
            Ok(files) => files,
            Err(TryRecvError::Empty) => return Polled::Nothing,
            Err(TryRecvError::Disconnected) => {
                self.scan = None;
                return Polled::Nothing;
            }
        };
        self.scan = None;
        self.merge(files, |p| !rescan || p.exists())
    }

    /// exists：当前图还在不在（刚打开时一定在）
    fn merge(&mut self, mut files: Vec<PathBuf>, exists: impl Fn(&Path) -> bool) -> Polled {
        let Some(cur) = self.files.get(self.index).cloned() else {
            self.files = files;
            return Polled::Changed;
        };
        if let Some(i) = files.iter().position(|p| same_path(p, &cur)) {
            self.index = i;
        } else if exists(&cur) {
            // 扩展名不在列表里、但能打开的图：放在最前面
            files.insert(0, cur);
            self.index = 0;
        } else {
            self.files = files;
            self.index = self.index.min(self.files.len().saturating_sub(1));
            return Polled::CurrentGone;
        }
        self.files = files;
        Polled::Changed
    }

    pub fn current(&self) -> Option<&Path> {
        self.files.get(self.index).map(PathBuf::as_path)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn step(&mut self, delta: isize, wrap: bool) -> Step {
        let n = self.files.len();
        if n <= 1 {
            return Step::Single;
        }
        let target = self.index as isize + delta;
        if (0..n as isize).contains(&target) {
            self.index = target as usize;
            return Step::Moved;
        }
        if !wrap {
            return Step::AtEdge;
        }
        self.index = target.rem_euclid(n as isize) as usize;
        Step::Wrapped
    }

    pub fn go(&mut self, index: usize) {
        if !self.files.is_empty() {
            self.index = index.min(self.files.len() - 1);
        }
    }

    /// 当前文件被删除或移走：显示下一张（删的是最后一张时显示前一张）
    pub fn remove_current(&mut self) {
        if self.index < self.files.len() {
            self.files.remove(self.index);
        }
        if self.index >= self.files.len() {
            self.index = self.files.len().saturating_sub(1);
        }
    }

    /// 预读顺序：下一张、上一张、下两张……
    pub fn neighbors(&self, n: usize) -> Vec<PathBuf> {
        let len = self.files.len();
        let mut out = Vec::new();
        if len <= 1 {
            return out;
        }
        for k in 1..=n as isize {
            for d in [k, -k] {
                let i = (self.index as isize + d).rem_euclid(len as isize) as usize;
                if i != self.index && !out.contains(&self.files[i]) {
                    out.push(self.files[i].clone());
                }
            }
        }
        out
    }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    a == b || (cfg!(windows) && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase())
}

fn scan(dir: &Path, sort: SortKey, desc: bool) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| is_image(p))
        .collect();
    sort_files(&mut files, sort);
    if desc {
        files.reverse();
    }
    files
}

/// 先按文件名自然排序，再按所选的键稳定排序（键相同时保持文件名顺序）
pub fn sort_files(files: &mut [PathBuf], sort: SortKey) {
    crate::natsort::sort_paths(files);
    match sort {
        SortKey::Name => {}
        SortKey::Modified => files.sort_by_cached_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()),
        SortKey::Size => files.sort_by_cached_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)),
        SortKey::Type => files.sort_by_cached_key(|p| p.extension().map(|e| e.to_string_lossy().to_lowercase())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(n: usize, index: usize) -> Folder {
        Folder { files: (0..n).map(|i| PathBuf::from(format!("/d/{i}.png"))).collect(), index, scan: None }
    }

    #[test]
    fn stepping_and_wrapping() {
        let mut f = folder(3, 2);
        assert_eq!(f.step(1, false), Step::AtEdge);
        assert_eq!(f.step(1, true), Step::Wrapped);
        assert_eq!(f.index, 0);
        assert_eq!(f.step(-1, true), Step::Wrapped);
        assert_eq!(f.index, 2);
        assert_eq!(f.step(-1, true), Step::Moved);
        assert_eq!(folder(1, 0).step(1, true), Step::Single);
    }

    #[test]
    fn remove_and_neighbors() {
        let mut f = folder(4, 3);
        f.remove_current();
        assert_eq!(f.index, 2);
        assert_eq!(f.neighbors(1), vec![PathBuf::from("/d/0.png"), PathBuf::from("/d/1.png")]);
        let mut g = folder(1, 0);
        g.remove_current();
        assert!(g.is_empty() && g.current().is_none());
    }

    #[test]
    fn merge_keeps_current() {
        let mut f = Folder::single(PathBuf::from("/d/2.png"));
        assert_eq!(f.merge(folder(4, 0).files, |_| true), Polled::Changed);
        assert_eq!((f.index, f.len()), (2, 4));
        let mut g = Folder::single(PathBuf::from("/d/odd.bin"));
        g.merge(folder(2, 0).files, |_| true);
        assert_eq!((g.index, g.len()), (0, 3));
    }

    #[test]
    fn rescan_notices_removed_current() {
        let mut f = folder(4, 2);
        let mut files = folder(4, 0).files;
        files.remove(2);
        assert_eq!(f.merge(files, |_| false), Polled::CurrentGone);
        // 原位置上的下一张
        assert_eq!(f.current(), Some(Path::new("/d/3.png")));
        let mut g = folder(2, 1);
        assert_eq!(g.merge(vec![PathBuf::from("/d/0.png")], |_| false), Polled::CurrentGone);
        assert_eq!(g.index, 0);
    }
}
