//! 生成测试用的图片：`cargo run --release --example samples -- <目录>`
//! 大照片（细条纹检查摩尔纹）、透明 PNG、动图、超过纹理上限的大图、坏文件。

use image::{Delay, Frame, Rgba, RgbaImage, codecs::gif::{GifEncoder, Repeat}};

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "samples".into()));
    std::fs::create_dir_all(&dir).unwrap();
    let photo = image::RgbImage::from_fn(4000, 3000, |x, y| {
        let (fx, fy) = (x as f32 / 4000.0, y as f32 / 3000.0);
        let stripes = if x > 3000 && ((x / 2) % 2 == 0) { 60 } else { 0 };
        let d = ((fx - 0.4).powi(2) + (fy - 0.5).powi(2)).sqrt();
        let ring = if (d * 60.0).sin() > 0.0 { 30 } else { 0 };
        image::Rgb([(fx * 200.0) as u8 + stripes, (fy * 180.0) as u8 + ring, 140 - (fx * 100.0) as u8])
    });
    photo.save(dir.join("01 photo.jpg")).unwrap();
    let alpha = RgbaImage::from_fn(640, 480, |x, y| {
        let d = ((x as f32 - 320.0).powi(2) + (y as f32 - 240.0).powi(2)).sqrt();
        Rgba([255, 120, 40, (255.0 - d.min(255.0)) as u8])
    });
    alpha.save(dir.join("02 alpha.png")).unwrap();
    let mut gif = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut gif);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for i in 0..12u32 {
            let img = RgbaImage::from_fn(320, 240, |x, y| {
                let on = (x / 20 + y / 20 + i) % 2 == 0;
                if on { Rgba([20 * i as u8, 180, 255 - 20 * i as u8, 255]) } else { Rgba([30, 30, 30, 255]) }
            });
            enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(80, 1))).unwrap();
        }
    }
    std::fs::write(dir.join("03 anim.gif"), gif).unwrap();
    let big = image::RgbImage::from_fn(17000, 6000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x / 64 + y / 64) % 2 * 200) as u8]));
    big.save(dir.join("04 big.png")).unwrap();
    std::fs::write(dir.join("05 broken.jpg"), b"\xFF\xD8\xFF\xE0 not really a jpeg").unwrap();
    println!("samples in {}", dir.display());
}
