//! Developer-only comparison of the real egui glyph rasterizer, without a desktop window.
//! cargo run --release --example font_probe -- dist/font-qa

use egui::epaint::text::{HintingTarget, SmoothHinting};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId};

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&out).unwrap();
    let fonts = std::path::PathBuf::from(std::env::var_os("WINDIR").expect("Windows font probe")).join("Fonts");
    let bytes = std::fs::read(fonts.join("msyh.ttc")).unwrap();
    let modes = [
        ("baseline", true, SmoothHinting::default()),
        ("gridfit", true, SmoothHinting { light: false, symmetric_rendering: false, preserve_linear_metrics: false }),
        ("light", true, SmoothHinting { light: true, symmetric_rendering: false, preserve_linear_metrics: true }),
        ("outline", false, SmoothHinting::default()),
    ];
    for ppp in [1.0, 1.25, 1.5, 2.0] {
        let mut sheet = image::RgbaImage::from_pixel(1000, 320, image::Rgba([255, 255, 255, 255]));
        for (i, (name, hinting, target)) in modes.iter().enumerate() {
            let ctx = egui::Context::default();
            ctx.set_theme(egui::Theme::Light);
            ctx.set_pixels_per_point(ppp);
            let mut defs = FontDefinitions::default();
            let mut data = FontData::from_owned(bytes.clone());
            data.index = 1;
            data.tweak.hinting = Some(*hinting);
            data.tweak.hinting_target = HintingTarget::Smooth(*target);
            defs.font_data.insert("probe".into(), data.into());
            defs.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "probe".into());
            ctx.set_fonts(defs);
            let mut sample = None;
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                sample = Some(ui.ctx().fonts_mut(|f| {
                    let text = format!("{name}: 打开图片  文件关联  常用格式  默认应用  LiteView 012345");
                    let g = f.layout_no_wrap(text, FontId::proportional(14.0), Color32::BLACK);
                    (g, f.image())
                }));
            });
            let (galley, atlas) = sample.unwrap();
            let mut coverage = 0.0;
            let mut soft = 0;
            let mut ink = 0;
            for row in &galley.rows {
                for glyph in &row.glyphs {
                    let uv = glyph.uv_rect;
                    if uv.is_nothing() {
                        continue;
                    }
                    let pos = row.visuals.mesh.vertices[glyph.first_vertex as usize].pos;
                    for y in uv.min[1]..uv.max[1] {
                        for x in uv.min[0]..uv.max[0] {
                            let alpha = atlas[(x as usize, y as usize)].a();
                            if !glyph.chr.is_ascii() && alpha > 0 {
                                coverage += alpha as f64 / 255.0;
                                ink += 1;
                                if alpha < 255 {
                                    soft += 1;
                                }
                            }
                            let px = (pos.x * ppp).round() as i32 + i32::from(x - uv.min[0]) + 12;
                            let py = (pos.y * ppp).round() as i32 + i32::from(y - uv.min[1]) + 15 + i as i32 * 75;
                            if px >= 0 && py >= 0 && px < sheet.width() as i32 && py < sheet.height() as i32 {
                                sheet.put_pixel(px as u32, py as u32, image::Rgba([255 - alpha, 255 - alpha, 255 - alpha, 255]));
                            }
                        }
                    }
                }
            }
            println!("dpi={ppp} mode={name} ink={coverage:.1} edge_ratio={:.3}", soft as f64 / ink as f64);
        }
        sheet.save(out.join(format!("font-{}pct.png", (ppp * 100.0) as u32))).unwrap();
    }
}
