# 1.0.0 启动崩溃反馈跟进

## 当前结论

2026-10-20 收到转述：安装后打开提示“LiteView 已停止工作”。系统为 Windows 11 家庭中文版，用户报告 26H2、26300.9711、体验包 1000.26100.372.0。

2026-10-22 追加转述：渠道为 Microsoft Store 版，系统仍为 26H2；另有人描述“测试了一下 HEIC 兼容性”，尚不确定与崩溃是否为同一次操作、同一台设备。复查见文末“商店版 + HEIC 复查”。

重启后追加反馈：“安装了商店版本，但是打开图片没有反应，无法启动”“WIN11 GITHUB 下载的安装包，也是同样情况，无法启动”“win11 26H2，微软商店下载无法打开，菜单图标有些模糊”。尚不能把这些转述合并成同一台电脑；GitHub 那条只明确 Win11，没有明确系统构建号。共同运行路径（图形初始化、驱动、解码等）的排查优先级提高，但未排除渠道或关联问题。“菜单图标模糊”独立记录，位置、DPI 和截图待确认。

**仍未复现，仍无故障模块、异常代码或崩溃转储，不能宣布修复，也不能归因于系统、显卡、HEIC 扩展或商店包。** 空启动即失败还是打开 HEIC 才失败、HEIC 来自什么设备、是否装了 HEVC 视频扩展，均待补充。

转贴中的 PNG 为 B 站 36×66 搜索图标，并非崩溃截图。现有发行验证覆盖 Server 2025 的安装和包身份 GUI 回放，不能代替这台 Windows 11 设备的启动复现。`--diagnostics` 在窗口及图形初始化之前退出，其通过不代表 GUI 启动通过。

本轮只读核查了 Partner Center 的 LiteView 健康状况页（产品 `9PCG7D394JFC`，过去 1 个月）：故障列表为“找不到数据”，并提示“没有来自已选择加入的设备的数据”。这表示没有可供定位的遥测，不能反证用户反馈。截图与页面记录仅在忽略的 `dist/support-feedback-20261020/` 留存，未写入公开资料。

## 最小补充信息

1. ~~安装渠道~~：已答复为 Microsoft Store 版。
2. 空启动就失败，还是双击/打开 HEIC 才失败；若只在 HEIC 失败，说明照片来自什么手机或相机、大小，不要求上传私人图片。
3. 开始菜单搜索“查看可靠性历史记录”，找到 LiteView 的红叉，打开“查看技术详细信息”，提供故障模块名称、异常代码、故障偏移。截图时遮住用户名和私人路径。
4. “设置 → 应用 → 已安装的应用”里搜索 HEIF、HEVC，是否两个扩展都在（只装 HEIF 没有 HEVC 是本机无法模拟的分支）。
不把运行 CMD/PowerShell 脚本作为普通用户的排障前置条件。先由开发侧检查正式包和重放真实启动；现有收集器仅保留为内部工具。

## 只读诊断工具

`packaging/collect_windows_support.ps1` 不启动 LiteView，不要求管理员，不更改文件关联、证书、驱动或系统安全设置，不读取图片，不生成内存转储，不联网上传。

采集范围：Windows 版本、显示适配器名称及驱动版本、当前用户的 `ice.LiteView` 商店包版本，以及最近 7 天最多 500 条 Application Error 事件中属于 `LiteView.exe` 的故障签名。事件输出采用字段白名单，排除完整路径、用户标识、报告标识和原始消息。无匹配记录不等于启动成功；读取权限受限单独标注。

```powershell
./packaging/collect_windows_support.ps1 -OutputDirectory ./dist/support-local
# 可选地核对 GitHub 独立版的文件版本和 SHA256，只检查文件、不执行：
./packaging/collect_windows_support.ps1 -OutputDirectory ./dist/support-binary -Binary 'C:\path\LiteView.exe'
./packaging/test_windows_support.ps1
```

纯事件夹具测试只验证提取与隐私过滤，不是启动崩溃复现。得到真实故障签名之后，建立能触发同一退出码/故障模块的启动回归，再做针对性修复。不要先更换默认渲染后端或全局吞掉 panic 来宣称兼容性修复。

## 先前的只读工具验证

- `packaging/test_windows_support.ps1` 在 PowerShell 7 与 Windows PowerShell 5.1 均通过：保留模块/异常码/偏移，剔除路径与无关事件，缺失字段不编造。
- 在本机 Windows 11 专业版 25H2 / 26200.9550 运行只读收集成功，未找到 LiteView 崩溃事件。与报告设备版本不同，且没有进行 GUI 启动复现；不是对原问题的验收。
- `dist/LiteView-启动诊断-20261020.zip` 已生成并验证，只有收集脚本、双击启动器和说明三份文件，不包含本机收集结果。
- 本轮没有修改产品运行路径，没有升级版本号、发布 Release、回复外部评论或关闭打印 issue。

## 开发侧正式包实测（2026-10-20 追加）

收到“不要让用户运行 CMD，检查是否漏打包环境”的反馈后，已经直接测试公开的 1.0.0 EXE，未使用本地重新编译的二进制替代正式包。

### 包及依赖

- GitHub ZIP SHA256：`8092073e819902ea63afdc4554d2a104af00558a28d88249c56db3c9c592bd93`。
- GitHub安装程序 SHA256：`1905595ed9abba9d2c78f8547001965db67d3cda548fe3080a3a60411667ac67`。两者均与本次查询到的 GitHub Release asset digest 相同。
- ZIP、原提交 MSIX 内的 EXE 与受测文件逐字节哈希一致：`21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a`。
- 实际 PE 有 20 个不同的直接导入 DLL、321 个导入符号，无延迟导入。全部是系统 DLL/API-set；以仅 System32 搜索方式解析，321 个符号全部找到。
- 未导入 VCRUNTIME/MSVCP140、MinGW/libgcc/libwinpthread；现有发布工作流使用静态 CRT。没有需要用户另外安装的 .NET 或 WebView2 运行时。MSIX 没有外部 Framework PackageDependency。
- DX12 真实启动加载的是系统 `d3dcompiler_47.dll`，没有从开发工具目录加载 `dxcompiler.dll`。wgpu 30 的 Auto 编译器可以回退到系统 FXC，不需要为了启动盲目追加 DXC 分发文件。
- HEIC/HEIF、AVIF 使用 WIC 系统解码器，确实可能需要额外图像/视频扩展；代码对缺少 codec 有单独错误提示。这是特定格式的可选能力，不是空启动的必需运行库。本轮没有把 PNG/JPEG/WebP 通过扩大成这些格式也通过。
- 图标、版本、PerMonitorV2、longPathAware、common-controls 资源检查再次通过。
- **安装程序本轮未在日常用户账户重新安装，也没有宣称已静态解包 Inno 6.7 安装器。** 其哈希对应此前在一次性 Windows runner 通过安装/升级/卸载及 EXE 哈希检查的同一公开安装包。本轮也没有重新执行 MSIX 安装/AUMID 激活。

### 真正启动，不是 `--diagnostics`

`packaging/windows_startup_probe.py` 为每次运行创建一个新 Win32 desktop，仅让被测子进程使用它；从不调用 SwitchDesktop，不向用户桌面发送输入。枚举该私有 desktop 中属于被测进程的窗口，确认输入 desktop 前后不变。使用程序自身的 `LITEVIEW_SCRIPT` 获取实际 framebuffer，不是拍摄用户屏幕。

每次只复制受测 EXE，隔离 APPDATA、LOCALAPPDATA、TEMP、配置及工作目录；PATH 只保留 Windows 与 System32，不继承 SDK、Rust、Python、开发环境变量或凭据。最终测试工具还给子进程设置随机 USERNAME 环境值，隔离程序按该值命名的单实例管道；不创建或更改 Windows 用户账户。判定同时要求：首帧日志、图片显示检查点（有图片时）、非空截图、测试图片像素确实出现在截图中、正常退出且不超时。没有只用“退出码是 0”判定成功。

| 场景 | 数量 | 结果 |
| --- | ---: | --- |
| 默认后端，空启动与 PNG | 2 | 全部通过 |
| 指定 Vulkan / DX12 / GL，各测空启动及 PNG | 6 | 全部通过 |
| 默认后端，JPEG 与 WebP | 2 | 全部通过 |
| 全新绿色版配置与无 portable 标记的全新普通版配置，打开 PNG | 2 | 全部通过 |

共 12/12 通过，每次完整回放约 2.1–2.4 秒，退出码均为 `0x00000000`。普通版配置场景只验证配置路径，不等同于重新安装。

### 验证检测器真的会报错

只修改忽略目录内的临时测试副本，原正式包未变：

1. 把必需导入 `kernel32.dll` 的名称替换为不存在的等长名字，真实 Windows 加载器返回 `0xC0000135`；静态依赖检查和启动检测均失败，准确识别缺 DLL。这个是故障注入对照，不是用户故障的复现。
2. 把动态 Vulkan loader 的名字替换为不存在的等长名字，真实启动自动选择 DX12，仍能显示 PNG 并正常退出。证明“机器未安装 Vulkan loader”本身不必导致启动失败；不能证明某个有缺陷的显卡驱动也会被同样兜底。
3. `test_windows_startup.py` 的 8 项单元/真实启动测试全部通过，包含无截图但退出 0、纯色截图、错误图片、非零退出码的反例。最后运行全部 Python 打包/启动测试共 30 项，全部通过；随机单实例管道模式下两种全新配置也再次通过。已把测试接入本地 CI 配置；尚未推送或声称远程 CI 已执行。

开发侧复跑：

```powershell
python -m pip install -r packaging/requirements-windows-validation.txt
$env:LITEVIEW_TEST_BINARY = (Resolve-Path 'dist/release-v1.0.0-verified/binary/LiteView.exe').Path
$env:LITEVIEW_TEST_OUTPUT = 'dist/startup-regression-new' # 必须是新目录
python -m unittest discover -s packaging -p test_windows_startup.py -v
```

证据保存在忽略目录 `dist/startup-audit-20261020/`，包括 `runtime-imports.json`、`matrix-summary.json`、各场景的日志、真实渲染截图、加载模块清单和最终复测 `regression-isolated-ipc/`。不把机器路径或故障注入 EXE 放进用户发行包。

### 结论边界

**目前未发现运行库漏打包，正式 EXE 在本机真实启动测试通过；仍未复现报告中的崩溃，未发布所谓修复版。**

10-20 实测时本机是 Windows 11 专业版 25H2 / 26200.9550，不是反馈的家庭版 26H2 / 26300.9711。私有 desktop 隔离的是显示/输入，不是干净虚拟机；真实模块记录还显示本机已有桌面/录屏/游戏叠加层注入。因此，清空 PATH 不能声称排除了所有本机环境差异，也没有覆盖所有显卡、商店身份激活或用户自己的图片。下一步应针对真实故障模块建立同型复现，不把“本机通过”解释为用户反馈不成立，不要求用户盲装运行库或换系统。

## 商店版 + HEIC 复查（2026-10-22）

### 环境与解码链

- 本节重启前实测时，本机为 Windows 11 专业版 25H2 / **26200.9711**（与报告的 26300.9711 修订号相同、主版本号不同；注册表 ProductName 仍显示 “Windows 10 Pro”，是系统旧字段）。HEIF 图像扩展 1.2.48.0、HEVC 视频扩展 2.5.33.0；显卡 AMD Radeon RX 9070 XT（另有 GameViewer 虚拟显示器）。
- 开始复查时 `src` 自 `v1.0.0` 标签起无改动；本节旧版受测 GUI 是公开 EXE `21ddc58f…ac6a`。
- 实测 HEIC 解码链：`windowscodecs` → MFPlat → AppX StateRepository 查媒体扩展目录 → `msheif_store.dll` → `HEVCDECODER_STORE.dll` → `d3d11.dll`，**全部在 LiteView 进程内加载**。两个扩展 DLL 只导入系统 API-set、MFPlat/D2D/D3D11/DXGI，没有 VCLibs 等框架依赖。因此扩展或其下游若出现访问冲突，会直接带崩 LiteView；这是需要核对的风险路径，本轮没有观察到实际发生。

### 样本与结果

样本：本机 WIC HEIF 编码器生成的 640×480、4032×3024 HEIC；以及 nokiatech/heif 公开样本 115 个（一致性 C0xx/MIAF/多层、grid、overlay、alpha、burst、动画序列、HDR、增益图、8256×5504 大图、VVC 编码 .heif）。只做入站下载，留在忽略目录，不进仓库、不进发行包。

| 场景 | 结果 |
| --- | --- |
| 当前源码解码路径（`decode_file`，同 `decode-N` 线程；另测已是 STA 的线程、两线程并发各 10 次、Loader 解完立即 drop 并退出 ×5） | 全部退出码 0 |
| 115 个样本各自独立进程解码 | 84 解出、31 返回错误，0 崩溃 / 0 超时 |
| 公开 1.0.0 EXE 私有 desktop 逐个打开 115 个样本 | 84 显示、31“无法打开此图片”，全部退出码 0，结果与上一行逐个一致 |
| 同一文件夹 115 个样本，预读 2、单实例开启，方向键翻页 400 ms ×1 轮、60 ms ×2 轮 | 均到达结束检查点并正常退出 |
| 商店清单右键 verb：`--edit` / `--mosaic` × 4 个样本（含 8256×5504） | 全部退出码 0；编辑器内存峰值约 698 MB（查看模式 525 MB） |
| 借用本机已安装的全信任商店包（Windows Terminal）身份，`Invoke-CommandInDesktopPackage`：7 个代表样本解码、Loader 退出竞态、1.0.0 EXE `--diagnostics` 及 7 个 HEIC GUI 回放 | 全部退出码 0；`packaged=true`、AUMID 与 WinRT LocalFolder 正确解析；旧快照未见被借用包数据目录新增/改写；当时未核对删除，不作为完整差异审计 |

包内外 `AppPolicyGetMediaFoundationCodecLoading` 均为 All（0），包身份本身没有把第三方编解码器切换为 InboxOnly。借用身份**不是** LiteView 自己的商店签名身份；这一项随后用商店正式版补测，见下文“商店正式版实测”。借用身份这一步没有注册、安装任何包，没有改证书、开发者模式或文件关联。

### 顺带发现：HEIF 失败被报成“已损坏”（非崩溃）

31 个打不开的样本里，15 个 VVC（H.266）编码 `.heif`（WIC：“操作不受支持。”）和 11 个 `msf1` 图像序列（WIC：“此文件不符合相关的文件格式规范。”）都是有效文件，只是系统扩展不支持，LiteView 却显示“文件已损坏或不完整”；只有 3 个 `mif2`/`mif3` 品牌在识别阶段就正确显示“不支持这种格式”。做 HEIC 兼容性测试的人用这类公开样本时会被误导。

已在源码修复（未发布）：逐步记录 WIC 的 HRESULT，VVC 在 `GetFrame` 返回 `WINCODEC_ERR_UNSUPPORTEDOPERATION`（0x88982F81），本次普通 HEIC 截断/损坏对照没有返回它（不推广到所有容器、编码或损坏方式），直接映射为 `Unsupported`。纯图像序列返回 `MF_E_INVALID_FILE_FORMAT`（0xC00D36BE），截断、打乱 meta 的普通 HEIC 也返回同一个码，不能按码区分；改为看 ftyp 品牌：有序列品牌（msf1、hevc、avis…）且没有任何静态图品牌（mif1、heic、avif…）时，系统解码失败报“不支持”。Nokia 连拍/动画样本带 mif1/heic，有静态主图，照常显示。

验证：26 个有效文件改为“不支持这种格式”。22 个人为截断/打乱的普通 HEIC 对照中 19 个仍为“已损坏”，另 3 个损坏落在可容错的位置、仍能解出；VVC 和纯序列文件的损坏副本报“不支持”或“已损坏”，两种都不误导（完好时同样解不了）。C006（带辅助图像）、C039 返回的 `MF_E_INVALID_FORMAT`/`MF_E_UNEXPECTED` 与真实损坏相同，保留“已损坏”。本机 Clippy（全 targets、`-D warnings`）与 117 项 release 单元测试通过；修改后的 GUI 逐个打开 115 个样本，显示/无法打开的划分与 1.0.0 完全一致，只改了提示文字；中文界面截图确认。

### 结论边界

**在本节这一组系统修订号、HEIF/HEVC 扩展版本和显卡的测试中，商店正式版 + HEIC + 真实外壳激活未观察到 1.0.0 崩溃；这不能否定用户反馈，也没有发布修复版。** 当时未覆盖：26300 主版本（后续独立 EXE 回归见下节）、用户的扩展版本、“只有 HEIF 没有 HEVC”、用户的显卡/驱动/注入叠加层、用户自己的照片。

### 商店正式版实测（2026-10-22，经用户同意安装，测后已卸载）

- `winget install --id 9PCG7D394JFC --source msstore` 装上商店 1.0.0.0（SignatureKind=Store，包族 `ice.LiteView_rz8m2sa64m4em`）；安装后的 EXE SHA256 仍为 `21ddc58f…ac6a`，商店重签没有改动 EXE。
- 在 LiteView 自己的包身份内（`Invoke-CommandInDesktopPackage`，就地运行已安装 EXE）：`--diagnostics` 报告 packaged=true、AUMID `ice.LiteView_rz8m2sa64m4em!LiteView`、配置目录即 WinRT LocalFolder；115 个样本 GUI 回放 84 显示 / 31 无法打开，与非打包逐个一致；60 ms 翻页 2 轮正常退出。包数据目录只多出 MSIX 注册表虚拟化文件（`SystemAppData\Helium\User.dat` 等）。
- 真实外壳激活，经包 ProgID 的 DelegateExecute（即双击/右键 verb 实际走的路径）：开始菜单式冷启动空窗口；运行中再“打开” 45 MP HDR HEIC 和 iPhone 拼块 HEIC（单实例转发）；`edit` verb 打开 VVC `.heif`；关窗；从 HEIC 冷启动；`mosaic` verb 冷启动打开 iPhone HEIC 并进入马赛克工具。全部正常显示/提示，三次关闭退出码均为 0，单实例始终只有一个进程；Application Error/WER/Hang、TWinUI 激活失败、修复注册均为 0 条。VVC 文件在正式版上仍显示“文件已损坏或不完整”，即上节已修复的旧行为。
- 测试工具误用 `IApplicationActivationManager::ActivateForFile` 时，Windows 报“此应用不支持指定的协定或未安装”（TWinUI 5961），随即以 ForceTargetApplicationShutdown + RepairAppRegistration 修复注册，**强制结束了正在运行的 LiteView**，修复注册本身失败（0x80073CF9），没有任何崩溃事件，包状态仍为 Ok，之后的激活全部正常。LiteView 的 `.heic` ProgID 与照片、画图、截图工具的形态完全相同（`Windows.File`、`DesktopAppXActivateOptions=32`、同一 DelegateExecute），真实双击和 verb 路径没有出现这种情况，因此判断为测试调用方式不对，不是 LiteView 缺陷；没有对其它应用重复这个调用（会强制关闭它们）。用户报告的“已停止工作”属于崩溃提示，强制结束不会弹它，这条路径不太可能就是原报告。
- 卸载后：商店包、`Packages\ice.LiteView_rz8m2sa64m4em` 数据目录和 AppX ProgID 均已删除；GitHub 版的注册和 `.heic` 默认程序（`LiteView.heic`）未变。

下一步仍取决于真实故障签名：若故障模块是 `msheif_store.dll`、`HEVCDECODER_STORE.dll`、`mfplat.dll` 或 `windowscodecs.dll`，方向是把 HEIF/AVIF 等 WIC 系统解码放进子进程隔离，扩展崩溃只让该图显示“无法打开”；若是显卡驱动或叠加层模块，需要结合调用栈及空启动/独立解码对照区分渲染初始化与 WIC 下游（HEIC 链也涉及 D3D11），不能直接排除 HEIC。拿到签名前不做“猜测式兼容修复”。

开发侧工具与证据在忽略目录 `dist/heic-repro-20261021/`：`probe/`（调用 LiteView 解码/Loader 的独立 harness，含逐步 WIC HRESULT 的 `wic` 子命令）、`run_corpus.py`、`run_nav.py`、`run_verbs.py`、`gui_heic.py`、`make_corrupt.py`、`in_package.ps1`/`in_package_driver.py`（包身份内运行）、`store_activate.ps1`/`StoreActivation.cs`（经 ProgID 的真实外壳激活）、`events_dump.ps1`、`progid_dump.ps1`；各次运行的日志、截图、`summary.json`/`result.json`/`activation.json` 均在其中。回放复制的 EXE 共 258 份，经 SHA256 核对（正式包或本机修复构建）后已删除。

## 重启到 26H2 后收尾（2026-10-22）

### 环境与判定修复

重启后读取系统注册表确认 **Windows 11 Pro 26H2 / 26300.9711**；HEIF 1.2.48.0、HEVC 2.5.33.0 未变。本轮没有重新安装 Store 包、修改文件关联或驱动，测试仍只在私有 desktop 内运行，不切换用户桌面。

复审发现旧测试有“误通过”路径，已通过反例重现并修正：

1. 包内 policy 返回码未参与判定；诊断/导航的超时 `None` 被当作未执行而通过。共享检查现要求每个已请求步骤明确为 0，缺失也失败；保留之前收集的证据。旧 Python driver 已接入 `packaging/windows_probe_checks.py`。
2. `Invoke-CommandInDesktopPackage` 不转交子进程退出码，旧 PowerShell 入口只打印结果。现在通过 `packaging/check_windows_probe_result.ps1` 拒绝缺文件、无效 JSON、非对象及非 pass 结果；外层入口的模拟 pass/fail/timeout/无效 JSON 对照全部通过。包数据快照也补上了删除/旧条目的差异。
3. 旧 GUI corpus 固定等 2.5 秒后，没见到显示日志就记为错误页。这不能区分慢解码、未完成和真实错误，也未检查截图。新增仅用于开发回放的 `waitloaded`：当前图真正显示或错误状态绘制后才继续；验证必须同时有终态、检查点、可读取且非空白的 framebuffer、私有桌面证据及正常退出。无图的 Pending 反例会超时，不能记作错误页。
4. 旧正式版没有 `waitloaded`；`--legacy-release` 仅可验证已显示图片，未显示且没有明确错误证据的情况标为 **unverified（不通过）**。历史 84/31 是旧脚本记录，不能说旧脚本已自动严谨验证了每个错误页；本轮修改版的重新回放弥补该证据缺口。
5. HEIF ftyp 长度不是 4 的倍数，或超过 4 KiB 完整扫描上限时，不再根据不完整品牌集合推断“纯序列”。成功解码路径仍不变。
6. 真实启动测试现在同时进入 CI 和 Release Windows job，避免 tag 发布绕过 main/PR 门禁；远程 CI 未触发，不能把本地通过写成远程完成。

### 实测结果

| 对象 | 测试 | 结果 |
| --- | --- | --- |
| 公开 1.0.0 EXE | 默认 / Vulkan / DX12 / GL 各空启动及 PNG，默认 JPEG/WebP，两种全新配置 | 12/12 通过 |
| 本轮修改版 | 相同启动矩阵 | 12/12 通过 |
| 本轮修改版 | 115 个公开 HEIC/HEIF，各自独立进程，等待实际终态 | 84 显示 / 31 错误页；0 意外崩溃、0 超时、0 unverified |
| 本轮修改版 | 32 个损坏样本（含普通 HEIC、序列、VVC） | 3 仍可解码 / 29 错误页；0 意外崩溃、0 超时 |
| 正式版及修改版 | 缺必需 DLL、缺 Vulkan loader 的隔离副本 | 缺 DLL 正确拒绝并报 0xC0000135；缺 Vulkan loader 正常回退 |
| 修改版 | Pending 故障注入、正常 PNG、损坏 PNG | Pending 按预期超时失败；正常/错误终态正确区分 |

119 项 Rust release 测试、release Clippy 全 targets（`-D warnings`）、release 构建通过。Python 全部打包/判定/真实启动测试 **41 项通过、无跳过**。PowerShell 7 和 5.1 的支持报告隐私、包回执、安装器进程树和 MSIX 模拟激活测试通过；EXE 图标、版本、DPI 资源检查通过。序列 C026、VVC bricks 的实际截图确认提示“不支持这种格式”；资源存在不等于菜单图标的视觉清晰度已验收。

- 正式 EXE SHA256：`21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a`。
- 修改版 GNU EXE SHA256：`f2c7cf4c1cd23f31cce03b617896d2ce8c39764563187e8909bea7cd99ef97bb`。不是商店签名的已安装版本，不替代未来 MSVC/商店候选的验收。
- 修改版的系统依赖审计为 21 DLL / 351 符号，全部从 System32 解析。首轮门禁因白名单未含系统自带 `msvcrt.dll` 报错，核对 Microsoft 系统文件后已补入并复测；没有放宽到 VCRUNTIME/MSVCP、MinGW 外部分发 DLL，也没有要求用户安装运行库。
- 新证据：忽略目录 `dist/followup-26h2-20261022/` 的 `matrix-summary.json`、`heic-corpus/`、`damaged-corpus/`、`final-gate.txt`、`rust-test.txt`、`clippy.txt`、`powershell-tests.txt`、`package-driver-controls-final/`、`package-wrapper-e2e/`。早期失败记录保留，最终验收以 final-gate 及矩阵记录为准。

当前源码开发侧复跑（需开发机已有验证依赖；不是交给普通用户的脚本）：

```powershell
$env:LITEVIEW_TEST_BINARY = (Resolve-Path 'target/release/LiteView.exe').Path
$env:LITEVIEW_CORPUS_BINARY = $env:LITEVIEW_TEST_BINARY
$env:LITEVIEW_TEST_OUTPUT = 'dist/startup-new' # 新目录
$env:LITEVIEW_CORPUS_OUTPUT = 'dist/corpus-controls-new' # 新目录
python -m unittest discover -s packaging -p 'test_*.py' -v
python packaging/windows_corpus_probe.py --binary $env:LITEVIEW_CORPUS_BINARY --corpus dist/heic-repro-20261021/corpus --output dist/heic-new
```

### 显卡 / DX 假设与下一步

目前不能把“Windows 11 两种渠道无法启动”直接归因于 DX。按可验证性排序：

1. **图形设备/窗口初始化失败。** Windows 默认先选 Vulkan，不是 DX12。`src/gpu.rs::pick` 只验证适配器枚举非空，`src/app/mod.rs::run` 随后只调用一次 `eframe::run_native`；找到适配器后设备/表面创建失败没有应用层再次选择后端。预测：相同故障机空启动也失败，换后端可能恢复。需要该设备对照；本机强制 Vulkan、DX12、GL 均成功，无法据此定位其驱动。
2. **图片触发的系统解码器/驱动问题。** 首图解码与建窗并行，WIC HEIC 链也加载 D3D11；即使故障模块是 GPU 驱动，也不能断言与 HEIC 无关。预测：空启动和 PNG/JPEG 成功，仅某类照片失败。要先区分触发操作再考虑解码子进程隔离。
3. **文件激活、配置、叠加层或安全软件等环境差异。** 不同渠道反馈未确认来自同一设备；不能只因有 GitHub 反馈就排除全部包/关联问题。预测：直接空启动、文件双击或新配置之间表现不同。

代码还有可观测性缺口：Windows release 为 GUI 子系统，普通 `run_native` 错误由 `main` 返回，没有原生错误对话框；日志默认也不落盘。可呈现“无反应”，但这不同于已经确认的原生访问冲突，尚未在报告设备上验证。后续需为初始化失败建立回归并设计有界回退/可见提示，不能简单循环重启或全局吞异常。

**本轮未修改默认后端、未发布、未宣称原崩溃修复。** 需要优先补充：从开始菜单不带图片能否打开；GPU 型号；可靠性历史记录的故障模块、异常码（如有）。普通用户不需要运行命令，不盲装 DirectX/.NET/WebView2。打印、HEIC 方向、跨平台/无障碍、签名和实体硬件验收仍保留在 TODO。

## 干净虚拟环境准备（2026-10-22，尚未运行）

用户只能获得“打开图片没有反应、无法启动”，因此提出使用虚拟机，不再以取得更多用户信息作为开发侧测试的前置条件。

- 硬件只读检查：Ryzen 7 9800X3D、约 47 GB 内存，主板 MSI MAG B850M MORTAR WIFI；`VirtualizationFirmwareEnabled=False`、`HypervisorPresent=False`，Windows Sandbox / Hyper-V / VMP / WHP 组件均未启用。需要用户在 BIOS 开启 SVM，组件安装需管理员权限；本轮未启用组件、改 BIOS 或重启。
- 优先 Windows Sandbox：微软 [FAQ](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-faq) 明确使用与主机相同的 Windows build，不必先下载独立 ISO。实际进入来宾后仍需记录并检查其版本。
- Codex 工作区 `outputs/LiteView-26H2-Sandbox/` 已准备约 69 MiB 离线包：正式 GitHub 安装器、修改版、受限 Python 测试运行时、四个开发样本和两份 `.wsb`。只映射只读输入及专用结果目录，网络/剪贴板/音视频/打印机共享关闭；第一组禁用 vGPU 使用 WARP，第二组启用 vGPU。
- 安装只发生在来宾；脚本要求 Sandbox 默认账户及专用映射，拒绝在主机运行。安装器/安装后 EXE/测试输入均核对哈希，结果缺失不得假报通过。沙盒未运行，只有工具的主机自检；没有虚拟机结果，也没有新故障复现。
- Sandbox 不是完整 Store 用户环境；24H2 起缺部分内置商店应用。vGPU 也不能模拟任意实体显卡驱动。若需要真实 Store 安装或完整系统生命周期，再单独准备微软 26H2 Enterprise 90 天评估 VM，遵守注册及许可条件，不绕过授权。

后续进展（同日）：用户明确允许启用沙盒组件。管理员辅助脚本执行成功（退出码 0），复查 `Containers-DisposableClientVM` 状态 Enabled、RestartNeeded=True，固件虚拟化仍为 False。使用 `-NoRestart`，没有自动重启、修改 BIOS 或启动沙盒。回执保存在测试包的 `sandbox-feature-result.json`；下一步由用户开启 SVM 并重启后，再实际运行隔离对照。

### SVM 开启后的复查与组件重试（2026-10-22 19:29）

- 用户已开启 SVM 并重启。19:27 只读复查 `VirtualizationFirmwareEnabled=True`、`HypervisorPresent=False`；当前启动时间为 19:09:45。管理员审计显示沙盒等组件已回到 Disabled，之前回执只是重启前提交成功，不是最终生效。
- 19:00:07 的 System / Hyper-V-Hypervisor 事件 42 明确记录“在 BIOS 中不存在或未启用 SVM”；随后容器管理器报“该系统上没有虚拟机监控程序”。19:05 CBS 记录容器基础层创建失败，错误 0x800706D9 / HRESULT_FROM_WIN32(1753)，外层为 0x800F0922 并回滚。这定位的是本机沙盒安装失败链，不是 LiteView 的故障签名。
- 当前 SVM 已开启；BCD 当前启动项未发现显式 `hypervisorlaunchtype Off`，BFE / MpsSvc / RPC 服务运行，不据此盲改启动配置、关闭安全策略或修改驱动。
- 按此前明确授权，19:29 再次以管理员权限执行 `Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All -NoRestart`。退出码 0，沙盒状态 Enabled、RestartNeeded=True；回执标记 `staged_pending_restart`，仍需用户正常重启 Windows 后验证实际生效，无需再改 BIOS，没有自动重启。
- 原回执保留；新回执 `host-diagnostics/sandbox-retry-20261022-192911.json`，System / CBS 窄范围错误记录及只读审计也保存在测试包 `host-diagnostics/`。尚无来宾 `finished.json`，WARP / vGPU 两组均未运行，原用户故障仍未复现。

## 26H2 沙盒实际测试（2026-10-22 19:44–20:18）

**结论：干净 26H2 的软件渲染启动测试通过，原用户故障未复现。请求启用 vGPU 的来宾实际仍使用 WARP，所以没有取得硬件 GPU 对照，也未覆盖真实 Store 包激活。** 本轮不改产品默认后端、不改主机驱动/安全策略、不发布所谓修复版。

### 版本与隔离证据

- 用户 19:38 重启后，主机 `VirtualizationFirmwareEnabled=True`、`HypervisorPresent=True`，Sandbox `InstallState=1`，虚拟机实际启动成功。原始输入 199 项 SHA256 均核对。
- 初始 `01-software-warp.wsb` 来宾报告 **24H2 / 26100.9711**，严格版本检查停止，保留错误结果；不能把主机 26H2 或 FAQ 的 same-build 表述直接当作来宾 26H2 验收。
- 在两个分别新建的来宾内使用已下载的微软 KB5121794：MSU SHA256 `bf43eb13b7ebde920c2201473bfd5c531b45e0f46acd8e0843f27f9744fb4b43`，微软签名 Valid，DISM 对完整 CAB 检查 Applicable=Yes；WUSA 返回 3010，包状态 InstallPending。各自仅重启来宾一次，恢复后实测 `DisplayVersion=26H2`、`CurrentBuild=26300`、`UBR=9711`，再进入严格测试。
- 未修改版本号冒充升级、未绕过签名/适用性、未下载 ISO。该来宾内启用包路线是本次受限实验成功，不声称微软对所有 Sandbox 升级情形提供支持保证。主机启动时间一直保持 19:38:07，未再次重启主机。
- 网络、剪贴板、音视频输入、打印机共享关闭，只映射只读测试输入/启用包及专用可写结果。正式 GitHub 安装器只在来宾安装，安装后 EXE 哈希核对为 `21ddc58f7d9d9e3d5589566e725fce1fc209288bfe82018adaaac6eaf058ac6a`；探针复制该 EXE 到各临时目录执行，不等于外壳文件关联或 Store 身份激活。

### 实际测试结果

| 会话 | 请求图形模式 | 实际应用渲染器 | 必测启动 | 控制测试 | 修改版 HEIC/HEIF | 旧正式版 HEIC/HEIF |
|---|---|---|---|---|---|---|
| 03 | vGPU Disable | DX12 / Cpu / Microsoft Basic Render Driver | 14/14 通过 | 19/19 通过 | 4/4 完成错误页 | 4 项 unverified |
| 04 | vGPU Enable | DX12 / Cpu / Microsoft Basic Render Driver | 14/14 通过 | 19/19 通过 | 4/4 完成错误页 | 4 项 unverified |

- 每组必测为正式 EXE、修改版各 7 项：默认/DX12 的空启动和 PNG、默认 JPEG/WebP、无预置配置 PNG。两组合计 28/28；有第一帧、图片显示及帧缓冲校验，不仅是进程退出 0。已抽查 PNG 与修改版错误页截图，错误页为不支持格式提示。
- 来宾未发现 HEIF/HEVC/AV1 商店扩展。修改版 8 次 HEIC/HEIF 均完成到错误页、无崩溃/超时，不能说图片成功解码。旧正式版缺终态协议，8 次结果保留 unverified，不计完整 HEIC 验收。
- 每组还有 8 项强制 Vulkan/GL 探索测试退出 1。日志分别是无可用 Vulkan surface、GL 1.1 不满足 3.3+。默认启动会自动选 DX12 并正常显示，因此这只是缺少指定后端的受控失败，**不是已复现用户默认启动故障**。
- 为排除“启用包/来宾重启才出现软件回退”的推断，新建 05 未升级、未重启的 24H2 基础来宾，请求 vGPU Enable；实际应用同样为 DX12 / Cpu / Basic Render Driver，PNG 探针通过。dxdiag 在 90 秒超时并终止，没有取得有效 dxdiag 报告；05 不计入 26H2 验收。
- 主机 Sandbox `AllowVGPU` 策略路径未配置，RX 9070 XT WMI 状态 OK；没有足够证据确定为何未投射硬件 GPU，不盲改其他显示设备或驱动。微软文档允许在不兼容图形环境回退 WARP，但不能仅凭此判定本机驱动存在缺陷。

### 证据与剩余工作

证据根目录：Codex 工作区的 `outputs/LiteView-26H2-Sandbox/`（本机，不入库），汇总 `results-index.json`，每组有 `environment.json`、`summary.json`、`finished.json`、逐例日志/截图/模块列表；启用包回执为各组 `enablement-result.json`。

- 03：`results/03-26h2-software-warp/run-20261022-200003-d4344d2e/`
- 04：`results/04-26h2-virtual-gpu/run-20261022-200812-2dba785c/`
- 05：`results/05-pre-reboot-vgpu/diagnostic-20261022-201428/`

已关闭本轮创建的测试沙盒，结果保留。硬件 GPU、真实 Store/外壳激活、用户具体图片仍未覆盖；启动设备/窗口创建后续失败缺重试及可见错误反馈仍是独立代码风险，不因软件环境通过而关闭。主故障与“菜单图标模糊”待办保持未完成，不把继续索取普通用户命令输出作为唯一后续路径。

## 根因定位：窗口出现之前的失败没有任何出口（2026-10-22 续）

**结论：1.0.0 的窗口先隐藏创建，画完第一帧才显示；在这之前任何失败——Vulkan 窗口表面 / 设备 / 交换链出错、显卡驱动或隐式层崩溃或卡住、系统字体解析 panic——用户都什么也看不到。** 这与“点了没反应 / 打开图片没有反应 / 无法启动”一致，并且商店版与 GitHub 版走的是同一段代码，能统一两个渠道的反馈。下表全部在本机用正式 1.0.0 EXE 复现；**报告用户机器上具体是哪一种触发，仍未直接观测到**（没有故障签名），所以修复同时覆盖全部已复现的路径，并补上能让下一次故障留下证据的记录。

### 复现方法

私有 desktop、Medium 完整性（经 Explorer 令牌启动，见下“测试环境偏差”），全新配置。自写 Vulkan 故障注入层只通过子进程环境变量（`VK_ADD_LAYER_PATH` / `VK_INSTANCE_LAYERS`）加载，不写注册表、不改驱动和系统字体；字体场景用子进程的 `WINDIR` 指向临时目录。stderr 重定向到文件（真实 GUI 启动没有 stderr）。

| 场景（注入点） | 1.0.0 | 修复后 |
| --- | --- | --- |
| 窗口表面不受支持（`vkGetPhysicalDeviceSurfaceSupportKHR` = false） | 退出 1，无窗口；stderr `incompatible_surface_backends: VULKAN` | 本进程换 DX12，正常显示 |
| 同上但强制 DX12（`WGPU_BACKEND=dx12`） | 正常显示 | — |
| 建设备失败（`vkCreateDevice` 返回错误） | 退出 1，无窗口 | 换 DX12，正常显示 |
| 交换链失败（`vkCreateSwapchainKHR`） | panic 退出 101（wgpu 默认把校验错误当致命），无窗口 | 捕获后换 DX12，正常显示 |
| 驱动在建设备时崩溃（访问冲突） | 0xC0000005，再启动仍崩 | 首次仍崩（WER 照常）；记录 `lvfault.dll+0x1A51 (main thread, Vulkan)`，再启动自动用 DX12 |
| 隐式层在建实例时崩溃 | 0xC0000005，再启动仍崩 | 同上，再启动用 DX12 |
| 驱动卡在 acquire / present，3 秒后双击图片 | 第一个进程隐身挂住；**双击的进程 0.1 秒把图片转给挂住的进程后退出——“打开图片没有反应”** | 挂住的进程不开管道；双击的进程等到判定卡住（约 4.6 秒）后用 DX12 打开图片 |
| `msyh.ttc` 换成单字体文件（字体集序号 1 不存在） | epaint `fonts.rs:990` panic 退出 101，无窗口 | 退回 0 号字体，正常显示 |
| 所有接口都失败（强制 Vulkan + 表面不受支持） | 退出 1，无窗口 | 错误对话框（错误原文 + 记录位置），确定后退出 1 |
| 对照：无注入 / 只加载观察层 / 中文用户名单实例转发 | 正常 | 正常 |

看似 `visible` 的只有 winit 内部消息窗口；1.0.0 在所有故障场景中 LiteView 主窗口始终不可见。

### 修复（随 v1.1.0 发布）

- `src/gpu.rs` + `src/app/mod.rs::run`：按 Vulkan → DX12 → GL 逐个试；窗口显示前 `run_native` 返回错误或 panic，就在本进程换下一个接口重试（界面状态尚未使用）。`WGPU_BACKEND` 指定时只用它。**默认首选仍是 Vulkan，没有改默认顺序。**
- `src/startup.rs`：启动前在配置目录 `startup.json` 记下接口与进程；第二帧开始时确认。上次没确认就退出或超过 8 秒仍未确认（卡住）→ 该接口排到最后。崩溃在解码扩展模块或工作线程上时不怪显卡接口。成功后从失败名单移除；换版本重新从首选试；全部失败时清空名单。
- Windows 原生崩溃：`SetUnhandledExceptionFilter` 只用栈写 `last-crash.txt`（异常码、模块 + 偏移、主 / 工作线程、接口），再交给原处理（仍显示“已停止工作”并上报 WER）。panic 写入 `errors.log`（超过 256 KB 轮换）。下次启动显示在“设置 → 关于”：图形接口、已跳过的接口、上次的问题——普通用户可直接截图，不需要命令行。
- 单实例管道改为第一帧确认后才开；另一个实例正在启动时，新进程先等它确认或判定卡住，不会把图片转给挂住的进程。
- `src/ui/fonts.rs`：先用 epaint 同版本的 skrifa 预检字体，序号不存在退回 0 号，无法解析就跳过（新增的 `skrifa =0.44.0` 已在依赖树里）。

验证：119→123 项 Rust release 测试（新增启动判定、崩溃记录归因、接口排序、坏字体不 panic）、release Clippy 全 targets `-D warnings`、41 项 Python 打包/启动测试、12 组真实启动矩阵（默认/Vulkan/DX12/GL、PNG/JPEG/WebP、两种全新配置）全部通过。修复版 SHA256 `ee9c30d1ba4f7d5743679e2b274e986015ac0aa31a7f6605fe44794664bd5e04`（本机 GNU 构建，不是商店候选）。证据与工具在忽略目录 `dist/vk-fault-20261022/`（`runs/old-1` 为 1.0.0，`runs/fix-1`、`fix-2` 为修复版）。

### 测试环境偏差与排除项

- **此前所有探针都在提升权限（High IL）的 shell 里运行**，真实用户从资源管理器 / 开始菜单启动是 Medium IL。Vulkan 加载器在提升权限进程里忽略 `VK_ADD_LAYER_PATH` 等变量，第一次注入因此没有生效。本轮改用 Explorer 令牌的 Medium IL 重跑，对照场景同样正常，所以权限差异本身不是故障原因，旧结论不受影响，但之后的真实启动测试应在 Medium IL 进行。
- 已排除：中文 `USERNAME` 的单实例管道转发（0.1 秒转发、图片显示）；UI 自动化客户端反复遍历窗口（输入法 / 读屏 / 取词同类访问，6 轮无异常）；截断的 `msyh.ttc`（目录完整时不会 panic，只有字体集序号对不上才会）。
- 本机 WER 被策略关闭（HKLM `Windows Error Reporting\Disabled=1`），所以这里崩溃不弹窗；普通电脑上同样的 0xC0000005 会显示“LiteView 已停止工作”，与第一条反馈吻合。
- 26H2 不是必要条件：本机与用户另一台 26H2 电脑都正常；触发取决于各机器的驱动、隐式层（本机就有 OBS、WeGame、GamePP、Steam 四个）和字体状态。

### 仍未覆盖

- 报告用户机器上的具体触发；发布修复版后，用户若仍遇到问题，“关于”页的接口与上次问题即可作为故障签名。
- 第一次崩溃本身仍会发生（之后才会自动换接口）；`__fastfail`、栈溢出不经过未处理异常过滤器，不会有模块记录；卡住的进程不会被结束，留在后台直到重启或手动结束；正在启动的另一个实例卡住时，第二次双击最多等 8 秒。
- 商店签名包身份与真实文件关联激活下的修复版、硬件 GPU 以外的真实厂商驱动（Intel / NVIDIA）、HEIC 解码扩展崩溃分支、“菜单图标模糊”仍待验证。

### v1.1.0 正式包复核（2026-10-22）

GitHub Release v1.1.0（Actions 运行 39815282764，四目标构建与发布全部成功）中的 Windows EXE 为 MSVC 静态 CRT 构建，SHA256 `603753626bb9b6b7a6930cc9a4d56fcbaa3e76d962a8408246902519aa7775c3`，与商店候选 `LiteView-store-1.1.0.0-x64.msix` 内的 EXE 逐字节相同。用同一故障注入矩阵（Medium IL、私有 desktop）复测这个正式 EXE：12 个场景结果与本机 GNU 修复版一致——表面不受支持 / 建设备失败 / 交换链失败本进程换 DX12；驱动与隐式层崩溃再启动换 DX12，崩溃记录为 `0xC0000005 lvfault.dll+0x1A51 (main thread, Vulkan)`；两种卡住场景双击图片约 6 秒后用 DX12 打开；单字体 `msyh.ttc` 正常显示；全部失败时 0.4 秒弹出错误对话框。证据 `dist/vk-fault-20261022/runs/rel-1/`。CI（运行 39812306162）Windows 真实启动 19 项测试通过、无跳过。

### 之后：Windows 改用 Direct3D 11（2026-10-23，未发布）

为降内存，Windows 版不再用 eframe / wgpu，改为 winit + egui-winit + 自己的 Direct3D 11 画法（`src/app/host.rs`、`src/platform/d3d11.rs`）。启动保护照旧：接口顺序改为 `d3d11` → `warp`（硬件特性级 11.1 ~ 10.0，不需要 DX12；不行用系统自带的 WARP 软件渲染），建设备前 `begin`、第一帧前崩溃 / 卡住下次先用 WARP，`LITEVIEW_RENDERER=d3d11|warp` 强制指定（不记失败）。程序不再导入或加载 `vulkan-1.dll`、`d3d12.dll`，上面的 Vulkan 故障注入只适用于 1.1.0；对应的 CI 对照改为「强制 WARP 能显示」和「启动记录里上次 d3d11 没画出第一帧 → 这次用 WARP，失败记录与『关于』里的说明正确」（`packaging/test_windows_startup.py`）。

另修一处：启动超过 8 秒时再双击，第二个实例会把第一个的接口判成失败并换上自己的启动记录；第一个之后画出第一帧时，原来只在记录仍是自己的时才把接口洗白，于是以后每次都排到后面（在 Windows 上就是一直用 WARP 软件渲染）。现在按本进程实际用的接口洗白，不动别的实例的记录（`startup::on_first_frame`，单元测试覆盖）。
