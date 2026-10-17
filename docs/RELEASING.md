# 发布流程

## 本地验收

1. cargo test --locked --release --lib
2. cargo clippy --locked --release --all-targets -- -D warnings
3. cargo build --locked --release
4. Windows：pwsh -NoProfile -File packaging/test_windows_resources.ps1 -Binary target/release/LiteView.exe
5. cargo install cargo-about --version 0.9.2 --features cli --locked
6. cargo about generate --locked -o LICENSES.html packaging/licenses.hbs
7. python -m unittest discover -s packaging -p "test_*.py"
8. python packaging/package.py --binary target/release/LiteView.exe --platform windows --arch x64

GNU/MinGW 用户需把完整工具链的 bin 目录放到 PATH。若 .cargo/config.toml 指定了 target-dir，
第 4、8 步改用真实可执行文件路径。资源检查只读取 EXE，验证大/小图标可提取、版本信息与 DPI/长路径清单，不启动应用或修改文件关联；也支持 Windows PowerShell 5.1。
Windows 资源生成失败现在会阻止构建，不能再交付无图标、无清单的程序。打包器需要 Python 3.11+，不依赖 Pillow；assets 中已经包含平台图标。
打包暂存目录保存在 dist/.stage-*，不会递归清理任何已有目录。

Windows 安装包需 Inno Setup 6：

    ISCC /DAppVersion=0.1.2 /DBinaryDir=ABSOLUTE_BINARY_DIRECTORY packaging/windows.iss

程序不需要管理员权限；安装包也按当前用户安装。请用隔离账户或虚拟机验证安装、升级、注册关联、卸载。
不要为了测试在开发者的日常账户里改默认应用。

## CI 与版本发布

- 普通 CI 执行三平台静态检查、Rust 测试、构建；Windows 的普通 CI 与 Release 均检查最终 EXE 的图标、版本和清单资源。
- Release 工作流支持手动触发：仅构建并上传 Actions artifacts，不创建公开版本。
- 本机可安全运行 `powershell -NoProfile -ExecutionPolicy Bypass -File packaging/test_installer_process.ps1` 或 `pwsh -NoProfile -File packaging/test_installer_process.ps1`：仅加载辅助函数并使用合成父/子进程验证完整进程树等待和非零退出码传播，不运行安装器或读写注册表。常规 CI 覆盖两个 PowerShell 版本，Release 也运行该回归；每步限时 2 分钟。
- Windows 打包门禁在一次性 GitHub runner 上运行 `packaging/test_windows_installer.ps1`：用户级安装、同 AppId 从合成的 0.0.0 安装包升级、卸载、保留配置和用户图片、跳过关联/绿色版后来接管关联时不误删注册信息。使用 `Start-Process -Wait` 等待整个进程树，不用启动进程退出或卸载注册表项消失代替清理完成；独立生命周期步骤限时 5 分钟。脚本拒绝本机日常账户运行，也拒绝覆盖既有注册。此测试不等于旧版本程序数据迁移、交互式安装向导或系统默认应用验收。
- 推送与 Cargo.toml 完全匹配的 vX.Y.Z 标签后，生成第三方许可，在 Windows x64、Linux x64、macOS arm64 / x64 执行 release Clippy（全 targets、零警告）、测试并构建。
- 只有全部发布构建成功才创建 GitHub Release；附所有平台包与 SHA256SUMS.txt。不接受用 macOS 的 continue-on-error 掩盖发布失败。
- `v0.*` 及含预发布后缀的标签自动标记为 GitHub Pre-release，且不设置为 Latest。v0.1.2 是开发预览，不代表 v1.0 功能或原生验收全部完成；已有公开版本保留，不覆盖旧标签或旧附件。
- 当前没有 Windows 签名或 macOS 公证。拿到证书并加入安全的 CI 凭据之前，不得宣称已签名。

## 发布前仍需人工验证

- Windows HEIF / AV1 扩展安装后的真实 HEIC / AVIF 与 EXIF 方向。
- Windows 安装/升级/卸载；macOS Gatekeeper、拖文件打开；Linux 桌面集成和运行依赖。
- 屏幕阅读器（Narrator / VoiceOver）与纯键盘编辑流程。已有控件语义、自动对比度测试不替代原生无障碍验收。
- 长动图播放、回退、最后一帧导出；GIF 与 JPEG XL 的 Windows 脚本验收已通过，其余原生平台仍需复核。

第三方许可证采用 about.toml 中的显式允许列表。遇到未知许可或解析失败应查清来源，
不要添加全局忽略或把未完成的清单塞进发布包。
