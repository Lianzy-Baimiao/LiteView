//! 连续翻页：按住 ← → / PgUp PgDn，或按住工具栏、窗口边上的翻页按钮。按下立刻翻一张；按住半秒后开始连续翻，
//! 每张显示出来后停 0.3 秒再翻（大图要等解码完，不会还没看到就跳过）。系统的按键重复（约每秒 30 次）不用，太快看不清。
//! 循环翻回第一张 / 最后一张时多停一会儿，让人看到窗口中间的提示；不循环时到头就停。

use std::time::{Duration, Instant};

use super::App;

/// 按住多久开始连续翻
pub(crate) const DELAY: Duration = Duration::from_millis(500);
/// 连续翻时每张至少显示多久
pub(crate) const STEP: Duration = Duration::from_millis(300);
/// 循环到另一头后多停一会儿
pub(crate) const WRAP_PAUSE: Duration = Duration::from_millis(1200);
/// 新图还没显示出来时，隔多久再看一次
const POLL: Duration = Duration::from_millis(50);

/// 翻了一张之后怎么样
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nav {
    Moved,
    /// 越过开头 / 末尾回到另一头
    Wrapped,
    /// 没翻：到头了、只有一张、弹出了确认框
    Stopped,
}

#[derive(Clone, Copy, Debug)]
struct Hold {
    dir: isize,
    /// 当前这张显示出来的时刻（还在解码时为 None）
    shown: Option<Instant>,
    /// 显示出来后停多久再翻
    wait: Duration,
}

/// 每帧由键盘和翻页按钮填 pressed / held，`tick` 决定翻不翻
#[derive(Default)]
pub(crate) struct Flip {
    /// 这一帧刚按下（-1 上一张、1 下一张）
    pub pressed: Option<isize>,
    /// 这一帧结束时还按着
    pub held: Option<isize>,
    /// 按下的翻页按钮（egui 按住超过 0.8 秒就不算点击、不再报告按着，所以自己记着）
    pub button: Option<egui::Id>,
    hold: Option<Hold>,
}

impl Flip {
    /// 这一帧要不要翻、往哪翻；ready = 当前图已经显示出来（或已报错）
    pub(crate) fn tick(&mut self, now: Instant, ready: bool) -> Option<isize> {
        let (pressed, held) = (self.pressed.take(), self.held.take());
        if let Some(d) = pressed {
            // 按下立刻翻；还按着就从这张显示出来起计时
            self.hold = (held == Some(d)).then_some(Hold { dir: d, shown: None, wait: DELAY });
            return Some(d);
        }
        let Some(h) = self.hold.as_mut().filter(|h| Some(h.dir) == held) else {
            self.hold = None;
            return None;
        };
        if !ready {
            return None;
        }
        let shown = *h.shown.get_or_insert(now);
        if now < shown + h.wait {
            return None;
        }
        h.shown = None;
        h.wait = STEP;
        Some(h.dir)
    }

    /// 翻完的结果：循环到另一头多停一会儿，到头就不再连续翻
    pub(crate) fn moved(&mut self, nav: Nav) {
        match nav {
            Nav::Moved => {}
            Nav::Wrapped => {
                if let Some(h) = &mut self.hold {
                    h.wait = h.wait.max(WRAP_PAUSE);
                }
            }
            Nav::Stopped => self.hold = None,
        }
    }

    /// 下一次要看的时刻（没按着就是 None）
    pub(crate) fn due(&self, now: Instant) -> Option<Instant> {
        let h = self.hold?;
        Some(h.shown.map_or(now + POLL, |s| s + h.wait))
    }
}

impl App {
    /// 键盘与翻页按钮都看过之后：该翻就翻，按着时到点再画一帧
    pub(crate) fn flip_tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let ready = self.cur.as_ref().is_some_and(|c| c.loaded.is_some() || c.preview.is_some());
        if let Some(d) = self.flip.tick(now, ready) {
            let nav = self.navigate(d);
            log::debug!("翻页 {d:+}：{nav:?}，第 {} / {} 张", self.folder.index + 1, self.folder.len());
            self.flip.moved(nav);
        }
        if let Some(at) = self.flip.due(now) {
            ctx.request_repaint_after(at.saturating_duration_since(now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// 一帧：pressed / held 由调用方给
    fn frame(f: &mut Flip, t: Instant, pressed: Option<isize>, held: Option<isize>, ready: bool) -> Option<isize> {
        f.pressed = pressed;
        f.held = held;
        f.tick(t, ready)
    }

    #[test]
    fn a_tap_turns_one_page() {
        let (mut f, t) = (Flip::default(), Instant::now());
        // 同一帧里按下又松开（快速点一下）
        assert_eq!(frame(&mut f, t, Some(1), None, true), Some(1));
        for i in 1..40 {
            assert_eq!(frame(&mut f, t + ms(i * 50), None, None, true), None);
        }
        assert!(f.due(t).is_none());
    }

    #[test]
    fn holding_waits_half_a_second_then_turns_every_step_after_each_image_shows() {
        let (mut f, t) = (Flip::default(), Instant::now());
        assert_eq!(frame(&mut f, t, Some(1), Some(1), true), Some(1));
        f.moved(Nav::Moved);
        // 新图 100 ms 后才显示出来：从那时起算半秒
        assert_eq!(frame(&mut f, t + ms(50), None, Some(1), false), None);
        assert_eq!(frame(&mut f, t + ms(100), None, Some(1), true), None);
        assert_eq!(frame(&mut f, t + ms(599), None, Some(1), true), None);
        assert_eq!(frame(&mut f, t + ms(600), None, Some(1), true), Some(1));
        f.moved(Nav::Moved);
        // 下一张马上显示：0.3 秒后再翻
        assert_eq!(frame(&mut f, t + ms(616), None, Some(1), true), None);
        assert_eq!(f.due(t + ms(616)), Some(t + ms(916)));
        assert_eq!(frame(&mut f, t + ms(915), None, Some(1), true), None);
        assert_eq!(frame(&mut f, t + ms(916), None, Some(1), true), Some(1));
        // 松开就停
        assert_eq!(frame(&mut f, t + ms(2000), None, None, true), None);
        assert!(f.due(t + ms(2000)).is_none());
    }

    #[test]
    fn wrapping_pauses_and_the_edge_stops() {
        let (mut f, t) = (Flip::default(), Instant::now());
        assert_eq!(frame(&mut f, t, Some(1), Some(1), true), Some(1));
        f.moved(Nav::Wrapped);
        assert_eq!(frame(&mut f, t + ms(10), None, Some(1), true), None);
        assert_eq!(frame(&mut f, t + ms(1209), None, Some(1), true), None);
        assert_eq!(frame(&mut f, t + ms(1210), None, Some(1), true), Some(1));
        // 不循环、到头了：按着也不再翻
        f.moved(Nav::Stopped);
        for i in 1..30 {
            assert_eq!(frame(&mut f, t + ms(1210 + i * 100), None, Some(1), true), None);
        }
    }

    #[test]
    fn switching_direction_or_letting_go_restarts() {
        let (mut f, t) = (Flip::default(), Instant::now());
        assert_eq!(frame(&mut f, t, Some(1), Some(1), true), Some(1));
        // 改按另一边：立刻往回翻一张，重新等半秒
        assert_eq!(frame(&mut f, t + ms(200), Some(-1), Some(-1), true), Some(-1));
        assert_eq!(frame(&mut f, t + ms(216), None, Some(-1), true), None);
        assert_eq!(frame(&mut f, t + ms(715), None, Some(-1), true), None);
        assert_eq!(frame(&mut f, t + ms(716), None, Some(-1), true), Some(-1));
        // 按着的方向变了（另一个键还按着）也停
        assert_eq!(frame(&mut f, t + ms(800), None, Some(1), true), None);
        assert_eq!(frame(&mut f, t + ms(5000), None, Some(1), true), None);
    }
}
