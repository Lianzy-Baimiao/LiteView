//! 编辑的核心（与界面无关，都有单元测试）：
//! - `history`：文档（8 位 RGBA 是唯一数据源）与撤销历史（按 256×256 分块只存改动过的块，总内存有上限）
//! - `stroke`：实时预览的一笔 / 一个框：马赛克、模糊、画笔、铅笔、橡皮擦
//! - `ops`：裁剪、调整大小、旋转翻转、填充、贴图、颜料桶
//! - `resample`：多线程的高质量缩放（可只重算一块区域，结果与整张重算完全一致）

pub mod history;
pub mod ops;
pub mod resample;
pub mod shape;
pub mod stroke;

/// 撤销历史与实时效果的分块大小
pub const TILE: u32 = 256;

/// 图片上的整数矩形（像素），右下不含
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct IRect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub fn new(x0: u32, y0: u32, x1: u32, y1: u32) -> Self {
        Self { x0, y0, x1: x1.max(x0), y1: y1.max(y0) }
    }

    pub fn from_size(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self::new(x, y, x + w, y + h)
    }

    pub fn full(w: u32, h: u32) -> Self {
        Self::new(0, 0, w, h)
    }

    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn area(&self) -> u64 {
        u64::from(self.width()) * u64::from(self.height())
    }

    pub fn intersect(&self, o: &IRect) -> IRect {
        let r = IRect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) };
        if r.is_empty() { IRect::EMPTY } else { r }
    }

    pub fn intersects(&self, o: &IRect) -> bool {
        !self.intersect(o).is_empty()
    }

    /// 同时包住两个矩形的最小矩形（空矩形不算）
    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        IRect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    /// 浮点坐标的范围（向外取整）裁到图片 w × h 之内
    pub fn around(x0: f32, y0: f32, x1: f32, y1: f32, w: u32, h: u32) -> IRect {
        let c = |v: f32, max: u32| v.clamp(0.0, max as f32) as u32;
        IRect::new(c(x0.floor(), w), c(y0.floor(), h), c(x1.ceil(), w), c(y1.ceil(), h))
    }

    /// self 减去 o：最多四块（上、下、左、右）
    pub fn subtract(&self, o: &IRect) -> Vec<IRect> {
        let i = self.intersect(o);
        if i.is_empty() {
            return if self.is_empty() { Vec::new() } else { vec![*self] };
        }
        [
            IRect::new(self.x0, self.y0, self.x1, i.y0),
            IRect::new(self.x0, i.y1, self.x1, self.y1),
            IRect::new(self.x0, i.y0, i.x0, i.y1),
            IRect::new(i.x1, i.y0, self.x1, i.y1),
        ]
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect()
    }

    /// 与这个矩形相交的分块（分块坐标）
    pub fn tiles(&self) -> impl Iterator<Item = (u32, u32)> {
        let (tx0, ty0) = (self.x0 / TILE, self.y0 / TILE);
        let (tx1, ty1) = (self.x1.div_ceil(TILE), self.y1.div_ceil(TILE));
        let empty = self.is_empty();
        (ty0..if empty { ty0 } else { ty1 }).flat_map(move |ty| (tx0..tx1).map(move |tx| (tx, ty)))
    }
}

/// 分块 (tx, ty) 在 w × h 的图上的范围
pub fn tile_rect(tx: u32, ty: u32, w: u32, h: u32) -> IRect {
    IRect::new(tx * TILE, ty * TILE, ((tx + 1) * TILE).min(w), ((ty + 1) * TILE).min(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_math() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 15, 15);
        assert_eq!(a.intersect(&b), IRect::new(5, 5, 10, 10));
        assert_eq!(a.union(&b), IRect::new(0, 0, 15, 15));
        assert!(a.intersect(&IRect::new(20, 20, 30, 30)).is_empty());
        let parts = a.subtract(&b);
        assert_eq!(parts.iter().map(IRect::area).sum::<u64>(), 100 - 25);
        assert_eq!(a.subtract(&a), Vec::<IRect>::new());
        assert_eq!(IRect::new(0, 0, 300, 10).tiles().count(), 2);
        assert_eq!(IRect::EMPTY.tiles().count(), 0);
        assert_eq!(IRect::around(-3.2, 1.5, 8.1, 4.0, 6, 6), IRect::new(0, 1, 6, 4));
        assert_eq!(tile_rect(1, 0, 300, 100), IRect::new(256, 0, 300, 100));
    }
}
