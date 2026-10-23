//! 交给打印机的像素：比需要的清晰度高的图先用 Lanczos 缩小（与看图同一套），透明部分合成到白纸，
//! 转成 GDI 要的 24 位 DIB（BGR、每行 4 字节对齐、从下往上）。放大交给驱动（不白白多交几倍数据）。

use image::RgbaImage;
use rayon::prelude::*;

use crate::editor::IRect;
use crate::editor::resample::{Axis, Filter, render};

/// 最高按这个清晰度出图（再高肉眼看不出，数据量却成倍增加）
pub const MAX_PPI: f32 = 600.0;
/// 出图上限（像素）：大幅面绘图仪也不至于要几个 GB 内存
const MAX_PIXELS: f64 = 120e6;

/// 要交给打印机的像素尺寸：源图在 rect_mm 上超过 min(设备 dpi, MAX_PPI) 时缩小到这个清晰度，否则原样
pub fn target_px(src: [u32; 2], rect_mm: egui::Vec2, device_dpi: [f32; 2]) -> [u32; 2] {
    let inch = rect_mm / super::layout::MM_PER_INCH;
    let want = |axis: usize, len: f32| (len * device_dpi[axis].min(MAX_PPI)).max(1.0);
    let s = (want(0, inch.x) / src[0] as f32).min(want(1, inch.y) / src[1] as f32).min(1.0) as f64;
    let s = s.min((MAX_PIXELS / (f64::from(src[0]) * f64::from(src[1]))).sqrt());
    if s >= 1.0 {
        return src;
    }
    let px = |v: u32| ((f64::from(v) * s).round() as u32).max(1);
    [px(src[0]), px(src[1])]
}

/// GDI 的 24 位 DIB
pub struct Dib {
    pub width: u32,
    pub height: u32,
    /// 每行字节数（4 字节对齐）
    pub stride: usize,
    /// 从最下面一行开始
    pub bits: Vec<u8>,
}

fn stride(width: u32) -> usize {
    (width as usize * 3).div_ceil(4) * 4
}

/// src 转成 size 大小的 DIB（size 不大于 src；相等时不缩放）
pub fn to_dib(src: &RgbaImage, size: [u32; 2]) -> Dib {
    let [w, h] = size;
    let st = stride(w);
    let mut bits = vec![0u8; st * h as usize];
    if [w, h] == [src.width(), src.height()] {
        let raw = src.as_raw();
        let sw = w as usize * 4;
        bits.par_chunks_mut(st).enumerate().for_each(|(d, line)| {
            let row = &raw[(h as usize - 1 - d) * sw..][..sw];
            for (px, out) in row.as_chunks::<4>().0.iter().zip(line.as_chunks_mut::<3>().0) {
                let a = u32::from(px[3]);
                // 合成到白纸：c·a + 255·(1 − a)
                let mix = |c: u8| ((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
                out.copy_from_slice(&[mix(px[2]), mix(px[1]), mix(px[0])]);
            }
        });
        return Dib { width: w, height: h, stride: st, bits };
    }
    // 缩小：一条一条算（中间结果只有一条那么大），每条内部多线程
    let (ax, ay) = (Axis::new(src.width(), w, Filter::Lanczos3), Axis::new(src.height(), h, Filter::Lanczos3));
    let band = band_rows(src.height(), w, h);
    for y0 in (0..h).step_by(band as usize) {
        let y1 = (y0 + band).min(h);
        let pm = render(src, &ax, &ay, IRect::new(0, y0, w, y1), true);
        for (k, row) in pm.chunks_exact(w as usize * 4).enumerate() {
            let d = (h - 1 - (y0 + k as u32)) as usize;
            for (px, out) in row.as_chunks::<4>().0.iter().zip(bits[d * st..][..w as usize * 3].as_chunks_mut::<3>().0) {
                // 预乘透明度：c_pm + 255 − a
                let white = 255 - px[3];
                out.copy_from_slice(&[px[2] + white, px[1] + white, px[0] + white]);
            }
        }
    }
    Dib { width: w, height: h, stride: st, bits }
}

/// 每条多少行：中间结果（源行数 × 输出宽 × 16 字节）约 32 MB
fn band_rows(src_h: u32, w: u32, h: u32) -> u32 {
    let src_rows_per_row = (src_h as f32 / h.max(1) as f32).max(1.0);
    let budget = 32.0 * 1048576.0 / (w.max(1) as f32 * 16.0 * src_rows_per_row);
    (budget as u32).clamp(16, 512)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::resample::resize;

    fn noise(w: u32, h: u32) -> RgbaImage {
        let mut s = 7u32;
        RgbaImage::from_fn(w, h, |_, _| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let v = s.to_le_bytes();
            image::Rgba([v[0], v[1], v[2], v[3]])
        })
    }

    #[test]
    fn same_size_flips_rows_pads_and_composites_on_white() {
        let mut src = RgbaImage::new(3, 2);
        src.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
        src.put_pixel(1, 0, image::Rgba([10, 20, 30, 0]));
        src.put_pixel(2, 0, image::Rgba([0, 0, 0, 128]));
        src.put_pixel(0, 1, image::Rgba([200, 100, 50, 255]));
        let dib = to_dib(&src, [3, 2]);
        assert_eq!(dib.stride, 12);
        assert_eq!(dib.bits.len(), 24);
        // 最上面一行在最后；BGR
        assert_eq!(&dib.bits[12..15], &[30, 20, 10]);
        assert_eq!(&dib.bits[15..18], &[255, 255, 255]);
        assert_eq!(&dib.bits[18..21], &[127, 127, 127]);
        assert_eq!(&dib.bits[0..3], &[50, 100, 200]);
    }

    #[test]
    fn banded_downscale_matches_a_whole_resize() {
        let src = noise(613, 977);
        for size in [[201, 320], [37, 59], [613, 976]] {
            let dib = to_dib(&src, size);
            let full = resize(&src, size[0], size[1], Filter::Lanczos3, true);
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let i = (y * size[0] + x) as usize * 4;
                    let p = &full[i..i + 4];
                    let white = 255 - p[3];
                    let d = (size[1] - 1 - y) as usize * dib.stride + x as usize * 3;
                    assert_eq!(&dib.bits[d..d + 3], &[p[2] + white, p[1] + white, p[0] + white], "{size:?} ({x}, {y})");
                }
            }
        }
        assert!(band_rows(977, 201, 320) >= 16);
    }

    #[test]
    fn target_resolution_is_capped_and_never_upscaled() {
        // 4800 万像素放 A4 宽（8 × 6 英寸）、600 dpi 打印机：缩到 4800 × 3600
        assert_eq!(target_px([8000, 6000], egui::vec2(203.2, 152.4), [600.0, 600.0]), [4800, 3600]);
        // 1200 dpi 打印机也最多 600 ppi
        assert_eq!(target_px([8000, 6000], egui::vec2(203.2, 152.4), [1200.0, 1200.0]), [4800, 3600]);
        // 300 dpi 打印机按 300
        assert_eq!(target_px([8000, 6000], egui::vec2(203.2, 152.4), [300.0, 300.0]), [2400, 1800]);
        // 源图不够：原样交出去，由驱动放大
        assert_eq!(target_px([1200, 900], egui::vec2(203.2, 152.4), [600.0, 600.0]), [1200, 900]);
        // 比例不变
        let [w, h] = target_px([9000, 3000], egui::vec2(254.0, 84.7), [600.0, 600.0]);
        assert!((w as f32 / h as f32 - 3.0).abs() < 0.01, "{w} {h}");
        // 大幅面也有上限
        let [w, h] = target_px([30000, 20000], egui::vec2(1500.0, 1000.0), [600.0, 600.0]);
        assert!(f64::from(w) * f64::from(h) <= MAX_PIXELS * 1.01, "{w} {h}");
    }
}
