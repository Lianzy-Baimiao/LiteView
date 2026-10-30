//! WebM（Matroska）容器：只取第一条视频轨的每一帧（VP8 / VP9 码流）和它附带的透明通道（BlockAdditional，ID 1），
//! 以及帧的时间。不解码，解码见 `vp9`（Windows 用系统的视频扩展）。表情包（Telegram 等）就是这种：VP9 + 透明。

use std::time::Duration;

use super::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Vp8,
    Vp9,
}

/// 颜色怎么从 YUV 转回来（容器的 Colour 元素；没有时按 BT.601、有限范围，VP9 也是这样默认）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    pub bt709: bool,
    pub full_range: bool,
}

#[derive(Debug)]
pub struct Frame<'a> {
    /// 显示时间（从 0 开始）
    pub time: Duration,
    pub data: &'a [u8],
    /// 透明通道（另一条同样大小的 VP8 / VP9 码流，亮度就是不透明度）
    pub alpha: Option<&'a [u8]>,
}

#[derive(Debug)]
pub struct Webm<'a> {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    /// 轨道标了带透明通道（AlphaMode = 1）
    pub alpha: bool,
    pub colour: Colour,
    /// 每帧的标称时长（DefaultDuration）
    pub frame_duration: Option<Duration>,
    pub frames: Vec<Frame<'a>>,
}

impl Webm<'_> {
    /// 每帧显示多久：下一帧的时间减这一帧的；最后一帧用标称时长（没有时用前一帧的，再没有 1/30 秒）
    pub fn delays(&self) -> Vec<Duration> {
        let n = self.frames.len();
        let fallback = self.frame_duration.unwrap_or(Duration::from_micros(33_333));
        (0..n)
            .map(|i| match self.frames.get(i + 1) {
                Some(next) => next.time.saturating_sub(self.frames[i].time),
                None => self.frame_duration.or_else(|| (i > 0).then(|| self.frames[i].time.saturating_sub(self.frames[i - 1].time))).unwrap_or(fallback),
            })
            .map(|d| if d.is_zero() { fallback } else { d })
            .collect()
    }
}

/// 文件头是 EBML、文档类型是 webm / matroska
pub fn is_webm(data: &[u8]) -> bool {
    data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) && {
        let head = &data[..data.len().min(64)];
        head.windows(4).any(|w| w == b"webm") || head.windows(8).any(|w| w == b"matroska")
    }
}

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TIMECODE_SCALE: u32 = 0x2A_D7B1;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const DEFAULT_DURATION: u32 = 0x23_E383;
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const ALPHA_MODE: u32 = 0x53C0;
const COLOUR: u32 = 0x55B0;
const MATRIX: u32 = 0x55B1;
const RANGE: u32 = 0x55B9;
const CLUSTER: u32 = 0x1F43_B675;
const CLUSTER_TIME: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const BLOCK_ADDITIONS: u32 = 0x75A1;
const BLOCK_MORE: u32 = 0xA6;
const BLOCK_ADD_ID: u32 = 0xEE;
const BLOCK_ADDITIONAL: u32 = 0xA5;
/// 这些出现在 Segment 下一层：大小未知的 Cluster 遇到它们就结束
const TOP: [u32; 7] = [CLUSTER, 0x1C53_BB6B, 0x1254_C367, 0x1941_A469, 0x1043_A770, INFO, TRACKS];

fn bad(what: &str) -> Error {
    Error::Corrupt(format!("WebM: {what}"))
}

/// 变长整数：(值, 字节数)；id = true 时保留长度标记位（元素 ID 的写法）
fn vint(d: &[u8], at: usize, id: bool) -> Option<(u64, usize)> {
    let b = *d.get(at)?;
    let n = b.leading_zeros() as usize + 1;
    if n > 8 || at + n > d.len() {
        return None;
    }
    let mut v = if id { u64::from(b) } else { u64::from(b) & (0xFF >> n) };
    for k in 1..n {
        v = (v << 8) | u64::from(d[at + k]);
    }
    Some((v, n))
}

/// 一个元素：ID、内容的范围；大小未知时内容到 end 为止（由调用方再切）
struct Element {
    id: u32,
    start: usize,
    end: usize,
    unknown: bool,
}

fn element(d: &[u8], at: usize, end: usize) -> Option<Element> {
    let (id, n) = vint(d, at, true)?;
    let (size, m) = vint(d, at + n, false)?;
    let start = at + n + m;
    let unknown = size == (1u64 << (7 * m)) - 1;
    // 文件截断时内容只到文件末尾（前面读到的帧照样能放）
    let stop = if unknown { end } else { start.saturating_add(usize::try_from(size).unwrap_or(usize::MAX)) };
    (start <= end).then_some(Element { id: u32::try_from(id).ok()?, start, end: stop.min(end), unknown })
}

fn uint(d: &[u8]) -> u64 {
    d.iter().take(8).fold(0, |v, b| (v << 8) | u64::from(*b))
}

/// [start, end) 里的子元素（大小未知的子元素截到下一个顶层元素之前）
fn children(d: &[u8], start: usize, end: usize) -> impl Iterator<Item = Element> + '_ {
    let mut at = start;
    std::iter::from_fn(move || {
        if at >= end {
            return None;
        }
        let mut e = element(d, at, end)?;
        if e.unknown {
            // 往后找下一个顶层元素的 ID
            let mut i = e.start;
            while i + 4 <= e.end {
                let id = u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
                if TOP.contains(&id) && i > e.start {
                    break;
                }
                i += 1;
            }
            e.end = if i + 4 <= e.end { i } else { e.end };
        }
        at = e.end;
        Some(e)
    })
}

#[derive(Default)]
struct Track {
    number: u64,
    codec: Option<Codec>,
    width: u32,
    height: u32,
    alpha: bool,
    colour: Option<Colour>,
    default_duration: Option<u64>,
}

fn track(d: &[u8], e: &Element) -> Option<Track> {
    let mut t = Track::default();
    let mut video = false;
    for c in children(d, e.start, e.end) {
        let v = &d[c.start..c.end];
        match c.id {
            TRACK_NUMBER => t.number = uint(v),
            TRACK_TYPE => video = uint(v) == 1,
            CODEC_ID => {
                t.codec = match v {
                    b"V_VP9" => Some(Codec::Vp9),
                    b"V_VP8" => Some(Codec::Vp8),
                    _ => None,
                }
            }
            DEFAULT_DURATION => t.default_duration = Some(uint(v)),
            VIDEO => {
                for p in children(d, c.start, c.end) {
                    let v = &d[p.start..p.end];
                    match p.id {
                        PIXEL_WIDTH => t.width = uint(v) as u32,
                        PIXEL_HEIGHT => t.height = uint(v) as u32,
                        ALPHA_MODE => t.alpha = uint(v) == 1,
                        COLOUR => {
                            let mut col = Colour { bt709: false, full_range: false };
                            for q in children(d, p.start, p.end) {
                                let v = uint(&d[q.start..q.end]);
                                match q.id {
                                    // 1 = BT.709；其余（含 2 = 未指定）按 BT.601
                                    MATRIX => col.bt709 = v == 1,
                                    // 2 = 全范围
                                    RANGE => col.full_range = v == 2,
                                    _ => {}
                                }
                            }
                            t.colour = Some(col);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (video && t.codec.is_some() && t.width > 0 && t.height > 0).then_some(t)
}

/// 块头：轨道号、相对 Cluster 的时间、标志；返回 (轨道, 相对时间, 码流开始处)。有分装（lacing）的块不支持
fn block_header(d: &[u8], e: &Element) -> Option<(u64, i16, usize)> {
    let (track, n) = vint(d, e.start, false)?;
    let at = e.start + n;
    if at + 3 > e.end {
        return None;
    }
    let rel = i16::from_be_bytes([d[at], d[at + 1]]);
    let lacing = (d[at + 2] >> 1) & 3;
    (lacing == 0).then_some((track, rel, at + 3))
}

pub fn parse(d: &[u8]) -> Result<Webm<'_>, Error> {
    let mut at = 0;
    let mut segment = None;
    while let Some(e) = (at < d.len()).then(|| element(d, at, d.len())).flatten() {
        match e.id {
            EBML => {}
            SEGMENT => {
                segment = Some(e);
                break;
            }
            _ => {}
        }
        at = e.end;
    }
    let seg = segment.ok_or_else(|| bad("no Segment"))?;
    let mut scale = 1_000_000u64;
    let mut video: Option<Track> = None;
    let mut frames = Vec::new();
    let mut base: Option<i64> = None;
    for e in children(d, seg.start, seg.end) {
        match e.id {
            INFO => {
                for c in children(d, e.start, e.end) {
                    if c.id == TIMECODE_SCALE {
                        scale = uint(&d[c.start..c.end]).max(1);
                    }
                }
            }
            TRACKS => {
                if video.is_none() {
                    video = children(d, e.start, e.end).filter(|c| c.id == TRACK_ENTRY).find_map(|c| track(d, &c));
                }
            }
            CLUSTER => {
                let Some(t) = &video else { return Err(bad("Cluster before Tracks")) };
                let mut cluster_time = 0i64;
                for c in children(d, e.start, e.end) {
                    let (block, alpha) = match c.id {
                        CLUSTER_TIME => {
                            cluster_time = uint(&d[c.start..c.end]) as i64;
                            continue;
                        }
                        SIMPLE_BLOCK => (Some(c), None),
                        BLOCK_GROUP => {
                            let (mut block, mut alpha) = (None, None);
                            for g in children(d, c.start, c.end) {
                                match g.id {
                                    BLOCK => block = Some(g),
                                    BLOCK_ADDITIONS => {
                                        for more in children(d, g.start, g.end).filter(|m| m.id == BLOCK_MORE) {
                                            let (mut id, mut data) = (1, None);
                                            for m in children(d, more.start, more.end) {
                                                match m.id {
                                                    BLOCK_ADD_ID => id = uint(&d[m.start..m.end]),
                                                    BLOCK_ADDITIONAL => data = Some(&d[m.start..m.end]),
                                                    _ => {}
                                                }
                                            }
                                            if id == 1 {
                                                alpha = data;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            (block, alpha)
                        }
                        _ => continue,
                    };
                    let Some(b) = block else { continue };
                    let Some((track, rel, data_at)) = block_header(d, &b) else { return Err(bad("laced or truncated block")) };
                    if track != t.number || data_at >= b.end {
                        continue;
                    }
                    let ticks = cluster_time + i64::from(rel);
                    let first = *base.get_or_insert(ticks);
                    let ns = (ticks - first).max(0) as u64 * scale;
                    frames.push(Frame { time: Duration::from_nanos(ns), data: &d[data_at..b.end], alpha: alpha.filter(|a| !a.is_empty()) });
                }
            }
            _ => {}
        }
    }
    // 没有 VP8 / VP9 视频轨（只有声音、H.264 / AV1 等）：不是坏了，是不支持
    let t = video.ok_or(Error::Unsupported)?;
    if frames.is_empty() {
        return Err(bad("no frames"));
    }
    Ok(Webm {
        codec: t.codec.expect("checked"),
        width: t.width,
        height: t.height,
        alpha: t.alpha,
        colour: t.colour.unwrap_or(Colour { bt709: false, full_range: false }),
        frame_duration: t.default_duration.filter(|&n| n > 0).map(Duration::from_nanos),
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼一个 EBML 元素（大小用 8 字节写法，或「未知」）
    fn el(id: u32, body: &[u8]) -> Vec<u8> {
        let mut v = id.to_be_bytes().iter().copied().skip_while(|&b| b == 0).collect::<Vec<_>>();
        v.push(0x01);
        v.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
        v.extend_from_slice(body);
        v
    }

    fn unknown(id: u32, body: &[u8]) -> Vec<u8> {
        let mut v = id.to_be_bytes().iter().copied().skip_while(|&b| b == 0).collect::<Vec<_>>();
        v.push(0xFF);
        v.extend_from_slice(body);
        v
    }

    fn block(track: u8, rel: i16, data: &[u8]) -> Vec<u8> {
        let mut v = vec![0x80 | track];
        v.extend_from_slice(&rel.to_be_bytes());
        v.push(0x80);
        v.extend_from_slice(data);
        v
    }

    fn sample(cluster: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
        let video = [el(PIXEL_WIDTH, &[2, 0]), el(PIXEL_HEIGHT, &[1, 0]), el(ALPHA_MODE, &[1]), el(COLOUR, &[el(MATRIX, &[1]), el(RANGE, &[2])].concat())].concat();
        let entry = [el(TRACK_NUMBER, &[1]), el(TRACK_TYPE, &[1]), el(CODEC_ID, b"V_VP9"), el(DEFAULT_DURATION, &40_000_000u32.to_be_bytes()), el(VIDEO, &video)].concat();
        let audio = [el(TRACK_NUMBER, &[2]), el(TRACK_TYPE, &[2]), el(CODEC_ID, b"A_OPUS")].concat();
        let tracks = el(TRACKS, &[el(TRACK_ENTRY, &audio), el(TRACK_ENTRY, &entry)].concat());
        let info = el(INFO, &el(TIMECODE_SCALE, &[0x0F, 0x42, 0x40]));
        let body = [
            el(CLUSTER_TIME, &[100]),
            el(SIMPLE_BLOCK, &block(1, 0, b"key")),
            el(SIMPLE_BLOCK, &block(2, 5, b"opus")),
            el(BLOCK_GROUP, &[el(BLOCK, &block(1, 40, b"inter")), el(BLOCK_ADDITIONS, &el(BLOCK_MORE, &[el(BLOCK_ADD_ID, &[1]), el(BLOCK_ADDITIONAL, b"alpha")].concat()))].concat()),
        ]
        .concat();
        let seg = [info, tracks, cluster(&body), el(CLUSTER, &[el(CLUSTER_TIME, &[0xB4]), el(SIMPLE_BLOCK, &block(1, 0, b"third"))].concat())].concat();
        [el(EBML, &el(0x4282, b"webm")), el(SEGMENT, &seg)].concat()
    }

    #[test]
    fn reads_the_video_track_frames_alpha_and_times() {
        for (name, cluster) in [("known", (|b: &[u8]| el(CLUSTER, b)) as fn(&[u8]) -> Vec<u8>), ("unknown size", |b: &[u8]| unknown(CLUSTER, b))] {
            let data = sample(cluster);
            assert!(is_webm(&data));
            let w = parse(&data).unwrap();
            assert_eq!((w.codec, w.width, w.height, w.alpha), (Codec::Vp9, 512, 256, true), "{name}");
            assert_eq!(w.colour, Colour { bt709: true, full_range: true });
            let got: Vec<_> = w.frames.iter().map(|f| (f.time.as_millis(), f.data, f.alpha)).collect();
            assert_eq!(got, [(0, &b"key"[..], None), (40, &b"inter"[..], Some(&b"alpha"[..])), (80, &b"third"[..], None)], "{name}");
            assert_eq!(w.delays(), [Duration::from_millis(40); 3]);
        }
    }

    #[test]
    fn rejects_what_is_not_a_vp9_webm() {
        assert!(!is_webm(b"\x1a\x45\xdf\xa3\x01\x00\x00\x00\x00\x00\x00\x00docx"));
        assert!(parse(&[el(EBML, &el(0x4282, b"webm"))].concat()).is_err());
        let mut data = sample(|b| el(CLUSTER, b));
        let at = data.windows(5).position(|w| w == b"V_VP9").unwrap();
        data[at..at + 5].copy_from_slice(b"V_AV1");
        assert!(parse(&data).is_err());
        // 截断：已经读到的帧还在
        let full = sample(|b| el(CLUSTER, b));
        assert!(parse(&full[..full.len() - 3]).is_ok());
    }
}
