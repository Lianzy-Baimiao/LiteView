use super::{Error, Options};

/// image 0.25.10 charges TIFF metadata against the tiny pixel buffer, so a
/// valid ICC profile can disappear for small images. Read the first IFD's
/// bounded, uncompressed ICC field directly (classic TIFF and BigTIFF).
pub fn icc(data: &[u8], opts: &Options) -> Result<Option<Vec<u8>>, Error> {
    let Some(profile) = profile(data) else { return Ok(None) };
    if profile.len() as u64 > opts.max_alloc.min(16 << 20) {
        return Err(Error::TooLarge { width: 0, height: 0 });
    }
    Ok(Some(profile.to_vec()))
}

fn profile(data: &[u8]) -> Option<&[u8]> {
    let little = match data.get(..2)? { b"II" => true, b"MM" => false, _ => return None };
    let read = |offset: usize, size: usize| -> Option<u64> {
        let bytes = data.get(offset..offset.checked_add(size)?)?;
        Some(if little { bytes.iter().rev().fold(0, |n, &b| (n << 8) | u64::from(b)) }
            else { bytes.iter().fold(0, |n, &b| (n << 8) | u64::from(b)) })
    };
    let (ifd, count_size, entry_size, offset_size) = match read(2, 2)? {
        42 => (usize::try_from(read(4, 4)?).ok()?, 2, 12, 4),
        43 if read(4, 2)? == 8 && read(6, 2)? == 0 => (usize::try_from(read(8, 8)?).ok()?, 8, 20, 8),
        _ => return None,
    };
    let count = usize::try_from(read(ifd, count_size)?).ok()?;
    let start = ifd.checked_add(count_size)?;
    let end = start.checked_add(count.checked_mul(entry_size)?)?;
    data.get(start..end)?;
    for offset in (start..end).step_by(entry_size) {
        if read(offset, 2)? != 34675 { continue; }
        if !matches!(read(offset + 2, 2)?, 1 | 7) { return None; }
        let size = usize::try_from(read(offset + 4, offset_size)?).ok()?;
        let value = offset + 4 + offset_size;
        let at = if size <= offset_size { value } else { usize::try_from(read(value, offset_size)?).ok()? };
        return data.get(at..at.checked_add(size)?);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_ifd_counts_and_offsets_do_not_panic() {
        for size in 0..96 {
            for head in [b"II*\0".as_slice(), b"MM\0*", b"II+\0\x08\0\0\0"] {
                let mut bytes = vec![255; size];
                let n = size.min(head.len());
                bytes[..n].copy_from_slice(&head[..n]);
                assert!(icc(&bytes, &Options::default()).unwrap().is_none());
            }
        }
    }

    #[test]
    fn bigtiff_profile_and_budget() {
        for little in [false, true] {
            let mut bytes = vec![0u8; 80];
            bytes[..2].copy_from_slice(if little { b"II" } else { b"MM" });
            let mut put = |at: usize, value: u64, n: usize| {
                let encoded = if little { value.to_le_bytes() } else { value.to_be_bytes() };
                bytes[at..at + n].copy_from_slice(if little { &encoded[..n] } else { &encoded[8 - n..] });
            };
            put(2, 43, 2); put(4, 8, 2); put(8, 16, 8);
            put(16, 1, 8); put(24, 34675, 2); put(26, 7, 2);
            put(28, 12, 8); put(36, 64, 8);
            bytes[64..76].copy_from_slice(b"test-profile");
            assert_eq!(icc(&bytes, &Options::default()).unwrap(), Some(b"test-profile".to_vec()));
            assert!(icc(&bytes, &Options { max_alloc: 8, ..Default::default() }).is_err());
        }
    }
}
