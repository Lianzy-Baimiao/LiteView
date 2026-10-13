//! 监听当前文件夹：新增、删除、改名后刷新浏览列表；图片被外部修改后丢掉缓存（当前图重新载入）。
//! 事件常常一下来一串（复制大文件、保存时先写临时文件再替换），攒 300 ms 没有新事件再处理。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

const SETTLE: Duration = Duration::from_millis(300);

#[derive(Default, Debug, PartialEq)]
pub struct Changes {
    /// 文件夹里的图片有增删改名：重新扫描
    pub list: bool,
    /// 内容可能变了的图片（含被删除、改名的）
    pub paths: Vec<PathBuf>,
}

pub struct Watch {
    pub dir: PathBuf,
    _watcher: notify::RecommendedWatcher,
    rx: Receiver<notify::Event>,
    pending: Changes,
    last: Option<Instant>,
}

impl Watch {
    /// 开始监听 dir（不含子文件夹）；有事件时让界面重绘
    pub fn new(dir: &Path, ctx: egui::Context) -> Option<Self> {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |r: notify::Result<notify::Event>| {
            if let Ok(e) = r
                && tx.send(e).is_ok()
            {
                ctx.request_repaint_after(SETTLE);
            }
        })
        .map_err(|e| log::warn!("无法监听文件夹：{e}"))
        .ok()?;
        watcher.watch(dir, RecursiveMode::NonRecursive).map_err(|e| log::warn!("无法监听 {}：{e}", dir.display())).ok()?;
        Some(Self { dir: dir.to_path_buf(), _watcher: watcher, rx, pending: Changes::default(), last: None })
    }

    /// 事件停下 300 ms 后返回攒下的变化；还没停下时返回 None 并安排重绘
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<Changes> {
        while let Ok(e) = self.rx.try_recv() {
            if classify(&e, &mut self.pending) {
                self.last = Some(Instant::now());
            }
        }
        let last = self.last?;
        let wait = SETTLE.saturating_sub(last.elapsed());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return None;
        }
        self.last = None;
        Some(std::mem::take(&mut self.pending))
    }
}

/// 记下一个事件；返回是否与图片有关
fn classify(e: &notify::Event, out: &mut Changes) -> bool {
    let images: Vec<&PathBuf> = e.paths.iter().filter(|p| super::folder::is_image(p)).collect();
    if images.is_empty() {
        return false;
    }
    match e.kind {
        // 只改了属性、访问时间：不管
        EventKind::Access(_) | EventKind::Modify(notify::event::ModifyKind::Metadata(_)) => return false,
        EventKind::Modify(notify::event::ModifyKind::Data(_)) => {}
        // 新建、删除、改名（保存时用临时文件替换原文件也是改名）
        _ => out.list = true,
    }
    for p in images {
        if !out.paths.contains(p) {
            out.paths.push(p.clone());
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, ModifyKind, RenameMode};

    fn ev(kind: EventKind, paths: &[&str]) -> notify::Event {
        let mut e = notify::Event::new(kind);
        for p in paths {
            e = e.add_path(PathBuf::from(p));
        }
        e
    }

    #[test]
    fn classifies_events() {
        let mut c = Changes::default();
        assert!(!classify(&ev(EventKind::Create(CreateKind::File), &["/d/notes.txt"]), &mut c));
        assert!(classify(&ev(EventKind::Modify(ModifyKind::Data(DataChange::Content)), &["/d/a.png"]), &mut c));
        assert!(!c.list);
        assert!(classify(&ev(EventKind::Modify(ModifyKind::Name(RenameMode::Both)), &["/d/.a.png.1-0.tmp", "/d/a.png"]), &mut c));
        assert!(c.list);
        assert_eq!(c.paths, vec![PathBuf::from("/d/a.png")]);
    }

    #[test]
    fn sees_new_file() {
        let dir = std::env::temp_dir().join(format!("liteview-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = egui::Context::default();
        let Some(mut w) = Watch::new(&dir, ctx.clone()) else { return };
        image::RgbaImage::new(2, 2).save(dir.join("new.png")).unwrap();
        let t = Instant::now();
        let got = loop {
            if let Some(c) = w.poll(&ctx) {
                break c;
            }
            assert!(t.elapsed().as_secs() < 10, "no event");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(got.list && got.paths.iter().any(|p| p.ends_with("new.png")), "{got:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
