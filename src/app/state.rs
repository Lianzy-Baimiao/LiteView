//! 打开、切换图片；解码完成后换上新图。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use super::{After, App, Current, Dialog, Shown, Tex};
use crate::decode::{Content, Decoded};
use crate::i18n::tr;
use crate::viewer::folder::{Folder, Polled, Step, same_path};
use crate::viewer::loader::Loaded;
use crate::viewer::playback::Playback;
use crate::viewer::texture::{AnimTexture, StillTextures};

impl App {
    /// 打开一个文件（命令行、拖入、Ctrl+O、粘贴的文件）
    pub(crate) fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        if self.needs_rotation_prompt() {
            self.dialog = Some(Dialog::SaveRotation { after: After::Open(path) });
            return;
        }
        let same_dir = self.folder.current().and_then(|c| c.parent()) == path.parent() && !self.folder.scanning();
        match self.folder.files.iter().position(|p| p == &path) {
            Some(i) if same_dir => self.folder.go(i),
            _ => self.folder = Folder::open(&path, self.settings.sort, self.settings.sort_desc, Some(ctx.clone())),
        }
        self.show_current();
    }

    /// 显示 folder 的当前项；解码完成前继续显示旧图
    pub(crate) fn show_current(&mut self) {
        let Some(path) = self.folder.current().map(PathBuf::from) else {
            self.cur = None;
            self.tex = Tex::None;
            self.shown = None;
            self.play = None;
            return;
        };
        self.cur = Some(Current { path: path.clone(), loaded: None, since: Instant::now(), clipboard: false });
        if let Some(l) = self.loader.request(&path) {
            self.on_loaded(l);
        }
        self.loader.preload(&self.folder.neighbors(self.settings.preload as usize));
    }

    pub(crate) fn needs_rotation_prompt(&self) -> bool {
        !self.view.xf.is_identity()
            && self.cur.as_ref().is_some_and(|c| !c.clipboard && crate::save::can_save_rotation(&c.path))
    }

    pub(crate) fn navigate(&mut self, delta: isize) {
        if self.needs_rotation_prompt() {
            self.dialog = Some(Dialog::SaveRotation { after: After::Step(delta) });
            return;
        }
        let t = tr();
        match self.folder.step(delta, self.settings.wrap_around) {
            Step::Moved => {}
            Step::Wrapped => self.show_toast(if delta > 0 { t.wrapped_first } else { t.wrapped_last }),
            Step::AtEdge => {
                self.show_toast(if delta > 0 { t.at_last } else { t.at_first });
                return;
            }
            Step::Single => {
                if !self.folder.scanning() && self.cur.is_some() {
                    self.show_toast(t.only_one);
                }
                return;
            }
        }
        self.show_current();
    }

    pub(crate) fn go(&mut self, index: usize) {
        if self.needs_rotation_prompt() {
            self.dialog = Some(Dialog::SaveRotation { after: After::Go(index) });
            return;
        }
        if index != self.folder.index && index < self.folder.len() {
            self.folder.go(index);
            self.show_current();
        }
    }

    /// 确认框之后继续
    pub(crate) fn resume(&mut self, after: After, ctx: &egui::Context) {
        self.view.xf = Default::default();
        match after {
            After::Step(d) => self.navigate(d),
            After::Go(i) => self.go(i),
            After::Open(p) => self.open_path(p, ctx),
            After::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub(crate) fn poll_loader(&mut self, ctx: &egui::Context) {
        match self.folder.poll() {
            Polled::Nothing => {}
            Polled::Changed => {
                self.loader.preload(&self.folder.neighbors(self.settings.preload as usize));
                ctx.request_repaint();
            }
            // 当前图被外部删除或移走：和在这里删除一样显示下一张
            Polled::CurrentGone => {
                self.view.xf = Default::default();
                self.show_toast(tr().err_not_found);
                self.show_current();
            }
        }
        let keep = self.cur.as_ref().map(|c| c.path.clone());
        let done = self.loader.poll(keep.as_deref());
        if let Some(c) = &self.cur
            && c.loaded.is_none()
            && done.contains(&c.path)
            && let Some(l) = self.loader.get(&c.path)
        {
            self.on_loaded(l);
        }
    }

    /// 监听当前文件夹：列表变化时重新扫描；图片被外部修改后丢掉缓存，当前图重新载入
    pub(crate) fn poll_watch(&mut self, ctx: &egui::Context) {
        let dir = self.cur.as_ref().filter(|c| !c.clipboard).and_then(|_| self.folder.dir()).map(PathBuf::from);
        if dir != self.watch_dir {
            self.watch = dir.as_deref().and_then(|d| crate::viewer::watch::Watch::new(d, ctx.clone()));
            self.watch_dir = dir;
        }
        let Some(changes) = self.watch.as_mut().and_then(|w| w.poll(ctx)) else { return };
        self.self_saved.retain(|(_, t)| t.elapsed().as_secs_f32() < 3.0);
        for p in &changes.paths {
            self.loader.forget(p);
        }
        if changes.list {
            self.folder.rescan(self.settings.sort, self.settings.sort_desc, Some(ctx.clone()));
        }
        let reload = self.cur.as_ref().is_some_and(|c| {
            !c.clipboard
                && changes.paths.iter().any(|p| same_path(p, &c.path))
                && c.path.exists()
                && !self.self_saved.iter().any(|(p, _)| same_path(p, &c.path))
        });
        if reload && !self.needs_rotation_prompt() {
            log::info!("当前图被外部修改，重新载入");
            self.reload_current();
        }
    }

    /// 重新解码当前图（文件被修改、保存之后）
    pub(crate) fn reload_current(&mut self) {
        let Some(c) = &mut self.cur else { return };
        let path = c.path.clone();
        self.loader.forget(&path);
        c.loaded = None;
        c.since = Instant::now();
        if let Some(l) = self.loader.request(&path) {
            self.on_loaded(l);
        }
    }

    /// 当前图解码完成：换纹理、重置显示
    pub(crate) fn on_loaded(&mut self, l: Loaded) {
        let Some(cur) = &mut self.cur else { return };
        cur.loaded = Some(l.clone());
        self.view.xf = Default::default();
        self.play = None;
        match l {
            Ok(d) => {
                self.shown = Some(Shown { width: d.info.width, height: d.info.height, has_alpha: d.info.has_alpha });
                match &d.content {
                    Content::Still { image, levels } => {
                        self.tex = Tex::Still(StillTextures::new("image", image.clone(), levels));
                    }
                    Content::Animation { frames, plays, .. } => {
                        let s = &self.settings;
                        self.play = Some(Playback::new(frames.len(), *plays, s.anim_autoplay, s.anim_always_loop));
                        self.tex = Tex::Anim { decoded: d.clone(), tex: AnimTexture::new() };
                    }
                }
                self.pending_reset = true;
            }
            Err(_) => {
                self.tex = Tex::None;
                self.shown = None;
            }
        }
    }

    /// 粘贴的图片（没有源文件）
    pub(crate) fn show_pasted(&mut self, d: Decoded) {
        if self.needs_rotation_prompt() {
            self.view.xf = Default::default();
        }
        self.paste_seq += 1;
        self.folder = Folder::empty();
        self.cur = Some(Current {
            path: PathBuf::from(format!("clipboard-{}.png", self.paste_seq)),
            loaded: None,
            since: Instant::now(),
            clipboard: true,
        });
        self.on_loaded(Ok(Arc::new(d)));
    }

    /// 正在显示的解码结果
    pub(crate) fn decoded(&self) -> Option<&Arc<Decoded>> {
        self.cur.as_ref()?.loaded.as_ref()?.as_ref().ok()
    }

    pub(crate) fn is_animated(&self) -> bool {
        self.decoded().is_some_and(|d| d.is_animated())
    }
}
