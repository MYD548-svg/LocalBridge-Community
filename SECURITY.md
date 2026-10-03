# Security Policy

## 报告漏洞

请优先使用 GitHub 仓库的 **Private vulnerability reporting / Security Advisory** 私下报告安全问题。不要先创建公开 Issue，也不要公开真实凭据、可直接利用的 PoC、完整敏感日志或用户数据。

如果仓库界面暂时没有显示私密漏洞报告入口，请只创建一个不包含漏洞细节的普通 Issue，说明“需要私下安全联系渠道”；维护者会转入私密沟通。不要把漏洞细节作为附件或评论补充到该公开 Issue。

报告中建议包含：受影响版本、最小复现条件、预期安全边界、实际越界结果以及已经完成的脱敏处理。

## 安全边界

LocalBridge 的重点安全边界包括：

- MCP public Tool Registry 与 PEP/capability policy；
- active workspace 的结构化路径授权；
- Shell、普通进程与命令生命周期；
- `elevated_exec → Privileged Broker → Windows UAC` 管理员路线；
- Runtime API Key、Tunnel 凭据与诊断脱敏；
- loopback-only 本地监听；
- Tunnel 认证与恢复；
- bundled runtime / Toolbox 的版本、来源和校验值；
- 安装目录、用户数据目录与卸载后的文件边界。

完整模式允许普通开发进程使用**当前 Windows 用户 Token**运行。LocalBridge 仍约束自己的结构化路径/workdir 输入，但不承诺对子进程提供额外的 OS 级活动工作区沙箱。获得管理员 Token 必须经过独立 Broker 和用户 UAC；整个主程序不会被整体提权。

LocalBridge control-plane 在所有权限模式下都不是 MCP 可委托能力。

ChatGPT 网页候选通过扩展 Native Messaging 和当前用户命名管道接入，不新增供网页访问的 HTTP/WebSocket 服务。正式宿主只接受固定扩展 ID；配对批准绑定界面显示的工作区、文件系统身份和权限摘要，一次性授权通过内部握手绑定 MCP 会话，忽略模型身份字段。撤销、工作区或权限变化、停止服务会使旧授权失效并取消该会话的执行。开发版使用不同公钥、ID、宿主名和独立应用构建，不能接入正式应用。

网页消息作为不可信候选，仅在扩展自身窗口确认后执行；结果回填需另一次用户操作，有草稿时拒绝覆盖，并由用户在 ChatGPT 点击发送。内容进入在线聊天。导入的 ZIP 通过文件清单、哈希、路径、权限及版本检查；这些哈希证明包内一致性，不是发布者签名。应使用与安装包来自同一提交的官方候选产物。请求日志在执行前落盘，重复返回既有状态；不确定结果不会自动重放，不承诺所有异常窗口下的 exactly-once。

## 测试与披露规则

- 只使用合成凭据复现问题。
- 不在 Issue、PR、fixture、截图、日志或诊断样本中加入真实 secret。
- 不通过放宽 fail-closed 行为来修复测试。
- 未知能力默认拒绝，直到有明确安全评审和测试。
- Privileged Broker 不得暴露未认证网络监听。
