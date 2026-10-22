# LiteView 隐私政策 / Privacy Policy

生效日期 / Effective date: 2026-10-22

## 中文

LiteView 是本地图片查看、编辑和打码工具。Microsoft Store 的发布者显示名称为“白描ice”，项目维护地址为 https://github.com/Lianzy-Baimiao/LiteView 。

### 图片与个人信息

- LiteView 不要求登录，不包含广告、用户行为分析或自动上传崩溃报告的服务。应用本身不向开发者发送图片、文件路径、使用记录或设备标识。
- 你打开的图片及其 EXIF、ICC 等元数据在本机处理。图片信息面板可展示相机、拍摄时间和 GPS；GPS 默认折叠。打开本机已映射的网络位置时，Windows 会按你的操作访问该位置。
- 复制、粘贴、保存、覆盖、重命名、删除和设置壁纸只在你操作时执行。图片缓存位于内存；保存或导出产生你选择的文件，设置壁纸会另外保存壁纸文件。复制的内容进入 Windows 剪贴板，剪贴板历史或跨设备同步由你的系统设置控制。
- 保存时可能按设置保留部分元数据。打码图片重新编码，并移除 EXIF 和嵌入缩略图等数据；ICC 色彩配置可能保留。请检查最终导出的图片，不要仅凭轻度模糊或马赛克假设内容无法识别。

### 设置与诊断

- 偏好设置、窗口位置等存放在本机用户目录。商店版使用该包的本地应用数据目录，独立安装版使用 `%APPDATA%\LiteView`，绿色版使用程序旁的 `config` 目录。
- 商店版设置由 Windows 管理，可能参与系统的应用数据备份；卸载通常会删除其本地应用数据。独立版卸载保留设置。用户图片不因卸载而由 LiteView 删除。
- 为了在启动失败后自动换用其他图形接口、并方便你反馈问题，1.1.0 起会在上述设置目录写三个小文件：`startup.json`（用了哪个图形接口、上次启动是否完成）、`last-crash.txt`（程序崩溃时的异常码和出错模块）、`errors.log`（程序内部错误，超过 256 KB 自动轮换）。它们不含图片内容，错误信息中可能出现文件路径；只保存在本机，不会自动上传。“设置 → 关于”会显示其中的摘要，可通过“打开配置文件所在目录”查看或删除。
- 完整调试日志仍默认关闭。用户或测试人员主动设置 `LITEVIEW_LOG` 时，日志写入其指定的本地文件，可能含文件路径和错误信息，不会自动上传。其他开发测试输出也仅在明确启用时写入指定位置。分享诊断前请检查并删除不愿公开的信息。

### Microsoft Store 与外部链接

Microsoft Store 的账号、购买（如适用）、分发、更新和系统诊断由微软按其隐私声明处理，LiteView 不控制这些服务。你主动打开项目网站、支持页面或系统解码扩展页面时，相应浏览器、商店和第三方服务适用各自的隐私政策。某些图片格式需要系统解码扩展，扩展的安装由用户决定。

### 联系与变更

隐私问题可通过 https://github.com/Lianzy-Baimiao/LiteView/issues 联系维护者。GitHub issue 是公开的，请勿提交密码、证件、私人图片或未经处理的敏感日志。如今后数据处理方式发生变化，本政策和生效日期会同步更新。

## English

LiteView is a local image viewer, editor and redaction tool. Its Microsoft Store publisher display name is 白描ice. The project is maintained at https://github.com/Lianzy-Baimiao/LiteView .

### Images and personal information

LiteView requires no account and includes no advertising, usage analytics or automatic crash-report upload service. The application does not send images, file paths, usage history or device identifiers to the developer. Images and metadata such as EXIF and ICC are processed locally. The information panel may display camera details, dates and GPS coordinates; GPS is collapsed by default. Windows may access network locations when you choose files stored there.

Copying, pasting, saving, overwriting, renaming, deleting and setting wallpaper occur at your request. Image caches are in memory; saving/exporting creates files you select, and setting wallpaper saves a wallpaper file. Copied data enters the Windows clipboard, whose history and cross-device synchronization follow your system settings.

Saving may retain some metadata according to your settings. Redacted images are re-encoded and EXIF/embedded thumbnails are removed; ICC color profiles may remain. Review the exported result: light blur or mosaic does not guarantee that information is unrecognizable.

### Settings and diagnostics

Preferences and window state are stored locally. The Store edition uses its package's local application-data folder; the standalone installer uses `%APPDATA%\LiteView`; the portable edition uses `config` beside the executable. Windows manages Store-edition settings and may include them in application-data backups; uninstalling normally removes that local data. The standalone uninstaller retains settings. LiteView does not delete your images during uninstall.

Starting with 1.1.0, LiteView writes three small files in the settings folder above so it can switch to another graphics API after a failed start and so you can report problems: `startup.json` (which graphics API was used and whether the last start completed), `last-crash.txt` (exception code and faulting module after a crash) and `errors.log` (internal errors, rotated above 256 KB). They contain no image content; error text may include file paths. They stay on this device and are never uploaded. Settings > About shows a summary, and "Open settings folder" lets you view or delete them.

Full debug logging remains off by default. If you explicitly set `LITEVIEW_LOG`, logs are written to the local file you specify and may contain paths and error details. Other development-test outputs are also written only when explicitly enabled. Nothing is uploaded automatically. Review and redact diagnostic files before sharing.

### Store, external services and contact

Microsoft handles Store accounts, purchases (if any), distribution, updates and system diagnostics under its own privacy statement. External websites and system-codec pages you choose to open are subject to the relevant browser, Store and service policies. Installing optional codecs is your choice.

Contact the maintainer through https://github.com/Lianzy-Baimiao/LiteView/issues . Issues are public: do not post credentials, identity documents, private images or unredacted sensitive logs. This policy and its effective date will be updated if data handling changes.
