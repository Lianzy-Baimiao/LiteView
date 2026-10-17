# 安装与运行 / Installation

## Windows 10 / 11 (x64)

- **绿色版**：解压 windows-x64.zip，运行 LiteView.exe。包内的 LiteView.portable 标记使配置保存在程序旁的 config 目录。
- **安装版**：运行 windows-x64-setup.exe，默认仅为当前用户安装到 %LOCALAPPDATA%\Programs\LiteView，不需要管理员权限。可选桌面快捷方式及格式注册。
- 在软件「设置 → 文件关联」中选「常用格式」或「全部格式」，点「前往系统设置」，再按扩展名确认使用 LiteView。勾选是本次要设置的范围，不代表已成为默认应用；页面显示实际完成数量，可只看待设置项，返回后自动刷新。
- 注册为增量操作：切换勾选不会移除以前注册的格式或破坏已有默认设置。「移除打开方式注册」才会清理 LiteView 的注册，并需要再次确认。Windows 的默认应用仍由用户决定，不破解 UserChoice，也不模拟系统点击。
- 卸载不删除用户图片及配置。安装包未签名时 Windows 可能提示未知发布者；请先核对来源及 SHA256SUMS.txt，勿关闭系统安全功能。

## Linux (x64)

解压 linux-x64.tar.gz 后运行 ./LiteView。二进制在 Ubuntu 22.04 构建；更旧的 glibc 系统可能需要从源码构建。
桌面会话需要 X11 或 Wayland，以及可用的图形驱动。无需 root 权限。

可选桌面集成：将二进制安装到 PATH 中（例如 ~/.local/bin/LiteView），
把 liteview.desktop 放到 ~/.local/share/applications/，把 liteview.png 放到
~/.local/share/icons/hicolor/256x256/apps/。然后在系统「打开方式」中选择 LiteView。
本版本不自动修改 Linux 默认应用。

## macOS

按机器选择 macos-arm64（Apple Silicon）或 macos-x64（Intel），解压并将 LiteView.app 拖到 Applications。
最低系统声明为 macOS 11，但实际兼容性仍需对应机器验证。
当前构建未签名、未公证；Gatekeeper 可能阻止首次打开。只对核对过来源的包使用系统「隐私与安全性」中的允许打开流程，不关闭 Gatekeeper。

## 配置与隐私

- Windows 安装版：%APPDATA%\LiteView\settings.json
- Linux：~/.config/liteview/settings.json（或 XDG_CONFIG_HOME）
- macOS：~/Library/Application Support/LiteView/settings.json
- 绿色模式：程序旁 config/settings.json

程序运行时不联网、不上传图片。图像缓存仅在内存；「设为桌面背景」会按用户操作保存一份壁纸。
动图缓存限制约束预缓存的 RGBA 帧；大动图还需要压缩源文件、解码器合成缓冲和少量当前/待显示帧内存，不等于进程总内存上限。
v0.1.1 限制源文件为 512 MiB，并对各路径的已知像素/工作缓冲执行预算检查；这不是整个进程的内存硬上限。
WebP 默认无损；在「设置 → 编辑」或编辑模式的另存选项中切换有损与品质。JPEG/BMP 保存会铺白，但不会清除当前编辑画布的透明度或撤销记录。

## English quick start

Windows: extract the portable ZIP and run LiteView.exe, or use the per-user installer.
Linux: extract the archive and run ./LiteView in an X11/Wayland desktop session.
macOS: choose the matching architecture and move LiteView.app to Applications.
Unsigned builds may trigger OS security warnings. Verify the download source and checksum; do not disable OS security features.
Choose English in Settings → General. Open with Ctrl+O, navigate with arrows, zoom with the wheel,
enter Edit with E, redact with M, and close with Esc. GIF/APNG/WebP animations play automatically;
use comma/period for individual frames and the context menu to export the selected frame.
