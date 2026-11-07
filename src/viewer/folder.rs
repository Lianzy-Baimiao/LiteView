//! 文件夹浏览列表：打开一张图后在后台扫描同目录下所有支持的格式，扫描不阻塞显示第一张。
//! 排序「跟随资源管理器」时，同时问正开着这个文件夹的资源管理器窗口，按它显示的先后排。

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

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

/// 浏览顺序（由设置决定，见 `Settings::order`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Order {
    pub key: SortKey,
    pub desc: bool,
    /// 跟随资源管理器：有资源管理器窗口开着这个文件夹就按它显示的先后，没有就按 key / desc（这时是文件名升序）
    pub explorer: bool,
}

/// 读完目录后最多再等资源管理器多久。它卡住时列表不能一直出不来（第一张图早已显示，不受影响）
const EXPLORER_WAIT: Duration = Duration::from_secs(1);

/// 后台扫描的结果
struct Scanned {
    files: Vec<PathBuf>,
    /// 按资源管理器窗口里的先后排好了
    from_explorer: bool,
    /// 这次是跟随资源管理器的扫描（问了，但可能没找到窗口）
    follow: bool,
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
    /// 后台扫描；bool = 是重新扫描（文件夹有变化、又从外面打开了这里的图），不是刚打开
    scan: Option<(Receiver<Scanned>, bool)>,
    /// 现在的先后来自资源管理器窗口：重新扫描时那个窗口不在了，就保持现在的先后，新文件放最后（不突然变回按文件名）
    explorer: bool,
}

impl Folder {
    pub fn empty() -> Self {
        Self { files: Vec::new(), index: 0, scan: None, explorer: false }
    }

    /// 只有这一张（扫描完成前、或从剪贴板打开）
    pub fn single(path: PathBuf) -> Self {
        Self { files: vec![path], index: 0, scan: None, explorer: false }
    }

    /// 打开一张图并在后台扫描它所在的文件夹；扫描完成时让界面重绘
    pub fn open(path: &Path, order: Order, ctx: Option<egui::Context>) -> Self {
        let mut f = Self::single(path.to_path_buf());
        f.start_scan(order, ctx, false);
        f
    }

    /// 文件夹有变化、或又从外面打开了这里的一张图：在后台重新扫描（正在扫描的作废）。跟随资源管理器时也重新问它的顺序
    pub fn rescan(&mut self, order: Order, ctx: Option<egui::Context>) {
        self.start_scan(order, ctx, true);
    }

    /// 换成固定的排序（设置里改的）：就地重排，当前图不变
    pub fn resort(&mut self, order: Order) {
        let cur = self.current().map(PathBuf::from);
        sort_files(&mut self.files, order.key);
        if order.desc {
            self.files.reverse();
        }
        self.explorer = false;
        if let Some(i) = cur.and_then(|c| self.files.iter().position(|p| *p == c)) {
            self.index = i;
        }
    }

    fn start_scan(&mut self, order: Order, ctx: Option<egui::Context>, rescan: bool) {
        let Some(dir) = self.dir().map(Path::to_path_buf) else { return };
        let current = self.current().map(Path::to_path_buf);
        let (tx, rx) = channel();
        self.scan = Some((rx, rescan));
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                let scanned = scan(&dir, order, current.as_deref());
                if tx.send(scanned).is_ok()
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
        let scanned = match rx.try_recv() {
            Ok(scanned) => scanned,
            Err(TryRecvError::Empty) => return Polled::Nothing,
            Err(TryRecvError::Disconnected) => {
                self.scan = None;
                return Polled::Nothing;
            }
        };
        self.scan = None;
        let files = self.arrange(scanned, rescan);
        self.merge(files, |p| !rescan || p.exists())
    }

    /// 定下扫描结果的先后：资源管理器给了顺序就用；跟随资源管理器、但重新扫描时窗口不在了，保持现在的先后
    fn arrange(&mut self, scanned: Scanned, rescan: bool) -> Vec<PathBuf> {
        let Scanned { mut files, from_explorer, follow } = scanned;
        if from_explorer {
            self.explorer = true;
        } else if follow && rescan && self.explorer {
            keep_order(&mut files, &self.files);
        } else {
            self.explorer = false;
        }
        files
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

/// current：当前显示的图（好几个资源管理器窗口开着这个文件夹时，选焦点在它上面的那个）
fn scan(dir: &Path, order: Order, current: Option<&Path>) -> Scanned {
    // 跟随资源管理器：在另一个线程里问它，同时读目录
    let asked = order.explorer.then(|| ask_explorer(dir, current));
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.path()).filter(|p| is_image(p)).collect(),
        Err(_) => Vec::new(),
    };
    sort_files(&mut files, order.key);
    if order.desc {
        files.reverse();
    }
    let waited = std::time::Instant::now();
    let names = asked.and_then(|rx| rx.recv_timeout(EXPLORER_WAIT).ok().flatten());
    if let Some(names) = &names {
        follow_explorer(&mut files, names);
        log::info!("按资源管理器的顺序：{} 张图（窗口里 {} 项），读完目录后又等了 {:.0} ms", files.len(), names.len(), waited.elapsed().as_secs_f64() * 1000.0);
    }
    Scanned { files, from_explorer: names.is_some(), follow: order.explorer }
}

/// 在另一个线程里问资源管理器（它卡住时这边等到 EXPLORER_WAIT 就不等了，那个线程问完自己结束）
fn ask_explorer(dir: &Path, current: Option<&Path>) -> Receiver<Option<Vec<OsString>>> {
    let (tx, rx) = channel();
    let (dir, current) = (dir.to_path_buf(), current.map(Path::to_path_buf));
    let _ = std::thread::Builder::new().name("explorer-order".into()).spawn(move || {
        let _ = tx.send(crate::platform::explorer_order(&dir, current.as_deref()));
    });
    rx
}

/// 比较文件名用（Windows 的文件名不分大小写）
fn name_key(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// 按资源管理器窗口里的先后排：窗口里有的在前、按窗口的顺序；没有的（被筛选掉、隐藏、刚加的）保持现在的顺序接在后面
fn follow_explorer(files: &mut [PathBuf], names: &[OsString]) {
    let mut rank = HashMap::with_capacity(names.len());
    for (i, n) in names.iter().enumerate() {
        rank.entry(n.to_string_lossy().to_lowercase()).or_insert(i);
    }
    files.sort_by_cached_key(|p| rank.get(&name_key(p)).copied().unwrap_or(usize::MAX));
}

/// 重新扫描时找不到资源管理器窗口了：已有的图保持原来的先后，新文件放最后
fn keep_order(files: &mut [PathBuf], before: &[PathBuf]) {
    let rank: HashMap<String, usize> = before.iter().enumerate().map(|(i, p)| (name_key(p), i)).collect();
    files.sort_by_cached_key(|p| rank.get(&name_key(p)).copied().unwrap_or(usize::MAX));
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
        Folder { files: (0..n).map(|i| PathBuf::from(format!("/d/{i}.png"))).collect(), index, scan: None, explorer: false }
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| PathBuf::from(format!("/d/{n}"))).collect()
    }

    fn names(files: &[PathBuf]) -> Vec<String> {
        files.iter().map(|p| crate::natsort::file_name(p)).collect()
    }

    fn scanned(names: &[&str], from_explorer: bool, follow: bool) -> Scanned {
        Scanned { files: paths(names), from_explorer, follow }
    }

    #[test]
    fn follows_the_explorer_window_and_keeps_the_rest_after_it() {
        // 窗口里按修改时间倒序，还有文件夹、非图片；c.png 被筛掉了（不在窗口里），排在后面且保持原来的文件名顺序
        let mut files = paths(&["a.png", "b.jpg", "c.png", "d.gif", "e.webp"]);
        let shown: Vec<OsString> = ["sub", "E.WEBP", "notes.txt", "b.jpg", "d.gif", "a.png"].iter().map(OsString::from).collect();
        follow_explorer(&mut files, &shown);
        assert_eq!(names(&files), ["e.webp", "b.jpg", "d.gif", "a.png", "c.png"]);
        // 窗口里一张图都没有（比如还没列出来）：照原样
        let mut same = paths(&["a.png", "b.png"]);
        follow_explorer(&mut same, &[]);
        assert_eq!(names(&same), ["a.png", "b.png"]);
    }

    #[test]
    fn rescan_without_the_window_keeps_the_explorer_order() {
        // 原来按资源管理器排的是 c、a、b；窗口关了以后 b 被删、加了 d：c、a 照旧，d 放最后
        let mut f = Folder { files: paths(&["c.png", "a.png", "b.png"]), index: 1, scan: None, explorer: true };
        let files = f.arrange(scanned(&["a.png", "c.png", "d.png"], false, true), true);
        assert_eq!(f.merge(files, |_| true), Polled::Changed);
        assert_eq!(names(&f.files), ["c.png", "a.png", "d.png"]);
        assert_eq!(f.current(), Some(Path::new("/d/a.png")));
        assert!(f.explorer);
        // 窗口又在了：用它新的顺序
        let files = f.arrange(scanned(&["d.png", "a.png", "c.png"], true, true), true);
        f.merge(files, |_| true);
        assert_eq!(names(&f.files), ["d.png", "a.png", "c.png"]);
        // 刚打开（不是重新扫描）、没有窗口：按文件名，不再算资源管理器的顺序
        let mut g = Folder { explorer: true, ..Folder::single(PathBuf::from("/d/b.png")) };
        let files = g.arrange(scanned(&["a.png", "b.png"], false, true), false);
        g.merge(files, |_| true);
        assert!(!g.explorer);
        assert_eq!((names(&g.files), g.index), (vec!["a.png".to_owned(), "b.png".to_owned()], 1));
        // 固定排序的重新扫描：不保持资源管理器的顺序
        let mut h = Folder { files: paths(&["b.png", "a.png"]), index: 0, scan: None, explorer: true };
        let files = h.arrange(scanned(&["a.png", "b.png"], false, false), true);
        h.merge(files, |_| true);
        assert!(!h.explorer);
        assert_eq!(names(&h.files), ["a.png", "b.png"]);
    }

    #[test]
    fn resort_to_a_fixed_order_keeps_the_current_image() {
        let mut f = Folder { files: paths(&["c.png", "a.png", "b.png"]), index: 0, scan: None, explorer: true };
        f.resort(Order { key: SortKey::Name, desc: true, explorer: false });
        assert!(!f.explorer);
        assert_eq!(f.current(), Some(Path::new("/d/c.png")));
        assert_eq!(f.index, 0);
        f.resort(Order { key: SortKey::Name, desc: false, explorer: false });
        assert_eq!(f.index, 2);
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
