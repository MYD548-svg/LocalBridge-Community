# LocalBridge Community

本仓库为 `MYD548-svg/LocalBridge-Community`，产品更新、安装包与扩展下载统一使用本仓库的 **[Releases](https://github.com/MYD548-svg/LocalBridge-Community/releases)**。上游来源与第三方组件说明见 [来源与同步说明](docs/UPSTREAM-SYNC.md) 和 [许可证](THIRD_PARTY_NOTICES.md)。

**当前发行状态：本仓库尚未发布正式 Release。** 源码包含桌面本地连接和 ChatGPT 网页集成候选。Actions 成功后输出同次构建的 Windows 安装包、扩展 ZIP、安装图解、校验和与来源记录；候选不等于正式发行，也不等于已完成干净 Windows 安装或真实 ChatGPT 验收。

## 安装与 ChatGPT 网页接入

当前目标为 Windows 11 x64、Edge/Chrome（Chromium 120+）。安装包携带 Python、工具运行时、浏览器宿主及配套扩展；普通用户无需安装 Node.js、Rust、MSVC 或额外代理。

1. 使用本项目同次构建的 Windows 安装包。正式发行可从本仓库 Releases 获取；发布前候选在对应提交的 Actions 产物中。
2. 打开 LocalBridge，选择允许使用的项目文件夹，并启动本地服务，使用本地连接模式。
3. 打开“设置 → ChatGPT 网页”，选择 Edge 或 Chrome，点击“准备配套扩展（推荐）”。文件已准备后，还需浏览器手动加载。
4. 在地址栏打开 `edge://extensions` 或 `chrome://extensions`，开启开发者模式，点击加载解压目录的按钮，选择应用提供的固定目录。
5. 打开或刷新 ChatGPT，固定并点击 LocalBridge 扩展图标，选择“连接这台电脑”。回到应用核对项目与权限，批准配对，再返回扩展启用当前聊天。
6. 点击扩展中的“准备首次只读调用”，检查填入内容并手动发送；核对助手请求后确认执行，看到真实结果再回填并发送。

网页接入无需 API Key、Tunnel ID 或 Codex 桌面程序。详细操作与故障处理见 [中文安装图解](extensions/chatgpt-web/INSTALL.html) 和 [发行与下载说明](docs/PRODUCT-DISTRIBUTION.md)。

## 单独导入与更新

新安装包已携带配套扩展，通常不必单独下载。需要手动导入时，请选择 `LocalBridge-ChatGPT-Web-v<版本>.zip`，不要选择 Source code ZIP 或 Actions 附件的外层 ZIP。扩展下载入口按应用版本、来源提交、协议和附件清单核验配套关系，没有可用发行时会明确说明。

更新扩展前，先在所有扩展窗口停止并断开，在浏览器管理页关闭 LocalBridge，再到应用确认替换文件。完成后重新启用/重新加载扩展、刷新 ChatGPT，并重新启用当前聊天。固定目录 `%LOCALAPPDATA%\LocalBridge\browser-extension\current` 请保留。

## 其他连接方式

- **Codex 桌面接入**：需安装并登录 Codex，在应用的“Codex 桌面连接”中设置；参见 [本地连接说明](docs/LOCAL-CODEX-USAGE.md)。
- **OpenAI Tunnel 兼容连接**：作为单独的高级模式使用，需要该模式的连接配置；它不属于网页扩展的首次安装流程。

本地工具用于读取、搜索和修改项目、执行开发命令、查看 Git 状态及 Windows 维护。权限范围由应用中的项目和编辑/完整/管理员模式决定；管理员操作需要明确确认与 UAC。网页工具执行和结果发送分别由用户确认。回填并发送的内容会进入在线聊天，输入框有草稿时暂停回填，未知结果不会自动重发。详见 [安全与权限说明](SECURITY.md)。

## 构建、验证与正式发行

开发者需要 Node.js 24、Rust 1.85.0、MSVC 与 Windows SDK；社区源码构建另需 Go 1.26.2。锁文件与校验保留：

```powershell
node scripts/test/ci-gate.mjs
```

安装包与扩展必须来自同次验证。正式发布使用成功 Community Build 的现有产物，不再次执行 Windows 编译；工作流不会随普通推送或标签自动发布。详见 [社区构建说明](docs/COMMUNITY-BUILD.md)、[配套发行说明](docs/PRODUCT-DISTRIBUTION.md) 和 [网页检查报告](docs/CHATGPT-WEB-TEST-REPORT.md)。

## License

自有源码采用 [MIT License](LICENSE)。上游与第三方组件保留原版权和许可证，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
