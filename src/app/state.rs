//! 打开、切换图片；解码完成后换上新图（预读过的图先显示缩略图）。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::flip::Nav;
use super::{After, App, Current, Dialog, Shown, Tex};

/// 放大看过后回到适应窗口，停多久放掉原图
const SETTLE: Duration = Duration::from_secs(3);
use crate::decode::{Content, Decoded};
use crate::i18n::tr;
use crate::viewer::folder::{Folder, Polled, Step, same_path};
use crate::viewer::loader::{Loaded, Preview};
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
            self.loader.release();
            return;
        };
        self.cur = Some(Current { path: path.clone(), loaded: None, preview: None, since: Instant::now(), clipboard: false });
        self.full_requested = false;
        self.settle_since = None;
        self.pending_action = None;
        self.loader.request(&path);
        self.sync_current();
        self.loader.preload(&self.folder.neighbors(self.settings.preload as usize));
    }

    /// 要原图像素（放大到缩略图不够清楚、编辑、复制、另存为）：留住或马上解码，到了之后换上
    pub(crate) fn want_full(&mut self) {
        let Some(c) = self.cur.as_ref().filter(|c| !c.clipboard && c.loaded.is_none()) else { return };
        if !self.full_requested {
            self.full_requested = true;
            let path = c.path.clone();
            self.loader.want_full(&path);
        }
    }

    /// 把缓存里的状态反映到画面上：先显示缩略图；要原图时换上原图；不再需要时（停在适应窗口 3 秒）换回缩略图、放掉原图
    fn sync_current(&mut self) {
        let Some(c) = &self.cur else { return };
        if c.clipboard {
            return;
        }
        let path = c.path.clone();
        let showing_full = c.loaded.is_some();
        let shown_preview = c.preview.clone();
        let full = self.loader.get(&path);
        let fresh = self.loader.fresh_preview(&path);
        // 停在这张缩略图的比例上（适应窗口），换成它就和原图准确缩小一样
        let (reset, animating, zoom) = (self.pending_reset, self.view.animating(), self.view.zoom);
        let fits = move |p: &Preview| !reset && !animating && (zoom - p.zoom).abs() < 1e-5;
        if !showing_full {
            match (shown_preview, fresh, full) {
                // 还什么都没显示：按当前窗口做的缩略图优先，然后是原图（第一张图、动图、比窗口小的图、打不开），
                // 最后是按之前窗口大小做的缩略图（先拉伸着显示，新的解完换上）
                (None, Some(p), _) => self.on_preview(p),
                (None, None, Some(l)) => self.on_loaded(l),
                (None, None, None) => {
                    if let Some(p) = self.loader.preview(&path) {
                        self.on_preview(p);
                    }
                }
                // 窗口大小变了：有按新大小做的缩略图、比例对上了就换上，不用原图
                (Some(old), Some(p), _) if !Arc::ptr_eq(&old, &p) && fits(&p) && self.pending_action.is_none() && self.pending_edit.is_none() => {
                    self.swap_to_preview(p);
                    if self.full_requested {
                        self.full_requested = false;
                        self.loader.release_full(&path);
                    }
                }
                (Some(_), _, Some(l)) if self.full_requested || l.is_err() => self.on_upgrade(l),
                _ => {}
            }
            return;
        }
        let idle = self.editor.is_none() && self.pending_action.is_none() && self.pending_edit.is_none();
        match fresh.filter(|p| idle && fits(p)) {
            Some(p) => {
                // 放大看过的：在适应窗口停 3 秒再放掉（免得来回缩放反复解码）；没放大过的（第一张图）马上换
                let since = *self.settle_since.get_or_insert_with(Instant::now);
                let wait = if self.full_requested { SETTLE.saturating_sub(since.elapsed()) } else { Duration::ZERO };
                if wait.is_zero() {
                    self.swap_to_preview(p);
                    self.full_requested = false;
                    self.loader.release_full(&path);
                } else {
                    self.ctx.request_repaint_after(wait);
                }
            }
            None => self.settle_since = None,
        }
    }

    /// 换回缩略图：缩放、位置、旋转都不动；原图的纹理和像素放掉
    fn swap_to_preview(&mut self, p: Arc<Preview>) {
        let Some(cur) = &mut self.cur else { return };
        log::debug!("换回缩略图 {}", cur.path.display());
        cur.loaded = None;
        cur.preview = Some(p.clone());
        self.settle_since = None;
        self.tex = Tex::Still(StillTextures::preview("image", &p));
    }

    pub(crate) fn needs_rotation_prompt(&self) -> bool {
        !self.view.xf.is_identity() && self.cur.as_ref().is_some_and(|c| !c.clipboard && crate::save::can_save_rotation(&c.path))
    }

    /// 上一张 / 下一张；到头、循环、只有一张时在窗口中间提示
    pub(crate) fn navigate(&mut self, delta: isize) -> Nav {
        if self.needs_rotation_prompt() {
            self.dialog = Some(Dialog::SaveRotation { after: After::Step(delta) });
            return Nav::Stopped;
        }
        let t = tr();
        let nav = match self.folder.step(delta, self.settings.wrap_around) {
            Step::Moved => Nav::Moved,
            Step::Wrapped => {
                self.show_notice(if delta > 0 { t.wrapped_first } else { t.wrapped_last });
                Nav::Wrapped
            }
            Step::AtEdge => {
                self.show_notice(if delta > 0 { t.at_last } else { t.at_first });
                return Nav::Stopped;
            }
            Step::Single => {
                if !self.folder.scanning() && self.cur.is_some() {
                    self.show_notice(t.only_one);
                }
                return Nav::Stopped;
            }
        };
        self.pending_edit = None;
        self.show_current();
        nav
    }

    pub(crate) fn go(&mut self, index: usize) {
        if self.needs_rotation_prompt() {
            self.dialog = Some(Dialog::SaveRotation { after: After::Go(index) });
            return;
        }
        if index != self.folder.index && index < self.folder.len() {
            self.pending_edit = None;
            self.folder.go(index);
            self.show_current();
        }
    }

    /// 确认框之后继续
    pub(crate) fn resume(&mut self, after: After, ctx: &egui::Context) {
        self.view.xf = Default::default();
        match after {
            After::Step(d) => {
                self.navigate(d);
            }
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
        self.loader.poll();
        // 结果也可能是 request / preload 时收进缓存的，所以每帧直接看缓存
        self.sync_current();
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
        c.preview = None;
        c.since = Instant::now();
        self.full_requested = false;
        self.settle_since = None;
        self.loader.request(&path);
        self.sync_current();
    }

    /// 开始显示另一张图：复位旋转、播放；同样尺寸且「锁定缩放」时保持缩放与位置
    fn begin_show(&mut self, shown: Shown) {
        self.view.xf = Default::default();
        self.play = None;
        let before = self.shown.replace(shown);
        // 「锁定缩放与位置」：切到同样尺寸的图时保持缩放与位置，方便对比
        let same = before.is_some_and(|b| (b.width, b.height) == (shown.width, shown.height));
        if !(self.settings.lock_zoom && same) {
            self.pending_reset = true;
        }
        if self.settings.fit_window {
            self.pending_window_fit = true;
        }
    }

    /// 切到预读的图：先显示缩略图，原图解完后 `on_upgrade`
    fn on_preview(&mut self, p: Arc<Preview>) {
        let Some(cur) = &mut self.cur else { return };
        log::debug!("先显示缩略图 {}", cur.path.display());
        cur.preview = Some(p.clone());
        self.begin_show(Shown { width: p.info.width, height: p.info.height, has_alpha: p.info.has_alpha });
        self.tex = Tex::Still(StillTextures::preview("image", &p));
    }

    /// 显示缩略图时原图解完了：直接换上，缩放、位置、旋转都不动
    fn on_upgrade(&mut self, l: Loaded) {
        if let (Ok(d), Tex::Still(t)) = (&l, &mut self.tex)
            && let Content::Still { image, levels } = &d.content
            && self.shown.is_some_and(|s| (s.width, s.height) == (d.info.width, d.info.height))
        {
            t.upgrade(image.clone(), levels);
            if let Some(cur) = &mut self.cur {
                log::debug!("换上原图 {}", cur.path.display());
                cur.loaded = Some(l.clone());
                cur.preview = None;
            }
            return;
        }
        // 动图、打不开、或文件在这期间变了：当新图处理
        self.on_loaded(l);
    }

    /// 当前图解码完成：换纹理、重置显示
    pub(crate) fn on_loaded(&mut self, l: Loaded) {
        let Some(cur) = &mut self.cur else { return };
        cur.loaded = Some(l.clone());
        cur.preview = None;
        match l {
            Ok(d) => {
                self.begin_show(Shown { width: d.info.width, height: d.info.height, has_alpha: d.info.has_alpha });
                match &d.content {
                    Content::Still { image, levels } => {
                        self.tex = Tex::Still(StillTextures::new("image", image.clone(), levels));
                    }
                    Content::Animation { delays, plays, .. } => {
                        let s = &self.settings;
                        let mut p = Playback::new(delays.len(), *plays, s.anim_autoplay, s.anim_always_loop);
                        p.set_speed(s.anim_speed);
                        self.play = Some(p);
                        self.tex = Tex::Anim {
                            decoded: d.clone(),
                            tex: AnimTexture::new(),
                            frames: crate::viewer::animation::AnimationFrames::new(d.clone(), self.ctx.clone()),
                        };
                    }
                }
            }
            Err(_) => {
                self.view.xf = Default::default();
                self.play = None;
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
        self.loader.release();
        let path = PathBuf::from(format!("clipboard-{}.png", self.paste_seq));
        self.cur = Some(Current { path, loaded: None, preview: None, since: Instant::now(), clipboard: true });
        self.full_requested = false;
        self.settle_since = None;
        self.pending_action = None;
        self.on_loaded(Ok(Arc::new(d)));
    }

    /// 正在显示的解码结果（只显示缩略图时是 None，要像素时先 `want_full`）
    pub(crate) fn decoded(&self) -> Option<&Arc<Decoded>> {
        self.cur.as_ref()?.loaded.as_ref()?.as_ref().ok()
    }

    /// 有图可以操作（原图，或按窗口缩好的缩略图）
    pub(crate) fn has_image(&self) -> bool {
        self.decoded().is_some() || self.cur.as_ref().is_some_and(|c| c.preview.is_some())
    }

    pub(crate) fn is_animated(&self) -> bool {
        self.decoded().is_some_and(|d| d.is_animated())
    }

    /// 打印对话框开着（快捷键都不处理）
    pub(crate) fn printing(&self) -> bool {
        #[cfg(windows)]
        {
            self.print.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// 开着对话框（确认框、设置、重命名、打印）：查看模式的快捷键和浮动工具栏都不出来
    pub(crate) fn modal_open(&self) -> bool {
        self.dialog.is_some() || self.settings_page.is_some() || self.rename.is_some() || self.printing()
    }
}
