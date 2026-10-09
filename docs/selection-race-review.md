# 连续划词时序检查 · 2026-09-09

本轮原计划在独立桌面窗口进行快速换选区、翻译中重选和关闭后重新划词。Computer Use 的 `node_repl` 两次初始化均失败，重置后仍返回 `windows sandbox failed: helper_unknown_error: setup refresh had errors`。未成功执行桌面输入，原生 UIA/剪贴板端到端验收仍未完成。

完成的替代检查使用真实 Overlay 组件和可控 Tauri IPC 返回顺序，没有调用模型，也没有读取用户工作内容。它用于复现具体的前端竞态，不能代替原生桌面验证。

## 复现与修复

1. **同一请求的加载状态晚于最终结果到达。** 新请求编号能隔离旧选区，但同一编号的异步读取可能倒序完成。修复前，最终译文和附注会被早先的 translating 快照覆盖。现在按等待、捕获、翻译、终态的顺序接受更新，拒绝阶段倒退；同阶段的焦点更新和本地 IPC 错误后的恢复仍允许。
2. **旧关闭回复清空新选区的解释。** 手动关闭的原生调用返回前，用户可能已打开新选区及其解释。修复前，旧调用成功会无条件执行解释重置。现在成功、拒绝和异常返回都先确认请求仍属于当前选区，旧回复不再改变新选区。

两处均先用回归脚本观察到断言失败，再修复并重跑。改动位于 `src/overlay/Overlay.tsx` 和 `src/overlay/overlayState.ts`；没有改动生成提示词、附注样式或后端捕获代码。

## 验证结果

- 75 项前端测试和 TypeScript 检查通过。
- `tests/browser/selection-race-regression.cjs` 在旧关闭回复成功、返回 false、抛出异常三种情况下均通过：旧选区读取隔离、同请求旧阶段隔离、翻译中关闭后重新打开、保留新解释、无旧附注残留以及正确复制新译文。
- 既有 `tests/browser/reading-regression.cjs` 通过，覆盖公式、解释取消、缓存、附注、复制和窄窗口显示。
- 通过 `npm run app:build:exe` 构建新版可执行文件。

运行竞态检查需要 Node 能解析 Playwright，并安装 Edge：

```powershell
foreach ($outcome in @('true', 'false', 'reject')) {
  $env:QA_DISMISS_RESULT = $outcome
  node tests/browser/selection-race-regression.cjs
  if ($LASTEXITCODE -ne 0) { throw 'Regression failed' }
}
Remove-Item Env:QA_DISMISS_RESULT
```

测试窗口和 Vite 服务由脚本的 finally 块关闭，没有保留临时截图或测试文档。保留回归脚本与这份说明，便于桌面入口恢复后继续原生验收。

## 稳定性收尾复查 · 2026-09-20

### 修复：迟到的通信错误清空有效译文

通过真实 Overlay 组件复现：先挂起一次 get_latest_capture，随后交付有效译文，再让旧读取抛出异常。修复前，reportDeliveryError 会无条件将当前内容改为 requestId=0 的通信错误提示，译文和附注被清空。新增回归断言首先失败，确认问题存在。

修复后，界面一旦接受过有效请求，读取或确认交付的通信错误只记录诊断，不覆盖已接收的翻译状态；尚未收到任何请求时仍保留原有连接失败提示。模型翻译失败继续走原来的失败状态及重试入口。

### 检查范围

- 连续划词、同请求晚到加载状态、关闭中重新划词；旧关闭回复成功、false、异常三种结果。
- 已交付译文后旧 IPC 读取失败；新选区复制反馈不受旧复制成功/失败影响。
- 悬停、选文拖动、自动消失和重新进入浮层。
- 新增重试回归：等待重试回复时交付新选区，再返回旧重试成功、false、异常。
- 快速结果、慢响应提示、计时器重置与加载窗口移动后的动画起点。
- 真实 Windows WebView 四角展开/收起，边界、按钮、滚动位置。

浏览器脚本使用真实组件与模拟 IPC，未调用模型。前台切换仅覆盖现有 Rust 提交条件测试，未宣称完成跨应用划词的系统输入端到端验收。原生四角报告对应主屏 150% 缩放，不能外推多屏及所有 DPI。

前端 92 项测试、TypeScript 检查通过；Rust 179 项通过、1 项忽略；debug 内嵌前端构建与四角原生验收通过。验收 WebView 临时缓存确认清理。此次只生成验收用 debug 程序，未替换用户正式安装版本，正式打包留到版本路径核对后统一进行。

## 正式构建与入口核对 · 2026-09-21

已用 `npm run app:build:exe` 成功更新唯一正式入口 `D:\CodexProjects\Translay\src-tauri\target\release\translay.exe`。没有新增备用 exe 或 NSIS 安装包；当前未发现旧安装包。构建时间与 SHA256 保存在 `tests/artifacts/release-build.json`。

通过真实用户环境只读核对：构建前没有运行中的 Translay，桌面/开始菜单快捷方式和注册表 Run 项中没有找到指向 Translay 的入口。此范围不包括手工放置在任意其他目录的副本或所有第三方启动管理器。

随后启动上述正式文件，等待 6 秒后进程仍运行，实际 ExecutablePath 与正式路径完全一致，保留运行供用户使用。本项只验证进程启动和运行路径，不代表在线模型、视觉动画或跨应用划词端到端验收。
