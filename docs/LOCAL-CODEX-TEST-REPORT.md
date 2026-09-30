# 本地 Codex 连接候选验证报告

实际验证测试-非正式发布。基线 `de756be2b57f48a908108922ea869b84a70fab87`，工作分支 `codex/local-codex`。本轮在本地收敛后执行一次普通推送；推送结果与最终 SHA 由交付消息记录。推送后不查询 Actions，不创建 PR、不合并、不发布、不重装。

## 环境与证据

Rust/Cargo 1.85.0、Node 24.16.0、Git 2.46.0、Codex 桌面自带 CLI 0.159.0。已确认缺少 MSVC `link.exe`，未发现 Visual Studio Build Tools 或 Windows SDK 的标准安装目录；没有主动安装。完整 Cargo 检查在依赖构建脚本链接阶段终止，尚未到达项目完整类型检查。

新证据保留于项目内 `实际验证测试-非正式发布/2026-09-30-local-codex-1630/`，包括独立输出、隔离 CLI 夹具、配置读回记录、环境与门禁日志、源码输入摘要。旧测试安装包和已有未跟踪文件保留，未批量清理文件。

最终可执行集合证据位于该目录的 `final-local/LOCAL-TEST-MATRIX.json`，逐项日志同目录保存。原 19 项门禁本地结果为 **11 PASS、3 BLOCKED、5 NOT_RUN**；额外敏感扫描、官方配置命令、暂存差异检查、传输模块 metadata 和适配器 metadata 最终均为 PASS。基础集合实际执行 37 项，其中 bundled Python 子套件实际执行 11 项；前端实际执行 23 项。最终没有保留已知源码或可运行测试失败作为推送条件。

`BUILD-ENVIRONMENT.json` 与 `BUILD-BLOCKERS.json` 记录环境探测及当前源码的完整 Rust 测试编译、Clippy 尝试；两者均在依赖构建脚本缺少 `link.exe` 时阻断。metadata 检查使用已有 Rust 1.85 标准库，不是完整构建替代方案。原失败及修正后的相关日志均保留，报告没有把失败尝试改成通过。

## 原 19 项共享门禁

| 项目 | 本地结果 | 范围或阻断原因 |
| --- | --- | --- |
| toolchains | PASS | 固定 Rust/Cargo、Node、Git 版本；不代表 MSVC/SDK 可用。 |
| dependencies | PASS | 在全新独立目录执行锁定依赖的离线 npm ci，避免清理现有 node_modules。 |
| tunnel-source | PASS | bundled 模式保持原固定 Tunnel 载荷；community 源码构建未执行。 |
| bundled-integrity | PASS | Python、编码工具和 Tunnel 固定载荷及元数据完整性。 |
| test-base | PASS | 37 项共享测试基础、输出引用、构建与实际 Python 回归；新增适配器暂存与失败证据测试。 |
| format | PASS | 使用匹配的 Rust 1.85 rustfmt；包含全部新 Rust 文件。 |
| public-release | PASS | 公开内容策略、发布预检及敏感内容扫描。 |
| licenses | PASS | 依赖许可证，以及主参考 MIT 版权与随包许可；比较来源精确提交已记录。 |
| schema44 | PASS | 架构扫描通过；行为断言仍由 Rust 测试负责。 |
| frontend-test | PASS | 8 个文件、23 项测试，包含本地/兼容引导、独立连接状态与明确取消参数。 |
| frontend-build | PASS | 类型检查和生产构建，输出在独立目录，关闭自动清空输出。 |
| runtime-resources | BLOCKED | 适配器/Broker 的完整编译和 Clippy 预检需要缺失的 Windows 构建环境。 |
| staged-integrity | NOT_RUN | 当前版本适配器/Broker 未能构建，未用旧二进制替代。 |
| auth-repeat | NOT_RUN | Rust 测试可执行文件未生成；原 Tunnel 十次认证断言保留。 |
| rust-test | BLOCKED | 缺少 MSVC 链接器；完整 Rust 行为测试未执行。 |
| rust-clippy | BLOCKED | 同一 Windows 构建环境阻断完整 Clippy。 |
| nsis-package | NOT_RUN | 完整当前版本构建被阻断；本轮未生成安装包。 |
| package-integrity | NOT_RUN | 没有本轮新安装包。 |
| artifacts | NOT_RUN | 没有本轮新安装包及可核验的发行二进制。 |

## 新增覆盖及实际执行边界

| 验证 | 本地结果 | 证据与限制 |
| --- | --- | --- |
| 指定参考许可证、源码、相关测试阅读 | PASS | 四项目固定提交及文件 blob；SDK 许可证迁移已记录，没有移植 SDK 代码。上游测试执行 NOT_RUN。 |
| Win32 管道与转发模块独立类型检查 | PASS | Rust 1.85 metadata 检查实际 codec/pipe/runtime 源码；凭据与安装摘要边界使用签名桩，不链接、不执行，不能替代完整应用编译。 |
| 适配器独立类型检查 | PASS | 同样只检查实际适配器源码及已检查的传输接口，没有生成可运行 EXE。 |
| bundled Python 输出回归 | PASS | 11 项：真实 Windows shell 各重复 10 次、失败输出、延迟读者、超时、终态、并发光标及句柄；句柄基线 195、结束 195。 |
| 官方配置命令隔离验证 | PASS | 桌面自带 CLI，中文/空格路径、自定义 CODEX_HOME、重复 add 10 次、get 读回、remove、其他 MCP/模型和登录夹具保留。只证明官方命令合同；用户真实配置未修改。 |
| LocalBridge 配置归属/备份行为 | NOT_RUN | Rust 测试已加入精确归属、其他设置保留、客户端缺失及只读配置保护；完整构建阻断。首次接入、冲突、断开及并发保护需要当前应用实机复核。 |
| 实际本地管道协议 | NOT_RUN | 已加入初始化、通知、并发、同 ID 跨会话、错误、半帧时限、取消、超时、权限、错误安装、停止、不重放及 retained-stderr 十次测试；需可运行当前 Rust 测试文件。 |
| 原 retained-stderr Rust 场景重复十次 | NOT_RUN | 接入共享 auth-repeat，原断言保留，当前 Rust 构建阻断。Python 输出回归不能替代这一场景。 |
| 目录/模式切换与取消任务的实机行为 | NOT_RUN | 前端调用参数与后端保护已实现；停止与任务终止的实机确认仍需完整当前应用。 |
| 真实 Codex 桌面工具调用 | NOT_RUN | 当前版本不可构建，未用旧安装包替代。 |
| 约 30 分钟持续使用 | NOT_RUN | 同上。 |
| 新安装包内容/哈希/许可证实测 | NOT_RUN | 已加入构建证据和安装内容断言；没有新安装包可验。 |
| 云端构建与 Actions | NOT_RUN | 本轮推送后立即停止，不查询云端结果。 |

## 失败修复记录

1. 首轮新增代码发现重复任务查询方法，已去重；Rust 格式和独立类型检查补查。
2. 无法完整链接时，继续执行独立门禁；没有移除测试或改低断言。
3. 转发退出补充套接字关闭、半帧期限和非阻塞管道写入；适配器区分正常 STDIN 结束与异常断开，诊断只写标准错误。
4. 配置修改补充锁定目标、隔离官方命令、前后其他设置对比和读回；CODEX_HOME 改变及另一安装的记录拒绝静默接管。
5. 官方 CLI 中文路径初测失败源于测试解析器的默认编码；显式使用 UTF-8 后原路径断言通过，没有放宽断言。
6. 新前端测试最初放在共享测试发现范围以外；移动到原测试目录后实际执行。兼容引导首屏测试依据实际欢迎页校正，原测试保留。
7. 独立前端输出环境变量最初缺少 Node 全局类型；使用受约束环境类型读取后类型检查通过。原构建会清空缓存目录，已关闭自动清空并改用独立输出；旧测试安装包未覆盖。
8. 最终 metadata 复查脚本先把 rustup 版本参数传给裸编译器，随后暴露独立解包工具缺少标准库；改为明确调用现有 Rust 1.85 的 rustup 入口后，两项原断言通过，保留失败日志。
9. 最终审查补充连接设置命令之间的互斥，接入、断开与模式切换在加载记录前序列化，避免多窗口覆盖。补跑格式与敏感扫描；其完整运行行为仍因 Windows 构建环境阻断而 NOT_RUN。

当前结果只代表已列明范围内的本地验证通过。完整 Rust 编译、行为回归、当前安装包及实机验收未完成，不能认定正式发布或完整验收通过。
