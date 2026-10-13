//! 单实例：已经开着 LiteView 时，双击新图片在原窗口打开。
//! Windows 用命名管道（按用户名区分），其它系统用 Unix 套接字（$XDG_RUNTIME_DIR 或临时目录）。
//! 消息就是一行：`open|edit|mosaic <路径>`。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default().chars().filter(|c| c.is_alphanumeric()).collect()
}

#[cfg(windows)]
fn name() -> String {
    format!(r"\\.\pipe\LiteView-{}", user())
}

#[cfg(not(windows))]
fn name() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("liteview-{}.sock", user()))
}

/// 交给已经开着的实例；成功时本进程应直接退出
pub fn send(r: &Request) -> bool {
    #[cfg(windows)]
    let stream = std::fs::OpenOptions::new().write(true).open(name());
    #[cfg(not(windows))]
    let stream = std::os::unix::net::UnixStream::connect(name());
    let Ok(mut s) = stream else { return false };
    #[cfg(windows)]
    unsafe {
        // 让原窗口可以到最前面
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
    }
    s.write_all(encode(r).as_bytes()).is_ok()
}

/// 在后台线程等别的实例发来的请求
pub fn serve(on: impl Fn(Request) + Send + 'static) {
    let _ = std::thread::Builder::new().name("single-instance".into()).spawn(move || serve_loop(&on));
}

#[cfg(windows)]
fn serve_loop(on: &dyn Fn(Request)) {
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Foundation::ERROR_PIPE_CONNECTED;
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_INBOUND;
    use windows::Win32::System::Pipes::*;
    let n = crate::platform::windows::wide(std::ffi::OsStr::new(&name()));
    loop {
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
        if h.is_invalid() {
            return;
        }
        let connected = match unsafe { ConnectNamedPipe(h, None) } {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        };
        // 交给 File 管理句柄（读完自动关闭）
        let mut f = unsafe { std::fs::File::from_raw_handle(h.0) };
        let mut s = String::new();
        if connected && f.read_to_string(&mut s).is_ok()
            && let Some(r) = decode(&s)
        {
            on(r);
        }
    }
}

#[cfg(not(windows))]
fn serve_loop(on: &dyn Fn(Request)) {
    let p = name();
    let _ = std::fs::remove_file(&p);
    let Ok(listener) = std::os::unix::net::UnixListener::bind(&p) else { return };
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

    #[test]
    fn roundtrip() {
        let r = request(Path::new(r"C:\图片\a b.png"), false, true);
        assert_eq!(decode(&encode(&r)), Some(r));
        assert_eq!(decode("open /x/y.jpg").unwrap().path, PathBuf::from("/x/y.jpg"));
    }
}
