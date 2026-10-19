# Translay Mac 开发原型

同一 Translay 仓库内的独立验证入口，使用根目录锁定的前端依赖，Rust 依赖由本目录的 `src-tauri/Cargo.lock` 单独锁定。

当前实现：菜单栏单击／右键菜单、日常／学习模式状态、原生配置窗口、辅助功能信任检查与用户主动请求授权。配置窗口直接复用正式版的 `src/settings/SettingsView.tsx`、`settings.css`、品牌图片和本地字体，保留翻译／模型导航、模式卡片、思考开关，以及本地／API 模型表单的视觉设计。

原生程序已接通共享配置控制器和 Rust 后端：模型表单可编辑、保存、切换来源和测试连接；支持本地模型发现、按提供商保存 Keychain 密钥、掩码展示与删除。配置与模式会保存到原型应用目录，划词图标开关在每次启动时恢复开启，与现有 Windows 规则一致。菜单栏与配置页保持状态同步，退出后拒绝新的设置变更。

划词开关尚未启动自动取词；快捷键取词、翻译浮层和正式应用的完整移植仍是后续工作。共享模块与 Windows 兼容边界见 [`docs/shared-settings-architecture.md`](../../docs/shared-settings-architecture.md)。

应用标识为 `com.malusry.translay.macos-spike`，名称为 `Translay Mac Preview`，与正式版分开。原型读写自己的开发配置目录，密钥保存在独立 Keychain service；不读写正式版模型配置和凭据。

## 共享样式边界

正式版的 `SettingsView` 默认继续绘制 Windows 的窗口控制、拖动区域和可用配置项。原型显式选择原生标题栏，并在翻译页追加 Mac 权限区域；原生程序使用相同的 Settings 控制器与 IPC 契约，浏览器仅使用只读预览适配。`src/style.css` 仅放原型提示、权限区域、小窗口滚动及 Mac 字体回退，通用表单、模式卡片、按钮、配色和字体资源均从正式版引用，避免维护两套设计。

Mac 默认内容区为 680 × 640，以容纳追加的权限区域；最小 640 × 480 时内容区可滚动。系统标题栏位于 WebView 内容区之外。WKWebView 的真实排版、字体和原生输入行为仍需真机验证。

## 在 Windows 上开发和检查

在仓库根目录安装依赖并运行：

```powershell
npm.cmd ci
npm.cmd run macos:preview
```

预览地址为 `http://127.0.0.1:1422`。浏览器中显示的是布局样例，辅助功能状态为「需要 Mac 验证」，系统授权按钮不可用。这里的显示不能作为 macOS 菜单、窗口或权限验收证据。

```powershell
npm.cmd run macos:check
npm.cmd test
cargo check --manifest-path experiments/macos-spike/src-tauri/Cargo.toml --locked
```

`macos:check` 构建原型前端、检查 Rust 格式并运行无 GUI 依赖的状态测试。`npm test` 同时覆盖原程序与原型的前端测试。Windows 的完整 Rust 检查验证公共 Tauri API 的类型和配置；Mac 专属代码由 macOS 构建验证。原型原生程序在非 Mac 系统上会明确拒绝启动。

可选浏览器回归：在已有 Playwright 和 Microsoft Edge 的开发环境中运行 `node experiments/macos-spike/tools/browser-regression.cjs`。它检查翻译／本地模型／API 表单、品牌字体实际渲染、只读边界、原生窗口控制的前端分离、小窗口滚动、浏览器预览的权限边界、旧快照隔离，以及模拟 IPC 下的授权／撤销刷新、原生配置编辑、保存失败重试、迟到连接隔离与密钥移除。截图与字体证据保存在忽略的 `tests/artifacts/macos-spike/`；不把模拟结果记为 Mac 原生验收。

## 在 Mac 上运行

安装 Tauri 的 macOS 开发依赖、Rust 和 Node.js 22，克隆同一仓库及开发分支，然后在仓库根目录运行：

```sh
npm ci
npm run macos:check
npm run macos:dev
```

`macos:dev` 使用根目录的 Tauri CLI，在本原型目录启动原生程序。配置窗口使用系统标题栏及原生窗口按钮；关闭窗口后程序留在菜单栏，右键「配置…」可重新打开。应用菜单提供编辑操作、Command-W 关闭窗口、Command-Q 退出，以及 Command-, 打开配置。

构建 Apple Silicon 测试包：

```sh
npm run macos:build -- --target aarch64-apple-darwin --bundles app --ci
```

构建 Intel 测试包前先安装目标，再构建：

```sh
rustup target add x86_64-apple-darwin
npm run macos:build -- --target x86_64-apple-darwin --bundles app --ci
```

产物在本目录 `src-tauri/target/<target>/release/bundle/macos/`。没有指定 target 时，路径中不含 `<target>`。

预览包使用 ad-hoc 签名，尚未获得 Developer ID 签名及 Apple 公证；从网络下载后，macOS 可能要求在「隐私与安全性」中确认打开。发布用签名与公证属于后续阶段。辅助功能授权绑定实际程序身份，开发启动与打包程序需要分别核实授权。

## GitHub 构建

`.github/workflows/macos-spike.yml` 在 `codex/macos-*` 分支的相关代码 push、相关 PR 或手动触发时执行。Windows job 检查原型前端、状态逻辑与公共 Rust 编译，macOS job 分别运行共享库、原型库及临时 Keychain 数据操作测试，构建 Apple Silicon 和 Intel 包。

在 GitHub 的 Actions → macOS Prototype → 对应运行 → Artifacts 下载测试产物。归档前使用 `ditto` 保存 `.app` 的目录和可执行权限，产物保留 14 天。该工作流上传测试产物，不创建或发布正式 Release。

CI 的编译结果与真实交互验收分别记录。以下清单需要可交互的 Mac 环境执行。

## 真机验收清单

- [ ] 记录 commit、macOS 版本、机器架构、开发／打包启动方式。
- [ ] 单击菜单栏图标一次只切换一次开关；图案及菜单勾选同步；模式保持原值。
- [ ] 快速连续点击两次回到原开关值；重复多次没有延迟回跳。
- [ ] 右键及触控板次要点按仅打开菜单；Control-点按也进入菜单路径。Control-点按的 AppKit 事件映射尚未验证。
- [ ] 右键选择日常／学习模式后仅更新模式；再次选择同一模式保持原值；配置窗口显示同步。
- [ ] 关闭、最小化、菜单重开配置窗口正常；关闭配置后菜单栏仍工作；Command-W 与 Command-Q 行为正确。
- [ ] 原生窗口按钮、窗口拖动和小窗口滚动正常，浏览器样例不作为这些项目的证据。
- [ ] 翻译／模型页面保留现有字体、品牌渐变、模式卡片与按钮效果；原生配置项可编辑、保存、测试连接，浏览器配置项、密钥输入、测试与保存不可用且有明确提示。
- [ ] 设置本地模型与 API 配置、保存并重启后恢复；切换来源、单击开关与模式互不覆盖；保存失败仍保留表单草稿。
- [ ] 使用测试密钥验证 Keychain 保存、提供商隔离、掩码和删除；应用身份变化后的授权提示单独记录，完整密钥不出现在配置文件。
- [ ] 连接成功、错误鉴权、超时、修改配置后迟到的结果及失败重试均与当前表单对应。
- [ ] 未授权时显示「未授权」；点击「请求授权」由系统提示；打开系统设置、授予或撤销权限后返回窗口能刷新真实状态。
- [ ] 退出后没有迟到点击、菜单或窗口回调重新改变状态／显示窗口。

后续取词与浮层场景见仓库的 `docs/macos-port-plan.md`。当前清单尚未标记为真机通过。
