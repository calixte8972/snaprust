# SnapRust 开发计划

> 当前状态：V0.4 已完成。Windows 上已打通 `Ctrl + Shift + A` → 虚拟桌面截图 → 框选 → 标注 → 复制图片 / 创建独立置顶钉图窗口。

## 1. 技术栈与范围

### V0.1 固定技术栈

- 后端：Rust、Tauri 2、`windows`（windows-rs）、`image`、`serde`
- 前端：Vanilla TypeScript、HTML、CSS、Vite
- 目标平台：Windows 10/11，MSVC 工具链，WebView2
- 暂不引入：Tokio、前端框架、SQLite、OCR、云服务

依赖遵循“用到时引入”的原则。Tauri 与全局快捷键属于第一个切片；`windows`、`image` 和 `serde` 在截图数据结构与 GDI 截屏切片中加入，避免只有名字、没有用途的依赖。

### V0.1 功能边界

必须完成：

1. `Ctrl + Shift + A` 在应用失去焦点时也能进入截图模式。
2. Rust 在遮罩显示前完成屏幕抓取，避免把 SnapRust 自己截进去。
3. 全屏遮罩显示抓取结果，支持鼠标拖拽选择。
4. 松开鼠标后生成所选区域图片。
5. `Ctrl + C` 将图片写入 Windows 剪贴板；第一版也可在松开后自动复制，但界面必须明确提示结果。
6. `Esc` 随时取消；零面积框选不触发裁剪。
7. 错误可见且能恢复到空闲状态，不能留下无法关闭的全屏窗口。

V0.1 不做：多显示器完整体验、跨屏框选、标注、保存历史、OCR、安装包自动更新。

## 2. 核心设计

### 2.1 状态机

```text
Idle
  │ Ctrl+Shift+A
  ▼
CapturingScreen ──失败──► Idle + 错误提示
  │ 截图成功后才显示遮罩
  ▼
Selecting
  ├─ Esc ──────────────► Cancelled ─► Idle
  └─ 鼠标松开
       ▼
Selected
  ├─ Ctrl+C/自动复制成功 ► Copied ──► Idle
  ├─ Esc ──────────────► Cancelled ─► Idle
  └─ 复制失败──────────► Selected + 错误提示
```

Rust 是会话状态的权威来源；TypeScript 只维护当前指针交互和选择框视觉状态。V0.1 同一时间只允许一个截图会话。

### 2.2 截图顺序

正确顺序是：

1. 热键触发。
2. 若遮罩已显示，忽略重复触发。
3. Rust 抓取目标显示器并保存会话图像。
4. Rust 把图像元数据传给前端。
5. 最后显示、置顶并聚焦遮罩窗口。

不能先显示半透明遮罩再截图，否则截图内容会包含遮罩自身。

### 2.3 坐标约定

前端传递矩形，而不是直接传两个未经规范化的点：

```ts
type SelectionRect = {
  x: number;
  y: number;
  width: number;
  height: number;
  viewportWidth: number;
  viewportHeight: number;
};
```

Rust 根据截图物理像素尺寸计算比例：

```text
scale_x = image_width  / viewport_width
scale_y = image_height / viewport_height
```

裁剪前统一进行规范化、四舍五入和边界夹取。V0.2 将显示器物理原点、负坐标和独立 DPI 纳入模型：Rust 以 `SM_X/Y/CX/CYVIRTUALSCREEN` 获取整个虚拟桌面，用 `EnumDisplayMonitors`/`GetMonitorInfoW` 枚举显示器，并把有效 DPI 与物理边界随截图元数据交给 UI。遮罩按虚拟桌面的物理位置与尺寸定位；前端用“实际截图像素 ÷ 当前 WebView 视口”进行运行时校准，向 Rust 提交虚拟桌面绝对物理矩形。Rust 不再依据某一台显示器的逻辑缩放重新推算选区，因此 100%/125%/150% 混合缩放和负坐标跨屏共用同一物理像素协议。

### 2.4 模块边界

```text
src-tauri/src/
├── main.rs                 # Tauri 生命周期、插件和命令装配
├── commands.rs             # 前端可调用的薄命令层
├── hotkey/mod.rs           # 注册 Ctrl+Shift+A，触发截图会话
├── window/mod.rs           # 遮罩窗口显示、隐藏、聚焦和恢复
├── screenshot/
│   ├── mod.rs              # 截图领域类型与公共接口
│   ├── monitor.rs          # 显示器枚举、边界、DPI 信息
│   └── capture.rs          # Windows 抓屏、像素转换与裁剪
└── clipboard/mod.rs        # Windows DIB/剪贴板所有权与写入

src/
├── main.ts                 # 页面启动、事件和快捷键绑定
├── screenshot.ts           # Tauri command/event 的类型化封装
├── selection.ts            # 纯前端选择框状态与坐标规范化
└── style.css               # 截图背景、遮罩、选区和提示
```

`commands.rs` 不承载业务算法，只做反序列化、调用 Rust 领域模块、把错误转换成可展示文本。

### 2.5 剪贴板策略

V0.1 使用 windows-rs 调用 Win32 剪贴板 API，写入兼容性好的 DIB 数据。所有 `OpenClipboard`、全局内存分配与所有权转移都封装在 `clipboard` 模块；成功交给系统的内存不得由应用再次释放，任何失败路径都必须关闭剪贴板并释放仍由应用拥有的内存。

## 3. 实施里程碑

### M0：工程骨架与全局快捷键

交付：

- Tauri 2 + Vanilla TypeScript 工程可构建。
- 创建隐藏的 `overlay` 窗口：无边框、全屏、置顶、不显示在任务栏。
- Rust 注册 `Ctrl + Shift + A`。
- 按热键显示并聚焦遮罩；按 `Esc` 调用 Rust 隐藏窗口。
- 重复按热键不会创建重复窗口。

验收：

```powershell
npm run check
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri dev
```

人工验证：切换到其他应用，按 `Ctrl + Shift + A`，看到 SnapRust 遮罩；按 `Esc` 后原应用恢复可见。

### M1：单显示器抓屏

交付：

- `monitor` 返回目标显示器的物理边界和缩放信息。
- `capture` 通过 Win32 GDI 抓取屏幕，输出统一的 RGBA 图像。
- 截图保存在 Rust 会话状态中；前端只接收用于显示的编码结果或受控资源地址。
- 严格保证“截图在先、遮罩在后”。

验收：截图四角与实际屏幕一致，颜色通道正确，遮罩本身不出现在截图中。

### M2：框选交互

交付：

- Pointer Events 驱动鼠标按下、拖动、松开。
- 支持从任意方向拖拽。
- 选区外半透明，选区内显示原图；实时展示宽 × 高。
- 窗口失焦、`Esc`、右键均可安全取消。
- TypeScript 单元测试覆盖矩形规范化和边界夹取。

验收：向四个方向拖动结果一致；零面积选择不提交；快速点击/拖出窗口不会卡在 Selecting 状态。

### M3：Rust 裁剪与 Windows 剪贴板

交付：

- `crop_selection` 接受逻辑选区和 viewport 尺寸，在 Rust 中换算、校验并裁剪。
- `clipboard` 写入 Windows 剪贴板。
- `Ctrl + C` 和确认动作复制当前选区；成功后自动退出截图模式。
- 错误提示保留选区，允许重试或取消。

验收：粘贴到画图、微信/聊天应用和 Word 中尺寸正确、颜色正确；边缘选区不越界；连续截图不会锁死剪贴板。

### M4：V0.1 稳定化

交付：

- Rust 单元测试覆盖裁剪坐标、边界和错误输入。
- 日志与用户错误提示不泄露原始图像数据。
- 热键冲突时给出明确错误。
- `cargo fmt --check`、`cargo clippy -- -D warnings`、前端类型检查和生产构建全部通过。
- 在 Windows 100%、125%、150% 缩放上进行人工回归并记录已知限制。

验收：从干净环境按 README 可启动；连续完成 20 次截图/取消无残留窗口、无明显资源增长。

## 4. 后续版本

### V0.2 截图体验

- [x] 多显示器与负坐标虚拟桌面
- [x] 每显示器 DPI 信息、跨屏坐标映射与显示器边界提示
- [x] 显示鼠标逻辑/物理坐标、截图尺寸和当前显示器缩放比例
- [ ] 放大镜
- [ ] 键盘微调选区

本次 V0.2 的范围以用户定义的前四项为准：多显示器、DPI 缩放、鼠标坐标、截图尺寸。放大镜和键盘微调是原路线中可选的后续体验增强，不阻塞 V0.2 完成。

### V0.3 标注

- [x] 箭头、矩形、椭圆、文字、画笔、马赛克
- [x] 标注模型与最终栅格化仍由 Rust 管理；前端负责工具交互和预览
- [x] 颜色、线宽、撤销、重做与清空

#### V0.3 标注架构

选区生成后，前端以 Canvas 只做即时预览和鼠标交互；它向 Rust 传递的是受限的结构化命令，而不是最终 PNG：

```text
Canvas Pointer Events
       ↓
Annotation[]（arrow / rectangle / ellipse / brush / mosaic / text）
       ↓ set_capture_annotations
Rust 验证坐标、颜色、尺寸和复杂度
       ↓ Ctrl+C
Rust 对选区原图栅格化全部 Annotation[]
       ↓
CF_DIB Windows 剪贴板
```

`annotation.rs` 是最终渲染的唯一权威：箭头、几何线条、画笔和马赛克直接写入 `RgbaImage`；文字使用可用的 Windows UI 字体（优先微软雅黑）经 `fontdue` 栅格化。前端画布仅用于让用户在复制前看到交互结果，不能替代 Rust 的最终导出。

### V0.4 钉图

- [x] 独立无边框置顶窗口；每张钉图使用唯一 Tauri 窗口标签
- [x] Rust 保存最终合成后的 PNG，钉图前端不持有截图业务源数据
- [x] 拖动移动、滚轮缩放、`Shift + 滚轮`/键盘透明度、双击/`Esc` 关闭
- [x] 使用 `HWND`、`WS_EX_LAYERED` 与 `SetLayeredWindowAttributes` 调整原生窗口透明度
- [x] 窗口销毁时清理 Rust `PinStore`，创建失败时回滚已插入图片

#### V0.4 钉图架构

```text
Annotation[] + Selected RgbaImage
              ↓ Rust 最终栅格化
PinStore（label → PNG bytes / width / height）
              ↓ WebviewWindowBuilder + binary IPC
pin.html（仅显示与交互）
              ├─ startDragging
              ├─ setSize（滚轮缩放）
              ├─ SetLayeredWindowAttributes（透明度）
              └─ close_pin（删除状态并关闭）
```

动态 WebView 必须从异步 Tauri command 创建。同步 command 会阻塞 WebView 初始化，表现为只出现白色窗口壳且 IPC 不返回；本项目以 `async fn pin_selected_capture` 规避该 UI 调度死锁，不需要额外引入 Tokio 依赖。

### V0.5 OCR

- OCR 作为截图后的独立能力，不阻塞核心截图链路
- 出现后台任务后再评估 Tokio

### V0.6 历史记录

- SQLite 保存路径、尺寸、时间、OCR 文本、标签和收藏状态
- 图片文件与元数据的生命周期必须保持一致

## 5. 主要风险与处理原则

| 风险 | V0.1 处理 | 后续处理 |
| --- | --- | --- |
| Windows 125%/150% 缩放导致坐标错位 | 使用 viewport 与物理图片双尺寸换算并做专项回归 | V0.2 引入 per-monitor DPI 完整模型 |
| 遮罩被截入图片 | 强制先抓屏再显示窗口 | 保持为不可破坏的调用顺序 |
| 多屏存在负坐标 | V0.1 明确限定单目标显示器 | V0.2 使用虚拟桌面物理坐标 |
| 全局热键被其他软件占用 | 启动时报告注册失败，不静默失效 | 增加可配置快捷键 |
| Win32 资源泄漏 | 对 DC、bitmap、global memory 使用小型 RAII 封装 | 用压力测试和诊断工具验证 |
| 全屏窗口无法退出 | Rust 命令、前端 Esc、窗口失焦路径均回到 Idle | 增加托盘菜单紧急退出 |
| 前端持有完整业务状态 | Rust 管理截图会话与最终图片 | 后续 OCR/历史同样通过命令边界 |

## 6. 工程质量门槛

每个里程碑至少执行：

```powershell
npm run check
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

涉及窗口、热键、DPI、剪贴板的行为无法只靠单元测试证明正确，必须附带 Windows 人工验收记录。

## 7. 当前执行状态

- [x] 确认仓库为空并检查 Rust/Node 工具链
- [x] 决定使用 Tauri 2 Vanilla TypeScript
- [x] M0：创建工程骨架
- [x] M0：注册全局快捷键
- [x] M0：显示/取消截图遮罩
- [x] M0：完成类型检查、编译、Clippy 和生产构建
- [x] M0：人工按键验收 `Ctrl + Shift + A` 与 `Esc`
- [x] M1：通过 windows-rs + GDI 实现主显示器抓屏
- [x] M1：截图存入 Rust 会话，严格先抓屏后显示遮罩
- [x] M1：前端读取 PNG 数据并显示为截图背景
- [x] M1：真实桌面抓屏测试通过（主屏尺寸一致，约 0.13 秒）
- [x] M1：人工检查遮罩中的截图内容、色彩和缩放
- [x] M2：Pointer Events 框选、任意方向拖动、选区遮罩与实时逻辑尺寸
- [x] M2：Pointer Capture、`Esc`、右键和 Pointer 取消路径
- [x] M3：Rust 按 viewport → 物理像素比例换算、边界夹取并裁剪
- [x] M3：通过 windows-rs 写入 `CF_DIB` Windows 图片剪贴板
- [x] M3：`Ctrl+C`/`Enter` 复制成功后自动退出截图模式
- [x] M4：坐标、裁剪、像素转换与 DIB 转换单元测试
- [x] M4：格式化、Clippy、类型检查、生产前端构建
- [x] M4：当前桌面约 125% DPI 的热键、框选、复制、画图粘贴与 Esc 取消验收
- [ ] M4：补充验证第二个粘贴目标，以及 Windows 100%/150% 缩放
- [x] V0.2：使用 Windows 虚拟桌面物理边界抓取全部显示器
- [x] V0.2：使用 `EnumDisplayMonitors`、`GetMonitorInfoW` 和有效 DPI 枚举显示器
- [x] V0.2：遮罩按虚拟桌面物理原点/尺寸显示，支持负坐标副屏布局
- [x] V0.2：显示虚拟桌面截图尺寸、显示器数、显示器边界与实时逻辑/物理坐标
- [x] V0.2：当前单屏 125% DPI 实机验证热键、坐标映射与 Esc 取消
- [ ] V0.2：接入至少两台真实显示器，完成跨屏框选人工回归
- [x] V0.3：结构化标注协议、Rust 输入验证与最终 `RgbaImage` 合成
- [x] V0.3：箭头、矩形、椭圆、画笔、马赛克、文字工具及颜色/线宽设置
- [x] V0.3：Canvas 即时预览、撤销、重做、清空和复制前同步
- [x] V0.3：标注栅格化与边界输入的 Rust 单元测试
- [x] V0.3：真实桌面回归——六种工具预览、撤销/重做、Rust 合成与画图粘贴
- [x] V0.4：Rust `PinStore`、最终标注合成图片与动态置顶窗口
- [x] V0.4：独立 `pin.html` 多页面构建及最小 Tauri 窗口权限
- [x] V0.4：拖动、滚轮缩放、原生透明度、复位、双击/`Esc` 关闭
- [x] V0.4：真实 Windows 窗口创建、图片显示、缩放、透明度、拖动和销毁回归
- [x] 性能优化：整屏、选区和钉图统一使用 PNG 二进制 IPC，移除业务层 Base64
- [x] 性能优化：标注预览缓存已提交内容，PointerMove 每动画帧最多重绘一次
- [x] 稳定性优化：释放 Blob URL/Canvas 像素缓冲，取消过期异步载入并串行化复制/钉图
- [x] 混合 DPI：前端运行时校准并提交虚拟桌面绝对物理选区，Rust 直接按物理矩形裁剪
- [x] 马赛克一致性：前端和 Rust 使用相同块边界、RGBA 平均值与整数舍入规则
- [x] 长画笔优化：采样上限、距离过滤与前后端 Ramer–Douglas–Peucker 路径简化
- [x] 钉图体验：鼠标锚点即时缩放预览、90ms 延迟原生提交、最小操作尺寸与工作区边界保护
- [x] 文字体验：截图原位多行输入、确认/取消、键盘交互与 512 字符协议上限
- [x] 性能基准：Rust 分段计时、前端端到端计时、HUD/底栏/控制台展示与后台 PNG 编码
- [x] 截图显示时序：隐藏预加载并解码 PNG、透明首帧预热 WebView 合成器、就绪后再显示截图遮罩
- [x] 钉图显示时序：隐藏且原生全透明创建、无阴影预热 WebView、图片就绪后一次性恢复阴影和不透明度

### 2026-08-24 实施记录

- Rust：1.97.1（`x86_64-pc-windows-msvc`）
- Node：24.18.1；npm：11.16.0
- 解析后的关键依赖：Tauri 2.11.5、全局快捷键插件 2.3.2、image 0.25.10、Vite 7.3.6
- `npm run check`：通过
- `npm run build`：通过
- `cargo check`：通过
- `cargo clippy --all-targets -- -D warnings`：通过
- BGRA → RGBA 单元测试：通过
- GDI 真实主屏抓取测试：在沙箱外通过；沙箱内按预期被 Windows 拒绝屏幕读取
- Windows GUI 自动按键验收：应用可启动且无热键注册错误，但自动化授权超时，保留人工检查项
- 新增：6 个普通单元测试通过，1 个真实桌面抓屏测试默认忽略
- 新增：`Ctrl+C` 使用 `CF_DIB`；剪贴板被占用时以 15ms 间隔最多重试 6 次
- 新增：逻辑选区用 `floor(start)`/`ceil(end)` 映射物理像素，避免高 DPI 边缘漏像素
- 实机验收：`Ctrl+Shift+A` 成功显示 `1920 × 1200` 截图遮罩；自动化环境的逻辑画面为 `1536 × 960`，验证当前系统约为 125% DPI。
- 实机验收：拖出选区后 Rust 返回 `600 × 375`；按 `Ctrl+C` 后遮罩自动关闭，Windows 画图成功粘贴并识别为 `600 × 375` 图片对象。
- 实机验收：再次触发截图后按 `Esc`，SnapRust 遮罩窗口消失且画图保留在前台。
- V0.2 编译验证：`npm run check`、`npm run build`、`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 和 `cargo build` 全部通过；Rust 单元测试为 6 通过、1 个真实桌面测试忽略。
- V0.2 实机验收：`Ctrl+Shift+A` 显示虚拟桌面 `(0, 0) 1920 × 1200`，并报告 `1` 个显示器；自动化可见画面为 `1536 × 960`，符合 125% 缩放。
- V0.2 实机验收：移动/按下到逻辑 `(400, 300)` 时，界面读数为物理 `(500, 375)`、主显示器、`125%`，验证前端到截图物理像素的 DPI 换算。
- V0.2 实机验收：按 `Esc` 后 SnapRust 从可见窗口列表移除。当前设备只有一台显示器，跨实际双屏/负坐标回归留作明确的硬件验证项。
- V0.3 静态验收：新增 `fontdue`，`npm run check`、`npm run build`、`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 和 `cargo test` 全部通过；Rust 单元测试为 9 通过、1 个真实桌面抓屏测试忽略。
- V0.3 实机验收：箭头、矩形、椭圆、画笔、马赛克、中文文字均可在 Canvas 中创建；标注计数、`Ctrl+Z` 和 `Ctrl+Y` 正常。
- V0.3 实机验收：箭头/矩形以及中文文字分别经过 Rust 最终合成，SnapRust 自动退出，画图成功粘贴并保持选区尺寸、颜色和中文字符。
- V0.3 修复：Serde 带标签枚举原先没有转换变体内部的 `fontSize`/`blockSize`，导致文字或马赛克复制失败；现使用 `rename_all_fields = "camelCase"` 并增加协议回归测试。编辑器也会直接显示复制阶段的 Rust 错误。
- V0.3 优化：Windows 中文字体通过进程级 `OnceLock` 缓存，首次解析后后续文字标注无需重复加载字体文件。
- V0.4 静态验收：`npm run check`、多页面 `npm run build`、`cargo fmt --check`、Clippy 零警告与 Rust 测试全部通过；新增钉图状态存取/删除测试，总计 12 通过、1 个真实桌面测试忽略。
- V0.4 实机修复：动态 WebView 从同步 command 创建时卡在 `build()`，只产生白色窗口壳；将 `pin_selected_capture` 改为 Tauri 异步 command 后窗口正常加载且遮罩正确退出，没有新增 Tokio 依赖。
- V0.4 实机验收：`376 × 250` 选区创建独立无边框置顶钉图；滚轮后窗口由 `378 × 252`（含边框捕获尺寸）放大至 `415 × 277`，HUD 显示 `110%`。
- V0.4 实机验收：windows-rs 原生透明度降至 `95%` 成功；拖动后窗口原点从 `(733, 413)` 移至 `(796, 451)`；双击关闭后 SnapRust 可见窗口清零并触发 PinStore 清理。
- 性能优化静态验收：整屏与选区元数据不再携带 `imageDataUrl`，PNG 通过 `tauri::ipc::Response` 直接返回；删除 SnapRust 对 `base64` 的直接依赖，并增加二进制 PNG 协议回归测试。
- 标注优化：原图与已提交标注缓存在离屏 Canvas，拖动草稿通过 `requestAnimationFrame` 合帧；画笔过滤过密采样点，撤销/重做/清空仅在状态变化时重建缓存。
- 生命周期优化：每次退出或开始新截图都会撤销 Blob URL、清空大尺寸 Canvas 像素缓冲；会话版本号会丢弃取消后晚到的截图结果，复制与钉图操作互斥。
- 内存峰值优化：选区裁剪完成后 Rust 立即释放虚拟桌面原始像素；选区图片解码完成后前端立即撤销整屏 Blob URL，不让不可见的全屏预览滞留到输出阶段。
- 本轮验证：`npm run check`、`npm run build`、`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 与 `cargo test` 全部通过；Rust 测试为 13 通过、1 个真实桌面测试忽略。
- 混合 DPI 协议升级：`select_capture_region` 不再接收逻辑 viewport 矩形，改为 `PhysicalSelectionRect`；自动测试覆盖 `(-1920, 0)` 的 100% 左屏与 `(0, 0)` 的 150% 主屏之间的跨屏裁剪。
- 马赛克预览升级：Canvas 逐块读取当前已提交画面的 RGBA 像素，按与 Rust 相同的整数平均规则回填，不再使用深色网格占位效果。
- 画笔路径升级：前端松开鼠标时先简化再传输，Rust 保存前执行同算法防御性规范化；约 2,000 点的带转角测试路径压缩到不超过 5 点并保留首尾与转角。
- 三项优先级最终验收：`npm run check`、16 模块生产构建、`cargo fmt --check`、Clippy 零警告与 `cargo test` 全部通过；Rust 为 14 通过、1 个交互式测试忽略，真实当前桌面抓取在沙箱外额外通过（约 0.13 秒）。前端纯函数样例验证跨屏映射 `2,001 → 4` 点路径简化，以及 2×2 RGBA 马赛克平均值与 Rust 测试一致。
- 钉图缩放升级：滚轮阶段只做 CSS 即时预览，90ms 静默期后使用物理窗口尺寸提交；保持鼠标锚点，窗口小于工作区时完整夹取，超出工作区时至少保留 64px 可操作区域。纯函数验证中心锚点放大与右下角工作区夹取。
- 文字标注升级：移除 `window.prompt`，新增跟随 Canvas 缩放/滚动定位的原位多行编辑器；`Ctrl+Enter`/确认提交，`Esc`/取消只退出文字编辑，复制或钉图前会同步已确认文字。
- 性能基准升级：Rust 返回 GDI 抓屏、裁剪、标注渲染、剪贴板、钉图 PNG 和窗口创建耗时；前端记录元数据/PNG IPC、浏览器解码和端到端耗时。当前机器 GUI 样本为抓屏约 214ms、整屏 PNG/IPC 247ms、解码 12ms、裁剪 20ms、选区 PNG/IPC 45ms、选区解码 7.1ms。
- 基准驱动优化：整屏和选区 PNG 编码改用 `tauri::async_runtime::spawn_blocking`，避免 CPU 密集编码占用窗口命令线程；使用 Tauri 已内置运行时，未增加直接 Tokio 依赖。GUI 自动验收确认性能 HUD 与选区底栏正常显示；继续操作时检测到用户实体输入，按安全规则停止自动鼠标/键盘验收。
- 后三项最终静态验收：`npm run check`、17 模块生产构建、`cargo fmt --check`、Clippy 零警告与 `cargo test` 全部通过；Rust 为 14 通过、1 个交互式桌面测试忽略。钉图几何纯函数样例验证中心锚点缩放、工作区夹取与最小缩放 `0.16`。
- 截图黑帧优化：Rust 抓屏后只定位隐藏遮罩并发送重置事件；前端在隐藏状态完成 PNG IPC 与解码，随后先显示内容透明的窗口、等待两帧让 WebView2 合成器就绪，再原子切换到截图选择界面。隐藏加载期间重复快捷键会被活动会话拦截，加载或 reveal 失败会自动清空会话，避免后续热键失效。
- 黑帧优化静态验收：`npm run check`、17 模块生产构建、`cargo fmt --check`、Clippy 零警告与 `cargo test` 全部通过；Rust 为 14 通过、1 个交互式桌面测试忽略。合成器预热带 100ms 超时兜底，避免极端限帧场景卡住截图会话。
- 钉图闪烁优化：动态钉图窗口改为 `visible(false)`，创建后通过 windows-rs 将整个 HWND 设为 0% 原生透明度；前端并行读取元数据/PNG并完成图片解码后，先无焦点、无阴影地显示透明窗口供 WebView2 预热两帧，最终命令再启用阴影、设置焦点并将 HWND 切到 100%。初始化失败会关闭隐藏窗口并触发 `PinStore` 清理。
- 钉图闪烁优化静态验收：`npm run check`、18 模块生产构建、`cargo fmt --check`、Clippy 零警告与 `cargo test` 全部通过；Rust 为 14 通过、1 个交互式桌面测试忽略。截图遮罩和钉图窗口共用带 100ms 兜底的合成器预热模块。
