# Translay 技术验证 01

> 历史归档：本文记录 0.1.0 阶段的验证环境与当时的 debug 产物，
> 不作为当前版本的运行说明。请以项目根目录 `README.md` 为准。

日期：2026-07-26  
版本：0.1.0  
平台：Windows 11 / x86_64-pc-windows-msvc

## 结论

原型代码、自动测试、debug 构建和 Tauri 开发模式启动已经完成。实际启动时应用成功常驻、注册 `Ctrl+Shift+T`，且 `MainWindowHandle=0`，没有普通可见主窗口。

“UI Automation 获取真实第三方应用选区，并显示浮层后保持原应用焦点”的端到端人工步骤本次没有完成：准备自动操作记事本时检测到用户正在窗口中输入，自动化按照安全规则停止。因此本技术验证已经证明各组成部分可编译并能进入运行态，但还不能诚实地宣称端到端可行性已经由本机实测最终证明。

## 已实现内容

- Tauri 2.11.5、React 19.2.8、TypeScript 5.9.3、Rust 2024 edition。
- 启动时只创建一个隐藏的 `overlay` WebView 窗口并创建系统托盘。
- Rust 侧注册 `Ctrl+Shift+T`；重复注册/注销路径幂等。
- 触发时立即记录原前台 HWND、PID 和可执行文件名。
- UI Automation 在独立 MTA 工作线程执行：
  - `GetFocusedElement`，失败时回退到前台 HWND 对应元素；
  - 检查 `CurrentIsPassword`；
  - 从焦点元素向父级最多遍历 8 层；
  - 优先 `UIA_TextPattern2Id / IUIAutomationTextPattern2`；
  - 再尝试 `UIA_TextPatternId / IUIAutomationTextPattern`；
  - `GetSelection`、`GetText`、`GetBoundingRectangles`；
  - 合并多段选区和多个矩形。
- 900ms UIA 等待上限；新请求取消旧请求，旧结果不能提交。
- 浮层预创建并隐藏，前端只渲染数据。
- Win32 浮层控制：
  - `WS_EX_NOACTIVATE`；
  - `WS_EX_TOOLWINDOW`，移除 `WS_EX_APPWINDOW`；
  - `HWND_TOPMOST`；
  - `SetWindowPos(... SWP_NOACTIVATE | SWP_SHOWWINDOW)`；
  - 隐藏时也使用 `SWP_NOACTIVATE`。
- UIA 物理屏幕坐标、目标显示器 `rcWork`、有效 DPI 和逻辑尺寸缩放。
- 浮层位置优先顺序：选区下方、上方、右方、左方，最后夹取到工作区。
- 显示后等待 25ms，再比较 `GetForegroundWindow` 与原 HWND；结果显示在浮层中并写入不含原文的结构化日志。
- 5 秒自动隐藏；再次按快捷键立即隐藏。

## 关键技术决策

### UIA 与超时隔离

UI Automation TextPattern 的调用是跨进程调用，目标 provider 卡住时可能阻塞。每次捕获使用独立 COM MTA 工作线程，协调线程只等待 900ms。超时后该工作线程可能仍在系统调用中，但 Tauri 事件循环和后续捕获不会等待它；请求代次也阻止迟到结果回写。

更强的下一阶段方案是把 UIA 放进独立 helper 进程，超时后可以终止整个 helper，从而提供硬取消。

### TextPattern2 优先

焦点元素先请求 `UIA_TextPattern2Id`。`IUIAutomationTextPattern2` 继承 TextPattern，因此使用继承的 `GetSelection`。如果 provider 不提供 TextPattern2，再请求 TextPattern。部分应用把模式挂在 Document/Edit 的父级，因此实现了有限父级遍历。

### 不激活浮层

Tauri 配置中的 `focus: false`、`alwaysOnTop`、`skipTaskbar` 只作为第一层配置。Windows 侧仍显式写入 `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`，移除 `WS_EX_APPWINDOW`，显示时使用 `SWP_NOACTIVATE`。这符合 Microsoft 对 [WS_EX_NOACTIVATE](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles) 和 [SetWindowPos/SWP_NOACTIVATE](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos) 的定义。

### DPI 与多显示器

UIA 的边界矩形按物理屏幕像素处理。用 `MonitorFromRect` 选择与选区最近的显示器，用 `GetMonitorInfoW.rcWork` 限制位置，用目标显示器有效 DPI 缩放浮层逻辑尺寸。位置算法支持负坐标显示器。

### 竞态处理

`CaptureSession` 为每次触发分配递增 ID并取消上一个令牌。结果提交前检查“当前 ID + 未取消”。最终检查与 `OverlayManager.show` 还共享协调锁，关闭“检查通过后、新请求到达前旧浮层仍显示”的 TOCTOU 时间窗。

## 官方资料

- [Tauri 2 Global Shortcut](https://v2.tauri.app/plugin/global-shortcut/)
- [Tauri 2 System Tray](https://v2.tauri.app/learn/system-tray/)
- [Microsoft: UI Automation TextPattern overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-ui-automation-textpattern-overview)
- [Microsoft: Text and TextRange control patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-about-text-and-textrange-patterns)
- [Microsoft: Extended window styles](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles)
- [Microsoft: SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos)

## 实际环境

| 项目 | 本机结果 |
|---|---|
| Node.js | 24.16.0 |
| npm | 可用；PowerShell 策略阻止 `npm.ps1`，使用 `npm.cmd` |
| rustc / Cargo | 1.95.0 stable，`x86_64-pc-windows-msvc` |
| 独立 `cl.exe` | 当前 shell 的 PATH 中未发现 |
| MSVC 链接环境 | Rust/Tauri debug 构建成功 |
| WebView2 | 148.0.3967.54 |
| 工作区初始状态 | 空目录，非 Git 仓库 |

## 实际检查结果

| 检查 | 结果 | 备注 |
|---|---|---|
| `npm.cmd run typecheck` | 通过 | TypeScript 无错误 |
| `npm.cmd run build` | 通过 | Vite 7.3.6，32 modules |
| `cargo check` | 通过 | 无 warning |
| `cargo test` | 通过 | 7 passed，0 failed |
| 四角位置测试 | 通过 | 四个屏幕角均不越出工作区 |
| 负坐标显示器测试 | 通过 | 覆盖左侧/上侧副屏 |
| DPI 尺寸测试 | 通过 | 96/144/192 DPI |
| 快捷键重复状态测试 | 通过 | 重复注册/注销逻辑幂等 |
| 快速请求代次测试 | 通过 | 旧请求被取消且不能成为 current |
| `tauri build --debug --no-bundle` | 通过 | 生成 `src-tauri/target/debug/translay.exe` |
| Tauri dev 启动 | 通过 | Vite 1420、Rust dev build、托盘启动日志、全局快捷键注册成功 |
| 后台无普通主窗口 | 通过 | 运行中 `MainWindowHandle=0` |
| 真正选区捕获 | 未完成 | GUI 测试因检测到用户输入而停止 |
| 显示浮层后的真实焦点保持 | 未完成 | 已实现运行时比较，但未完成第三方应用端到端触发 |

Cargo 官方索引在本机多次超时，依赖获取和本地检查使用了一次性 rsproxy source replacement；项目最终不固化镜像配置，`Cargo.lock` 仍是标准 crates.io source。

## 自动测试覆盖

- `CaptureSession`：新请求取消旧请求；显式取消使当前请求失效。
- `HotkeyService` 状态机：连续 register/register/unregister/unregister 不崩溃。
- `OverlayManager`：
  - 四角不出界；
  - 负坐标显示器；
  - 优先选区下方；
  - DPI 逻辑尺寸缩放。

全局快捷键的真实系统注册已在 Tauri dev 启动中成功，但“重复真实 OS 注册”没有在测试进程里抢占第二个系统热键；幂等服务会在第二次调用前短路。

## UI Automation 人工兼容性记录

下表只记录实际状态。除“记事本窗口已启动、测试因用户输入中止”外，没有推断任何应用的支持情况。

| 应用 | 测试状态 | 控件类型 | TextPattern / TextPattern2 | 选区矩形 | 失败原因或错误码 | 下一阶段剪贴板/OCR |
|---|---|---|---|---|---|---|
| Edge 或 Chrome | 未测试 | 待测 | 待测 | 待测 | 无实测数据 | 待实测决定 |
| VS Code | 未测试 | 待测 | 待测 | 待测 | 无实测数据 | 待实测决定 |
| Windows 记事本 | 未完成 | 待测 | 待测 | 待测 | 自动化检测到用户正在输入，主动停止；不是 UIA 失败 | 待重新实测决定 |
| PDF 阅读器 | 未测试 | 待测 | 待测 | 待测 | 无实测数据 | PDF provider 不足时评估剪贴板/OCR |
| 一个聊天软件 | 未测试 | 待测 | 待测 | 待测 | 无实测数据 | 待实测决定 |
| 软件原生菜单或设置页面文字 | 未测试 | 待测 | 待测 | 待测 | 无实测数据 | 不可选择文字可能需要 OCR |

### 人工记录方法

每个应用重复以下步骤并把结果补入表格：

1. 运行 `npm.cmd run tauri dev`。
2. 在目标应用选中一段不敏感的测试文本。
3. 按 `Ctrl+Shift+T`。
4. 记录浮层的应用名、捕获方式、耗时、矩形、焦点结果和错误码。
5. 若失败，用 Accessibility Insights for Windows 检查焦点元素控件类型、`IsPassword`、TextPattern/TextPattern2 支持情况。
6. 分别测试普通文本、屏幕四角、125%/150% 缩放和副显示器。
7. 连续快速按快捷键，确认旧内容不会在新内容之后出现。
8. 在密码框中触发，必须看到 `PROTECTED_INPUT` 且没有原文。

## 已知限制

- 900ms 是软超时；被卡住的 UIA worker 无法从同一进程安全强杀，极端 provider 可能留下阻塞线程。
- 某些 Chromium/Electron/PDF/自绘控件不暴露 TextPattern，或只返回空选区/空矩形。
- 应用名当前来自可执行文件 stem；UWP、多进程宿主或 ApplicationFrameHost 的名称可能不够友好。
- 只读取焦点元素及 8 层父级，不做全窗口后代搜索，避免昂贵遍历。
- provider 如果错误地不标记密码控件，客户端无法完全识别其保护语义。
- 浮层是只读技术信息窗口；`WS_EX_NOACTIVATE` 意味着它不适合作为需要键盘交互的普通 UI。
- 未实现剪贴板、OCR 或跨权限级别（例如管理员目标、普通权限 Translay）的降级方案。

## 下一阶段建议

1. 先完成表格中的真实应用矩阵和 DPI/多屏焦点测试。
2. 将 UIA 移到最小权限 helper 进程，提供硬超时、崩溃隔离和并发上限。
3. 如果下一阶段需要记录控件类型，先扩展并审查隐私约束；当前日志严格限于应用名、捕获方式、文本长度、耗时和错误码。
4. 只有兼容性数据证明必要时，才按明确优先级加入剪贴板回退；OCR 应作为更后置且需用户知情的路径。
5. 增加快捷键配置 UI 前，仍保持 `config.rs` 单一配置源和冲突错误。
