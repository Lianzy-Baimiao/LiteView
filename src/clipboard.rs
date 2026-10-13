//! 剪贴板（arboard）。Windows 上复制图片会同时放 PNG 和位图（CF_DIBV5）两种格式：
//! 透明通道不丢，微信、QQ、Word、浏览器都能粘贴。
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

/// 读剪贴板（阻塞；在后台线程调用）
pub fn get() -> Content {
    let (tx, rx) = channel();
    if worker().lock().ok().and_then(|w| w.send(Req::Get(tx)).ok()).is_none() {
        return Content::Nothing;
    }
    rx.recv().unwrap_or(Content::Nothing)
}
