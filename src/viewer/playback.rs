//! 动图播放：按每帧自己的延时；按绝对时间排帧，长时间播放不累积误差。

use std::time::{Duration, Instant};

pub struct Playback {
    pub index: usize,
    pub playing: bool,
    pub speed: f32,
    count: usize,
    plays: Option<u32>,
    plays_done: u32,
    due: Option<Instant>,
}

pub const SPEEDS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// 提前这么多以内醒来也换帧（计时器的误差）
const SLACK: Duration = Duration::from_millis(2);

impl Playback {
    pub fn new(count: usize, plays: Option<u32>, autoplay: bool, always_loop: bool) -> Self {
        Self {
            index: 0,
            playing: autoplay && count > 1,
            speed: 1.0,
            count: count.max(1),
            plays: if always_loop { None } else { plays },
            plays_done: 0,
            due: None,
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    fn finished(&self) -> bool {
        self.plays.is_some_and(|p| self.plays_done >= p)
    }

    pub fn toggle(&mut self) {
        if !self.playing && self.finished() {
            self.index = 0;
            self.plays_done = 0;
        }
        self.playing = !self.playing && self.count > 1;
        self.due = None;
    }

    /// 逐帧：自动暂停
    pub fn step(&mut self, delta: isize) {
        self.playing = false;
        self.due = None;
        self.index = (self.index as isize + delta).rem_euclid(self.count as isize) as usize;
    }

    pub fn seek(&mut self, index: usize) {
        self.index = index.min(self.count - 1);
        self.due = None;
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
        self.due = None;
    }

    /// 帧还在解码时不追帧；就绪后给这一帧完整显示时间。
    pub fn buffering(&mut self) {
        self.due = None;
    }

    /// 推进到 now 应显示的帧；返回离下一帧还有多久（暂停或播完时 None）。离换帧不到 SLACK 也算到点：
    /// 计时器早醒一点时不为了这一点再画一次旧帧
    pub fn tick(&mut self, now: Instant, delay_of: impl Fn(usize) -> Duration) -> Option<Duration> {
        if !self.playing || self.count < 2 {
            return None;
        }
        let speed = self.speed.max(0.01);
        let scaled = |d: Duration| d.div_f32(speed);
        let mut due = *self.due.get_or_insert_with(|| now + scaled(delay_of(self.index)));
        let mut steps = 0;
        while now + SLACK >= due {
            if self.index + 1 >= self.count {
                self.plays_done += 1;
                if self.finished() {
                    self.playing = false;
                    self.due = None;
                    return None;
                }
                self.index = 0;
            } else {
                self.index += 1;
            }
            due += scaled(delay_of(self.index));
            steps += 1;
            // 落后太多（窗口被挡住、机器卡顿）：从现在重新计时，不追帧
            if steps > self.count {
                due = now + scaled(delay_of(self.index));
                break;
            }
        }
        self.due = Some(due);
        Some(due - now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Duration = Duration::from_millis(100);

    #[test]
    fn plays_then_stops_on_last_frame() {
        let mut p = Playback::new(3, Some(1), true, false);
        let t0 = Instant::now();
        assert_eq!(p.tick(t0, |_| D), Some(D));
        p.tick(t0 + D, |_| D);
        assert_eq!(p.index, 1);
        p.tick(t0 + D * 2, |_| D);
        assert_eq!(p.index, 2);
        assert_eq!(p.tick(t0 + D * 3, |_| D), None);
        assert_eq!((p.index, p.playing), (2, false));
        p.toggle();
        assert!(p.playing && p.index == 0);
    }

    #[test]
    fn no_drift_and_no_catch_up() {
        let mut p = Playback::new(4, None, true, false);
        let t0 = Instant::now();
        p.tick(t0, |_| D);
        // 晚了 30 ms 才醒：下一帧仍按原定时间
        assert_eq!(p.tick(t0 + D + Duration::from_millis(30), |_| D), Some(Duration::from_millis(70)));
        // 落后很多：不追帧
        p.tick(t0 + D * 100, |_| D);
        assert!(p.playing);
        assert_eq!(p.tick(t0 + D * 100, |_| D), Some(D));
    }

    #[test]
    fn buffering_does_not_skip_slow_frames_or_change_pause_state() {
        let mut p = Playback::new(6, None, true, false);
        let t = Instant::now();
        p.tick(t, |_| D);
        p.tick(t + D, |_| D);
        assert_eq!(p.index, 1);
        p.buffering();
        assert_eq!(p.tick(t + D * 100, |_| D), Some(D));
        assert_eq!(p.index, 1);
        p.step(1);
        p.buffering();
        assert!(!p.playing);
    }

    #[test]
    fn waking_a_little_early_still_changes_frame() {
        let mut p = Playback::new(4, None, true, false);
        let t0 = Instant::now();
        p.tick(t0, |_| D);
        // 早 1 ms 醒：算到点，下一帧仍按原定时间（不累积误差）
        assert_eq!(p.tick(t0 + D - Duration::from_millis(1), |_| D), Some(D + Duration::from_millis(1)));
        assert_eq!(p.index, 1);
        // 早 5 ms：还不到，接着等
        assert_eq!(p.tick(t0 + D * 2 - Duration::from_millis(5), |_| D), Some(Duration::from_millis(5)));
        assert_eq!(p.index, 1);
    }

    #[test]
    fn step_pauses_and_wraps() {
        let mut p = Playback::new(3, None, true, false);
        p.step(-1);
        assert_eq!((p.index, p.playing), (2, false));
        assert!(Playback::new(1, None, true, false).tick(Instant::now(), |_| D).is_none());
    }
}
