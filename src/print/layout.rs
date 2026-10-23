//! 页面几何（毫米）：纸张、打印机能印到的范围、图片放在哪里。预览和输出用同一个结果，不各写一套比例。

use egui::{Rect, Vec2, pos2, vec2};

/// 一英寸多少毫米
pub const MM_PER_INCH: f32 = 25.4;

/// 打印机报告的纸张几何（设备像素；方向已按纸张设置转好）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Device {
    pub dpi: [f32; 2],
    /// 整张纸
    pub paper_px: [i32; 2],
    /// 能印到的范围：左上角在纸上的位置、大小
    pub offset_px: [i32; 2],
    pub printable_px: [i32; 2],
}

impl Device {
    /// 还没从打印机读到时：按纸张尺寸、没有硬件边距
    pub fn nominal(paper_mm: Vec2) -> Self {
        let dpi = 600.0;
        let px = [(paper_mm.x / MM_PER_INCH * dpi).round() as i32, (paper_mm.y / MM_PER_INCH * dpi).round() as i32];
        Self { dpi: [dpi, dpi], paper_px: px, offset_px: [0, 0], printable_px: px }
    }

    pub fn paper_mm(&self) -> Vec2 {
        vec2(self.paper_px[0] as f32 / self.dpi[0], self.paper_px[1] as f32 / self.dpi[1]) * MM_PER_INCH
    }

    /// 能印到的范围（毫米，纸张左上角为原点）
    pub fn printable_mm(&self) -> Rect {
        let to = |px: i32, axis: usize| px as f32 / self.dpi[axis] * MM_PER_INCH;
        let min = pos2(to(self.offset_px[0], 0), to(self.offset_px[1], 1));
        Rect::from_min_size(min, vec2(to(self.printable_px[0], 0), to(self.printable_px[1], 1)))
    }

    /// 纸上的毫米矩形 → 打印时的设备像素（原点是能印到的范围的左上角，GDI 打印 DC 就是这样）
    pub fn to_device(&self, r: Rect) -> [i32; 4] {
        let x = |mm: f32| (mm / MM_PER_INCH * self.dpi[0]).round() as i32 - self.offset_px[0];
        let y = |mm: f32| (mm / MM_PER_INCH * self.dpi[1]).round() as i32 - self.offset_px[1];
        [x(r.min.x), y(r.min.y), x(r.max.x), y(r.max.y)]
    }

    pub fn valid(&self) -> bool {
        self.dpi.iter().all(|d| d.is_finite() && *d > 0.0)
            && self.paper_px.iter().all(|&v| v > 0)
            && self.printable_px.iter().all(|&v| v > 0)
            && self.offset_px.iter().all(|&v| v >= 0)
    }
}

/// 图片尺寸怎么定
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// 保持比例，在能印到的范围里尽量大
    Fit,
    /// 长边多少毫米（宽高锁定比例）
    LongSide(f32),
    /// 按图片记录的分辨率
    Dpi([f32; 2]),
}

/// 放好的图片
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// 图片在纸上的位置（毫米）
    pub rect: Rect,
    /// 实际打印的范围：能印到的范围扣掉用户边距（超出的部分会被裁掉）
    pub area: Rect,
    /// 超出 area 的量（左、上、右、下，毫米；都是 0 = 完整打印）
    pub over: [f32; 4],
}

impl Placement {
    pub fn overflows(&self) -> bool {
        self.over.iter().any(|&v| v > 0.05)
    }

    /// 打印出来每英寸多少个图片像素（取两个方向里小的）
    pub fn ppi(&self, image_px: [u32; 2]) -> f32 {
        let s = self.rect.size() / MM_PER_INCH;
        (image_px[0] as f32 / s.x).min(image_px[1] as f32 / s.y)
    }
}

/// 把 image_px（显示方向的图片像素）放到纸上。margin = 用户边距（毫米，四边相同，叠在硬件边距上）
pub fn place(dev: &Device, margin: f32, image_px: [u32; 2], size: Size) -> Option<Placement> {
    if !dev.valid() || image_px[0] == 0 || image_px[1] == 0 || !margin.is_finite() {
        return None;
    }
    let paper = Rect::from_min_size(pos2(0.0, 0.0), dev.paper_mm());
    let area = dev.printable_mm().intersect(paper.shrink(margin.max(0.0)));
    if !(area.width() > 0.0 && area.height() > 0.0) {
        return None;
    }
    let (iw, ih) = (image_px[0] as f32, image_px[1] as f32);
    let wh = match size {
        Size::Fit => {
            let s = (area.width() / iw).min(area.height() / ih);
            vec2(iw * s, ih * s)
        }
        Size::LongSide(mm) => {
            if !(mm.is_finite() && mm > 0.0) {
                return None;
            }
            let s = mm / iw.max(ih);
            vec2(iw * s, ih * s)
        }
        Size::Dpi([dx, dy]) => {
            if !(dx.is_finite() && dy.is_finite() && dx > 0.0 && dy > 0.0) {
                return None;
            }
            vec2(iw / dx, ih / dy) * MM_PER_INCH
        }
    };
    // 在纸上居中；能放进 area 的方向最小平移进去（硬件边距不对称时）
    let x = axis(paper.center().x, wh.x, area.min.x, area.max.x);
    let y = axis(paper.center().y, wh.y, area.min.y, area.max.y);
    let rect = Rect::from_min_size(pos2(x, y), wh);
    let over = [area.min.x - rect.min.x, area.min.y - rect.min.y, rect.max.x - area.max.x, rect.max.y - area.max.y].map(|v| v.max(0.0));
    Some(Placement { rect, area, over })
}

/// 一个方向上的起点：以 center 居中；长度不超过 [lo, hi] 时挪进去，超过时保持居中
fn axis(center: f32, len: f32, lo: f32, hi: f32) -> f32 {
    let start = center - len / 2.0;
    if len <= hi - lo + 1e-3 { start.max(lo).min(hi - len) } else { start }
}

/// 方向「自动」：图比较宽就横向
pub fn landscape(auto: Option<bool>, image_px: [u32; 2]) -> bool {
    auto.unwrap_or(image_px[0] > image_px[1])
}

/// 照片尺寸快捷填写：框（毫米，不分横竖）按图片方向转过来后，能放进去的同比例图的长边
pub fn fit_long_side(box_mm: [f32; 2], image_px: [u32; 2]) -> f32 {
    let (short, long) = (box_mm[0].min(box_mm[1]), box_mm[0].max(box_mm[1]));
    let (iw, ih) = (image_px[0].max(1) as f32, image_px[1].max(1) as f32);
    let (bw, bh) = if iw >= ih { (long, short) } else { (short, long) };
    let s = (bw / iw).min(bh / ih);
    iw.max(ih) * s
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4: Vec2 = Vec2::new(210.0, 297.0);

    /// 600 dpi，四边各 margin_mm 的硬件边距（左上 / 右下可以不同）
    fn printer(paper: Vec2, lt: [f32; 2], rb: [f32; 2]) -> Device {
        let px = |mm: f32| (mm / MM_PER_INCH * 600.0).round() as i32;
        let paper_px = [px(paper.x), px(paper.y)];
        let offset_px = [px(lt[0]), px(lt[1])];
        let printable_px = [paper_px[0] - offset_px[0] - px(rb[0]), paper_px[1] - offset_px[1] - px(rb[1])];
        Device { dpi: [600.0, 600.0], paper_px, offset_px, printable_px }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.1
    }

    #[test]
    fn fit_keeps_ratio_centers_and_never_overflows() {
        let dev = printer(A4, [5.0, 5.0], [5.0, 5.0]);
        for px in [[4000, 3000], [3000, 4000], [8000, 100], [100, 8000], [1, 1], [640, 480]] {
            let p = place(&dev, 0.0, px, Size::Fit).unwrap();
            assert!(!p.overflows(), "{px:?}: {p:?}");
            assert!(close(p.rect.width() / p.rect.height(), px[0] as f32 / px[1] as f32) || p.rect.height() < 1.0);
            // 一边贴满
            assert!(close(p.rect.width(), 200.0) || close(p.rect.height(), 287.0), "{px:?}: {:?}", p.rect);
            assert!(close(p.rect.center().x, 105.0) && close(p.rect.center().y, 148.5), "{px:?}: {:?}", p.rect);
        }
    }

    #[test]
    fn asymmetric_hardware_margins_shift_only_as_needed() {
        // 上 3 mm、下 17 mm：贴满高度的长条图只能放在能印的范围里（不居中于纸）
        let dev = printer(A4, [4.0, 3.0], [4.0, 17.0]);
        let p = place(&dev, 0.0, [1000, 4000], Size::Fit).unwrap();
        assert!(!p.overflows());
        assert!(close(p.rect.min.y, 3.0) && close(p.rect.max.y, 280.0), "{:?}", p.rect);
        // 小图仍按纸居中
        let small = place(&dev, 0.0, [1000, 1000], Size::LongSide(50.0)).unwrap();
        assert!(close(small.rect.center().x, 105.0) && close(small.rect.center().y, 148.5), "{:?}", small.rect);
    }

    #[test]
    fn user_margin_adds_to_hardware_margin() {
        let dev = printer(A4, [3.0, 3.0], [3.0, 3.0]);
        let p = place(&dev, 10.0, [4000, 3000], Size::Fit).unwrap();
        assert!(close(p.area.min.x, 10.0) && close(p.area.max.x, 200.0), "{:?}", p.area);
        assert!(close(p.rect.width(), 190.0));
        // 用户边距比硬件小：按硬件
        let q = place(&dev, 1.0, [4000, 3000], Size::Fit).unwrap();
        assert!(close(q.area.min.x, 3.0), "{:?}", q.area);
        // 边距大到没地方印
        assert!(place(&dev, 120.0, [4000, 3000], Size::Fit).is_none());
    }

    #[test]
    fn explicit_sizes_report_overflow_instead_of_cropping() {
        let dev = printer(A4, [5.0, 5.0], [5.0, 5.0]);
        let ok = place(&dev, 0.0, [4000, 3000], Size::LongSide(152.0)).unwrap();
        assert!(!ok.overflows() && close(ok.rect.width(), 152.0) && close(ok.rect.height(), 114.0), "{ok:?}");
        let big = place(&dev, 0.0, [4000, 3000], Size::LongSide(260.0)).unwrap();
        assert!(big.overflows());
        // 宽 260 > 200：左右各超出 30，仍居中；高 195 放得下
        assert!(close(big.over[0], 30.0) && close(big.over[2], 30.0) && big.over[1] == 0.0 && big.over[3] == 0.0, "{:?}", big.over);
        // 300 dpi 的 2480 × 3508（A4 扫描件）正好一张 A4
        let scan = place(&printer(A4, [0.0, 0.0], [0.0, 0.0]), 0.0, [2480, 3508], Size::Dpi([300.0, 300.0])).unwrap();
        assert!(close(scan.rect.width(), 210.0) && close(scan.rect.height(), 297.0) && !scan.overflows(), "{scan:?}");
        // 手机照片常写 72 dpi：算出来很大，报告超出
        let phone = place(&dev, 0.0, [4032, 3024], Size::Dpi([72.0, 72.0])).unwrap();
        assert!(phone.overflows() && close(phone.rect.width(), 1422.4), "{:?}", phone.rect);
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let dev = printer(A4, [5.0, 5.0], [5.0, 5.0]);
        assert!(place(&dev, 0.0, [0, 100], Size::Fit).is_none());
        assert!(place(&dev, f32::NAN, [100, 100], Size::Fit).is_none());
        for s in [Size::LongSide(0.0), Size::LongSide(f32::INFINITY), Size::Dpi([0.0, 300.0]), Size::Dpi([f32::NAN, 300.0])] {
            assert!(place(&dev, 0.0, [100, 100], s).is_none(), "{s:?}");
        }
        let mut bad = dev;
        bad.dpi = [0.0, 600.0];
        assert!(place(&bad, 0.0, [100, 100], Size::Fit).is_none());
    }

    #[test]
    fn device_mapping_and_ppi() {
        let dev = printer(A4, [5.0, 5.0], [5.0, 5.0]);
        let p = place(&dev, 0.0, [2400, 1800], Size::LongSide(203.2)).unwrap();
        // 8 英寸放 2400 像素 = 300 ppi
        assert!(close(p.ppi([2400, 1800]), 300.0), "{}", p.ppi([2400, 1800]));
        let [x0, y0, x1, y1] = dev.to_device(p.rect);
        assert!((x1 - x0 - 4800).abs() <= 1 && (y1 - y0 - 3600).abs() <= 1, "{:?}", [x0, y0, x1, y1]);
        // 原点是能印到的范围的左上角
        assert_eq!(dev.to_device(Rect::from_min_size(pos2(5.0, 5.0), vec2(1.0, 1.0)))[..2], [0, 0]);
        let nominal = Device::nominal(A4);
        assert!(close(nominal.paper_mm().x, 210.0) && nominal.printable_mm() == Rect::from_min_size(pos2(0.0, 0.0), nominal.paper_mm()));
    }

    #[test]
    fn photo_presets_follow_the_image_orientation() {
        // 6 寸 102 × 152：4:3 横图 136 × 102，竖图 102 × 136（框的短边先顶住）；3:2 横图正好 152 × 101
        assert!(close(fit_long_side([102.0, 152.0], [4000, 3000]), 136.0));
        assert!(close(fit_long_side([102.0, 152.0], [3000, 4000]), 136.0));
        assert!(close(fit_long_side([152.0, 102.0], [3000, 2000]), 152.0));
        assert!(close(fit_long_side([210.0, 297.0], [2000, 3000]), 297.0));
        assert!(landscape(None, [4000, 3000]) && !landscape(None, [3000, 4000]) && !landscape(None, [100, 100]));
        assert!(!landscape(Some(false), [4000, 3000]));
    }
}
