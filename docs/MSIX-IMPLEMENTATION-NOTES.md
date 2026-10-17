# LiteView MSIX 实施笔记

核查日期：2026-10-17。仅依据微软第一方文档；本文没有修改软件代码、安装包/证书或执行商店提交。清单示例是实施模板，不是已通过 MakeAppx、安装或商店认证的产物。

## 推荐决策

- 首版采用包含现有 x64 EXE 的完整 MSIX：`packagedClassicApp` + `mediumIL` + `runFullTrust`，最低系统 `10.0.19041.0`。[1][2]
- 包身份来自 Partner Center；产品版本与商店包版本分开。商店包用如 `1.0.0.0`，不能直接照搬 `0.1.3.0`。[11]
- 图片关联通过清单登记；有命令行参数的节点必须用 `uap3:Verb`，不是 `uap2:Verb`。[3][4]
- 默认关联状态：商店版比较 `ASSOCSTR_APPID` 与当前完整 AUMID；非打包版继续精确比较自己的 ProgID。不用显示名、EXE basename 或字符串包含判断。[8][9]
- 配置优先取 `Windows.Storage.ApplicationData.Current.LocalFolder.Path`；可在一次性 MTA 工作线程获取并缓存，避免改变 UI 线程的 COM 模型。[12][14]
- 首先在隔离 Windows VM/CI 中注册未打包布局验证；签名安装测试也在隔离环境进行，不导入用户宿主的根证书。[1][15][16]

## 1. Win32 最小清单与 19041 兼容性

微软手动打包文档直接给出 `packagedClassicApp` + `mediumIL` 的完整模板。`uap10` 从 Windows 10 2004 / build 19041 开始可用；当 `MinVersion >= 10.0.19041.0` 时，官方推荐这两个属性，而不是旧 `EntryPoint="windows.fullTrustApplication"`。两者等价，不需要同时写；写了相互矛盾的值会报错。`mediumIL` 不是管理员权限，`runFullTrust` 是该桌面应用所需能力。[1][2]

以下为无关联时的基础模板。`Example.LiteView`、`CN=ExamplePublisher` 必须在正式提交前替换为 Partner Center 的精确身份，PublisherDisplayName 也必须正确；不从 GitHub 用户名推断证书主体。图像文件必须真实存在。[1]

```xml
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap uap10 rescap">
  <Identity Name="Example.LiteView" Publisher="CN=ExamplePublisher"
            Version="1.0.0.0" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>LiteView</DisplayName>
    <PublisherDisplayName>ExamplePublisher</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Resources>
    <Resource Language="zh-CN" />
    <Resource Language="en-US" />
  </Resources>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop"
                       MinVersion="10.0.19041.0"
                       MaxVersionTested="10.0.26200.0" />
  </Dependencies>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
  <Applications>
    <Application Id="App" Executable="LiteView.exe"
                 uap10:RuntimeBehavior="packagedClassicApp"
                 uap10:TrustLevel="mediumIL">
      <uap:VisualElements DisplayName="LiteView" Description="Image viewer"
                          Square150x150Logo="Assets\Square150x150Logo.png"
                          Square44x44Logo="Assets\Square44x44Logo.png"
                          BackgroundColor="transparent" />
    </Application>
  </Applications>
</Package>
```

`MaxVersionTested="10.0.26200.0"` 是针对当前测试计划的示例，不是已完成该版本测试的声明；生成器应允许明确指定实际最高已测版本。它是兼容行为/重新索引设置，不是操作系统最高安装限制。首版仍需在 19041 级别的 Windows 10 环境验证，而不是在 Windows 11 能启动就宣称所有 Windows 10 可用。[10]

`Application Id="App"` 发布后保持稳定。AUMID 是包族名加 `!App`，不是包全名加 `!App`；包族名不含版本和架构。运行时优先调用 `GetCurrentApplicationUserModelId` 获取完整值，不手工推导发布者散列。[2][17]

## 2. 图片关联与编辑/马赛克右键

对有参数的桌面文件激活，按微软完整示例的命名空间组合：`uap:Extension` → `uap3:FileTypeAssociation` → `uap2:SupportedVerbs` → `uap3:Verb`。**`uap2:Verb` 没有 `Parameters` 属性**；`uap3:Verb` 从 build 14393 开始支持该属性，在 19041 下可用。[3][4][5]

在根 Package 增加 `xmlns:uap2="http://schemas.microsoft.com/appx/manifest/uap/windows10/2"`、`xmlns:uap3="http://schemas.microsoft.com/appx/manifest/uap/windows10/3"`，并在 `IgnorableNamespaces` 中加入它们。然后在 Application 内、VisualElements 后增加：

```xml
<Extensions>
  <uap:Extension Category="windows.fileTypeAssociation">
    <uap3:FileTypeAssociation Name="liteviewimages"
                             Parameters="&quot;%1&quot;"
                             MultiSelectModel="Single">
      <uap:SupportedFileTypes>
        <uap:FileType>.png</uap:FileType>
        <uap:FileType>.jpg</uap:FileType>
        <uap:FileType>.jpeg</uap:FileType>
      </uap:SupportedFileTypes>
      <uap2:SupportedVerbs>
        <uap3:Verb Id="edit" Parameters="--edit &quot;%1&quot;"
                   MultiSelectModel="Single">Edit with LiteView</uap3:Verb>
        <uap3:Verb Id="mosaic" Parameters="--mosaic &quot;%1&quot;"
                   MultiSelectModel="Single">Mosaic with LiteView</uap3:Verb>
      </uap2:SupportedVerbs>
    </uap3:FileTypeAssociation>
  </uap:Extension>
</Extensions>
```

实施注意：

- `Name` 必须小写、无空格；登记的扩展名应来自 LiteView 的真实解码能力。示例只列三种，不代表完整产品支持列表。[5]
- `%1` 必须加引号并在 XML 中写作 `&quot;`，避免中文、空格等路径被拆开。全信任桌面应用接收的是参数；UWP 则是不同的激活事件机制。[3]
- `Single` 是当前只处理一个路径的保守起点。如果实现一次接收多文件，再按官方 `Player`/`Document` 行为调整，并单独验证单实例转发，不直接宣称批量打开支持。[3][4]
- FileTypeAssociation 可以提供“打开方式”和特定文件类型的编辑 verb；**不保证出现在 Windows 11 首层现代右键，也不是针对任意文件/文件夹的全局扩展**。首版按关联文件/经典菜单体验验收。现代首层通用菜单需要 `IExplorerCommand`、COM 注册及 `windows.fileExplorerContextMenus`，是另一项实现，不应只加这几行 XML 就宣传已支持。[6]
- 这些清单注册不是用户默认应用选择。不要写 UserChoice、抢默认或用 `AllowSilentDefaultTakeOver` 名字猜测可以批量强制设默认。

## 3. 默认应用设置直达链接

官方支持：Windows 11 21H2、22H2 安装 2023-04 累积更新，以及 Windows 11 23H2 或以后，可以使用应用专属查询参数。[7]

```text
ms-settings:defaultapps?registeredAUMID=<URI-escaped-current-AUMID>
```

`registeredAUMID` 用于 Package Manager 通过声明文件类型或 URI 协议的清单注册的应用。为 GitHub 版使用 `registeredAppUser=<注册名称>`，不能混用两种参数。[7]

建议用真正的 URI component 转义函数处理整个 AUMID，保留 `ms-settings:defaultapps` 无参数回退。系统启动成功只代表设置应用接受了链接，不证明一定跳到了目标页；因此 UI 始终保留手动查找 LiteView 的说明。Win10 直接使用通用页，不承诺应用页直达。

官方另提醒：OS 升级后，可能需增加清单 `MaxVersionTested` 才能触发重新索引；文档给出的阈值是 21H2 的 `10.0.22000.1817`、22H2 的 `10.0.22621.1555`。不能为了兼容 Win10 把 MaxVersionTested 永久锁在 19041，也不要把它误当 MinVersion 提高到 Win11。[7][10]

## 4. 默认应用身份查询

`ASSOCSTR_APPID` 的官方定义就是：返回用户为该文件类型或 URI 协议配置的默认应用 AppUserModelID，Windows 10 引入。因此可用：

```text
AssocQueryStringW(ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_APPID,
                 L".png", NULL, buffer, &length)
```

采用两次调用取长度再读值。第一次 `pszOut=NULL` 返回 `S_FALSE` 并给出字符数，这不是查询失败；第二次必须成功且获得非空字符串。输入必须是扩展名，而不是 LiteView 的 ProgID，否则查到的是注册信息而不是当前用户的该格式默认。[8][9]

推荐判定：

- 当前进程有包族名和 AUMID：将查询出的完整 APPID 与当前 AUMID 精确比较；不把其他包族、旧包全名或 GitHub 版算成当前版默认。
- 当前进程明确没有包身份：沿用既有 `ASSOCSTR_PROGID` 与合法 LiteView ProgID 的精确匹配；不要仅凭 APPID 显示名称匹配。
- 查询失败、空值或当前包没有可用 AUMID：显示未知/待设置，记录诊断，不回退到“含 LiteView 就算成功”。包身份错误不能被当成“肯定是非打包版”。

启动时缓存 GetCurrentPackageFamilyName / GetCurrentApplicationUserModelId 合理；它们不随单次进程运行改变。关联结果本身仍需焦点恢复/定时刷新，不能随进程身份永久缓存。[17][18]

包族名/AUMID 可加入单实例管道和互斥量标识，使商店版激活不被 GitHub 版吞掉；保持用户范围，使用稳定、无路径分隔符的编码/散列。该隔离属于工程建议，不是权限边界；现有管道 ACL/用户验证仍须保留。

## 5. 配置目录：LocalFolder 优先，避免猜测重定向

MSIX 安装目录受保护且只读，不能启用程序旁 portable 配置。`ApplicationData.Current.LocalFolder` 返回当前应用实际本地数据目录；取得 `.Path` 后可继续用现有 Rust 文件 I/O，不必把所有配置读写重写成 WinRT StorageFile。[12][13]

注意 LocalFolder 内容在支持应用备份的设备上可参与备份。如果要存只应留在当前设备、无需备份的缓存，使用 `LocalCacheFolder` 而非把它和长期设置混在一起。[12]

### 不影响 UI 线程 COM 模型的建议

仅在确认包身份后，在启动时创建一次普通工作线程：该线程调用 `RoInitialize(RO_INIT_MULTITHREADED)`，取 `ApplicationData.Current.LocalFolder.Path`，转换为拥有所有权的 `PathBuf`，释放 WinRT 对象，再平衡 `RoUninitialize`，最后返回并缓存路径。每次成功的初始化（包括 `S_FALSE`）都要平衡释放。这样无需在 UI 主线程选择/改变 COM apartment，也不应每帧创建线程。[14]

`config_dir()` 对外可以仍返回 PathBuf，内部缓存查询的 `Result`；失败时记录明确错误并采用不会与非打包版混写的保守策略。不得因为一次 WinRT 失败就去包目录写文件或覆盖 GitHub 版的设置。

### 关于手拼 LocalState 路径

`SHGetKnownFolderPath(FOLDERID_LocalAppData, KF_FLAG_NO_PACKAGE_REDIRECTION, ...)` 的官方保证只是“返回未打包进程会获得的路径”。它**没有保证**在这个路径后拼接 `Packages/<PFN>/LocalState` 永远等价于 `ApplicationData.LocalFolder`。此布局常见，但不是这个 API 的路径契约；正式实现优先使用 LocalFolder，不把手拼路径当成已核实的通用 accessor。[19]

`KF_FLAG_FORCE_APP_DATA_REDIRECTION` 文档虽提到返回与 LocalFolder 等效的目录，也明确说明它用于 .NET 兼容性、**不应由应用直接使用**，不宜为绕过 WinRT 而选用。[19]

不应假设所有 `APPDATA` 读写均自动隔离。微软文档区分应用类型、虚拟化与 OS 版本，1903+ 对已有真实 AppData 文件的读写和新文件重定向也不同；显式使用实际 LocalFolder 比依赖这些兼容层更可预测。旧 GitHub 配置如需迁移，应明确复制/导入，不与旧版本同时写同一个文件。[13]

## 6. 构建、未签名提交与安全测试

### 纯构建/商店产物

`MakeAppx pack /v /h SHA256 /d <staging> /p <output.msix>` 负责打包，不需要在构建步骤中购买/调用公众信任证书。不要使用 `/nv` 跳过验证。MakeAppx 的语义验证有限，成功不能证明包一定可安装。[20]

商店的 MSIX 提交流程不要求开发者先购买/提供 CA 证书，微软认证后重签；可保留未签名正式身份 MSIX 作为商店上传产物。此“未签名商店产物”与下面带特殊 OID 的 unsigned 测试身份不是一回事。[11][21]

### 无证书测试：优先开发布局

微软手工打包文档明确支持在打包/签名前从 staging 注册：

```powershell
Add-AppxPackage -Register '<staging>\AppxManifest.xml'
```

在具有开发部署条件的隔离 VM 中执行，不加 `-DisableDevelopmentMode`；该参数用于已安装/暂存包的重注册，不是放行未签名开发布局。需要启用 Developer Mode 或相应开发策略时，应在测试环境配置，不擅自更改用户宿主。该操作仍然注册应用、文件类型和系统状态，不是只读测试。[1][15]

### Windows 11 完整 unsigned 包测试

Windows 11 有 `Add-AppxPackage -Path <test.msix> -AllowUnsigned`，但须同时满足官方专用测试要求：[16]

- 测试 Publisher 加入 `OID.2.25.311729368913984317654407730594956997722=1`。
- 含 EXE 的未签名包通常需要管理员权限，并影响全部用户。
- 该 OID 使测试包身份与签名包不同，防止冒充/冲突；测试结果不等于真实 Store 身份的升级迁移已测过。
- 此特性不用于广泛分发。发布产物必须移除 OID 并使用正式身份；不是把参数作为普通消费者安装的替代方案。

因此，若用它做 CI 测试，应生成独立测试副本，不修改或覆盖正式待上传 MSIX。

### 真实签名安装回归

如需验证完整签名、更新和卸载，在一次性 VM/CI 中创建测试证书、签名并只信任该测试证书。官方测试指引使用 `LocalMachine\TrustedPeople`，**不是 Root**，证书 Subject 必须与测试清单 Publisher 一致。这仍改变测试机器的信任，不是在用户电脑上“无副作用”操作；本次没有执行。[22]

## 7. 版本与架构

- 商店 Windows 10/11 包版本第四位保留给 Store，上传时为 `0`；前三段为整数，第一段不能 `0`。为 LiteView 的 `0.1.x` 产品版本设置独立、单调增长的商店版本，例如首包 `1.0.0.0`；生成器显式校验，不默默截断 SemVer 或添加任意第四位时间戳。[11]
- 主二进制为 x64 时，Identity 用 `ProcessorArchitecture="x64"`；不能标 `neutral` 伪装架构无关。官方接受单架构 MSIX，不要求第一版同时提供 x86/ARM64。仅在真实编译和测试对应二进制后增加其他架构。[1][11]
- `MinVersion="10.0.19041.0"` 是本方案的最低 OS 选择，不是所有 MSIX 的统一最低版本。不要将其宣传成兼容所有 Windows 10；在 19041 之前明确不适用。[2][10]
- 是否依赖 VCLibs、其他 runtime 或原生 DLL 取决于实际 EXE，不因“Win32”就随意添加框架依赖。首包应逐项核验并包含所需授权文件、资源和运行库；安装和启动需要在干净系统验证。

## 最小验收矩阵

1. MakeAppx 保留语义校验，检查清单身份、版本、命名空间与文件路径；不要把 XML 可解析当作 schema 已验证。
2. Windows 10 19041+ 和 Windows 11 各一次干净安装/启动；Win11 设置直达及通用页回退。
3. 双击图片、打开方式、编辑/马赛克 verb、中文/空格路径；首实例与已有实例均测试。
4. 商店和 GitHub 版并存：默认状态分别识别，单实例管道不串台，配置不互相覆盖。
5. 配置落在实际 LocalFolder；包内无 portable 标记；升级保留设置、卸载后符合预期。
6. 正式身份与测试 OID 身份严格分离；无测试证书、密钥或虚假主体进入正式发布产物。

## 第一方来源

1. [Generating MSIX package components / 手工打包模板与开发注册](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-manual-conversion)
2. [Application schema / uap10 19041 与激活组合](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-application)
3. [Integrate your desktop app with Windows using packaging extensions](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/desktop-to-uwp-extensions)
4. [uap3:Verb](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap3-verb) / [uap2:Verb](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap2-verb)
5. [uap3:FileTypeAssociation](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap3-filetypeassociation) / [uap2:SupportedVerbs](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap2-supportedverbs)
6. [Add a File Explorer context menu command to a packaged desktop app](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/integrate-packaged-app-with-file-explorer)
7. [Launch the Default Apps settings page](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-default-apps-settings)
8. [ASSOCSTR / ASSOCSTR_APPID](https://learn.microsoft.com/en-us/windows/win32/api/shlwapi/ne-shlwapi-assocstr)
9. [AssocQueryStringW](https://learn.microsoft.com/en-us/windows/win32/api/shlwapi/nf-shlwapi-assocquerystringw)
10. [TargetDeviceFamily](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-targetdevicefamily)
11. [MSIX app package requirements / 商店签名、版本、架构](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)
12. [ApplicationData.LocalFolder](https://learn.microsoft.com/en-us/uwp/api/windows.storage.applicationdata.localfolder)
13. [Understanding how packaged desktop apps run on Windows](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes)
14. [RoInitialize](https://learn.microsoft.com/en-us/windows/win32/api/roapi/nf-roapi-roinitialize)
15. [Add-AppxPackage / Register 与 DisableDevelopmentMode](https://learn.microsoft.com/en-us/powershell/module/appx/add-appxpackage)
16. [Create an unsigned MSIX package](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package)
17. [GetCurrentApplicationUserModelId](https://learn.microsoft.com/en-us/windows/win32/api/appmodel/nf-appmodel-getcurrentapplicationusermodelid)
18. [GetCurrentPackageFamilyName](https://learn.microsoft.com/en-us/windows/win32/api/appmodel/nf-appmodel-getcurrentpackagefamilyname)
19. [KNOWN_FOLDER_FLAG](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/ne-shlobj_core-known_folder_flag)
20. [Create an MSIX package or bundle with MakeAppx.exe](https://learn.microsoft.com/en-us/windows/msix/package/create-app-package-with-makeappx-tool)
21. [Code signing options / MSIX submissions: no signing needed](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)
22. [Create a certificate for package signing / TrustedPeople 与安全影响](https://learn.microsoft.com/en-us/windows/msix/package/create-certificate-package-signing)
