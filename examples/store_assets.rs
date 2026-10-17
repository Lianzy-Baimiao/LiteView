//! Regenerate MSIX assets from the same vector used by the application's window icon.
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/msix/Assets"));
    std::fs::create_dir_all(&output)?;
    for (name, size) in [("StoreLogo", 50), ("Square44x44Logo", 44), ("Square150x150Logo", 150)] {
        for scale in [100, 125, 150, 200, 400] {
            let px = (size * scale + 50) / 100;
            let pixels = liteview::ui::icons::rasterize_color(include_str!("../assets/app.svg"), px);
            image::save_buffer(output.join(format!("{name}.scale-{scale}.png")), &pixels, px, px, image::ColorType::Rgba8)?;
        }
    }
    Ok(())
}
