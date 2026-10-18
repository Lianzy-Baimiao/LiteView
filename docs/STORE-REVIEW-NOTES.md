# Store 审核操作说明

以下是提交草稿使用的审核指引，不是测试通过报告。软件不需要账号、订阅、付款或测试登录凭据；无需提供用户个人图片。

2026-10-17：下方 English reviewer notes 部分共 1807 字符，已保存到 Partner Center 的 Additional Testing Info；重新加载后逐字核对一致，凭据表保持为空。保存说明时未送审；同日用户随后在后台自行提交，核对状态已进入预处理。

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
- 送审后已补充 Server 2025 runner 的 WACK：总体 PASS，13 必需项全过，保留 1 可选失败；没有修改已提交的上方英文说明。它不代替 Windows 10/11 实机或商店认证，见 `WACK-VALIDATION-NOTES.md`。
- 随后补齐已安装包的四场景 GUI 回放及真实 AUMID 诊断，使用实际 WinRT LocalFolder；证据见 `validation/store-gui-1.0.0-20261017/README.md`。未将这些补测资料重新提交后台或改变当前认证流程。
- IARC 年龄分级已于 2026-10-17 经用户最后确认保存，Microsoft Store / IARC / PEGI 为 3+、ESRB 为 E；送审前核对分级 ID 为“待定”。详见 `STORE-LISTING.md`，不冒充正式分级证书或商店认证结果。
- 2026-10-18 已从 Partner Center 核实认证通过；用户自行点击“立即发布”后状态为“正在发布”。尚未核实公开商店交付完成，详见 `STORE-LISTING.md`。此结果不将既有未完成的本地人工验收倒记为通过，也没有修改已送审的英文说明。
