# LiteView WACK 验证准备笔记

核查日期：2026-10-17。本文是环境与执行方法调查，**不是 WACK 通过报告，也不是 Microsoft Store 认证结果**。

调查时已知状态：主机为 Windows 11 Pro，build 26200；`HyperVisorPresent=False`；尚未找到 Windows App Certification Kit（WACK）。本次调查没有安装 SDK、导入证书、启用虚拟化或运行 WACK。后续用户已于 2026-10-17 自行提交商店审核，最后核对进入预处理；送审不代替本地测试。

## 1. 工具从哪里来

WACK 随 [Windows SDK](https://learn.microsoft.com/en-us/windows/apps/windows-sdk/) 提供，不等于 Windows App SDK/NuGet 包。默认位置是：

```text
C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe
```

核查时，[官方下载页](https://learn.microsoft.com/en-us/windows/apps/windows-sdk/downloads) 列出的受支持 SDK 包括 28000 和 26100 系列。26100.9457（2026-09）可作为当前 Win11 测试机的候选固定工具版本；不必仅为安装 WACK 修改产品的 `MinVersion`。实际执行时须记录安装器版本、`appcert.exe` 版本和操作系统版本，不能只写“安装了最新版”。

### 已确证的组件与参数

从 [Microsoft 官方 26100.9457 安装器](https://go.microsoft.com/fwlink/?linkid=2382321) 的内嵌 manifest 和帮助信息确认：

- 显示名称：`Windows App Certification Kit`。
- 功能 ID：**`OptionId.WindowsSoftwareLogoToolkit`**，不是 `OptionId.AppCertificationKit`。
- 依赖：`OptionId.AvrfExternal`（Application Verifier）。WACK 本身包含多个按计算机安装的 MSI。
- 支持 `/features`、`/quiet`、`/norestart`、`/log`、`/list`、`/help` 和 `/layout`。
- `/layout` 是下载离线安装布局，不是便携运行；内嵌帮助要求布局下载全部功能，不能承诺它只下载 WACK。

检查对象的最终下载地址：

```text
https://download.microsoft.com/download/46742ab5-6592-4968-a793-129e7f3bc55a/KIT_BUNDLE_WINDOWSSDK_MEDIACREATION/winsdksetup.exe
```

长度为 1,450,360 字节，SHA-256 为：

```text
b0bdbad38ae74c40ccae8cdbcf229807627fba2bc3100be28301c4ba884f755e
```

本次仅下载到进程内存，并读取 Burn/CAB 内的 `UserExperienceManifest.xml`、MSI 条目和内嵌帮助；**没有落地或运行安装器**。这个哈希用于标识本次检查对象，不替代实际安装前的 Microsoft Authenticode 签名验证。

以下命令仅供未来经授权的隔离测试机执行；本次未执行：

```powershell
# 先人工确认安装器来自微软、签名有效，再用该版本的 /list 核对组件。
.\winsdksetup.exe /list
.\winsdksetup.exe /features OptionId.WindowsSoftwareLogoToolkit /quiet /norestart /log "C:\WackQa\sdk-install.log"
```

没有找到微软保证的 WACK 便携模式。复制 `appcert.exe` 或手工解包几个 MSI，不等于完整安装；不要为了“免安装”自行拼接 COM、注册表和 Application Verifier 依赖。

## 2. 如何运行及是否需要预安装

[微软 WACK 文档](https://learn.microsoft.com/en-us/windows/uwp/debug-test-perf/windows-app-certification-kit) 给出两种入口。在测试机的管理员终端中，按安装版本的 `appcert.exe /?` 自检参数后执行。

直接测试尚未预安装的包：

```powershell
$appcert = 'C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe'
& $appcert reset
& $appcert test -appxpackagepath 'C:\WackQa\LiteView-test.msix' -reportoutputpath 'C:\WackQa\LiteView-wack.xml'
```

测试已安装应用：

```powershell
& $appcert reset
& $appcert test -packagefullname '<实际 PackageFullName>' -reportoutputpath 'C:\WackQa\LiteView-installed-wack.xml'
```

因此，**不必一律先手动安装再跑 WACK**；`-appxpackagepath` 就是包路径入口。但动态测试仍需要能够部署、激活和运行应用，不能把未签名商店上传包视为普通可侧载安装包。

### 用户会话要求有文档边界

同一官方页面的 “What's new” 说可以集成到没有交互用户会话的自动化测试；CLI 章节的 Important 则仍要求活动用户会话，并明确不能在 Session 0 运行。这里存在说明不一致。

稳妥做法是用完整 Windows 11 桌面环境及活动用户会话，记录具体工具版本与完整报告。不能据此承诺无人登录的 CI runner 一定覆盖全部测试，也不能不经实测断言所有 headless 场景均不可用。

## 3. 未签名商店包的测试策略

[Microsoft Store 会为 MSIX 重新签名](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)，因此提交 MSIX 不要求开发者购买公开 CA 证书。但这不意味着正式未签名上传包可以随意在本地安装。

推荐保留已上传的正式 MSIX 不变，在隔离测试机制作测试签名副本：

- 证书 Subject 必须匹配清单的 `Publisher`，不要为方便测试修改正式应用身份。
- 仅在隔离环境按[微软测试签名指南](https://learn.microsoft.com/en-us/windows/msix/package/create-certificate-package-signing)信任测试证书；示例使用 `LocalMachine\TrustedPeople`，不是根证书存储 `Root`。这仍是全机器信任变更，不应擅自在日常工作机执行。
- 记录正式包和测试签名副本的 SHA-256，以及清单和内部 EXE 的一致性。签名副本字节不同，不能声称原上传文件已按原字节完成全部本地认证。

不导入证书的开发测试可参考 [unpackaged layout 注册](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-manual-conversion)的 `Add-AppxPackage -Register <staging>\AppxManifest.xml`，再按实际包全名测试。但必须说明部署方式，不承诺它覆盖正式签名包的所有 packaging/signature 测试。

Windows 11 的 [`-AllowUnsigned` 路径](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package)需要特殊 Publisher OID：`OID.2.25.311729368913984317654407730594956997722=1`，且 EXE 包通常涉及管理员和全用户注册。这会引入开发身份差异，不应修改正式上传包来绕过信任要求。

### 包内 EXE 没有独立签名，不等于商店必拒

[Desktop Bridge WACK 测试说明](https://learn.microsoft.com/en-us/windows/uwp/debug-test-perf/windows-desktop-bridge-app-tests)把检查每个 PE 文件数字签名的 “Digitally signed file test” 列在 **Optional tests** 下。可选测试用于提示，不决定上架 pass/fail；必须区分 required failures、optional warnings 和未运行项目。这条结论针对 MSIX/Desktop Bridge，不应套用到独立 EXE/MSI 商店分发路线。

## 4. 测试环境选择

### 首选：完整 Windows 11 VM 或专用测试机

安装所需 WACK 组件，并在活动用户会话内测试。将测试证书和安装副作用限定在这个环境，完成后保留报告、回滚快照。若要在当前工作机安装 WACK 或信任证书，须另行获得授权。

本机 `HyperVisorPresent=False` 只说明当前未运行 hypervisor，不能证明 CPU 不支持虚拟化，也不能假设 Windows Sandbox 已可直接使用。[Sandbox 安装要求](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-install)涉及硬件虚拟化、Windows 功能和可能的重启；本次不启用这些功能。[Sandbox 概述](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/)还说明 Windows 11 24H2 起不包含内置 Store apps，因此它不应未经核实就替代完整 Win11 包部署/商店测试环境。

### GitHub Windows Server 2025 runner：只能先视为补充

核查时的[官方镜像清单](https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md)标识镜像 `20261013.288.2`，系统为 Windows Server 2025，列有 Windows SDK `10.0.26100.0`。**有 SDK 不等于有 WACK，也不证明全部认证测试适用。** 实际镜像以运行记录的 `Set up job > Runner Image` 为准。

当前微软 SDK 支持列表没有明确列出 Server 2025；没有找到第一方保证“GitHub Server 2025 hosted runner 支持完整 WACK 桌面认证”。同时，[GitHub hosted runner 文档](https://docs.github.com/en/actions/concepts/runners/github-hosted-runners)不正式支持嵌套虚拟化，因此不推荐在标准 hosted runner 内再开 Win11 VM 来宣称获得受支持验收环境。

CI 可先用于构建、清单和包结构检查、辅助回归。如果尝试 WACK，必须记录系统、会话、工具版本和所有未运行测试，不能将部分通过当作 Win11 全部验收通过。

## 5. 最终应保存的证据

- 原上传包、测试包的 SHA-256；应用版本、架构、Identity、Publisher；签名或开发部署方式。
- Windows 版本/build、用户会话环境、SDK 和 `appcert.exe` 的精确版本。
- 完整命令和原始 XML/HTML 报告，不能只保存进程退出码或“成功”截图。
- Required failures、optional warnings、not run/inapplicable 分别统计；灰色或不适用测试不能算通过。
- 安装、启动、图片打开/关联、更新和卸载的独立人工回归记录。

## 6. 一次性 runner 的补充验证脚本

手动工作流 `.github/workflows/store-validation.yml` 只下载已发布工作流 `39027759847` 的 `Store-submission-x64`，不重建、不发布、不替换已送审的包。`packaging/test_store_wack.ps1` 同时固定正式 MSIX 与内部 EXE 哈希，并核对身份；测试签名后再次核对清单与 EXE 未变。

- 入口有 `GITHUB_ACTIONS`、`RUNNER_ENVIRONMENT=github-hosted` 和 Windows 三重保护，禁止在日常电脑执行。
- WACK 缺失时只在临时 runner 安装已核验的微软组件；安装器必须同时通过固定 SHA-256 和 Microsoft Authenticode 签名检查。记录工具版本、操作系统、会话 ID、镜像版本和完整命令。
- 仅对副本临时签名，非导出私钥；仅信任到 runner 的 `TrustedPeople`。测试后清理包与证书。证书、私钥和签名副本不在 artifact 上传目录内。
- WACK 有 30 分钟进程超时，工作流保存原始 XML/HTML、标准输出/错误和失败阶段。缺报告、未知结果、超时都不能视为通过。
- `packaging/wack_report.py` 的 `reported-pass` 仅代表报告明确写出 PASS 且进程退出 0；它不代表 Windows 10/11 验收、全部子测试执行或微软商店认证。

判读依据不是猜测 XML 节点：[Microsoft TemplateStudio 固定版本源码](https://github.com/microsoft/TemplateStudio/blob/062d0c12519bcab0e0b4ac146ee2ad163140b344/code/test/TemplateStudioForWPF.Tests/WACK/Wpf/WindowsAppCertKitTests.cs) 同时要求退出码 0 与 **`OVERALL_RESULT="PASS"` 属性**；[Microsoft React Native Windows 的测试脚本](https://github.com/microsoft/react-native-windows/blob/56d0ff9e9e3677f2e00b8faaa5f2bef1cef5d846/.ado/scripts/TestWACK.ps1) 读取 `REPORT.REQUIREMENTS.REQUIREMENT` 和 `TEST.MESSAGES.MESSAGE.TEXT`。这两份源码不是稳定的完整 schema 契约，因此脚本保留原始报告，required/optional/not-run 分类须按实际报告复核。

后续如果增加包身份 GUI 回放，[`Invoke-CommandInDesktopPackage`](https://learn.microsoft.com/en-us/powershell/module/appx/invoke-commandindesktoppackage?view=windowsserver2025-ps) 可以给任意 EXE 分配包身份，`-PreventBreakaway` 保持子进程上下文，但微软明确其 token 与真实 AppId 启动不完全相同。应在包内 bootstrap 中设置脚本环境，再由普通子进程运行原始 EXE，不依赖 cmdlet 继承调用方环境，也不把回放当成真实 AUMID 激活。根据 [MSIX 排障文档](https://learn.microsoft.com/en-us/windows/msix/manage/troubleshoot-msix-container)，Developer Mode 只对 Windows 10 build 18922 之前的这条调试路径有要求；不应为 Server 2025 回放启用它。

**当前结论：WACK 尚未运行；本地验收未完成；Microsoft Store 已由用户提交审核，但尚未通过认证。WACK 通过也不等于商店审核通过，商店可能运行本地未覆盖的测试。**
