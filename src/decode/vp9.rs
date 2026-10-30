//! Windows：WebM 里的 VP8 / VP9 交给系统的视频解码器（Media Foundation 的 MFT；VP9 来自「VP9 视频扩展」，
//! Windows 11 一般自带）。表情包的透明通道是另一条同样大小的码流，用第二个解码器解，它的亮度就是不透明度。
//! 解码器出来的是 NV12（亮度一层、色度隔行交错一层），这里转成 RGBA。

use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use image::RgbaImage;
use rayon::prelude::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::Interface;

use super::webm::{Codec, Colour, Webm};
use super::{AnimFormat, AnimSource, Content, Decoded, Error, Frame, Info, Kind, Options};

thread_local! {
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

/// 这个线程上 COM（MTA）和 Media Foundation 准备好了
fn start() -> bool {
    STARTED.with(|s| {
        if !s.get() {
            unsafe {
                let com = CoInitializeEx(None, COINIT_MULTITHREADED);
                s.set(com.is_ok() && MFStartup(MF_VERSION, MFSTARTUP_LITE).is_ok());
            }
        }
        s.get()
    })
}

/// 解出来的一帧（NV12）：只留看得见的 w × h
pub struct Planes {
    pub w: usize,
    pub h: usize,
    /// 亮度，w × h
    pub y: Vec<u8>,
    /// 色度 U V 交错，(w / 2 向上取整) × 2 字节一行，共 (h / 2 向上取整) 行
    pub uv: Vec<u8>,
}

struct Decoder {
    mft: IMFTransform,
    /// 看得见的大小（容器里写的）
    w: u32,
    h: u32,
    /// 解码器给的整帧大小（可能对齐得比看得见的大）
    frame: (u32, u32),
    /// 第几帧（时间戳用）
    n: i64,
    out: Vec<Planes>,
}

impl Decoder {
    /// 找系统的 VP8 / VP9 解码器；没装扩展时 None
    fn new(codec: Codec, w: u32, h: u32) -> Option<Self> {
        if !start() {
            return None;
        }
        let sub = match codec {
            Codec::Vp9 => MFVideoFormat_VP90,
            Codec::Vp8 => MFVideoFormat_VP80,
        };
        unsafe {
            let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: sub };
            let (mut list, mut count) = (std::ptr::null_mut::<Option<IMFActivate>>(), 0u32);
            MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_LOCALMFT | MFT_ENUM_FLAG_SORTANDFILTER, Some(&input), None, &mut list, &mut count).ok()?;
            let activates: Vec<Option<IMFActivate>> = (0..count as usize).map(|i| std::ptr::read(list.add(i))).collect();
            CoTaskMemFree(Some(list as *const _));
            for a in activates.iter().flatten() {
                let Ok(mft) = a.ActivateObject::<IMFTransform>() else { continue };
                if let Some(d) = Self::setup(mft, sub, w, h) {
                    return Some(d);
                }
            }
            None
        }
    }

    unsafe fn setup(mft: IMFTransform, sub: windows::core::GUID, w: u32, h: u32) -> Option<Self> {
        unsafe {
            let t = MFCreateMediaType().ok()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            t.SetGUID(&MF_MT_SUBTYPE, &sub).ok()?;
            t.SetUINT64(&MF_MT_FRAME_SIZE, (u64::from(w) << 32) | u64::from(h)).ok()?;
            mft.SetInputType(0, &t, 0).ok()?;
            let mut d = Self { mft, w, h, frame: (w, h), n: 0, out: Vec::new() };
            d.set_output()?;
            d.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).ok()?;
            d.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).ok()?;
            Some(d)
        }
    }

    /// 输出选 NV12；记下解码器给的整帧大小
    fn set_output(&mut self) -> Option<()> {
        unsafe {
            for i in 0.. {
                let t = self.mft.GetOutputAvailableType(0, i).ok()?;
                if t.GetGUID(&MF_MT_SUBTYPE).ok()? == MFVideoFormat_NV12 {
                    self.mft.SetOutputType(0, &t, 0).ok()?;
                    if let Ok(s) = t.GetUINT64(&MF_MT_FRAME_SIZE) {
                        self.frame = ((s >> 32) as u32, s as u32);
                    }
                    return Some(());
                }
            }
        }
        None
    }

    /// 送进一帧的码流，取出已经解好的
    fn push(&mut self, data: &[u8]) -> Result<(), Error> {
        let fail = |e: windows::core::Error| Error::Corrupt(format!("VP9 decoder: {}", e.message()));
        unsafe {
            let buf = MFCreateMemoryBuffer(data.len() as u32).map_err(fail)?;
            let mut p = std::ptr::null_mut();
            buf.Lock(&mut p, None, None).map_err(fail)?;
            std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
            buf.Unlock().map_err(fail)?;
            buf.SetCurrentLength(data.len() as u32).map_err(fail)?;
            let s = MFCreateSample().map_err(fail)?;
            s.AddBuffer(&buf).map_err(fail)?;
            // 100 纳秒为单位；只用来让解码器知道先后
            s.SetSampleTime(self.n * 333_333).map_err(fail)?;
            s.SetSampleDuration(333_333).map_err(fail)?;
            self.n += 1;
            loop {
                match self.mft.ProcessInput(0, &s, 0) {
                    Ok(()) => break,
                    // 还有解好的没取：先取再送
                    Err(e) if e.code() == MF_E_NOTACCEPTING => self.pull()?,
                    Err(e) => return Err(fail(e)),
                }
            }
        }
        self.pull()
    }

    /// 送完了：把解码器里剩下的都取出来
    fn finish(&mut self) -> Result<(), Error> {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
        }
        self.pull()
    }

    fn pull(&mut self) -> Result<(), Error> {
        let fail = |e: windows::core::Error| Error::Corrupt(format!("VP9 decoder: {}", e.message()));
        loop {
            unsafe {
                let info = self.mft.GetOutputStreamInfo(0).map_err(fail)?;
                let provides = info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32) != 0;
                let sample = if provides {
                    None
                } else {
                    let s = MFCreateSample().map_err(fail)?;
                    let size = info.cbSize.max(self.frame.0 * self.frame.1 * 3 / 2);
                    s.AddBuffer(&MFCreateMemoryBuffer(size).map_err(fail)?).map_err(fail)?;
                    Some(s)
                };
                let mut out = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: std::mem::ManuallyDrop::new(sample), dwStatus: 0, pEvents: std::mem::ManuallyDrop::new(None) }];
                let mut status = 0u32;
                let r = self.mft.ProcessOutput(0, &mut out, &mut status);
                let sample = std::mem::ManuallyDrop::take(&mut out[0].pSample);
                std::mem::ManuallyDrop::drop(&mut out[0].pEvents);
                match r {
                    Ok(()) => {
                        if let Some(s) = sample {
                            let planes = self.read(&s).map_err(fail)?;
                            self.out.push(planes);
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                    // 第一帧后解码器知道了真正的大小：重新选输出格式
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                        self.set_output().ok_or_else(|| Error::Corrupt("VP9 decoder: no NV12 output".into()))?;
                    }
                    Err(e) => return Err(fail(e)),
                }
            }
        }
    }

    /// 从解码器给的 NV12 里取出看得见的那块
    unsafe fn read(&self, s: &IMFSample) -> windows::core::Result<Planes> {
        unsafe {
            let buf = s.ConvertToContiguousBuffer()?;
            let (w, h) = (self.w.min(self.frame.0) as usize, self.h.min(self.frame.1) as usize);
            let fh = self.frame.1 as usize;
            let (mut p, mut pitch) = (std::ptr::null_mut(), 0i32);
            let two_d = buf.cast::<IMF2DBuffer>().ok();
            let mut len = 0u32;
            match &two_d {
                Some(b) => b.Lock2D(&mut p, &mut pitch)?,
                None => buf.Lock(&mut p, None, Some(&mut len))?,
            }
            let stride = if pitch > 0 { pitch as usize } else { self.frame.0 as usize };
            let total = if two_d.is_some() { stride * fh * 3 / 2 } else { len as usize };
            let data = std::slice::from_raw_parts(p, total);
            let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
            let mut planes = Planes { w, h, y: Vec::with_capacity(w * h), uv: Vec::with_capacity(cw * 2 * ch) };
            let ok = stride * fh * 3 / 2 <= data.len();
            if ok {
                for r in 0..h {
                    planes.y.extend_from_slice(&data[r * stride..r * stride + w]);
                }
                let base = stride * fh;
                for r in 0..ch {
                    planes.uv.extend_from_slice(&data[base + r * stride..base + r * stride + cw * 2]);
                }
            }
            match &two_d {
                Some(b) => b.Unlock2D()?,
                None => buf.Unlock()?,
            }
            if !ok {
                return Err(windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_007A_u32 as i32)));
            }
            Ok(planes)
        }
    }
}

/// YUV（NV12）+ 透明（另一帧的亮度）→ RGBA（不预乘）
pub fn to_rgba(c: &Planes, alpha: Option<&Planes>, colour: Colour) -> RgbaImage {
    // (Kr, Kb)：BT.601 / BT.709
    let (kr, kb) = if colour.bt709 { (0.2126f32, 0.0722f32) } else { (0.299, 0.114) };
    let kg = 1.0 - kr - kb;
    let (ys, cs, y0) = if colour.full_range { (1.0f32, 1.0f32, 0.0f32) } else { (255.0 / 219.0, 255.0 / 224.0, 16.0) };
    let (rv, gu, gv, bu) = (2.0 * (1.0 - kr) * cs, 2.0 * (1.0 - kb) * kb / kg * cs, 2.0 * (1.0 - kr) * kr / kg * cs, 2.0 * (1.0 - kb) * cs);
    let (w, h) = (c.w, c.h);
    let alpha = alpha.filter(|a| a.w == w && a.h == h);
    let mut out = vec![0u8; w * h * 4];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(r, line)| {
        let uv = &c.uv[(r / 2) * w.div_ceil(2) * 2..];
        for x in 0..w {
            let yv = (f32::from(c.y[r * w + x]) - y0) * ys;
            let (u, v) = (f32::from(uv[(x / 2) * 2]) - 128.0, f32::from(uv[(x / 2) * 2 + 1]) - 128.0);
            let px = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            let a = alpha.map_or(255, |a| a.y[r * w + x]);
            line[x * 4..x * 4 + 4].copy_from_slice(&[px(yv + rv * v), px(yv - gu * u - gv * v), px(yv + bu * u), a]);
        }
    });
    RgbaImage::from_raw(w as u32, h as u32, out).expect("buffer size")
}

/// 从头按顺序解出每一帧（f 返回 false 时停）。透明通道缺了某一帧时从那以后都不透明（码流断了接不上）
pub fn each_frame(w: &Webm, mut f: impl FnMut(usize, RgbaImage) -> bool) -> Result<(), Error> {
    let mut color = Decoder::new(w.codec, w.width, w.height).ok_or(Error::MissingCodec(Kind::Webm))?;
    let mut alpha = if w.alpha && w.frames.first().is_some_and(|fr| fr.alpha.is_some()) { Decoder::new(w.codec, w.width, w.height) } else { None };
    let mut done = 0usize;
    let mut emit = |color: &mut Decoder, alpha: &mut Option<Decoder>, done: &mut usize, last: bool| -> bool {
        while !color.out.is_empty() {
            let a = match alpha {
                Some(a) if a.out.is_empty() && !last => return true,
                Some(a) if !a.out.is_empty() => Some(a.out.remove(0)),
                _ => None,
            };
            let c = color.out.remove(0);
            let i = *done;
            *done += 1;
            if !f(i, to_rgba(&c, a.as_ref(), w.colour)) {
                return false;
            }
        }
        true
    };
    for fr in &w.frames {
        color.push(fr.data)?;
        match (&mut alpha, fr.alpha) {
            (Some(a), Some(data)) => {
                if a.push(data).is_err() {
                    alpha = None;
                }
            }
            (Some(_), None) => alpha = None,
            _ => {}
        }
        if !emit(&mut color, &mut alpha, &mut done, false) {
            return Ok(());
        }
    }
    color.finish()?;
    if let Some(a) = &mut alpha
        && a.finish().is_err()
    {
        alpha = None;
    }
    emit(&mut color, &mut alpha, &mut done, true);
    if done == 0 {
        return Err(Error::Corrupt("VP9: no frames decoded".into()));
    }
    Ok(())
}

/// 这么大以上的 WebM 当视频，不打开：看图时整个文件要读进内存，翻文件夹时还会预读
const MAX_FILE: usize = 64 << 20;

/// 整个解码：先解进缓存（放不下时后面的边播边解），只有一帧时当静态图
pub fn decode(data: &[u8], opts: &Options) -> Result<Decoded, Error> {
    if data.len() > MAX_FILE {
        return Err(Error::TooLarge { width: 0, height: 0 });
    }
    let w = super::webm::parse(data)?;
    let working = opts.check_working(w.width, w.height, 12)?;
    let cache = opts.anim_cache.min((opts.max_alloc - working) as usize);
    let delays = w.delays();
    let mut frames: Vec<Frame> = Vec::new();
    let (mut bytes, mut truncated) = (0usize, false);
    each_frame(&w, |i, img| {
        let size = img.len();
        if !frames.is_empty() && bytes + size > cache {
            truncated = true;
            return false;
        }
        bytes += size;
        frames.push(Frame { image: Arc::new(img), delay: delays.get(i).copied().unwrap_or(Duration::from_millis(33)) });
        true
    })?;
    let first = &frames[0].image;
    let info = Info { format: "WebM", width: first.width(), height: first.height(), bits: 8, channels: 4, has_alpha: w.alpha, file_size: 0, jpeg_quality: None, exif: None, icc: None };
    let content = if frames.len() == 1 && !truncated {
        Content::Still { image: frames.pop().expect("one frame").image, levels: Vec::new() }
    } else if truncated {
        let source = Some(Arc::new(AnimSource { data: data.into(), format: AnimFormat::Webm, options: *opts }));
        Content::Animation { frames, delays, plays: None, source }
    } else {
        let delays = frames.iter().map(|f| f.delay).collect();
        Content::Animation { frames, delays, plays: None, source: None }
    };
    Ok(Decoded { info, content })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 带透明的 VP9 WebM（tests/fixtures/alpha.webm：64 × 48、6 帧、每帧 100 ms，红色方块从左往右移，四周全透明）：
    /// 帧数、时间、方块的位置和颜色、四周透明。没装 VP9 视频扩展的机器（CI）解不了，跳过
    #[test]
    fn vp9_webm_with_alpha_decodes_frames_colour_and_transparency() {
        let data = include_bytes!("../../tests/fixtures/alpha.webm");
        let d = match super::super::decode_bytes(data, "webm", &Options::default()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skipped: {e:?}");
                return;
            }
        };
        assert_eq!((d.info.format, d.info.width, d.info.height, d.info.has_alpha), ("WebM", 64, 48, true));
        let Content::Animation { frames, delays, .. } = &d.content else { panic!("animation") };
        assert_eq!(frames.len(), 6);
        assert!(delays.iter().all(|t| t.as_millis() == 100), "{delays:?}");
        // 第 i 帧方块左边在 x = 4 + 5i，宽 20、上下 12 ~ 32
        for (i, f) in frames.iter().enumerate() {
            let img = &f.image;
            let x = 4 + 5 * i as u32 + 10;
            let p = img.get_pixel(x, 22).0;
            assert!(p[0] > 200 && p[1] < 60 && p[2] < 60 && p[3] > 240, "frame {i} square at x {x}: {p:?}");
            assert!(img.get_pixel(1, 1).0[3] < 16 && img.get_pixel(62, 44).0[3] < 16, "frame {i} corners transparent");
        }
    }
}
