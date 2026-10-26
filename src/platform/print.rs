//! Windows 打印：列打印机、读纸张与可打印范围、驱动自己的「属性」对话框、用 GDI 输出一页。
//! 只在打开打印对话框后用（启动时不碰打印服务）；读打印机可能很慢（网络打印机），调用方放在后台线程。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use image::RgbaImage;
use windows::Win32::Foundation::{E_FAIL, ERROR_CANCELLED, ERROR_INSUFFICIENT_BUFFER, HWND, POINT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Printing::*;
use windows::Win32::Storage::Xps::*;
use windows::Win32::UI::WindowsAndMessaging::IDOK;
use windows::core::{HRESULT, PCWSTR, PWSTR};

use crate::print::layout::{Device, Placement};
use crate::print::raster;
use crate::viewer::view::Transform;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 以 0 结尾（或填满）的 UTF-16
fn from_wide(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

unsafe fn from_pwstr(p: PWSTR) -> String {
    if p.is_null() { String::new() } else { unsafe { p.to_string().unwrap_or_default() } }
}

fn last_error() -> windows::core::Error {
    windows::core::Error::from_thread()
}

/// 打印服务（Print Spooler）没在运行：RPC_S_SERVER_UNAVAILABLE
pub fn spooler_stopped(e: &windows::core::Error) -> bool {
    e.code() == HRESULT::from_win32(1722)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Printer {
    pub name: String,
    pub default: bool,
}

/// 本机和连接的打印机，默认打印机排第一（其余按系统顺序）
pub fn printers() -> windows::core::Result<Vec<Printer>> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (mut need, mut n) = (0u32, 0u32);
    if let Err(e) = unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut need, &mut n) }
        && e.code() != HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0)
    {
        return Err(e);
    }
    if need == 0 {
        return Ok(Vec::new());
    }
    // 按 8 字节对齐（里面是指针）
    let mut buf = vec![0u64; (need as usize).div_ceil(8)];
    {
        let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len() * 8) };
        unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, Some(bytes), &mut need, &mut n)? };
    }
    let infos = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<PRINTER_INFO_4W>(), n as usize) };
    let default = default_printer();
    let mut list: Vec<Printer> = infos
        .iter()
        .map(|i| unsafe { from_pwstr(i.pPrinterName) })
        .filter(|name| !name.is_empty())
        .map(|name| Printer { default: default.as_deref() == Some(name.as_str()), name })
        .collect();
    list.sort_by_key(|p| !p.default);
    Ok(list)
}

fn default_printer() -> Option<String> {
    let mut len = 0u32;
    let _ = unsafe { GetDefaultPrinterW(None, &mut len) };
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    unsafe { GetDefaultPrinterW(Some(PWSTR(buf.as_mut_ptr())), &mut len) }.as_bool().then(|| from_wide(&buf))
}

/// 打开的打印机（用完自动关）
struct Handle(PRINTER_HANDLE);

impl Handle {
    fn open(name: &str) -> windows::core::Result<Self> {
        let w = wide(name);
        let mut h = PRINTER_HANDLE::default();
        unsafe { OpenPrinterW(PCWSTR(w.as_ptr()), &mut h, None)? };
        Ok(Self(h))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { ClosePrinter(self.0) };
    }
}

/// 驱动的打印设置（DEVMODE 连同驱动私有的部分）
#[derive(Clone)]
pub struct DevMode {
    /// 按 8 字节对齐
    buf: Vec<u64>,
}

impl DevMode {
    fn alloc(bytes: usize) -> Self {
        Self { buf: vec![0u64; bytes.max(std::mem::size_of::<DEVMODEW>()).div_ceil(8)] }
    }

    fn ptr(&self) -> *const DEVMODEW {
        self.buf.as_ptr().cast()
    }

    fn ptr_mut(&mut self) -> *mut DEVMODEW {
        self.buf.as_mut_ptr().cast()
    }

    /// 驱动要的大小（负数 = 出错）
    fn size_for(h: &Handle, name: &[u16]) -> windows::core::Result<usize> {
        let n = unsafe { DocumentPropertiesW(None, h.0, PCWSTR(name.as_ptr()), None, None, 0) };
        if n <= 0 { Err(last_error()) } else { Ok(n as usize) }
    }

    /// 打印机的默认设置
    fn defaults(h: &Handle, name: &[u16]) -> windows::core::Result<Self> {
        let mut dm = Self::alloc(Self::size_for(h, name)?);
        let r = unsafe { DocumentPropertiesW(None, h.0, PCWSTR(name.as_ptr()), Some(dm.ptr_mut()), None, DM_OUT_BUFFER.0) };
        if r < 0 { Err(last_error()) } else { Ok(dm) }
    }

    pub fn paper(&self) -> i16 {
        unsafe { (*self.ptr()).Anonymous1.Anonymous1.dmPaperSize }
    }

    pub fn landscape(&self) -> bool {
        unsafe { i32::from((*self.ptr()).Anonymous1.Anonymous1.dmOrientation) == DMORIENT_LANDSCAPE as i32 }
    }

    /// 改纸张（0 = 不改）、方向、份数；自定义纸张尺寸的字段清掉，免得盖过纸张编号
    pub fn set(&mut self, paper: i16, landscape: bool, copies: i16) {
        let dm = unsafe { &mut *self.ptr_mut() };
        let mut fields = dm.dmFields.0 & !(DM_PAPERLENGTH.0 | DM_PAPERWIDTH.0);
        let p = unsafe { &mut dm.Anonymous1.Anonymous1 };
        if paper != 0 {
            p.dmPaperSize = paper;
            fields |= DM_PAPERSIZE.0;
        }
        p.dmOrientation = (if landscape { DMORIENT_LANDSCAPE } else { DMORIENT_PORTRAIT }) as i16;
        p.dmCopies = copies.max(1);
        dm.dmFields = DEVMODE_FIELD_FLAGS(fields | DM_ORIENTATION.0 | DM_COPIES.0);
    }

    /// 交给驱动核对（不支持的组合由驱动改成能用的）
    fn merged(&self, h: &Handle, name: &[u16]) -> windows::core::Result<Self> {
        let mut out = Self::alloc(Self::size_for(h, name)?);
        let r = unsafe { DocumentPropertiesW(None, h.0, PCWSTR(name.as_ptr()), Some(out.ptr_mut()), Some(self.ptr()), DM_IN_BUFFER.0 | DM_OUT_BUFFER.0) };
        if r < 0 { Err(last_error()) } else { Ok(out) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Paper {
    /// DMPAPER_*
    pub id: i16,
    /// 驱动给的名字（随系统语言）
    pub name: String,
    /// 纵向的宽、高（毫米）
    pub mm: [f32; 2],
}

/// 一台打印机能做什么
#[derive(Clone)]
pub struct Caps {
    pub papers: Vec<Paper>,
    pub defaults: DevMode,
    /// 驱动一次能印几份（1 = 我们自己重复输出）
    pub max_copies: u32,
    /// 打印到文件（Microsoft Print to PDF、XPS 等）：先问存到哪里，不让驱动在后台线程弹窗
    pub to_file: bool,
}

pub fn caps(printer: &str) -> windows::core::Result<Caps> {
    let h = Handle::open(printer)?;
    let name = wide(printer);
    let defaults = DevMode::defaults(&h, &name)?;
    let port = port_of(&h).unwrap_or_default();
    let (device, wport) = (PCWSTR(name.as_ptr()), wide(&port));
    let port_w = PCWSTR(wport.as_ptr());
    let count = |cap| unsafe { DeviceCapabilitiesW(device, port_w, cap, None, Some(defaults.ptr())) };
    let n = count(DC_PAPERS).max(0) as usize;
    let mut ids = vec![0u16; n];
    let mut names = vec![0u16; n * 64];
    let mut sizes = vec![POINT::default(); n];
    if n > 0 {
        unsafe {
            DeviceCapabilitiesW(device, port_w, DC_PAPERS, Some(PWSTR(ids.as_mut_ptr())), Some(defaults.ptr()));
            DeviceCapabilitiesW(device, port_w, DC_PAPERNAMES, Some(PWSTR(names.as_mut_ptr())), Some(defaults.ptr()));
            DeviceCapabilitiesW(device, port_w, DC_PAPERSIZE, Some(PWSTR(sizes.as_mut_ptr().cast())), Some(defaults.ptr()));
        }
    }
    let papers = (0..n)
        .filter(|&i| sizes[i].x > 0 && sizes[i].y > 0)
        .map(|i| Paper { id: ids[i] as i16, name: from_wide(&names[i * 64..(i + 1) * 64]), mm: [sizes[i].x as f32 / 10.0, sizes[i].y as f32 / 10.0] })
        .collect();
    let max_copies = count(DC_COPIES).max(1) as u32;
    let to_file = port.split(',').any(|p| matches!(p.trim().to_ascii_uppercase().as_str(), "FILE:" | "PORTPROMPT:"));
    Ok(Caps { papers, defaults, max_copies, to_file })
}

fn port_of(h: &Handle) -> Option<String> {
    let mut need = 0u32;
    let _ = unsafe { GetPrinterW(h.0, 5, None, &mut need) };
    if need == 0 {
        return None;
    }
    let mut buf = vec![0u64; (need as usize).div_ceil(8)];
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len() * 8) };
    unsafe { GetPrinterW(h.0, 5, Some(bytes), &mut need) }.ok()?;
    let info = unsafe { &*buf.as_ptr().cast::<PRINTER_INFO_5W>() };
    Some(unsafe { from_pwstr(info.pPortName) })
}

/// 这些设置下的纸张几何（驱动核对过的设置一起返回）
pub fn device(printer: &str, dm: &DevMode) -> windows::core::Result<(Device, DevMode)> {
    let h = Handle::open(printer)?;
    let name = wide(printer);
    let merged = dm.merged(&h, &name)?;
    let dc = unsafe { CreateICW(PCWSTR::null(), PCWSTR(name.as_ptr()), PCWSTR::null(), Some(merged.ptr())) };
    if dc.is_invalid() {
        return Err(last_error());
    }
    let dev = read_device(dc);
    let _ = unsafe { DeleteDC(dc) };
    if !dev.valid() {
        return Err(windows::core::Error::new(E_FAIL, "printer reported no paper size"));
    }
    Ok((dev, merged))
}

fn read_device(dc: HDC) -> Device {
    let cap = |i| unsafe { GetDeviceCaps(Some(dc), i) };
    Device {
        dpi: [cap(LOGPIXELSX) as f32, cap(LOGPIXELSY) as f32],
        paper_px: [cap(PHYSICALWIDTH), cap(PHYSICALHEIGHT)],
        offset_px: [cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY)],
        printable_px: [cap(HORZRES), cap(VERTRES)],
    }
}

/// 驱动自己的「属性」对话框（在主线程调用，owner = 主窗口）。取消或出错返回 None
pub fn properties(owner: Option<isize>, printer: &str, dm: &DevMode) -> Option<DevMode> {
    let h = Handle::open(printer).ok()?;
    let name = wide(printer);
    let mut out = DevMode::alloc(DevMode::size_for(&h, &name).ok()?);
    let hwnd = owner.map(|h| HWND(h as *mut _));
    let r = unsafe { DocumentPropertiesW(hwnd, h.0, PCWSTR(name.as_ptr()), Some(out.ptr_mut()), Some(dm.ptr()), DM_IN_BUFFER.0 | DM_IN_PROMPT.0 | DM_OUT_BUFFER.0) };
    (r == IDOK.0).then_some(out)
}

/// 一次打印：点「打印」时定下来，交给后台线程
pub struct Job {
    pub printer: String,
    /// 已含纸张、方向（份数在这里设）
    pub devmode: DevMode,
    /// 打印队列里显示的名字
    pub title: String,
    /// 打印到文件时存到哪里
    pub output: Option<std::path::PathBuf>,
    pub copies: u32,
    /// 驱动一次能印几份
    pub max_copies: u32,
    /// 原图像素与显示方向（编辑画布为不变换）
    pub image: Arc<RgbaImage>,
    pub transform: Transform,
    pub placement: Placement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 已交给打印队列（不等于已经印完）
    Sent,
    Cancelled,
}

pub fn run(job: Job, cancel: &AtomicBool) -> windows::core::Result<Outcome> {
    let copies = job.copies.clamp(1, 999);
    // 驱动印不了这么多份：每份一页，自己重复
    let by_driver = copies <= job.max_copies;
    let mut dm = job.devmode.clone();
    let (paper, landscape) = (dm.paper(), dm.landscape());
    dm.set(paper, landscape, if by_driver { copies as i16 } else { 1 });
    let name = wide(&job.printer);
    let dm = dm.merged(&Handle::open(&job.printer)?, &name)?;
    // 先把像素准备好（几百毫秒到几秒），再开始打印任务
    let rotated = (!job.transform.is_identity()).then(|| crate::save::apply_transform(&job.image, job.transform));
    let src = rotated.as_ref().unwrap_or(&*job.image);
    let dc = unsafe { CreateDCW(PCWSTR::null(), PCWSTR(name.as_ptr()), PCWSTR::null(), Some(dm.ptr())) };
    if dc.is_invalid() {
        return Err(last_error());
    }
    let dev = read_device(dc);
    let result = (|| {
        if !dev.valid() {
            return Err(windows::core::Error::new(E_FAIL, "printer reported no paper size"));
        }
        let size = raster::target_px([src.width(), src.height()], job.placement.rect.size(), dev.dpi);
        let dib = raster::to_dib(src, size);
        if cancel.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled);
        }
        let title = wide(&job.title);
        let output: Option<Vec<u16>> = job.output.as_ref().map(|p| {
            use std::os::windows::ffi::OsStrExt;
            p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
        });
        let info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(title.as_ptr()),
            lpszOutput: output.as_ref().map_or(PCWSTR::null(), |o| PCWSTR(o.as_ptr())),
            ..Default::default()
        };
        if unsafe { StartDocW(dc, &info) } <= 0 {
            let e = last_error();
            // 驱动自己弹的「存到哪里」被取消
            return if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) { Ok(Outcome::Cancelled) } else { Err(e) };
        }
        let pages = if by_driver { 1 } else { copies };
        for _ in 0..pages {
            let page = (|| {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(false);
                }
                if unsafe { StartPage(dc) } <= 0 {
                    return Err(last_error());
                }
                draw(dc, &dev, &job.placement, &dib)?;
                if unsafe { EndPage(dc) } <= 0 {
                    return Err(last_error());
                }
                Ok(true)
            })();
            match page {
                Ok(true) => {}
                Ok(false) => {
                    unsafe { AbortDoc(dc) };
                    return Ok(Outcome::Cancelled);
                }
                Err(e) => {
                    unsafe { AbortDoc(dc) };
                    return Err(e);
                }
            }
        }
        if unsafe { EndDoc(dc) } <= 0 {
            return Err(last_error());
        }
        Ok(Outcome::Sent)
    })();
    let _ = unsafe { DeleteDC(dc) };
    result
}

/// 画一页：只在实际打印的范围里画（与预览一样，超出的部分裁掉）；放大交给驱动的 HALFTONE
fn draw(dc: HDC, dev: &Device, p: &Placement, dib: &raster::Dib) -> windows::core::Result<()> {
    let [x0, y0, x1, y1] = dev.to_device(p.rect);
    let [c0, c1, c2, c3] = dev.to_device(p.area);
    unsafe {
        IntersectClipRect(dc, c0, c1, c2, c3);
        SetStretchBltMode(dc, HALFTONE);
        let _ = SetBrushOrgEx(dc, 0, 0, None);
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: dib.width as i32,
            // 正数 = 从下往上
            biHeight: dib.height as i32,
            biPlanes: 1,
            biBitCount: 24,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let n = unsafe {
        StretchDIBits(dc, x0, y0, x1 - x0, y1 - y0, 0, 0, dib.width as i32, dib.height as i32, Some(dib.bits.as_ptr().cast()), &info, DIB_RGB_COLORS, SRCCOPY)
    };
    if n == 0 || n == GDI_ERROR { Err(last_error()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print::layout::{Size, place};

    const PDF: &str = "Microsoft Print to PDF";

    /// 系统的 PDF 渲染把第一页画成 PNG（宽 width 像素）。放在子进程里做：Windows.Data.Pdf 与同一进程里建过的 WARP 设备
    /// （d3d11 的测试）一起时，测试进程退出时会崩（0xC0000409）。程序本身不用 Windows.Data.Pdf
    fn render_pdf(pdf: &std::path::Path, png: &std::path::Path, width: u32) -> RgbaImage {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "platform::print::tests::render_pdf_in_child", "--ignored", "--nocapture", "--test-threads=1"])
            .env("LITEVIEW_RENDER_PDF", pdf)
            .env("LITEVIEW_RENDER_PNG", png)
            .env("LITEVIEW_RENDER_WIDTH", width.to_string())
            .output()
            .unwrap();
        assert!(out.status.success(), "PDF render child: {}\n{}{}", out.status, String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        image::open(png).unwrap().to_rgba8()
    }

    /// render_pdf 的子进程（平时不跑；没有那几个环境变量时什么也不做）
    #[test]
    #[ignore = "render_pdf 在子进程里调用"]
    fn render_pdf_in_child() {
        let (Some(pdf), Some(png), Some(width)) =
            (std::env::var_os("LITEVIEW_RENDER_PDF"), std::env::var_os("LITEVIEW_RENDER_PNG"), std::env::var("LITEVIEW_RENDER_WIDTH").ok().and_then(|w| w.parse::<u32>().ok()))
        else {
            return;
        };
        let (pdf, png) = (std::path::Path::new(&pdf), std::path::Path::new(&png));
        use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
        use windows::Storage::{CreationCollisionOption, FileAccessMode, StorageFile, StorageFolder};
        let file = StorageFile::GetFileFromPathAsync(&windows::core::HSTRING::from(pdf.as_os_str())).unwrap().join().unwrap();
        let doc = PdfDocument::LoadFromFileAsync(&file).unwrap().join().unwrap();
        assert_eq!(doc.PageCount().unwrap(), 1);
        let page = doc.GetPage(0).unwrap();
        let folder = StorageFolder::GetFolderFromPathAsync(&windows::core::HSTRING::from(png.parent().unwrap().as_os_str())).unwrap().join().unwrap();
        let name = windows::core::HSTRING::from(png.file_name().unwrap().to_string_lossy().as_ref());
        let out = folder.CreateFileAsync(&name, CreationCollisionOption::ReplaceExisting).unwrap().join().unwrap();
        let stream = out.OpenAsync(FileAccessMode::ReadWrite).unwrap().join().unwrap();
        let options = PdfPageRenderOptions::new().unwrap();
        options.SetDestinationWidth(width).unwrap();
        page.RenderWithOptionsToStreamAsync(&stream, &options).unwrap().join().unwrap();
        stream.FlushAsync().unwrap().join().unwrap();
    }

    /// 打到 Microsoft Print to PDF，再用系统 PDF 渲染核对纸张、方向、位置、颜色。没有这个打印机（或打印服务）的机器跳过
    #[test]
    fn prints_to_pdf_at_the_laid_out_position() {
        let Ok(list) = printers() else {
            eprintln!("skipped: print spooler not available");
            return;
        };
        if !list.iter().any(|p| p.name == PDF) {
            eprintln!("skipped: {PDF} not installed");
            return;
        }
        let caps = caps(PDF).unwrap();
        let a4 = caps.papers.iter().find(|p| (p.mm[0] - 210.0).abs() < 1.0 && (p.mm[1] - 297.0).abs() < 1.0).expect("A4");
        let mut dm = caps.defaults.clone();
        dm.set(a4.id, true, 1);
        let (dev, dm) = device(PDF, &dm).unwrap();
        let paper = dev.paper_mm();
        assert!((paper.x - 297.0).abs() < 1.0 && (paper.y - 210.0).abs() < 1.0, "landscape A4: {paper:?}");
        // 上三分之一绿；下面左红右蓝。3:2，长边 150 mm
        let img = RgbaImage::from_fn(300, 200, |x, y| match (y < 67, x < 150) {
            (true, _) => image::Rgba([0, 160, 0, 255]),
            (false, true) => image::Rgba([220, 0, 0, 255]),
            (false, false) => image::Rgba([0, 0, 220, 255]),
        });
        let placement = place(&dev, 0.0, [300, 200], Size::LongSide(150.0)).unwrap();
        let dir = std::env::temp_dir().join(format!("liteview-print-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("out.pdf");
        let _ = std::fs::remove_file(&pdf);
        let job = Job { printer: PDF.into(), devmode: dm, title: "LiteView test".into(), output: Some(pdf.clone()), copies: 1, max_copies: caps.max_copies, image: Arc::new(img), transform: Transform::default(), placement };
        assert_eq!(run(job, &AtomicBool::new(false)).unwrap(), Outcome::Sent);
        // 打印服务在后台写文件：等它写完（大小不再变）
        let mut last = 0;
        for _ in 0..150 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let len = std::fs::metadata(&pdf).map_or(0, |m| m.len());
            if len > 0 && len == last {
                break;
            }
            last = len;
        }
        assert!(last > 0, "no PDF written");
        // 要 4 像素 / 毫米；系统按显示缩放放大输出（150% 的屏上是 1.4 倍），每毫米多少像素按实际宽度算
        let png = render_pdf(&pdf, &dir.join("page.png"), 1188);
        assert!(png.width() >= 1188, "rendered {} px wide", png.width());
        let ppm = png.width() as f32 / paper.x;
        assert!((png.height() as f32 / ppm - paper.y).abs() <= 0.5, "landscape page: {}x{}", png.width(), png.height());
        let bbox = |want: [u8; 3]| {
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
            for (x, y, p) in png.enumerate_pixels() {
                if (0..3).all(|c| (i32::from(p[c]) - i32::from(want[c])).abs() < 60) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
            assert!(x1 > x0, "colour {want:?} not found");
            [x0, y0, x1, y1].map(|v| v as f32 / ppm)
        };
        let close = |got: [f32; 4], want: [f32; 4]| got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 1.0);
        // 150 × 100 mm，在 297 × 210 的纸上居中：x 73.5 ~ 223.5，y 55 ~ 155；绿色到 1/3 处
        let r = placement.rect;
        assert!((r.min.x - 73.5).abs() < 0.2 && (r.min.y - 55.0).abs() < 0.2, "{r:?}");
        let split_y = r.min.y + r.height() * 67.0 / 200.0;
        let (green, red, blue) = (bbox([0, 160, 0]), bbox([220, 0, 0]), bbox([0, 0, 220]));
        assert!(close(green, [r.min.x, r.min.y, r.max.x, split_y]), "green {green:?}");
        assert!(close(red, [r.min.x, split_y, r.center().x, r.max.y]), "red {red:?}");
        assert!(close(blue, [r.center().x, split_y, r.max.x, r.max.y]), "blue {blue:?}");
        // 要看输出：LITEVIEW_KEEP_PRINT_TEST=1
        if std::env::var_os("LITEVIEW_KEEP_PRINT_TEST").is_none() {
            let _ = std::fs::remove_dir_all(&dir);
        } else {
            eprintln!("kept {}", dir.display());
        }
    }
}
