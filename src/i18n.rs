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
    export_frame: "导出当前帧…", "Export current frame…";
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
    // 编辑工具栏
    undo: "撤销", "Undo";
    redo: "重做", "Redo";
    t_select: "选择", "Select";
    t_crop: "裁剪到选区", "Crop to selection";
    t_resize: "调整大小", "Resize";
    t_rotate: "旋转 / 翻转", "Rotate / flip";
    rotate_180: "旋转 180°", "Rotate 180°";
    t_pencil: "铅笔", "Pencil";
    t_brush: "刷子", "Brush";
    t_eraser: "橡皮擦", "Eraser";
    t_fill: "颜料桶", "Fill";
    t_picker: "取色器", "Color picker";
    t_text: "文字", "Text";
    t_shape: "形状", "Shapes";
    t_redact: "打码", "Redact";
    color1: "主色（左键）", "Primary color (left click)";
    color2: "辅色（右键）", "Secondary color (right click)";
    swap_colors: "交换主色与辅色", "Swap colors";
    custom_color: "自定义颜色", "Custom color";
    recent: "最近使用", "Recent";
    done: "完成", "Done";
    done_hint: "保存并回到查看", "Save and go back to viewing";
    copy_and_exit: "复制到剪贴板并退出", "Copy to clipboard and exit";
    more: "更多", "More";
    // 属性栏
    p_brush: "画笔", "Brush";
    p_rect: "框选", "Rectangle";
    p_mosaic: "马赛克", "Mosaic";
    p_blur: "模糊", "Blur";
    p_size: "大小", "Size";
    p_level: "强度", "Strength";
    p_width: "粗细", "Width";
    p_opacity: "不透明度", "Opacity";
    p_highlighter: "荧光笔", "Highlighter";
    p_tolerance: "容差", "Tolerance";
    p_erase_to: "擦成", "Erase to";
    p_transparent: "透明", "Transparent";
    p_secondary: "辅色", "Secondary color";
    p_outline: "轮廓", "Outline";
    p_fill: "填充", "Fill";
    p_font: "字体", "Font";
    p_text_bg: "背景填辅色", "Fill background";
    p_select_hint: "拖动框选；拖动选区内部可移动，拉四周的点可缩放", "Drag to select; drag inside to move, drag the handles to resize";
    p_picker_hint: "左键取主色，右键取辅色", "Left click picks the primary color, right click the secondary";
    p_select_all: "全选", "Select all";
    shape_line: "直线", "Line";
    shape_arrow: "箭头", "Arrow";
    shape_rect: "矩形", "Rectangle";
    shape_round: "圆角矩形", "Rounded rectangle";
    shape_ellipse: "椭圆", "Ellipse";
    // 状态栏、提示
    st_sel: "选区 {} × {}", "Selection {} × {}";
    need_selection: "先用「选择」拖出一个区域", "Drag out a selection first";
    edit_8bit: "编辑后保存为 8 位", "Saved as 8-bit after editing";
    edit_large: "图片很大，编辑可能会慢", "This image is large; editing may be slow";
    saving: "正在保存…", "Saving…";
    cut: "剪切", "Cut";
    delete_sel: "删除", "Delete";
    // 对话框
    exit_edit_title: "保存修改？", "Save changes?";
    exit_edit_body: "图片有未保存的修改。", "The image has unsaved changes.";
    overwrite_title: "覆盖原图？", "Overwrite the original?";
    overwrite_body: "原来的图片会被替换，不能恢复。", "The original image will be replaced. This can't be undone.";
    overwrite_btn: "覆盖保存", "Overwrite";
    expand_title: "粘贴的图片比画布大", "The pasted image is larger than the canvas";
    expand_body: "扩大画布以放下整张图片？", "Enlarge the canvas to fit the whole image?";
    expand_btn: "扩大画布", "Enlarge canvas";
    keep_size: "保持大小", "Keep size";
    resize_title: "调整大小", "Resize";
    by_percent: "百分比", "Percentage";
    by_pixels: "像素", "Pixels";
    r_width: "宽", "Width";
    r_height: "高", "Height";
    keep_ratio: "保持宽高比", "Keep aspect ratio";
    save_options: "保存选项", "Save options";
    keep_meta: "保留 EXIF（相机信息、拍摄时间、GPS 位置）", "Keep EXIF (camera, date taken, GPS location)";
    keep_meta_redacted: "图片打过码。EXIF 里可能有位置等信息，默认不保留。", "This image was redacted. EXIF may contain your location, so it's not kept by default.";
    s_keep_meta: "保存时保留 EXIF（打过码的图不保留）", "Keep EXIF when saving (never for redacted images)";
    s_undo_steps: "撤销步数", "Undo steps";
    s_undo_mb: "撤销内存上限（MB）", "Undo memory limit (MB)";
    s_redact_default: "打码默认", "Redaction default";
    // 文件关联
    assoc_desc: "轻量看图：看图、编辑、打码", "Lightweight image viewer with editing and redaction";
    ctx_edit_with: "用 LiteView 编辑", "Edit with LiteView";
    ctx_mosaic_with: "用 LiteView 打码", "Redact with LiteView";
    a_registered: "已注册为可以打开图片的程序。", "LiteView is registered as an image viewer.";
    a_not_registered: "还没有注册。不点下面的按钮，LiteView 不会写注册表。", "Not registered. LiteView never writes to the registry unless you click below.";
    a_note: "Windows 不允许程序自己改默认应用：点「设为默认看图软件」后，在打开的系统设置里选 LiteView。", "Windows doesn't let apps change defaults themselves: after clicking “Set as default”, choose LiteView in the Settings page that opens.";
    a_groups: "常用|更多格式|专业|相机 RAW", "Common|More|Professional|Camera RAW";
    a_set_default: "设为默认看图软件", "Set as default viewer";
    a_unregister: "取消关联", "Remove associations";
    a_select_all: "全选", "Select all";
    a_restore: "恢复默认", "Reset";
    a_is_default: "（当前默认）", " (default)";
    a_windows_only: "文件关联目前只支持 Windows。", "File associations are Windows-only for now.";
    a_removed: "已取消关联", "Associations removed";
    ask_title: "设为默认看图软件？", "Make LiteView your default image viewer?";
    ask_body: "以后双击图片就用 LiteView 打开。", "Images will open in LiteView when you double-click them.";
    ask_later: "以后再说", "Later";
    ask_never: "不再询问", "Don't ask again";
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
