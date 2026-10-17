# LiteView 商店提交草稿

状态：供用户确认，尚未填写或提交 Partner Center；页面预留不代表已上架。

- 产品名称：LiteView
- Store ID：9PCG7D394JFC
- 产品版本：1.0.0；MSIX 版本：1.0.0.0
- 平台：Windows 10 2004（19041）及以上 / Windows 11，x64
- 建议分类：照片与视频（以后台可选项为准）
- 建议定价：免费（提交前确认）
- 支持地址：https://github.com/Lianzy-Baimiao/LiteView/issues
- 隐私政策拟用地址：https://github.com/Lianzy-Baimiao/LiteView/blob/main/docs/PRIVACY.md
- 隐私政策需先提交到公开仓库并验证可匿名访问，才能填写为有效公开地址。

## 中文描述

看图、简单编辑、打码，不用来回换软件。

LiteView 是一个离线图片工具。打开图片后，可以浏览同文件夹的其他图片，用滚轮缩放，拖动查看细节。GIF、APNG 和动态 WebP 可以播放、暂停、调速和逐帧查看，也能把其中一帧单独保存。

需要改图时，直接进入编辑：裁剪、调整尺寸、旋转，或者加上文字、箭头和简单图形。想遮住电话号码、聊天记录等内容，可以框选或用画笔涂抹马赛克、模糊。保存打码图片时会移除 EXIF 和嵌入缩略图；请检查导出效果，轻度打码并不保证内容无法识别。

不需要账号，没有广告。图片在本机处理，不会由 LiteView 自动上传。界面支持中文和英文，并跟随系统切换深浅色。

使用说明：
- 动图支持查看和导出帧，不支持动画编辑。
- HEIC、HEIF、AVIF 等格式可能需要另外安装系统解码扩展。
- RAW 主要显示内嵌预览，PSD 主要读取合并图，不提供专业 RAW 显影或 PSD 图层编辑。
- 默认看图应用仍需你在 Windows 设置中确认。

## English description

View, make a quick edit, and redact an image without switching apps.

LiteView is an offline image viewer with folder navigation, wheel zoom and animated-image playback. Pause GIF, APNG or animated WebP files, step through frames, or save an individual frame.

Use the built-in editor to crop, resize, rotate, add text, arrows and simple shapes. Cover private details with a mosaic or blur brush, or select a rectangular area. Saving a redacted image removes EXIF and embedded thumbnails. Review the result: light blur or mosaic does not guarantee that details are unrecognizable.

No account and no advertising. Images are processed locally and are not automatically uploaded by LiteView. The interface supports English and Chinese, with light and dark themes.

Animated images are view-only, with frame export. HEIC/HEIF/AVIF may require separately installed system codecs. RAW support focuses on embedded previews; PSD support reads the merged image, not editable layers. Windows default-app choices remain under your control.

## 图片与审核说明

- 可用真实功能截图：`docs/images/preview-viewer.png`、`preview-editor.png`、`preview-redaction.png`。这些拍摄于开发版；上传前需核对最终商店版本和后台尺寸要求，不能假称为当前 MSIX 的实机截图。
- `runFullTrust` 用途：Rust/egui Win32 桌面应用在 medium integrity 下运行，读取用户主动选择的本机图片、编辑并保存、访问剪贴板和使用文件对话框；不要求管理员权限，不安装后台服务。
- 文件类型由 MSIX 清单登记。经典右键提供编辑/打码；不宣称 Windows 11 首层现代菜单扩展。
- 无账号、服务器、订阅、应用内购买或登录测试凭据。
- 年龄评级问卷、市场、定价、版权声明和最终提交需按实际情况由用户确认，不预填虚构答案。
