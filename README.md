# SnapRust

SnapRust 是一个用 Rust + Tauri 2 构建的 Windows 截图工具。当前已完成 V0.4 的截图、标注与钉图闭环：

```text
Ctrl + Shift + A → 框选 → 标注 → 复制图片 / 钉到桌面
```

详细设计、里程碑、验收标准和风险记录见 [docs/DEVELOPMENT_PLAN.md](docs/DEVELOPMENT_PLAN.md)。

## 当前进度

- 已完成 Tauri 2 + Vanilla TypeScript 工程骨架
- 已在 Rust 中注册全局快捷键 `Ctrl + Shift + A`
- 已实现无边框、全屏、置顶、隐藏启动的截图遮罩窗口
- 已实现 `Esc`/右键取消
- 已通过 windows-rs + Win32 GDI 抓取主显示器
- 已保证先抓屏、再显示遮罩，截图保存在 Rust 会话状态中
- 截图 PNG 会在隐藏窗口中完成加载与解码，再以透明首帧预热合成器后显示，避免快捷键触发时短暂黑屏
- 已把截图编码为 PNG 并显示在前端遮罩中
- 已实现任意方向鼠标框选、四周遮罩与实时尺寸显示
- 已在 Rust 中完成逻辑坐标 → 物理像素换算、边界夹取和图片裁剪
- 已实现 `Ctrl+C` / `Enter` 将选区作为 Windows 图片复制到剪贴板
- 已实现复制成功后自动退出截图模式
- 已改为抓取整个 Windows 虚拟桌面，支持副屏在左侧/上方时的负坐标布局
- 已枚举每台显示器的物理边界、主屏标记和有效 DPI 缩放比例
- 已在遮罩中显示虚拟桌面尺寸、显示器边界提示及实时逻辑/物理鼠标坐标
- 已进入 V0.3：支持箭头、矩形、椭圆、画笔、马赛克与文字标注
- 标注预览由 Canvas 完成，最终图像始终由 Rust 重新合成后写入剪贴板
- 已完成 V0.4 钉图：Rust 创建独立无边框置顶窗口并持有最终 PNG
- 钉图支持拖动、滚轮缩放、`Shift + 滚轮`/`[`/`]` 调透明度、`0` 复位、双击/`Esc` 关闭
- 整屏、选区与钉图 PNG 均通过二进制 IPC 传输，不再把大图转换为 Base64 字符串
- 标注拖动使用已提交画面缓存与逐帧合并重绘，并在退出时主动释放 Blob URL 和 Canvas 像素内存
- 进入标注后立即释放不再使用的虚拟桌面原图，降低 4K 与多显示器场景的内存峰值
- 混合 DPI 选区使用虚拟桌面绝对物理像素协议，支持负坐标与不同显示缩放比例之间的跨屏映射
- 马赛克预览与 Rust 输出使用相同的块平均算法；长画笔在前后端执行路径简化
- 文字标注使用截图内联多行编辑器，支持 `Ctrl+Enter` 确认、`Esc` 取消，不再弹出浏览器 prompt
- 钉图滚轮缩放使用鼠标锚点即时预览，并在停止滚动后提交原生窗口尺寸与屏幕边界修正
- 钉图窗口隐藏且以原生 0% 透明度创建，PNG 解码和 WebView2 合成器预热完成后再连同阴影一次性显示，避免创建时闪烁
- 截图 HUD、标注底栏和开发控制台提供抓屏、PNG/IPC、解码、裁剪、合成、剪贴板与钉图创建耗时

## 开发环境

- Windows 10/11
- Microsoft C++ Build Tools
- Microsoft Edge WebView2
- Rust stable MSVC 工具链
- Node.js + npm

## 启动

```powershell
npm install
npm run tauri dev
```

程序启动后窗口默认隐藏。在任意应用中按 `Ctrl + Shift + A` 进入截图模式：

1. 拖动鼠标框选区域；选区外会变暗，边框旁会显示尺寸。
2. 松开鼠标后进入标注编辑器；可选择箭头、矩形、椭圆、画笔、马赛克或文字，并调整颜色和粗细。文字工具点击截图后会在原位显示多行输入框，使用 `Ctrl+Enter`/“确认”提交，`Esc`/“取消”放弃。
3. 使用 `Ctrl + Z` 撤销、`Ctrl + Y` 重做，或点击“清空”移除标注。
4. 按 `Ctrl + C` 或 `Enter` 复制图片，或点击“📌 钉图”创建独立置顶窗口。两种输出都由 Rust 对原选区执行最终标注栅格化。
5. 钉图中拖动可移动，滚轮以鼠标位置为中心缩放，`Shift + 滚轮` 或 `[`/`]` 调透明度，`0` 复位，双击或 `Esc` 关闭。缩放会先即时预览，停止滚动约 90ms 后再提交原生窗口尺寸，并防止窗口完全移出显示器工作区。
6. 输出成功后遮罩自动关闭；截图阶段可用 `Esc` 或鼠标右键取消。

截图会先覆盖整个 Windows 虚拟桌面，并在原生窗口保持隐藏时加载、解码截图；WebView 合成器以透明内容完成预热后，截图和遮罩才一次性出现。因此不会把遮罩本身截入图片，也不会在等待 PNG 时显示黑色占位帧。前端始终把逻辑视口坐标按截图的实际物理像素尺寸换算；例如 125% 缩放时，逻辑 `(400, 300)` 对应物理 `(500, 375)`。多显示器下，虚拟桌面的物理原点可以为负数，屏幕轮廓会以虚线显示在遮罩中。

截图 HUD 与标注底栏会显示当前链路的关键性能数据。开发模式下，浏览器控制台还会输出以 `[SnapRust performance]` 开头的完整分段记录；PNG 编码通过 Tauri 内置异步运行时的后台阻塞任务执行，不会占住窗口命令线程，项目仍不直接依赖 Tokio。

创建钉图时，动态窗口会先隐藏并保持原生透明度为 0；前端完成钉图 PNG 的二进制 IPC、Blob 解码和两帧合成器预热后，Rust 才启用窗口阴影、设置焦点并将透明度恢复到 100%。加载失败的隐藏钉图窗口会自动关闭，`PinStore` 中的图片数据也会随窗口销毁清理。

当前开发机已实际验收单显示器 125% DPI 场景。多显示器的 Windows API 枚举、虚拟桌面抓取、负坐标窗口定位和前端布局均已实现；仍建议在接入第二台实际显示器后完成一次跨屏人工回归。

## 质量检查

```powershell
npm run check
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

真实屏幕抓取测试默认忽略，因为它需要交互式 Windows 桌面。显式运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml screenshot::tests::captures_virtual_desktop_with_expected_dimensions -- --ignored --exact
```
