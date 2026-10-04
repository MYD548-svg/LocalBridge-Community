# 下载来源与扩展引导实施检查报告

日期：2026-10-04（UTC）。分支：codex/chatgpt-web-integration。代码基线：b67d03e430932d870589dd9aa9e11f234b196d1d。依据：[产品发行与引导说明](PRODUCT-DISTRIBUTION.md)。最终提交号和远端核对结果在交付回复中记录，不为补写 SHA 再次推送。

## 本轮修改与检查结论

产品更新和扩展附件统一使用本仓库。主程序更新按发行清单中的数字版本比较；当前版本扩展另行校验来源提交、身份、协议和兼容范围，不用当前扩展的限制误挡未来主程序升级。没有正式 Release 时如实返回无发布状态，不回退到上游。

构建链将同一份已验证发行 ZIP 用于安装资源和独立附件。应用核对内置描述、包哈希与编译提交后准备固定目录；首次安装、重复准备、替换及较旧内置包分别处理。六步引导、扩展弹窗与安装图解同步更新，文件准备、配对、当前聊天启用和真实工具成功分别呈现。操作错误不会被成功轮询清掉，首次只读调用只填入说明，仍需用户发送和确认执行。

| 检查 | 状态 | 结果及边界 |
| --- | --- | --- |
| 桌面前端测试 | PASS | 32 项，包含接口状态、精确附件归属与向导前置条件 |
| 扩展测试 | PASS | 17 项，包含按状态提示、首次只读说明、不自动执行及保留待处理请求 |
| TypeScript / 生产构建 | PASS | npm run build；桌面及发行扩展生成成功，实际 ZIP 校验通过 |
| 扩展资源复用 | PASS | 第二次 build:extension 返回 REUSED；提交、构建输入、ZIP 和内置描述一致才复用 |
| 开发包隔离 | PASS | 实际开发构建成功；发行 ZIP 和 bundle.json 哈希未改变，开发身份与输出目录不同 |
| 跨平台 Node 检查 | PASS | 88 项，0 FAIL、0 SKIPPED；包含发行候选来源/附件、资源复用、旧 PASS 失效及原构建回归 |
| 平台无关 Rust 模块 | PASS | 21 项；原发行/更新/包管理模块通过，Clippy -D warnings 通过 |
| 许可证 | PASS | npm 166、Cargo 484；扩展 ZIP 纳入原 LICENSE，依赖锁文件不变 |
| 架构/公开导出 | PASS | schema44 残留扫描、公开导出规则检查 |
| 格式与敏感信息 | PASS | 修改的 Rust 文件格式检查、git diff --check；高置信敏感信息为零 |
| workflow | PASS（静态） | 三份 YAML 解析；主分支/普通分支触发范围分开，tag 不构建；发布仅手动、默认关闭 |
| Windows Python 包装入口 | UNVERIFIED | 完整 Node 批次为 88 PASS、1 FAIL；失败为 Linux 无法执行 Windows python.exe（EACCES）。该项仍在 Windows CI 中，未删减门禁 |
| Windows/MSVC/Tauri/原生管道 | NOT_RUN | 未在当前 Linux 工作区执行完整应用编译与原生行为测试 |
| 实际 NSIS 安装资源 | NOT_RUN | 新 ZIP/描述提取与哈希核验已接入原 Windows 门禁，本机未以旧 EXE 替代 |
| Edge/Chrome/真实 ChatGPT/干净安装 | NOT_RUN | 需实际浏览器和 Windows 环境验收，单元测试与构建不能代替 |
| 新 GitHub Actions / 正式发布 | UNVERIFIED / NOT_RUN | 本轮不查询新 Action 状态，不创建 tag 或 Release，不执行发布 workflow |

Rust 检查采用临时 Linux harness，按路径引用仓库原模块；仅临时目录前缀和 Windows 原子替换适配为 Linux 实现。它不覆盖 Windows MoveFileEx、Tauri 命令、注册表、宿主及管道行为，不能视为完整 Windows 应用测试。

本地构建包含基线 HEAD 与未提交改动，生成文件保留在忽略目录，不作为最终正式发行证据。最终云端候选必须在新提交上重新经过既有十九阶段门禁；安装 EXE、ZIP、宿主与清单要求同源。新增手动发布配置只复用成功 Community Build 的配套候选，不重编译、不覆盖旧发行。

交付边界：一次普通提交和推送，仅用 git ls-remote 核对远端 SHA，随后停止。不强推、不 rerun、不监控新 Actions、不创建 PR/tag/Release。以下保留此前 Action 修复的历史记录，其计数和当时的构建行为不代表本轮结果。

---

# 历史记录：ChatGPT 网页集成 Action 修复检查报告

日期：2026-10-04（香港时间）。分支：codex/chatgpt-web-integration。修复基线：11f80f2617094ccba490131671f983c2d744bfbc。最终上传 SHA 与远端核对结果以本次交付回复为准；不为补写 SHA 再次推送。

## 已确认的故障及修复

[CI #80 / run 37130983033](https://github.com/MYD548-svg/LocalBridge-Community/actions/runs/37130983033) 精确对应基线，十九阶段为 13 PASS、auth-repeat FAIL、5 NOT_RUN。十项 Windows 编译/lint 预检、三个原生 release 组件及 staging 完整性均通过，前端与发行扩展构建通过。十次鉴权及十次 stderr 回归全部通过。

唯一失败是 local_connection::tests::browser_authorization_actual_pipe_durable_dedup_and_revocation：夹具对工作区使用 canonicalize()，产生 Windows 身份路径 \\?\ 前缀；CodingToolsRuntime 明确拒绝该类执行路径，因此在 bundled MCP 启动前返回 InvalidConfiguration。该测试后续授权、去重和撤销断言尚未执行，完整 Rust、NSIS 与最终证据阶段也未执行。上一轮授权条件 lint 已通过，不是这次故障。

本轮夹具改用 WorkspaceValidator 的 execution_path()，把同一身份校验后的普通路径同时用于浏览器授权 stamp 和 MCP 工作区。生产代码继续拒绝 verbatim 输入、核验冻结的目录文件身份。新增 Windows 原生回归直接验证普通执行路径被接纳、同一目录的 verbatim 别名被拒绝，不启动 Python 或开放端口。

撤销测试现在先在六十秒固定期限内观察到命令的那一次真实文件写入，并确认任务仍在运行，再撤销授权、验证连接关闭及任务取消。命令只发送一次，断言和一次写入计数保留；不会用一秒 yield 响应冒充 PowerShell 已执行副作用。使用已有 windows_powershell 选择器保持测试环境明确。

## 门禁、诊断和缓存

原十九个外层阶段、十项特性编译/lint、--locked、-D warnings、完整 Cargo --no-fail-fast，以及十次鉴权/十次 stderr/本地协议重复覆盖全部保留。顶层按依赖收集独立失败，依赖未满足标 BLOCKED 并说明 blockedBy；正式 NSIS 打包要求所有前序必需门禁通过，成功候选上传仍要求总门禁成功。

每个阶段、编译检查和 auth-repeat 子命令记录实际命令、开始结束、耗时、退出码、执行 ID、stdout/stderr 和日志位置。独立执行文件保留，Tauri hook 再次运行同名资源命令时不覆盖前次日志。同步预检监督进程启动前先失效旧 PASS。启动异常、超时和清理失败落盘；超时仅清理本任务子进程及后代，不按进程名全局结束程序。行为过滤器必须实际执行测试，零测试不能 PASS。

历史成功 CI 的 Rust 全套约 17 分钟、NSIS 约 14 分钟；#80 的资源准备约 29 分钟、auth-repeat 约 17 分钟。外层普通阶段上限三十分钟，资源准备九十分钟、auth-repeat 六十分钟；特性预检单命令三十分钟，资源命令四十五分钟，行为子命令十分钟。超时不是失败重试，未执行或被阻断项目不标为通过。

两个 Windows workflow 保留 npm 缓存，增加 Cargo registry/git 及 debug/release 编译依赖缓存；键包含 OS、Rust 1.85、MSVC 目标、Cargo.lock 和构建配置。安装包、staging、扩展 ZIP、测试报告和 PASS 证据均不缓存，每次仍执行 Cargo 校验和行为测试。always() 上传全部阶段日志及 broker、adapter、host、发行/开发扩展证据。Tauri 资源重建保留，未引入未经证明的产物复用协议；最终实际 NSIS 提取与哈希校验保留。

release/no-console 测试需要 LOCALBRIDGE_RELEASE_EXE 才真正执行。当前 pre-package Rust 门禁未提供本轮发行 EXE，报告明确为 NOT_RUN；内部 SKIPPED 不视为新发行应用行为通过。真实 ChatGPT、UAC、干净安装等仍未验收。

## 当前云工作区可运行的验证

| 检查 | 状态 | 结果及边界 |
| --- | --- | --- |
| 桌面测试 | PASS | 29 项，相关代码和输入自检查以来未变化 |
| 扩展测试 | PASS | 11 项，执行确认、页面观察、草稿与未知结果恢复 |
| TypeScript / 生产构建 | PASS | npm run build；桌面及发行扩展生成成功 |
| 开发扩展构建 | PASS | --development；发行/开发实际 ZIP 哈希与各自本轮构建证据一致，身份与目录隔离 |
| 跨平台 Node 回归 | PASS | 84 项，0 FAIL、0 SKIPPED；包含新增诊断执行器及原构建/协议回归 |
| 执行器回归 | PASS | 独立多失败、依赖阻断、stdout/stderr、启动异常、超时及忽略 SIGTERM 的后代清理、零测试拒绝、旧 PASS 失效、重复执行日志保留 |
| 运行时完整性 | PASS | bundled-only；固定 Python/MCP/Tunnel 供给与摘要不变 |
| 许可证 | PASS | npm 166、Cargo 484；无新增应用运行时依赖 |
| 架构/公开导出 | PASS | schema44 残留扫描、公开导出规则测试 |
| 格式与敏感信息 | PASS | 两个修改 Rust 文件使用 1.85 rustfmt；任务文件无高风险敏感信息；git diff --check |
| workflow | PASS（静态） | YAML 解析、事件过滤、缓存范围及诊断/产物路径核对；不是云端执行结果 |
| Windows/MSVC | NOT_RUN | 当前 Linux 无完整原生 Windows 执行环境；历史基线的十项预检 PASS 不代表本轮新测试已通过 |
| 新路径/浏览器管道回归 | NOT_RUN | Rust 源码和 CI 已接入；格式与静态审查不能替代原生执行 |
| Windows 进程树超时清理 | NOT_RUN | Linux 子进程回归通过；taskkill 分支尚未在 Windows 实机执行 |
| Windows embedded Python 包装 | NOT_RUN | 本机跨平台 Node 批次不包含仅支持 Windows 的包装入口 |
| NSIS / 新 release EXE | NOT_RUN | 本机不以旧 EXE 替代；由本次 Windows Action 构建并校验 |

npm/Cargo 依赖声明和锁文件未修改。原有夹具、备份、输出和未知用户文件保留。本机 ZIP 证据记录基线 HEAD 与 dirty 工作树；本机输出不作为最终云端候选。本机日志位于 /tmp/localbridge-*.log，生成证据位于忽略目录 tests/artifacts/。

## 一次上传和验收边界

代码冻结前复核目标远端、开放 PR 与 workflow 事件。只在 codex/chatgpt-web-integration 普通推送一次，不强推、不推标签、不创建 PR 或 Release。此分支当前无开放 PR，另一分支的 PR #1 不修改；按现有事件过滤预期只触发一轮 bundled CI、一个 Windows 作业。

推送后仅以 git ls-remote 核对远端 SHA，然后停止；不查询新 Action 状态、不下载新产物、不 rerun、不补推报告。新提交的 Action 结果为 UNVERIFIED。预期成功候选仍为安装 EXE、发行扩展 ZIP、中文安装指引、校验和与 BUILD-PROVENANCE；失败保留完整诊断。

真实 Edge/Chrome/ChatGPT 调用、干净安装与升级、注册归属、UAC、断线/刷新/休眠及长时间使用：NOT_RUN。Action 构建成功不能替代这些验收。
