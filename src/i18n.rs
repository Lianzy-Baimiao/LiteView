//! 界面文字：简体中文（默认）、English。`tr()` 取当前语言的文字表。

use std::sync::atomic::{AtomicBool, Ordering};

use crate::settings::Language;

macro_rules! strings {
    ($($key:ident: $zh:expr, $en:expr;)*) => {
        pub struct Strings { $(pub $key: &'static str,)* }
        pub static ZH: Strings = Strings { $($key: $zh,)* };
        pub static EN: Strings = Strings { $($key: $en,)* };
        #[cfg(test)]
        #[test]
        fn translations_are_complete_and_placeholders_match() {
            $(
                assert!(!$en.trim().is_empty(), "missing English string: {}", stringify!($key));
                assert_eq!($zh.matches("{}").count(), $en.matches("{}").count(), "placeholder mismatch: {}", stringify!($key));
                assert!(!$en.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)), "untranslated English string: {}", stringify!($key));
            )*
        }
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
    webp_lossless: "WebP 无损压缩", "Lossless WebP";
    webp_quality: "WebP 有损品质", "Lossy WebP quality";
    saved_unverified: "文件已写入 {}，但读回校验失败。编辑内容仍保留，请检查文件后再关闭。", "File written to {}, but verification failed. Edits are kept; check the file before closing.";
    save_drops_exif: "此格式不保留 EXIF。", "This format does not preserve EXIF.";
    save_drops_icc: "此格式不保留 ICC 色彩配置，其他软件中的颜色可能不同。", "This format does not preserve ICC profiles; colors may differ in other apps.";
    save_flattens_alpha: "透明区域保存为白色背景；当前画布及撤销记录保留透明度。", "Transparency is flattened onto white in the file; the canvas and undo history keep alpha.";
    save_binary_alpha: "GIF 仅支持全透明或不透明，半透明像素会被量化。", "GIF only supports binary transparency; partial alpha is quantized.";
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
    s_accent: "强调色", "Accent color";
    s_custom: "自选", "Custom";
    wallpaper_set: "已设为桌面背景", "Set as desktop background";
    s_theme: "主题", "Theme";
    s_light: "浅色", "Light";
    s_dark: "深色", "Dark";
    s_confirm_delete: "删除前确认", "Confirm before deleting";
    s_frame_cap: "省电模式", "Power saving";
    s_frame_cap_auto: "自动", "Auto";
    s_frame_cap_60: "60 Hz", "60 Hz";
    s_frame_cap_off: "不限", "Off";
    s_frame_cap_hint: "只影响拖动、缩放、动图等连续画面，静止时不画。自动：高刷新率屏降到不低于 60 帧的均匀帧率（144 Hz → 72）", "Only affects continuous motion (drag, zoom, animations); nothing is drawn while idle. Auto: high-refresh screens use an even rate of at least 60 fps (144 Hz → 72)";
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
    lock_zoom: "锁定缩放：切图时保持缩放比例（同样尺寸的图连位置也保持）", "Lock zoom: keep the zoom when changing images (and the position for images of the same size)";
    s_fit_window: "窗口适应图片大小", "Resize window to fit the image";
    s_autoplay: "打开时自动播放", "Play automatically";
    s_always_loop: "始终循环（忽略文件里的次数）", "Always loop (ignore the file's play count)";
    s_anim_cache: "帧缓存上限（MB）", "Frame cache limit (MB)";
    s_confirm_overwrite: "覆盖原图前确认", "Confirm before overwriting";
    s_jpeg_quality: "JPEG 质量", "JPEG quality";
    s_version: "版本 {}", "Version {}";
    s_gpu: "图形接口：{}", "Graphics API: {}";
    s_gpu_skipped: "已跳过：{}（在这台电脑上启动失败过）", "Skipped: {} (failed to start on this PC before)";
    s_last_problem: "上次的问题：{}", "Last problem: {}";
    gpu_no_first_frame: "没有画出第一帧就退出或卡住", "Exited or froze before the first frame";
    gpu_no_adapter: "没有找到可用的显卡接口", "No usable graphics API was found";
    startup_failed: "LiteView 无法启动。\n\n{}\n\n详细记录在：{}", "LiteView could not start.\n\n{}\n\nDetails were saved in: {}";
    s_license: "MIT 开源许可。图标：Fluent UI System Icons（MIT）。", "MIT licensed. Icons: Fluent UI System Icons (MIT).";
    s_open_config: "打开配置文件所在目录", "Open settings folder";
    s_homepage: "项目主页", "Project page";
    s_coming: "下一个版本提供", "Coming in the next version";
    s_keys: "快捷键", "Shortcuts";
    keys_table: "操作|查看|编辑\n上一张 / 下一张|← → / PgUp PgDn / 鼠标侧键；按住连续翻|—\n第一张 / 最后一张|Home / End|—\n缩放|滚轮 / Ctrl+= / Ctrl+- / + -|同左\n适应窗口 / 100%|Ctrl+0 / Ctrl+1，双击切换|Ctrl+0 / Ctrl+1\n平移|左键拖动|空格 + 左键拖动 / 中键拖动\n全屏|F11|F11\n无边框（Windows）|Ctrl+B|—\n动图 播放 / 暂停|空格|—\n上一帧 / 下一帧|, / .|—\n旋转|Ctrl+R / Ctrl+Shift+R|同左（改像素）\n编辑 / 打码|E / M|—\n复制 / 剪切 / 粘贴|Ctrl+C / — / Ctrl+V|Ctrl+C / Ctrl+X / Ctrl+V\n复制文件路径|Ctrl+Shift+C|—\n全选 / 删除选区|—|Ctrl+A / Del\n裁剪到选区 / 调整大小|—|Ctrl+Shift+X / Ctrl+W\n撤销 / 重做|—|Ctrl+Z / Ctrl+Y\n保存 / 另存为|Ctrl+S / Ctrl+Shift+S|Ctrl+S / Ctrl+Shift+S\n删除文件|Del / Shift+Del|—\n重命名 / 图片信息|F2 / I|—\n打开 / 设置|Ctrl+O / Ctrl+,|—\n工具|—|选择 S、裁剪 C、铅笔 P、刷子 B、橡皮擦 E、颜料桶 G、取色器 I、文字 T、形状 U、打码 M\n圆圈 / 笔的大小|—|[ / ]\n交换主色 / 辅色|—|X\n完成编辑|—|Ctrl+Enter\nEsc|关闭窗口（全屏时先退出全屏）|依次取消；都没有时退出编辑", "Action|Viewing|Editing\nPrevious / next|← → / PgUp PgDn / mouse side buttons; hold to keep going|—\nFirst / last|Home / End|—\nZoom|Wheel / Ctrl+= / Ctrl+- / + -|Same\nFit / 100%|Ctrl+0 / Ctrl+1, double-click|Ctrl+0 / Ctrl+1\nPan|Drag|Space + drag / middle-drag\nFull screen|F11|F11\nBorderless (Windows)|Ctrl+B|—\nPlay / pause animation|Space|—\nPrevious / next frame|, / .|—\nRotate|Ctrl+R / Ctrl+Shift+R|Same (changes pixels)\nEdit / redact|E / M|—\nCopy / cut / paste|Ctrl+C / — / Ctrl+V|Ctrl+C / Ctrl+X / Ctrl+V\nCopy file path|Ctrl+Shift+C|—\nSelect all / delete selection|—|Ctrl+A / Del\nCrop to selection / resize|—|Ctrl+Shift+X / Ctrl+W\nUndo / redo|—|Ctrl+Z / Ctrl+Y\nSave / save as|Ctrl+S / Ctrl+Shift+S|Ctrl+S / Ctrl+Shift+S\nDelete file|Del / Shift+Del|—\nRename / image info|F2 / I|—\nOpen / settings|Ctrl+O / Ctrl+,|—\nTools|—|Select S, Crop C, Pencil P, Brush B, Eraser E, Fill G, Picker I, Text T, Shapes U, Redact M\nCircle / brush size|—|[ / ]\nSwap colors|—|X\nFinish editing|—|Ctrl+Enter\nEsc|Close the window (leave full screen first)|Cancel step by step, then leave editing";
    close: "关闭", "Close";
    borderless: "无边框", "Borderless";
    always_on_top: "窗口置顶", "Always on top";
    rotate_flip: "旋转和翻转", "Rotate and flip";
    file_menu: "文件", "File";
    window_menu: "窗口", "Window";
    toolbar_outside: "工具栏放不下时放到图片下面", "Toolbar below the image when it doesn't fit";
    toolbar_outside_tip: "无边框、窗口比底部工具栏还窄时，把工具栏放到图片下面单独的小窗口里，不再被截断。这个小窗口要多占一些内存，所以默认不开", "When borderless and the window is narrower than the bottom toolbar, the toolbar goes in a small window of its own under the image instead of being cut off. That window takes some extra memory, so this is off by default";
    minimize: "最小化", "Minimize";
    maximize: "最大化", "Maximize";
    restore_window: "还原", "Restore";
    borderless_tip: "无边框：鼠标移到顶部出现标题栏，拖图片移动窗口；Ctrl+B 恢复", "Borderless: the title bar appears at the top, drag the image to move the window; Ctrl+B to go back";
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
    p_smear: "涂抹", "Smudge";
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
    p_bold: "粗体", "Bold";
    p_italic: "斜体", "Italic";
    p_underline: "下划线", "Underline";
    p_strike: "删除线", "Strikethrough";
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
    a_registered: "已添加到系统「打开方式」。勾选不会取消已有的默认设置。", "Added to Open with. Changing this selection does not undo existing defaults.";
    a_not_registered: "尚未注册；仅勾选格式不会修改系统。", "Not registered. Selecting formats alone makes no system changes.";
    a_note: "先选想用 LiteView 打开的格式，再去 Windows 确认。已设好的不用重复选。", "Choose formats, then confirm in Windows. Formats already set need no further action.";
    a_packaged_note: "商店版已随安装登记图片格式。这里勾选的是要检查的范围，默认应用仍由你在 Windows 中确认。", "The Store package registers image formats at installation. Select a checklist here, then confirm defaults in Windows.";
    a_packaged_steps: "点击前往商店版 LiteView 的默认应用设置。若打开的是首页，请搜索 LiteView；与安装版并存时，请注意区分。", "Open default-app settings for the Store edition. If the overview opens, search for LiteView. Choose the Store edition if both editions are installed.";
    a_packaged_managed: "格式和右键菜单由 Windows 随商店包安装、更新和卸载，无需手动注册。", "Windows manages formats and context menus when installing, updating or uninstalling the Store package.";
    a_groups: "常用|更多格式|专业|相机 RAW", "Common|More|Professional|Camera RAW";
    a_open_settings: "前往系统设置", "Open Windows Settings";
    a_choose_formats: "选择图片格式", "Choose image formats";
    a_unregister: "移除打开方式注册", "Remove Open with registration";
    a_common: "常用格式", "Common formats";
    a_select_all: "全部格式", "All formats";
    a_clear: "清空选择", "Clear selection";
    a_is_default: " · 已默认", " · Default";
    a_pending: " · 待设置", " · Pending";
    a_progress: "已设为默认 {} / {} 种", "Default for {} / {} selected formats";
    a_pending_count: "还有 {} 种需要确认。返回本页后自动刷新。", "{} still need confirmation. Status refreshes when you return.";
    a_empty: "请至少选择一种格式。清空选择不会取消已有的关联。", "Select at least one format. Clearing this selection does not remove existing associations.";
    a_complete: "所选格式都已默认使用 LiteView。", "All selected formats already default to LiteView.";
    a_refresh: "刷新状态", "Refresh status";
    a_only_pending: "只看所选的待设置格式", "Only show selected pending formats";
    a_system_steps: "按钮会登记所选格式，不会自动接管默认应用。在系统里点扩展名并选 LiteView；若打开的是首页，先搜索 LiteView。", "The button registers selected formats, not defaults. In Settings, select each extension and choose LiteView. If the overview opens, search for LiteView first.";
    a_open_failed: "格式已登记，但未能打开系统设置。请手动进入「设置 → 应用 → 默认应用」，搜索 LiteView。详情：{}", "Formats registered, but Settings could not open. Go to Settings > Apps > Default apps and search for LiteView. Details: {}";
    a_remove_note: "这会移除 LiteView 登记的所有格式和右键菜单。若某些格式正默认使用 LiteView，请先在系统设置中改选其他应用；此操作不会替你选择新的默认应用。", "This removes all LiteView format registrations and context menus. If LiteView is the default, choose another app in Settings first; removal does not choose a replacement.";
    a_windows_only: "文件关联目前只支持 Windows。", "File associations are Windows-only for now.";
    a_removed: "已移除打开方式注册", "Open with registration removed";
    ask_title: "设为默认看图软件？", "Make LiteView your default image viewer?";
    ask_body: "先选择图片格式，再到 Windows 设置里确认。不会自动替换你现在的默认应用。", "Choose image formats, then confirm in Windows Settings. Your current defaults will not be replaced automatically.";
    ask_later: "以后再说", "Later";
    ask_never: "不再询问", "Don't ask again";
    print: "打印…", "Print…";
    pr_title: "打印", "Print";
    pr_printer: "打印机", "Printer";
    pr_default: "（默认）", " (default)";
    pr_paper: "纸张", "Paper";
    pr_orientation: "方向", "Orientation";
    pr_auto: "自动", "Auto";
    pr_portrait: "纵向", "Portrait";
    pr_landscape: "横向", "Landscape";
    pr_size: "尺寸", "Size";
    pr_fit: "适合页面", "Fit to page";
    pr_custom: "指定尺寸", "Custom";
    pr_actual: "实际大小", "Actual size";
    pr_width: "宽", "Width";
    pr_height: "高", "Height";
    pr_5in: "5 寸", "3.5×5 in";
    pr_6in: "6 寸", "4×6 in";
    pr_actual_dpi: "按图片记录的 {} DPI", "From the image's {} DPI";
    pr_no_dpi: "这张图没有记录分辨率，按适合页面打印", "This image has no recorded resolution; it prints fitted to the page";
    pr_copies: "份数", "Copies";
    pr_more: "更多设置", "More settings";
    pr_margin: "边距", "Margin";
    pr_properties: "打印机属性…", "Printer properties…";
    pr_print: "打印", "Print";
    pr_info: "打印尺寸 {} × {} mm，约 {} PPI", "Print size {} × {} mm, about {} PPI";
    pr_low_res: "清晰度偏低，打印出来可能发虚", "Low resolution: the print may look soft";
    pr_over: "超出可打印区域 {} mm，超出的部分会被裁掉", "Exceeds the printable area by {} mm; that part will be cut off";
    pr_use_fit: "改用适合页面", "Use Fit to page";
    pr_frame: "动图：打印第 {} 帧", "Animation: prints frame {}";
    pr_unprintable: "灰色是打印机印不到的边", "Grey edges are outside the printer's printable area";
    pr_loading: "正在读取打印机…", "Reading printers…";
    pr_reading: "正在读取打印机设置…", "Reading printer settings…";
    pr_none: "没有找到打印机。可在 Windows 设置 → 蓝牙和其他设备 → 打印机和扫描仪中添加", "No printer found. Add one in Windows Settings → Bluetooth & devices → Printers & scanners";
    pr_spooler: "Windows 打印服务（Print Spooler）没有运行", "The Windows print service (Print Spooler) is not running";
    pr_preparing: "正在准备图片…", "Preparing the image…";
    pr_sending: "正在交给打印机…", "Sending to the printer…";
    pr_sent: "已交给打印机", "Sent to the printer";
    pr_cancelled: "已取消打印", "Printing cancelled";
    pr_failed: "打印失败：{}", "Printing failed: {}";
    pr_file_filter: "打印输出", "Print output";
    pr_bad_margin: "边距太大，纸上没有地方可印", "The margin is too large: nothing fits on the paper";
    keys_print_row: "打印|Ctrl+P|Ctrl+P", "Print|Ctrl+P|Ctrl+P";
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
