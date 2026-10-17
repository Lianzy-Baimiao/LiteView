# Store 审核操作说明

以下是提交草稿使用的审核指引，不是测试通过报告。软件不需要账号、订阅、付款或测试登录凭据；无需提供用户个人图片。

2026-10-17：下方 English reviewer notes 部分共 1807 字符，已保存到 Partner Center 的 Additional Testing Info；重新加载后逐字核对一致，凭据表保持为空。没有点击 Review and publish 或提交认证。

## English reviewer notes

LiteView is an offline Windows desktop image viewer/editor. No account, login credentials, subscription, ads or online service are required. Please use non-sensitive sample images and edit copies, not originals.

1. Launch LiteView from Start. Open a local PNG/JPEG or drag one onto the window. Use the wheel to zoom and the arrow keys to browse images in the same folder.
2. Press E to edit. Try crop, text or arrow tools, then save a sample copy. Press M from the viewer to open the redaction tools. Rectangle mosaic and blur are available. Review the exported image; light redaction is not a guarantee of secrecy.
3. Open an animated GIF. Space toggles playback; comma and period step backward/forward. Animation editing is not supported; individual frames can be exported.
4. Settings (Ctrl+,) provides English/Chinese and light/dark themes. Image processing stays on the device. The app does not automatically upload pictures or require administrator elevation.
5. Windows manages the packaged file associations. Choosing LiteView as the default viewer requires the user's confirmation in Windows Settings. It does not overwrite protected UserChoice entries.

The runFullTrust capability launches the Rust/egui Win32 executable and supports native windows, GPU rendering, local file dialogs and clipboard access. There are no services, drivers, startup tasks or downloaded executable code. Package settings use the package LocalFolder.

Store screenshots show the actual 1.0.0 viewer/editor, with original demo artwork and a fictional contact card. The capture executable is byte-identical to the MSIX payload, but these captures are not claimed as installed-package or WACK test evidence. HEIC/HEIF/AVIF can depend on optional system codecs; RAW reads embedded previews and PSD reads the merged image.

## 提交边界

- 安装、升级、卸载及 AUMID 激活的 CI 证据见 `RELEASE-1.0.0-VALIDATION.md`；本机新增的八次截图回放只覆盖说明中的基础界面场景，并不是以上所有步骤的人工验收。
- WACK 尚未运行；准备方法见 `WACK-VALIDATION-NOTES.md`。
- IARC 年龄分级等待用户授权向 IARC 共享发布者显示名称和开发者账号邮箱，尚未填写或提交。
- 商店仍保留人工发布开关；保存审核说明不等于提交认证。
