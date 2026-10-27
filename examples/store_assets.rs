//! Regenerate MSIX assets from the same vector used by the application's window icon.
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/msix/Assets"));
    std::fs::create_dir_all(&output)?;
    let save = |name: String, px: u32| -> Result<(), Box<dyn std::error::Error>> {
        let pixels = liteview::ui::icons::rasterize_color(include_str!("../assets/app.svg"), px);
        image::save_buffer(output.join(name), &pixels, px, px, image::ColorType::Rgba8)?;
        Ok(())
    };
    for (name, size) in [("StoreLogo", 50), ("Square44x44Logo", 44), ("Square150x150Logo", 150)] {
        for scale in [100, 125, 150, 200, 400] {
            save(format!("{name}.scale-{scale}.png"), (size * scale + 50) / 100)?;
        }
    }
    // Start menu, taskbar, Explorer and context menus pick Square44x44Logo by pixel size; without these the shell scales
    // the 44 px image down and the icon looks blurry. The icon has no backplate, so the unplated form is the same image.
    for size in [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256] {
        save(format!("Square44x44Logo.targetsize-{size}.png"), size)?;
        save(format!("Square44x44Logo.targetsize-{size}_altform-unplated.png"), size)?;
    }
    Ok(())
}
