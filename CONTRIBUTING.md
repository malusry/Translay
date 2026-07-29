# 参与贡献

感谢你愿意帮助改进 Translay。

## 开始开发

Translay 目前以 Windows 10/11 为主要运行环境。请先安装 Node.js 20+、
Rust stable、Microsoft C++ Build Tools / Windows SDK 和 WebView2 Runtime。

```powershell
npm.cmd install
npm.cmd run app:dev
```

## 提交改动

1. 先创建一个范围清晰的分支。
2. 保持改动聚焦，并为行为变化补充测试。
3. 提交前运行完整检查：

```powershell
npm.cmd run check
```

4. 在 Pull Request 中说明改动内容、原因、用户影响和验证方式。

请勿提交 API Key、个人数据、构建产物或第三方受限素材。安全漏洞不要通过公开
Issue 披露，请遵循 [`SECURITY.md`](SECURITY.md)。
