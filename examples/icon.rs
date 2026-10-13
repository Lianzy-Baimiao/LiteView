//! 从 assets/app.svg 生成 assets/app.ico（16 ~ 256 各尺寸）：`cargo run --example icon`

use image::codecs::ico::{IcoEncoder, IcoFrame};

fn main() {
    let svg = std::fs::read_to_string("assets/app.svg").expect("assets/app.svg");
    let frames: Vec<IcoFrame> = [16u32, 20, 24, 32, 40, 48, 64, 128, 256]
        .iter()
        .map(|&px| {
            let rgba = liteview::ui::icons::rasterize_color(&svg, px);
            IcoFrame::as_png(&rgba, px, px, image::ExtendedColorType::Rgba8).expect("encode frame")
        })
        .collect();
    let f = std::fs::File::create("assets/app.ico").expect("create app.ico");
    IcoEncoder::new(f).encode_images(&frames).expect("write app.ico");
    println!("assets/app.ico");
}
