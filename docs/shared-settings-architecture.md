# Windows / macOS 共享配置链路

本阶段把模型配置与连接测试接入 Mac 原型。共享业务集中到 `crates/translay-core`；Windows 保留现有系统适配，Mac 使用自己的应用目录和 Keychain。取词、剪贴板与翻译浮层的完整平台拆分属于下一阶段。

## 当前结构

```text
src/settings/
  Settings.tsx                两端共用的配置控制器、请求隔离与保存反馈
  SettingsView.tsx            两端共用的表单与视觉组件
  settingsIpc.ts              相同的命令和事件契约

crates/translay-core/src/
  model_config.rs            配置数据、校验、历史模型、文件存储
  settings.rs                配置保存、权限无关的凭据状态与连接快照
  credentials.rs             凭据接口、提供商作用域和掩码
  model_transport.rs         模型请求、连接测试、协议与错误处理
  reasoning.rs               原有提供商思考策略
  local_model_detection.rs   原有本地模型发现
  portals.rs                 模型服务官方链接白名单

src-tauri/src/               Windows 应用
  model_config.rs            原配置目录与旧版迁移适配
  credential_store.rs        原 Windows 凭据管理器实现
  translation_service.rs     翻译、解释与缓存，调用共享模型传输
  app/commands.rs            原命令接口、Windows 菜单刷新与窗口行为

experiments/macos-spike/
  src/App.tsx                共用 Settings 控制器，追加 Mac 权限内容
  src/previewSettings.ts     仅浏览器预览的只读适配，无磁盘或网络写入
  src-tauri/src/settings.rs  相同配置命令、原生窗口和状态同步
  src-tauri/src/credentials.rs  独立 Keychain 服务
  src-tauri/src/desktop.rs   菜单栏、窗口、配置加载与退出保护
```

## Windows 兼容边界

- 原正式版应用标识、配置文件名、配置目录、旧版配置迁移与凭据目标名保持原值。
- 配置 JSON 的字段、枚举值与默认值保持原值；不包含 API Key。
- 保留既有启动时开启划词图标、进程内开关与模型保存不覆盖开关的规则。
- 提供商配置保存不擅自切换模型来源，也不覆盖已生效的模式或思考偏好；原校验和保存顺序保留。
- Windows 凭据读取、写入、删除和旧凭据迁移仍由原生实现执行。
- 托盘单双击、取词、剪贴板、浮层、快捷键、窗口生命周期和单实例实现未在本阶段重写。
- Windows Cargo.lock 仅增加本地共享库条目，不升级既有依赖。

原配置、思考策略与本地发现测试随代码移入共享库，继续在 Windows 和 macOS 上执行；其余 Windows 回归保留。配置页清除密钥按钮会保留输入焦点，避免空输入失焦先隐藏操作，鼠标清除与原保存流程均有浏览器回归。

## Mac 配置能力

原生程序已接入配置读取、编辑、保存、来源切换、思考偏好、本地模型发现、连接测试、密钥掩码与删除，以及官方页面打开。菜单栏模式与配置页使用同一份保存状态；设置写入串行执行，退出后拒绝新的变更。菜单模式允许在尚未配置模型时选择。

配置位置由原型应用标识 `com.malusry.translay.macos-spike` 对应的 `app_config_dir` 决定。Mac 原型不迁移正式版或旧 Windows 配置，不读取正式版凭据。Keychain service 为 `com.malusry.translay.macos-spike.model-api-key`，account 使用共享的提供商作用域规则；完整密钥不进入 JSON 或事件载荷。

浏览器入口仅提供只读样式预览，不能连接模型、保存设置或调用系统授权。浏览器中的本地／API 标签仅切换样例表单；原生程序的同一标签会实际保存当前模型来源。

单击菜单栏仍只切换划词图标开关；快速连续点击逐次处理，模式由右键菜单或配置页中的明确选择改变。划词开关尚未连接自动取词，快捷键翻译和浮层仍待后续开发。

## 验证方式

- `npm run check`：原前端、共享 Rust 库、Windows Rust 编译与测试。
- `npm run macos:check`：Mac 前端、格式和无 GUI 状态测试。
- `cargo check --manifest-path experiments/macos-spike/src-tauri/Cargo.toml --locked`：原型桌面接口编译。
- `node tests/browser/brand-regression.cjs`：原 Windows 配置页字体、模式、保存、连接及多种缩放回归。
- `node experiments/macos-spike/tools/browser-regression.cjs`：浏览器只读边界及原生 IPC 模拟下的配置交互、失败重试、迟到结果隔离与密钥移除。
- Mac CI 另运行完整原型库测试，在唯一的临时 Keychain service 中使用假凭据验证创建、读取、更新与删除，结束后清理；不会访问原型或正式版的凭据服务。

共享库的集成测试使用临时目录、内存凭据和真实 loopback HTTP 服务，验证重启恢复、校验失败不写入、提供商密钥隔离、配置不含密钥、连接鉴权与 HTTP 失败恢复。

CI 编译与 Keychain 数据操作不代表真实桌面交互已通过。开发／打包身份变更后的钥匙串授权提示、WKWebView 编辑与字体、关闭重开、菜单栏点击、系统辅助功能授权及焦点仍需实际 Mac 验收，清单见原型 README。

Keychain API 依据：[security-framework passwords](https://docs.rs/security-framework/latest/security_framework/passwords/index.html)。
