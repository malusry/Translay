# Translay 0.2 选区捕获可靠性修复

> 历史归档：本文记录 0.2.0 阶段的验证环境与当时的 debug 产物，
> 不作为当前版本的运行说明。请以项目根目录 `README.md` 为准。

日期：2026-07-26  
版本：0.2.0  
平台：Windows 11 / x86_64-pc-windows-msvc

## 结论

0.2 已完成可编译、可运行的 UIA→Clipboard 捕获管线，保留了用户已有的 388×172 极简浅色浮层。16 个 Rust 单元测试、TypeScript 检查、前端构建、Rust 检查和 Tauri debug 构建均通过。新程序实际启动后保持响应，`MainWindowHandle=0`，说明没有显示普通主窗口；存活也证明启动 setup 已完成全局快捷键注册。

本轮不能宣称 Edge/X 端到端已实测通过：本机确实存在打开 X 推文的 Edge 窗口，但 Windows 自动化的应用控制授权超时，未执行选择或快捷键。Edge/X 最终走 `UIA:TextPattern2`、`UIA:TextPattern` 还是 `ClipboardCopy`，必须由用户按下文步骤实际验证。

## Edge/X 原失败原因

0.1 的 `TEXT_PATTERN_UNSUPPORTED` 是候选发现范围过窄造成的代码级失败：

1. 只从 `GetFocusedElement` 开始，失败才取 `ElementFromHandle`。
2. 只沿 `ControlViewWalker` 向上最多 8 层。
3. 没有使用鼠标点元素、Raw/Content 视图，也没有从前台窗口根有限搜索 Document/TextPattern provider。
4. 发现候选 PID 与前台窗口 PID 不同时会立即失败；Chromium 的浏览器进程、渲染器进程和可访问性 provider 可能跨进程，这个判断不成立。

因此 X 页面里真正暴露 TextPattern 的 Document/provider 不在旧候选集合中，查询完有限父链后只能返回“不支持”。Chromium 官方也说明其可访问性树由渲染器内容跨进程汇入浏览器，并且主网页可访问性可能按辅助技术检测情况启用。参考：

- [Microsoft：Obtaining UI Automation Elements](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-obtainingelements)
- [Chromium Accessibility Technical Documentation](https://www.chromium.org/developers/design-documents/accessibility/)

## UI Automation 修改

候选优先级现在是：

1. `GetFocusedElement`
2. `ElementFromPoint(GetCursorPos)`
3. 两个种子的 Raw 祖先
4. Content 祖先
5. Control 祖先
6. `ElementFromHandle` 前台窗口根
7. 根子树中的 Document 或可取得 TextPattern/TextPattern2 的元素

约束：

- 用 `IUIAutomation::CompareElements` 去重，保留最先出现的高优先级候选。
- 最多 96 个候选、128 个子树元素、约 220ms 发现预算；外层 UIA 总等待 900ms。
- 子树只从触发时的前台 HWND 根开始，不遍历桌面根。
- 焦点/鼠标候选必须能沿 Raw 树回到前台窗口根。
- 不再以 `CurrentProcessId` 不同拒绝 Chromium/Electron provider。
- 每个候选依次尝试 TextPattern2、TextPattern；TextPattern2 调用失败后仍会尝试 TextPattern。
- 捕获期间持续检查原 HWND 仍有效且仍是前台窗口。
- 焦点或鼠标命中的 `CurrentIsPassword=true` 会返回不可回退的 `PROTECTED_INPUT`。
- 文本有效但 `GetBoundingRectangles` 为空时仍成功，`selection_rect=None`，浮层改用鼠标锚点。

成功来源只使用 `UIA:TextPattern2`、`UIA:TextPattern`、`ClipboardCopy`。

## ClipboardService 设计

### 保存与恢复

剪贴板回退只在用户按全局快捷键且 UIA 返回允许回退的错误后执行，不做后台监听。

1. 专用线程调用 `OleInitialize`，形成 OLE STA。
2. 读取前后剪贴板序列号；有内容时通过 `OleGetClipboard` 保存整个 `IDataObject`，为空时保存 Empty 状态。
3. 在复制前再次验证原 HWND 有效且仍在前台。
4. 等待热键 Ctrl/Shift 松开，避免把复制变成 Ctrl+Shift+C。
5. 一次 `SendInput` 发送 Ctrl down、C down、C up、Ctrl up；部分发送或失败时额外发送 C up、Ctrl up。
6. 以约 12ms 间隔轮询，最多 450ms 等待序列号变化。
7. 在有上限的 `OpenClipboard` 重试中读取 `CF_UNICODETEXT`，拒绝空白和超过 1,000,000 字符的内容。
8. 读取后记下序列号。恢复前若序列号已经改变，返回 `CLIPBOARD_CHANGED_EXTERNALLY` 警告，不覆盖第三方新内容。
9. 未被第三方修改时，用 `OleSetClipboard` 恢复原 `IDataObject`；非空对象再调用 `OleFlushClipboard`，避免恢复内容依赖 Translay 继续持有对象。原为空时恢复 Empty 状态。

Microsoft 对 OLE 剪贴板的说明：

- [OleGetClipboard](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-olegetclipboard)
- [OleSetClipboard](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-olesetclipboard)
- [OleFlushClipboard](https://learn.microsoft.com/en-us/windows/win32/api/ole2/nf-ole2-oleflushclipboard)
- [GetClipboardSequenceNumber](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber)
- [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)

### 恢复保证边界

`IDataObject` 比只保存 `CF_UNICODETEXT` 更能保留 HTML、RTF、图片、文件列表和自定义格式，但仍不承诺完全无损：

- 源应用的延迟渲染 provider 可能在恢复/flush 前失效。
- 某些自定义格式依赖源进程、进程内对象或私有协议。
- `OleFlushClipboard` 可能无法物化所有格式。
- 跨完整性级别目标可能通过 UIPI 拒绝 `SendInput`。
- 在捕获期间第三方修改剪贴板时，正确选择是保留第三方新内容，而不是恢复旧内容。

## 捕获管线与竞态

统一的 `CapturedSelection` 包含文本、来源、可空矩形、前台上下文、总耗时、剪贴板恢复状态和内部警告。

每次快捷键都会创建新 request ID，并取消旧令牌；浮层是否已经可见不再阻止新捕获。UIA 只在符合白名单的可恢复失败后进入 ClipboardService。剪贴板操作由进程内互斥锁串行化。提交前在协调锁内再次检查：

- request ID 仍为最新；
- 取消令牌未取消；
- 原 HWND 仍有效且仍为前台。

前台切换会直接丢弃结果，不在新应用上显示旧文本。旧自动隐藏计时器只隐藏同一 request ID 的浮层，不会隐藏更新后的结果。没有注册全局 Esc。

## 极简浮层与焦点

前端继续只显示小型 Translay 标识、正文和弱化耗时/状态；应用名、PID、坐标、方法和错误码不进入卡片主体。错误码只出现在 HTML `title` 和不含正文的结构化日志。

Windows 侧继续组合：

- `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`
- 移除 `WS_EX_APPWINDOW`
- `HWND_TOPMOST`
- `SetWindowPos(... SWP_NOACTIVATE | SWP_SHOWWINDOW)`
- Tauri `focus:false`、`skipTaskbar:true`、`decorations:false`

显示前后比较前台 HWND；若显示后焦点不再是原窗口，立即隐藏浮层并记录 `FOCUS_CHANGED_AFTER_OVERLAY`。

## 隐藏 WebView 数据同步修复

Edge/X 已出现过 Rust 日志显示 `UIA:TextPattern`、非零
`text_length`，但卡片仍停留在 React 的 waiting 状态。根因是旧顺序
在隐藏 WebView 可见之前发送一次性 `capture-result`；监听器尚未挂载时
事件不会重放。

修复后：

1. `LatestCaptureStore` 在内存中保存与最新 request ID 匹配的完整
   `CapturePayload`，旧代次不能覆盖新代次。
2. `OverlayManager.show` 先写 store，再定位并显示原生窗口；使用
   `IsWindowVisible` 的短周期有界确认代替固定长等待，确认可见后才 emit。
3. Tauri command `get_latest_capture` 只返回仍属于最新代次的 payload。
4. React 先 `await listen`，再 invoke command；事件和补取结果经过同一
   `applyPayload`，只接受不旧于当前 request ID 的数据。
5. 自动隐藏只清理与该浮层 request ID 相同的 store 内容，旧计时器不能
   清除新结果。前端不切回 waiting，避免下一次显示闪烁。

正文只存在于 Rust 内存 store 和当前 WebView state，不写入日志或磁盘。

### WebView2 暂停后的 ACK 交付协议

仅在 React 挂载后补取一次仍不足以覆盖 WebView2 暂停：原生窗口可见不
等于 JavaScript 已恢复，显示后的事件仍可能全部丢失。当前协议进一步改为：

- 前端统一的 `syncLatestCapture` 先 invoke `get_latest_capture`，通过
  request ID guard 应用后再 invoke `ack_capture(requestId)`。
- 同步在监听器注册后、每次 `capture-result`、页面恢复为 visible、
  `pageshow` 时触发；同时以 75ms 间隔做最长 1.5 秒的有界 getter 轮询。
- Rust 在 0/50/150/300ms 有限发送 `capture-result`。收到当前 request
  ACK 或新请求开始后立即停止旧重发。
- `ack_capture` 只有在 latest、shown 和 payload 的 request ID 全部一致时
  接受；旧 ACK 和重复 ACK 不会创建新计时器。
- 正常 5 秒阅读计时从首个有效 ACK 开始，不再从原生窗口显示开始。
- 永远没有 ACK 时，12 秒安全超时隐藏并清理，记录不含正文的
  `OVERLAY_DELIVERY_TIMEOUT`。

## 实际自动检查结果

| 检查 | 结果 |
|---|---|
| `npm.cmd run typecheck` | 通过 |
| `npm.cmd run build` | 通过，Vite 7.3.6，32 modules |
| `cargo fmt --all -- --check` | 通过 |
| `cargo check` | 通过，无 warning |
| `cargo test` | 通过，16 passed，0 failed |
| `npx.cmd tauri build --debug --no-bundle` | 通过 |
| debug 程序启动 | 通过；进程 Responding=True |
| 后台无普通主窗口 | 通过；`MainWindowHandle=0` |
| debug 产物 | `src-tauri/target/debug/translay.exe` |

自动测试覆盖：

- UIA 候选优先级和逻辑去重
- TextPattern2→TextPattern 尝试顺序
- 有文本、无矩形仍为成功
- 只有合格 UIA 失败才允许剪贴板回退
- 新请求取消旧请求
- 前台切换阻止提交
- 剪贴板序列号不变时超时
- 第三方序列号变化时不恢复
- Ctrl/C 错误路径释放
- 快捷键重复注册/注销幂等状态
- 屏幕四角、负坐标显示器、位置优先级和 96/144/192 DPI

## 人工兼容性与剪贴板格式记录

以下只记录实际执行状态，没有把“代码存在”当作“真实应用已通过”。

| 应用/场景 | 状态 | 控件类型 | TextPattern | 选区矩形 | 最终方法 | 备注 |
|---|---|---|---|---|---|---|
| Edge / X 英文推文 | 待用户测试 | 未取得 | 未取得 | 未取得 | 未确定 | 已发现打开的真实 X 页面；应用控制授权超时，未执行选择/热键 |
| Edge 普通网页正文 | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | 无实测数据 |
| Edge 地址栏 | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | 无实测数据 |
| Windows 记事本 | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | 无 0.2 端到端数据 |
| VS Code 编辑器 | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | 无实测数据 |
| VS Code 终端 | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | 终端常需要 ClipboardCopy |
| 普通文本 PDF | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | provider 不支持时可回退 ClipboardCopy；本阶段无 OCR |
| 一个聊天软件 | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | 无实测数据 |
| 原生菜单/设置文字 | 待用户测试 | 待测 | 待测 | 待测 | 未确定 | 不可选择文字不在本阶段处理范围 |
| 连续快速触发 | 自动代次测试通过；真实 UI 待测 | — | — | — | — | 旧请求逻辑上不能提交 |

剪贴板格式：

| 原剪贴板内容 | 实际状态 |
|---|---|
| 普通 Unicode 文本 | 待用户端到端测试 |
| HTML / RTF | 待用户端到端测试 |
| 图片 | 待用户端到端测试 |
| 文件列表 | 待用户端到端测试 |
| 应用自定义格式 | 未验证，不保证无损 |
| 捕获期间第三方复制 | 序列号保护单元测试通过；真实并发操作待测 |
| 原剪贴板为空 | Empty 分支已实现；真实操作待测 |

### 首要人工验收

1. 保持当前 debug 程序运行，或执行 `.\src-tauri\target\debug\translay.exe`。
2. 在 Edge 的 X 页面选中一段非敏感英文推文，按 `Ctrl+Shift+T` 后松开。
3. 确认浮层显示完全相同的选中文字，位置在选区或鼠标附近。
4. 用日志中的 `capture_method` 确认实际为 `UIA:TextPattern2`、`UIA:TextPattern` 或 `ClipboardCopy`；日志不会包含正文。
5. 立即继续在 Edge 输入/滚动，确认 Edge 仍有焦点。
6. 在触发前分别复制普通文本、富文本、图片和文件；触发后粘贴到合适应用，确认原内容仍可用。
7. 捕获期间快速在另一应用复制新内容，确认新内容没有被旧剪贴板覆盖。
8. 对 Edge 普通正文/地址栏、记事本、VS Code 编辑器/终端和文本 PDF 重复。
9. 快速选择两段不同文字并连续触发，确认不会出现旧结果覆盖新结果。

## 已知限制与下一步

- UIA 与 OLE provider 调用仍是软超时；进程内阻塞线程无法安全强杀。下一阶段可把两者移入最小权限 helper 进程，提供硬超时。
- UIA 有候选数/元素数/时间上限，超大或异常树可能在找到 provider 前达到上限。
- `CurrentIsPassword` 依赖 provider 正确标记；跨权限应用和自绘控件仍可能不完整。
- ClipboardCopy 会短暂改变系统剪贴板，虽然有序列号保护和恢复，仍需完成格式矩阵实测。
- 不支持 OCR，因此不可选择、无复制语义的画布、图片和菜单文字不会被捕获。
- 下一步应先补齐上表的真实兼容性数据，再决定是否需要 helper 进程、更多剪贴板格式验证或 OCR。
