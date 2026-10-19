# macOS 测试包验证与正式分发

Mac 原型与 Windows 正式版使用不同的应用入口、应用标识和构建流程。本文的检查只针对 `experiments/macos-spike`，不改变 Windows 运行逻辑或发布工作流。

## 先确定系统拒绝原因

系统提示无法验证开发者、App 已损坏、App 将损坏电脑时，不应统一归因为缺少签名。尤其是有害内容警告，需要保留原始系统判断，调查后再决定修复方式。

`experiments/macos-spike/tools/diagnose-macos.sh` 收集：

- 系统版本和机器架构。
- 原型程序的标识、版本、主程序 SHA-256、签名身份与完整性。
- 隔离属性的当前值、`syspolicy_check distribution` 与 `spctl --assess` 的结果和退出码。
- 最近 30 分钟明确提到 Translay 的系统安全日志，不请求管理员权限。

脚本不启动应用，不修改签名、隔离标记或安全设置，不读取模型配置或钥匙串。不存在的 App 会跳过文件检查，仍可输出系统信息与可见日志。日志为空、命令不可用或权限不足都不应解释成系统已放行。报告可能包含用户名和路径，分享前应检查。

使用方法见随包提供的 `MACOS-TESTING.txt`。如果检查结果不足以解释拦截，应继续获取相关系统诊断或按 Apple 支持渠道调查，不能通过改名、重新签名或关闭安全机制来掩盖检测结果。

## CI 能验证什么

Apple 芯片构建使用 GitHub 的 `xcode-27` 预览镜像，Intel 交叉构建使用 `macos-26`。每次记录真实系统版本，不能从标签名称推定某个补丁版本已经通过。预览镜像不可用时应保留失败状态，不静默回退并宣称已覆盖 macOS 27。

`tools/verify-package.sh` 对原始及 ZIP 解压后的 App 分别检查应用标识、CPU 架构、主程序可执行权限和严格签名完整性，并比较打包前后的文件内容。失败时不上传供试用的 App 归档；检查日志仍尽可能保留。

`tools/test-package-verifier.sh` 用实际构建包的临时副本验证拒绝错误 CPU、丢失执行权限、签名后修改文件，以及归档中被修改的 App。临时副本不会替代正常测试包。

测试附件同时提供内层 ZIP 与诊断脚本的 SHA-256、完整性验证结果和 CI 策略报告。GitHub 外层 ZIP 有自己的摘要，不能与内层 ZIP 的值混用。

以下结果必须分别记录：

| 项目 | 当前证据与限制 |
| --- | --- |
| 前端、Rust、Keychain 与构建 | 由已有自动检查覆盖 |
| App / ZIP 完整性 | 新增严格签名与解压往返检查 |
| 系统策略评估 | 记录真实结果；临时签名包的拒绝不伪装成通过 |
| Apple 公证 | 当前未接入；不能用完整性检查替代 |
| 浏览器下载后的首次启动 | 仍需在真实 Mac 上验证；CI 未复现该隔离状态 |
| 字体、窗口、菜单和权限交互 | 继续执行原型 README 的真机清单 |

## 正式签名与公证的接入条件

正式分发采用 Developer ID Application 签名、Hardened Runtime、Apple 公证和公证票据装订。需要 Apple Developer Program 团队具备相应证书及公证权限。普通 Apple ID 或临时签名不能替代这些条件；这不妨碍先完成源代码检查和开发验证。

实施顺序：

1. 查明并处理当前系统安全拦截；若发现实际有害内容，先定位来源、清理和重新验证。
2. 由账户持有人创建 Developer ID Application 证书，私钥保留在钥匙串或受保护的签名环境中。
3. 使用与临时包隔离的签名流程，显式指定 Developer ID 身份；缺少身份或公证凭据时直接失败，不回退为临时签名后继续发布。
4. 通过 Tauri 配置 `APPLE_SIGNING_IDENTITY`，启用 Hardened Runtime，并按实际功能声明所需 entitlements；不添加未用到的特权。
5. 接入 App Store Connect API 公证凭据：`APPLE_API_ISSUER`、`APPLE_API_KEY` 和 `APPLE_API_KEY_PATH`。签名证书及密钥通过受保护的环境或 GitHub Secrets 注入，不进入仓库、日志或聊天。
6. 构建并提交公证，检查 Apple 返回的状态及详细日志。只有 Accepted 才能进入分发阶段；公证失败需要按具体错误修复。
7. 将票据装订到 App，运行 `xcrun stapler validate`、严格签名检查、`syspolicy_check distribution` 和 `spctl --assess --type execute --verbose=4`。严格签名通过本身不表示策略通过。
8. 在票据装订完成后重新制作最终 ZIP，生成摘要并验证解压后内容；不要对最终 App 再修改资源或重签名。
9. 在真实 macOS 27 设备上通过浏览器下载最终包，验证首次启动与功能，再发布明确标注的 Mac 测试发行版。

证书配置和公证流程尚待具备账户条件后接入。本次不创建公开 Release，也不宣称临时包已通过系统安全检查。

## 依据

- [Apple：在 Mac 上安全地打开 App](https://support.apple.com/zh-cn/102445)
- [Apple：Signing your apps for Gatekeeper](https://developer.apple.com/developer-id/)
- [Apple：Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/)
- [Apple：Code Signing Tasks](https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/Procedures/Procedures.html)
- [Tauri：macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)
- [GitHub：Runner Images](https://github.com/actions/runner-images)
