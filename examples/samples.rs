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
    std::fs::write(dir.join("06 gps.jpg"), gps_jpeg()).unwrap();
    println!("samples in {}", dir.display());
}

/// 带 EXIF（相机、GPS、缩略图）的照片：检查打码后保存是否去掉元数据
fn gps_jpeg() -> Vec<u8> {
    use exif::{Field, In, Rational, Tag, Value};
    use image::ImageEncoder;
    let img = image::RgbImage::from_fn(1600, 1200, |x, y| image::Rgb([(x / 7) as u8, (y / 5) as u8, ((x + y) / 11) as u8]));
    let mut thumb = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut thumb).encode_image(&image::imageops::thumbnail(&img, 160, 120)).unwrap();
    let r = |n| Rational { num: n, denom: 1 };
    let f = |tag, ifd_num, value| Field { tag, ifd_num, value };
    let fields = [
        f(Tag::Make, In::PRIMARY, Value::Ascii(vec![b"LiteView Test Camera".to_vec()])),
        f(Tag::Orientation, In::PRIMARY, Value::Short(vec![1])),
        f(Tag::DateTimeOriginal, In::PRIMARY, Value::Ascii(vec![b"2026:10:14 08:00:00".to_vec()])),
        f(Tag::GPSLatitudeRef, In::PRIMARY, Value::Ascii(vec![b"N".to_vec()])),
        f(Tag::GPSLatitude, In::PRIMARY, Value::Rational(vec![r(31), r(14), r(0)])),
        f(Tag::GPSLongitudeRef, In::PRIMARY, Value::Ascii(vec![b"E".to_vec()])),
        f(Tag::GPSLongitude, In::PRIMARY, Value::Rational(vec![r(121), r(28), r(0)])),
        f(Tag::Compression, In::THUMBNAIL, Value::Short(vec![6])),
    ];
    let mut w = exif::experimental::Writer::new();
    for x in &fields {
        w.push_field(x);
    }
    w.set_jpeg(&thumb, In::THUMBNAIL);
    let mut tiff = std::io::Cursor::new(Vec::new());
    w.write(&mut tiff, false).unwrap();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    enc.set_exif_metadata(tiff.into_inner()).unwrap();
    enc.write_image(img.as_raw(), 1600, 1200, image::ExtendedColorType::Rgb8).unwrap();
    out
}
