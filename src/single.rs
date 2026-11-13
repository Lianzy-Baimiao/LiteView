//! 单实例：已经开着 LiteView 时，双击新图片在原窗口打开。
//! Windows 用命名管道（按用户名区分），其它系统用 Unix 套接字（$XDG_RUNTIME_DIR 或临时目录）。
//! 消息就是一行：`open|edit|mosaic <路径>`。
//!
//! 同时打开好几张（资源管理器里多选回车、启动中又双击）时几个进程差不多同时来：
//! - 先到的进程占一个标记（Windows 的命名互斥体，其它系统给套接字旁边的锁文件加锁），进程在它就在；
//! - 后来的看到标记已经有人占了，就等先到的那个开好管道（最多 `WAIT`），再把图交给它，不自己开窗口；
//! - 管道一次只接一个连接，服务端接上一个就马上建下一个，客户端遇到「忙」就再试。
//!
//! 以前客户端只试一次：管道正忙（另一个进程刚连上）或先到的还没开好管道时，就自己再开一个窗口。
//!
//! 「设置 → 常规 → 多窗口模式」开着时（`single_instance` = false）不占标记、不开管道，每次都开新窗口。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 另一个实例在（或正在启动）时，最多等它这么久开好管道（正常启动约 0.3–0.5 秒；卡住的由启动保护处理）
pub const WAIT: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub path: PathBuf,
    pub edit: bool,
    pub mosaic: bool,
}

fn encode(r: &Request) -> String {
    let verb = if r.mosaic { "mosaic" } else if r.edit { "edit" } else { "open" };
    format!("{verb} {}", r.path.display())
}

fn decode(s: &str) -> Option<Request> {
    let (verb, path) = s.trim().split_once(' ')?;
    Some(Request { path: PathBuf::from(path), edit: verb == "edit", mosaic: verb == "mosaic" })
}

fn user() -> String {
    let user: String = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default().chars().filter(|c| c.is_alphanumeric()).collect();
    // 单元测试用自己的管道和标记，碰不到正在运行的 LiteView
    if cfg!(test) { format!("{user}test{}", std::process::id()) } else { user }
}

#[cfg(windows)]
fn instance() -> String {
    let family = crate::platform::package::identity().map(|identity| identity.family.as_str());
    crate::platform::package::instance_name(&user(), family)
}

#[cfg(windows)]
fn name() -> String {
    format!(r"\\.\pipe\{}", instance())
}

#[cfg(not(windows))]
fn name() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("liteview-{}.sock", user()))
}

/// 占「有一个 LiteView 在运行」的标记，一直占到进程退出。返回 true = 已经有别的实例占着（开着或正在启动）
pub fn claim_running() -> bool {
    #[cfg(windows)]
    {
        claim_mutex(&format!(r"Local\{}-running", instance()))
    }
    #[cfg(not(windows))]
    {
        claim_lock(&name().with_extension("lock"))
    }
}

#[cfg(windows)]
fn claim_mutex(name: &str) -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, NO_ERROR, SetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    let n = crate::platform::windows::wide(std::ffi::OsStr::new(name));
    unsafe {
        SetLastError(NO_ERROR);
        // 句柄故意不关：进程退出时系统关，别的进程据此知道这个实例不在了
        match CreateMutexW(None, false, windows::core::PCWSTR(n.as_ptr())) {
            Ok(_) => GetLastError() == ERROR_ALREADY_EXISTS,
            Err(_) => false,
        }
    }
}

#[cfg(not(windows))]
fn claim_lock(path: &Path) -> bool {
    let Ok(f) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(path) else { return false };
    match f.try_lock() {
        Ok(()) => {
            // 文件一直开着，锁就一直在；进程退出时系统放掉
            std::mem::forget(f);
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(_) => false,
    }
}

/// 运行中从多窗口换回单窗口：没有别的实例占着「在运行」的标记，就由这个进程占上、接收以后打开的图，返回 true。
/// 别的实例占着时不留句柄：它关掉以后标记就没了，后来的进程不会空等这个不接收的窗口
pub fn claim_if_free() -> bool {
    #[cfg(windows)]
    {
        claim_mutex_if_free(&format!(r"Local\{}-running", instance()))
    }
    #[cfg(not(windows))]
    {
        claim_lock_if_free(&name().with_extension("lock"))
    }
}

#[cfg(windows)]
fn claim_mutex_if_free(name: &str) -> bool {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, NO_ERROR, SetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    let n = crate::platform::windows::wide(std::ffi::OsStr::new(name));
    unsafe {
        SetLastError(NO_ERROR);
        let Ok(h) = CreateMutexW(None, false, windows::core::PCWSTR(n.as_ptr())) else { return false };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            return false;
        }
        // 和启动时占的一样，句柄留到进程退出
        true
    }
}

#[cfg(not(windows))]
fn claim_lock_if_free(path: &Path) -> bool {
    let Ok(f) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(path) else { return false };
    if f.try_lock().is_err() {
        return false;
    }
    std::mem::forget(f);
    true
}

/// 交给已经开着的实例；成功时本进程应直接退出。wait：管道还没开好、正忙时最多等多久（0 = 只试一次）
pub fn send(r: &Request, wait: Duration) -> bool {
    send_to(&name(), r, wait)
}

#[cfg(windows)]
fn send_to(name: &str, r: &Request, wait: Duration) -> bool {
    let Some(mut s) = connect(name, wait) else { return false };
    unsafe {
        // 让原窗口可以到最前面
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
    }
    s.write_all(encode(r).as_bytes()).is_ok()
}

#[cfg(not(windows))]
fn send_to(name: &Path, r: &Request, wait: Duration) -> bool {
    let Some(mut s) = connect(name, wait) else { return false };
    s.write_all(encode(r).as_bytes()).is_ok()
}

/// 连上管道：忙（ERROR_PIPE_BUSY，别的客户端刚连上、服务端还没建好下一个）就再试；没有管道时只在 wait 内再试
#[cfg(windows)]
fn connect(name: &str, wait: Duration) -> Option<std::fs::File> {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PIPE_BUSY: i32 = 231;
    // 正忙说明有服务端在：至少再等一小会儿，免得只因为撞上别的客户端就自己开窗口
    let busy_wait = wait.max(Duration::from_secs(2));
    let start = Instant::now();
    loop {
        let err = match std::fs::OpenOptions::new().write(true).open(name) {
            Ok(f) => return Some(f),
            Err(e) => e,
        };
        let limit = match err.raw_os_error() {
            Some(ERROR_PIPE_BUSY) => busy_wait,
            Some(ERROR_FILE_NOT_FOUND) => wait,
            _ => return None,
        };
        if start.elapsed() >= limit {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 连上套接字：还没有（文件不在）或没人在听（拒绝连接）时只在 wait 内再试
#[cfg(not(windows))]
fn connect(name: &Path, wait: Duration) -> Option<std::os::unix::net::UnixStream> {
    let start = Instant::now();
    loop {
        match std::os::unix::net::UnixStream::connect(name) {
            Ok(s) => return Some(s),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) && start.elapsed() < wait => {}
            Err(_) => return None,
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 在后台线程等别的实例发来的请求
pub fn serve(on: impl Fn(Request) + Send + 'static) {
    let _ = std::thread::Builder::new().name("single-instance".into()).spawn(move || serve_loop(&name(), &on));
}

#[cfg(windows)]
fn create_pipe(n: &[u16]) -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_INBOUND;
    use windows::Win32::System::Pipes::*;
    let h = unsafe {
        CreateNamedPipeW(
            windows::core::PCWSTR(n.as_ptr()),
            PIPE_ACCESS_INBOUND,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            0,
            4096,
            0,
            None,
        )
    };
    (!h.is_invalid()).then_some(h)
}

#[cfg(windows)]
fn serve_loop(name: &str, on: &dyn Fn(Request)) {
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Foundation::{ERROR_NO_DATA, ERROR_PIPE_CONNECTED};
    use windows::Win32::System::Pipes::ConnectNamedPipe;
    let n = crate::platform::windows::wide(std::ffi::OsStr::new(name));
    let Some(mut h) = create_pipe(&n) else { return };
    loop {
        // 客户端在这之前就连上了（PIPE_CONNECTED），或连上、写完、关掉了（NO_DATA：写的还在缓冲区里）都要读
        let connected = match unsafe { ConnectNamedPipe(h, None) } {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult() || e.code() == ERROR_NO_DATA.to_hresult(),
        };
        // 先建好下一个实例再读这一个：管道名一直在，后来的客户端不会以为没有服务端
        let next = create_pipe(&n);
        // 交给 File 管理句柄（读完自动关闭）
        let mut f = unsafe { std::fs::File::from_raw_handle(h.0) };
        let mut s = String::new();
        if connected
            && f.read_to_string(&mut s).is_ok()
            && let Some(r) = decode(&s)
        {
            on(r);
        }
        drop(f);
        let Some(n2) = next.or_else(|| create_pipe(&n)) else { return };
        h = n2;
    }
}

#[cfg(not(windows))]
fn serve_loop(name: &Path, on: &dyn Fn(Request)) {
    let _ = std::fs::remove_file(name);
    let Ok(listener) = std::os::unix::net::UnixListener::bind(name) else { return };
    for mut s in listener.incoming().flatten() {
        let mut text = String::new();
        if s.read_to_string(&mut text).is_ok()
            && let Some(r) = decode(&text)
        {
            on(r);
        }
    }
}

pub fn request(path: &Path, edit: bool, mosaic: bool) -> Request {
    Request { path: path.to_path_buf(), edit, mosaic }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn roundtrip() {
        let r = request(Path::new(r"C:\图片\a b.png"), false, true);
        assert_eq!(decode(&encode(&r)), Some(r));
        assert_eq!(decode("open /x/y.jpg").unwrap().path, PathBuf::from("/x/y.jpg"));
    }

    /// 测试专用的管道 / 套接字名，不碰正在运行的 LiteView
    #[cfg(windows)]
    fn test_name() -> String {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        format!(r"\\.\pipe\liteview-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    #[cfg(not(windows))]
    fn test_name() -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        std::env::temp_dir().join(format!("lv-test-{}-{}.sock", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
    }

    /// 在后台线程开一个服务端（测试结束时随进程退出）
    #[cfg(windows)]
    fn start_server(name: &str) -> Arc<Mutex<Vec<Request>>> {
        let got = Arc::new(Mutex::new(Vec::new()));
        let (sink, name) = (got.clone(), name.to_owned());
        std::thread::spawn(move || serve_loop(&name, &move |r| sink.lock().unwrap().push(r)));
        got
    }

    #[cfg(not(windows))]
    fn start_server(name: &Path) -> Arc<Mutex<Vec<Request>>> {
        let got = Arc::new(Mutex::new(Vec::new()));
        let (sink, name) = (got.clone(), name.to_owned());
        std::thread::spawn(move || serve_loop(&name, &move |r| sink.lock().unwrap().push(r)));
        got
    }

    #[test]
    fn many_images_opened_at_once_all_reach_the_running_window() {
        let name = test_name();
        let got = start_server(&name);
        // 多选几张回车：几个进程差不多同时连（以前只有一个连得上，其余各开一个窗口）
        let senders: Vec<_> = (0..8)
            .map(|i| {
                let name = name.clone();
                std::thread::spawn(move || send_to(&name, &request(Path::new(&format!("/d/{i}.png")), false, false), Duration::from_secs(3)))
            })
            .collect();
        assert!(senders.into_iter().all(|s| s.join().unwrap()), "every sender got through");
        let deadline = Instant::now() + Duration::from_secs(3);
        while got.lock().unwrap().len() < 8 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut paths: Vec<_> = got.lock().unwrap().iter().map(|r| r.path.clone()).collect();
        paths.sort();
        assert_eq!(paths, (0..8).map(|i| PathBuf::from(format!("/d/{i}.png"))).collect::<Vec<_>>());
    }

    #[test]
    fn a_sender_waits_for_an_instance_that_is_still_starting() {
        let name = test_name();
        let sender = {
            let name = name.clone();
            std::thread::spawn(move || send_to(&name, &request(Path::new("/d/late.png"), true, false), Duration::from_secs(3)))
        };
        std::thread::sleep(Duration::from_millis(300));
        let got = start_server(&name);
        assert!(sender.join().unwrap());
        let deadline = Instant::now() + Duration::from_secs(3);
        while got.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(got.lock().unwrap().first().map(|r| (r.path.clone(), r.edit)), Some((PathBuf::from("/d/late.png"), true)));
    }

    #[test]
    fn without_a_running_instance_sending_gives_up_at_once() {
        let t = Instant::now();
        assert!(!send_to(&test_name(), &request(Path::new("/d/x.png"), false, false), Duration::ZERO));
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn the_second_claim_sees_the_first() {
        #[cfg(windows)]
        let claim = || claim_mutex(&format!(r"Local\liteview-test-{}-running", std::process::id()));
        #[cfg(not(windows))]
        let claim = || claim_lock(&std::env::temp_dir().join(format!("lv-test-{}.lock", std::process::id())));
        assert!(!claim(), "first one runs");
        assert!(claim(), "the next one sees it");
    }

    #[test]
    fn switching_back_to_one_window_only_claims_a_free_mark() {
        #[cfg(windows)]
        let (startup, later) = {
            let name = format!(r"Local\liteview-test-{}-later", std::process::id());
            let n2 = name.clone();
            (move || claim_mutex(&name), move || claim_mutex_if_free(&n2))
        };
        #[cfg(not(windows))]
        let (startup, later) = {
            let path = std::env::temp_dir().join(format!("lv-test-{}-later.lock", std::process::id()));
            let p2 = path.clone();
            (move || claim_lock(&path), move || claim_lock_if_free(&p2))
        };
        // 没人占着：占上；再来一次（另一个窗口也换回单窗口）：已经有人占着，不占
        assert!(later(), "free mark is taken");
        assert!(!later(), "a taken mark is left alone");
        // 之后启动的进程看得到它
        assert!(startup());
    }
}