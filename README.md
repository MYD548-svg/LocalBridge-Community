# LocalBridge

当前源码包含 **尚未发布的 Codex 本地连接候选改造**：已安装并登录 Codex 的 Windows 用户可通过随包适配器接入 LocalBridge，连接与工具执行在本机完成，模型仍由 Codex 联网调用。保留 OpenAI Tunnel 高级兼容模式。

本轮未生成新版安装包，完整 Windows 编译与实机验收仍受构建环境阻断，不能据此认定正式发布。参见 [本地连接使用说明](docs/LOCAL-CODEX-USAGE.md)、[来源与适配说明](docs/LOCAL-CODEX-ADAPTATION.md) 和 [本地验证报告](docs/LOCAL-CODEX-TEST-REPORT.md)。下文下载流程对应已发布的 Tunnel 版本。

## 让 ChatGPT 直接参与本地开发与 Windows 维护

LocalBridge 将 ChatGPT 插件与 Windows 本地环境连接起来。无需反复上传文件或复制命令，就能让 ChatGPT 阅读和修改项目、运行开发任务，并协助完成常见的系统检查与维护工作。

已发布的 Tunnel 安装包约 **21 MB**，内置 Python、Coding Runtime、Tunnel 和常用工具，无需另外配置系统 Python、Node.js、Rust 或 Docker。候选本地连接版的安装包大小尚未验证。

---

## 一个插件，连接完整的本地工作流

- 阅读、搜索和修改项目文件
- 运行测试、构建及开发命令
- 查看 Git 状态、提交记录和代码差异
- 管理后台命令与长时间任务
- 检查 Windows 服务、日志和运行环境
- 执行常见系统诊断与管理员维护操作

无论是修复 Bug、重构项目、排查构建问题，还是检查 Windows 运行状态，都可以直接在 ChatGPT 对话中继续完成。

## 轻量安装，工具内置

LocalBridge 将运行所需的工具统一放入安装包，不依赖系统 PATH，也不会在使用过程中临时安装 Python 包。

- 内置固定版本的 Python Embedded Runtime
- 内置 Coding Runtime 和 OpenAI Tunnel 客户端
- 不需要安装 pip、venv 或 Docker
- 运行工具随 LocalBridge 版本统一更新，避免环境漂移
- 无遥测、无使用统计、无崩溃信息上传

当前提供 Windows 安装版。“自包含”表示无需额外准备开发运行环境，不代表免安装 Portable 版本。

---

## 下载与使用

当前支持 **Windows 11 x64**。

1. 前往 **[Releases](../../releases)**，下载 `LocalBridge_0.1.5_x64-setup.exe`。
2. 安装后选择本地项目并完成连接设置。
3. 根据应用引导创建 **Local Bridge** ChatGPT 插件连接。
4. 回到 ChatGPT，开始处理本地开发或系统维护任务。

## 权限与安全

LocalBridge 提供编辑、完整和管理员三种权限模式。管理员操作通过明确确认和 Windows UAC 启用；Tunnel 模式的 Runtime API Key 保存在 Windows 安全凭据中，不写入普通配置文件。本地模式不需要 Tunnel ID 或 Runtime API Key。

完整的权限边界和安全设计见 [SECURITY.md](SECURITY.md)。

---

## 从源码构建

开发者需要 Git、Node.js 24、Rust 1.85.0、MSVC 构建工具及 Windows SDK；社区源码构建还需要 Go 1.26.2。完整流程见 [社区构建说明](docs/COMMUNITY-BUILD.md)：

```powershell
node scripts/test/ci-gate.mjs
```

---

## License

LocalBridge 自有源码使用 [MIT License](LICENSE)。第三方组件保持各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
