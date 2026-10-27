//! 按需解码未缓存的动图帧。只有一个后台解码器，解好的帧放在一个小窗口里：播放时从界面要的帧
//! （在播缓存帧时是第一张未缓存的帧）起往后 AHEAD 帧，帧到点时已经解好，换帧不用等解码；
//! 暂停时只解界面要的那一帧，停在缓存帧上时什么都不解。
//! 解码器只能往前走：窗口里有已经错过的帧（回退、循环）时从头恢复合成状态。离开图片立即取消。

use std::ops::Range;
use std::sync::{Arc, Condvar, Mutex};

use image::RgbaImage;

use crate::decode::{AnimSource, Content, Decoded, Error};

/// 播放时的窗口帧数（含界面正要的那一帧）：播一帧时后台解下一帧。在播缓存帧时先解好缓存后面的帧，
/// 播到缓存边界不用再从第 0 帧解一遍（1080p、256 MB 缓存时约 0.3 s 的卡顿）。
const AHEAD: usize = 2;

#[derive(Default)]
struct State {
    /// 要解好的帧
    window: Range<usize>,
    /// 界面在等窗口的第一帧（不是在播缓存帧时提前解的帧）：解好时叫醒界面
    waiting: bool,
    /// 解好的帧，都在窗口里
    ready: Vec<(usize, Arc<RgbaImage>)>,
    /// 解码在这一帧出错，或动图到这一帧就结束了：它和后面的帧都拿不到
    failed: Option<(usize, Error)>,
    stopped: bool,
}

impl State {
    /// 窗口里还没解出来的帧
    fn missing(&self) -> impl Iterator<Item = usize> + '_ {
        let end = self.failed.as_ref().map_or(usize::MAX, |f| f.0);
        self.window.clone().filter(move |&i| i < end && !self.ready.iter().any(|r| r.0 == i))
    }

    /// 换窗口，扔掉窗口外的帧；返回是否换了
    fn set(&mut self, window: Range<usize>, waiting: bool) -> bool {
        self.waiting = waiting;
        if self.window == window {
            return false;
        }
        self.ready.retain(|r| window.contains(&r.0));
        self.window = window;
        true
    }
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

struct FrameStream {
    shared: Arc<Shared>,
}

impl FrameStream {
    fn new(source: Arc<AnimSource>, wake: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new(Shared { state: Mutex::new(State::default()), changed: Condvar::new() });
        let worker = shared.clone();
        std::thread::spawn(move || {
            loop {
                {
                    let mut s = worker.state.lock().unwrap();
                    while !s.stopped && s.missing().next().is_none() {
                        s = worker.changed.wait(s).unwrap();
                    }
                    if s.stopped {
                        return;
                    }
                }
                // 这一轮要解的下一帧；解码出错时它就是坏掉（或不存在）的那一帧
                let (mut next, mut interrupted) = (0, false);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    source.each_frame(|i, image| {
                        next = i + 1;
                        let mut s = worker.state.lock().unwrap();
                        if s.stopped || s.missing().any(|m| m < i) {
                            interrupted = true;
                            return false;
                        }
                        if s.window.contains(&i) && !s.ready.iter().any(|r| r.0 == i) {
                            s.ready.push((i, Arc::new(image)));
                            if s.waiting && s.window.start == i {
                                drop(s);
                                wake();
                                s = worker.state.lock().unwrap();
                            }
                        }
                        // 窗口满了先停在这里，等界面往前播再解下一帧，不多压一张解好的帧
                        while !s.stopped && !s.window.is_empty() && s.missing().next().is_none() {
                            s = worker.changed.wait(s).unwrap();
                        }
                        // 回退、循环：窗口里有这一轮已经错过的帧，从头再解；停在缓存帧上（窗口为空）：
                        // 这一轮结束，不留着解码器
                        interrupted = s.stopped || s.window.is_empty() || s.missing().any(|m| m <= i);
                        !interrupted
                    })
                }));
                let error = match result {
                    Ok(Ok(())) if interrupted => None,
                    Ok(Ok(())) => Some(Error::Corrupt("animation ended before requested frame".into())),
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some(Error::Corrupt("animation decoder panicked".into())),
                };
                if let Some(e) = error {
                    let mut s = worker.state.lock().unwrap();
                    let waiting = s.waiting && s.window.start >= next;
                    s.failed = Some((next, e));
                    drop(s);
                    if waiting {
                        wake();
                    }
                }
            }
        });
        Self { shared }
    }

    /// 界面要第 index 帧（未缓存）：播放时顺带往后解，暂停时只解这一帧
    fn get(&self, index: usize, playing: bool) -> Result<Option<Arc<RgbaImage>>, Error> {
        let mut s = self.shared.state.lock().unwrap();
        if s.set(index..index + if playing { AHEAD } else { 1 }, true) {
            self.shared.changed.notify_one();
        }
        if let Some((at, e)) = &s.failed
            && index >= *at
        {
            return Err(e.clone());
        }
        Ok(s.ready.iter().find(|r| r.0 == index).map(|r| r.1.clone()))
    }

    /// 界面在显示缓存帧（first 是第一张未缓存的帧）：播放时先把 first 起的帧解好，暂停时不解
    fn ahead(&self, first: usize, playing: bool) {
        let mut s = self.shared.state.lock().unwrap();
        if s.set(first..first + if playing { AHEAD } else { 0 }, false) {
            self.shared.changed.notify_one();
        }
    }
}

impl Drop for FrameStream {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().stopped = true;
        self.shared.changed.notify_one();
    }
}

/// 界面只请求帧、读取最后一张已就绪的帧，不在 UI 线程解码或等待。
pub struct AnimationFrames {
    decoded: Arc<Decoded>,
    stream: Option<FrameStream>,
    displayed: (usize, Arc<RgbaImage>),
    failed: Option<usize>,
}

impl AnimationFrames {
    pub fn new(decoded: Arc<Decoded>, ctx: egui::Context) -> Self {
        let source = match &decoded.content {
            Content::Animation { source, .. } => source.clone(),
            _ => None,
        };
        let displayed = (0, decoded.first_image().clone());
        Self { decoded, stream: source.map(|s| FrameStream::new(s, move || ctx.request_repaint())), displayed, failed: None }
    }

    /// playing：在播放，后台提前解后面的帧；暂停时只解要显示的这一帧
    pub fn request(&mut self, index: usize, playing: bool) -> Result<bool, Error> {
        let Content::Animation { frames, delays, .. } = &self.decoded.content else { return Ok(true) };
        if index >= delays.len() {
            return Err(Error::Corrupt("frame index out of range".into()));
        }
        if let Some(frame) = frames.get(index) {
            self.displayed = (index, frame.image.clone());
            if let Some(stream) = &self.stream {
                stream.ahead(frames.len(), playing);
            }
            return Ok(true);
        }
        if self.failed == Some(index) {
            return Ok(false);
        }
        self.failed = None;
        if let Some(stream) = &self.stream {
            let result = match stream.get(index, playing) {
                Ok(result) => result,
                Err(e) => {
                    self.failed = Some(index);
                    return Err(e);
                }
            };
            if let Some(image) = result {
                self.displayed = (index, image);
                return Ok(true);
            }
            return Ok(false);
        }
        Err(Error::Corrupt("animation frame unavailable".into()))
    }

    pub fn displayed(&self) -> (usize, &RgbaImage) {
        (self.displayed.0, &self.displayed.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{Options, decode_bytes};
    use image::{
        Delay, Frame, Rgba,
        codecs::gif::{GifEncoder, Repeat},
    };
    use std::time::{Duration, Instant};

    fn animation() -> Arc<Decoded> {
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            encoder.set_repeat(Repeat::Infinite).unwrap();
            for i in 0..6 {
                let image = RgbaImage::from_pixel(4, 3, Rgba([i * 40, 0, 0, 255]));
                encoder.encode_frame(Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(40, 1))).unwrap();
            }
        }
        Arc::new(decode_bytes(&bytes, "gif", &Options { anim_cache: 48, ..Options::default() }).unwrap())
    }

    fn wait_frame(frames: &mut AnimationFrames, index: usize) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !frames.request(index, true).unwrap() {
            assert!(Instant::now() < deadline, "frame {index} timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        let (shown, image) = frames.displayed();
        assert_eq!(shown, index);
        assert_eq!(image.get_pixel(0, 0).0[0], index as u8 * 40);
    }

    /// 等后台把 index 帧解好（不经过界面请求）
    fn wait_ready(frames: &AnimationFrames, index: usize) {
        let shared = &frames.stream.as_ref().unwrap().shared;
        let deadline = Instant::now() + Duration::from_secs(3);
        while !shared.state.lock().unwrap().ready.iter().any(|r| r.0 == index) {
            assert!(Instant::now() < deadline, "frame {index} not decoded ahead");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn decodes_ahead_so_streamed_frames_are_ready_when_due() {
        let mut frames = AnimationFrames::new(animation(), egui::Context::default());
        // 还在播缓存帧：缓存后面的第一帧先解好，播到缓存边界不用等
        assert!(frames.request(0, true).unwrap());
        wait_ready(&frames, 1);
        for index in 1..6 {
            // 到点时这一帧已经解好，同时后台去解下一帧
            assert!(frames.request(index, true).unwrap(), "frame {index} was not ready when due");
            assert_eq!(frames.displayed().1.get_pixel(0, 0).0[0], index as u8 * 40);
            if index < 5 {
                wait_ready(&frames, index + 1);
            }
            assert!(frames.stream.as_ref().unwrap().shared.state.lock().unwrap().ready.len() <= AHEAD);
        }
        // 循环回第 0 帧（缓存帧）：后台从头再把第 1 帧解好
        assert!(frames.request(0, true).unwrap());
        wait_ready(&frames, 1);
        assert!(frames.request(1, true).unwrap());
    }

    #[test]
    fn paused_decodes_only_the_shown_frame() {
        let mut frames = AnimationFrames::new(animation(), egui::Context::default());
        let ready = |frames: &AnimationFrames| -> Vec<usize> {
            frames.stream.as_ref().unwrap().shared.state.lock().unwrap().ready.iter().map(|r| r.0).collect()
        };
        // 停在缓存帧上：什么都不解，不多占内存
        assert!(frames.request(0, false).unwrap());
        std::thread::sleep(Duration::from_millis(50));
        assert!(ready(&frames).is_empty());
        // 暂停时一帧一帧往后看：只解要显示的那一帧
        let deadline = Instant::now() + Duration::from_secs(3);
        while !frames.request(3, false).unwrap() {
            assert!(Instant::now() < deadline, "paused frame timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(frames.displayed().1.get_pixel(0, 0).0[0], 3 * 40);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(ready(&frames), [3]);
        // 接着播放：后台把下一帧解好
        assert!(frames.request(3, true).unwrap());
        wait_ready(&frames, 4);
        // 回到缓存帧上暂停：扔掉解好的帧
        assert!(frames.request(0, false).unwrap());
        assert!(ready(&frames).is_empty());
    }

    #[test]
    fn tiny_cache_plays_every_frame_seeks_backwards_and_wraps() {
        let decoded = animation();
        assert_eq!(decoded.content.frame_count(), 6);
        let Content::Animation { frames, .. } = &decoded.content else { panic!() };
        assert_eq!(frames.len(), 1);
        let mut frames = AnimationFrames::new(decoded.clone(), egui::Context::default());
        for index in [0, 1, 2, 3, 4, 5, 2, 0, 5, 1] {
            wait_frame(&mut frames, index);
            // 复制/导出与播放走相同的帧编号，绝不夹到最后一张缓存帧。
            assert_eq!(decoded.content.image_at(index).unwrap().get_pixel(0, 0).0[0], index as u8 * 40);
        }
    }

    #[test]
    fn apng_webp_jxl_and_apng_poster_match_full_decode_beyond_cache() {
        for (data, ext) in [
            (&include_bytes!("../../tests/fixtures/animation.png")[..], "png"),
            (&include_bytes!("../../tests/fixtures/poster.png")[..], "png"),
            (&include_bytes!("../../tests/fixtures/animation.webp")[..], "webp"),
            (&include_bytes!("../../tests/fixtures/animation.jxl")[..], "jxl"),
        ] {
            let full = decode_bytes(data, ext, &Options::default()).unwrap();
            let small = Arc::new(decode_bytes(data, ext, &Options { anim_cache: 8 * 6 * 4, ..Options::default() }).unwrap());
            let Content::Animation { frames: expected, delays, plays, .. } = &full.content else { panic!() };
            let Content::Animation { frames, delays: actual, plays: actual_plays, source, .. } = &small.content else { panic!() };
            assert_eq!(expected.len(), 6);
            assert_eq!(frames.len(), 1);
            assert!(source.is_some());
            assert_eq!(actual, delays);
            assert_eq!(actual_plays, plays);
            let mut streaming = AnimationFrames::new(small.clone(), egui::Context::default());
            for index in [0, 1, 2, 5, 3, 0, 4] {
                let deadline = Instant::now() + Duration::from_secs(3);
                // 暂停时逐帧查看
                while !streaming.request(index, false).unwrap() {
                    assert!(Instant::now() < deadline, "{ext} frame {index} timed out");
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(streaming.displayed().1, expected[index].image.as_ref());
                assert_eq!(small.content.image_at(index).unwrap().as_ref(), expected[index].image.as_ref());
            }
        }
    }

    #[test]
    fn lossless_jxl_animation_matches_original_apng_pixels_and_timeline() {
        let png = decode_bytes(include_bytes!("../../tests/fixtures/animation.png"), "png", &Options::default()).unwrap();
        let jxl = decode_bytes(include_bytes!("../../tests/fixtures/animation.jxl"), "jxl", &Options::default()).unwrap();
        let Content::Animation { frames: expected, delays, plays, .. } = &png.content else { panic!() };
        let Content::Animation { frames, delays: actual, plays: actual_plays, .. } = &jxl.content else { panic!() };
        assert_eq!(frames.len(), expected.len());
        assert_eq!(actual, delays);
        // cjxl 0.11.1 drops the APNG loop count (also verified with djxl).
        assert_eq!(*plays, Some(2));
        assert_eq!(*actual_plays, None);
        for (a, b) in frames.iter().zip(expected) {
            assert_eq!(a.image, b.image);
        }
    }

    #[test]
    fn rapid_scrubbing_keeps_only_latest_request() {
        let mut frames = AnimationFrames::new(animation(), egui::Context::default());
        for i in [5, 1, 4, 2, 5, 3] {
            let _ = frames.request(i, false);
        }
        wait_frame(&mut frames, 3);
        assert!(frames.request(6, true).is_err());
    }

    #[test]
    fn missing_frame_reports_error_instead_of_retrying_forever() {
        let Content::Animation { source: Some(source), .. } = &animation().content else { panic!() };
        let stream = FrameStream::new(source.clone(), || {});
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match stream.get(6, true) {
                Err(Error::Corrupt(_)) => break,
                Ok(None) => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                _ => panic!("expected missing frame error"),
            }
        }
        // 错误不阻止重新选择一个有效帧。
        while stream.get(2, true).unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn dropping_stream_cancels_idle_decoder() {
        let Content::Animation { source: Some(source), .. } = &animation().content else { panic!() };
        let stream = FrameStream::new(source.clone(), || {});
        let shared = stream.shared.clone();
        drop(stream);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Arc::strong_count(&shared) > 1 {
            assert!(Instant::now() < deadline, "decoder not cancelled");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
