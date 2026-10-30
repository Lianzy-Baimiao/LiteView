//! Windows：资源管理器同款排序、安全替换文件、启动计时与内存、标题栏颜色。

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{COLORREF, FILETIME, HWND};
use windows::Win32::Graphics::Dwm::{DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_ACL_ERRORS, REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::core::PCWSTR;

pub fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// 按文件名排序，与资源管理器一致（StrCmpLogicalW）
pub fn sort_logical(paths: &mut [PathBuf]) {
    let mut keyed: Vec<(Vec<u16>, PathBuf)> = paths
        .iter()
        .map(|p| (wide(p.file_name().unwrap_or(p.as_os_str())), p.clone()))
        .collect();
    keyed.sort_by(|(a, _), (b, _)| {
        let r = unsafe { StrCmpLogicalW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr())) };
        r.cmp(&0).then_with(|| a.cmp(b))
    });
    for (dst, (_, p)) in paths.iter_mut().zip(keyed) {
        *dst = p;
    }
}

/// 用 `replacement` 替换 `replaced`，保留原文件的属性、权限、创建时间
pub fn replace_file(replaced: &Path, replacement: &Path) -> windows::core::Result<()> {
    let (a, b) = (wide(replaced.as_os_str()), wide(replacement.as_os_str()));
    unsafe {
        ReplaceFileW(
            PCWSTR(a.as_ptr()),
            PCWSTR(b.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS,
            None,
            None,
        )
    }
}

fn filetime_100ns(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

pub fn ms_since_process_start() -> f64 {
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u).is_err() {
            return 0.0;
        }
        let now = GetSystemTimePreciseAsFileTime();
        filetime_100ns(now).saturating_sub(filetime_100ns(c)) as f64 / 10_000.0
    }
}

/// (专用工作集, 工作集)，MB。任务管理器「内存」一栏显示的是专用工作集
pub fn memory_mb() -> (f64, f64) {
    let mut pmc = PROCESS_MEMORY_COUNTERS_EX2::default();
    let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut pmc).cast::<PROCESS_MEMORY_COUNTERS>(), cb) }.as_bool();
    if ok { (pmc.PrivateWorkingSetSize as f64 / 1048576.0, pmc.WorkingSetSize as f64 / 1048576.0) } else { (0.0, 0.0) }
}

/// 「打开方式」对话框
pub fn open_with_dialog(path: &Path) {
    use std::os::windows::process::CommandExt;
    let mut arg = std::ffi::OsString::from("shell32.dll,OpenAs_RunDLL ");
    arg.push(path.as_os_str());
    let _ = std::process::Command::new("rundll32.exe").raw_arg(arg).spawn();
}

/// 标题栏：深色 / 浅色；Win11 上把标题栏颜色设成与看图背景一致（Win10 不支持，忽略错误）
pub fn style_titlebar(hwnd: isize, dark: bool, caption: [u8; 3], text: [u8; 3]) {
    let hwnd = HWND(hwnd as *mut core::ffi::c_void);
    let colorref = |c: [u8; 3]| COLORREF(u32::from(c[0]) | u32::from(c[1]) << 8 | u32::from(c[2]) << 16);
    let dark_flag: i32 = dark.into();
    let (cap, txt) = (colorref(caption), colorref(text));
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&raw const dark_flag).cast(), 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, (&raw const cap).cast(), 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, (&raw const txt).cast(), 4);
    }
}

/// 标题栏 / 任务栏图标：按窗口所在显示器的缩放直接画出准确尺寸（16 / 32 × 缩放），不缩放现成的位图。
/// eframe 在窗口第一次成为活动窗口时会换上它自己从 64 px 缩小的（发虚）图标，所以每帧检查、被换掉就换回来。
pub fn ensure_window_icon(hwnd: isize, scale: f32, render: impl Fn(u32) -> Vec<u8>) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, DestroyIcon, HICON, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SendMessageW, WM_GETICON, WM_SETICON,
    };
    struct Icons {
        hwnd: isize,
        sizes: (u32, u32),
        small: HICON,
        big: HICON,
    }
    thread_local! {
        static ICONS: std::cell::RefCell<Option<Icons>> = const { std::cell::RefCell::new(None) };
    }
    let hwnd_ = HWND(hwnd as *mut core::ffi::c_void);
    let sizes = ((16.0 * scale).round().max(16.0) as u32, (32.0 * scale).round().max(32.0) as u32);
    let create = |px: u32| -> Option<HICON> {
        let mut png = Vec::new();
        image::ImageEncoder::write_image(image::codecs::png::PngEncoder::new(&mut png), &render(px), px, px, image::ExtendedColorType::Rgba8).ok()?;
        // 0x00030000：图标资源格式版本；尺寸与 PNG 一致，系统不再缩放
        unsafe { CreateIconFromResourceEx(&png, true, 0x0003_0000, px as i32, px as i32, LR_DEFAULTCOLOR) }.ok()
    };
    let set = |kind: u32, icon: HICON| unsafe { SendMessageW(hwnd_, WM_SETICON, Some(WPARAM(kind as usize)), Some(LPARAM(icon.0 as isize))) };
    ICONS.with(|cell| {
        let mut cur = cell.borrow_mut();
        if let Some(c) = cur.as_ref().filter(|c| c.hwnd == hwnd && c.sizes == sizes) {
            let small = unsafe { SendMessageW(hwnd_, WM_GETICON, Some(WPARAM(ICON_SMALL as usize)), None) };
            if small.0 != c.small.0 as isize {
                set(ICON_BIG, c.big);
                set(ICON_SMALL, c.small);
            }
            return;
        }
        let (Some(small), Some(big)) = (create(sizes.0), create(sizes.1)) else { return };
        // 先设大图标再设小图标（只设大的时系统会自己把它缩成小的）
        set(ICON_BIG, big);
        set(ICON_SMALL, small);
        if let Some(old) = cur.replace(Icons { hwnd, sizes, small, big }) {
            unsafe {
                let _ = DestroyIcon(old.small);
                let _ = DestroyIcon(old.big);
            }
        }
    });
}

/// 窗口所在显示器的刷新率（Hz）；取不到时 None
pub fn refresh_hz(hwnd: isize) -> Option<f32> {
    use windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow};
    unsafe {
        let monitor = MonitorFromWindow(HWND(hwnd as *mut core::ffi::c_void), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFOEXW { monitorInfo: MONITORINFO { cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32, ..Default::default() }, ..Default::default() };
        if !GetMonitorInfoW(monitor, (&raw mut info).cast()).as_bool() {
            return None;
        }
        let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        if !EnumDisplaySettingsW(PCWSTR(info.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode).as_bool() {
            return None;
        }
        // 0 / 1 = 硬件默认值，不是真的刷新率
        (mode.dmDisplayFrequency > 1).then_some(mode.dmDisplayFrequency as f32)
    }
}

/// 系统强调色（「设置 → 个性化 → 颜色」里选的），注册表 DWM\AccentColor（ABGR）
pub fn system_accent() -> Option<[u8; 3]> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let key = wide(std::ffi::OsStr::new("Software\\Microsoft\\Windows\\DWM"));
    let name = wide(std::ffi::OsStr::new("AccentColor"));
    let mut v: u32 = 0;
    let mut len = 4u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_DWORD, None, Some((&raw mut v).cast()), Some(&mut len)) };
    (r == ERROR_SUCCESS).then_some([v as u8, (v >> 8) as u8, (v >> 16) as u8])
}

/// 设为桌面背景
pub fn set_wallpaper(path: &Path) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW};
    let p = wide(path.as_os_str());
    unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(p.as_ptr() as *mut core::ffi::c_void), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) }.map_err(|e| e.to_string())
}

/// 拖到缩放不同的显示器上时，用系统给的新位置和大小（Win11 起；Win10 不变）。
///
/// winit 0.30.13 收到 WM_DPICHANGED 时不用系统给的矩形，自己按原来的逻辑大小算一个，再用 MonitorFromWindow
/// 检查它在不在新显示器上；拖动中窗口其实还在原来的显示器上，于是又被推回去，缩放来回变，窗口一次比一次大、
/// 闪烁、拖不动（winit #4600）。winit 主线已改成 Win11 上直接用系统的矩形（#4341），还没发到 0.30；
/// 升级到带这个改动的 winit 后删掉这里。
pub fn use_system_dpi_rect(hwnd: isize) {
    if windows_build().is_none_or(|b| b >= 22000) {
        dpi_rect::install(HWND(hwnd as *mut core::ffi::c_void));
    }
}

/// 窗口所在显示器的工作区（去掉任务栏），物理像素 (左, 上, 右, 下)
pub fn work_area(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let ok = unsafe { GetMonitorInfoW(MonitorFromWindow(HWND(hwnd as *mut core::ffi::c_void), MONITOR_DEFAULTTONEAREST), &mut info) }.as_bool();
    ok.then_some((info.rcWork.left, info.rcWork.top, info.rcWork.right, info.rcWork.bottom))
}

/// 窗口外框，物理像素 (左, 上, 右, 下)
pub fn window_rect(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut r = windows::Win32::Foundation::RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut core::ffi::c_void), &mut r) }.ok()?;
    Some((r.left, r.top, r.right, r.bottom))
}

/// 浮动工具栏的小窗口：不出现在任务栏和 Alt+Tab 里，点它不抢图片窗口的焦点；Win11 圆角
pub fn style_dock(hwnd: isize) {
    use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW};
    let h = HWND(hwnd as *mut core::ffi::c_void);
    let pref = DWMWCP_ROUND;
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
        let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, (&raw const pref).cast(), 4);
    }
}

/// 显示在 r（物理像素，左上右下），不激活
pub fn show_at(hwnd: isize, r: (i32, i32, i32, i32)) {
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos};
    let _ = unsafe { SetWindowPos(HWND(hwnd as *mut core::ffi::c_void), None, r.0, r.1, r.2 - r.0, r.3 - r.1, SWP_NOACTIVATE | SWP_SHOWWINDOW) };
}

/// 这个窗口在最前面（有键盘焦点的那个）
pub fn is_foreground(hwnd: isize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe { GetForegroundWindow() }.0 as isize == hwnd
}

/// 把最前面换成 hwnd（本进程的窗口在最前面时系统才允许）
pub fn bring_to_front(hwnd: isize) {
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    let _ = unsafe { SetForegroundWindow(HWND(hwnd as *mut core::ffi::c_void)) };
}

pub fn hide(hwnd: isize) {
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
    let _ = unsafe { ShowWindow(HWND(hwnd as *mut core::ffi::c_void), SW_HIDE) };
}

/// 鼠标在屏幕上的位置，物理像素
pub fn cursor_pos() -> Option<(i32, i32)> {
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = windows::Win32::Foundation::POINT::default();
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

/// 一次同时改位置和大小（分两次改时中间那一下会看得见）
pub fn set_window_rect(hwnd: isize, r: (i32, i32, i32, i32)) {
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};
    let _ = unsafe { SetWindowPos(HWND(hwnd as *mut core::ffi::c_void), None, r.0, r.1, r.2 - r.0, r.3 - r.1, SWP_NOZORDER | SWP_NOACTIVATE) };
}

/// 拖窗口边缘改大小时保持这个宽高比（0 = 不管）。无边框、窗口跟着图片时用
pub fn set_resize_aspect(aspect: f32) {
    aspect::RATIO.store(aspect.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// 在窗口过程前面接一层，处理 WM_SIZING（winit 不处理它）
pub fn keep_resize_aspect(hwnd: isize) {
    aspect::install(HWND(hwnd as *mut core::ffi::c_void));
}

/// 拖动中的矩形按宽高比改：拖左右边时按宽算高，拖上下边时按高算宽，拖角时取大的一边；不动被拖的对边
pub(crate) fn fit_aspect(r: (i32, i32, i32, i32), edge: u32, aspect: f32) -> (i32, i32, i32, i32) {
    // WMSZ_LEFT 1, RIGHT 2, TOP 3, TOPLEFT 4, TOPRIGHT 5, BOTTOM 6, BOTTOMLEFT 7, BOTTOMRIGHT 8
    let (mut l, mut t, mut rt, mut b) = r;
    let (w, h) = ((rt - l) as f32, (b - t) as f32);
    let by_width = match edge {
        1 | 2 => true,
        3 | 6 => false,
        _ => w / aspect >= h,
    };
    if by_width {
        let nh = (w / aspect).round() as i32;
        if matches!(edge, 3..=5) { t = b - nh } else { b = t + nh }
    } else {
        let nw = (h * aspect).round() as i32;
        if matches!(edge, 1 | 4 | 7) { l = rt - nw } else { rt = l + nw }
    }
    (l, t, rt, b)
}

mod aspect {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, SetWindowLongPtrW, WM_SIZING, WNDPROC};

    pub(super) static RATIO: AtomicU32 = AtomicU32::new(0);
    static NEXT: OnceLock<WNDPROC> = OnceLock::new();

    pub fn install(hwnd: HWND) {
        if NEXT.get().is_some() {
            return;
        }
        let old = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc as *const () as isize) };
        if old != 0 {
            let _ = NEXT.set(unsafe { std::mem::transmute::<isize, WNDPROC>(old) });
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let Some(&next) = NEXT.get() else { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) } };
        let aspect = f32::from_bits(RATIO.load(Ordering::Relaxed));
        if msg == WM_SIZING && lparam.0 != 0 && aspect > 0.0 {
            let r = unsafe { &mut *(lparam.0 as *mut RECT) };
            (r.left, r.top, r.right, r.bottom) = super::fit_aspect((r.left, r.top, r.right, r.bottom), wparam.0 as u32, aspect);
            return LRESULT(1);
        }
        unsafe { CallWindowProcW(next, hwnd, msg, wparam, lparam) }
    }
}

/// Windows 的版本号（26100 等）；读不到时 None
fn windows_build() -> Option<u32> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let key = wide(std::ffi::OsStr::new("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"));
    let name = wide(std::ffi::OsStr::new("CurrentBuildNumber"));
    let mut buf = [0u16; 16];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
    (r == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)])).and_then(|s| s.parse().ok())
}

/// 在 winit 的窗口过程前面接一层：WM_DPICHANGED 处理期间，winit 移动窗口的 SetWindowPos 改成系统给的矩形。
/// 只接一个窗口（主窗口）。
mod dpi_rect {
    use std::cell::Cell;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, SWP_NOMOVE, SWP_NOSIZE, SetWindowLongPtrW, WINDOWPOS, WM_DPICHANGED, WM_WINDOWPOSCHANGING, WNDPROC,
    };

    /// winit 原来的窗口过程
    static NEXT: OnceLock<WNDPROC> = OnceLock::new();

    thread_local! {
        /// 正在处理的 WM_DPICHANGED 里系统给的矩形
        static SUGGESTED: Cell<Option<RECT>> = const { Cell::new(None) };
    }

    pub fn install(hwnd: HWND) {
        if NEXT.get().is_some() {
            return;
        }
        let old = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, proc as *const () as isize) };
        if old != 0 {
            let _ = NEXT.set(unsafe { std::mem::transmute::<isize, WNDPROC>(old) });
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let Some(&next) = NEXT.get() else { return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) } };
        // 窗口过程里 panic 会直接结束进程；线程局部变量销毁后 get / set 会 panic，所以都用 try_with
        match msg {
            WM_DPICHANGED if lparam.0 != 0 => {
                let rect = unsafe { *(lparam.0 as *const RECT) };
                let outer = SUGGESTED.try_with(|s| s.replace(Some(rect))).ok().flatten();
                let r = unsafe { CallWindowProcW(next, hwnd, msg, wparam, lparam) };
                let _ = SUGGESTED.try_with(|s| s.set(outer));
                r
            }
            WM_WINDOWPOSCHANGING if lparam.0 != 0 => {
                if let Some(rect) = SUGGESTED.try_with(Cell::get).ok().flatten() {
                    place(unsafe { &mut *(lparam.0 as *mut WINDOWPOS) }, rect);
                }
                unsafe { CallWindowProcW(next, hwnd, msg, wparam, lparam) }
            }
            _ => unsafe { CallWindowProcW(next, hwnd, msg, wparam, lparam) },
        }
    }

    /// 同时改位置和大小的那次（winit 自己算的矩形）换成系统给的；只改样式、Z 序等的不动
    pub(super) fn place(pos: &mut WINDOWPOS, rect: RECT) {
        if pos.flags.0 & (SWP_NOMOVE.0 | SWP_NOSIZE.0) == 0 {
            (pos.x, pos.y, pos.cx, pos.cy) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
        }
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WINDOWPOS};

    #[test]
    fn only_a_move_and_resize_takes_the_suggested_rect() {
        let rect = RECT { left: 10, top: 20, right: 110, bottom: 220 };
        let placed = |flags| {
            let mut p = WINDOWPOS { x: 1, y: 2, cx: 3, cy: 4, flags, ..Default::default() };
            super::dpi_rect::place(&mut p, rect);
            (p.x, p.y, p.cx, p.cy)
        };
        assert_eq!(placed(SWP_NOZORDER | SWP_NOACTIVATE), (10, 20, 100, 200));
        // winit 改样式、只改大小或只移动的调用不动
        assert_eq!(placed(SWP_NOZORDER | SWP_NOMOVE | SWP_NOSIZE), (1, 2, 3, 4));
        assert_eq!(placed(SWP_NOMOVE), (1, 2, 3, 4));
        assert_eq!(placed(SWP_NOSIZE), (1, 2, 3, 4));
    }

    #[test]
    fn resizing_keeps_the_aspect_and_holds_the_opposite_side() {
        use super::fit_aspect;
        // 宽高比 2：拖右边、左边按宽算高（下边动）；拖上边按高算宽（右边动）
        assert_eq!(fit_aspect((0, 0, 400, 100), 2, 2.0), (0, 0, 400, 200));
        assert_eq!(fit_aspect((100, 0, 500, 100), 1, 2.0), (100, 0, 500, 200));
        assert_eq!(fit_aspect((0, 0, 400, 300), 3, 2.0), (0, 0, 600, 300));
        // 拖角取大的一边，对角不动
        assert_eq!(fit_aspect((0, 0, 400, 300), 4, 2.0), (-200, 0, 400, 300));
        assert_eq!(fit_aspect((0, 0, 800, 300), 8, 2.0), (0, 0, 800, 400));
        assert_eq!(fit_aspect((0, 0, 800, 300), 5, 2.0), (0, -100, 800, 300));
    }

    /// 真的 winit 窗口收 WM_DPICHANGED（换成另一个缩放）：没接时 winit 用自己算的矩形，接上后用系统给的
    #[test]
    fn dpi_change_takes_the_rect_windows_suggests() {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SendMessageW, WM_DPICHANGED};
        use winit::application::ApplicationHandler;
        use winit::event::WindowEvent;
        use winit::event_loop::{ActiveEventLoop, EventLoop};
        use winit::platform::windows::EventLoopBuilderExtWindows;
        use winit::window::{Window, WindowId};

        // 故意和「保持逻辑大小」算出来的不一样
        const S1: RECT = RECT { left: 130, top: 120, right: 907, bottom: 675 };
        const S2: RECT = RECT { left: 150, top: 140, right: 705, bottom: 584 };

        #[derive(Default)]
        struct Probe {
            rects: Option<(RECT, RECT)>,
        }
        impl ApplicationHandler for Probe {
            fn resumed(&mut self, el: &ActiveEventLoop) {
                if self.rects.is_some() {
                    return;
                }
                let attrs = Window::default_attributes()
                    .with_visible(false)
                    .with_position(winit::dpi::PhysicalPosition::new(100, 100))
                    .with_inner_size(winit::dpi::PhysicalSize::new(640, 480));
                let window = el.create_window(attrs).unwrap();
                let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else { panic!("no HWND") };
                let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
                let change = |dpi: u32, rect: RECT| {
                    unsafe { SendMessageW(hwnd, WM_DPICHANGED, Some(WPARAM(((dpi << 16) | dpi) as usize)), Some(LPARAM(&raw const rect as isize))) };
                    let mut now = RECT::default();
                    unsafe { GetWindowRect(hwnd, &mut now) }.unwrap();
                    now
                };
                // 96 = 100 %，144 = 150 %；先换成和现在不同的
                let (a, b) = if window.scale_factor() > 1.2 { (96, 144) } else { (144, 96) };
                let unhooked = change(a, S1);
                super::dpi_rect::install(hwnd);
                let hooked = change(b, S2);
                self.rects = Some((unhooked, hooked));
                // Drop 只投递销毁消息：收到 Destroyed 再退出
                drop(window);
            }

            fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
                if matches!(event, WindowEvent::Destroyed) {
                    el.exit();
                }
            }
        }

        let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
        let mut probe = Probe::default();
        event_loop.run_app(&mut probe).unwrap();
        let (unhooked, hooked) = probe.rects.expect("window created");
        assert_eq!(hooked, S2);
        // 这条失败 = winit 自己已经用系统给的矩形了，可以删掉 use_system_dpi_rect
        assert_ne!(unhooked, S1, "winit now applies the suggested rect itself");
    }
}
