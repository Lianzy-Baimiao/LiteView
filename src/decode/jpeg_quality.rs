//! 识别标准 IJG 品质量化表；JPEG 本身没有通用的「质量」字段。
//! 只在所有 DQT 表精确对应同一个 1..100 品质时返回值；自定义/混合表回退到用户设置。
//! 在解码线程已有文件字节上检查，不在保存 UI 中再次读取原文件。

// JPEG Annex K tables, in natural coefficient order.
#[rustfmt::skip]
const LUMA: [u8; 64] = [
    16,11,10,16,24,40,51,61, 12,12,14,19,26,58,60,55,
    14,13,16,24,40,57,69,56, 14,17,22,29,51,87,80,62,
    18,22,37,56,68,109,103,77, 24,35,55,64,81,104,113,92,
    49,64,78,87,103,121,120,101, 72,92,95,98,112,100,103,99,
];
#[rustfmt::skip]
const CHROMA: [u8; 64] = [
    17,18,24,47,99,99,99,99, 18,21,26,66,99,99,99,99,
    24,26,56,99,99,99,99,99, 47,66,99,99,99,99,99,99,
    99,99,99,99,99,99,99,99, 99,99,99,99,99,99,99,99,
    99,99,99,99,99,99,99,99, 99,99,99,99,99,99,99,99,
];
#[rustfmt::skip]
const ZIGZAG: [usize; 64] = [
    0,1,8,16,9,2,3,10,17,24,32,25,18,11,4,5,
    12,19,26,33,40,48,41,34,27,20,13,6,7,14,21,28,
    35,42,49,56,57,50,43,36,29,22,15,23,30,37,44,51,
    58,59,52,45,38,31,39,46,53,60,61,54,47,55,62,63,
];

pub(super) fn estimate(data: &[u8]) -> Option<u8> {
    if !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut rest = &data[2..];
    let mut tables: [Option<[u8; 64]>; 2] = [None, None];
    loop {
        if *rest.first()? != 0xff {
            return None;
        }
        while rest.first() == Some(&0xff) {
            rest = &rest[1..];
        }
        let marker = *rest.first()?;
        rest = &rest[1..];
        if matches!(marker, 0x00 | 0xd0..=0xd9) {
            return None;
        }
        if marker == 0x01 {
            continue;
        }
        let len = u16::from_be_bytes(rest.get(..2)?.try_into().ok()?) as usize;
        let mut segment = rest.get(2..len)?;
        rest = rest.get(len..)?;
        if marker == 0xda {
            break; // Never scan entropy-coded pixels or mistake FF DB inside metadata for DQT.
        }
        if marker != 0xdb {
            continue;
        }
        while !segment.is_empty() {
            let spec = segment[0];
            // Conservative: only 8-bit, standard table IDs. Extended/custom tables are not guessed.
            if spec > 1 {
                return None;
            }
            let table: [u8; 64] = segment.get(1..65)?.try_into().ok()?;
            if tables[spec as usize].replace(table).is_some() {
                return None;
            }
            segment = &segment[65..];
        }
    }
    tables[0]?;
    (1..=100).find(|&quality| {
        let scale = if quality < 50 { 5000 / u32::from(quality) } else { 200 - 2 * u32::from(quality) };
        tables.iter().zip([&LUMA, &CHROMA]).all(|(table, base)| {
            table
                .as_ref()
                .is_none_or(|table| table.iter().zip(ZIGZAG).all(|(&value, i)| value == ((u32::from(base[i]) * scale + 50) / 100).clamp(1, 255) as u8))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(quality: u8, gray: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
        if gray {
            encoder.encode_image(&image::GrayImage::new(8, 8)).unwrap();
        } else {
            encoder.encode_image(&image::RgbImage::from_fn(8, 8, |x, y| image::Rgb([x as u8 * 30, y as u8 * 30, 45]))).unwrap();
        }
        bytes
    }

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xff, marker];
        out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn recovers_all_standard_rgb_and_grayscale_qualities() {
        for quality in 1..=100 {
            for gray in [false, true] {
                assert_eq!(estimate(&jpeg(quality, gray)), Some(quality), "q={quality} gray={gray}");
            }
        }
    }

    #[test]
    fn skips_metadata_and_marker_fill_bytes() {
        let bytes = jpeg(63, false);
        let mut marked = bytes[..2].to_vec();
        marked.push(0xff); // Legal marker fill byte.
        marked.extend(segment(0xe1, &[0xff, 0xdb, 0, 67, 0, 255]));
        marked.extend_from_slice(&bytes[2..]);
        assert_eq!(estimate(&marked), Some(63));
    }

    #[test]
    fn single_and_combined_tables_are_supported() {
        let bytes = jpeg(76, false);
        let offsets: Vec<_> = bytes.windows(2).enumerate().filter_map(|(i, w)| (w == [0xff, 0xdb]).then_some(i)).collect();
        assert_eq!(offsets.len(), 2);
        let luma = &bytes[offsets[0] + 4..offsets[0] + 69];
        let chroma = &bytes[offsets[1] + 4..offsets[1] + 69];
        for payload in [luma.to_vec(), [luma, chroma].concat()] {
            let header = [vec![0xff, 0xd8], segment(0xdb, &payload), segment(0xda, &[1, 1, 0, 0, 63, 0])].concat();
            assert_eq!(estimate(&header), Some(76));
        }
    }

    #[test]
    fn custom_mixed_and_extended_tables_fall_back() {
        let bytes = jpeg(74, false);
        let offsets: Vec<_> = bytes.windows(2).enumerate().filter_map(|(i, w)| (w == [0xff, 0xdb]).then_some(i)).collect();
        let mut custom = bytes.clone();
        custom[offsets[0] + 5] += 1;
        assert_eq!(estimate(&custom), None);
        let mut mixed = bytes.clone();
        let other = jpeg(85, false);
        let at = other.windows(2).rposition(|w| w == [0xff, 0xdb]).unwrap();
        mixed[offsets[1] + 5..offsets[1] + 69].copy_from_slice(&other[at + 5..at + 69]);
        assert_eq!(estimate(&mixed), None);
        for spec in [2, 3, 0x10, 0xf0] {
            let mut unsupported = bytes.clone();
            unsupported[offsets[0] + 4] = spec;
            assert_eq!(estimate(&unsupported), None);
        }
    }

    #[test]
    fn truncated_headers_and_invalid_lengths_never_panic() {
        let bytes = jpeg(90, false);
        let sos = bytes.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
        for end in 0..sos + 10 {
            assert_eq!(estimate(&bytes[..end]), None, "end={end}");
        }
        for length in [0, 1, 65535] {
            let mut broken = bytes.clone();
            broken[4..6].copy_from_slice(&(length as u16).to_be_bytes());
            assert_eq!(estimate(&broken), None);
        }
        assert_eq!(estimate(&[0xff, 0xd8, 0xff, 0xff]), None);
        assert_eq!(estimate(&[0xff, 0xd8, 0xff, 0xd9]), None);
    }
}
