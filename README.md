# SnapRust

SnapRust 是一个用 Rust + Tauri 2 构建的 Windows 截图工具。当前已进入 V0.7B，并完成截图、标注、钉图、本地 OCR、LLM 翻译与历史记录闭环：

```text
Ctrl + Shift + A → 框选 → 标注 / OCR / 翻译 → 复制图片、文字 / 钉到桌面 → Alt + H 查看历史
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
- 已完成 V0.4 钉图：Windows 上由 Rust 创建独立原生无边框置顶 HWND，并直接持有最终 RGBA 像素
- 已完成 V0.5 OCR MVP：使用 Windows 内置 `Windows.Media.Ocr` 在本地识别原始选区，不捆绑 Tesseract、模型文件或云服务
- OCR 结果在图片外侧面板显示，提供识别语言、行数、处理尺寸和耗时；结果可直接修改并一键复制，识别行可回指并高亮截图中的原始文字区域
- OCR 面板会枚举 Windows 已安装的语言包，支持“自动跟随系统”或选择指定语言后立即重新识别
- 超过 Windows OCR 尺寸上限的选区会在 Rust 中保持比例缩小；OCR 在后台阻塞任务中运行，不冻结标注界面
- 已接入截图后 LLM 翻译 MVP：点击“翻译”会自动执行 OCR（若尚未识别），支持中文、英文、日文、韩文、俄文、西班牙文、法文和德文目标语言；首个提供商为 DeepSeek，模型可选择，译文可复制，翻译请求由 Rust 后端发送
- 翻译后端已抽象为 Provider Adapter，支持 DeepSeek、OpenAI、OpenAI-compatible 网关和本机 Ollama；模型名称可在设置中配置，截图/OCR 主链路不依赖具体服务
- 已完成 V0.6 历史记录：复制图片或钉图成功后自动保存最终 PNG、尺寸、时间、OCR 文本、标签和收藏状态
- `Alt + H` 打开历史窗口，可按 OCR 文本或自定义标签搜索、仅显示收藏、复制历史图片、重新钉图或删除记录
- 历史同时限制为最多 500 条、最多 2 GiB PNG；达到任一上限后自动清理最旧的未收藏截图
- 历史支持多选、全选当前结果、批量收藏、批量删除，以及导出选中 PNG 与 CSV 元数据
- 钉图支持拖动、滚轮缩放、`Shift + 滚轮`/`[`/`]` 调透明度、`0` 复位、双击/`Esc` 关闭
- 整屏与选区 PNG 通过二进制 IPC 传输，不再把大图转换为 Base64 字符串；Windows 原生钉图直接接收 Rust `RgbaImage`，不再编码或传输钉图 PNG
- 标注拖动使用已提交画面缓存与逐帧合并重绘，并在退出时主动释放 Blob URL 和 Canvas 像素内存
- 进入标注后立即释放不再使用的虚拟桌面原图，降低 4K 与多显示器场景的内存峰值
- 混合 DPI 选区使用虚拟桌面绝对物理像素协议，支持负坐标与不同显示缩放比例之间的跨屏映射
- 马赛克预览与 Rust 输出使用相同的块平均算法；长画笔在前后端执行路径简化
- 文字标注使用截图内联多行编辑器，支持 `Ctrl+Enter` 确认、`Esc` 取消，不再弹出浏览器 prompt
- Windows 钉图不再创建 WebView：独立原生消息线程直接处理 `WM_MOUSEWHEEL`，一次 `SetWindowPos` 后同步执行 `WM_PAINT`，滚轮消息返回前完成高质量 GDI 重绘
- 钉图创建前像素已经在 Rust 内存中就绪，原生窗口首帧直接绘制图片，不存在 PNG IPC、Blob 解码、WebView2 预热、黑底或旧窗口透明空洞阶段
- 已加入 Windows 系统托盘：可从托盘开始截图、打开历史、打开翻译设置或退出程序
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

LLM 翻译支持 DeepSeek、OpenAI、OpenAI-compatible 网关和本机 Ollama。启动后点击截图遮罩中的“⚙ 设置”，即可直接配置提供商、API Key、模型和端点；默认模型为 `deepseek-v4-flash`。配置保存在 SnapRust 应用数据目录，不写入截图历史 SQLite。

也可以通过环境变量提供初始配置：

```powershell
$env:SNAPRUSTRANSLATOR_PROVIDER = "deepseek" # deepseek / openai / openai-compatible / ollama
$env:SNAPRUSTRANSLATOR_API_KEY = "你的 DeepSeek API 密钥"
$env:SNAPRUSTRANSLATOR_MODEL = "deepseek-v4-flash" # 可选
npm run tauri dev
```

选择 `deepseek` 时也可以直接使用 DeepSeek 官方变量 `DEEPSEEK_API_KEY`。OpenAI 默认端点为 `https://api.openai.com/v1`，OpenAI-compatible 默认端点为 `http://127.0.0.1:3000/v1`，Ollama 默认端点为 `http://127.0.0.1:11434/v1`；程序会请求端点下的 `/chat/completions` 路径。首次点击“翻译”前，OCR 文字仍只在本机处理；点击后，当前编辑框中的 OCR 文本会主动发送到配置的 LLM 服务。

程序启动后窗口默认隐藏。在任意应用中按 `Ctrl + Shift + A` 进入截图模式：

程序启动后会常驻 Windows 系统托盘。右键或左键点击托盘图标可选择“开始截图”“截图历史”“翻译设置”或“退出 SnapRust”。

1. 拖动鼠标框选区域；选区外会变暗，边框旁会显示尺寸。
2. 松开鼠标后进入标注编辑器；可选择箭头、矩形、椭圆、画笔、马赛克或文字，并调整颜色和粗细。文字工具点击截图后会在原位显示多行输入框，使用 `Ctrl+Enter`/“确认”提交，`Esc`/“取消”放弃。
3. 使用 `Ctrl + Z` 撤销、`Ctrl + Y` 重做，或点击“清空”移除标注。
4. 点击工具栏的 `OCR` 识别原始选区中的文字。结果会显示在截图右侧（窄窗口时显示在下方）；语言下拉框可选择“自动（系统）”或任一已安装的 OCR 语言，切换后会立即重新识别。结果可直接修正错字，再点击“复制文字”写入 Windows 文本剪贴板；悬停或聚焦下方的识别行会高亮截图对应文字区域，点击会将画布滚动到该位置。OCR 不会把当前箭头、马赛克或文字标注带入识别输入。
5. 点击工具栏的 `翻译`，或在 OCR 面板中点击翻译按钮。若尚未执行 OCR，程序会先识别原始选区；随后将当前编辑后的 OCR 文本发送到翻译服务，目标语言可在翻译面板中选择，译文可单独复制。翻译网络失败不会影响图片复制和钉图。
6. 在截图图片上右键可打开操作菜单：复制图片、钉图、OCR、OCR 并翻译、向左/向右旋转 90°、重置旋转或销毁当前截图。旋转会参与 Rust 最终合成，复制和钉图得到的是真正旋转后的图片。
7. 按 `Ctrl + C` 或 `Enter` 复制图片，或点击“📌 钉图”创建独立置顶窗口。两种图片输出都由 Rust 对原选区执行最终标注栅格化。
8. 钉图中拖动可移动，滚轮以鼠标位置为中心缩放，`Shift + 滚轮` 或 `[`/`]` 调透明度，`0` 复位，双击或 `Esc` 关闭。钉图上右键可复制当前图片、左右旋转 90°、重置视图或销毁钉图；旋转会直接更新原生窗口持有的像素。Windows 原生 HWND 在自己的窗口过程中处理输入，位置和尺寸通过一次 Win32 调用原子更新，并在滚轮消息返回前同步重绘图片；缩放/透明度以图片外侧右上方的小号白字 HUD 显示，同时防止窗口完全移出显示器工作区。
9. 输出成功后遮罩自动关闭；截图阶段可用 `Esc` 或鼠标右键取消。
10. 在没有进行截图时按 `Alt + H` 打开截图历史。历史项会显示缩略图、保存时间、尺寸和 OCR 摘要；可以复制、重新钉图、收藏或删除。点击“标签”可输入以逗号分隔的自定义分类（例如 `代码, 报错`），它们会显示在卡片中并参与搜索。底栏会显示实际 PNG 占用与条目数；缩略图右上角可多选，工具栏可全选当前筛选结果、批量收藏、批量取消收藏、批量删除，或将选中项目导出为原始 PNG 与 `metadata.csv`。导出会自动创建在 Windows“下载/SnapRust Exports/SnapRust-时间戳/”目录中，其中包含 PNG 原图和可由 Excel 打开的 UTF-8 BOM `metadata.csv`；删除会要求确认，并永久移除本地 PNG 与数据库记录。

截图会先覆盖整个 Windows 虚拟桌面，并在原生窗口保持隐藏时加载、解码截图；WebView 合成器以透明内容完成预热后，截图和遮罩才一次性出现。因此不会把遮罩本身截入图片，也不会在等待 PNG 时显示黑色占位帧。前端始终把逻辑视口坐标按截图的实际物理像素尺寸换算；例如 125% 缩放时，逻辑 `(400, 300)` 对应物理 `(500, 375)`。多显示器下，虚拟桌面的物理原点可以为负数，屏幕轮廓会以虚线显示在遮罩中。

截图 HUD 与标注底栏会显示当前链路的关键性能数据。开发模式下，浏览器控制台还会输出以 `[SnapRust performance]` 开头的完整分段记录；PNG 编码通过 Tauri 内置异步运行时的后台阻塞任务执行，不会占住窗口命令线程，项目仍不直接依赖 Tokio。

创建钉图时，Rust 将最终 `RgbaImage` 转为原生 GDI 可直接绘制的 BGRA 像素，并为每张钉图启动独立 Windows 消息线程。窗口首帧、缩放重绘、透明度、拖动和关闭全部在原生窗口中完成；创建失败会回滚状态，窗口销毁会同步清理 HWND 注册记录和 `PinStore` 图片内存。旧的 Tauri/WebView 钉图页面仅保留为非 Windows 回退实现。

OCR 使用当前 Windows 已安装的 OCR 语言包，并完全在本机处理图片。语言列表从 `OcrEngine::AvailableRecognizerLanguages` 实时读取，只展示设备真正可用的语言；若某种语言没有出现在下拉框，请在 Windows“设置 → 时间和语言 → 语言和区域”中为该语言安装 OCR 组件。识别工作复用 Tauri 内置异步运行时的后台阻塞线程，COM apartment 与后台线程保持相同生命周期，项目仍没有直接依赖 Tokio。

历史记录使用内置 SQLite 数据库和独立 PNG 文件，均位于当前用户的 SnapRust 应用数据目录下的 `history/` 子目录。只会保存复制图片或钉图成功后的 Rust 最终合成图；取消截图、仅查看标注或仅复制 OCR 文字不会产生历史记录。OCR 文本和用户维护的标签都会随记录保存，因此可以在 `Alt + H` 历史窗口中搜索。一次截图最多可保存 12 个标签，每个标签最长 48 个字符。为了避免长期使用无限增长，每次保存后会同时将历史控制在最多 500 条、PNG 合计最多 2 GiB；任一上限超出时，按时间自动清理最旧的未收藏项。收藏记录永不被自动删除，因此若收藏记录本身已经超过任一上限，程序会如实保留并在历史窗口显示实际占用。选中历史项后点击“导出”不会改变本地历史，会在当前用户的 Windows“下载”目录下创建 `SnapRust Exports/SnapRust-时间戳/`，其中包含 PNG 原图和可由 Excel 打开的 UTF-8 BOM `metadata.csv`（ID、尺寸、时间、收藏、标签与 OCR 文字）。

当前开发机已实际验收单显示器 125% DPI 场景。多显示器的 Windows API 枚举、虚拟桌面抓取、负坐标窗口定位和前端布局均已实现；仍建议在接入第二台实际显示器后完成一次跨屏人工回归。

## Release 体积与分发

当前配置的 `bundle.active` 为 `false`，因此默认只编译程序本体，不生成 NSIS/MSI 安装器。2026-08-26 在当前 Windows MSVC 工具链执行 `npm run build` 和 `cargo build --release --manifest-path src-tauri/Cargo.toml` 后：

- `src-tauri/target/release/snaprust.exe`：`11,617,280` 字节，约 `11.08 MiB`；这是可以分发的程序本体。
- `src-tauri/target/release/snaprust.pdb`：约 `6.58 MiB`；它是调试符号，普通用户无需下载。
- 前端生产资源：约 `62 KiB`，已经嵌入 EXE，不需要单独复制 `dist`。

因此目前直接发送单文件时可按约 `11 MB` 估算。OCR 使用 Windows 自带组件，没有附加模型文件；SQLite 通过 Rust 静态链接嵌入 EXE，不需要用户另行安装数据库。若后续启用不内置 WebView2 Runtime 的 NSIS/MSI，安装包通常仍会处于十几 MB 以内，但最终值必须以实际 bundler 输出为准；若选择离线内置完整 WebView2 Runtime，体积会显著增加。目标 Windows 必须已有或安装 Microsoft Edge WebView2 Runtime。

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
