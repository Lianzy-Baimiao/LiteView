//! 打印（首版只有 Windows，见 docs/PRINTING-PLAN.md）：`layout` 页面几何，`raster` 交给打印机的像素，
//! `dpi` 图片文件记录的分辨率。打印机接口在 `platform::print`，对话框在 `app::print_ui`。

pub mod dpi;
pub mod layout;
pub mod raster;
