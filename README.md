# LiteView 轻量看图

Rust 编写的轻量图片查看器：同文件夹翻图、鼠标滚轮缩放、GIF 自动播放与逐帧查看；
自带画图级编辑，以及马赛克 / 模糊画笔和框选打码。现代扁平界面，中英文，跟随系统深浅色。

> v1.0.0 正式版。Windows 看图、编辑和打码是本版的主要使用范围；系统扩展、平台原生验收与后续优化的边界见 [TODO](docs/TODO.md)。

### v1.0.0 更新

- 转为正式版本，保留原有设置和独立安装版/绿色版的使用方式。
- 新增 Microsoft Store MSIX 打包：使用正式产品身份，提供多倍率图标、中英文系统菜单和清单管理的图片关联。商店版已提交审核，尚未上架。
- 商店版使用独立配置目录和单实例通道；不会修改或移除独立版的格式注册。

- 文件关联页增加常用/全部格式选择、完成进度和待设置筛选；从系统设置返回后自动刷新。
- 登记格式改为增量操作，不会因取消勾选而删除已有注册。Windows 默认应用仍由用户在系统设置中确认。
- 改善 Windows 小字号中文在 125% 缩放下偏厚、边缘发硬的问题；英文与数字优先使用 Segoe UI，字号不变。
- 修复 Windows GNU 构建漏嵌 EXE 图标、版本信息和 DPI 清单的问题；新增成品资源检查。

预算不是整个进程的内存硬上限；系统解码器内部、SVG 解析/滤镜、GPU 与并行预读仍有独立开销，详见 PRD 的 OPT-01。

## 界面预览

以下截图来自已发布的 Windows 1.0.0 正式版（2026-10-17），不是界面设计稿。
图片为原创演示素材，资料卡为虚构数据；文字、箭头和马赛克均由 LiteView 实际操作生成。[截图来源与复现方法](docs/images/store-v1.0.0/README.md)

### 看图 · 深色主题

内容优先，底部浮动工具栏提供翻图、缩放、旋转与编辑入口。

![LiteView 深色看图界面：原创山湖插画与底部浮动工具栏](docs/images/store-v1.0.0/zh-cn-viewer.png)

### 编辑 · 文字与箭头标注

在同一窗口进入编辑，添加中文文字、绘制箭头，并调整形状控制点。

![LiteView 编辑界面：在图片上添加中文文字和可调整的箭头](docs/images/store-v1.0.0/zh-cn-editor.png)

### 打码 · 框选马赛克

框选需要遮挡的区域，可切换画笔或框选、马赛克或模糊，并调整强度。

![LiteView 打码界面：用框选马赛克遮挡虚构联系电话和邮箱](docs/images/store-v1.0.0/zh-cn-redaction.png)

### 动图 · 暂停与逐帧查看

暂停 GIF，前后逐帧查看，也可以单独导出当前帧。

![LiteView 动图界面：暂停在 GIF 第 4 帧，显示逐帧按钮、时间轴和速度](docs/images/store-v1.0.0/zh-cn-animation.png)

## 功能

- GIF / APNG / 动态 WebP 自动播放、调速、逐帧、导出当前帧；超出缓存时后台按需解码，不再只循环前几帧。
- 常见位图、SVG、JPEG XL、RAW 内嵌预览、PSD 合并图；HEIC / AVIF 等在 Windows 上依赖系统解码扩展。并非任意格式都能无条件打开。
- 裁剪、缩放、旋转、形状、文字、画笔、撤销重做；动图仅查看，不进入编辑。
- 打码保存重新编码，并去掉 EXIF / 缩略图等可能泄露原内容的数据；覆盖前确认、原子写入。
- 运行时离线，无广告、无后台常驻服务；MIT 开源。

## 下载与平台

[v1.0.0 正式版](https://github.com/Lianzy-Baimiao/LiteView/releases/tag/v1.0.0) · [全部版本](https://github.com/Lianzy-Baimiao/LiteView/releases) · [安装说明](docs/INSTALL.md)

Windows 10 / 11 优先。本仓库包含 Windows 绿色包与用户级安装包、Linux x64 归档、macOS Intel / Apple Silicon 应用包的构建流程。
某个平台的包只有对应 CI 成功后才可用；添加打包脚本不等于已经在该系统实测。
发布包附第三方许可和 SHA-256 校验值。当前未做代码签名或 macOS 公证。
Microsoft Store 1.0.0 已于 2026-10-17 提交审核，尚未通过认证或上架；审核通过后仍需手动发布。未签名的提交候选包不会作为普通 GitHub 安装包提供。[隐私政策](docs/PRIVACY.md)

## 快捷键

| 操作 | 快捷键 |
|---|---|
| 打开 / 粘贴 | Ctrl+O / Ctrl+V |
| 上一张 / 下一张 | ← / → |
| 适应窗口 / 原始大小 | Ctrl+0 / Ctrl+1 |
| 编辑 / 打码 | E / M |
| 复制图片 / 保存 | Ctrl+C / Ctrl+S |
| 动图播放暂停 / 前后帧 | Space / , / . |
| 图片信息 / 关闭窗口 | I / Esc |

更多快捷键见软件设置。

## 从源码构建

Rust 1.95+；建议使用当前 stable 工具链。

- Windows 推荐 MSVC 工具链和 Visual Studio C++ Build Tools。
GNU/MinGW 的本机路径配置见 [.cargo/config.example.toml](.cargo/config.example.toml)。

- Ubuntu 构建依赖：

```sh
sudo apt-get install libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

```sh
cargo test --locked --release --lib
cargo clippy --locked --release --all-targets -- -D warnings
cargo build --locked --release
```

发布与本地打包流程见 [RELEASING](docs/RELEASING.md)。

## English

LiteView is an offline Rust image viewer with folder navigation, wheel zoom, animated-image playback
and a small Paint-style editor with mosaic/blur redaction. Choose English in Settings → General.
Windows is the primary tested platform; Linux and macOS packaging is included but still needs native validation.
See [installation](docs/INSTALL.md) and [remaining work](docs/TODO.md).

## License

[MIT](LICENSE). Third-party dependencies and icons retain their own licenses;
see [third-party notices](THIRD-PARTY-NOTICES.md) and the generated LICENSES.html in release packages.
