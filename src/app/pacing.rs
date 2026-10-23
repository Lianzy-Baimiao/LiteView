//! 省电模式：连续画面（拖动、缩放、动图、淡入淡出）最多画多快。静止时 egui 本来就不画（不加定时重绘）。
//! 自动：高刷新率屏按刷新周期的整数倍画（144 Hz → 72、160 Hz → 80），帧间隔均匀，不低于 60 帧；
//! 60 Hz：最多 60 帧；不限：跟显示器。按理想时间线排下一帧（不按实际开始时间），不会越睡越慢。

use std::time::{Duration, Instant};

use crate::settings::FrameCap;

/// 两帧之间至少隔多久；None = 不限
pub fn interval(cap: FrameCap, refresh_hz: Option<f32>) -> Option<Duration> {
    let secs = match (cap, refresh_hz) {
        (FrameCap::Unlimited, _) => return None,
        // 刷新率本来就不超过 60：不用限（再限只会因为计时误差偶尔错过一次刷新）
        (FrameCap::Hz60, Some(hz)) if hz <= 61.0 => return None,
        (FrameCap::Hz60, _) => 1.0 / 60.0,
        (FrameCap::Auto, Some(hz)) => {
            let k = (hz / 60.0).floor().max(1.0);
            if k < 2.0 {
                return None;
            }
            k / hz
        }
        // 不知道刷新率（非 Windows）：不猜
        (FrameCap::Auto, None) => return None,
    };
    Some(Duration::from_secs_f64(f64::from(secs)))
}

/// 这一帧画之前要睡多久、下一帧最早什么时候。due = 上一帧排好的这一帧的时间
fn schedule(due: Option<Instant>, now: Instant, interval: Duration) -> (Duration, Instant) {
    match due {
        // 连续画：早了就等到排好的时间，接着排下一帧
        Some(d) if now < d => (d - now, d + interval),
        // 稍晚（仍在这一格里）：不等，沿原来的时间线排，节奏不乱
        Some(d) if now < d + interval => (Duration::ZERO, d + interval),
        // 刚从静止醒来（或很慢的内容）：马上画，从现在重新排
        _ => (Duration::ZERO, now + interval),
    }
}

#[derive(Default)]
pub struct Pacer {
    due: Option<Instant>,
    refresh: Option<(Instant, Option<f32>)>,
}

impl Pacer {
    /// 每帧画完界面、交给显卡之前调用；refresh 取显示器刷新率（每秒最多查一次）
    pub fn pace(&mut self, cap: FrameCap, refresh: impl FnOnce() -> Option<f32>) {
        let now = Instant::now();
        if self.refresh.is_none_or(|(at, _)| now.duration_since(at) > Duration::from_secs(1)) {
            self.refresh = Some((now, refresh()));
        }
        let Some(step) = interval(cap, self.refresh.and_then(|(_, hz)| hz)) else {
            self.due = None;
            return;
        };
        let (sleep, next) = schedule(self.due, now, step);
        if !sleep.is_zero() {
            std::thread::sleep(sleep);
        }
        self.due = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_uses_even_divisions_of_high_refresh_rates() {
        let ms = |d: Option<Duration>| d.map(|d| (d.as_secs_f64() * 1000.0 * 100.0).round() / 100.0);
        assert_eq!(ms(interval(FrameCap::Auto, Some(144.0))), Some(13.89));
        assert_eq!(ms(interval(FrameCap::Auto, Some(160.0))), Some(12.5));
        assert_eq!(ms(interval(FrameCap::Auto, Some(120.0))), Some(16.67));
        assert_eq!(ms(interval(FrameCap::Auto, Some(240.0))), Some(16.67));
        // 60 / 75 / 100 Hz：分频后不到 60，不限
        for hz in [60.0, 75.0, 100.0] {
            assert_eq!(interval(FrameCap::Auto, Some(hz)), None);
        }
        assert_eq!(interval(FrameCap::Auto, None), None);
        assert_eq!(ms(interval(FrameCap::Hz60, Some(144.0))), Some(16.67));
        assert_eq!(interval(FrameCap::Hz60, Some(60.0)), None);
        assert_eq!(interval(FrameCap::Unlimited, Some(144.0)), None);
    }

    #[test]
    fn continuous_frames_keep_an_even_cadence_and_idle_wakes_immediately() {
        let step = Duration::from_millis(10);
        let t0 = Instant::now();
        // 从静止醒来：不等
        let (sleep, due) = schedule(None, t0, step);
        assert_eq!((sleep, due), (Duration::ZERO, t0 + step));
        // 连续画、比排好的早 3 ms：等 3 ms，下一帧再往后排一格
        let (sleep, next) = schedule(Some(due), t0 + Duration::from_millis(7), step);
        assert_eq!((sleep, next), (Duration::from_millis(3), t0 + step * 2));
        // 晚了 4 ms（还在这一格里）：不等，也不把时间线往后推
        let (sleep, next) = schedule(Some(next), t0 + Duration::from_millis(24), step);
        assert_eq!((sleep, next), (Duration::ZERO, t0 + step * 3));
        // 很久没画（静止后又动）：马上画，从现在重新排
        let late = t0 + Duration::from_secs(5);
        assert_eq!(schedule(Some(next), late, step), (Duration::ZERO, late + step));
    }
}
