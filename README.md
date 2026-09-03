# SnapRust

SnapRust 是一款面向 Windows 的轻量级截图工具，使用 Rust、Tauri 2 和原生 Win32 API 构建。它把截图、标注、裁剪、本地 OCR、LLM 翻译、钉图与历史管理整合在一个工作流中。

```text
Ctrl + Shift + A
        ↓
框选屏幕区域
        ↓
标注 / 长截图 / 裁剪 / OCR / 翻译 / 添加窗口边框
        ↓
复制图片 / 钉到桌面 / 保存到历史
```

> 当前版本为开发版本，主要支持 Windows 10/11。项目尚未提供正式安装器或自动更新服务。

## 功能概览

- 全局快捷键截图，应用隐藏时也可唤起。
- 支持完整 Windows 虚拟桌面、负坐标显示器和混合 DPI 布局。
- 支持手动滚轮、稳定帧检测和纵向拼接的长截图。
- 提供箭头、矩形、椭圆、画笔、马赛克和多行文字标注。
- 支持图片裁剪、左右旋转、编辑器缩放，以及 macOS、Windows 11、拍立得边框。
- 使用 Windows 内置 OCR，本地识别图片文字，无需下载 OCR 模型。
- 支持 DeepSeek、OpenAI、OpenAI-compatible 服务和本机 Ollama 翻译。
- 原生 Windows 钉图窗口，支持拖动、缩放、透明度、旋转和右键操作。
- 使用 SQLite 和 PNG 文件保存本地截图历史，支持搜索、标签、收藏、批量操作和导出。
- 常驻系统托盘，可快速开始截图、打开历史或配置翻译。
- 最终图片由 Rust 后端重新合成，复制、钉图和历史记录保持一致。

## 系统要求

运行 SnapRust：

- Windows 10 或 Windows 11
- Microsoft Edge WebView2 Runtime

从源码开发还需要：

- Node.js 和 npm
- Rust stable MSVC 工具链
- Microsoft C++ Build Tools
- Windows SDK

## 快速开始

安装依赖并启动开发版本：

```powershell
npm install
npm run tauri dev
```

SnapRust 启动后默认隐藏并常驻系统托盘。按 `Ctrl + Shift + A` 开始截图，或点击托盘图标选择对应操作。

### 构建发布版本

```powershell
npm run build
cargo build --release --manifest-path src-tauri/Cargo.toml
```

生成的程序位于：

```text
src-tauri/target/release/snaprust.exe
```

当前 `bundle.active` 为 `false`，因此默认只生成可执行文件，不生成 MSI 或 NSIS 安装包。前端资源会嵌入程序，无需单独分发 `dist`。发布体积会随编译器和依赖变化；当前本地 release 构建约为 8.6 MiB。

## 使用方法

### 1. 截图与框选

按 `Ctrl + Shift + A` 后，SnapRust 会先抓取虚拟桌面，再显示截图遮罩，因此不会把遮罩本身截入图片。

拖动鼠标选择区域，松开后进入图片编辑器。截图阶段按 `Esc` 或鼠标右键可以取消。

需要截取网页、文档或聊天记录时，先框选其中稳定的可滚动内容区域，再在编辑器点击 `长截图`。SnapRust 会隐藏编辑器并激活选区下方的窗口；接下来由你手动向下滚动，程序在每次滚动稳定后自动采集并通过相邻画面的重叠部分完成拼接。按 `Enter` 完成，按 `Esc` 取消；完成后会回到编辑器，长图仍可继续标注、裁剪、OCR、翻译、复制或钉图。

### 2. 编辑图片

编辑器支持：

- 箭头、矩形和椭圆
- 自由画笔
- 马赛克
- 多行文字
- 裁剪
- 手动滚轮长截图
- 左右旋转 90°
- 图片缩放与适应窗口
- macOS、Windows 11 和拍立得边框
- 撤销、重做和清空标注

边框选择器提供“无边框”“macOS”“Windows 11”和“拍立得”四种选项。窗口边框会紧贴图片；所有预设均使用完全不透明输出，不添加外部透明阴影，避免图片发送后出现异常杂边。

在图片上点击右键，可以直接执行复制、钉图、裁剪、OCR、翻译、旋转或销毁当前截图等操作。

### 3. OCR 与翻译

点击 `OCR` 后，SnapRust 使用 Windows 内置 OCR 识别原始选区。识别前会自动压平透明像素、放大尺寸较小或较窄的文字区域、轻度锐化，并按图片边缘颜色补充留白，以改善小字和贴边文字的识别率。识别结果可以直接修改、复制，并可通过识别行定位图片中的对应区域。

OCR 语言可以跟随 Windows 用户语言，也可以手动选择已安装的语言包。已知截图语言时，手动选择对应语言通常比自动模式更准确。

点击 `翻译` 时，如果当前截图尚未执行 OCR，程序会先识别文字，再把 OCR 编辑框中的文本发送给已配置的 LLM 服务。翻译失败不会影响图片复制、钉图或本地 OCR。

### 4. 复制与钉图

按 `Ctrl + C` 或 `Enter` 将最终图片复制到 Windows 剪贴板。复制和钉图成功后，图片会自动进入截图历史。

钉图使用独立的原生置顶窗口：

- 拖动图片：移动钉图
- 滚轮：以鼠标位置为中心缩放
- `Shift + 滚轮`：调整透明度
- `[` / `]`：调整透明度
- `0`：恢复初始大小、位置和透明度
- 双击或 `Esc`：销毁钉图
- 右键：复制、旋转、重置视图或销毁

### 5. 截图历史

按 `Alt + H` 或通过托盘打开截图历史。历史页面支持：

- 按 OCR 文字或标签搜索
- 收藏与取消收藏
- 自定义标签
- 复制历史图片或重新钉图
- 多选和批量收藏
- 批量删除
- 导出原始 PNG 和 `metadata.csv`

导出目录为：

```text
Windows 下载目录/SnapRust Exports/SnapRust-时间戳/
```

历史默认最多保留 500 条、PNG 合计最多 2 GiB。超过任一限制时，程序会自动清理最旧的未收藏记录；收藏项不会被自动删除。

## 快捷键

### 全局快捷键

| 快捷键 | 操作 |
| --- | --- |
| `Ctrl + Shift + A` | 开始截图 |
| `Alt + H` | 打开截图历史 |

### 截图编辑器

| 快捷键 | 操作 |
| --- | --- |
| `Ctrl + C` / `Enter` | 复制最终图片 |
| `Esc` | 取消当前操作或退出截图 |
| `Ctrl + Z` | 撤销标注 |
| `Ctrl + Y` | 重做标注 |
| `Ctrl + +` | 放大图片 |
| `Ctrl + -` | 缩小图片 |
| `Ctrl + 0` | 恢复适应窗口 |
| `Ctrl + Enter` | 确认文字标注 |
| 裁剪时 `Enter` | 应用裁剪 |

## 翻译配置

通过系统托盘的“翻译设置”即可直接配置服务。设置页面支持提供商、API Key、模型和 API 端点。

| 提供商 | 默认端点 | 默认模型 | API Key |
| --- | --- | --- | --- |
| DeepSeek | `https://api.deepseek.com` | `deepseek-v4-flash` | 必需 |
| OpenAI | `https://api.openai.com/v1` | `gpt-4o-mini` | 必需 |
| OpenAI-compatible | `http://127.0.0.1:3000/v1` | `custom-model` | 可选 |
| Ollama | `http://127.0.0.1:11434/v1` | `llama3.2` | 不需要 |

端点应填写 API 根地址，不要附加 `/chat/completions`；SnapRust 会自动请求该路径。OpenAI-compatible 和 Ollama 服务必须提供兼容的 Chat Completions 接口。

也可以在首次运行前通过环境变量提供初始配置：

```powershell
$env:SNAPRUSTRANSLATOR_PROVIDER = "deepseek"
$env:SNAPRUSTRANSLATOR_API_KEY = "你的 API Key"
$env:SNAPRUSTRANSLATOR_MODEL = "deepseek-v4-flash"
$env:SNAPRUSTRANSLATOR_ENDPOINT = "https://api.deepseek.com"
npm run tauri dev
```

支持的提供商值为：

```text
deepseek
openai
openai-compatible
ollama
```

使用 DeepSeek 时，也兼容 `DEEPSEEK_API_KEY` 环境变量。环境变量只用于配置文件尚不存在时的初始值；在 UI 中保存设置后，应用会优先读取本地配置文件。

## 数据与隐私

以下数据保存在当前 Windows 用户的 SnapRust 应用数据目录：

```text
snaprust-translation.json       翻译提供商、端点和模型（不含 API Key）
history/                       截图历史
  snaprust-history.sqlite3     元数据、OCR 文字、标签和收藏状态
  images/                      最终 PNG
  thumbnails/                  历史缩略图
```

需要注意：

- OCR 完全在本机执行，不会上传图片。
- 只有主动点击“翻译”时，当前 OCR 文本才会发送到所配置的 LLM 服务。
- 翻译提供商、端点和模型保存在本地 JSON 文件中；API Key 单独保存在当前 Windows 用户的凭据管理器中。旧版本 JSON 中的明文密钥会在启动时自动迁移并从配置文件移除。
- 截图图片不会发送给翻译服务；发送的是 OCR 编辑框中的文字。
- 取消截图、只执行 OCR 或只复制文字不会创建历史图片。
- 删除历史记录会同时删除对应的本地 PNG 和数据库记录。

## 项目结构

```text
snaprust/
├─ index.html                  截图、编辑、设置和历史界面
├─ src/
│  ├─ main.ts                 前端状态与交互
│  ├─ screenshot.ts           Tauri 命令封装
│  ├─ image-processing.ts     前端图像预览处理
│  └─ style.css               主界面样式
├─ src-tauri/
│  ├─ src/
│  │  ├─ screenshot/          虚拟桌面抓取、坐标映射和最终合成
│  │  ├─ annotation.rs        标注栅格化
│  │  ├─ clipboard/           Windows 图片剪贴板
│  │  ├─ ocr.rs               Windows OCR
│  │  ├─ translation.rs       多提供商 LLM 翻译
│  │  ├─ history.rs           SQLite 历史与导出
│  │  ├─ pin.rs               钉图状态与创建入口
│  │  ├─ pin/native.rs        原生 Windows 钉图窗口
│  │  ├─ tray.rs              系统托盘
│  │  └─ hotkey/              全局快捷键
│  └─ tauri.conf.json         Tauri 配置
└─ docs/
   └─ DEVELOPMENT_PLAN.md     设计记录与开发计划
```

核心原则是让 Rust 保存截图会话并负责最终输出，TypeScript 负责交互和实时预览：

```text
Win32 抓屏 → Rust 截图会话 → Tauri 二进制 IPC → Canvas 交互预览
                         ↓
             Rust 最终合成与 PNG/剪贴板输出
                         ↓
               钉图 / 历史记录 / 导出
```

## 开发与质量检查

前端检查：

```powershell
npm run check
npm run build
```

Rust 检查：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

部分测试依赖交互式 Windows 桌面或已安装的 OCR 语言，因此默认标记为忽略。可显式执行真实屏幕抓取测试：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml screenshot::tests::captures_virtual_desktop_with_expected_dimensions -- --ignored --exact
```

## 已知限制

- 当前仅支持 Windows，其他平台只保留有限的 Tauri 回退代码。
- 截图基于 Windows 桌面抓取。普通窗口和无边框窗口通常可以正常截图；独占全屏游戏、DRM 视频、受保护内容、系统安全桌面或反作弊限制场景可能出现黑屏。游戏中建议使用“无边框窗口”模式。
- 长截图适用于内容位置稳定的纵向滚动区域，最多拼接 24 段、16,000 像素高且不超过 1,600 万像素。视频、动画、吸顶/悬浮元素、横向滚动、需要管理员权限的窗口或不响应 Windows 滚轮消息的软件可能无法可靠拼接。
- OCR 可用语言取决于 Windows 已安装的 OCR 语言包。
- LLM 翻译需要网络服务或本机 Ollama，并受对应模型、额度和服务可用性影响。
- 全局快捷键可能与其他软件冲突，目前尚未提供 UI 自定义快捷键。
- 项目尚未启用安装包、代码签名和自动更新。

更详细的设计演进和后续计划见 [开发计划](docs/DEVELOPMENT_PLAN.md)。
