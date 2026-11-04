//! 各系统专有的部分。公共代码只调用这里的函数，不直接用系统接口。

pub mod assoc;
#[cfg(windows)]
pub mod d3d11;
pub mod package;
#[cfg(windows)]
pub mod print;
#[cfg(windows)]
pub mod windows;

use std::path::Path;

/// 进程启动到现在的毫秒数（用于启动计时）
pub fn ms_since_process_start() -> f64 {
    #[cfg(windows)]
    {
        windows::ms_since_process_start()
    }
    #[cfg(not(windows))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

/// 进程占用的物理内存，MB（Windows 上是专用工作集，即任务管理器显示的值；其它系统是常驻内存）
pub fn memory_mb() -> f64 {
    #[cfg(windows)]
    {
        windows::memory_mb().0
    }
    // 以前读 /proc/self/statm 再按 4 KB 一页算：16 KB / 64 KB 页的 ARM 内核上差好几倍
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status").ok().and_then(|s| linux_private_mb(&s)).unwrap_or(0.0)
    }
    #[cfg(target_os = "macos")]
    {
        macos_resident_mb().unwrap_or(0.0)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        0.0
    }
}

/// /proc/self/status 里进程自己的内存（RssAnon，与 Windows 的专用工作集最接近；老内核没有时用 VmRSS），单位 kB
#[cfg(any(target_os = "linux", test))]
fn linux_private_mb(status: &str) -> Option<f64> {
    let field = |key: &str| status.lines().find_map(|l| l.strip_prefix(key)?.trim().strip_suffix("kB")?.trim().parse::<f64>().ok());
    field("RssAnon:").or_else(|| field("VmRSS:")).map(|kb| kb / 1024.0)
}

/// 常驻内存（mach_task_basic_info.resident_size，与活动监视器的「实际内存」一样含共享页）
#[cfg(target_os = "macos")]
fn macos_resident_mb() -> Option<f64> {
    // libSystem 里的；与 libc crate 的声明一致
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }
    const MACH_TASK_BASIC_INFO: u32 = 20;
    // mach_task_basic_info：virtual_size、resident_size、resident_size_max（u64），两个 time_value_t、policy、suspend_count；
    // 按 4 字节对齐，一共 12 个 natural_t
    let mut info = [0u32; 12];
    let mut count = info.len() as u32;
    let r = unsafe { task_info(mach_task_self_, MACH_TASK_BASIC_INFO, info.as_mut_ptr().cast(), &mut count) };
    if r != 0 || count < 4 {
        return None;
    }
    let resident = u64::from(info[2]) | (u64::from(info[3]) << 32);
    Some(resident as f64 / 1048576.0)
}

/// 在文件管理器里显示并选中文件
pub fn show_in_folder(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut arg = std::ffi::OsString::from("/select,\"");
        arg.push(path.as_os_str());
        arg.push("\"");
        let _ = std::process::Command::new("explorer.exe").raw_arg(arg).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(dir) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
}

/// 「打开方式」对话框（只有 Windows 有）
pub fn open_with(path: &Path) {
    #[cfg(windows)]
    {
        windows::open_with_dialog(path);
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

pub fn has_open_with() -> bool {
    cfg!(windows)
}

/// 用系统默认程序打开网址
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// 移到回收站
pub fn trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// 系统强调色（只有 Windows 有；其它系统 None = 用默认的蓝色）
pub fn system_accent() -> Option<[u8; 3]> {
    #[cfg(windows)]
    {
        windows::system_accent()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn has_wallpaper() -> bool {
    cfg!(windows)
}

/// 设为桌面背景（只有 Windows）
pub fn set_wallpaper(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows::set_wallpaper(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("unsupported".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn linux_memory_comes_from_status_in_kilobytes() {
        let status = "Name:\tliteview\nVmRSS:\t  204800 kB\nRssAnon:\t   51200 kB\nRssFile:\t  153600 kB\n";
        assert_eq!(super::linux_private_mb(status), Some(50.0));
        assert_eq!(super::linux_private_mb("VmRSS:\t 1024 kB\n"), Some(1.0), "old kernels without RssAnon");
        assert_eq!(super::linux_private_mb("Name:\tx\n"), None);
    }
}
