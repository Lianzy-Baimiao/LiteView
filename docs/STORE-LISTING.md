# LiteView 商店提交记录

状态：2026-10-18 已核实 LiteView **通过微软认证**，用户随后自行点击“立即发布”。2026-10-22 经用户同意，已从 Microsoft Store 实际安装 **1.0.0.0 / SignatureKind=Store**，确认本机所在市场已能交付；完成测试后已卸载。不重复送审或点击发布，不据此宣称所有市场搜索收录均已完成。IARC 正式证书/ID 仍待独立核对。

## 1.3.1.0 更新（2026-10-26，已提交认证）

- 包：`LiteView-store-1.3.1.0-x64.msix`，SHA-256 `8b28cc706b4a2826a3c9d7da77d2f9f7eeb439fee7c85fe1b9588ac26d8b39c9`；来自 Release 运行 40259222552（标签 v1.3.1）的 `Store-submission-x64` artifact（一次性 runner 的 MSIX 安装、启动、升级、卸载回归通过）。身份 `ice.LiteView` / `CN=56079E3F-5861-4D9C-B9C3-93F76224C42E` / 1.3.1.0 / x64，未签名（由商店签名）。包内 EXE 与 GitHub v1.3.1 绿色版里的相同（`419543e…281d`）。
- 提交：2026-10-26 用户授权自行提交，临时 Edge 窗口自动登录 Partner Center。开始时 Submission 3（1.3.0.0）已上架、没有进行中的提交。新建 Submission 4 **1152921505702229955**：上传本包（概览显示 Validated，保留 runFullTrust 受限能力警告），1.3.0.0 随保存从提交中移除，设备仍只勾选 Windows 10/11 Desktop；中英文“此版本的新增功能”在 1.3.0 原文前加一行本版修复（394 / 1291 字符，重新加载后逐字一致；说明、功能、关键词、截图未改）；提交选项沿用“通过认证后立即发布”，runFullTrust 用途说明自动沿用原文；点击“提交进行认证”后总览显示“正在认证更新”。商店在认证通过前仍为 Submission 3（1.3.0.0）。满意度调查未代填。截图在忽略目录 `dist/store-1.3.1-submission/`。

## 1.3.0.0 更新（2026-10-24 提交认证，已上架）

- 包：`LiteView-store-1.3.0.0-x64.msix`，SHA-256 `e02ce2e586193ff11a8f5fae2c303f9fa63ea1e4298a9dd09416cf9b88ed747e`；来自 Release 运行 40040534755 的 `Store-submission-x64` artifact（一次性 runner 的 MSIX 安装、启动、升级、卸载回归通过）。身份 `ice.LiteView` / `CN=56079E3F-5861-4D9C-B9C3-93F76224C42E` / 1.3.0.0 / x64，未签名（由商店签名）。包内 EXE 与 GitHub v1.3.0 相同（`b29f9c6…481c`）。
- 提交：2026-10-24 用户在临时 Edge 窗口登录 Partner Center 后授权执行。开始时 Submission 2（1.1.0.0）已上架、没有进行中的提交。新建 Submission 3 **1152921505702218467**：上传本包（Validated，保留 runFullTrust 受限能力警告），1.1.0.0 随保存从提交中移除，设备仍只勾选 Windows 10/11 Desktop；中英文“此版本的新增功能”改为涵盖 1.2.0 与 1.3.0（294 / 928 字符，重新加载后逐字一致；说明、功能、关键词、截图未改）；提交选项沿用“通过认证后立即发布”，runFullTrust 用途说明沿用 1.0.0 原文；点击“提交进行认证”后总览显示“正在认证更新”。商店在认证通过前仍为 Submission 2（1.1.0.0）。1.2.0.0 没有上传，由本包取代。截图在忽略目录 `dist/store-1.3.0-submission/`。

## 1.2.0.0 更新（2026-10-23，未上传，由 1.3.0.0 取代）

- 包：`LiteView-store-1.2.0.0-x64.msix`，SHA-256 `32ae4cf90b93f2cc992982f11f4cd987e44871b8116cfc820e65c3014a889e1a`；来自 Release 运行 40017626731 的 `Store-submission-x64` artifact（一次性 runner 的 MSIX 安装、启动、升级、卸载回归通过）。身份 `ice.LiteView` / `CN=56079E3F-5861-4D9C-B9C3-93F76224C42E` / 1.2.0.0 / x64，未签名（由商店签名）。包内 EXE 与 GitHub v1.2.0 相同（`c541abe…5c4f`）。
- 状态：GitHub v1.2.0 已发布为 Latest。本记录写入时**尚未上传或送审**。Partner Center 同一时间只能有一个进行中的提交：上传前先确认 1.1.0.0（Submission 2）已认证发布或已结束。

## 1.1.0.0 更新（2026-10-22 提交认证，已上架）

- 包：`LiteView-store-1.1.0.0-x64.msix`，SHA-256 `bfc57c4b63a3d700b888ce69b7c2aed647e19f74663d431366ad6e9be63a8037`；来自 Release 运行 39815282764 的 `Store-submission-x64` artifact（MakePri / MakeAppx 通过，一次性 runner 的 MSIX 安装、启动、升级、卸载回归通过）。身份 `ice.LiteView` / `CN=56079E3F-5861-4D9C-B9C3-93F76224C42E` / 1.1.0.0 / x64，未签名（由商店签名）。包内 EXE 与 GitHub v1.1.0 相同（`6037536…75c3`）。
- 提交：2026-10-22 用户登录 Partner Center 并上传本包（Submission 2 **1152921505702205787**，包页面仅 1.1.0.0、Validated，设备仍只勾选 Windows 10/11 Desktop，保留 runFullTrust 受限能力警告）。随后按用户授权填写中英文“此版本的新增功能”（215 / 610 字符，重新加载后逐字一致；说明、功能、关键词未改），提交选项改为**通过认证后立即发布**，runFullTrust 用途说明沿用 1.0.0 原文，并点击“提交进行认证”。总览显示“正在认证更新”，商店当前仍为 Submission 1（1.0.0.0），认证通过后才会推送 1.1.0.0。满意度调查未代填。截图在忽略目录 `dist/store-1.1.0-submission/`。

## 实际交付（2026-10-22）

- `winget install --id 9PCG7D394JFC --source msstore` 成功安装包族 `ice.LiteView_rz8m2sa64m4em`，版本 1.0.0.0，签名类型 Store。
- 商店安装后的 EXE SHA256 为 `21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a`，与 GitHub 公开版相同。
- 该次真实包身份和 ProgID 文件激活测试发生在 **25H2 / 26200.9711**；测试后卸载，未改变 GitHub 版默认关联。原始证据为忽略目录 `dist/heic-repro-20261021/store-identity/`、`store-activation/`，测试判定的局限及后续补强见 `STARTUP-CRASH-20261020.md`。
- 本机随后重启到 26H2 的独立 EXE 回归不是再次商店安装，不替代 Win10/11 完整人工、无障碍及其他市场验收。

## 提交背景

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

## 发布进度（2026-10-18）

- 产品 **LiteView / 9PCG7D394JFC**，Submission 1 **1152921505702157204**，已认证包为 `LiteView-store-1.0.0.0-x64.msix`。核对的是 LiteView，不是 FeatherMD。
- 发布前 Partner Center 明确显示“你的产品已通过认证过程”，状态为“准备发布”，“立即发布”可用。只读定价页核对：全球 **240** 个现有市场、公开可发现、**CNY 0**；原有未来市场选项仍启用，未作修改。
- 用户亲自点击“立即发布”后，重新核对显示“正在发布”，并说明“你的产品已通过认证过程。它已开始发布，并将很快在 Microsoft Store 中提供”；进度条为提交/预处理/认证完成、发布进行中。助手没有重复点击、撤销或重建/替换提交包。
- 产品标识展开区的 Microsoft Store 深层链接与 Web Store URL 仍显示“产品上线后可用”。因此不提供虚构的已上线链接、不承诺立即搜索可见，也不把本次状态检查说成商店签名包已下载安装验收。下一步只需核对实际发布完成和公开商店交付，不重新送审当前版本。
- 状态证据保存在忽略的 `dist/store-publication-20261018/`：`approved-awaiting-publication.txt/.jpg`、`pricing-before-publication.txt`、`publishing.txt/.jpg`。本次没有修改 GitHub v1.0.0 发行附件、运行本机原生测试或改动 FeatherMD；没有创建自动监控。

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
- 10-17 当时尚未完成：Windows 10/11 商店版人工验收、目标桌面环境复测、微软认证及最终发布。后续认证/实际交付见本文开头，人工验收仍有剩余项。Server 2025 的补充 WACK 结果见 `WACK-VALIDATION-NOTES.md`；当次没有撤回送审、替换包或点击发布。
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
- 免费定价与年龄分级已确认并保存；认证及本机交付已核实，后续只复核未确认的证书/市场状态，不预填虚构答案或重复点击发布。
