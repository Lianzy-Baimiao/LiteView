//! 按需解码未缓存的动图帧。只有一个后台解码器、一个最新请求和一个结果槽，
//! 不积压滑块请求；顺播复用解码器，回退/循环从头恢复合成状态。离开图片立即取消。

use std::sync::{Arc, Condvar, Mutex};

use image::RgbaImage;

use crate::decode::{AnimSource, Content, Decoded, Error};

type FrameResult = Result<Arc<RgbaImage>, Error>;

#[derive(Default)]
struct State {
    wanted: Option<usize>,
    result: Option<(usize, FrameResult)>,
    stopped: bool,
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
                    while !s.stopped && (s.wanted.is_none() || s.result.as_ref().map(|r| r.0) == s.wanted) {
                        s = worker.changed.wait(s).unwrap();
                    }
                    if s.stopped {
                        return;
                    }
                }
                let mut interrupted = false;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    source.each_frame(|i, image| {
                        let mut s = worker.state.lock().unwrap();
                        while !s.stopped && (s.wanted.is_none() || s.result.as_ref().map(|r| r.0) == s.wanted) {
                            s = worker.changed.wait(s).unwrap();
                        }
                        if s.stopped {
                            interrupted = true;
                            return false;
                        }
                        let wanted = s.wanted.expect("requested frame");
                        if wanted < i {
                            interrupted = true;
                            return false;
                        } // 回退：重建解码器与处置/混合状态
                        if wanted == i {
                            s.result = Some((i, Ok(Arc::new(image))));
                            drop(s);
                            wake();
                        }
                        true
                    })
                }));
                let error = match result {
                    Ok(Ok(())) if interrupted => None,
                    Ok(Ok(())) => {
                        let s = worker.state.lock().unwrap();
                        (s.wanted.is_some() && s.result.as_ref().map(|r| r.0) != s.wanted)
                            .then(|| Error::Corrupt("animation ended before requested frame".into()))
                    }
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some(Error::Corrupt("animation decoder panicked".into())),
                };
                if let Some(e) = error {
                    let mut s = worker.state.lock().unwrap();
                    if let Some(i) = s.wanted {
                        s.result = Some((i, Err(e)));
                        drop(s);
                        wake();
                    }
                }
            }
        });
        Self { shared }
    }

    fn get(&self, index: usize) -> Result<Option<Arc<RgbaImage>>, Error> {
        let mut s = self.shared.state.lock().unwrap();
        if s.wanted != Some(index) {
            s.wanted = Some(index);
            self.shared.changed.notify_one();
        }
        match &s.result {
            Some((i, result)) if *i == index => result.clone().map(Some),
            _ => Ok(None),
        }
    }

    fn idle(&self) {
        self.shared.state.lock().unwrap().wanted = None;
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

    pub fn request(&mut self, index: usize) -> Result<bool, Error> {
        let Content::Animation { frames, delays, .. } = &self.decoded.content else { return Ok(true) };
        if index >= delays.len() {
            return Err(Error::Corrupt("frame index out of range".into()));
        }
        if let Some(frame) = frames.get(index) {
            self.displayed = (index, frame.image.clone());
            if let Some(stream) = &self.stream {
                stream.idle();
            }
            return Ok(true);
        }
        if self.failed == Some(index) {
            return Ok(false);
        }
        self.failed = None;
        if let Some(stream) = &self.stream {
            let result = match stream.get(index) {
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
        while !frames.request(index).unwrap() {
            assert!(Instant::now() < deadline, "frame {index} timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        let (shown, image) = frames.displayed();
        assert_eq!(shown, index);
        assert_eq!(image.get_pixel(0, 0).0[0], index as u8 * 40);
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
    fn apng_webp_and_apng_poster_match_full_decode_beyond_cache() {
        for (data, ext) in [
            (&include_bytes!("../../tests/fixtures/animation.png")[..], "png"),
            (&include_bytes!("../../tests/fixtures/poster.png")[..], "png"),
            (&include_bytes!("../../tests/fixtures/animation.webp")[..], "webp"),
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
                while !streaming.request(index).unwrap() {
                    assert!(Instant::now() < deadline, "{ext} frame {index} timed out");
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(streaming.displayed().1, expected[index].image.as_ref());
                assert_eq!(small.content.image_at(index).unwrap().as_ref(), expected[index].image.as_ref());
            }
        }
    }

    #[test]
    fn rapid_scrubbing_keeps_only_latest_request() {
        let mut frames = AnimationFrames::new(animation(), egui::Context::default());
        for i in [5, 1, 4, 2, 5, 3] {
            let _ = frames.request(i);
        }
        wait_frame(&mut frames, 3);
        assert!(frames.request(6).is_err());
    }

    #[test]
    fn missing_frame_reports_error_instead_of_retrying_forever() {
        let Content::Animation { source: Some(source), .. } = &animation().content else { panic!() };
        let stream = FrameStream::new(source.clone(), || {});
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match stream.get(6) {
                Err(Error::Corrupt(_)) => break,
                Ok(None) => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                _ => panic!("expected missing frame error"),
            }
        }
        // 错误不阻止重新选择一个有效帧。
        while stream.get(2).unwrap().is_none() {
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
