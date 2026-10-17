# 已安装商店包的补充验证

来源：[运行 39037154456](https://github.com/Lianzy-Baimiao/LiteView/actions/runs/39037154456)，提交 `73f02ed`，2026-10-17。一次性 Windows Server 2025 build 26100，活动用户会话 2；全工作流 1 分 44 秒。这里不是 Windows 10/11 人工验收或商店认证。

## 结果

| 检查 | 结果 |
| --- | --- |
| 原送审 MSIX 与内部 EXE 哈希、身份 | 匹配；未重建、未改正式包 |
| WACK 10.0.26100.8249 | 总体 PASS，13 必需项全过；10 可选项通过，1 可选项失败 |
| 临时签名副本安装 | 成功 |
| 真实 AUMID 诊断激活 | 正确包身份、版本 1.0.0 和 WinRT LocalFolder |
| 已安装包身份：看图 | 通过，截图非空白，退出 0 |
| 已安装包身份：文字与箭头 | 通过，实际保存后图片像素变化，退出 0 |
| 已安装包身份：矩形马赛克 | 通过，演示图实际保存，退出 0 |
| 已安装包身份：GIF 逐帧 | 通过，截图为第 4 / 48 帧，退出 0 |
| 包与临时证书清理 | 无错误；没有上传签名副本、证书或私钥 |

`gui/gui-result.json` 保存四个场景的截图/源文件哈希、退出码与各自运行环境。截图为实际客户区 1024 × 768，已逐张查看；前三张与前次回放字节一致，GIF 的悬停提示略有变化，内容仍为第 4 帧。它们是测试截图，不替换已经上传到商店的 1500 × 975 宣传截图。

## 启动与配置的边界

先通过 `IApplicationActivationManager` 真正激活 `ice.LiteView_rz8m2sa64m4em!LiteView`，证据为 `gui/real-aumid-diagnostics.json`。

GUI 通过 `Invoke-CommandInDesktopPackage -PreventBreakaway` 启动 Python bootstrap，再运行安装目录中的原始 EXE。bootstrap 与每个场景都验证 `packaged=true`、正确 AUMID 及配置目录；此方法的 token 不等同于真实 AppId 激活，因此不把这四次 GUI 回放描述成四次真实 AUMID 界面启动。没有启用 Developer Mode 或全局调试。

前次 [运行 39036801584](https://github.com/Lianzy-Baimiao/LiteView/actions/runs/39036801584) 已通过四场景回放，但使用了显式的 `LITEVIEW_CONFIG_DIR` 测试覆盖。复核发现该覆盖的优先级高于 MSIX 默认路径，因此本次移除覆盖并逐场景重新校验：所有场景使用实际 `...\Packages\ice.LiteView_rz8m2sa64m4em\LocalState`。仅修改测试脚本，没有改发布程序来迁就测试。

## WACK 与证据

本轮 WACK 退出 0、`PARTIAL_RUN="FALSE"`，原始 XML SHA-256 为 `92874af50c4f2786a9ecf2c2d2003f0931d9cb3bb7f201e58608c0c473e3889d`。可选 Blocked executables 的六条消息与首次一致，详见 `wack-summary.json` 以及 `../wack-1.0.0-20261017/README.md`，仍保留 S mode 兼容性审查事项。

`SHA256.json` 记录归档原始数据的逐文件哈希（不包含本 README 与哈希清单自身）；Git 属性保留字节，不转换行尾。原送审 MSIX 为 `2130cb363d94fce37d06f0cef4222dc84795c408a4bda6d0c8a91103eb925b06`；本轮临时签名副本为 `a308eaee625dfb3f7ce97d9f4e630f3c7a714023bdbc190da0364e180b921030`。所有输入图片都是新生成的虚构演示文件，不涉及用户图片。

剩余：Windows 10/11 标准用户实机、系统文件激活/默认应用设置、S mode 外部程序兼容性和微软商店认证；本轮没有撤回审核、修改后台资料或点击发布。
