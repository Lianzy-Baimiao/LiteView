# LiteView 商店提交草稿

状态：用户确认后，已在 Partner Center 保存首次上架草稿；没有提交认证或发布。

2026-10-17：GitHub v1.0.0 已作为正式版 / Latest 发布。对应的 MSIX 已在一次性 Windows runner 完成安装、真实 AUMID 启动、独立配置、升级和卸载检查，并下载核对 SHA-256；未签名候选保存在 Actions artifact，后续经用户确认上传商店草稿，不是公开侧载安装包。尚缺 Windows 10/11 商店版人工验收、WACK、最终资料确认及微软审核。证据见 `RELEASE-1.0.0-VALIDATION.md`。

- 产品名称：LiteView
- Store ID：9PCG7D394JFC
- 产品版本：1.0.0；MSIX 版本：1.0.0.0
- 平台：Windows 10 2004（19041）及以上 / Windows 11，x64
- 已保存分类：照片 + 视频
- 已保存定价：免费，基准货币 CNY，零售价 0（重新打开页面核对通过）
- 支持地址：https://github.com/Lianzy-Baimiao/LiteView/issues
- 隐私政策地址：https://raw.githubusercontent.com/Lianzy-Baimiao/LiteView/main/docs/PRIVACY.md
- 隐私政策已提交到公开仓库，并于 2026-10-17 验证可匿名访问；商店属性中已填写。

## 后台草稿进度（2026-10-17）

- 用户确认范围：上传 1.0.0 商店包、填写免费上架草稿，暂不提交审核。
- 包：上传 `LiteView-store-1.0.0.0-x64.msix`，SHA-256 为 `2130cb363d94fce37d06f0cef4222dc84795c408a4bda6d0c8a91103eb925b06`；后台显示 `Validated` / 完成。识别为 x64、Windows.Desktop、最低 10.0.19041.0；仅选桌面设备，未授权自动拓展至未来设备系列。
- 定价：CNY 0；保留后台默认的全球 240 个市场与未来市场选项。市场范围在实际提交前仍应由用户复核，不把中国大陆开发者身份等同于只在中国销售。
- 属性：照片 + 视频、公开隐私政策、项目网站和 GitHub Issues 支持地址已保存。隐私问题包含本地访问个人信息，因应用可能打开私人图片和 GPS/EXIF，保留“是，我的产品使用个人信息”，不将“无上传”误写为“完全不使用个人信息”。未填写私人电话、地址，也未宣称完成无障碍认证。
- 商店文案：中文（中国）和英语（美国）的描述、简短描述、各五项功能和各七个关键词已保存，并重新进入页面核对。首次上架的“此版本的新增功能”留空。
- 提交选项：已保存 runFullTrust 用途说明；设置为“除非我选择‘立即发布’，否则不发布此提交”。该开关不代替最终提交审核的用户确认。
- 尚未完成：当前商店版截图、IARC 年龄评级、Windows 10/11 人工验收、WACK 和最终资料确认。中英文列表因没有截图仍显示“未完成”；没有用旧开发版截图冒充新版本截图。
- 后台 `runFullTrust` 警告表示仍需微软批准受限能力；包的 `Validated` 不代表权限已获批或商店认证通过。

### 已保存的检索与简短文案

中文简短描述：看图、简单编辑和打码，不用来回换软件。离线处理，无需账号，无广告。

中文关键词：图片查看器、图片编辑、马赛克、动图、裁剪、照片浏览、截图打码。

English short description: View, make a quick edit, and redact an image without switching apps. Works offline, with no account or advertising.

English keywords: image viewer; photo editor; redaction; mosaic; animated GIF; crop; offline.

### 已保存的受限能力说明

后台限制 500 字符，保存内容如下：

> LiteView is a Rust/egui Win32 image viewer/editor, packaged as packagedClassicApp at mediumIL. runFullTrust launches the desktop EXE and enables native windows, GPU rendering, file dialogs and clipboard. It reads user-selected images/folders and edits/saves files at the user's request. No elevation, services, drivers, startup tasks, downloaded code, accounts, ads or telemetry. Processing is local. MSIX owns associations; settings use package LocalFolder.

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
- 免费定价已由用户确认并保存；年龄评级问卷、市场、版权声明和最终提交仍需按实际情况由用户确认，不预填虚构答案。
