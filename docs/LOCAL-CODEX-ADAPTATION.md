# LocalBridge 本地 Codex 连接适配

状态：实际验证测试-非正式发布。2026-10-03 源码及已完成验证按授权单次推送，不持续监控 Actions，不发布或合并；完整编译、安装包和实机验收尚未完成。

## 来源核验

精确提交、文件 URL 和 Git blob 标识见 `provenance/local-mcp-references.json`。源码和相关测试均经过阅读；未把阅读记录当作上游测试执行结果。

| 项目 | 固定提交 | 核验与用途 |
| --- | --- | --- |
| sparfenyuk/mcp-proxy | `153a96a61fde2bf5a23961c64a3dd96b5e385108` | 主参考，MIT，Sergey Parfenyuk。阅读 proxy_server、streamablehttp_client、mcp_server 及代理和进度测试。移植会话初始化、传输与代理分层、完整工具结果转发、关闭会话的必要逻辑。 |
| supercorp-ai/supergateway | `1a2445620428e5217ee987d8585fb6db308f2ef9` | MIT。对照 Streamable HTTP→STDIO 转发和 bridgeCancellation 测试；取消通知必须保留原请求 ID。未复制源码。 |
| punkpeye/mcp-proxy | `7fbe09d7aa697a3b314af3016eb13ff10dc50b48` | MIT。已阅读核心 proxyServer.ts，对照双向消息及会话处理。未复制源码。 |
| modelcontextprotocol/rust-sdk | `ae2f9c9b45a2c98d24ee345406e79f507c9f9282` | 阅读 STDIO、初始化、关闭及并发响应测试。该提交 LICENSE 明确处于 Apache-2.0/MIT 贡献迁移，普通文档使用 CC-BY-4.0；不能简单标注为统一 MIT。只作协议与行为参考，没有引入依赖或移植 SDK 源码。 |

主参考的完整 MIT 许可保存在 `docs/licenses/mcp-proxy-MIT.txt`，随安装包进入 `licenses/mcp-proxy-MIT.txt`；版权和修改说明同时进入源码及 THIRD_PARTY_NOTICES。没有分发第三方代理 EXE，也没有运行时桥接组件下载。

## Rust 适配范围

`local_connection/codec.rs` 提供有界帧和 STDIO 单行消息；`pipe.rs` 提供 Windows 当前用户命名管道；`runtime.rs` 移植会话级初始化、结果转发与关闭顺序；`bin/mcp-adapter/main.rs` 仅负责 STDIO 与管道转发。

上游使用 SDK 客户端会话和工具处理器；本适配使用现有 LocalBridge Rust 1.85 工具链，不增加 rmcp，也不替换执行引擎。为了保持原请求 ID、取消、JSON-RPC 错误、structuredContent、文本和输出引用，转发完整信封，不重新构建 CallToolResult 或分配新 ID。

实际路径是 Codex STDIO → 随包适配器 → 当前用户命名管道 → 桌面进程内部权限服务 → 原执行引擎。桌面进程使用现有权限服务的认证回环 HTTP 入口，内部随机凭据和 HTTP 会话头只留在桌面进程。适配器收到的命令参数只有公开的安装路径身份摘要。管道没有权限修改、目录切换或服务启动接口。

管道拒绝远程连接，ACL 限制当前用户，并核验双方进程 SID、规范化可执行路径及安装目录。连接期间固定对端进程和可执行文件，拒绝错误安装身份。帧限制 16 MiB，半帧和写入阻塞具有时限；停止时关闭活动管道和请求套接字。每条连接单独初始化原权限服务会话，取消通知走独立路径，关闭时删除该会话。失败请求不自动重放。

## 设置与配置归属

独立 schema 2 `connection-profile.json` 增加 `auto_connect_enabled`。新用户默认 local，schema 1 升级一次后改为 local；此后尊重用户模式选择及断开状态。原 settings、startup-profile 和安全凭据保留；升级前连接记录另存永久备份。未知版本或损坏的连接设置拒绝覆盖，并在主界面展示故障。

settings schema 5 为项目增加 `display_name`，前端投影为 `name`。历史项目从文件夹名补齐，保留 ID、目录身份、路径及活动项目。项目及常规设置的读改写串行化；重命名不触发服务重启，后续添加不隐式改变活动项目。显式切换先重新核验目录身份，再处理任务取消和服务切换。

local 启动与恢复分支不读取 Runtime API Key，不启动 Tunnel，不等待 Tunnel 就绪；手动停止记录仍有效。原恢复与权限控制面继续管理本地服务。

配置操作复用桌面自带官方 `codex mcp add/remove`。接入、断开与模式切换在读取连接记录前串行化。先备份，再在保留的临时 CODEX_HOME 中运行官方命令，核对其他 TOML 设置及条目归属，然后在拒绝并发写入/删除的目标文件锁下提交并读回。不会复制或编辑 auth.json。条目归属要求安装路径及安装摘要一致；同名他人条目和归属改变会拒绝操作。CODEX_HOME 改变时，先断开旧目录中的本安装条目，避免遗留无法管理的配置。

目录、模式和断开操作检查运行/排队任务；界面列出受影响任务，用户明确取消后，后端确认服务停止再继续。主界面启动不依赖旧引导、配置成功或工具调用。服务就绪后自动接入；自动操作和手动断开共享串行锁并重新检查意图。真实成功调用仍单独统计，初始化或配置写入不能代替实际调用验收。

## 验证边界

本轮摘要及历史适配矩阵见 [验证报告](LOCAL-CODEX-TEST-REPORT.md)；2026-10-03 的详细本地证据保存在 `实际验证测试-非正式发布/2026-10-03-local-projects/验收记录/`。本轮新的 Rust 源码尚未完成类型检查。官方 CLI 隔离验证不能替代完整 Rust 测试、实际适配器进程、真实 Codex 桌面调用及安装包验收。保留原 19 项门禁及适配器编译预检、哈希/安装内容检查和实际管道行为测试。
