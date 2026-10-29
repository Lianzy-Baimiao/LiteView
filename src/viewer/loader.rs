//! 后台解码：两个解码线程。当前要看的图排在最前面，前后几张在空闲时一张一张预读（内存峰值低）。
//! 每张图解码后按窗口大小缩好一张缩略图，直接交给显卡（内存里不留像素）；按窗口大小看图时只用它。
//! 原图像素只在要用时才留（放大到缩略图不够清楚、编辑、复制、另存为：`want_full`），换图后放掉。
//! 只保留当前图和前后几张，超出上限时先丢最远的。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use crate::decode::{self, Content, Decoded, Error, Info, Options};
use crate::settings::FitMode;
use crate::viewer::texture::{exact_fits, exact_size, exact_source, resample};
use crate::viewer::view::View;

pub type Loaded = Result<Arc<Decoded>, Error>;

/// 新打开的图怎么显示（预读的缩略图按它缩小）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub fit: FitMode,
    /// 画布大小（逻辑像素）
    pub canvas: egui::Vec2,
    pub ppp: f32,
    pub max_side: u32,
}

/// 缩略图：zoom 是按 Target 适应窗口的比例，tex 就是这个比例下准确缩小的结果（只在显卡上）
pub struct Preview {
    pub info: Info,
    pub zoom: f32,
    pub size: [u32; 2],
    pub tex: egui::TextureHandle,
}

impl Preview {
    /// 显卡上的大小（核显时用的是共享内存，也算进缓存上限）
    fn bytes(&self) -> usize {
        self.size[0] as usize * self.size[1] as usize * 4
    }
}

/// 打开时就按原图显示（不比窗口大、「原始大小」）、太大放不进一张纹理、动图：不做缩略图
pub(crate) fn make_preview(d: &Decoded, target: &Target, ctx: &egui::Context) -> Option<Preview> {
    let Content::Still { image, levels } = &d.content else { return None };
    if image.dimensions() != (d.info.width, d.info.height) {
        return None;
    }
    let zoom = View::fit_zoom(target.fit, target.canvas, target.ppp, egui::vec2(d.info.width as f32, d.info.height as f32));
    if zoom >= 0.999 {
        return None;
    }
    let (tw, th) = exact_size(d.info.width, d.info.height, zoom);
    if !exact_fits(tw, th, target.max_side) {
        return None;
    }
    // 与显示时准确缩小用同一级源、同一个算法：比例对上时和换上原图后算出来的一样
    let all: Vec<&image::RgbaImage> = std::iter::once(&**image).chain(levels.iter().map(|l| &**l)).collect();
    let sizes: Vec<_> = all.iter().map(|l| l.dimensions()).collect();
    let pixels = resample(all[exact_source(&sizes, tw, th)], tw, th);
    // 线性插值：比例对上时像素对齐，与最近邻一样；对不上（期间缩放了）时拉伸得柔和些
    let tex = ctx.load_texture("preview", pixels, egui::TextureOptions::LINEAR);
    Some(Preview { info: d.info.clone(), zoom, size: [tw, th], tex })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// 正在看的图：随时可以开始
    Current,
    /// 留着原图、还没有按这个窗口做的缩略图：从内存里的原图缩小，然后放掉原图
    Shrink,
    /// 预读：同时只解一张，且要等当前图解完
    Preload,
}

#[derive(Clone)]
struct Job {
    path: PathBuf,
    kind: Kind,
    generation: u64,
    /// Shrink 用的原图
    decoded: Option<Arc<Decoded>>,
}

/// 解码中、或已解完等界面取走的任务
struct Busy {
    path: PathBuf,
    kind: Kind,
    generation: u64,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    busy: Vec<Busy>,
    stopped: bool,
    target: Option<Target>,
}

impl Queue {
    /// 这一版文件已经在排队解码、或解码中（缩小不算）
    fn decoding(&self, path: &Path, generation: u64) -> bool {
        let same = |p: &Path, k: Kind, g: u64| p == path && k != Kind::Shrink && g == generation;
        self.jobs.iter().any(|j| same(&j.path, j.kind, j.generation)) || self.busy.iter().any(|b| same(&b.path, b.kind, b.generation))
    }

    /// 下一个能开始的任务：当前图随时；缩小在没有别的后台任务时；预读在什么都不忙时
    fn take(&mut self) -> Option<Job> {
        let background = self.busy.iter().any(|b| b.kind != Kind::Current);
        let find = |k: Kind| self.jobs.iter().position(|j| j.kind == k);
        let i = find(Kind::Current)
            .or_else(|| if background { None } else { find(Kind::Shrink) })
            .or_else(|| if self.busy.is_empty() { find(Kind::Preload) } else { None })?;
        let job = self.jobs.remove(i)?;
        self.busy.push(Busy { path: job.path.clone(), kind: job.kind, generation: job.generation });
        Some(job)
    }
}

struct Shared {
    queue: Mutex<Queue>,
    cv: Condvar,
    ctx: Mutex<Option<egui::Context>>,
    opts: Mutex<Options>,
}

/// 解码线程交回的结果
struct Done {
    path: PathBuf,
    kind: Kind,
    generation: u64,
    /// 完整解码（Shrink 没有）
    full: Option<Loaded>,
    /// 缩略图是按哪个 Target 做的（None = 还不知道窗口大小，没做）
    evaluated: Option<Target>,
    preview: Option<Arc<Preview>>,
}

type Output = (Option<Loaded>, Option<Target>, Option<Arc<Preview>>);

fn work(job: &Job, target: Option<Target>, ctx: Option<&egui::Context>, opts: &Options) -> Output {
    let full = job.decoded.is_none().then(|| decode::decode_file(&job.path, opts).map(Arc::new));
    complete(job, target, ctx, full)
}

/// full：解码结果（Shrink 是 None，用任务里带的原图）。窗口大小或界面还不知道时不做缩略图，之后再缩小
fn complete(job: &Job, target: Option<Target>, ctx: Option<&egui::Context>, full: Option<Loaded>) -> Output {
    let shrink = |d: &Decoded| match (target, ctx) {
        (Some(t), Some(ctx)) => (Some(t), make_preview(d, &t, ctx).map(Arc::new)),
        _ => (None, None),
    };
    match (&job.decoded, full) {
        // 刚离开的图：只做缩略图，原图由界面放掉
        (Some(d), _) => {
            let (evaluated, preview) = shrink(d);
            (None, evaluated, preview)
        }
        (None, full) => {
            let full = full.unwrap_or_else(|| Err(Error::Corrupt("no decode".into())));
            // 同时做好缩略图：按窗口大小看时只用它，原图由缓存决定留不留
            let (evaluated, preview) = full.as_ref().map_or((None, None), |d| shrink(d));
            (Some(full), evaluated, preview)
        }
    }
}

fn worker(shared: &Shared, tx: &Sender<Done>) {
    loop {
        let (job, target) = {
            let mut q = shared.queue.lock().expect("lock");
            loop {
                if q.stopped {
                    return;
                }
                if let Some(job) = q.take() {
                    break (job, q.target);
                }
                q = shared.cv.wait(q).expect("lock");
            }
        };
        let opts = *shared.opts.lock().expect("lock");
        let ctx = shared.ctx.lock().expect("lock").clone();
        let t = Instant::now();
        // 解码 / 缩小时 panic 只算这张图坏了；线程不能死，否则任务一直占着位置
        let (full, evaluated, preview) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&job, target, ctx.as_ref(), &opts))).unwrap_or_else(|_| match job.kind {
            Kind::Shrink => (None, target, None),
            _ => (Some(Err(Error::Corrupt("decoder panicked".into()))), None, None),
        });
        log::debug!("{:?} {} 用时 {:.1} ms", job.kind, job.path.display(), t.elapsed().as_secs_f64() * 1000.0);
        let Job { path, kind, generation, decoded } = job;
        drop(decoded);
        // 界面取走结果时才算完成（busy 里一直留着），所以同一张不会被重复解码
        if tx.send(Done { path, kind, generation, full, evaluated, preview }).is_err() {
            return;
        }
        if let Some(ctx) = &ctx {
            ctx.request_repaint();
        }
    }
}
#[derive(Default)]
struct Entry {
    /// 完整解码（或打不开的原因）
    full: Option<Loaded>,
    preview: Option<Arc<Preview>>,
    /// 缩略图按哪个 Target 判断过（判断过但不适用时 preview 是 None）
    evaluated: Option<Target>,
}

impl Entry {
    fn bytes(&self) -> usize {
        let full = self.full.as_ref().and_then(|f| f.as_ref().ok()).map_or(0, |d| d.bytes());
        full + self.preview.as_ref().map_or(0, |p| p.bytes())
    }

    /// 切过来能马上显示：有原图，或有按当前窗口做的缩略图
    fn ready_for(&self, target: Option<Target>) -> bool {
        self.full.is_some() || (self.preview.is_some() && self.evaluated == target)
    }
}

pub struct Loader {
    shared: Arc<Shared>,
    rx: Receiver<Done>,
    cache: HashMap<PathBuf, Entry>,
    current: Option<PathBuf>,
    /// 预读的前后几张，按顺序（越靠前越先保留）
    neighbors: Vec<PathBuf>,
    target: Option<Target>,
    /// 文件变了 / 换了内容：之前开始的解码结果作废
    generations: HashMap<PathBuf, u64>,
    budget: usize,
    /// 收到了新结果，下次 poll 时整理缓存
    dirty: bool,
    /// 收到了结果（解码线程空出来了）：`wake` 时叫醒解码线程
    wake_pending: bool,
    /// 当前图要留原图（放大、编辑、复制要用）；换图后清掉
    pinned: Option<PathBuf>,
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
        Self { shared, rx, cache: HashMap::new(), current: None, neighbors: Vec::new(), target: None, generations: HashMap::new(), budget, dirty: false, wake_pending: false, pinned: None }
    }

    /// 界面建好后告诉解码线程，解完时让界面重绘
    pub fn set_context(&self, ctx: egui::Context) {
        *self.shared.ctx.lock().expect("lock") = Some(ctx);
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
        self.trim();
        self.shared.cv.notify_all();
    }

    pub fn set_options(&self, opts: Options) {
        *self.shared.opts.lock().expect("lock") = opts;
    }

    /// 窗口大小 / 显示方式：之后预读的缩略图按它做
    pub fn set_target(&mut self, target: Target) {
        if self.target != Some(target) {
            self.target = Some(target);
            self.shared.queue.lock().expect("lock").target = Some(target);
        }
    }

    fn generation(&self, path: &Path) -> u64 {
        self.generations.get(path).copied().unwrap_or(0)
    }

    /// 把解码线程送回来的结果收进缓存；返回收到了完整解码的图
    fn drain(&mut self) -> Vec<PathBuf> {
        let mut done = Vec::new();
        let mut any = false;
        while let Ok(d) = self.rx.try_recv() {
            any = true;
            let mut q = self.shared.queue.lock().expect("lock");
            if let Some(i) = q.busy.iter().position(|b| b.path == d.path && b.kind == d.kind && b.generation == d.generation) {
                q.busy.remove(i);
            }
            drop(q);
            if d.generation != self.generation(&d.path) {
                continue;
            }
            let entry = match d.full {
                Some(_) => self.cache.entry(d.path.clone()).or_default(),
                // 缩小完成时图已经不要了
                None => match self.cache.get_mut(&d.path) {
                    Some(e) => e,
                    None => continue,
                },
            };
            if d.evaluated.is_some() {
                entry.evaluated = d.evaluated;
                entry.preview = d.preview;
            }
            if let Some(full) = d.full {
                entry.full = Some(full);
                done.push(d.path);
            }
            self.dirty = true;
        }
        // 不在这里唤醒解码线程：先整理缓存（排好缩小任务）、界面也换下不要的原图后再唤醒（`wake`），
        // 否则下一张预读会抢在原图放掉之前开始
        self.wake_pending |= any;
        done
    }

    /// 要看这一张：有原图就返回；有按当前窗口做的缩略图就不解码（显示它就够了）；
    /// 否则排到最前面解码（之前排队的「当前图」作废）。不整理缓存（前后几张还没更新）；之后要调用 `preload`
    pub fn request(&mut self, path: &Path) -> Option<Loaded> {
        if self.current.as_deref() != Some(path) {
            self.pinned = None;
        }
        self.current = Some(path.to_path_buf());
        self.drain();
        if let Some(full) = self.cache.get(path).and_then(|e| e.full.clone()) {
            return Some(full);
        }
        if self.fresh_preview(path).is_none() {
            self.decode_now(path);
        }
        None
    }

    /// 当前图要原图像素（放大到缩略图不够清楚、编辑、复制、另存为）：留住，没有就马上解码
    pub fn want_full(&mut self, path: &Path) -> Option<Loaded> {
        self.current = Some(path.to_path_buf());
        self.pinned = Some(path.to_path_buf());
        self.drain();
        if let Some(full) = self.cache.get(path).and_then(|e| e.full.clone()) {
            return Some(full);
        }
        self.decode_now(path);
        None
    }

    /// 当前图不再要原图：有按当前窗口做的缩略图时下次整理缓存就放掉
    pub fn release_full(&mut self, path: &Path) {
        if self.pinned.as_deref() == Some(path) {
            self.pinned = None;
        }
        self.dirty = true;
    }

    fn decode_now(&mut self, path: &Path) {
        let generation = self.generation(path);
        let mut q = self.shared.queue.lock().expect("lock");
        q.jobs.retain(|j| j.kind != Kind::Current && !(j.path == path && j.kind == Kind::Preload));
        if !q.decoding(path, generation) {
            q.jobs.push_front(Job { path: path.to_path_buf(), kind: Kind::Current, generation, decoded: None });
            self.shared.cv.notify_all();
        }
    }

    /// 缩略图（可能是按之前的窗口大小做的）
    pub fn preview(&self, path: &Path) -> Option<Arc<Preview>> {
        self.cache.get(path)?.preview.clone()
    }

    /// 按当前窗口大小做的缩略图
    pub fn fresh_preview(&self, path: &Path) -> Option<Arc<Preview>> {
        let e = self.cache.get(path)?;
        if e.evaluated != self.target || self.target.is_none() {
            return None;
        }
        e.preview.clone()
    }

    pub fn get(&self, path: &Path) -> Option<Loaded> {
        self.cache.get(path)?.full.clone()
    }

    /// 当前图的前后几张（按优先顺序）：不在里面的丢掉，缺的排队预读（替换掉之前排队的预读）
    pub fn preload(&mut self, paths: &[PathBuf]) {
        self.neighbors = paths.iter().filter(|p| self.current.as_ref() != Some(*p)).cloned().collect();
        self.drain();
        self.trim();
        let target = self.target;
        let missing: Vec<&PathBuf> = self.neighbors.iter().filter(|p| !self.cache.get(*p).is_some_and(|e| e.ready_for(target))).collect();
        let mut q = self.shared.queue.lock().expect("lock");
        let keep = |p: &PathBuf| self.current.as_ref() == Some(p) || self.neighbors.contains(p);
        q.jobs.retain(|j| match j.kind {
            Kind::Current => true,
            Kind::Shrink => keep(&j.path),
            Kind::Preload => false,
        });
        for p in missing {
            let generation = self.generation(p);
            if !q.decoding(p, generation) {
                q.jobs.push_back(Job { path: p.clone(), kind: Kind::Preload, generation, decoded: None });
            }
        }
        self.shared.cv.notify_all();
    }

    /// 只留当前图和前后几张；有了按当前窗口做的缩略图就放掉原图（当前图要原图时除外）；超出上限时先丢最远的
    fn trim(&mut self) {
        self.dirty = false;
        let current = self.current.clone();
        let neighbors = &self.neighbors;
        self.cache.retain(|p, _| current.as_ref() == Some(p) || neighbors.contains(p));
        let mut shrink = Vec::new();
        let unpinned = current.iter().filter(|c| self.pinned.as_ref() != Some(*c));
        for p in unpinned.chain(&self.neighbors) {
            let Some(e) = self.cache.get_mut(p) else { continue };
            let Some(Ok(d)) = &e.full else { continue };
            match (e.evaluated == self.target, &e.preview) {
                // 缩略图做好了：原图放掉
                (true, Some(_)) => e.full = None,
                // 不适用（太小、原始大小、动图）：留着原图
                (true, None) => {}
                // 还没按这个窗口做过：先在后台缩小，做好前原图还在，切回来照样马上显示
                (false, _) if self.target.is_some() => shrink.push((p.clone(), d.clone())),
                (false, _) => {}
            }
        }
        let mut total: usize = self.cache.values().map(Entry::bytes).sum();
        let mut evicted = Vec::new();
        for p in self.neighbors.iter().rev() {
            if total <= self.budget {
                break;
            }
            if let Some(e) = self.cache.remove(p) {
                total -= e.bytes();
                evicted.push(p.clone());
            }
        }
        if shrink.is_empty() && evicted.is_empty() {
            return;
        }
        let mut q = self.shared.queue.lock().expect("lock");
        q.jobs.retain(|j| j.kind == Kind::Current || !evicted.contains(&j.path));
        for (path, d) in shrink {
            let queued = q.jobs.iter().any(|j| j.kind == Kind::Shrink && j.path == path) || q.busy.iter().any(|b| b.kind == Kind::Shrink && b.path == path);
            if !evicted.contains(&path) && !queued {
                let generation = self.generations.get(&path).copied().unwrap_or(0);
                q.jobs.push_back(Job { path, kind: Kind::Shrink, generation, decoded: Some(d) });
            }
        }
    }

    /// 不再看文件夹里的图（粘贴的图、文件夹空了）：全部放掉
    pub fn release(&mut self) {
        self.current = None;
        self.pinned = None;
        self.neighbors.clear();
        self.cache.clear();
        self.shared.queue.lock().expect("lock").jobs.clear();
    }

    /// 放进已解码好的结果（保存、改名后沿用），它就是当前图
    pub fn insert(&mut self, path: &Path, result: Loaded) {
        self.forget(path);
        self.current = Some(path.to_path_buf());
        self.cache.insert(path.to_path_buf(), Entry { full: Some(result), ..Default::default() });
    }

    /// 文件变了或被删了：丢掉缓存，之前开始的解码作废
    pub fn forget(&mut self, path: &Path) {
        if self.pinned.as_deref() == Some(path) {
            self.pinned = None;
        }
        self.cache.remove(path);
        *self.generations.entry(path.to_path_buf()).or_default() += 1;
        self.shared.queue.lock().expect("lock").jobs.retain(|job| job.path != path);
    }

    /// 取回解码完成的结果并整理缓存；返回这次收到了哪些图的完整解码。
    /// 不叫醒解码线程：界面先按新的缓存换掉画面（换回缩略图时放掉原图），然后调用 `wake`
    pub fn poll(&mut self) -> Vec<PathBuf> {
        let done = self.drain();
        if self.dirty {
            self.trim();
        }
        done
    }

    /// 收到过结果就叫醒解码线程（排着的缩小、预读可以开始了）
    pub fn wake(&mut self) {
        if std::mem::take(&mut self.wake_pending) {
            self.shared.cv.notify_all();
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let mut queue = self.shared.queue.lock().expect("lock");
        queue.stopped = true;
        queue.jobs.clear();
        self.shared.cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn still(w: u32, h: u32, red: u8) -> Loaded {
        Ok(Arc::new(decode::from_rgba(image::RgbaImage::from_pixel(w, h, image::Rgba([red, 0, 0, 255])), "PNG")))
    }

    fn target() -> Target {
        Target { fit: FitMode::Fit, canvas: egui::vec2(100.0, 100.0), ppp: 1.0, max_side: 8192 }
    }

    /// 没有解码线程：测试自己取任务、交结果
    fn manual() -> (Loader, Sender<Done>) {
        let mut l = Loader::new(0, Options::default(), 1 << 30);
        let (tx, rx) = channel();
        l.rx = rx;
        (l, tx)
    }

    fn take(l: &Loader) -> Option<Job> {
        l.shared.queue.lock().unwrap().take()
    }

    /// 像解码线程一样做完一个任务（解码结果由测试给出）；有 target 时也做缩略图
    fn finish(tx: &Sender<Done>, job: &Job, full: Loaded, target: Option<Target>) {
        let ctx = egui::Context::default();
        let (full, evaluated, preview) = complete(job, target, Some(&ctx), job.decoded.is_none().then_some(full));
        tx.send(Done { path: job.path.clone(), kind: job.kind, generation: job.generation, full, evaluated, preview }).unwrap();
    }

    fn red(l: &Loader, p: &Path) -> Option<u8> {
        Some(l.get(p)?.ok()?.first_image().get_pixel(0, 0)[0])
    }

    #[test]
    fn dropping_loader_releases_idle_workers() {
        let loader = Loader::new(2, Options::default(), 1 << 20);
        let shared = Arc::downgrade(&loader.shared);
        drop(loader);
        let start = Instant::now();
        while shared.strong_count() != 0 {
            assert!(start.elapsed().as_secs() < 5, "idle workers leaked");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn old_decode_cannot_replace_newly_saved_or_reloaded_pixels() {
        let (mut loader, tx) = manual();
        let path = PathBuf::from("image.png");
        assert!(loader.request(&path).is_none());
        let old = take(&loader).unwrap();
        loader.insert(&path, still(2, 1, 200));
        finish(&tx, &old, still(2, 1, 10), None);
        assert!(loader.poll().is_empty());
        assert_eq!(red(&loader, &path), Some(200));
        loader.forget(&path);
        assert!(loader.request(&path).is_none());
        let new = take(&loader).unwrap();
        finish(&tx, &old, still(2, 1, 10), None);
        finish(&tx, &new, still(2, 1, 150), None);
        assert_eq!(loader.poll(), vec![path.clone()]);
        assert_eq!(red(&loader, &path), Some(150));
    }

    #[test]
    fn decodes_in_background_and_caches() {
        let dir = std::env::temp_dir().join(format!("liteview-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.png");
        image::RgbaImage::from_pixel(8, 4, image::Rgba([1, 2, 3, 255])).save(&p).unwrap();
        let mut l = Loader::new(1, Options::default(), 1 << 30);
        assert!(l.request(&p).is_none());
        let t = Instant::now();
        while l.poll().is_empty() {
            assert!(t.elapsed().as_secs() < 10, "timeout");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let d = l.request(&p).unwrap().unwrap();
        assert_eq!((d.info.width, d.info.height), (8, 4));
        assert!(l.request(&dir.join("missing.png")).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finished_but_unread_result_is_not_decoded_again() {
        let (mut l, tx) = manual();
        let p = PathBuf::from("a.png");
        assert!(l.request(&p).is_none());
        let job = take(&l).unwrap();
        // 已解完、还没被取走时再要同一张：先收结果，不重复排队
        finish(&tx, &job, still(4, 4, 7), None);
        assert!(l.request(&p).is_some_and(|r| r.is_ok()));
        assert!(take(&l).is_none());
        // 预读已经在解的图成了当前图：等它，不另起一次
        l.preload(&[PathBuf::from("b.png")]);
        let pre = take(&l).unwrap();
        assert_eq!(pre.kind, Kind::Preload);
        assert!(l.request(Path::new("b.png")).is_none());
        assert!(take(&l).is_none());
        finish(&tx, &pre, still(4, 4, 9), None);
        assert!(l.request(Path::new("b.png")).is_some());
    }

    #[test]
    fn original_pixels_are_kept_only_while_needed() {
        let (mut l, tx) = manual();
        l.set_target(target());
        let (a, b, c) = (PathBuf::from("a.png"), PathBuf::from("b.png"), PathBuf::from("c.png"));
        l.request(&a);
        l.preload(std::slice::from_ref(&b));
        let job = take(&l).unwrap();
        assert_eq!(job.kind, Kind::Current);
        // 当前图解完之前不预读
        assert!(take(&l).is_none());
        finish(&tx, &job, still(800, 600, 1), Some(target()));
        l.poll();
        // 按窗口大小看：当前图也只留缩略图（800×600 在 100×100 的窗口里是 12.5%）
        assert!(l.cache[&a].full.is_none());
        assert_eq!(l.fresh_preview(&a).map(|p| (p.zoom, p.size)), Some((0.125, [100, 75])));
        let pre = take(&l).unwrap();
        assert_eq!((pre.kind, pre.path.as_path()), (Kind::Preload, b.as_path()));
        finish(&tx, &pre, still(800, 600, 2), Some(target()));
        l.poll();
        assert!(l.cache[&b].full.is_none() && l.cache[&b].preview.is_some());
        // 放大（要原图）：重新解码并留住
        assert!(l.want_full(&a).is_none());
        let job = take(&l).unwrap();
        assert_eq!((job.kind, job.path.as_path()), (Kind::Current, a.as_path()));
        finish(&tx, &job, still(800, 600, 1), Some(target()));
        l.poll();
        assert!(l.cache[&a].full.is_some());
        // 不再需要：放掉
        l.release_full(&a);
        l.poll();
        assert!(l.cache[&a].full.is_none());
        // 切到 b：有按当前窗口做的缩略图，不用解码；接着预读 c
        assert!(l.request(&b).is_none());
        l.preload(&[c.clone(), a.clone()]);
        let next = take(&l).unwrap();
        assert_eq!((next.kind, next.path.as_path()), (Kind::Preload, c.as_path()));
        // 跳到很远：之前的都不留
        l.request(Path::new("z.png"));
        l.preload(&[]);
        assert!(l.cache.is_empty());
    }

    #[test]
    fn over_budget_drops_the_farthest_neighbor_first() {
        let (mut l, tx) = manual();
        // 「原始大小」：不做缩略图，邻居留原图
        let actual = Target { fit: FitMode::Actual, ..target() };
        l.set_target(actual);
        l.set_budget(still(64, 64, 0).unwrap().bytes() * 2);
        let (cur, next, prev) = (PathBuf::from("b.png"), PathBuf::from("c.png"), PathBuf::from("a.png"));
        l.request(&cur);
        l.preload(&[next.clone(), prev.clone()]);
        for _ in 0..3 {
            let job = take(&l).unwrap();
            finish(&tx, &job, still(64, 64, 0), Some(actual));
            l.poll();
        }
        assert!(l.cache.contains_key(&cur) && l.cache.contains_key(&next));
        assert!(!l.cache.contains_key(&prev));
    }

    #[test]
    fn current_image_first_and_one_background_job_at_a_time() {
        let mut q = Queue::default();
        let job = |p: &str, kind| Job { path: p.into(), kind, generation: 0, decoded: None };
        q.jobs.extend([job("p1", Kind::Preload), job("p2", Kind::Preload), job("s", Kind::Shrink), job("c", Kind::Current)]);
        let path = |j: Option<Job>| j.map(|j| j.path);
        assert_eq!(path(q.take()), Some("c".into()));
        // 当前图在解：缩小可以同时做，预读要等
        assert_eq!(path(q.take()), Some("s".into()));
        assert!(q.take().is_none());
        q.busy.clear();
        assert_eq!(path(q.take()), Some("p1".into()));
        assert!(q.take().is_none());
        // 预读在解时，新的当前图不用等
        q.jobs.push_front(job("c2", Kind::Current));
        assert_eq!(path(q.take()), Some("c2".into()));
    }

    #[test]
    fn preview_is_the_fitted_exact_downscale() {
        let t = Target { fit: FitMode::Fit, canvas: egui::vec2(600.0, 450.0), ppp: 1.0, max_side: 8192 };
        let ctx = egui::Context::default();
        let big = still(1600, 1200, 3).unwrap();
        let p = make_preview(&big, &t, &ctx).expect("smaller than the window needs");
        let zoom = View::fit_zoom(FitMode::Fit, t.canvas, t.ppp, egui::vec2(1600.0, 1200.0));
        assert_eq!(p.zoom, zoom);
        let (w, h) = exact_size(1600, 1200, zoom);
        assert_eq!((p.size, p.tex.size()), ([w, h], [w as usize, h as usize]));
        // 打开时就按原图显示的：不做缩略图
        assert!(make_preview(&still(300, 200, 3).unwrap(), &t, &ctx).is_none());
        assert!(make_preview(&big, &Target { fit: FitMode::Actual, ..t }, &ctx).is_none());
        let frame = |red| decode::Frame { image: still(1600, 1200, red).unwrap().first_image().clone(), delay: std::time::Duration::from_millis(100) };
        let anim = Decoded {
            info: big.info.clone(),
            content: Content::Animation { frames: vec![frame(1), frame(2)], delays: vec![std::time::Duration::from_millis(100); 2], plays: None, source: None },
        };
        assert!(make_preview(&anim, &t, &ctx).is_none());
    }
}
