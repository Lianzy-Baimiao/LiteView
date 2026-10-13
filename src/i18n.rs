//! 界面文字：简体中文（默认）、English。`tr()` 取当前语言的文字表。

use std::sync::atomic::{AtomicBool, Ordering};

use crate::settings::Language;

macro_rules! strings {
    ($($key:ident: $zh:expr, $en:expr;)*) => {
        pub struct Strings { $(pub $key: &'static str,)* }
        pub static ZH: Strings = Strings { $($key: $zh,)* };
        pub static EN: Strings = Strings { $($key: $en,)* };
    };
}

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn tr() -> &'static Strings {
    if ENGLISH.load(Ordering::Relaxed) { &EN } else { &ZH }
}

pub fn set_language(lang: Language) {
    let english = match lang {
        Language::ZhCn => false,
        Language::En => true,
        Language::Auto => sys_locale::get_locale().is_some_and(|l| !l.to_ascii_lowercase().starts_with("zh")),
    };
    ENGLISH.store(english, Ordering::Relaxed);
}

/// 依次替换 `{}`
pub fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut parts = template.split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (i, part) in parts.enumerate() {
        if let Some(a) = args.get(i) {
            out.push_str(&a.to_string());
        }
        out.push_str(part);
    }
    out
}

strings! {
    app_name: "LiteView", "LiteView";
    // 工具栏
    prev: "上一张", "Previous";
    next: "下一张", "Next";
    zoom_in: "放大", "Zoom in";
    zoom_out: "缩小", "Zoom out";
    fit: "适应窗口", "Fit to window";
    actual: "原始大小", "Actual size";
    rotate_cw: "顺时针旋转", "Rotate clockwise";
    rotate_ccw: "逆时针旋转", "Rotate counterclockwise";
    flip_h: "水平翻转", "Flip horizontal";
    flip_v: "垂直翻转", "Flip vertical";
    edit: "编辑", "Edit";
    mosaic: "打码", "Redact";
    delete: "删除", "Delete";
    info: "图片信息", "Image info";
    settings: "设置", "Settings";
    open: "打开图片", "Open image";
    paste: "粘贴", "Paste";
    fullscreen: "全屏", "Full screen";
    play: "播放", "Play";
    pause: "暂停", "Pause";
    frame_prev: "上一帧", "Previous frame";
    frame_next: "下一帧", "Next frame";
    speed: "播放速度", "Playback speed";
    anim_no_edit: "动图不支持编辑", "Animated images can't be edited";
    // 右键菜单
    copy_image: "复制图片", "Copy image";
    copy_path: "复制文件路径", "Copy file path";
    save_as: "另存为…", "Save as…";
    rename: "重命名", "Rename";
    set_wallpaper: "设为桌面背景", "Set as desktop background";
    show_in_folder: "在资源管理器中显示", "Show in folder";
    show_in_folder_mac: "在访达中显示", "Show in Finder";
    show_in_folder_linux: "打开所在文件夹", "Open containing folder";
    open_with: "打开方式…", "Open with…";
    // 轻提示
    copied: "已复制", "Copied";
    saved: "已保存", "Saved";
    trashed: "已移到回收站", "Moved to Recycle Bin";
    trashed_mac: "已移到废纸篓", "Moved to Trash";
    deleted: "已删除", "Deleted";
    wrapped_first: "已回到第一张", "Back to the first image";
    wrapped_last: "已跳到最后一张", "Jumped to the last image";
    at_last: "已经是最后一张", "This is the last image";
    at_first: "已经是第一张", "This is the first image";
    clipboard_image: "剪贴板图片", "Clipboard image";
    image_files: "图片", "Images";
    all_files: "所有文件", "All files";
    only_one: "只有这一张图片", "This is the only image";
    nothing_to_paste: "剪贴板里没有图片", "No image on the clipboard";
    // 空状态
    empty_hint: "或把图片拖到这里，Ctrl+V 粘贴", "or drop an image here, Ctrl+V to paste";
    // 打不开
    cannot_open: "无法打开此图片", "Can't open this image";
    err_unsupported: "不支持这种格式", "This format isn't supported";
    err_corrupt: "文件已损坏或不完整", "The file is damaged or incomplete";
    err_codec: "缺少系统解码扩展", "A system codec is missing";
    err_codec_heic: "需要从微软商店安装「HEIF 图像扩展」和「HEVC 视频扩展」", "Install “HEIF Image Extensions” and “HEVC Video Extensions” from the Microsoft Store";
    err_denied: "没有访问权限", "Access denied";
    err_not_found: "文件已被删除或移动", "The file was deleted or moved";
    err_too_large: "图片过大", "The image is too large";
    open_other: "用其它程序打开", "Open with another app";
    get_from_store: "去微软商店安装", "Get it from the Microsoft Store";
    // 确认框
    delete_title: "删除这张图片？", "Delete this image?";
    delete_body_trash: "文件会移到回收站。", "The file will be moved to the Recycle Bin.";
    delete_body_trash_mac: "文件会移到废纸篓。", "The file will be moved to the Trash.";
    delete_body_forever: "文件会被彻底删除，不能恢复。", "The file will be permanently deleted. This can't be undone.";
    delete_btn: "删除", "Delete";
    delete_forever_btn: "彻底删除", "Delete permanently";
    cancel: "取消", "Cancel";
    dont_ask: "不再询问", "Don't ask again";
    rotate_save_title: "保存旋转？", "Save rotation?";
    rotate_save_body: "旋转后的图片还没有保存。", "The rotated image hasn't been saved.";
    rotate_save_jpeg: "JPEG 只改方向标记，画质无损。", "For JPEG only the orientation tag changes — no quality loss.";
    save: "保存", "Save";
    dont_save: "不保存", "Don't save";
    failed: "失败：{}", "Failed: {}";
    // 设置
    s_general: "常规", "General";
    s_view: "查看", "Viewing";
    s_anim: "动图", "Animation";
    s_edit: "编辑", "Editing";
    s_assoc: "文件关联", "File associations";
    s_about: "关于", "About";
    s_language: "语言", "Language";
    s_follow_system: "跟随系统", "System";
    s_theme: "主题", "Theme";
    s_light: "浅色", "Light";
    s_dark: "深色", "Dark";
    s_confirm_delete: "删除前确认", "Confirm before deleting";
    s_wrap: "到最后一张后回到第一张", "Wrap around at the end of the folder";
    s_fit: "打开时", "When opening";
    s_fit_fit: "适应窗口", "Fit to window";
    s_fit_actual: "原始大小", "Actual size";
    s_fit_width: "适应宽度", "Fit width";
    s_wheel: "滚轮", "Mouse wheel";
    s_wheel_zoom: "缩放", "Zoom";
    s_wheel_nav: "上一张 / 下一张", "Previous / next";
    s_smooth: "缩放动画", "Smooth zoom";
    s_inertia: "拖动惯性", "Drag inertia";
    s_background: "背景", "Background";
    s_bg_theme: "跟随主题", "Theme";
    s_bg_black: "黑", "Black";
    s_bg_white: "白", "White";
    s_bg_checker: "棋盘格", "Checkerboard";
    s_double: "双击", "Double-click";
    s_double_fit: "适应窗口 / 100%", "Fit / 100%";
    s_double_full: "全屏", "Full screen";
    s_preload: "预读前后几张", "Preload neighbors";
    s_cache: "缓存上限（MB）", "Cache limit (MB)";
    s_nearest: "放大超过多少改用最近邻（%）", "Show pixels above zoom (%)";
    s_sort: "排序", "Sort by";
    s_sort_name: "文件名", "Name";
    s_sort_time: "修改时间", "Date modified";
    s_sort_size: "大小", "Size";
    s_sort_type: "类型", "Type";
    s_sort_desc: "降序", "Descending";
    s_autoplay: "打开时自动播放", "Play automatically";
    s_always_loop: "始终循环（忽略文件里的次数）", "Always loop (ignore the file's play count)";
    s_anim_cache: "帧缓存上限（MB）", "Frame cache limit (MB)";
    s_confirm_overwrite: "覆盖原图前确认", "Confirm before overwriting";
    s_jpeg_quality: "JPEG 质量", "JPEG quality";
    s_version: "版本 {}", "Version {}";
    s_license: "MIT 开源许可。图标：Fluent UI System Icons（MIT）。", "MIT licensed. Icons: Fluent UI System Icons (MIT).";
    s_open_config: "打开配置文件所在目录", "Open settings folder";
    s_homepage: "项目主页", "Project page";
    s_coming: "下一个版本提供", "Coming in the next version";
    close: "关闭", "Close";
    name_exists: "已有同名文件", "A file with that name already exists";
    // 图片信息
    i_name: "文件名", "Name";
    i_folder: "位置", "Folder";
    i_dims: "尺寸", "Dimensions";
    i_size: "文件大小", "File size";
    i_format: "格式", "Format";
    i_frames: "帧", "Frames";
    i_frames_fmt: "{} 帧，{} 秒", "{} frames, {} s";
    i_modified: "修改时间", "Modified";
    i_camera: "相机", "Camera";
    i_lens: "镜头", "Lens";
    i_aperture: "光圈", "Aperture";
    i_shutter: "快门", "Shutter";
    i_iso: "ISO", "ISO";
    i_focal: "焦距", "Focal length";
    i_taken: "拍摄时间", "Taken";
    i_gps: "GPS 位置", "GPS location";
    i_gps_show: "显示 GPS 位置", "Show GPS location";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_placeholders() {
        assert_eq!(fill("帧 {} / {}", &[&3, &10]), "帧 3 / 10");
        assert_eq!(fill("no args", &[]), "no args");
        assert_eq!(fill("{} {}", &[&"a"]), "a ");
    }
}
