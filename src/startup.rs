//! 启动保护：记下这次用哪个显卡接口启动；第一帧画出来之前崩溃或卡住，下次启动换下一个接口。
//! 1.0.0 选中 Vulkan 后，设备 / 窗口表面 / 交换链出错、驱动崩溃或卡住都没有退路，用户只看到「点了没反应」。
//!
//! 状态存在配置目录的 `startup.json`；原生崩溃的位置（模块 + 偏移、异常码、线程）写到 `last-crash.txt`，
//! 下次启动读出来显示在「设置 → 关于」，用户能截图反馈，不需要运行命令。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// 第一帧之前超过这么久还没确认，就算卡住（正常启动约 0.3–0.5 秒）
const HANG_SECS: u64 = 8;

/// 在这些模块里崩溃说明是图片解码（系统 / 商店解码扩展），不怪显卡接口
const CODEC_MODULES: &[&str] = &["heif", "hevc", "av1", "webp", "mfplat", "msmpeg2vdec", "windowscodecs", "wmphoto"];

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
pub struct State {
    /// 换了版本就重新从首选接口试
    pub version: String,
    /// 这台机器上启动失败过的接口（之后排到最后）
    pub failed: Vec<String>,
    /// 正在启动、还没画出第一帧的那次
    pub attempt: Option<Attempt>,
    /// 上一次启动失败或崩溃的说明
    pub last_failure: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Attempt {
    pub backend: String,
    pub pid: u32,
    /// UNIX 秒
    pub started: u64,
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Clean,
    /// 另一个实例刚开始启动（同时双击多张图）
    Concurrent,
    /// 上次没画出第一帧就崩溃、被结束或卡住
    Failed,
}

/// 上次留下的启动记录怎么算
pub fn classify(attempt: Option<&Attempt>, me: u32, now: u64, alive: impl Fn(u32) -> bool) -> Verdict {
    let Some(a) = attempt else { return Verdict::Clean };
    if a.pid == me {
        return Verdict::Clean;
    }
    if alive(a.pid) && now.saturating_sub(a.started) < HANG_SECS {
        Verdict::Concurrent
    } else {
        Verdict::Failed
    }
}

/// 原生崩溃记录（一行：`pid=…;main=0|1;text=…`）
#[derive(Debug, PartialEq)]
pub struct Crash {
    pub pid: u32,
    /// 崩在主线程（显卡初始化、画图都在主线程；解码在 decode-N 线程）
    pub main_thread: bool,
    pub text: String,
}

impl Crash {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().strip_prefix("pid=")?;
        let (pid, rest) = s.split_once(";main=")?;
        let (main, text) = rest.split_once(";text=")?;
        Some(Self { pid: pid.parse().ok()?, main_thread: main == "1", text: text.to_owned() })
    }

    pub fn line(&self) -> String {
        format!("pid={};main={};text={}", self.pid, u8::from(self.main_thread), self.text.replace(['\r', '\n'], " "))
    }

    /// 是不是显卡接口的问题：卡住 / 快速失败（没有记录）怪接口；解码扩展里或工作线程上崩溃不怪
    pub fn blames_backend(crash: Option<&Self>) -> bool {
        crash.is_none_or(|c| {
            let lower = c.text.to_ascii_lowercase();
            c.main_thread && !CODEC_MODULES.iter().any(|m| lower.contains(m))
        })
    }
}

/// 失败过的接口排到最后（全都失败过时仍按原顺序再试一遍）
pub fn order(preferred: &[&'static str], failed: &[String]) -> Vec<&'static str> {
    let (bad, good): (Vec<_>, Vec<_>) = preferred.iter().partition(|b| failed.iter().any(|f| f == *b));
    good.into_iter().chain(bad).collect()
}

/// 本进程用 backend 画出了第一帧：它不再算失败；启动记录是自己的就清掉。
/// 启动慢（超过 HANG_SECS）时又双击，另一个实例会把这个接口判成失败、换上它自己的记录；
/// 这里照样把接口洗白，否则以后每次都排到后面（Windows 上就是一直用 WARP 软件画）。返回是否改了
pub fn on_first_frame(s: &mut State, me: u32, backend: &str) -> bool {
    let mine = s.attempt.take_if(|a| a.pid == me).is_some();
    let before = s.failed.len();
    s.failed.retain(|f| f != backend);
    mine || s.failed.len() != before
}

static CONFIRMED: AtomicBool = AtomicBool::new(false);
/// 本进程正在用（最后一次 begin）的接口
static BACKEND: Mutex<String> = Mutex::new(String::new());
static INFO: Mutex<Info> = Mutex::new(Info { backend: String::new(), failed: Vec::new(), last_failure: None });

/// 「设置 → 关于」显示
#[derive(Clone, Default, Debug)]
pub struct Info {
    /// 正在用的接口
    pub backend: String,
    /// 这台机器上启动失败过、已排到后面的接口
    pub failed: Vec<String>,
    /// 上次启动或运行中的问题
    pub last_failure: Option<String>,
}

pub fn info() -> Info {
    INFO.lock().map(|i| i.clone()).unwrap_or_default()
}

fn update_info(f: impl FnOnce(&mut Info)) {
    if let Ok(mut i) = INFO.lock() {
        f(&mut i);
    }
}

fn state_path() -> PathBuf {
    crate::settings::config_dir().join("startup.json")
}

pub(crate) fn crash_path() -> PathBuf {
    crate::settings::config_dir().join("last-crash.txt")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn read() -> State {
    std::fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write(s: &State) {
    let p = state_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let Ok(text) = serde_json::to_string_pretty(s) else { return };
    if let Err(e) = crate::fsutil::atomic_write(&p, |w| w.write_all(text.as_bytes())) {
        log::warn!("保存启动记录失败：{e}");
    }
}

/// 一次启动过程。第一帧确认之后不再写（`confirm` 已经清掉了文件里的记录）
pub struct Guard {
    state: State,
    crash_seen: bool,
}

impl Guard {
    /// 读上次的记录：没画出第一帧的那次算失败，崩溃位置记为「上次问题」。
    /// 另一个实例正在启动时先等它画出第一帧（之后能转给它）或判定失败，最多 HANG_SECS 秒；
    /// 否则在它卡住时再双击，也会用同一个接口再卡一次。
    pub fn load() -> Self {
        let mut state = read();
        while classify(state.attempt.as_ref(), std::process::id(), now(), pid_alive) == Verdict::Concurrent {
            std::thread::sleep(std::time::Duration::from_millis(100));
            state = read();
        }
        let version = env!("CARGO_PKG_VERSION");
        if state.version != version {
            state = State { version: version.to_owned(), ..State::default() };
        }
        let crash = std::fs::read_to_string(crash_path()).ok().and_then(|t| Crash::parse(&t));
        let crash_seen = crash.is_some();
        if classify(state.attempt.as_ref(), std::process::id(), now(), pid_alive) == Verdict::Failed
            && let Some(a) = state.attempt.take()
        {
            let crash = crash.filter(|c| c.pid == a.pid);
            let what = crash.as_ref().map_or_else(|| crate::i18n::tr().gpu_no_first_frame.to_owned(), |c| c.text.clone());
            if Crash::blames_backend(crash.as_ref()) && crate::gpu::preferred().contains(&a.backend.as_str()) && !state.failed.contains(&a.backend) {
                state.failed.push(a.backend.clone());
            }
            log::warn!("上次用 {} 启动没有画出第一帧：{what}", a.backend);
            state.last_failure = Some(format!("{}: {what}", crate::gpu::display_name(&a.backend)));
        } else if let Some(c) = crash {
            // 第一帧之后（看图过程中）的崩溃：只记下来
            state.last_failure = Some(c.text);
        }
        update_info(|i| {
            i.failed = state.failed.iter().map(|b| crate::gpu::display_name(b).to_owned()).collect();
            i.last_failure = state.last_failure.clone();
        });
        Self { state, crash_seen }
    }

    pub fn failed(&self) -> &[String] {
        &self.state.failed
    }

    /// 马上要用这个接口创建窗口
    pub fn begin(&mut self, backend: &str) {
        self.state.attempt = Some(Attempt { backend: backend.to_owned(), pid: std::process::id(), started: now() });
        write(&self.state);
        if let Ok(mut b) = BACKEND.lock() {
            backend.clone_into(&mut b);
        }
        if std::mem::take(&mut self.crash_seen) {
            let _ = std::fs::remove_file(crash_path());
        }
        update_info(|i| i.backend = crate::gpu::display_name(backend).to_owned());
    }

    /// 这个接口在本进程里没能初始化（窗口还没显示）：记下来，换下一个
    pub fn fail(&mut self, backend: &str, error: &str) {
        // 强制指定的接口（Windows 的 LITEVIEW_RENDERER、其它系统的 WGPU_BACKEND）不记（不参与自动换接口）
        if crate::gpu::preferred().contains(&backend) && !self.state.failed.iter().any(|f| f == backend) {
            self.state.failed.push(backend.to_owned());
        }
        self.state.attempt = None;
        self.state.last_failure = Some(format!("{}: {error}", crate::gpu::display_name(backend)));
        write(&self.state);
        update_info(|i| {
            i.failed = self.state.failed.iter().map(|b| crate::gpu::display_name(b).to_owned()).collect();
            i.last_failure = self.state.last_failure.clone();
        });
    }

    /// 所有接口都失败：清掉失败记录，下次重新从首选接口试（可能只是远程桌面等临时环境）
    pub fn gave_up(&mut self, error: &str) {
        self.state.failed.clear();
        self.state.attempt = None;
        self.state.last_failure = Some(error.to_owned());
        write(&self.state);
    }
}

/// 第一帧已经画出来：清掉启动记录
pub fn confirm() {
    if CONFIRMED.swap(true, Ordering::SeqCst) {
        return;
    }
    let backend = BACKEND.lock().map(|b| b.clone()).unwrap_or_default();
    if backend.is_empty() {
        return;
    }
    let mut s = read();
    if on_first_frame(&mut s, std::process::id(), &backend) {
        write(&s);
    }
    let shown = crate::gpu::display_name(&backend).to_owned();
    update_info(|i| i.failed.retain(|f| *f != shown));
}

pub fn confirmed() -> bool {
    CONFIRMED.load(Ordering::SeqCst)
}

/// 正在用软件渲染（Windows 的 WARP）：画一帧全靠 CPU
pub fn software() -> bool {
    BACKEND.lock().is_ok_and(|b| *b == "warp")
}

/// 记一条退出原因，下次启动显示（panic 或第一帧之后的错误；`main_thread` = 怪不怪显卡接口）
pub fn record_exit(text: &str, blame_backend: bool) {
    let c = Crash { pid: std::process::id(), main_thread: blame_backend, text: text.to_owned() };
    let p = crash_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, c.line());
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let alive = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let _ = CloseHandle(h);
        alive
    }
}

#[cfg(target_os = "linux")]
fn pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// 其它系统只按时间判断
#[cfg(not(any(windows, target_os = "linux")))]
fn pid_alive(_: u32) -> bool {
    true
}

static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// panic 写进日志文件（release 是 GUI 程序，stderr 谁也看不到），再交给默认处理
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let at = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
        let line = format!("panic in {}: {}{at}", thread.name().unwrap_or("?"), info.payload_as_str().unwrap_or("?"));
        log::error!("{line}");
        append_error_log(&line);
        if let Ok(mut p) = LAST_PANIC.lock() {
            *p = Some(line);
        }
        previous(info);
    }));
}

/// 最近一次 panic 的说明（catch_unwind 拿到的 payload 没有位置）
pub fn last_panic(payload: &(dyn std::any::Any + Send)) -> String {
    LAST_PANIC.lock().ok().and_then(|mut p| p.take()).unwrap_or_else(|| {
        payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
    })
}

/// 本地错误日志（不上传）：配置目录 `errors.log`，超过 256 KB 换成 `errors.old.log`
pub fn append_error_log(line: &str) {
    use std::io::Write;
    let p = crate::settings::config_dir().join("errors.log");
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::rename(&p, p.with_file_name("errors.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "[{} v{}] {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"));
    }
}

/// 没法继续时给用户看得见的提示（窗口可能根本没出现）
pub fn fatal_dialog(error: &str) {
    let t = crate::i18n::tr();
    let dir = crate::settings::config_dir();
    let body = crate::i18n::fill(t.startup_failed, &[&error, &dir.display()]);
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW};
        let w = |s: &str| crate::platform::windows::wide(std::ffi::OsStr::new(s));
        let (b, c) = (w(&body), w(t.app_name));
        MessageBoxW(None, windows::core::PCWSTR(b.as_ptr()), windows::core::PCWSTR(c.as_ptr()), MB_OK | MB_ICONERROR | MB_SETFOREGROUND);
    }
    #[cfg(not(windows))]
    eprintln!("{body}");
}

/// 原生崩溃（访问冲突等，含显卡驱动、解码扩展里的）：把模块 + 偏移、异常码写进 `last-crash.txt`，
/// 然后照常交给 Windows 错误报告。崩溃时堆可能已经坏了，处理函数里只用栈。
#[cfg(windows)]
pub mod crash {
    use std::fmt::Write as _;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    use windows::Win32::Foundation::{CloseHandle, HMODULE};
    use windows::Win32::Storage::FileSystem::{CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_SHARE_READ, WriteFile};
    use windows::Win32::System::Diagnostics::Debug::{EXCEPTION_POINTERS, LPTOP_LEVEL_EXCEPTION_FILTER, SetUnhandledExceptionFilter};
    use windows::Win32::System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::core::PCWSTR;

    static PATH: OnceLock<Vec<u16>> = OnceLock::new();
    static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
    static PREVIOUS: AtomicUsize = AtomicUsize::new(0);

    /// 在主线程调用一次
    pub fn install() {
        let _ = PATH.set(crate::platform::windows::wide(super::crash_path().as_os_str()));
        MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
        let previous = unsafe { SetUnhandledExceptionFilter(Some(filter)) };
        PREVIOUS.store(previous.map_or(0, |f| f as usize), Ordering::SeqCst);
    }

    /// 定长缓冲区（不分配内存）
    struct Buf {
        data: [u8; 768],
        len: usize,
    }

    impl std::fmt::Write for Buf {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            let room = self.data.len() - self.len;
            let mut n = s.len().min(room);
            while !s.is_char_boundary(n) {
                n -= 1;
            }
            self.data[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
            self.len += n;
            Ok(())
        }
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        unsafe {
            if let Some(record) = info.as_ref().and_then(|i| i.ExceptionRecord.as_ref())
                && let Some(path) = PATH.get()
            {
                let mut b = Buf { data: [0; 768], len: 0 };
                let main = GetCurrentThreadId() == MAIN_THREAD.load(Ordering::SeqCst);
                let _ = write!(b, "pid={};main={};text=0x{:08X} ", std::process::id(), u8::from(main), record.ExceptionCode.0 as u32);
                let addr = record.ExceptionAddress as usize;
                let mut module = HMODULE::default();
                let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
                if GetModuleHandleExW(flags, PCWSTR(addr as *const u16), &mut module).is_ok() {
                    let mut name = [0u16; 260];
                    let n = GetModuleFileNameW(Some(module), &mut name) as usize;
                    let name = &name[..n.min(name.len())];
                    let start = name.iter().rposition(|&c| c == 0x5C).map_or(0, |i| i + 1); // '\\'
                    for c in char::decode_utf16(name[start..].iter().copied()) {
                        let _ = b.write_char(c.unwrap_or('?'));
                    }
                    let _ = write!(b, "+0x{:X}", addr - module.0 as usize);
                } else {
                    let _ = write!(b, "@0x{addr:X}");
                }
                let _ = b.write_str(if main { " (main thread" } else { " (worker thread" });
                if let Ok(i) = super::INFO.try_lock()
                    && !i.backend.is_empty()
                {
                    let _ = write!(b, ", {}", i.backend);
                }
                let _ = b.write_str(")");
                if let Ok(h) = CreateFileW(PCWSTR(path.as_ptr()), FILE_GENERIC_WRITE.0, FILE_SHARE_READ, None, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, None) {
                    let _ = WriteFile(h, Some(&b.data[..b.len]), None, None);
                    let _ = CloseHandle(h);
                }
            }
            match PREVIOUS.load(Ordering::SeqCst) {
                0 => 0, // EXCEPTION_CONTINUE_SEARCH：照常显示「已停止工作」并交给 Windows 错误报告
                p => {
                    let previous: LPTOP_LEVEL_EXCEPTION_FILTER = Some(std::mem::transmute::<usize, unsafe extern "system" fn(*const EXCEPTION_POINTERS) -> i32>(p));
                    previous.map_or(0, |f| f(info))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(pid: u32, started: u64) -> Attempt {
        Attempt { backend: "vulkan".into(), pid, started }
    }

    #[test]
    fn stale_attempts_are_failures() {
        let dead = |_| false;
        let alive = |_| true;
        assert_eq!(classify(None, 1, 100, dead), Verdict::Clean);
        assert_eq!(classify(Some(&attempt(1, 0)), 1, 100, dead), Verdict::Clean);
        // 崩溃 / 被结束：进程已经不在
        assert_eq!(classify(Some(&attempt(7, 99)), 1, 100, dead), Verdict::Failed);
        // 同时启动的另一个实例
        assert_eq!(classify(Some(&attempt(7, 95)), 1, 100, alive), Verdict::Concurrent);
        // 还活着但早就该画出第一帧：卡住（驱动在 acquire / present 里不返回）
        assert_eq!(classify(Some(&attempt(7, 80)), 1, 100, alive), Verdict::Failed);
        // 系统时间往回调过
        assert_eq!(classify(Some(&attempt(7, 200)), 1, 100, alive), Verdict::Concurrent);
    }

    #[test]
    fn crash_record_roundtrip_and_blame() {
        let c = Crash { pid: 42, main_thread: true, text: "0xC0000005 amdvlk64.dll+0x1A2B (main thread, Vulkan)\r\n".into() };
        let back = Crash::parse(&c.line()).unwrap();
        assert_eq!((back.pid, back.main_thread), (42, true));
        assert!(!back.text.contains('\n'));
        assert!(Crash::blames_backend(Some(&back)));
        // 卡住 / 快速失败没有记录：怪接口
        assert!(Crash::blames_backend(None));
        // 解码扩展里崩溃，或者在工作线程上：不是显卡接口的问题
        let heif = Crash { pid: 1, main_thread: true, text: "0xC0000005 msheif_store.dll+0x10".into() };
        let worker = Crash { pid: 1, main_thread: false, text: "0xC0000005 ntdll.dll+0x10".into() };
        assert!(!Crash::blames_backend(Some(&heif)) && !Crash::blames_backend(Some(&worker)));
        assert_eq!(Crash::parse("garbage"), None);
    }

    #[test]
    fn first_frame_clears_the_backend_even_if_another_instance_took_over() {
        let state = |failed: &[&str], attempt: Option<Attempt>| State { version: "1".into(), failed: failed.iter().map(|f| (*f).to_owned()).collect(), attempt, last_failure: None };
        let d3d = |pid| Some(Attempt { backend: "d3d11".into(), pid, started: 5 });
        let warp = |pid| Some(Attempt { backend: "warp".into(), pid, started: 9 });
        // 平常：自己的记录清掉
        let mut s = state(&[], d3d(1));
        assert!(on_first_frame(&mut s, 1, "d3d11"));
        assert_eq!(s, state(&[], None));
        // 失败过、排在后面重试的接口这次成功了
        let mut s = state(&["d3d11"], d3d(1));
        assert!(on_first_frame(&mut s, 1, "d3d11"));
        assert_eq!(s, state(&[], None));
        // 启动慢：另一个实例把 d3d11 判成失败、正在用 WARP 启动。洗白 d3d11，不动它的记录
        let mut s = state(&["d3d11"], warp(2));
        assert!(on_first_frame(&mut s, 1, "d3d11"));
        assert_eq!(s, state(&[], warp(2)));
        // 那个实例用 WARP 画出第一帧：d3d11 真失败过时仍排在后面
        let mut s = state(&["d3d11"], warp(2));
        assert!(on_first_frame(&mut s, 2, "warp"));
        assert_eq!(s, state(&["d3d11"], None));
        // 没什么要改
        let mut s = state(&[], warp(2));
        assert!(!on_first_frame(&mut s, 1, "d3d11"));
    }

    #[test]
    fn failed_backends_go_last() {
        let pref = ["vulkan", "dx12", "gl"];
        assert_eq!(order(&pref, &[]), pref);
        assert_eq!(order(&pref, &["vulkan".into()]), ["dx12", "gl", "vulkan"]);
        assert_eq!(order(&pref, &["vulkan".into(), "dx12".into()]), ["gl", "vulkan", "dx12"]);
        assert_eq!(order(&pref, &["metal".into()]), pref);
    }
}
