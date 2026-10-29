//! Peak commit of one decode, for working out where the memory goes while a photo opens.
//! Each run is a fresh process (the OS peak only goes up), so call it once per mode.
//! `cargo run --release --example decode_peak -- <file> <mode>`, mode =
//! - `native`: the image crate's decoder into its own buffer (what the JPEG decoder alone costs)
//! - `old`: native buffer, then `into_rgba8` (LiteView before 2026-10-29)
//! - `inplace`: straight into the RGBA buffer, expanded in place (now)
//! - `full`: `liteview::decode::decode_file` with the default options (adds the pyramid)

#[cfg(windows)]
fn peak_mb() -> (f64, f64) {
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    let _ = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (c.PeakPagefileUsage as f64 / 1048576.0, c.PagefileUsage as f64 / 1048576.0)
}

#[cfg(not(windows))]
fn peak_mb() -> (f64, f64) {
    (0.0, 0.0)
}

fn main() {
    use image::{DynamicImage, ImageDecoder, ImageReader};
    let args: Vec<String> = std::env::args().collect();
    let (path, mode) = (&args[1], args[2].as_str());
    let data = std::fs::read(path).expect("read");
    let (before, _) = peak_mb();
    let t = std::time::Instant::now();
    let reader = || ImageReader::new(std::io::Cursor::new(&data)).with_guessed_format().expect("format");
    let pixels = match mode {
        "native" => {
            let d = reader().into_decoder().expect("decoder");
            let mut buf = vec![0u8; d.total_bytes() as usize];
            d.read_image(&mut buf).expect("decode");
            buf.len()
        }
        "old" => DynamicImage::from_decoder(reader().into_decoder().expect("decoder")).expect("decode").into_rgba8().len(),
        "inplace" => {
            let d = reader().into_decoder().expect("decoder");
            let (w, h) = d.dimensions();
            let bpp = usize::from(d.color_type().bytes_per_pixel());
            let mut buf = vec![0u8; w as usize * h as usize * 4];
            d.read_image(&mut buf[..w as usize * h as usize * bpp]).expect("decode");
            buf.len()
        }
        "full" => match liteview::decode::decode_file(std::path::Path::new(path), &Default::default()).expect("decode").content {
            liteview::decode::Content::Still { image, levels } => image.len() + levels.iter().map(|l| l.len()).sum::<usize>(),
            _ => 0,
        },
        // 缩到 scale 倍（显示时的准确缩小）：以前各条同时算再拼起来 / 现在一条条算、直接写进结果
        m if m.starts_with("resample-") => {
            use liteview::editor::IRect;
            use liteview::editor::resample::{Axis, Filter, render, resize_into};
            let mut parts = m.splitn(3, '-').skip(1);
            let (way, scale): (&str, f32) = (parts.next().unwrap(), parts.next().unwrap().parse().unwrap());
            let img = image::load_from_memory(&data).expect("decode").into_rgba8();
            let (w, h) = (((img.width() as f32 * scale) as u32).max(1), ((img.height() as f32 * scale) as u32).max(1));
            let (before, _) = peak_mb();
            let t = std::time::Instant::now();
            let out = if way == "old" {
                use rayon::prelude::*;
                let (ax, ay) = (Axis::new(img.width(), w, Filter::CatmullRom), Axis::new(img.height(), h, Filter::CatmullRom));
                let band = (h as usize / (rayon::current_num_threads() * 2).max(1)).clamp(16, 256) as u32;
                let bands: Vec<u32> = (0..h).step_by(band as usize).collect();
                let parts: Vec<Vec<u8>> = bands.par_iter().map(|&y0| render(&img, &ax, &ay, IRect::new(0, y0, w, (y0 + band).min(h)), true)).collect();
                let flat = parts.concat();
                egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &flat).pixels.len()
            } else {
                let mut px = vec![egui::Color32::TRANSPARENT; w as usize * h as usize];
                resize_into(bytemuck::cast_slice_mut(&mut px), &img, w, h, Filter::CatmullRom, true);
                px.len()
            };
            let (peak, _) = peak_mb();
            println!("{mode:18} {w}x{h}: {:.0} ms  peak commit +{:.1} MB over the decoded source", t.elapsed().as_secs_f64() * 1000.0, peak - before);
            std::hint::black_box(out);
            return;
        }
        // 做测试图：同一张图存成 16 位 RGB（按扩展名选 PNG / TIFF）
        m if m.starts_with("save16=") => {
            let out = &m["save16=".len()..];
            image::load_from_memory(&data).expect("decode").into_rgb16().save(out).expect("save");
            println!("{out}");
            return;
        }
        _ => panic!("mode: native / old / inplace / full / resample-old-0.9 / resample-new-0.9 / save16=<out.png|tif>"),
    };
    let (peak, now) = peak_mb();
    println!(
        "{mode:8} {:.0} ms  result {:.1} MB  peak commit +{:.1} MB (file {:.1} MB already read)  still committed {:.1} MB",
        t.elapsed().as_secs_f64() * 1000.0,
        pixels as f64 / 1048576.0,
        peak - before,
        data.len() as f64 / 1048576.0,
        now
    );
}
