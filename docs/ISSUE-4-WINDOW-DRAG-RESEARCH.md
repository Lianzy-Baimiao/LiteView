# Issue #4：窗口内拖图片移动窗口的可行性

评估日期：2026-11-02。评估基线：`68323a485031526a3334e2ac7c4bf4a23871c353`（1.7.1）；下列固定链接保留评估时的代码。用户随后要求执行修改，实施与验证结果见末节。

[Issue #4「窗口移动的建议」](https://github.com/Lianzy-Baimiao/LiteView/issues/4) 于 2026-11-01 提出：“在窗口模式下，图片的显示面积程度等于或小于100%时，拖动图片即可移动整个窗口，而不必找标题栏拖动。”评估时没有后续评论。

## 判断

**技术可行，建议采纳拖图片移动普通窗口的方向，但将触发条件明确为“图片完整放得下、无需平移”。** 现有无边框模式已经实现这一交互，普通窗口可以复用原生拖窗通路；主要工作是调整拖动分流、补齐窗口状态判断与验证。

“不超过 100%”有歧义。如果指缩放栏数值，不能用作是否拖窗的依据：代码中的 `zoom = 1.0` 表示一个图片像素对应一个屏幕物理像素，不代表图片填满窗口。以 1200×800 物理像素画布为例，6000×4000 图片在 50% 时仍是 3000×2000，需要平移；300×200 图片在 200% 时只有 600×400，反而完整可见。如果作者本意是“整张图片都显示出来”，则与推荐条件一致。来源：[缩放定义及显示尺寸计算](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/viewer/view.rs#L1)，同文件 `shown_size` / `image_rect`（第 94–103 行）。

## 已核实的实现依据

| 已有能力 | 代码依据及含义 |
| --- | --- |
| 左键拖动开始时决定移动窗口还是平移图片 | [canvas.rs:205–225](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/app/canvas.rs#L205)：已有 `drag_started_by(Primary)` 分流和 `window_op` 防止同一次操作同时平移、点击。 |
| 是否需要平移的几何判断 | [frameless.rs:226–232](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/app/frameless.rs#L226)：旋转后的图片尺寸乘当前缩放、除 `pixels_per_point` 后与画布比较，并留 0.5 点容差。调用方已传 `shown_size`；无需另按百分比或 `view.fitted` 推断。 |
| 当前普通窗口不触发的原因 | [frameless.rs:70–73](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/app/frameless.rs#L70)：策略显式要求 `borderless && (shift || !pannable)`。限制在应用策略层。 |
| 系统拖窗入口已经存在 | [frameless.rs:109–122](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/app/frameless.rs#L109)：`start_window_drag` 发出 `StartDrag`；非最大化时补偿跨过 egui 拖动阈值的位移，最大化时跳过补偿。 |
| Windows 主窗口会执行该命令 | [host.rs:344–346](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/app/host.rs#L344)：命令统一交给 `egui_winit::process_viewport_commands`，不以有无标题栏分支。 |
| 底层不要求无边框 | 锁定的 egui-winit 0.36.2 `src/lib.rs:1750–1757` 在窗口有焦点时调用 `window.drag_window()`；winit 0.30.13 Windows `window.rs:491–528` 释放鼠标捕获后以 `WM_NCLBUTTONDOWN / HTCAPTION` 启动系统拖动，没有 decorations 限制。见 [egui-winit 源码](https://docs.rs/egui-winit/0.36.2/src/egui_winit/lib.rs.html#1750)及 [winit 源码](https://docs.rs/winit/0.30.13/src/winit/platform_impl/windows/window.rs.html#491)。 |
| 跨 DPI 的现有保护可继续使用 | [host.rs:213–216](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/app/host.rs#L213) 创建主窗口时安装 DPI 修正；[windows.rs:178–187](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/platform/windows.rs#L178) 在 Win11 使用系统建议矩形。该保护不要求无边框；Win10 不启用这层修正。 |

源码依据来自仓库及本机 Cargo 缓存中的上述锁定版本，文中的 docs.rs 链接用于定位相同版本。原生拖窗 API 要求与左键按下紧密衔接；沿用当前“刚识别为拖动”的一次性触发，不应每帧重新启动。见 [winit `drag_window` 文档](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.drag_window)。

## 建议的交互与范围

- 普通查看窗口、图片完整放得下：左键拖动移动窗口。图片任一方向超出画布：左键拖动继续平移图片。可将已有的 Shift + 拖动强制移动窗口语义扩展到普通窗口，但不是本建议成立的必要条件。
- 普通窗口首版建议仅在非最大化、非全屏状态启用。最大化后能否从画布拖出还原，是产品选择；现有底层入口为最大化情况预留了处理，不能把“不支持”当作技术结论。原有无边框行为可保持。
- 不可只删掉 `frameless` 条件：当前拖动策略本身没有显式排除全屏，扩展时要明确状态保护。编辑画布走独立分支（[mod.rs:736–742](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/src/app/mod.rs#L736)），应保留编辑、打码的绘制和选区交互。
- 同一次拖动在开始时确定用途，跨屏、改变缩放或窗口状态后也不要在拖动中切换。双击适应/100% 或全屏仍由现有点击逻辑处理。

改动预计集中在 `app/frameless.rs` 的拖动策略及状态保护、相关交互测试、`i18n.rs` 的快捷键说明。`canvas.rs` 已有调用点，按需要调整。没有发现新增依赖、图像解码路径或渲染器改造的必要；实际性能开销未测量。

## 待实现后的验证

需要覆盖：普通窗口整图/大图/长图/旋转图；小图放大超过 100% 和大图缩小不到 100%；双击与工具栏、菜单、对话框不误触发；拖动松开后能继续操作；最大化、全屏与编辑状态；Windows 贴边分屏及跨不同 DPI、负坐标副屏拖动。单元测试可验证分流条件，系统拖窗的焦点、捕获与还原行为仍需真鼠标验收。

仓库的[无边框方案与历史实测](https://github.com/Lianzy-Baimiao/LiteView/blob/68323a485031526a3334e2ac7c4bf4a23871c353/docs/BORDERLESS-PLAN.md)已经记载拖窗通路的使用经验；它支持复用判断，但不能替代此次普通窗口扩展的实测。

## 实施与验证（2026-11-02，未发布）

- 已扩展 `app/frameless.rs` 的拖动判断：Windows 普通查看窗口整图可见时拖窗，超出画布时平移，Shift 可强制拖窗。普通窗口最大化、全屏、编辑、菜单或对话框打开时不从画布启动拖窗；无边框最大化时仍保留拖出还原。普通空窗口不启用。
- 复用 `StartDrag` 和已有位移补偿；启动拖窗时清除残余图片惯性。同一次拖动不因 Shift 状态变化而改变用途。画布可拖窗时显示抓手指针。
- 设置的快捷键表加入中英文操作说明；窗口状态限制放在悬停提示中，避免长句在表格内被截断。更新日志已记入“未发布”。
- `cargo test --locked --release --lib`：205 通过、0 失败、1 忽略（原有 PDF 子进程辅助测试）。新增五项测试用真实 egui 输入事件检查窗口命令和图片偏移，覆盖大小图、100% 边界、DPI、旋转、Shift、拖动中途切换修饰键、最大化/全屏、菜单/设置、空画布和双击。最后的说明文案与悬停提示调整不改变拖动逻辑。
- `cargo clippy --locked --release --all-targets -- -D warnings`：通过。
- `cargo build --locked --release`：最终构建通过，试用文件为 `dist/issue-4-20261102/LiteView.exe`。最终构建的中英文新增快捷键行均已在实际窗口检查，操作说明完整显示。
- Windows 桌面、125% DPI 实测：普通窗口首次拖动位移为 (+200, +100) 物理像素，松手再次拖动为 (-100, -50)，窗口尺寸均不变；小图在 201.14% 时仍可拖窗；大图在 100% 和 29.91% 时走平移分支，窗口不移动。最大化和全屏拖动后窗口位置/尺寸不变；编辑模式拖出选区，窗口不移动；设置打开时背景拖动不移动窗口；无边框拖动仍移动 (+100, +50)。
- 本地输入素材、窗口位置记录和运行日志位于 `dist/issue-4-20261102/`（忽略目录），主要记录为 `native-drag-observations.json`、`native-drag.log`。SVG 在当前解码器中先栅格化到 2048 像素边长，日志缩放按该显示尺寸计算；小图验证使用实际 300×200 PNG。

本次没有重做跨不同 DPI/负坐标副屏、系统贴边分屏或真实鼠标按住 Shift 的验收；Shift 与 DPI 的分流已由 egui 输入测试覆盖，跨屏仍复用现有系统拖动通路。
