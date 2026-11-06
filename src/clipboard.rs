//! 剪贴板（arboard）。Windows 上复制静态图片会同时放 PNG 和位图（CF_DIBV5）两种格式：
//! 透明通道不丢，微信、QQ、Word、浏览器都能粘贴。
//! GIF 动图复制原文件（Windows：CF_HDROP），保留全部帧、帧延时与循环次数。
//! Linux X11 上剪贴板内容要由持有它的对象一直提供，所以用一个常驻线程拿着 Clipboard。

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};

use image::RgbaImage;

pub enum Content {
    Files(Vec<PathBuf>),
    Image(RgbaImage),
    Path(PathBuf),
    Nothing,
}

enum Req {
    SetImage(RgbaImage, Sender<Result<(), String>>),
    SetFile(PathBuf, Sender<Result<(), String>>),
    Get(Sender<Content>),
}

fn worker() -> &'static Mutex<Sender<Req>> {
    static W: OnceLock<Mutex<Sender<Req>>> = OnceLock::new();
    W.get_or_init(|| {
        let (tx, rx) = channel::<Req>();
        std::thread::Builder::new()
            .name("clipboard".into())
            .spawn(move || {
                let mut cb = arboard::Clipboard::new().ok();
                for req in rx {
                    if cb.is_none() {
                        cb = arboard::Clipboard::new().ok();
                    }
                    match req {
                        Req::SetImage(img, reply) => {
                            let r = match &mut cb {
                                Some(cb) => {
                                    let (w, h) = img.dimensions();
                                    let data = arboard::ImageData { width: w as usize, height: h as usize, bytes: img.into_raw().into() };
                                    cb.set_image(data).map_err(|e| e.to_string())
                                }
                                None => Err("clipboard unavailable".into()),
                            };
                            let _ = reply.send(r);
                        }
                        Req::SetFile(path, reply) => {
                            let r = match &mut cb {
                                Some(cb) => {
                                    // arboard 的 Windows file_list 不会清空已有格式；残留的 PNG / DIB
                                    // 会让聊天软件优先粘贴静态图，所以先清空，再只放完整文件。
                                    cb.clear().and_then(|()| cb.set().file_list(&[path])).map_err(|e| e.to_string())
                                }
                                None => Err("clipboard unavailable".into()),
                            };
                            let _ = reply.send(r);
                        }
                        Req::Get(reply) => {
                            let _ = reply.send(cb.as_mut().map_or(Content::Nothing, read));
                        }
                    }
                }
            })
            .expect("spawn clipboard thread");
        Mutex::new(tx)
    })
}

fn read(cb: &mut arboard::Clipboard) -> Content {
    if let Ok(files) = cb.get().file_list()
        && !files.is_empty()
    {
        return Content::Files(files);
    }
    if let Ok(img) = cb.get_image()
        && let Some(rgba) = RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
    {
        return Content::Image(rgba);
    }
    if let Ok(text) = cb.get_text() {
        let p = PathBuf::from(text.trim().trim_matches('"'));
        if p.is_file() {
            return Content::Path(p);
        }
    }
    Content::Nothing
}

/// 复制图片（阻塞到写完；在后台线程调用）
pub fn set_image(img: RgbaImage) -> Result<(), String> {
    let (tx, rx) = channel();
    worker().lock().map_err(|e| e.to_string())?.send(Req::SetImage(img, tx)).map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// 复制完整文件（阻塞到写完；在后台线程调用）。先确认路径存在，路径失效时不清空剪贴板。
pub fn set_file(path: PathBuf) -> Result<(), String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let (tx, rx) = channel();
    worker().lock().map_err(|e| e.to_string())?.send(Req::SetFile(path, tx)).map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// 读剪贴板（阻塞；在后台线程调用）
pub fn get() -> Content {
    let (tx, rx) = channel();
    if worker().lock().ok().and_then(|w| w.send(Req::Get(tx)).ok()).is_none() {
        return Content::Nothing;
    }
    rx.recv().unwrap_or(Content::Nothing)
}
