# LiteView 1.0.0 发布验证

核对日期：2026-10-17。本文记录已完成的检查，不代替商店认证或未执行的平台人工验收。

## 发布身份

- 标签：`v1.0.0`；提交：`7d27c51f4de7bd9b1cdb5800bb3b55920ac71bfe`。
- [GitHub Release](https://github.com/Lianzy-Baimiao/LiteView/releases/tag/v1.0.0)：正式版、非草稿、非 Pre-release，Latest API 返回 `v1.0.0`。
- 发布时间：2026-10-17 11:38:51 UTC（北京时间 19:38:51）。
- [三平台常规 CI](https://github.com/Lianzy-Baimiao/LiteView/actions/runs/39027755878)：全部成功。
- [四目标发布工作流](https://github.com/Lianzy-Baimiao/LiteView/actions/runs/39027759847)：全部成功后才创建 Release；Windows x64、Linux x64、macOS arm64 / x64 均执行 release Clippy、Rust 测试、构建和打包检查。

## Windows 与 MSIX

- Windows release Rust 测试 115 项、Python 打包测试 10 项通过。
- 普通安装器完成隔离安装、合成旧版本升级、卸载与关联保护回归。
- MSIX 测试副本在一次性 runner 中临时签名，完成安装、真实 AUMID 激活、WinRT LocalFolder、普通版注册保护、合成 `1.0.1.0` 包升级及卸载检查。升级包内部产品版本仍为 `1.0.0`，不是跨产品版本的数据迁移验收。
- `installed.json` 和 `upgraded.json` 均报告 `packaged=true`、`ice.LiteView_rz8m2sa64m4em!LiteView`，配置路径均等于该包的 WinRT LocalState。报告保存在该次运行的 `windows-msix-test-reports` artifact。
- 下载后的绿色版 EXE 与 MSIX 内 EXE 的 SHA-256 一致；资源检查再次确认图标、1.0.0 版本、单一应用清单、PerMonitorV2、长路径与通用控件；导入表没有未随包附带的 VC/MinGW 可再分发运行库。
- 主 EXE 与安装器的 Authenticode 状态均为 `NotSigned`。临时测试证书不是公众信任签名，没有把测试签名包或私钥作为产物上传。

### 修复的测试误报

首次 MSIX 生命周期检查把正常退出判为失败。最小合成进程在 PowerShell 7 / 5.1 重现：通过 `GetProcessById` 延迟获取的 Process，单独调用 `WaitForExit` 后读取到空 ExitCode。修复为等待前持有可查询进程句柄，且不可读退出码仍然失败；异常退出显示十进制和十六进制状态。`packaging/test_msix_process.ps1` 覆盖退出 0 与退出 7，随后完整 MSIX 生命周期也通过。没有忽略退出码或关闭发布门禁。

## 下载核对

五个发布文件均已下载，与 Release 的 `SHA256SUMS.txt` 一致；ZIP/TAR.GZ 内容能够完整读取。

| 文件 | SHA-256 |
| --- | --- |
| Windows 主 EXE（包内） | `21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a` |
| 商店候选 `LiteView-store-1.0.0.0-x64.msix` | `2130cb363d94fce37d06f0cef4222dc84795c408a4bda6d0c8a91103eb925b06` |

未签名 MSIX 构建副本保存在 `Store-submission-x64` artifact，后续经用户确认上传至商店草稿；公共下载附件没有 MSIX、证书或测试签名副本。

## 商店跟进与剩余事项

- 发布后的跟进：用户确认后，已上传该 MSIX 并保存 CNY 0 定价、应用属性、中英文文案和受限能力用途说明。2026-10-17 用户随后自行提交审核，后台显示提交完成、预处理进行中、认证及发布尚未开始，且保留手动发布选项。进度见 `STORE-LISTING.md`。
- 当前版本截图已补齐：中英文各四张 1500 × 975 PNG 已上传，Store 一览显示“完成”。使用校验过哈希的发布 EXE、隔离配置和新生成的演示文件完成八次回放，退出码均为 0；编辑与马赛克文件实际保存，GIF 截图显示第 4 / 48 帧。截图与哈希清单见 `images/store-v1.0.0/README.md`。这不是已安装商店包的人工验收。
- 本次补充资料时重新执行 Python 打包单元测试 10 项全部通过，截图脚本语法检查、八张 PNG 的尺寸/哈希/非空白帧/回放退出码及演示保存前后变化检查通过；没有重建或替换 v1.0.0 发布二进制。
- 年龄分级已保存：用户授权后按实际功能填写 IARC 10.3 问卷，并由用户在后台完成最后确认与保存。提交前重新进入页面确认 Microsoft Store / IARC / PEGI 为 3+、ESRB 为 E，分级 ID 当时为“待定”。没有据此宣称正式分级证书已签发。
- 补充 WACK 已在一次性 Server 2025 runner 完成：总体 PASS、13 必需项全过，10 可选项通过、1 可选项 Blocked executables 失败并保留全部 6 条消息。使用预装 WACK `10.0.26100.8249` 和原送审包的临时签名副本，清单/EXE 未变；没有在日常电脑安装工具或信任证书。原始报告与边界见 `validation/wack-1.0.0-20261017/README.md`。
- 已安装包的 GUI 补测通过：[运行 39037154456](https://github.com/Lianzy-Baimiao/LiteView/actions/runs/39037154456) 分别记录真实 AUMID 诊断激活及四次包身份界面回放；看图、文字箭头保存、矩形打码保存、GIF 第 4 / 48 帧均通过，各场景退出 0，并使用实际 WinRT LocalFolder 而非配置覆盖目录。截图/身份/哈希/原始 WACK 归档见 `validation/store-gui-1.0.0-20261017/README.md`。测试仍是 Server 2025，不冒充 Windows 10/11 实机。
- 商店版 Windows 10/11 人工界面、系统默认应用与目标桌面环境的 WACK 复测、发布前资料复核仍需完成。上述 WACK 明确未部署应用，不能代替这些检查；用户已送审也不等于验收通过。
- GitHub 公众信任代码签名与 macOS 公证尚未办理；微软商店签名不能代替独立 EXE 签名。
- 其余已知限制继续按 `release-notes.md` 与 `TODO.md` 追踪；正式版编号不表示这些项目已经完成。
