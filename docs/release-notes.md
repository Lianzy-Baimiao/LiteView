# LiteView v1.8.0 — 拖图片就能移动窗口

Windows 普通窗口现在也能直接拖图片移动窗口：整张图片都看得见时，左键拖动即可，不必找标题栏；图片超出窗口时，左键拖动继续平移图片。感谢 [@suifenggg 在 Issue #4 中的建议](https://github.com/Lianzy-Baimiao/LiteView/issues/4)。

## 本版更新

- **整图可见时拖窗，放不下时平移**：判断依据是图片是否超出画布，不以 100% 缩放为界。大照片即使缩小到 50% 也可能需要平移；小图放大超过 100% 后仍能完整显示，就可以拖动窗口。判断同时考虑屏幕 DPI 和图片旋转。
- **Shift + 拖动强制移动窗口**：普通窗口也支持；同一次拖动开始后，中途按下或松开 Shift 不会改变用途。开始拖窗时清除残余图片惯性，避免图片继续滑动。
- **窗口状态与操作提示**：普通窗口最大化时不从画布拖出还原；全屏、编辑、菜单或对话框打开时不从画布启动拖窗。无边框最大化时保留原有拖出还原行为。可拖动时显示抓手指针，设置的快捷键页补充中英文操作说明和悬停提示。

## 验证

Windows 输入回归覆盖图片显示尺寸、100% 边界、DPI、旋转、Shift、拖动中途改变修饰键、最大化/全屏、菜单/设置、空画布及双击。桌面实测覆盖普通窗口连续拖动、大图平移、小图放大到约 201% 后拖窗、编辑选区、最大化、全屏和无边框；中英文新增快捷键行已检查。

发布工作流要求 Windows x64、Linux x64、macOS arm64 / x64 全部通过 release Clippy、测试、构建和打包检查，并通过一次性 Windows runner 的安装包与 MSIX 生命周期回归，才创建正式 Release。此更新的拖窗功能仅在 Windows 启用。

## 下载与运行

- Windows 10 / 11 x64：`windows-x64.zip` 为绿色版，解压后运行 `LiteView.exe`；`windows-x64-setup.exe` 为当前用户安装包，不需要管理员权限。也可从 [Microsoft Store](https://apps.microsoft.com/detail/9PCG7D394JFC) 安装，商店更新需微软审核后推送。
- Linux x64：解压 `linux-x64.tar.gz` 后运行 `./LiteView`，构建基线为 Ubuntu 22.04。
- macOS：按机器选择 `macos-arm64.tar.gz`（Apple Silicon）或 `macos-x64.tar.gz`（Intel），将 `LiteView.app` 拖入 Applications。
- 发布包附 `SHA256SUMS.txt`、安装说明和第三方许可证。绿色版通过 `LiteView.portable` 标记将配置保存在程序旁的 `config` 目录。GitHub Windows 包没有代码签名，macOS 包没有公证。

## 已知限制

- 本版没有重新实测跨不同 DPI/负坐标副屏拖动或系统贴边分屏；继续使用已有的原生拖窗与 Windows 11 DPI 修正。Windows 10 没有这层 DPI 修正。
- 装了 PowerToys FancyZones 时，按住 Shift 拖窗口可能贴进其分区，这是 FancyZones 的功能。
- 无边框只在 Windows 上；极小图片周围仍可能有背景，跨 DPI 拖动途中窗口会先采用系统建议大小，松开后再包住图片。
- WebM 只在 Windows 上能打开，需要系统 VP9 扩展；不放声音，超过 64 MB 的文件不打开。HEIC / AVIF 等格式依赖系统解码扩展。
- 写好的文字仅在当前编辑会话、对应像素未被覆盖时可以再次拿起；记忆的文字像素有 64 MB 上限。
- 大图解码、100% 查看、编辑、复制和打印会临时保留原图，内存占用高于适应窗口时；16 位 TIFF、HDR / EXR 可能需要额外缓冲。
- 打印目前只在 Windows 上提供，实体打印机的边距、份数和异常状态尚未在本机验证；“交给打印机”表示进入 Windows 打印队列，不代表纸已印完。
- 菜单受窗口大小限制，较矮窗口需要滚动；连续翻页的等待间隔暂不可设置。Linux / macOS 的原生桌面操作尚未完整实测。

完整开发与验证依据见 [Issue #4 实施记录](https://github.com/Lianzy-Baimiao/LiteView/blob/v1.8.0/docs/ISSUE-4-WINDOW-DRAG-RESEARCH.md)。
