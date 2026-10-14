//! 文档与撤销历史。
//!
//! 一步改动记下改动前的像素：局部改动按 256×256 分块只存动过的块，裁剪、调整大小、旋转这类整张图的
//! 改动存整张图。撤销时把存的块与图上的块互换，换下来的就是重做用的数据，所以每块只存一份。
//! 总内存（默认 512 MB）或步数超出上限时丢弃最早的步骤。历史只在内存里，不写磁盘。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use image::RgbaImage;

use super::{IRect, TILE, tile_rect};

/// 改动前的分块（还没改的块不在里面）
#[derive(Default)]
pub struct Saved {
    pub tiles: HashMap<(u32, u32), Vec<u8>>,
}

impl Saved {
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// 要改 rect 之前调用：把还没存过的块存下来
    pub fn touch(&mut self, img: &RgbaImage, rect: IRect) {
        let (w, h) = img.dimensions();
        for t in rect.intersect(&IRect::full(w, h)).tiles() {
            self.tiles.entry(t).or_insert_with(|| read_tile(img, tile_rect(t.0, t.1, w, h)));
        }
    }

    pub fn bytes(&self) -> usize {
        self.tiles.values().map(Vec::len).sum()
    }

    /// 改动前的像素（还没存的块就是图上现在的）
    pub fn original(&self, img: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
        let (w, h) = img.dimensions();
        match self.tiles.get(&(x / TILE, y / TILE)) {
            Some(data) => {
                let r = tile_rect(x / TILE, y / TILE, w, h);
                let i = (((y - r.y0) * r.width() + (x - r.x0)) * 4) as usize;
                [data[i], data[i + 1], data[i + 2], data[i + 3]]
            }
            None => img.get_pixel(x, y).0,
        }
    }

    /// 改动前 rect 这块区域的像素（按行连续）
    pub fn region(&self, img: &RgbaImage, rect: IRect) -> Vec<u8> {
        let (w, h) = img.dimensions();
        let rw = rect.width() as usize;
        let mut out = vec![0u8; rw * rect.height() as usize * 4];
        for t in rect.tiles() {
            let tr = tile_rect(t.0, t.1, w, h);
            let part = tr.intersect(&rect);
            let saved = self.tiles.get(&t);
            for y in part.y0..part.y1 {
                let dst = ((y - rect.y0) as usize * rw + (part.x0 - rect.x0) as usize) * 4;
                let n = part.width() as usize * 4;
                let src = match saved {
                    Some(d) => &d[(((y - tr.y0) * tr.width() + (part.x0 - tr.x0)) * 4) as usize..][..n],
                    None => &img.as_raw()[((y as usize * w as usize) + part.x0 as usize) * 4..][..n],
                };
                out[dst..dst + n].copy_from_slice(src);
            }
        }
        out
    }

    /// 存下来之后没有真的改动的块不留
    pub fn prune(&mut self, img: &RgbaImage) {
        let (w, h) = img.dimensions();
        self.tiles.retain(|t, data| read_tile(img, tile_rect(t.0, t.1, w, h)) != *data);
    }

    /// 放弃改动：把存的块写回图上；返回涉及的范围
    pub fn restore(self, img: &mut RgbaImage) -> IRect {
        let (w, h) = img.dimensions();
        let mut dirty = IRect::EMPTY;
        for (t, data) in self.tiles {
            let r = tile_rect(t.0, t.1, w, h);
            write_tile(img, r, &data);
            dirty = dirty.union(&r);
        }
        dirty
    }
}

pub fn read_tile(img: &RgbaImage, r: IRect) -> Vec<u8> {
    let w = img.width() as usize;
    let mut out = Vec::with_capacity(r.area() as usize * 4);
    for y in r.y0..r.y1 {
        let s = (y as usize * w + r.x0 as usize) * 4;
        out.extend_from_slice(&img.as_raw()[s..s + r.width() as usize * 4]);
    }
    out
}

pub fn write_tile(img: &mut RgbaImage, r: IRect, data: &[u8]) {
    let w = img.width() as usize;
    let n = r.width() as usize * 4;
    let raw: &mut [u8] = img;
    for (row, y) in (r.y0..r.y1).enumerate() {
        let s = (y as usize * w + r.x0 as usize) * 4;
        raw[s..s + n].copy_from_slice(&data[row * n..row * n + n]);
    }
}

/// 把存的块与图上的块互换（撤销 / 重做）
fn swap_tiles(img: &mut RgbaImage, tiles: &mut HashMap<(u32, u32), Vec<u8>>) -> IRect {
    let (w, h) = img.dimensions();
    let mut dirty = IRect::EMPTY;
    for (t, data) in tiles.iter_mut() {
        let r = tile_rect(t.0, t.1, w, h);
        let now = read_tile(img, r);
        write_tile(img, r, data);
        *data = now;
        dirty = dirty.union(&r);
    }
    dirty
}

enum Change {
    Tiles(HashMap<(u32, u32), Vec<u8>>),
    Whole(Arc<RgbaImage>),
}

struct Step {
    change: Change,
    /// 这一步打了码（撤销到它之前就不算打过码）
    redact: bool,
    /// 这一步之前 / 之后的版本号
    before: u64,
    after: u64,
}

impl Step {
    fn bytes(&self) -> usize {
        match &self.change {
            Change::Tiles(t) => t.values().map(Vec::len).sum(),
            Change::Whole(img) => img.len(),
        }
    }
}

/// 正在编辑的图
pub struct Doc {
    pub image: Arc<RgbaImage>,
    undo: VecDeque<Step>,
    redo: Vec<Step>,
    bytes: usize,
    max_steps: usize,
    max_bytes: usize,
    /// 当前版本号（每次改动换一个新号，撤销回到旧号）
    pub rev: u64,
    next_rev: u64,
    /// 最近一次保存时的版本号
    pub saved_rev: u64,
    /// 打过码、但那一步已经被丢弃（撤销不回去了）
    redact_base: bool,
    /// 自上次取走以来改过的范围（更新显示用）
    dirty: IRect,
    /// 尺寸变了（显示要整张重建）
    resized: bool,
}

impl Doc {
    pub fn new(image: RgbaImage, max_steps: usize, max_bytes: usize) -> Self {
        Self {
            image: Arc::new(image),
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
            max_steps: max_steps.max(1),
            max_bytes,
            rev: 0,
            next_rev: 1,
            saved_rev: 0,
            redact_base: false,
            dirty: IRect::EMPTY,
            resized: false,
        }
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn full(&self) -> IRect {
        IRect::full(self.width(), self.height())
    }

    /// 改图（后台线程还拿着这张图时先复制一份）
    pub fn image_mut(&mut self) -> &mut RgbaImage {
        Arc::make_mut(&mut self.image)
    }

    pub fn set_limits(&mut self, max_steps: usize, max_bytes: usize) {
        self.max_steps = max_steps.max(1);
        self.max_bytes = max_bytes;
        self.prune();
    }

    pub fn mark(&mut self, r: IRect) {
        self.dirty = self.dirty.union(&r.intersect(&self.full()));
    }

    /// 取走改过的范围；第二个值 = 尺寸变了
    pub fn take_dirty(&mut self) -> (IRect, bool) {
        (std::mem::take(&mut self.dirty), std::mem::take(&mut self.resized))
    }

    /// 局部改动做完：saved 是改之前存下的块（没真的改动的块会被去掉）；没有改动时不记一步
    pub fn commit(&mut self, mut saved: Saved, redact: bool) -> bool {
        saved.prune(&self.image);
        if saved.is_empty() {
            return false;
        }
        self.push(Change::Tiles(saved.tiles), redact);
        true
    }

    /// 整张图换成新的（裁剪、调整大小、旋转翻转）
    pub fn replace(&mut self, image: RgbaImage) {
        let resized = image.dimensions() != self.image.dimensions();
        let old = std::mem::replace(&mut self.image, Arc::new(image));
        self.push(Change::Whole(old), false);
        self.resized |= resized;
        self.dirty = self.full();
    }

    fn push(&mut self, change: Change, redact: bool) {
        let step = Step { change, redact, before: self.rev, after: self.next_rev };
        self.rev = self.next_rev;
        self.next_rev += 1;
        self.bytes += step.bytes();
        self.undo.push_back(step);
        for s in self.redo.drain(..) {
            self.bytes -= s.bytes();
        }
        self.prune();
    }

    /// 超出步数或内存上限：丢弃最早的步骤（至少留一步）
    fn prune(&mut self) {
        while self.undo.len() > 1 && (self.undo.len() > self.max_steps || self.bytes > self.max_bytes) {
            let s = self.undo.pop_front().expect("non-empty");
            self.bytes -= s.bytes();
            self.redact_base |= s.redact;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(mut s) = self.undo.pop_back() else { return false };
        self.apply(&mut s.change);
        self.rev = s.before;
        self.redo.push(s);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(mut s) = self.redo.pop() else { return false };
        self.apply(&mut s.change);
        self.rev = s.after;
        self.undo.push_back(s);
        true
    }

    fn apply(&mut self, change: &mut Change) {
        match change {
            Change::Tiles(tiles) => {
                let d = swap_tiles(Arc::make_mut(&mut self.image), tiles);
                self.mark(d);
            }
            Change::Whole(img) => {
                let resized = img.dimensions() != self.image.dimensions();
                std::mem::swap(&mut self.image, img);
                self.resized |= resized;
                self.dirty = self.full();
            }
        }
    }

    /// 有没保存的修改
    pub fn modified(&self) -> bool {
        self.rev != self.saved_rev
    }

    pub fn mark_saved(&mut self, rev: u64) {
        self.saved_rev = rev;
    }

    /// 当前的图打过码（保存时默认去掉元数据）
    pub fn redacted(&self) -> bool {
        self.redact_base || self.undo.iter().any(|s| s.redact)
    }

    /// 撤销历史占用的内存（字节）
    pub fn history_bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 7 + y * 13) as u8, (x * 3) as u8, (y * 5) as u8, 255]))
    }

    fn paint(doc: &mut Doc, r: IRect, v: u8) {
        let mut saved = Saved::default();
        saved.touch(&doc.image, r);
        let img = doc.image_mut();
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                img.put_pixel(x, y, image::Rgba([v, v, v, 255]));
            }
        }
        doc.commit(saved, v == 0);
    }

    #[test]
    fn undo_to_the_start_restores_every_pixel() {
        let orig = noise(600, 300);
        let mut doc = Doc::new(orig.clone(), 100, 1 << 30);
        paint(&mut doc, IRect::new(10, 10, 300, 200), 9);
        paint(&mut doc, IRect::new(250, 0, 600, 50), 0);
        doc.replace(image::imageops::crop_imm(&*doc.image, 5, 5, 100, 80).to_image());
        paint(&mut doc, IRect::new(0, 0, 40, 40), 200);
        doc.replace(image::imageops::rotate90(&*doc.image));
        assert_eq!(doc.image.dimensions(), (80, 100));
        assert!(doc.redacted());
        let after = (*doc.image).clone();
        while doc.undo() {}
        assert_eq!(*doc.image, orig);
        assert!(!doc.modified() && !doc.redacted());
        while doc.redo() {}
        assert_eq!(*doc.image, after);
        assert!(doc.modified());
    }

    #[test]
    fn modified_follows_saved_revision() {
        let mut doc = Doc::new(noise(64, 64), 100, 1 << 30);
        paint(&mut doc, IRect::new(0, 0, 8, 8), 1);
        doc.mark_saved(doc.rev);
        assert!(!doc.modified());
        doc.undo();
        assert!(doc.modified());
        doc.redo();
        assert!(!doc.modified());
        // 改了又改回去不算一步
        let mut saved = Saved::default();
        saved.touch(&doc.image, IRect::new(0, 0, 4, 4));
        assert!(!doc.commit(saved, false));
    }

    #[test]
    fn limits_drop_oldest_steps() {
        let mut doc = Doc::new(noise(512, 512), 3, 1 << 30);
        for i in 0..5 {
            paint(&mut doc, IRect::new(0, 0, 10, 10), i);
        }
        let mut n = 0;
        while doc.undo() {
            n += 1;
        }
        assert_eq!(n, 3);
        // 打码那一步（v = 0）被丢弃后仍然算打过码
        assert!(doc.redacted());
        let mut small = Doc::new(noise(512, 512), 100, 300_000);
        for i in 1..4 {
            paint(&mut small, IRect::new(0, 0, 300, 300), i);
        }
        // 每一步都超出上限时也至少留一步
        assert_eq!(small.undo.len(), 1);
    }

    #[test]
    fn saved_region_reads_originals() {
        let img = noise(300, 300);
        let mut saved = Saved::default();
        saved.touch(&img, IRect::new(250, 250, 260, 260));
        let mut changed = img.clone();
        changed.put_pixel(255, 255, image::Rgba([1, 2, 3, 4]));
        assert_eq!(saved.original(&changed, 255, 255), img.get_pixel(255, 255).0);
        let r = IRect::new(200, 240, 290, 270);
        let region = saved.region(&changed, r);
        assert_eq!(region, read_tile(&img, r));
        saved.prune(&changed);
        assert_eq!(saved.tiles.len(), 1);
    }
}
