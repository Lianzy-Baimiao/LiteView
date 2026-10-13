//! 各系统专有的部分。公共代码只调用这里的函数，不直接用系统接口。

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
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| s.split_whitespace().nth(1)?.parse::<f64>().ok())
            .map_or(0.0, |pages| pages * 4096.0 / 1048576.0)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        0.0
    }
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
