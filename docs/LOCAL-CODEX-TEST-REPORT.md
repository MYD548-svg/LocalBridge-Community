# 本地 Codex 连接候选验证报告

实际验证测试-非正式发布。基线 `de756be2b57f48a908108922ea869b84a70fab87`，工作分支 `codex/local-codex`。本轮在本地收敛后执行一次普通推送；推送结果与最终 SHA 由交付消息记录。推送后不查询 Actions，不创建 PR、不合并、不发布、不重装。

## 2026-10-02 管道响应关闭与测试同步修复

已核验提交 `2cb495488c681764e703960d855f78483007da5a` 的 [CI push 36973145250](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36973145250)，于香港时间 2026-10-02 **14:52** 结束：原 19 项门禁为 **13 PASS、1 FAIL、5 NOT_RUN**。六项编译/Clippy 预检、Broker/Adapter 发布构建与暂存完整性通过；原 Tunnel 认证和 retained-stderr 回归各 10 次通过。`auth-repeat` 的四项本地管道测试中，错误安装身份测试通过，其余三项分别因副作用文件等待超时、未初始化错误响应读取遇到 Win32 错误 233、stderr 内容不含标记而失败。上次 bearer 缺失已消除。后续完整 Rust 测试、独立 Clippy、NSIS、安装包完整性及最终产物五项未执行，仅有诊断产物，没有新安装包。

关闭缺陷已确认：正常工作线程返回后调用 `Pipe::close()`，其服务器路径执行 `DisconnectNamedPipe`，会丢弃未读响应。依据微软的[强制断开说明](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-disconnectnamedpipe)及[服务器关闭后的 CLOSING 状态说明](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/6b6c8b8b-c5ac-4fa5-9182-619459fce7c7)，本轮在本机重新进行有限的 Win32 对照：同一进程中的管道写入 8 字节帧后，强制断开再关闭时 Peek/Read 失败、错误 233、读到 0 字节；直接释放服务器句柄后 Peek 显示 8 字节，Read 完整读回相同 8 字节。脚本与日志保留于 `.local-tmp/pipe-close-fix-20261002/win32-close-comparison.*`。该试验只验证 Win32 关闭语义，不是完整 Rust、认证、协议或安装包验收。

本次提取工作线程处理及注销逻辑：正常返回后先移除连接登记，再释放最后的服务器句柄，不强制断开、不等待客户端读取。手动停止、身份拒绝、帧错误及写入失败保留强制关闭；原解析失败和非法协议版本的提前返回显式保留强制关闭。新增真实 Win32 管道 Rust 回归，在工作线程完成信号确认服务端句柄已释放后，客户端才读取，验证完整错误响应与原请求 ID，并检查登记已移除及 EOF。帧、JSON-RPC、认证、ACL 和双向安装身份校验没有调整。

stderr 场景保持 10 次循环，命令预算改为 120 秒、终态观察为 180 秒，明确要求 `ProcessFailed`，继续验证标记、稳定引用、终态重放及跨会话 `OutputNotFound`。管道与共享 HTTP 测试复用同一命令分类：仅已知运行/终态状态及明确可重试的传输等待超时有效，未知状态立即失败；只读输出对齐共享回归的明确可重试错误，始终使用相同会话/引用，不重提 `exec_command`。取消场景用另一会话的 `tools/list` 响应作为通知处理屏障，所有者休眠 120 秒、执行上限 180 秒。副作用场景保留 `Add-Content`，后续休眠 120 秒、执行上限 180 秒，在 60 秒内同时检查启动响应及文件完整的一行 `once`，实际错误立即失败；关闭并释放客户端句柄，等连接工作线程注销后才停止服务，最终仍断言恰好一次。专用 300 毫秒超时及 `ProcessTimedOut` 保留。诊断补充阶段、迭代、公开响应、命令/文件状态，不记录 bearer 或认证头。文件与 stderr 原失败的具体原因仍未完整定位，短预算只作为强假设，未宣称已确认另一生产缺陷。

本次实际本地证据保留于 `.local-tmp/pipe-close-fix-20261002/`：共享前 **11 项 PASS**，基础集合 **37 项 PASS**（含 bundled Python 子套件 **11 项 PASS**）、前端 **8 个文件 23 项 PASS**及生产构建通过，公开内容、许可证、架构与额外敏感扫描通过。锁定依赖在全新独立目录执行离线 `npm ci`，现有 `node_modules`、测试夹具、输出及日志全部保留。Rust 1.85 改动文件格式检查及 `git diff --check` 为 PASS。

默认、Broker、Adapter 六项完整编译/Clippy 预检均实际尝试，保持原参数矩阵的 `--locked`、默认 `--all-targets`、对应二进制 feature/目标范围及 `-D warnings`，使用独立于共享构建目录的 `.local-tmp/local-codex-target` 缓存。六项分别退出 101，均因缺少 MSVC `link.exe` 在依赖构建脚本阶段 **BLOCKED**，未到达项目完整编译或 lint。未安装工具链。新关闭回归、全部本地管道测试、Guard `[401, 401, 200]`、认证回归及完整 Rust 行为套件均 **NOT_RUN**；资源发布构建、当前版本暂存完整性及打包/安装包/产物验证未执行。原 19 项门禁本地映射为 **11 PASS、2 BLOCKED、6 NOT_RUN**，无当前可运行检查 FAIL；六项预检的环境阻断不能算通过。

静态复核确认：新增辅助方法/回归限于 Windows 测试构建，生产工作线程按原停止与资源回收逻辑注销；通知按同一连接顺序处理、取消不跨会话，输出读取仍绑定所有者。后续门禁保留 Adapter/Broker 暂存构建证据、安装包实际载荷、MIT 随包许可及逐项 SHA-256 检查，原 19 项门禁、重复次数与 Actions 配置未修改。这些审查不能替代执行结果；云端后续五项仍未验证。

本轮从 `2cb4954` 修改，仅提交运行时、共享辅助逻辑、管道回归及本报告，普通推送一次、核对远端 SHA 后停止；不查询新 Actions、不创建 PR、不合并或发布、不删除文件。**本次新提交的云端状态为 UNVERIFIED**。下文保留历史各轮记录，不作为本次新提交的通过证据。

## 2026-10-02 本地管道认证夹具修复（历史记录）

已核验提交 `b471d7ed06e9590ff5448fae5b332880e5d1d4b7` 的 [CI push 36957941525](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36957941525)，运行于香港时间 2026-10-02 11:23 结束，原 19 项门禁结果为 **13 PASS、1 FAIL、5 NOT_RUN**。六项编译/Clippy 预检、Broker 与 Adapter 发布构建、暂存完整性均通过，枚举大小修复已得到该提交云端验证。`auth-repeat` 内原 Tunnel 认证探测 10 次、retained stderr 回归 10 次通过，随后四项 `local_connection::tests` 全部在获取连接 bearer 时因 `Option::unwrap()` 遇到 `None` 失败，尚未进入实际管道行为断言。后续完整 Rust 测试、独立 Clippy、NSIS、安装包完整性和最终产物五项未执行；仅有诊断产物，没有安装包。上轮提交的云端结果由此更新为 FAIL。

共同原因已从源码确认：`PublicRuntimeFixture::start/start_in` 调用原隔离单元测试启动路径，该路径明确使用 `disabled_for_isolated_unit_test()`，因此 `local_connector_bearer()` 返回 `None`。本次增加仅 Windows 测试构建、crate 内可见的认证启动入口，使用原有 `ClientAuthenticator::generated()`，认证生成失败仍返回 `AuthenticationUnavailable`；与原测试入口共享权限、工作目录和生命周期设置。夹具新增 `start_authenticated/start_authenticated_in`，四个本地管道测试切换到新入口，五处 bearer 获取使用明确的 `expect`；原默认夹具、生产启动路径和外部接口不变。

现有 Guard 认证测试改用新夹具，保留缺失 bearer 返回 401、错误 bearer 返回 401、正确 bearer 返回 200 的全部断言。原管道测试的初始化、并发、权限、跨会话归属、取消、超时、不重放、十次输出回归、停止和错误安装身份断言全部保留，认证及 stderr 重复次数未改变，没有伪造 bearer、关闭认证、添加认证豁免或调整门禁/Actions。

本次本地证据保留于 `.local-tmp/auth-fixture-fix-20261002/`，`local-checks.json` 和 `rust-checks.json` 记录实际命令、退出码及逐项日志；该目录的前端输出与全新锁定依赖安装目录均保留。Rust 1.85 改动文件格式检查、`git diff --check` 和共享前 11 项检查为 PASS：基础集合 37 项（含 bundled Python 子套件 11 项）、前端测试 8 个文件 23 项、前端生产构建、公开内容预检、许可证及架构扫描全部通过；额外敏感扫描为 PASS。依赖检查使用全新目录中的离线 `npm ci`，未清理现有 `node_modules`。

默认、Broker、Adapter 的六项完整编译/Clippy 检查按共享预检参数矩阵分别实际尝试，保留 Rust 1.85、`--locked` 和对应的 `--all-targets`、`-D warnings`，使用独立构建缓存；均退出 101，在依赖构建脚本阶段因缺少 `link.exe` 记为 **BLOCKED**，未到达项目完整编译/Clippy 检查。对应的 `runtime-resources` 与完整 `rust-clippy` 检查为 BLOCKED；暂存完整性、认证回归/管道测试、完整 Rust 行为测试、NSIS、安装包完整性及最终产物为 NOT_RUN。本次 19 项本地映射为 **11 PASS、2 BLOCKED、6 NOT_RUN**，不代表共享门禁完整通过；未安装工具链，未以格式、静态审查、独立类型检查或历史成功结果替代当前完整验证。

已静态复核本地路由的初始化协议、请求 ID 原样转发、每连接独立 HTTP 会话、按会话取消、断连关闭/DELETE 及不重放路径、半帧/大小上限、双向用户与可执行文件身份校验；并复核后续打包的 Adapter/Broker/许可证映射、暂存构建证据、安装包载荷哈希和重复条目检查。未发现额外确定的源码阻断点，但这些审查不证明未执行的行为测试或实际安装包验证通过。

本次仅提交认证夹具、现有测试与报告修改，普通推送一次并核对远端 SHA 后停止，不创建 PR、不查询或监控新 Actions、不合并或发布，不删除文件，保留全部未跟踪内容。**本次新提交云端结果为 UNVERIFIED**；下文均为历史记录，不能作为本次新提交的完整验收结果。

## 2026-10-02 连接枚举大小修复（历史记录）

已检查提交 `61ff790a71cd2d6fbc288adaa72851a88b3b234d` 的 [CI push 36712102154](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36712102154)。运行于香港时间 2026-09-30 20:09 结束，结果为 **11 PASS、1 FAIL、7 NOT_RUN**。`runtime-resources` 内的完整测试编译预检 `test-compile` 已通过，随后 `test-clippy` 因 `driver.rs:89` 的 `clippy::large_enum_variant` 失败：`Tunnel` 载荷至少 264 字节，`Local` 载荷至少 48 字节。后续暂存完整性、认证重复测试、Rust 行为测试、独立 Clippy 门禁、NSIS、安装包完整性和最终产物阶段均未执行；仅有诊断产物，没有安装包。上次类型修复的新提交云端结果由此更新为 FAIL。

本次将 `RuntimeConnectionHandle::Tunnel` 的载荷改为 `Box<TunnelRuntime>`，同步更新正常启动、恢复启动和现有后台测试的三处构造。匹配分支通过自动解引用沿用原方法调用；取消、停止、资源释放逻辑、测试专用重导出和全部断言保留。未添加 lint 豁免，未修改外部接口、协议、配置格式或原 19 项门禁。

本次本地验证：Rust 1.85 rustfmt 改动文件检查及 `git diff --check` 为 PASS。使用独立本地构建缓存分别尝试 `cargo +1.85.0 test --manifest-path src-tauri/Cargo.toml --locked --all-targets --no-run` 和 `cargo +1.85.0 clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`；两者均退出 101，在依赖构建脚本阶段因缺少 `link.exe` 阻断，分别记为 **BLOCKED**，未到达项目完整编译或 Clippy 检查。日志保留于 `.local-tmp/local-codex-enum-fix-20261002/test-compile.log` 和同目录 `test-clippy.log`。未安装工具链，未以独立类型检查或历史云端成功结果代替本次完整验证。

本次仅提交上述修复和报告，普通推送一次并核对远端 SHA 后停止，不创建 PR、不查询新 Actions、不合并或发布，不删除文件，保留未跟踪内容。**本次新提交的云端结果为 UNVERIFIED**；下文保留此前各轮证据，不能作为本次新提交的验收结果。

## 2026-09-30 测试连接类型修复（历史记录）

已检查提交 `9512007c5213a4f8c6e3c4097f51e7b55d48fe84` 的 [CI push 36696285180](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/36696285180)。运行于香港时间 2026-09-30 17:32 结束，结果为 **11 PASS、1 FAIL、7 NOT_RUN**。基础测试 37 项、bundled Python 11 项、前端测试 23 项通过；第 12 项 `runtime-resources` 在 `test-compile` 因 `background.rs:874` 的 E0308 类型不匹配失败。后续暂存完整性、认证、Rust 测试、Clippy、NSIS、安装包完整性和最终产物阶段未执行，仅上传诊断文件，没有新安装包。

本次将该测试传入的 `TunnelRuntime` 包装为 `RuntimeConnectionHandle::Tunnel`，匹配生产驱动的新连接类型。`mcp::driver` 是私有模块，因此从 `mcp` 增加仅在 Windows 测试构建中启用、仅 crate 内可见的类型重导出，避免非测试构建出现未使用导入；对外接口和测试断言保持不变。已检索 `src-tauri` 与 `tests`，确认此测试构造函数只有这一处调用。

本次验证：Rust 1.85 rustfmt 改动文件检查及 `git diff --check` 为 PASS。原完整编译命令 `cargo +1.85.0 test --manifest-path src-tauri/Cargo.toml --locked --all-targets --no-run` 在独立本地构建缓存中尝试，退出码 101；依赖构建脚本因 `link.exe` 缺失而阻断，记为 **BLOCKED**，未到达项目完整类型检查。日志保留于 `.local-tmp/local-codex-type-fix-20260930/test-compile-final.log`。此结果不证明修复后完整 Rust 编译或行为测试通过。

本次保留原 19 项门禁与认证断言。提交并普通推送到 `codex/local-codex` 后只核对远端 SHA，随后停止，不创建 PR、不查询新 Actions、不合并或发布。**本次新提交的云端结果为 UNVERIFIED**；下文为原候选版本的本地验证记录。

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
