//! 后台解码：两个解码线程。当前要看的图排在最前面，前后几张在空闲时预读。
//! 结果按路径缓存，超过内存上限时淘汰最久没看的（当前图不淘汰）。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use crate::decode::{self, Decoded, Error, Options};

pub type Loaded = Result<Arc<Decoded>, Error>;

struct Job {
    path: PathBuf,
    current: bool,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    busy: Vec<PathBuf>,
}

struct Shared {
    queue: Mutex<Queue>,
    cv: Condvar,
    ctx: Mutex<Option<egui::Context>>,
    opts: Mutex<Options>,
}

struct Entry {
    result: Loaded,
    bytes: usize,
    used: u64,
}

pub struct Loader {
    shared: Arc<Shared>,
    rx: Receiver<(PathBuf, Loaded)>,
    cache: HashMap<PathBuf, Entry>,
    tick: u64,
    budget: usize,
}

impl Loader {
    pub fn new(threads: usize, opts: Options, budget: usize) -> Self {
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            cv: Condvar::new(),
            ctx: Mutex::new(None),
            opts: Mutex::new(opts),
        });
        for i in 0..threads {
            let (shared, tx) = (shared.clone(), tx.clone());
            std::thread::Builder::new().name(format!("decode-{i}")).spawn(move || worker(&shared, &tx)).expect("spawn decode thread");
        }
        Self { shared, rx, cache: HashMap::new(), tick: 0, budget }
    }

    /// 界面建好后告诉解码线程，解完时让界面重绘
    pub fn set_context(&self, ctx: egui::Context) {
        *self.shared.ctx.lock().expect("lock") = Some(ctx);
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
    }

    pub fn set_options(&self, opts: Options) {
        *self.shared.opts.lock().expect("lock") = opts;
    }

    /// 把解码线程送回来的结果收进缓存
    fn drain(&mut self) -> Vec<PathBuf> {
        let mut done = Vec::new();
        while let Ok((path, result)) = self.rx.try_recv() {
            let bytes = result.as_ref().map_or(0, |d| d.bytes());
            self.tick += 1;
            self.cache.insert(path.clone(), Entry { result, bytes, used: self.tick });
            done.push(path);
        }
        done
    }

    /// 要看这一张：已缓存就返回结果，否则排到最前面（之前排队的「当前图」作废）
    pub fn request(&mut self, path: &Path) -> Option<Loaded> {
        if !self.drain().is_empty() {
            self.evict(Some(path));
        }
        self.tick += 1;
        if let Some(e) = self.cache.get_mut(path) {
            e.used = self.tick;
            return Some(e.result.clone());
        }
        let mut q = self.shared.queue.lock().expect("lock");
        q.jobs.retain(|j| !j.current && j.path != path);
        if !q.busy.iter().any(|p| p == path) {
            q.jobs.push_front(Job { path: path.to_path_buf(), current: true });
            self.shared.cv.notify_one();
        }
        None
    }

    /// 预读：替换掉排队中的旧预读
    pub fn preload(&mut self, paths: &[PathBuf]) {
        let mut q = self.shared.queue.lock().expect("lock");
        q.jobs.retain(|j| j.current);
        for p in paths {
            if !self.cache.contains_key(p) && !q.busy.contains(p) && !q.jobs.iter().any(|j| &j.path == p) {
                q.jobs.push_back(Job { path: p.clone(), current: false });
            }
        }
        self.shared.cv.notify_all();
    }

    /// 取回解码完成的结果，按内存上限淘汰；返回这次完成了哪些
    pub fn poll(&mut self, keep: Option<&Path>) -> Vec<PathBuf> {
        let done = self.drain();
        if !done.is_empty() {
            self.evict(keep);
        }
        done
    }

    pub fn get(&self, path: &Path) -> Option<Loaded> {
        self.cache.get(path).map(|e| e.result.clone())
    }

    /// 放进已解码好的结果（改名后沿用）
    pub fn insert(&mut self, path: &Path, result: Loaded) {
        let bytes = result.as_ref().map_or(0, |d| d.bytes());
        self.tick += 1;
        self.cache.insert(path.to_path_buf(), Entry { result, bytes, used: self.tick });
    }

    /// 文件变了或被删了：丢掉缓存
    pub fn forget(&mut self, path: &Path) {
        self.cache.remove(path);
    }

    fn evict(&mut self, keep: Option<&Path>) {
        let mut total: usize = self.cache.values().map(|e| e.bytes).sum();
        while total > self.budget {
            let victim = self
                .cache
                .iter()
                .filter(|(p, _)| Some(p.as_path()) != keep)
                .min_by_key(|(_, e)| e.used)
                .map(|(p, _)| p.clone());
            let Some(v) = victim else { break };
            total -= self.cache.remove(&v).map_or(0, |e| e.bytes);
        }
    }
}

fn worker(shared: &Shared, tx: &Sender<(PathBuf, Loaded)>) {
    loop {
        let path = {
            let mut q = shared.queue.lock().expect("lock");
            loop {
                if let Some(job) = q.jobs.pop_front() {
                    q.busy.push(job.path.clone());
                    break job.path;
                }
                q = shared.cv.wait(q).expect("lock");
            }
        };
        let opts = *shared.opts.lock().expect("lock");
        let t = Instant::now();
        let result = decode::decode_file(&path, &opts).map(Arc::new);
        log::debug!("解码 {} 用时 {:.1} ms", path.display(), t.elapsed().as_secs_f64() * 1000.0);
        shared.queue.lock().expect("lock").busy.retain(|p| p != &path);
        if tx.send((path, result)).is_err() {
            return;
        }
        if let Some(ctx) = shared.ctx.lock().expect("lock").as_ref() {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_in_background_and_caches() {
        let dir = std::env::temp_dir().join(format!("liteview-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.png");
        image::RgbaImage::from_pixel(8, 4, image::Rgba([1, 2, 3, 255])).save(&p).unwrap();
        let mut l = Loader::new(1, Options::default(), 1 << 30);
        assert!(l.request(&p).is_none());
        let t = Instant::now();
        while l.poll(Some(&p)).is_empty() {
            assert!(t.elapsed().as_secs() < 10, "timeout");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let d = l.request(&p).unwrap().unwrap();
        assert_eq!((d.info.width, d.info.height), (8, 4));
        // 结果还在通道里没被取走时再要同一张：不能重复解码
        let q = dir.join("b.png");
        image::RgbaImage::new(2, 2).save(&q).unwrap();
        assert!(l.request(&q).is_none());
        let t = Instant::now();
        while l.shared.queue.lock().unwrap().busy.contains(&q) || !l.shared.queue.lock().unwrap().jobs.is_empty() {
            assert!(t.elapsed().as_secs() < 10, "timeout");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(l.request(&q).is_some_and(|r| r.is_ok()));
        assert!(l.shared.queue.lock().unwrap().jobs.is_empty());
        assert!(l.request(&dir.join("missing.png")).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
