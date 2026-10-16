# LiteView v0.1.1 — 开发预览

轻量离线看图、文件夹浏览、动图播放与逐帧，以及 Paint 风格编辑和马赛克/模糊打码。动图只查看，不编辑。本版是 **Pre-release**，不是 v1.0 完工声明。

## 本版更新

- 源文件限制为 512 MiB，各解码入口增加已知像素/工作缓冲预算检查；资源超限不能通过系统解码回退绕过。按需动图保留原预算，JPEG XL 接入分配跟踪，损坏的 PSD PackBits 数据不再补黑后假报成功。
- WebP 新增有损压缩与独立品质设置，默认仍为无损；两种模式均支持 EXIF/ICC。新增静态 libwebp 编码器，其许可和专利文本随包提供。
- 保存选项提示格式对 EXIF、ICC 和透明度的限制；元数据设置错误不再静默忽略。保存失败与写入成功后读回失败分别提示，另存格式不破坏编辑画布的透明度、色彩信息或撤销历史。
- 修复小尺寸 TIFF 的 ICC 读取、旧后台解码结果覆盖新保存图片、空闲解码线程退出和重复缩放任务堆积；临时文件独占创建，文件关联注册失败返回非零退出码。
- 包含此前未单独发布的长动图按需解码、GIF 导出抖动、JPEG 覆盖沿用可识别原品质，以及 Windows 安装/卸载进程树等待修复。

## 验证与发布门禁

Windows GNU 本机通过 106 项 Rust release 单元测试、release Clippy 全 targets 零警告、release 构建、3 项 Python 打包测试及两个 PowerShell 版本的安装进程树回归。隔离配置的 GUI 回放通过 PNG、无损 WebP、有损 WebP 的旋转、撤销、重做、保存与退出；非法关联配置在任何注册表操作前拒绝并返回退出码 1。本机未安装/卸载软件或修改默认应用。

标签发布工作流要求 Windows x64、Linux x64、macOS arm64 / x64 全部完成 release Clippy、测试、构建和包结构检查，并在一次性 Windows runner 上通过用户级安装、合成旧版升级、卸载及关联保留测试后才创建公开预览版。安装测试不等于真实旧版数据迁移或交互式向导验收。最新执行证据见仓库 Actions；旧版本的成功结果不能代替本版本门禁。

## 下载与运行

- Windows 10 / 11 x64：`windows-x64.zip` 为绿色版，解压后运行 `LiteView.exe`；`windows-x64-setup.exe` 为当前用户安装包，不需要管理员权限。
- Linux x64：解压 `linux-x64.tar.gz` 后运行 `./LiteView`，构建基线为 Ubuntu 22.04。
- macOS：按机器选择 `macos-arm64.tar.gz`（Apple Silicon）或 `macos-x64.tar.gz`（Intel），将 `LiteView.app` 拖入 Applications。
- 发布包附 `SHA256SUMS.txt`、安装说明和第三方许可证。绿色版通过 `LiteView.portable` 标记将配置保存在程序旁的 `config` 目录。

## 已知限制

- 预算检查不是进程内存硬上限：SVG 解析/滤镜、系统解码器、GPU 和并行预读仍有独立开销；进程级隔离和完整中途取消尚未实现。
- SVG 仍固定以 2048 px 栅格化，尚未随视口与缩放重绘。
- 真实 HEIC EXIF 方向及系统扩展组合、Linux/macOS 原生交互与桌面集成、屏幕阅读器和参考机性能验收仍未完成。
- 未进行 Windows 数字签名或 macOS 公证。请核对下载来源和校验值，不要关闭系统安全功能来绕过警告。

详细需求、证据边界和待办见仓库 `docs/PRD.md` §15.4 与 `docs/TODO.md`。
