# SnapRust 开发计划

> 当前状态：V0.7B 截图翻译与多提供商适配已完成。Windows 上已打通 `Ctrl + Shift + A` → 虚拟桌面截图 → 框选 → 标注 / 本地 OCR / LLM 翻译 → 复制图片或文字 / 创建独立置顶钉图窗口 → `Alt + H` 本地历史管理、自动清理与导出。

## 1. 技术栈与范围

### V0.1 固定技术栈

- 后端：Rust、Tauri 2、`windows`（windows-rs）、`image`、`serde`
- 前端：Vanilla TypeScript、HTML、CSS、Vite
- 目标平台：Windows 10/11，MSVC 工具链，WebView2
- 暂不引入：直接 Tokio 依赖、前端框架、SQLite、云服务和第三方 OCR 模型

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

- [x] Windows 独立原生无边框置顶 HWND；每张钉图使用唯一标签和独立消息线程
- [x] Rust 直接持有最终合成后的 BGRA 像素；Windows 钉图不再编码 PNG 或创建 WebView
- [x] 拖动移动、滚轮缩放、`Shift + 滚轮`/键盘透明度、双击/`Esc` 关闭
- [x] 使用 `HWND`、`WS_EX_LAYERED` 与 `SetLayeredWindowAttributes` 调整原生窗口透明度
- [x] 窗口销毁时清理 Rust `PinStore`，创建失败时回滚已插入图片

#### V0.4 钉图架构

```text
Annotation[] + Selected RgbaImage
              ↓ Rust 最终栅格化
PinStore（label → 生命周期与尺寸）
              ↓ Rust 直接传递 RgbaImage
原生 HWND + 独立 Windows 消息线程
              ├─ WM_PAINT / StretchDIBits（同步高质量重绘）
              ├─ WM_MOUSEWHEEL / SetWindowPos（鼠标锚点缩放）
              ├─ SetLayeredWindowAttributes（透明度）
              ├─ WM_NCLBUTTONDOWN（拖动）
              └─ WM_NCDESTROY（注销 HWND 并释放图片）
```

Windows 原生钉图不依赖 Tauri WebView 调度；旧的 `pin.html` 实现仅作为非 Windows 回退。每张原生钉图线程拥有自己的 HWND 与消息循环，截图主流程仍不直接依赖 Tokio。

### V0.5 OCR

- [x] 使用 Windows `Windows.Media.Ocr` 与当前用户语言包完成纯本地识别
- [x] Rust 从截图会话复制原始选区，排除前端标注对识别结果的干扰
- [x] 超大选区按 `OcrEngine::MaxImageDimension` 保持比例缩小
- [x] PNG 编码、WinRT 解码和 OCR 在 Tauri 后台阻塞任务中执行，不阻塞 UI
- [x] 图片外侧结果面板显示语言、行数、尺寸与耗时，并通过 Rust 写入 `CF_UNICODETEXT`
- [x] OCR 结果可直接编辑，修正后的文字可以一键复制
- [x] 枚举 Windows 已安装的 OCR 语言，支持自动或指定语言重新识别
- [x] 逐行坐标与文本区域高亮

当前任务复用 Tauri 已内置的异步运行时即可，没有为了单次 OCR 引入直接 Tokio 依赖。OCR 仍是截图后的独立能力，失败不会破坏复制图片或钉图链路。

### V0.6 历史记录

- [x] SQLite 保存图片文件名、尺寸、时间、OCR 文本、标签和收藏状态
- [x] 复制图片或钉图成功后自动保存 Rust 最终合成 PNG；取消和仅复制文字不落盘
- [x] `Alt + H` 打开历史窗口，支持 OCR 文本搜索、收藏筛选、复制图片与重新钉图
- [x] 删除操作要求前端明确确认，并删除同一记录对应的 PNG 与 SQLite 元数据
- [x] 历史卡片内联标签编辑；标签参与 OCR/标签统一搜索，并兼容旧数据库自动迁移
- [x] 自动保留最多 500 条且 PNG 总占用最多 2 GiB，优先删除最旧未收藏项；收藏永不自动删除
- [x] 当前筛选结果多选、全选、批量收藏、批量取消收藏和批量确认删除
- [x] 批量导出原始 PNG 与 UTF-8 BOM CSV 元数据到 Windows“下载/SnapRust Exports/时间戳”目录

### V0.7 主动触发的即时翻译（进行中）

产品定位不是系统输入法，而是“随时可调用的桌面翻译助手”。翻译必须由用户的明确快捷键、按钮或输入操作触发；不做后台读取剪贴板、不做输入监听、不在未经确认时把翻译文本写回其他应用。

```text
任意应用选中文字 ── Ctrl + Shift + T ──► 复制选区文字 ──► 翻译浮窗
                                                            ├── 复制译文
                                                            ├── 用户确认后替换原文
                                                            └── 固定为普通钉图式翻译卡片

截图框选 ── OCR ──► 点击“翻译” ──► 原文 / 译文并排显示 ──► 复制 / 保存历史
```

#### V0.7A：文本选择翻译

- [ ] 注册默认全局快捷键 `Ctrl + Shift + T`；快捷键仅在用户主动触发时读取当前选择
- [ ] Windows 路径通过一次受控 `Ctrl+C` 读取 `CF_UNICODETEXT`；选中内容无法复制的应用需展示明确失败提示，而不是猜测或注入文本
- [ ] 创建小型、无任务栏、可关闭的翻译浮窗，显示原文、译文、源语言、目标语言、耗时与错误状态
- [ ] 支持复制译文；“替换原文”必须由用户点击确认后才写剪贴板并发送粘贴动作
- [ ] 对同一段原文、源/目标语言做进程内缓存，避免短时间重复请求
- [ ] 所有文本传输由 Rust `translation` 模块执行；前端仅管理浮窗和状态

#### V0.7B：OCR 翻译闭环

- [x] OCR 面板增加“翻译”按钮，输入使用用户当前编辑后的 OCR 文本
- [x] OCR 译文与原文并列显示；支持分别复制，且不影响原有“复制 OCR 文字”行为
- [ ] 可选地将用户确认保存的译文写入历史元数据；默认不保存翻译内容
- [ ] 历史搜索继续匹配 OCR/标签；是否将译文纳入搜索必须是用户可见的独立设置

当前实现说明：V0.7B 使用 Provider Adapter 统一 Chat Completions 请求，内置 DeepSeek、OpenAI、OpenAI-compatible 网关和本机 Ollama 适配器；翻译请求只在用户点击“翻译”后由 Rust 发起。提供商、模型、端点和密钥通过环境变量或面板配置，不进入前端代码、截图历史或 SQLite。目标语言由 OCR 面板选择，提示词要求模型只返回译文。翻译失败只更新翻译面板状态，不影响复制图片、钉图和 OCR 原文复制。

#### V0.7 的翻译服务边界

- [x] 选定首个 LLM 翻译服务并实现 Provider Adapter / Chat Completions HTTP 边界，不把 HTTP/API 逻辑写进 TypeScript
- [x] UI 直接配置 DeepSeek、OpenAI、OpenAI-compatible 网关或 Ollama 的 API Key、模型、端点并保存；提供保存后测试请求
- [ ] UI 在首次启用在线翻译前明确提示“文本将发送至所选翻译服务”；提供总开关和清晰的失败/离线状态
- [ ] API 密钥不得写入前端、截图历史或明文 SQLite；Windows 上优先使用系统凭据存储或 DPAPI 保护
- [ ] 暂不捆绑离线翻译模型：避免将轻量 EXE 直接膨胀到数百 MiB/数 GiB；离线模型作为后续独立评估项

验收：在记事本、浏览器网页、Word/Office 类编辑器、VS Code/JetBrains 编辑器各人工验证一次“选中 → 热键 → 翻译 → 复制”；明确记录管理员窗口、密码框、游戏和不可复制控件的受限行为。网络超时、无网络、翻译服务限流、剪贴板占用和快速连续热键必须都能恢复到空闲状态。

### V0.8 设置、隐私与日常体验

- [ ] 设置窗口：源/目标语言、翻译服务、隐私开关、截图/历史/翻译快捷键及冲突提示（当前已先完成翻译服务配置子集）
- [ ] 翻译历史默认关闭；用户启用后才保存原文、译文、语言、时间，并复用现有 SQLite 保留策略
- [ ] 历史页增加“仅图片 / OCR / 翻译”筛选和统一全文搜索，但不让翻译数据默认泄露到导出
- [ ] 截图、OCR、翻译和历史操作提供一致的短状态提示、可读错误和键盘可访问性
- [ ] 补充真实双显示器、100%/125%/150% DPI、常用粘贴目标和高频截图压力回归

### V0.9 发布准备与稳定性

- [ ] 生成 NSIS 或 MSI 安装包，提供安装、卸载、开始菜单与可选开机启动
- [ ] 规划并接入 Windows 代码签名；在签名前明确发行者名称、证书采购方式和私钥保管流程
- [ ] 增加崩溃日志导出与“复制诊断信息”，默认不上传任何截图、OCR 或翻译文本
- [ ] 维护版本迁移、历史备份/恢复说明与发行说明；安装包在干净 Windows 用户账户中回归
- [ ] 自动更新、云同步和账号系统保持不做，除非后续有明确的用户需求与隐私方案

### 明确暂不做

- [ ] 真正的 Windows IME / 输入法：需要 TSF、COM 输入服务 DLL、系统注册、代码签名及长期兼容性维护，应作为独立产品/仓库评估
- [ ] 未经用户触发的剪贴板监听、输入记录或自动上传文本
- [ ] 默认联网、默认保存翻译历史、默认云端同步
- [ ] 大型本地翻译模型和 GPU 推理运行时

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
| 在线翻译泄露敏感文字 | V0.6 不涉及 | V0.7 首次启用明确告知、用户主动触发、服务可替换且可完全关闭 |
| 读取当前选中文字不兼容 | V0.6 不涉及 | 仅通过受控复制读取，失败时提示用户先手动复制；不注入进程或绕过应用安全边界 |
| 翻译网络延迟影响交互 | V0.6 不涉及 | 请求可取消、显示进行中/失败状态、短期缓存和超时；不阻塞截图/OCR 主链路 |
| 系统输入法复杂度失控 | V0.6 不涉及 | 明确不纳入 V0.x；后续若立项，单独采用 TSF/COM 组件、签名和兼容性测试计划 |

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
- [x] 钉图体验：鼠标锚点即时缩放预览、立即提交与在途最新值追赶、最小操作尺寸与工作区边界保护
- [x] 文字体验：截图原位多行输入、确认/取消、键盘交互与 512 字符协议上限
- [x] 性能基准：Rust 分段计时、前端端到端计时、HUD/底栏/控制台展示与后台 PNG 编码
- [x] 截图显示时序：隐藏预加载并解码 PNG、透明首帧预热 WebView 合成器、就绪后再显示截图遮罩
- [x] 钉图显示时序：隐藏且原生全透明创建、无阴影预热 WebView、图片就绪后一次性恢复阴影和不透明度
- [x] 钉图缩小透明区域：动态 WebView 启用原生透明，CSS 缩小预览未覆盖区域不再露出黑色窗口背景
- [x] V0.5：Windows 内置 OCR、超大选区等比缩放和原始选区识别
- [x] V0.5：图片外侧结果面板、识别元数据、错误提示和关闭交互
- [x] V0.5：通过 Rust `CF_UNICODETEXT` 一键复制识别文字
- [x] V0.5：英文合成图 OCR 自动测试与中英文真实截图人工回归
- [x] V0.5：OCR 结果编辑、修改后复制与空内容按钮状态
- [x] V0.5：已安装语言枚举、语言下拉选择与切换后自动重新识别
- [x] V0.5：OCR 后台线程级 COM apartment，连续枚举/识别不重复拆除 WinRT 环境
- [x] V0.5：将 Windows OCR 单词矩形合并为逐行区域，缩小识别后映射回原始选区；结果行悬停/聚焦高亮并可点击定位
- [x] V0.6：SQLite/PNG 本地历史、自动入库、OCR 搜索、收藏、历史复制/钉图与确认删除
- [x] V0.6：标签编辑、标签搜索与 SQLite 数据库版本迁移
- [x] V0.6：500 条未收藏优先自动清理，以及筛选范围内的批量收藏 / 删除
- [x] V0.6：2 GiB PNG 磁盘占用自动清理、占用可视化与批量 PNG/CSV 导出
- [ ] V0.7A：选中文字 `Ctrl + Shift + T` 翻译浮窗、复制译文与用户确认替换
- [x] V0.7B：OCR 原文/译文并列显示与分别复制
- [x] V0.7B：Provider Adapter、多模型配置和 DeepSeek/OpenAI/OpenAI-compatible/Ollama 翻译服务选择
- [ ] V0.7B：用户确认后保存译文到历史元数据
- [ ] V0.8：翻译服务与语言设置、隐私开关、双屏/DPI/常用编辑器回归
- [ ] V0.9：安装包、签名准备、诊断导出和干净环境发布回归

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
- 钉图缩放升级：滚轮阶段先做 CSS 即时预览，再使用物理窗口尺寸提交；保持鼠标锚点，窗口小于工作区时完整夹取，超出工作区时至少保留 64px 可操作区域。纯函数验证中心锚点放大与右下角工作区夹取。
- 文字标注升级：移除 `window.prompt`，新增跟随 Canvas 缩放/滚动定位的原位多行编辑器；`Ctrl+Enter`/确认提交，`Esc`/取消只退出文字编辑，复制或钉图前会同步已确认文字。
- 性能基准升级：Rust 返回 GDI 抓屏、裁剪、标注渲染、剪贴板、钉图 PNG 和窗口创建耗时；前端记录元数据/PNG IPC、浏览器解码和端到端耗时。当前机器 GUI 样本为抓屏约 214ms、整屏 PNG/IPC 247ms、解码 12ms、裁剪 20ms、选区 PNG/IPC 45ms、选区解码 7.1ms。
- 基准驱动优化：整屏和选区 PNG 编码改用 `tauri::async_runtime::spawn_blocking`，避免 CPU 密集编码占用窗口命令线程；使用 Tauri 已内置运行时，未增加直接 Tokio 依赖。GUI 自动验收确认性能 HUD 与选区底栏正常显示；继续操作时检测到用户实体输入，按安全规则停止自动鼠标/键盘验收。
- 后三项最终静态验收：`npm run check`、17 模块生产构建、`cargo fmt --check`、Clippy 零警告与 `cargo test` 全部通过；Rust 为 14 通过、1 个交互式桌面测试忽略。钉图几何纯函数样例验证中心锚点缩放、工作区夹取与最小缩放 `0.16`。
- 截图黑帧优化：Rust 抓屏后只定位隐藏遮罩并发送重置事件；前端在隐藏状态完成 PNG IPC 与解码，随后先显示内容透明的窗口、等待两帧让 WebView2 合成器就绪，再原子切换到截图选择界面。隐藏加载期间重复快捷键会被活动会话拦截，加载或 reveal 失败会自动清空会话，避免后续热键失效。
- 黑帧优化静态验收：`npm run check`、17 模块生产构建、`cargo fmt --check`、Clippy 零警告与 `cargo test` 全部通过；Rust 为 14 通过、1 个交互式桌面测试忽略。合成器预热带 100ms 超时兜底，避免极端限帧场景卡住截图会话。
- 钉图闪烁优化：动态钉图窗口改为 `visible(false)`，创建后通过 windows-rs 将整个 HWND 设为 0% 原生透明度；前端并行读取元数据/PNG并完成图片解码后，先无焦点、无阴影地显示透明窗口供 WebView2 预热两帧，最终命令再启用阴影、设置焦点并将 HWND 切到 100%。初始化失败会关闭隐藏窗口并触发 `PinStore` 清理。
- 钉图闪烁优化静态验收：`npm run check`、18 模块生产构建、`cargo fmt --check`、Clippy 零警告与 `cargo test` 全部通过；Rust 为 14 通过、1 个交互式桌面测试忽略。截图遮罩和钉图窗口共用带 100ms 兜底的合成器预热模块。
- 钉图缩小黑底修复：动态 `WebviewWindowBuilder` 补充 `transparent(true)`，使 `pin.css` 中透明的 `html`、`body` 和 `.pin-root` 真正透传到 Windows 桌面；滚轮缩小 CSS 预览阶段即使原生窗口尚未提交新尺寸，图片周围也保持透明。
- 钉图缩小黑底静态验收：核对 Tauri 2.11.5 源码确认该配置同时传递给窗口构建器和 WebView2 `with_transparent`；`npm run check`、18 模块生产构建、`cargo fmt --check`、Clippy 零警告和 `cargo test` 全部通过，Rust 为 14 通过、1 个交互式桌面测试忽略。
- 钉图缩放延迟优化：移除固定 90ms 防抖；Windows 后端以单次 `SetWindowPos` 原子更新物理位置和外部尺寸，并根据当前 inner/outer 差值换算目标窗口边框尺寸，避免两次 Tauri 调用造成的等待与位置跳变。
- Release 体积实测：当前 `bundle.active=false`；生产前端约 `50 KiB`，最新 `snaprust.exe` 为 `9,408,000` 字节（`8.97 MiB`），另有无需随普通发行版分发的 `5.93 MiB` PDB。
- 缩放延迟最终验收：`npm run check`、18 模块生产构建、`cargo fmt --check`、Clippy 零警告和 `cargo test` 全部通过；Rust 为 15 通过、1 个交互式桌面测试忽略，并新增目标 inner 尺寸到 Win32 outer 尺寸的边框差值及溢出测试。
- 钉图缩放热路径优化：移除 `requestAnimationFrame` 提交等待，空闲状态在滚轮事件内立即发起原生命令，在途期间只保留最新修订并在完成后立刻追赶；前端缓存位置、inner 尺寸和工作区，仅在初始化/拖动后刷新。Rust 改用直接 Win32 `GetClientRect`/`GetWindowRect` 计算边框差值，使每轮热路径从三次 Tauri 查询加一次更新缩减为单次 Command 内的原生读取和 `SetWindowPos`。
- 钉图缩放热路径最终验收：`npm run check`、18 模块生产构建、`cargo fmt --check`、Clippy 零警告、`cargo test` 与 Release 增量构建全部通过；Rust 为 15 通过、1 个交互式桌面测试忽略，优化后 EXE 为 `8.97 MiB`。
- 钉图缩放原生化：普通滚轮改由进程内 `WH_MOUSE_LL` 钩子处理。钩子仅在至少存在一个钉图且鼠标命中其根 HWND 时读取滚轮增量，依据当前物理窗口尺寸、初始宽高、鼠标屏幕坐标和目标显示器工作区直接执行一次 `SetWindowPos`；这条热路径不再经过 WebView 事件、JavaScript 缩放预览或 Tauri IPC。`Shift + 滚轮` 会跳过原生缩放并继续交给前端透明度逻辑，多张钉图通过 HWND 注册表独立匹配，销毁后同步注销；无钉图时通过原子计数直接跳过全部窗口查询。
- 原生缩放最终验收：应用实际启动并成功注册钩子，全局快捷键可进入截图覆盖层且 `Esc` 正常取消；`npm run check`、18 模块生产构建、`cargo fmt --check`、Clippy 零警告、`cargo test` 和 Release 构建全部通过，Rust 为 16 通过、1 个交互式桌面抓屏测试忽略。Release EXE 仍为 `9,408,000` 字节（`8.97 MiB`）。当前桌面自动化将透明全屏覆盖层误报为 `14 × 14`，无法可靠代替人工拖框，因此连续滚轮的主观丝滑度仍保留为一次人工回归项。
- Windows 原生钉图窗口：移除钉图运行时的 WebView2、JavaScript、Tauri Command 和全局低级鼠标钩子。每张钉图在独立线程创建 `WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED` 原生 HWND；窗口过程直接处理滚轮、拖动、双击、键盘复位/透明度与销毁。滚轮在一次 `SetWindowPos` 后同步 `InvalidateRect + UpdateWindow`，`WM_PAINT` 使用 `StretchDIBits` 的 `HALFTONE` 模式重绘图片与原生 HUD，消息返回时画面已经完成更新。
- 原生钉图数据与生命周期：Windows 路径直接将最终 `RgbaImage` 转为 BGRA 内存，不再执行钉图 PNG 编码、二进制 IPC、Blob 解码或合成器预热。窗口 `WM_NCDESTROY` 同步注销 label→HWND 并删除 `PinStore` 条目；非 Windows 仍保留原 Tauri/WebView 回退。
- 原生钉图最终验收：`npm run check`、18 模块生产构建、`cargo fmt --check`、Clippy 零警告、常规 Rust 测试 16 通过/2 个交互式测试忽略、Release 构建全部通过；新增的真实 HWND 测试已单独显式运行，确认窗口创建、一格 `WM_MOUSEWHEEL` 后宽高同步增加、关闭以及 HWND/图片状态清理。
- 钉图 HUD 外置布局：Windows 原生钉图不再把缩放/透明度信息绘制进图片客户区，而是创建一个 `112 × 20`、无背景、点击穿透且不抢焦点的独立原生 HUD 浮窗。HUD 以小号白字贴在图片右上方外侧，带一像素暗色文字阴影；图片上方没有空间时自动切换到右下方外侧，并始终限制在当前显示器工作区。新增纯函数测试验证上方定位、贴顶时下方回退与右边对齐。
- 外置 HUD 最终验收：Clippy 零警告、常规 Rust 测试 17 通过/2 个交互式测试忽略；真实 HWND 测试单独执行通过，确认主钉图与 HUD 窗口可共同创建、缩放和清理；Release 构建通过。最新 EXE 为 `9,396,224` 字节（约 `8.96 MiB`），PDB 约 `5.99 MiB`。
- V0.5 OCR 实现：新增独立 `ocr` 模块，通过 `InMemoryRandomAccessStream`、`BitmapDecoder`、`SoftwareBitmap` 和 `OcrEngine` 识别 Rust 会话中的未标注选区；识别超大图片前按系统上限等比缩小。命令使用 Tauri 内置 `spawn_blocking`，没有新增直接 Tokio、OCR 模型或云服务依赖。
- V0.5 OCR 界面：标注工具栏新增 `OCR`；结果显示在截图外侧的响应式面板中，包含系统识别语言、非空行数、源/处理尺寸和耗时。文字复制通过 Rust 写入 `CF_UNICODETEXT`，不会与 `Ctrl+C` 复制图片的原有行为混淆。
- V0.5 OCR 验收：`npm run build`、常规 Rust 测试（20 通过、3 个交互式测试忽略）和 Release 构建通过；显式 Windows OCR 测试从合成图片成功识别 `SNAPRUST OCR 12345`。真实 125% DPI 桌面框选 `982 × 494` 中英文区域，`zh-Hans-CN` 在约 `156ms` 返回结果，复制按钮显示“已复制”，`Esc` 正常退出。新 EXE 为 `9,521,152` 字节（约 `9.08 MiB`）。
- 钉图 HUD 配色调整：外置缩放/透明度提示改为黑色正文和一像素浅色阴影；透明窗口色键由黑色改为洋红色内部键值，避免黑色文字被 `LWA_COLORKEY` 一并透明。新增测试保证正文黑色与透明色键永远不同。
- V0.5 下一步：OCR 文本框改为可编辑，识别错误可原位修正；复制按钮始终读取编辑后的当前文本，空内容自动禁用，聚焦时以细绿色边线提示编辑状态。
- V0.5 语言切换：新增 `list_ocr_languages` 命令，使用 `OcrEngine::AvailableRecognizerLanguages` 返回标签、本地名称和显示名称；前端保留“自动（系统）”选项，只展示当前 Windows 已安装的语言。切换下拉框后立即用 `TryCreateFromLanguage` 重新识别，用户选择在后续截图会话中保留。
- OCR 连续调用稳定性：实测“枚举语言后立即指定语言识别”暴露重复 `CoInitializeEx`/`CoUninitialize` 与 windows-rs WinRT 工厂缓存之间的访问冲突；改为每个 Tauri blocking worker 使用线程局部 COM apartment，在线程退出时才注销。连续枚举与显式识别测试随后通过。
- OCR 语言验收：当前设备实际枚举到 `中文(中华人民共和国) · zh-Hans-CN`；使用该显式语言从合成图片成功识别 `SNAPRUST OCR 12345`。常规 Rust 测试为 21 通过、4 个系统/桌面测试忽略，语言标签控制字符输入会被拒绝；Clippy、TypeScript、生产前端和 Release 构建全部通过。最新 EXE 为 `9,597,952` 字节（约 `9.15 MiB`）。
- OCR 文本定位：Windows 返回逐词 `BoundingRect`，Rust 合并为行并在 OCR 为满足系统尺寸上限而缩小图片时映射回源选区；前端结果行支持鼠标悬停、键盘聚焦高亮及点击滚动画布定位。新增坐标缩放/边界单元测试，真实 Windows OCR 合成图测试同时断言至少存在一个有效定位行。
- V0.6 历史记录：新增 `rusqlite`（bundled SQLite）和 `history` Rust 模块。应用数据目录内以 SQLite 保存元数据、以独立 PNG 保存最终截图；`Alt + H` 复用隐藏 overlay 窗口打开历史，支持缩略图、OCR 文本搜索、仅收藏、复制、重新钉图与确认删除。单元测试覆盖保存、搜索、收藏、缩略图与图片/元数据删除闭环。
- V0.6 静态验收：`npm run check`、18 模块生产前端构建、`cargo fmt --check`、Clippy 零警告和 `cargo test` 全部通过；Rust 为 27 通过、4 个依赖真实 Windows 服务/桌面的测试忽略。Bundled SQLite 使 Release EXE 变为 `11,611,136` 字节（约 `11.07 MiB`），无需附带 DLL。
- V0.6 标签：`screenshots` 新增 `tags` 文本列；启动时用 `PRAGMA table_info` 检测旧库并只在缺列时执行迁移。标签最多 12 个、单项最多 48 字符、以逗号分隔输入且自动去重；历史统一搜索同时匹配 OCR 文本与标签。静态检查与保存/搜索/收藏/缩略图/删除单元测试再次通过。
- V0.6 历史管理：每次新截图成功保存后检查总历史量，超过 500 条时按保存时间删除最旧的未收藏记录；任何收藏都不会被自动清理。前端提供缩略图多选和当前筛选结果全选，Rust 对批量 ID 做正数、去重和最多 200 条限制后执行收藏/取消收藏或删除；批量删除仍必须经过浏览器确认。测试覆盖保留收藏的自动清理、批量去重收藏和批量删除。
- V0.6 历史清理与导出：保留策略升级为“最多 500 条且 PNG 合计最多 2 GiB”；每次保存后按时间删除最旧的未收藏记录，收藏记录永不自动删除。历史页显示实际 PNG 占用/上限和条目数。批量“导出”会将选中的 PNG 按当前选择顺序复制到 Windows“下载/SnapRust Exports/SnapRust-时间戳/”，并写入带 UTF-8 BOM 的 `metadata.csv`（ID、文件名、尺寸、时间、收藏、标签和 OCR 文字）；原历史不会修改。单元测试覆盖磁盘阈值清理、收藏保留、PNG 输出及 OCR/标签 CSV 转义。
- 本轮最终验收：`npm run check`、18 模块 Vite 生产构建、`cargo fmt --check`、`cargo check`、Clippy 零警告、`cargo test` 和 Release 构建全部通过；Rust 单元测试为 28 通过、4 个真实 Windows 服务/桌面测试忽略。最新 `snaprust.exe` 为 `11,617,280` 字节（约 `11.08 MiB`）。
