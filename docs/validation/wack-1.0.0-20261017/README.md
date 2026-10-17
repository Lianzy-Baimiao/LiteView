# 1.0.0 WACK 原始证据

来源：[手动验证运行 39036373722](https://github.com/Lianzy-Baimiao/LiteView/actions/runs/39036373722)，提交 `312b7d6`。测试于 2026-10-17 13:24:04–13:24:51 UTC 执行，退出码 0，`OVERALL_RESULT="PASS"`，`PARTIAL_RUN="FALSE"`。

- 环境：Windows Server 2025 Datacenter build 26100，镜像 `20261007.264.1`，活动用户会话 2，管理员。
- WACK：预装版本 `10.0.26100.8249`，Microsoft Authenticode 有效；本次没有走下载/安装 SDK 的备用分支。
- 原正式 MSIX：`2130cb363d94fce37d06f0cef4222dc84795c408a4bda6d0c8a91103eb925b06`。
- 测试签名副本：`1596fc1e541ad65c708756e4d5ea01ff387353bb75c0cdede289d0705a875d3a`。内部 EXE 和清单未变，测试后证书清理无错误。
- `wack.xml` 原始字节 SHA-256：`d367eeda3f5f1e84dafa348ae1d2875c69eb76d23987020ad5418395450c05b3`。XML 中引用的是 runner 上的 XSL 路径；保留原文，没有伪造 HTML 报告。

## 完整报告判读

| 类型 | PASS | FAIL | 未运行 / 未知 |
| --- | ---: | ---: | ---: |
| 必需检查 | 13 | 0 | 0 |
| 可选检查 | 10 | 1 | 0 |

可选 **Blocked executables** 检查有 6 条消息：`CreateProcessW`、`ShellExecuteW` 进程启动 API，以及 `CmD`、`ReG`、`cmd.exe`、`cdB` 字符串引用。该检查被原始报告明确标为 `OPTIONAL="TRUE"`，所以与总体 PASS 不矛盾。它是静态引用提示，不能单凭这份报告证明这些工具真的被执行，也不在这里擅自把所有提示认定为误报；仍保留为审查事项。没有删除字符串、关闭测试或改送审二进制来消除警告。

[微软 §4.2 的说明](https://learn.microsoft.com/en-us/windows/uwp/debug-test-perf/windows-desktop-bridge-app-tests#42-blocked-executables)针对 Windows 10 S 对部分外部可执行文件的限制，要求辨别实际调用目标；这不是全面禁止上述两个 API。可选项不参与本地总体 pass/fail，但也不豁免[商店可用性政策](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies-and-code-of-conduct#104-usability)。真实依赖 Cmd/PowerShell 等工具在 S mode 可能受限，见[微软 MSIX 准备指南](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-prepare)。本次没有完成 S mode 实机核查。

`reviewed-summary.json` 由补充了实际 XML 结构支持的 `packaging/wack_report.py` 生成，保留所有测试分类与消息；该原始报告也作为解析器回归用例。`validation.json` 是首次执行时的原始运行记录，不是后来改写的结论。

**边界：控制台明确写了 `Running tests without application deployment`。这次检查不含实际安装后 GUI 操作；Server 2025 也不是 Windows 10/11 实机验收。总体 PASS 不能替代 Microsoft Store 认证或 `runFullTrust` 审批。**
