//! 解码器变异 fuzz（开发用）：父进程起若干子进程，子进程按序号确定地生成变异的文件并在本进程里解码
//! （与看图时一样，包括系统 WIC 兜底）；子进程崩溃、卡住、一次解码超过 3 s 或提交内存涨 600 MB 以上都记下来。
//! `cargo run --release --example fuzz_decode -- <输出目录> <秒数> [并发]`；`… -- one <序号> <文件名>` 写出某个变异复现；
//! `… -- seeds` 列出种子。2026-10-28 跑 10 分钟、4 个进程、571 万个文件，没有发现问题。
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use liteview::decode::{Options, decode_bytes};

fn seeds() -> Vec<(&'static str, Vec<u8>)> {
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    let img = RgbaImage::from_fn(37, 23, |x, y| Rgba([(x * 7) as u8, (y * 11) as u8, ((x ^ y) * 5) as u8, if (x + y) % 5 == 0 { 120 } else { 255 }]));
    let mut out = Vec::new();
    for (ext, fmt) in [("png", ImageFormat::Png), ("bmp", ImageFormat::Bmp), ("tiff", ImageFormat::Tiff), ("ico", ImageFormat::Ico), ("tga", ImageFormat::Tga),
        ("qoi", ImageFormat::Qoi), ("webp", ImageFormat::WebP), ("ff", ImageFormat::Farbfeld), ("exr", ImageFormat::OpenExr)] {
        let mut b = std::io::Cursor::new(Vec::new());
        let d = if fmt == ImageFormat::OpenExr { DynamicImage::ImageRgba32F(DynamicImage::ImageRgba8(img.clone()).to_rgba32f()) } else { DynamicImage::ImageRgba8(img.clone()) };
        if d.write_to(&mut b, fmt).is_ok() {
            out.push((ext, b.into_inner()));
        }
    }
    for (ext, fmt) in [("jpg", ImageFormat::Jpeg), ("gif", ImageFormat::Gif), ("pnm", ImageFormat::Pnm), ("hdr", ImageFormat::Hdr)] {
        let mut b = std::io::Cursor::new(Vec::new());
        let d = if fmt == ImageFormat::Hdr { DynamicImage::ImageRgb32F(DynamicImage::ImageRgba8(img.clone()).to_rgb32f()) } else { DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(img.clone()).to_rgb8()) };
        if d.write_to(&mut b, fmt).is_ok() {
            out.push((ext, b.into_inner()));
        }
    }
    for f in ["animation.png", "animation.webp", "animation.jxl", "poster.png"] {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(f);
        out.push((if f.ends_with("jxl") { "jxl" } else if f.ends_with("webp") { "webp" } else { "png" }, std::fs::read(p).unwrap()));
    }
    let mut png = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(img.clone()).write_to(&mut png, ImageFormat::Png).unwrap();
    let b64 = base64_encode(&png.into_inner());
    out.push(("svg", format!(r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="200" height="120" viewBox="0 0 200 120"><defs><linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><filter id="f"><feGaussianBlur stdDeviation="3"/></filter></defs><rect width="200" height="120" fill="url(#g)"/><circle cx="60" cy="60" r="30" filter="url(#f)" fill="#0a0"/><text x="10" y="110" font-size="14">Hi 你好</text><image x="120" y="20" width="60" height="40" xlink:href="data:image/png;base64,{b64}"/></svg>"##).into_bytes()));
    // 最小的 PSD：8BPS v1，3 通道 8 位 RGB，不压缩
    let (w, h) = (4u32, 3u32);
    let mut psd = b"8BPS".to_vec();
    psd.extend_from_slice(&1u16.to_be_bytes());
    psd.extend_from_slice(&[0; 6]);
    psd.extend_from_slice(&3u16.to_be_bytes());
    psd.extend_from_slice(&h.to_be_bytes());
    psd.extend_from_slice(&w.to_be_bytes());
    psd.extend_from_slice(&8u16.to_be_bytes());
    psd.extend_from_slice(&3u16.to_be_bytes());
    psd.extend_from_slice(&[0; 12]);
    psd.extend_from_slice(&0u16.to_be_bytes());
    psd.extend((0..w * h * 3).map(|i| (i * 13) as u8));
    out.push(("psd", psd));
    out
}

fn base64_encode(d: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in d.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            s.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(seeds: &[(&'static str, Vec<u8>)], case: u64) -> (&'static str, Vec<u8>) {
    let mut r = Rng(case.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0x1234_5678);
    let (ext, base) = &seeds[r.below(seeds.len())];
    let mut d = base.clone();
    for _ in 0..1 + r.below(8) {
        if d.is_empty() {
            break;
        }
        let at = r.below(d.len());
        match r.below(9) {
            0 => d[at] ^= 1 << r.below(8),
            1 => d[at] = r.next() as u8,
            2 => {
                let v = [0u8, 0xFF, 0x7F, 0x80, 0x01][r.below(5)];
                let n = 1 + r.below(4);
                for b in d.iter_mut().skip(at).take(n) {
                    *b = v;
                }
            }
            // 大小字段：写一个很大 / 很小的 16 或 32 位数（大端、小端都试）
            3 => {
                let v: u32 = [0x7FFF_FFFF, 0xFFFF_FFFF, 0x0000_FFFF, 0x0001_0000, 0x4000_0000, 0, 1][r.below(7)];
                let bytes = if r.below(2) == 0 { v.to_be_bytes() } else { v.to_le_bytes() };
                let n = if r.below(2) == 0 { 4 } else { 2 };
                for (i, b) in bytes.iter().take(n).enumerate() {
                    if at + i < d.len() {
                        d[at + i] = *b;
                    }
                }
            }
            4 => d.truncate(at.max(1)),
            5 => {
                let n = 1 + r.below(64.min(d.len() - at));
                d.drain(at..at + n);
            }
            6 => {
                let n = 1 + r.below(64.min(d.len() - at));
                let chunk = d[at..at + n].to_vec();
                let to = r.below(d.len());
                d.splice(to..to, chunk);
            }
            7 => {
                let (_, other) = &seeds[r.below(seeds.len())];
                if !other.is_empty() {
                    let from = r.below(other.len());
                    let n = 1 + r.below(64.min(other.len() - from));
                    d.splice(at..at, other[from..from + n].iter().copied());
                }
            }
            _ => {
                let to = r.below(d.len());
                d.swap(at, to);
            }
        }
    }
    // 有时扩展名换成别的（走另一条嗅探 / 解码路径）
    let ext = if r.below(6) == 0 { seeds[r.below(seeds.len())].0 } else { ext };
    (ext, d)
}

/// 其它系统不量内存（只查崩溃、卡住、慢）
#[cfg(not(windows))]
fn peak_commit_mb() -> f64 {
    0.0
}

#[cfg(windows)]
fn peak_commit_mb() -> f64 {
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS::default();
    unsafe {
        let _ = K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32);
    }
    c.PeakPagefileUsage as f64 / 1048576.0
}

fn child(from: u64, to: u64) {
    let seeds = seeds();
    let opts = Options { max_alloc: 512 << 20, anim_cache: 64 << 20, ..Options::default() };
    let mut out = std::io::stdout().lock();
    for case in from..to {
        let (ext, data) = mutate(&seeds, case);
        let before = peak_commit_mb();
        writeln!(out, "B {case}").unwrap();
        out.flush().unwrap();
        let t = Instant::now();
        let r = decode_bytes(&data, ext, &opts);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let kind = match &r {
            Ok(_) => "ok".to_string(),
            Err(e) => format!("{e:?}").chars().take(40).collect(),
        };
        writeln!(out, "E {case} {ms:.1} {:.0} {kind}", peak_commit_mb() - before).unwrap();
        out.flush().unwrap();
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "child" {
        return child(a[2].parse().unwrap(), a[3].parse().unwrap());
    }
    if a[1] == "seeds" {
        for (ext, data) in seeds() {
            println!("{ext} {} bytes", data.len());
        }
        return;
    }
    if a[1] == "one" {
        // one <case> <file>：把这个变异写出来（复现用）
        let (ext, data) = mutate(&seeds(), a[2].parse().unwrap());
        std::fs::write(format!("{}.{ext}", a[3]), data).unwrap();
        return;
    }
    let out = std::path::PathBuf::from(&a[1]);
    std::fs::create_dir_all(&out).unwrap();
    let secs: u64 = a[2].parse().unwrap();
    let workers: u64 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(4);
    let batch = 500u64;
    let deadline = Instant::now() + Duration::from_secs(secs);
    let next = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let report = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let exe = std::env::current_exe().unwrap();
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let (next, report, exe) = (next.clone(), report.clone(), exe.clone());
            std::thread::spawn(move || {
                let mut done = 0u64;
                while Instant::now() < deadline {
                    let from = next.fetch_add(batch, std::sync::atomic::Ordering::SeqCst);
                    let mut start = from;
                    // 子进程崩溃 / 卡住后从下一个序号接着跑
                    while start < from + batch && Instant::now() < deadline {
                        let mut c = std::process::Command::new(&exe).args(["child", &start.to_string(), &(from + batch).to_string()])
                            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().unwrap();
                        let stdout = c.stdout.take().unwrap();
                        let (tx, rx) = std::sync::mpsc::channel::<String>();
                        std::thread::spawn(move || {
                            for l in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                                if tx.send(l).is_err() {
                                    break;
                                }
                            }
                        });
                        let mut open: Option<u64> = None;
                        let mut last = start;
                        loop {
                            match rx.recv_timeout(Duration::from_secs(10)) {
                                Ok(l) => {
                                    let f: Vec<&str> = l.splitn(5, ' ').collect();
                                    match f[0] {
                                        "B" => open = f[1].parse().ok(),
                                        "E" => {
                                            let case: u64 = f[1].parse().unwrap();
                                            let (ms, mb): (f64, f64) = (f[2].parse().unwrap(), f[3].parse().unwrap());
                                            if ms > 3000.0 || mb > 600.0 {
                                                report.lock().unwrap().push(format!("slow/memory case {case}: {ms:.0} ms, +{mb:.0} MB commit, {}", f.get(4).unwrap_or(&"")));
                                            }
                                            open = None;
                                            last = case + 1;
                                            done += 1;
                                        }
                                        _ => {}
                                    }
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                    let _ = c.kill();
                                    report.lock().unwrap().push(format!("HANG case {:?} (no output for 10 s)", open));
                                    last = open.map_or(last, |o| o + 1);
                                    break;
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                    let status = c.wait().unwrap();
                                    if let Some(o) = open {
                                        report.lock().unwrap().push(format!("CRASH case {o}: {status:?}"));
                                        last = o + 1;
                                    }
                                    break;
                                }
                            }
                        }
                        let _ = c.wait();
                        start = last;
                    }
                }
                done
            })
        })
        .collect();
    let total: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let report = report.lock().unwrap();
    let text = format!("decoded {total} mutated files; findings: {}\n{}\n", report.len(), report.join("\n"));
    std::fs::write(out.join("report.txt"), &text).unwrap();
    print!("{text}");
}
