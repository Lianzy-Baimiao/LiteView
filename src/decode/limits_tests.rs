use super::*;

fn limited(data: &[u8], ext: &str, budget: u64) {
    let opts = Options { max_alloc: budget, pyramid: false, ..Default::default() };
    assert!(matches!(decode_bytes(data, ext, &opts), Err(Error::TooLarge { .. })), "{ext} bypassed the pixel budget");
}

#[test]
fn allocation_limits_cover_animation_and_jxl() {
    for (data, ext) in [
        (&include_bytes!("../../tests/fixtures/animation.png")[..], "png"),
        (&include_bytes!("../../tests/fixtures/animation.webp")[..], "webp"),
        (&include_bytes!("../../tests/fixtures/animation.jxl")[..], "jxl"),
    ] {
        limited(data, ext, 16);
    }
    let image = image::RgbaImage::from_pixel(8, 6, image::Rgba([20, 40, 60, 255]));
    let data = crate::save::encode(&image, crate::save::Format::Gif, 90).unwrap();
    limited(&data, "gif", 16);
}

#[test]
fn allocation_limits_cover_svg_and_psd() {
    limited(br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#, "svg", 16);
    let mut psd = b"8BPS\0\x01\0\0\0\0\0\0\0\x03\0\0\0\x01\0\0\0\x02\0\x08\0\x03".to_vec();
    psd.extend_from_slice(&[0; 14]);
    psd.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
    limited(&psd, "psd", 4);
}

#[test]
fn allocation_rejection_cannot_fall_back_to_system_decoder() {
    let image = image::RgbaImage::from_pixel(8, 6, image::Rgba([20, 40, 60, 255]));
    for format in [crate::save::Format::Png, crate::save::Format::Jpeg, crate::save::Format::Bmp] {
        let data = crate::save::encode(&image, format, 90).unwrap();
        limited(&data, format.exts()[0], 16);
    }
}

#[test]
fn source_limit_applies_to_memory_and_file_inputs() {
    let data = include_bytes!("../../tests/fixtures/poster.png");
    let opts = Options { max_source: 8, ..Default::default() };
    assert!(matches!(decode_bytes(data, "png", &opts), Err(Error::TooLarge { .. })));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/poster.png");
    assert!(matches!(decode_file(&path, &opts), Err(Error::TooLarge { .. })));
    assert!(decode_file(&path, &Options::default()).is_ok());
}

#[test]
fn fallback_does_not_run_after_resource_rejection() {
    let result = fallback(Err(Error::TooLarge { width: 9, height: 8 }), || panic!("budget bypass"));
    assert!(matches!(result, Err(Error::TooLarge { width: 9, height: 8 })));
}

#[test]
fn streamed_frames_keep_original_budget() {
    let data = include_bytes!("../../tests/fixtures/animation.png");
    let opts = Options { anim_cache: 1, ..Default::default() };
    let decoded = decode_bytes(data, "png", &opts).unwrap();
    let Content::Animation { source: Some(source), .. } = decoded.content else { panic!("no stream") };
    let restricted = AnimSource { data: source.data.clone(), format: source.format, options: Options { max_alloc: 16, ..opts } };
    assert!(matches!(restricted.each_frame(|_, _| panic!("over-budget frame allocated")), Err(Error::TooLarge { .. })));
}

#[cfg(windows)]
#[test]
fn wic_obeys_the_same_budget() {
    let data = include_bytes!("../../tests/fixtures/poster.png");
    let opts = Options { max_alloc: 16, ..Default::default() };
    assert!(matches!(super::wic::decode(data, Kind::Raster(ImageFormat::Png), &opts), Err(Error::TooLarge { .. })));
}
