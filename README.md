# Translay — Windows 全局划词翻译

[![Check](https://github.com/malusry/Translay/actions/workflows/check.yml/badge.svg)](https://github.com/malusry/Translay/actions/workflows/check.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Translay 是一款面向 Windows 的开源划词翻译工具。应用启动后驻留系统托盘；用户在其他应用中选中文字并按 `Ctrl+Shift+T`，Translay 会优先通过 Windows UI Automation 读取选区，必要时执行一次受控的 `Ctrl+C` 回退，然后在鼠标或选区附近显示不抢焦点的极简翻译浮层。

当前版本为 0.4.0，支持 OpenAI-compatible 本地/API 模型配置与翻译调用；暂不包含 OCR、自动选区监控、历史记录或账户系统。

## 环境要求

- Windows 10/11
- Node.js 20 或更新版本
- Rust stable 与 Cargo，MSVC target
- Microsoft C++ Build Tools / Windows SDK
- Microsoft Edge WebView2 Runtime

官方环境说明：

- [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Tauri 2 global shortcut](https://v2.tauri.app/plugin/global-shortcut/)
- [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)

## 安装与运行

普通用户可以从
[GitHub Releases](https://github.com/malusry/Translay/releases)
下载 Windows `setup.exe` 安装程序。当前 Beta 安装包尚未进行商业代码签名，
Windows 可能显示来源未知提示；请只从本仓库下载。

从源码运行时，在项目根目录执行：

```powershell
npm.cmd install
npm.cmd run app:dev
```

生成 NSIS 安装包和正式可执行文件：

```powershell
npm.cmd run app:build
.\src-tauri\target\release\translay.exe
```

安装包生成在 `src-tauri\target\release\bundle\nsis\`。如果只需要未打包的
可执行文件，可以运行 `npm.cmd run app:build:exe`。

项目只使用 `src-tauri\target` 作为 Rust 构建目录，并以
`src-tauri\target\release\translay.exe` 作为唯一成品入口。不要直接保留或运行
其他 `target-*` 目录中的程序，以免误开旧版本。

启动成功后没有普通主窗口，托盘中会出现 Translay。退出请使用设置页左下角
或托盘菜单中的“彻底退出 Translay”。

使用方式：

1. 右键 Translay 托盘图标，打开“配置”。
2. 选择口语或学术风格，并配置本地模型或 API。
3. 保存后可使用“测试连接”确认模型服务能够响应。
4. 在目标应用中选中非敏感文字，按 `Ctrl+Shift+T`。
5. 浮层先显示“翻译中”，模型返回后显示中文译文。
6. 译文按有效字符数停留：0–32 个字符约 3 秒，33–80 个字符约 5 秒，
   更长内容约 7 秒；鼠标进入浮层后暂停原计时，离开后重新停留约 3 秒。

隐藏 WebView 的事件不是唯一数据源。Rust 会在显示前把最新
`CapturePayload` 暂存在内存中；React 通过初始化、事件、
`visibilitychange`、`pageshow` 和最长 1.5 秒的短轮询主动调用
`get_latest_capture`。应用结果后前端调用 `ack_capture(requestId)`：
普通 3.33 秒阅读计时只从最终译文或错误状态的 ACK 开始；“翻译中”的
ACK 不会启动隐藏计时。未收到 ACK 时 store 不会被普通计时器
清除，只由 12 秒安全超时回收。

快捷键的唯一配置位置是 [`src-tauri/src/config.rs`](src-tauri/src/config.rs) 中的 `GLOBAL_HOTKEY`。如果 `Ctrl+Shift+T` 已被其他程序占用，应用会在启动阶段返回注册错误。

## 捕获与剪贴板规则

UIA 候选按以下范围收集并去重：焦点元素、鼠标点元素、Raw 祖先、Content/Control 祖先、前台窗口根元素，以及根元素下受候选数、访问元素数和时间限制的 Document/TextPattern 子树。每个候选先试 TextPattern2，再试 TextPattern。Chromium 多进程 provider 通过“属于前台窗口 UIA 子树”校验，不按 PID 不同直接拒绝。

只有可恢复的 UIA 失败才进入 `ClipboardCopy`。回退在专用 OLE STA 中完成：

- 保存原剪贴板序列号和 `IDataObject`，原为空时记录空状态；
- 确认触发时 HWND 仍是有效前台窗口；
- 用 `SendInput` 发送完整 Ctrl+C，并在异常路径显式释放 C/Ctrl；
- 最多约 450ms 短轮询剪贴板序列号；
- 只把有长度上限的非空 Unicode 文本作为捕获结果；
- 恢复前再次比较序列号，第三方已经修改时绝不覆盖；
- 用 `OleSetClipboard` 恢复原 `IDataObject`，随后 `OleFlushClipboard` 物化延迟格式。

这是一种“尽可能安全”的恢复，不承诺所有应用自定义/延迟渲染格式完全无损。原文不会写入日志或磁盘；日志仅包含应用名、捕获方式、文本长度、耗时和错误码。

## 多语言基础

捕获层按 Unicode 文本处理，不区分英文、日文、韩文或其他语言。成功捕获后会生成轻量 `LanguageProfile`：

- 日文、韩文和中文提供本地脚本提示；
- 拉丁、西里尔、阿拉伯和希伯来文字只提供 `und-*` 脚本提示，最终语言由翻译 Provider 判断；
- 混合语言文本会被标记，但不会拆分或丢弃；
- 阿拉伯文、希伯来文等从右向左文本通过 `dir="auto"` 和 `unicode-bidi: plaintext` 正确显示。

模型调用统一使用 `TranslationRequest`，默认 `sourceLanguage=auto`、`targetLanguage=zh-CN`，翻译风格为口语 `conversational` 或学术 `academic`。本地提示不作为跳过翻译的依据，也不会把仅含汉字的文本武断认定为无需翻译。

## 模型配置与安全

右键托盘图标选择“配置”：

- 本地模型：支持 Ollama、LM Studio 等 OpenAI-compatible 服务，填写 Base URL 和模型名；
- API：填写 HTTPS Base URL、模型名和 API Key；DeepSeek 预设会自动填写官方接口与 `deepseek-v4-flash`；
- “模型思考”是独立开关，不与口语或学术风格绑定；
- 标题下方以状态点显示实际生效的模型：连接成功为绿色、失败为红色、尚未验证为灰色；
- “测试连接”只测试当前编辑内容，不保存或切换调用来源；仅右侧主按钮会保存并切换；
- 配置窗口使用 Translay 自定义无边框界面；顶部空白区域可拖动，右上角 `×` 或 `Esc` 仅隐藏窗口，翻译器继续驻留托盘；
- 本机 `localhost` / `127.0.0.1` 允许 HTTP，远程 API 强制 HTTPS；
- API Key 仅保存到 Windows 凭据管理器，`model-config.json` 不包含密钥；
- 已保存的 API Key 只显示安全摘要；点击摘要即可原位重新填写，完整密钥不会返回 WebView；
- “测试连接”与真实翻译统一调用 Chat Completions；只填写 `http://127.0.0.1:1234` 等主机地址时会自动补全 `/v1/chat/completions`，也可直接填写带 `/v1` 的 Base URL；
- 本地 Qwen3 将开关映射为 `/think` 或 `/no_think`；DeepSeek API 映射为 `thinking.type=enabled/disabled`；其他兼容服务不会收到未知的厂商专属字段；
- 捕获原文、译文和 API Key 均不写入日志。

## 检查与构建

```powershell
npm.cmd run check
npm.cmd run app:build
```

`npm.cmd run check` 会依次执行前端类型检查、前端测试与构建，以及
Rust 格式、编译和测试检查。

若本地无法访问 crates.io，可仅为当前命令指定可信镜像，不必把镜像写进项目配置。

正式发布由 [`.github/workflows/release.yml`](.github/workflows/release.yml)
在 Windows runner 上生成 NSIS 安装包，并先创建为 GitHub 草稿 Release。

## 代码结构

```text
src/
├─ main.tsx                 # 根据窗口参数按需加载界面
├─ overlay/                 # 浮层、动效、IPC、状态同步与相关测试
├─ settings/                # 设置界面、IPC、模型配置状态与相关测试
└─ shared/                  # 前端共用数据类型
src-tauri/
├─ src/app/                 # 启动、IPC、窗口、托盘与退出协调
├─ src/*.rs                 # 系统能力、捕获与翻译服务
├─ capabilities/            # Tauri 窗口权限
└─ icons/                   # 应用图标
assets/                     # 图标等可编辑源素材
docs/archive/               # 早期技术验证记录
```

- `src-tauri/src/app/`：Tauri 组装、命令边界、窗口、托盘、模型切换和退出协调。
- `src-tauri/src/selection_service.rs`：UIA 候选发现、TextPattern2/TextPattern 和软超时隔离。
- `src-tauri/src/clipboard_service.rs`：OLE `IDataObject` 快照、Ctrl+C、序列号保护、Unicode 读取和恢复。
- `src-tauri/src/foreground_context.rs`：前台 HWND、进程、应用名和有效性检查。
- `src-tauri/src/capture_coordinator.rs`：UIA→Clipboard 流水线、请求代次和提交门。
- `src-tauri/src/latest_capture_store.rs`：可恢复的最新 payload、代次保护和隐藏后清理。
- `src-tauri/src/capture_session.rs`：请求 ID、取消令牌和仅最新请求可提交。
- `src-tauri/src/overlay_manager.rs`：浮层窗口生命周期、Win32 无激活样式和投递握手。
- `src-tauri/src/overlay_policy.rs`：浮层定位、尺寸、DPI、阅读时长和悬停策略。
- `src-tauri/src/hotkey_service.rs`：Rust 侧全局快捷键的幂等注册/注销。
- `src-tauri/src/model_config.rs`：本地/API 配置、校验和非敏感持久化。
- `src-tauri/src/credential_store.rs`：Windows 凭据管理器 API Key 存储。
- `src-tauri/src/translation_service.rs`：OpenAI-compatible 连接测试和口语/学术翻译。
- `src-tauri/src/translation.rs`：多语言脚本提示、统一翻译请求、模式与内容类型。
- `src/settings/Settings.tsx` 与 `SettingsView.tsx`：配置状态容器与纯展示层。
- `src/overlay/Overlay.tsx` 与 `OverlayView.tsx`：浮层动效协调与纯展示层，不执行系统捕获。

早期实现决策与测试证据归档在
[`docs/archive/CAPTURE_PIPELINE_02.md`](docs/archive/CAPTURE_PIPELINE_02.md)，
0.1 的历史记录保留在
[`docs/archive/TECH_SPIKE_01.md`](docs/archive/TECH_SPIKE_01.md)。

当前浮层状态、动效原则和时序 Token 见
[`docs/MOTION_LANGUAGE.md`](docs/MOTION_LANGUAGE.md)。

## 参与贡献

欢迎提交问题和改进建议。开始开发前请阅读
[`CONTRIBUTING.md`](CONTRIBUTING.md)；安全问题请按照
[`SECURITY.md`](SECURITY.md) 私下报告。

## 许可证

Translay 使用 [MIT License](LICENSE) 开源。第三方依赖与素材仍分别遵循其自身许可证。
