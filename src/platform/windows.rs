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
