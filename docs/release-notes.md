# LiteView v1.9.1 — GIF 复制到 QQ 保留动画

修复查看 GIF 时右键复制或 Ctrl+C 后，粘贴到 QQ 变成静态单帧的问题。现在复制完整 GIF 原文件，保留全部帧、帧延时和循环次数。用户已在 QQ 中实测确认修复。

## 本版更新

- **复制完整 GIF**：播放中、暂停、逐帧查看或旋转后复制，都保留原动图；需要单帧图片时仍可使用「导出当前帧」。
- **清除旧静态格式**：复制 GIF 时清掉剪贴板残留的 PNG / 位图，避免聊天软件优先粘贴成静态图或上一张图片。
- **普通图片复制照常**：透明图片的透明度、查看时的旋转，以及编辑后的像素继续按原方式复制。

## 验证

真实复制入口与 Windows 系统剪贴板回归覆盖两帧 GIF、仅缓存一帧、中文路径、暂停与旋转、旧静态格式清除、粘贴回 LiteView、源文件失效和透明静态图复制；核对复制前后 GIF 文件字节完全一致。该测试已接入 Windows CI 和 Release。另完成本地 Release 测试、Clippy、运行库检查与隔离桌面启动检查。

发布工作流要求 Windows x64、Linux x64、macOS arm64 / x64 全部通过 release Clippy、测试、构建和打包检查，并通过一次性 Windows runner 的安装包与 MSIX 生命周期回归，才创建正式 Release。

## 下载与运行

- Windows 10 / 11 x64：`windows-x64.zip` 为绿色版，解压后运行 `LiteView.exe`；`windows-x64-setup.exe` 为当前用户安装包，不需要管理员权限。也可从 [Microsoft Store](https://apps.microsoft.com/detail/9PCG7D394JFC) 安装，商店更新需微软审核后推送。
- Linux x64：解压 `linux-x64.tar.gz` 后运行 `./LiteView`，构建基线为 Ubuntu 22.04。
- macOS：按机器选择 `macos-arm64.tar.gz`（Apple Silicon）或 `macos-x64.tar.gz`（Intel），将 `LiteView.app` 拖入 Applications。
- 发布包附 `SHA256SUMS.txt`、安装说明和第三方许可证。绿色版通过 `LiteView.portable` 标记将配置保存在程序旁的 `config` 目录。GitHub Windows 包没有代码签名，macOS 包没有公证。

## 已知限制

- GIF 按原文件复制，查看时的旋转不改写动画；复制后粘贴前，请保留原文件。其它动图格式仍按当前帧复制。
- 窗口临时变大时，透明的地方也算 LiteView 的窗口：点那里只是关掉菜单或对话框，不会点到后面的窗口；透明处在真实桌面上的显示和变大那一下还没有在多种显卡上实测。运行中按 Ctrl+B 换成的无边框窗口不能透明，变大时整块铺背景。
- 透出桌面时，点在透明处仍落在 LiteView 上；动图只看第一帧判断有没有透明。
- 设置在 Linux / macOS 上仍画在图片窗口里。
- 调色板 PNG 保存旋转后存成 RGB(A)，文件会变大；少数由系统组件解码的 TIFF / JPEG，保存旋转后在 LiteView 里看不出方向变化（文件本身正确）。
- WebM 只在 Windows 上能打开，需要系统 VP9 扩展；不放声音，超过 64 MB 的文件不打开。HEIC / AVIF 等格式依赖系统解码扩展。
- 大图解码、100% 查看、编辑、复制和打印会临时保留原图，内存占用高于适应窗口时；16 位 TIFF、HDR / EXR 可能需要额外缓冲。
- 打印目前只在 Windows 上提供，实体打印机的边距、份数和异常状态尚未在本机验证；“交给打印机”表示进入 Windows 打印队列，不代表纸已印完。
- 普通窗口较矮时菜单需要滚动；连续翻页的等待间隔暂不可设置。Linux / macOS 的原生桌面操作尚未完整实测。
