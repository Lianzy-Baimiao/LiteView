# Windows 签名与 Microsoft Store 上架

核查日期：2026-10-17（Asia/Shanghai）。本文区分官方规则、当前版本观察和后续建议；不是已完成签名或上架的声明。

## 结论

- LiteView 可以走 Microsoft Store 的桌面应用发布路线，不需要把 Rust/egui 重写成 UWP。是否通过仍取决于打包、测试、开发者身份验证和商店审核。[1][2]
- 建议保留 GitHub 安装版/绿色版，新增 **MSIX 商店版**：通过认证后，微软免费重签商店包，不需要为这条路线另买公众信任代码签名证书。[1]
- 如果直接把现有 **EXE/MSI 安装器** 提交商店，微软不会代签；安装器以及其中全部 PE 文件必须由开发者用受信任代码签名证书签名。[2]
- 商店的签名权益不等于给 GitHub EXE 签名，也不会让已经发布的 v0.1.2 自动变成签名版。商店外独立分发的包要单独规划签名。[1]

## “Windows 未签名”是什么意思

Authenticode 数字签名让 Windows 验证发布者身份和签名后的文件完整性。“未签名”表示没有这类可验证的发布者签名，不等于查出病毒，也不等于程序不能运行；同样，有签名也不保证程序没有漏洞或恶意行为。EXE 图标、版本信息和 SHA-256 校验值都不能代替发布者数字签名。[3][4]

用户下载未签名程序时，可能看到“未知发布者”或 SmartScreen 的“Windows 已保护你的电脑”。企业策略或 Smart App Control 也可能阻止执行。具体行为受 Windows 配置和文件信誉影响。[4]

**签名不保证立即消除 SmartScreen 提示。** 官方文档明确：OV、EV 和 Artifact Signing 的新文件都可能需要积累信誉；EV 已不再自动绕过 SmartScreen。不要为了“保证不弹蓝框”盲目购买更贵的 EV，也不要承诺某个下载量或天数后一定消失。商店安装路径与直接下载路径不同。[4]

## 当前 LiteView 状态（本次仓库与发行文件观察）

这些是本次技术检查结果，不是微软对软件的认证：

- v0.1.2 的主 EXE 与 Inno Setup 安装器，Authenticode 检查结果均为 `NotSigned`。
- v1.0.0 已于 2026-10-17 发布为正式版 / Latest；从该次发布产物下载的主 EXE 与安装器仍为 `NotSigned`。MSIX 已通过一次性 runner 的安装、激活、升级和卸载测试，但没有提交商店审核；完整证据见 `RELEASE-1.0.0-VALIDATION.md`。
- 软件是 MIT 开源的普通 Win32 桌面应用，日常运行不要求管理员权限，命令行可以接收图片路径。
- 默认配置保存在用户 `APPDATA` 下；带 portable 标记时才写到程序旁边。
- 初次核查时仓库没有 MSIX 清单。后续已新增 `packaging/msix/AppxManifest.xml`、`packaging/msix.py`、多倍率图标和中英文资源；本机 MakePri/MakeAppx 已实际生成通过默认语义校验的未签名候选包，仍不等于安装验收或商店认证。
- 文件关联逻辑已增加运行时包身份分支：商店版使用清单登记与 AUMID 查询，`--register` / `--unregister` 不修改独立版注册；系统默认选择仍由用户决定。
- 用户随后明确要求转正式版，产品版本更新为 `1.0.0`、商店包版本为 `1.0.0.0`；这是版本和发行策略，不是签名、商店上架或全部人工验收完成的声明。

### 已创建的商店产品草稿

2026-10-17，用户自行登录微软账号，并明确确认预留 `LiteView` 名称。Partner Center 随后显示该产品为 **MSIX 或 PWA 应用、处于草稿状态**。这不是提交审核、审核通过或已经上架。

| 字段 | Partner Center 显示值 |
| --- | --- |
| Store ID | `9PCG7D394JFC` |
| Package/Identity/Name | `ice.LiteView` |
| Package/Identity/Publisher | `CN=56079E3F-5861-4D9C-B9C3-93F76224C42E` |
| Package/Properties/PublisherDisplayName | `白描ice` |
| Package Family Name | `ice.LiteView_rz8m2sa64m4em` |

这些是包身份元数据，不是签名私钥或凭据。后续 MSIX 清单应使用后台分配的值，不能把显示名称 `LiteView` 直接当作 Package Identity。后台地址：[LiteView 产品概述](https://partner.microsoft.com/zh-CN/dashboard/products/9PCG7D394JFC/overview)。

预留页面提示必须在三个月内提交应用，否则会丢失保留名称。用户随后确认上传包并填写免费上架草稿，2026-10-17 已创建 Submission 1：包显示 `Validated`，CNY 0 定价、应用属性、中英文文案和 runFullTrust 用途说明已保存；设置为认证后仍需手动选择发布。没有提交认证或发布商店页面，截图、IARC、WACK 和人工验收仍未完成。完整进度见 `STORE-LISTING.md`。商店深层链接和 Web Store URL 在产品上线后才可用；本次未核实账号类型、国家/地区或身份审核状态。

## 方案 A：新增 MSIX 商店版（优先推荐）

### 官方规则

- MSIX/AppX 商店提交不要求购买 CA 信任证书，不需要提供 CA 的 `.pfx`/`.cer`，也不需要 USB 令牌或 HSM。认证通过后，商店使用微软证书重新签名包。[1]
- 普通全信任桌面应用可以打包；中等完整性级别（medium IL）的桌面应用应声明 `runFullTrust`。它不是管理员权限，也不表示应用自动变成 UWP/AppContainer。受限能力按 Partner Center 要求说明用途。[5]
- 文件类型通过包清单中的 `windows.fileTypeAssociation` 与 `SupportedFileTypes` 注册；右键命令也有对应清单扩展。注册为候选处理程序不等于强制替用户选择默认应用。[6][7]
- Win32/Desktop Bridge 产品必须提供隐私政策，即使主要功能是本地离线看图，也应准确说明图片、元数据及网络访问是否被收集或上传。商店还检查产品可测试性、功能完整性及宣传准确性。[8]

### LiteView 需要做的工作

1. 用现有 Rust EXE 构建 MSIX；使用 Partner Center 分配的包身份、发布者信息与正式图标。保留独立 GitHub 产物，不把现有绿色 ZIP 直接改后缀冒充 MSIX。
2. 商店包不带 portable 标记；确认配置写入可写的用户目录，测试打包后的路径/注册表重定向和卸载行为。
3. 在包清单登记图片类型、`runFullTrust` 及确实需要的右键功能；包内不沿用任意写外部注册表的安装逻辑。
4. 测试双击图片、带空格/中文的路径、多选激活、单实例转发、右键、默认应用页面、设置持久化、升级及卸载。迁移现有 GitHub 安装版时避免接管用户默认选择。
5. 准备隐私政策、支持地址、真实截图、商店描述、年龄分级和测试说明；通过 Windows App Certification Kit 等检查后提交审核。[1][8]

微软代签针对商店发布的包。本文不建议“下载商店签名包、拆出 EXE 后放 GitHub”，也不保证商店签名能任意复用于商店外分发；微软当前提交文档明确要求商店外 MSIX 分发者自行签名。[1]

## 方案 B：把 EXE/MSI 安装器上架商店

可以保留原安装技术，但不能跳过公众信任签名。官方要求包括：[2]

- `.exe` 或 `.msi` 离线安装器，不得运行时再下载安装主体。
- 安装器以及全部 PE 文件使用链到 Microsoft Trusted Root Program 成员 CA 的代码签名证书。
- 提交 HTTPS 的固定版本直链，提交后该地址的二进制内容不得更改；新版本使用新 URL。
- 静默安装，无安装界面；允许 UAC 对话框。

因此，这条路线对现有 Inno Setup 改造可能较少，但需要自行承担证书/签名服务以及安装器和版本地址的维护，不享受 MSIX 的免费包重签。[1][2]

## 直接下载版想签名，该做什么

1. **先确定签名主体。** 个人、个体经营者或公司应按实际法律身份申请，确认证书上希望展示的发布者名称；不要使用借来的公司资料或来源不明的共享证书。
2. **选合资格的服务。** 不满足微软云签名地区条件时，向 Microsoft Trusted Root Program 中合适的 CA 查询是否接受该国家/地区及个人/企业身份。价格、验证材料、令牌/云 HSM 与自动化方式以 CA 的实际报价为准，不把某一价格视作全球统一价。[9][10]
3. **本人完成身份验证和付费。** 公众信任代码签名通常需要硬件保护私钥（令牌/HSM 或托管云签名）；不要把可导出的私钥文件或密码提交到 Git。[9]
4. **接入发布流水线。** 先签主 EXE 和所附 PE，再生成并签安装器；使用 SHA-256 与 RFC 3161 时间戳；签完不再修改文件；验证签名和时间戳后生成 ZIP、校验和及发布附件。云服务具体命令按服务文档配置，传统硬件令牌不一定能直接用于 GitHub 托管 runner。[11]
5. **下一版本持续签名。** 保持一致发布者身份有利于信誉积累；不承诺签一次就没有任何 Windows 警告。[4]

本机验签可用 `Get-AuthenticodeSignature`；正式发布验证可用 `signtool verify /pa /all /v <文件>`。前者的 `NotSigned` 和后者验签失败都不应被包装成“已签名”。[11]

### Azure Artifact Signing（原 Trusted Signing）

- 当前名称为 **Artifact Signing**，微软文档也使用 Azure Artifact Signing；旧 Trusted Signing 名称及旧文档地址仍可能出现。[4][12]
- 服务当前 Public Trust 组织主体支持：美国、加拿大、欧盟、英国、澳大利亚、新西兰、日本、韩国、新加坡、瑞士、挪威、以色列；个人开发者仅支持美国、加拿大。[12]
- **如果申请主体/个人所在地是中国大陆，目前不在上述 Public Trust 资格名单内。** 不要因为能创建境外 Azure 资源或购买订阅就推定可以通过签名身份验证。Private Trust 没有这些地区限制，但它不能代替面向公众下载的 Public Trust。[12]
- Basic 参考价为 **US$9.99/月、5,000 次签名/月**；Premium 为 **US$99.99/月、100,000 次/月**，超额参考价 US$0.005/次。价格页说明这不是实际报价，币种、税费、协议及结算价可能变化；服务不按当月使用天数比例计费。[13]
- 服务托管密钥，可接 CI/CD；身份认证仍必须完成。当前快速入门提示身份验证可能需要 1–20 个工作日，补充材料时可能更长，不应承诺马上开通。[12]

地区信息采用服务自身快速入门页（更新于 2026-10-08）；Windows 签名选项概览仍只列较早的部分地区，因此不以该概览代替服务资格清单。[9][12]

## 开发者账号与费用

当前微软开户说明明确：通过新入口 [storedeveloper.microsoft.com](https://storedeveloper.microsoft.com/) 注册，**Individual 和 Company 两类账号都没有注册费**。不要继续引用过去的 US$19/US$99 当作当前统一规则；从 Partner Center、Xbox 或 Visual Studio 等旧入口进入可能仍显示旧流程。[14]

- Individual：文档定位为不涉及本人业务、行业或职业的独立开发者，以及业余、学校、个人或非商业项目。需要政府签发证件与自拍等身份验证。
- Company：除企业/组织外，也包括与本人业务、行业或职业相关的独立开发者和自由职业者。需要公司/商业资料及联系/任职验证；可用 D-U-N-S 或正式商业文件验证。
- **Individual 不能直接转成 Company**；要以公司身份发布，应另建 Company 开发者账号。应先确定真实发布主体，不能仅按“只有一个人开发”来选择账号类型。[14]

用户随后确认以中国大陆个人身份办理，并希望同时准备商店 MSIX 与 GitHub 直下载版签名。以下据此缩小选项，但没有核验用户证件、账号状态或个案签发结果。调查过程没有注册供应商账号、提交身份材料、联系销售、下单或开通付费资源。

## 中国大陆个人：GitHub 版签名候选

再次核查日期：2026-10-17。**“CA 提供个人产品”“CA 接受 CN 国家代码”“用户的具体订单通过验证”是三个不同层次。** 以下是两家 CA 的三个可询价产品，不是三个已确认会向该用户签发的服务。暂不考虑不支持大陆个人的 Artifact Signing。[12]

| 候选 | 官方确认的范围 | 尚须本人在付款前确认 | 适合的取舍 |
| --- | --- | --- | --- |
| SSL.com IV Code Signing + eSigner | IV 面向个人，不要求企业注册；通用接受国家列表明确含 China/CN；eSigner 支持 IV 与无人值守 CI/CD | 当前中国大陆自然人能否申请该 IV + eSigner 组合；具体中国证件/地址证明及中文姓名处理 | 官方自动化证据最完整，优先考虑 CI/CD 省事 |
| Certum Open Source Code Signing in the Cloud | 仅面向个人；开放源码项目；SimplySign 云密钥；每月 5,000 次签名限制 | 未找到明确针对大陆个人的签发承诺；需确认地区/证件、在售状态、非商业限制和自动化支持 | 如果一直免费开源，先核实这个低成本候选 |
| Certum Standard Code Signing in the Cloud | 明确可包含自然人数据；SimplySign 云密钥；每月 5,000 次限制 | 未找到明确针对大陆个人的签发承诺；需确认地区/证件、在售状态及无人值守能力 | 不希望受 Open Source 产品的非商业签名约束 |

### 1. SSL.com IV + eSigner

**资格与显示名称。** IV 产品页明确面向独立开发者、开源维护者、学生与业余开发者，证书/安装器展示经过验证的个人姓名，不要求注册企业。通用国家代码页明确接受 `China / CN`，这是正面地区证据，但该页面不是“中国大陆个人 IV + eSigner 一定通过”的专项承诺。申请仍受身份、地址和合规检查，购买前应获得产品级确认。[15][16]

**身份材料。** 官方指南支持政府签发的有效身份证件或护照及活体验证；也有人工上传途径，需要证件正反面/护照资料页和本人持证照片，可能需要额外地址材料，少数情况要求公证。指南的亚太例子没有逐项确认中国居民身份证，不能自行保证某种中国证件或中文账单一定被接受；应先确认是否需护照、翻译、公证及实名的拉丁字母拼写。[17]

**密钥与自动化。** eSigner 将私钥保存在其云 HSM，官方明确支持无人值守签名、Windows `signtool.exe`/CKA，以及 GitHub Actions。无需另买 USB 令牌。SSL.com 也有 CodeSignTool Action 示例；示例较旧，未来集成时应固定当前 Action 版本/提交并校验依赖，不能照抄 `@develop`。签名凭据只放受保护的 CI secrets，限制到可信发布分支/环境，不能交给外部 PR 或写进源码。[18][19]

**价格必须拆开看。** 官方 IV 页面显示一年基础证书 **US$129/年**，eSigner 是**另付订阅**，不是买证书就赠送永久云签名。2026-10-17 页面显示 eSigner Tier 1 低至 US$15/月，但月付/年付切换、签名次数与订单汇总的静态展示不足以确立完整首年报价；因此本文不把 `129 + 15 × 12` 当作已核实应付总额。应让供应商确认月/年付金额、计次方式、试用后扣费、超额和税费。[15][18]

硬件是替代路线，不应与 eSigner 一起默认加购：IV 页面当前 YubiKey 加价显示 US$379，而 eSigner 页面显示 US$249，两个官方页面不一致，不能据其中一个给用户承诺令牌到手价；还需确认寄送中国大陆、运费及税费。自带云 HSM 还有单独认证/基础设施费用，对 LiteView 目前没有必要。[15][18]

### 2. Certum：开源云签名与 Standard 云签名

**主体与使用范围。** Certum 的 Open Source Code Signing 只发给个人，Common Name 会加 `Open Source Developer`，Organization 字段为 `Open Source Developer`；不是只显示“LiteView”的任意品牌证书。官方要求公开项目页面能证明申请人与项目的关系。如果用于商业分发的软件，官方保留撤销证书的明确条款；MIT 许可本身允许商业使用，不代表该优惠证书允许所有商业用途。未来收费、双重授权或商业产品复用前必须确认或改用 Standard。Standard 明确支持自然人身份。[20]

**材料。** 两种产品的个人申请均需身份验证和以申请人名义出具的水电/电话等账单；可选自动验证、登记点、持证照片或公证等途径。Open Source 另需正在进行的公开开源项目地址。官方列举身份证件、护照、驾照等，但未在本次查到的产品/验证文档中明确承诺中国大陆证件和自然人均可通过，因此地区、账单语言与实名拼写仍需售前确认。[20]

**价格与云服务。** 官方商店显示 Open Source 云版 **€49 起**、Standard 云版 **€209 起**；这是云产品的展示起价，使用 SimplySign，不必另外买卡和读卡器，页面注明每月最多 5,000 次签名。最终有效期、税费、在售状态及是否另有自动化服务收费须按具体订单确认。商品页静态 HTML 同时包含不一致的库存提示，不能据此声称已经可立即购买或已经永久停售。[21][22]

不要把广告中的 **€25** 当作完整云签名价：那是 Open Source **激活码**产品，面向已有 Certum 加密卡和读卡器的人；官方卡/读卡器套装则是 €69 起。Standard 同样有激活码、硬件套装和云版，不是同一商品。[23]

**自动化边界。** 已读官方资料要求 SimplySign 手机应用生成访问码、桌面应用连接云端虚拟加密卡，并提供 SignTool/Jarsigner 指南。本次没有找到 Certum 对 GitHub 托管 runner 上“完全无人值守、无手机交互”作出的明确官方承诺，因此不能把“云密钥”直接等同“能原样接入现有 GitHub Actions”。可先按本人登录后的本地批量签名考虑；需要全自动时，购买前应确认官方支持的接口、会话和授权方式，不能采用导出私钥或绕过认证的方案。[21][22][24]

### 开源免费备选：SignPath Foundation

这不是给用户本人颁发的个人证书。官方条款说明证书颁给 **SignPath Foundation**，它作为已签软件的发布者；项目只能在受控、可验证的源码构建流程中使用。项目需已发布、持续维护、使用 OSI 批准的开源许可、无专有组件，成员使用 MFA，有明确签名角色、签名政策和隐私说明。**每个 release 仍需要人工批准签名**。[25]

LiteView 的 MIT 许可和已经发布 v0.1.2 是申请基础，但不足以保证录取：基金会明确要求可验证的项目信誉，有权拒绝，也没有承诺新项目必定获批。应作为零证书费用的申请候选，而不是“今天免费马上签好”的方案，更不能当作能签其他个人软件的通用证书。[25]

### 付款前只需确认这些问题

以下是建议由用户本人向选定供应商确认的事项；本次未发送询问或身份材料。

1. 是否当前接受居住中国大陆、没有企业主体的自然人申请该**具体代码签名产品及云服务**，而不只是可以注册账号或填写 CN？
2. 接受中国居民身份证还是需要护照？个人地址证明需要什么，中文材料是否需翻译/公证，姓名最终如何显示？
3. 基础证书、云签名、令牌、认证、运费、税费分别多少？签名次数如何计算？验证失败退款规则是什么？
4. 是否正式支持 GitHub Actions 托管 Windows runner 无人值守，是否每次/每段会话需手机批准，是否有独立自动化费？
5. 如果选 Certum Open Source，LiteView 的实际发行方式和未来收费计划是否符合其条款？如果选 SignPath，能否接受基金会发布者名称与人工审批？

**建议顺序：** 商店账号验证/MSIX 准备与证书调查并行；GitHub 签名若优先自动化，先确认 SSL.com IV + eSigner 的中国个人资格和完整报价；若优先低成本，先确认 Certum 开源云版的可用性和条款。获得明确产品级答复、用户接受实名展示和费用后，再配置签名流水线。当前不应盲买 EV、假填境外身份或上传任何证件到仓库。

## 官方来源

均于 2026-10-17 阅读，除当前仓库观察和明确标注的实施建议外，规则依据如下：

1. [MSIX app package requirements — Code signing for Microsoft Store submissions](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)
2. [MSI/EXE app package requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements)
3. [Authenticode Digital Signatures](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/authenticode)
4. [SmartScreen reputation for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
5. [App capability declarations — runFullTrust / restricted capabilities](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations)
6. [Integrate your desktop app with Windows using packaging extensions](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/desktop-to-uwp-extensions)
7. [uap:FileTypeAssociation schema](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap-filetypeassociation)
8. [Microsoft Store Policies — 10.1 / 10.5 Privacy](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies)
9. [Code signing options for Windows app developers](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)
10. [Microsoft Trusted Root Program participants](https://learn.microsoft.com/en-us/security/trusted-root/participants-list)
11. [SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)
12. [Quickstart: Set up Artifact Signing — Prerequisites / identity validation](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart)
13. [Artifact Signing pricing](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/)
14. [Steps to open a Microsoft Store developer account in Partner Center](https://learn.microsoft.com/en-us/windows/apps/publish/partner-center/open-a-developer-account)
15. [SSL.com IV Code Signing — 产品、身份与价格](https://www.ssl.com/products/software-integrity/code-signing/iv/)
16. [SSL.com Accepted Country Code Listing — China/CN](https://www.ssl.com/country-codes/)
17. [SSL.com Identity Validation: A Complete Guide](https://www.ssl.com/guide/identity-validation-for-ssl-com-certificates-a-complete-guide/)
18. [SSL.com eSigner for Code — 订阅与 CI/CD](https://www.ssl.com/esigner/)
19. [SSL.com Cloud Code Signing Integration with GitHub Actions](https://www.ssl.com/how-to/cloud-code-signing-integration-with-github-actions/)
20. [Certum Code Signing — Required documents](https://support.certum.eu/en/code-signing-required-documents/)
21. [Certum Open Source Code Signing in the Cloud](https://shop.certum.eu/open-source-code-signing-on-simplysign.html)
22. [Certum Standard Code Signing in the Cloud](https://shop.certum.eu/standard-code-signing-in-the-cloud.html)
23. [Certum 官方代码签名商品目录 — 激活码/套装/云版分别定价](https://shop.certum.eu/code-signing.html)
24. [Certum SignTool/Jarsigner 官方指南入口](https://support.certum.eu/en/signing-the-code-using-tools-like-signtool-and-jarsigner-instruction/)
25. [SignPath Foundation Terms — 资格、发布者和人工审批](https://signpath.org/terms)
