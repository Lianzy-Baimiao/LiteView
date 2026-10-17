# LiteView 商店提交记录

状态：2026-10-17 用户在 Partner Center 自行提交首次上架申请。随后核对后台：产品状态为“正在认证”，提交步骤已完成、预处理正在进行、认证和发布步骤尚未开始。尚未通过认证或上架。

2026-10-17：GitHub v1.0.0 已作为正式版 / Latest 发布。对应的 MSIX 已在一次性 Windows runner 完成安装、真实 AUMID 启动、独立配置、升级和卸载检查，并下载核对 SHA-256；经用户确认上传商店草稿后，后台显示 Validated。未签名候选不是公开侧载安装包。当前版本的中英文截图已补齐，Store 一览显示“完成”；年龄分级已保存，Microsoft Store 为 3+。用户已提交微软审核；送审后补跑的 Server 2025 WACK 总体 PASS、13 必需项全过，保留 1 可选失败。本地 Windows 10/11 商店版人工验收仍未完成，不能因已送审或 CI 检查通过而将其标为通过。证据见 `RELEASE-1.0.0-VALIDATION.md`。

- 产品名称：LiteView
- Store ID：9PCG7D394JFC
- 产品版本：1.0.0；MSIX 版本：1.0.0.0
- 平台：Windows 10 2004（19041）及以上 / Windows 11，x64
- 已保存分类：照片 + 视频
- 已保存定价：免费，基准货币 CNY，零售价 0（重新打开页面核对通过）
- 支持地址：https://github.com/Lianzy-Baimiao/LiteView/issues
- 隐私政策地址：https://raw.githubusercontent.com/Lianzy-Baimiao/LiteView/main/docs/PRIVACY.md
- 隐私政策已提交到公开仓库，并于 2026-10-17 验证可匿名访问；商店属性中已填写。

## 提交资料及进度（2026-10-17）

- 用户最初授权上传 1.0.0 商店包、填写免费上架草稿而暂不送审；随后用户在后台自行提交。此次跟进只核验状态，没有撤回认证或更改发布设置。
- 发布方式：提交后后台仍显示“单击‘立即发布’时，产品将开始发布”，手动发布暂缓选项仍有效。审核通过不等于自动上架。
- 包：上传 `LiteView-store-1.0.0.0-x64.msix`，SHA-256 为 `2130cb363d94fce37d06f0cef4222dc84795c408a4bda6d0c8a91103eb925b06`；后台显示 `Validated` / 完成。识别为 x64、Windows.Desktop、最低 10.0.19041.0；仅选桌面设备，未授权自动拓展至未来设备系列。
- 定价：提交前核对为 CNY 0；保留后台默认的全球 240 个市场与未来市场选项。此次跟进没有更改市场，不把中国大陆开发者身份等同于只在中国销售。
- 属性：照片 + 视频、公开隐私政策、项目网站和 GitHub Issues 支持地址已保存。隐私问题包含本地访问个人信息，因应用可能打开私人图片和 GPS/EXIF，保留“是，我的产品使用个人信息”，不将“无上传”误写为“完全不使用个人信息”。未填写私人电话、地址，也未宣称完成无障碍认证。
- 商店文案：中文（中国）和英语（美国）的描述、简短描述、各五项功能和各七个关键词已保存，并重新进入页面核对。首次上架的“此版本的新增功能”留空。
- 商店截图：中英文各四张 1500 × 975 PNG（看图、文字箭头、马赛克、GIF 逐帧），均已上传并保存对应语言说明。保存后总览的 Store 一览从“未完成”变为“完成”。截图来自正式 1.0.0 EXE，和 MSIX 内 EXE 字节相同，但不冒充已安装 MSIX 的界面验收；文件与来源见 `images/store-v1.0.0/README.md`。
- 提交选项：已保存 runFullTrust 用途说明；设置为“除非我选择‘立即发布’，否则不发布此提交”。该开关不代替最终提交审核的用户确认。
- 审核说明：在 Additional Testing Info 保存 1807 字符英文操作指引，重新加载后逐字一致。包含离线/无账号、使用非敏感样本、编辑与打码、动图、语言主题、用户确认默认应用及解码限制；凭据表保持为空。原文见 `STORE-REVIEW-NOTES.md`，没有声称 WACK 或商店认证已通过。
- 年龄分级：用户授权 IARC 信息共享后，已按应用实际功能填写问卷；用户在后台完成最后确认并保存。提交前重新进入分级页确认结果已保存，而非预览。详细结果及提交前核对的 ID 状态见下节。
- 尚未完成：Windows 10/11 商店版人工验收、目标桌面环境复测、微软认证及最终发布。Server 2025 的补充 WACK 结果见 `WACK-VALIDATION-NOTES.md`；本轮没有撤回送审、替换包或点击发布。
- 后台 `runFullTrust` 警告表示仍需微软批准受限能力；包的 `Validated` 不代表权限已获批或商店认证通过。

### 已保存的年龄分级（IARC 10.3）

- 应用类型：其他所有应用类型（图片工具，不是游戏或社交/通信应用）。
- 发行包内分级相关内容、应用内用户内容交互、推荐/推广的联机内容、有年龄限制的商品/活动、向其他用户共享当前精确位置、应用内数字商品购买、现金/数字资产奖励、浏览器/搜索引擎、主要为新闻/教育产品：均按实际功能回答“否”。没有声明直接取得其他分级委员会分级或计划通过物理介质发行。
- 上述回答不等于不读取本地个人信息：用户图片和 EXIF/GPS 仍可能包含私人信息，原有隐私说明不变。本地查看/保存/复制图片不等于应用提供用户间内容交流服务，用户主动打开的文件也不是随应用发行或由应用推荐的内容。

| 分级系统 | 后台结果 | 来源 |
| --- | --- | --- |
| Microsoft Store | 3+ | Microsoft 映射 |
| IARC 全局 | 3+ | IARC |
| ESRB（美国） | E / 所有人 | IARC |
| PEGI（欧洲） | 3+ | IARC |
| USK（德国） | 所有人 | IARC |
| DJCTQ（巴西） | L / 所有年龄段 | IARC |
| PCBP（俄罗斯） | 0 / 所有年龄段 | IARC |
| CCC（智利） | TE / 所有年龄段 | Microsoft 映射 |

提交前最后核对时，后台“当前分级 ID”显示“待定”；提交后未重新核验该 ID，不宣称正式 IARC 证书已签发。已保存分级不代表商店认证通过。产品功能改变而影响问卷答案时，需要重新填写。

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

- 已上传当前功能截图：`docs/images/store-v1.0.0/`，包含中英文各四张及 SHA-256 清单。旧 `preview-*.png` 仅作为开发版历史图片保留，没有用于本次商店提交。
- `runFullTrust` 用途：Rust/egui Win32 桌面应用在 medium integrity 下运行，读取用户主动选择的本机图片、编辑并保存、访问剪贴板和使用文件对话框；不要求管理员权限，不安装后台服务。
- 文件类型由 MSIX 清单登记。经典右键提供编辑/打码；不宣称 Windows 11 首层现代菜单扩展。
- 无账号、服务器、订阅、应用内购买或登录测试凭据。
- 免费定价与年龄分级已确认并保存，用户已自行提交认证；等待审核反馈并在正式发布前复核资料，不预填虚构答案或自动点击发布。
