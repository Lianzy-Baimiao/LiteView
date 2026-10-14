# LiteView 轻量看图

Rust 编写的轻量图片查看器：同文件夹翻图、鼠标滚轮缩放、GIF 自动播放与逐帧查看；
自带画图级编辑，以及马赛克 / 模糊画笔和框选打码。现代扁平界面，中英文，跟随系统深浅色。

> 开发预览版，不是 v1.0 正式发布。需求见 [PRD](docs/PRD.md)，进展及真实未完成项见 [TODO](docs/TODO.md)。

## 功能

- GIF / APNG / 动态 WebP 自动播放、调速、逐帧、导出当前帧；超出缓存时后台按需解码，不再只循环前几帧。
- 常见位图、SVG、JPEG XL、RAW 内嵌预览、PSD 合并图；HEIC / AVIF 等在 Windows 上依赖系统解码扩展。并非任意格式都能无条件打开。
- 裁剪、缩放、旋转、形状、文字、画笔、撤销重做；动图仅查看，不进入编辑。
- 打码保存重新编码，并去掉 EXIF / 缩略图等可能泄露原内容的数据；覆盖前确认、原子写入。
- 运行时离线，无广告、无后台常驻服务；MIT 开源。

## 下载与平台

[GitHub Releases](https://github.com/Lianzy-Baimiao/LiteView/releases) · [安装说明](docs/INSTALL.md)

Windows 10 / 11 优先。本仓库包含 Windows 绿色包与用户级安装包、Linux x64 归档、macOS Intel / Apple Silicon 应用包的构建流程。
某个平台的包只有对应 CI 成功后才可用；添加打包脚本不等于已经在该系统实测。
发布包附第三方许可和 SHA-256 校验值。当前未做代码签名或 macOS 公证。

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
