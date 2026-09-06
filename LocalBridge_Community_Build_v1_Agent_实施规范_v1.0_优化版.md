# LocalBridge Community Build v1 — Agent 实施规范

> 本文由《LocalBridge Community Build 实施方案》和《LocalBridge 自构建 Tunnel Client 实施方案》合并整理而成。
>
> 定位：**Community Build 是总体路线；Source-built Tunnel Client 是第一阶段的详细施工方案。**
>
> 第一版目标不是重写 LocalBridge，而是在尽量保持上游功能和接口兼容的前提下，把关键构建链、运行时来源、哈希、测试和发布过程变成可追踪、可复核、可重复执行的 Community Build。

---

# 1. 项目目标

维护一个独立的 LocalBridge Community Build，使最终安装包尽可能不依赖 LocalBridge 作者提供的预编译关键二进制。

第一版必须建立如下可信链：

```text
LocalBridge upstream source
        │
        ├── 固定版本 / commit
        │
        ▼
Community Fork
        │
        ├── LocalBridge App      ← 源码构建
        ├── Privileged Broker    ← 源码构建
        └── OpenAI tunnel-client ← OpenAI 官方源码构建
        │
        ▼
Runtime 来源锁定 / SHA256 验证
        │
        ▼
Tests
        │
        ▼
Provenance / Checksums
        │
        ▼
Community Installer
```

第一版完成后，用户必须能够明确知道：

- LocalBridge 基于哪个上游版本和 commit。
- Community Build 自己对应哪个 commit。
- Tunnel Client 来自哪里、哪个版本、哪个 commit。
- Tunnel Client 是否由 Community Build 自行编译。
- Privileged Broker 是否由当前源码自行编译。
- 所有关键 executable 的 SHA256。
- 哪些 Runtime 是源码构建。
- 哪些 Runtime 仍使用第三方官方预编译版本。
- 使用了什么工具链。
- 哪些测试已经通过。
- 最终 Installer 对应什么源码和构建记录。

---

# 2. 第一版的信任模型

Community Build 的目标不是：

> “绝对不存在任何预编译文件。”

而是：

> “不存在来源未知、版本未知、哈希未知、无法解释的预编译文件。”

允许保留第三方官方预编译 Runtime，例如 Python Embedded Runtime，但必须满足：

```text
官方来源
+
固定版本
+
固定下载来源
+
固定 SHA256
+
许可证明确
+
Provenance 明确
+
build_type 明确
```

每个 Runtime 必须属于以下之一：

```text
source-built
```

或：

```text
upstream-binary
```

禁止：

```text
unknown
```

---

# 3. 第一版明确不做的事情

第一版必须严格控制范围。

禁止：

1. 重写 OpenAI Tunnel wire protocol。
2. 自己重新实现 Tunnel Client。
3. 修改 MCP 协议。
4. 重写 LocalBridge UI。
5. 重写 Broker/UAC 架构。
6. 修改 workspace 权限模型。
7. 将 Elevated 默认开启。
8. 删除 LocalBridge 原有 checksum 验证。
9. 删除 MCP Guard Bearer authentication。
10. 将 API Key 或 Bearer Token 放入 argv。
11. 将 credential 写入配置文件。
12. 增加 `0.0.0.0` MCP listener。
13. 引入 Cloudflare companion。
14. 无必要升级 Tunnel Client。
15. 第一版实施 OS Sandbox。
16. 一次性加入大量 Community-only 功能。

第一版原则：

```text
90% upstream
10% community build infrastructure
```

---

# 4. 版本基线

## 4.1 LocalBridge

第一阶段基线：

```text
LocalBridge 0.1.5
```

Community Build 版本：

```text
0.1.5-community.1
```

如果后续只修改 Community Build 基础设施：

```text
0.1.5-community.2
0.1.5-community.3
```

上游升级：

```text
0.1.6
```

则重新开始：

```text
0.1.6-community.1
```

禁止直接在未来版本 `main` 上修改后仍声称产物基于 `0.1.5`。

正式构建前必须记录真实的：

```text
LocalBridge upstream commit
Community commit
```

---

## 4.2 OpenAI Tunnel Client

第一阶段固定：

```text
source:
https://github.com/openai/tunnel-client

version:
0.0.11

commit:
8d55683eeef80bc5e360d95abf4692454fafc615
```

第一版不得主动升级 Tunnel Client。

目标是最大程度保持：

```text
LocalBridge adapter
        ↕
tunnel-client CLI
```

兼容。

---

# 5. 推荐仓库结构

建议建立独立 Fork：

```text
LocalBridge-Community/
│
├── src/
├── src-tauri/
├── runtime/
│
├── scripts/
│   ├── build-tunnel-client.ps1
│   ├── verify-runtime.ps1
│   ├── build-community.ps1
│   └── generate-checksums.ps1
│
├── provenance/
│   ├── tunnel-client.json
│   ├── broker.json
│   ├── toolchain.json
│   └── runtime-lock.json
│
├── artifacts/
│
├── docs/
│   ├── COMMUNITY-BUILD.md
│   ├── SECURITY-MODEL.md
│   └── UPSTREAM-SYNC.md
│
├── .github/
│   └── workflows/
│       └── community-build.yml
│
├── runtime-manifest.toml
├── COMMUNITY-VERSION
├── THIRD-PARTY-NOTICES.md
└── SHA256SUMS.txt
```

原则：

- 尽量不改变上游已有目录。
- Community 新增内容集中放在 `scripts/`、`provenance/`、`docs/`、`.github/`。
- 降低未来同步上游时的冲突。

---

# 6. 分支与上游同步策略

Remote：

```text
origin   → Community Fork
upstream → zephyr7030/LocalBridge
```

添加上游：

```powershell
git remote add upstream https://github.com/zephyr7030/LocalBridge.git
```

建议分支：

```text
main
upstream-sync/*
security/*
feature/*
release/*
```

升级流程：

```text
发现 upstream 新版本
        ↓
建立 upstream-sync 分支
        ↓
查看 upstream diff
        ↓
安全审查
        ↓
同步代码
        ↓
重新构建关键 Runtime
        ↓
完整测试
        ↓
生成新的 community.x
```

禁止自动将 upstream `main` 直接合并进 Stable Release。

---

# 7. Agent 执行总流程

Community Build v1 按以下 Phase 执行。

```text
Phase 0  固定基线
Phase 1  自构建 OpenAI Tunnel Client
Phase 2  自构建 LocalBridge App + Privileged Broker
Phase 3  Runtime Supply Chain Lock
Phase 4  Unified Build Script
Phase 5  Integration / Security / E2E Tests
Phase 6  Community Installer + Release Artifacts
Phase 7  CI / SBOM / Reproducibility 增强
```

规则：

> 上一个 Phase 未 PASS，不得进入下一个 Release Phase。

---

# 8. Phase 0 — 固定构建基线

## 8.1 输入

- LocalBridge 目标版本。
- LocalBridge 对应真实 Git commit。
- OpenAI Tunnel Client 目标 commit。
- Windows 11 x64 构建环境。

## 8.2 工具链

第一阶段至少需要：

```text
Git
Node.js
npm
Rust / Cargo
Go
Windows SDK
Tauri 所需工具链
```

其中：

- OpenAI Tunnel Client 的目标 Go 工具链按目标 Tunnel Client commit 的 `go.mod` 确认；当前本文固定基线为 `Go 1.26.2`。
- LocalBridge 所需 Node.js / npm / Rust / Cargo / Windows SDK / Tauri 版本或版本范围，必须从**当前固定 LocalBridge commit** 的 `package.json`、lockfile、`Cargo.toml`、可能存在的 `rust-toolchain*`、CI workflow、构建脚本或官方开发文档中确认。
- 不得因为本文中的历史示例而覆盖当前固定 commit 的真实约束。
- 如果上游只给出版本范围，Community Build 应记录“上游要求范围 + 本次实际使用版本”；如果上游明确锁定版本，则按锁定版本执行。

执行：

```powershell
git --version
node --version
npm --version
rustc --version
cargo --version
go version
```

并根据当前 commit 补充实际使用的 Windows SDK / Tauri CLI 版本信息。

记录到：

```text
provenance/toolchain.json
```

至少包含：

```json
{
  "git": "...",
  "node_required": "...",
  "node_actual": "...",
  "npm_actual": "...",
  "rust_required": "...",
  "rustc_actual": "...",
  "cargo_actual": "...",
  "go_required": "1.26.2",
  "go_actual": "...",
  "windows_sdk": "...",
  "tauri_cli": "...",
  "target": "x86_64-pc-windows-msvc",
  "source_of_constraints": [
    "当前固定 commit 中实际用于确认工具链要求的文件"
  ]
}
```

如果实际工具链不满足当前固定 commit 的要求：

```text
BUILD_GATE = FAIL
```

不得继续正式构建。

## 8.3 Gate

必须确认：

- LocalBridge checkout 对应目标版本与目标 commit。
- Tunnel Client commit 正确。
- LocalBridge 与 Tunnel Client 的工具链约束均已从固定源码基线确认。
- 实际工具链满足对应约束。
- 工具链版本与约束来源已记录。
- 工作区没有意外修改。

失败则停止。

---

# 9. Phase 1 — Source-built OpenAI Tunnel Client

这是 Community Build 第一阶段最重要的施工任务。

目标：

> 完全不使用 LocalBridge 作者打包的 `tunnel-client.exe`，而是从 OpenAI 官方源码自行编译兼容 LocalBridge 0.1.5 的版本。

---

## 9.1 获取源码

建议工作目录：

```text
D:\LocalBridgeCommunity\
│
├── LocalBridge\
├── tunnel-client\
└── artifacts\
```

执行：

```powershell
mkdir D:\LocalBridgeCommunity
cd D:\LocalBridgeCommunity

git clone https://github.com/zephyr7030/LocalBridge.git
git clone https://github.com/openai/tunnel-client.git
```

---

## 9.2 固定 Tunnel Client commit

```powershell
cd D:\LocalBridgeCommunity\tunnel-client

git fetch --all --tags
git checkout 8d55683eeef80bc5e360d95abf4692454fafc615
git rev-parse HEAD
git status --porcelain
```

`HEAD` 必须严格为：

```text
8d55683eeef80bc5e360d95abf4692454fafc615
```

工作区必须为空。

禁止修改 OpenAI Tunnel Client 功能代码。

---

## 9.3 验证版本

检查：

```text
pkg/version/VERSION
```

必须为：

```text
0.0.11
```

检查：

```text
go.mod
```

目标工具链：

```text
go 1.26.2
```

不一致则停止。

---

## 9.4 运行 OpenAI 原生测试

```powershell
cd D:\LocalBridgeCommunity\tunnel-client

go mod download
go test ./...
```

必须：

```text
PASS
```

如果未经修改的目标 commit 本身无法通过测试：

> 停止，不得继续替换 LocalBridge Runtime。

记录失败原因。

---

## 9.5 构建 tunnel-client.exe

```powershell
cd D:\LocalBridgeCommunity\tunnel-client

$env:CGO_ENABLED="0"
$env:GOOS="windows"
$env:GOARCH="amd64"

New-Item -ItemType Directory -Force `
  D:\LocalBridgeCommunity\artifacts | Out-Null
```

构建：

```powershell
go build `
  -mod=readonly `
  -trimpath `
  -buildvcs=false `
  -ldflags "-s -w -X github.com/openai/tunnel-client/pkg/version.GitSHA=8d55683eeef80bc5e360d95abf4692454fafc615" `
  -o D:\LocalBridgeCommunity\artifacts\tunnel-client.exe `
  ./cmd/client
```

构建原则：

- `-mod=readonly`：禁止构建过程修改依赖状态。
- `-trimpath`：减少本机绝对路径进入 binary。
- `-buildvcs=false`：减少本机 Git 元数据影响。
- GitSHA 显式注入。

---

## 9.6 验证 binary

执行：

```powershell
D:\LocalBridgeCommunity\artifacts\tunnel-client.exe --version
D:\LocalBridgeCommunity\artifacts\tunnel-client.exe --help
```

确认：

```text
version = 0.0.11
```

并确认 LocalBridge 依赖的基础 CLI 能力仍存在，例如：

```text
run
doctor
init
```

---

## 9.7 计算 SHA256

```powershell
Get-FileHash `
  D:\LocalBridgeCommunity\artifacts\tunnel-client.exe `
  -Algorithm SHA256
```

保存为：

```text
NEW_TUNNEL_CLIENT_SHA256
```

必须使用真实结果。

---

## 9.8 记录 provenance

创建：

```text
provenance/tunnel-client.json
```

至少：

```json
{
  "component": "openai-tunnel-client",
  "source": "https://github.com/openai/tunnel-client",
  "version": "0.0.11",
  "commit": "8d55683eeef80bc5e360d95abf4692454fafc615",
  "compiler": "go1.26.2",
  "target": "windows-amd64",
  "cgo": false,
  "sha256": "REAL_SHA256"
}
```

不得记录：

```text
API Key
Tunnel Runtime Key
Tunnel ID
MCP Bearer
Windows Credential
```

---

## 9.9 替换 LocalBridge Runtime

开发阶段可以先备份原文件：

```powershell
cd D:\LocalBridgeCommunity\LocalBridge

Copy-Item `
  runtime\tunnel-client\tunnel-client.exe `
  runtime\tunnel-client\tunnel-client.upstream-localbridge.exe
```

注意：

> 该备份仅用于开发比较，最终 Release 中禁止存在。

复制自构建 binary：

```powershell
Copy-Item `
  D:\LocalBridgeCommunity\artifacts\tunnel-client.exe `
  runtime\tunnel-client\tunnel-client.exe `
  -Force
```

重新计算 SHA256，并确认与 `NEW_TUNNEL_CLIENT_SHA256` 一致。

---

## 9.10 更新完整性校验

文件：

```text
src-tauri/src/tunnel/bundle.rs
```

将：

```rust
TUNNEL_CLIENT_SHA256
```

更新为自构建 binary 的真实 SHA256。

只允许替换目标 hash。

禁止：

- 删除 `verify_bundle()`。
- 将 checksum mismatch 改成 warning。
- 允许任意 executable。
- 关闭 fail-closed 行为。

---

## 9.11 更新 runtime-manifest.toml

保留：

```toml
source = "https://github.com/openai/tunnel-client"
version = "0.0.11"
git_commit = "8d55683eeef80bc5e360d95abf4692454fafc615"
```

更新：

```toml
executable_sha256 = "<NEW_TUNNEL_CLIENT_SHA256>"
```

优先尝试：

```toml
vendoring = "source-built"
```

如果现有 schema 不接受，则不要为了字段名称破坏上游测试。

可退化为：

```toml
build_origin = "source"
build_toolchain = "go1.26.2"
build_git_commit = "8d55683eeef80bc5e360d95abf4692454fafc615"
```

如果 schema 同样不允许新增字段：

> 只更新 executable SHA256，并将完整构建来源记录放入 provenance。

---

## 9.12 LICENSE

第一版原则：

> 无必要不要修改 LICENSE。

确认当前：

```text
runtime/tunnel-client/LICENSE
```

与目标 OpenAI commit 一致。

如果一致：

```text
保留
```

如果不同：

- 从目标 OpenAI commit 复制 LICENSE。
- 重新计算 LICENSE SHA256。
- 更新对应校验和 manifest/notice。
- 保留 Apache-2.0 notice。

---

## 9.13 删除作者 binary 备份

最终构建前：

```powershell
Remove-Item `
  runtime\tunnel-client\tunnel-client.upstream-localbridge.exe `
  -Force
```

搜索：

```powershell
Get-ChildItem . -Recurse -File |
Where-Object {
    $_.Name -match "tunnel-client.*\.exe"
}
```

最终 Runtime 正常情况下只允许存在：

```text
runtime/tunnel-client/tunnel-client.exe
```

构建目录临时产物除外。

---

## 9.14 binary 来源验证

执行：

```powershell
go version -m `
  runtime\tunnel-client\tunnel-client.exe
```

保存：

```text
artifacts\tunnel-client-go-version.txt
```

应能够确认：

```text
github.com/openai/tunnel-client
```

及相关 Go module 信息。

---

## 9.15 Phase 1 Gate

以下全部 PASS：

- [ ] OpenAI Tunnel Client 来自官方源码。
- [ ] commit 正确。
- [ ] VERSION 为 0.0.11。
- [ ] `go test ./...` PASS。
- [ ] binary 为本次源码构建。
- [ ] binary SHA256 已记录。
- [ ] `bundle.rs` 使用新 SHA256。
- [ ] checksum 未被关闭。
- [ ] Runtime 中不存在作者原 binary 备份。
- [ ] `go version -m` 来源验证成功。
- [ ] provenance 已生成。
- [ ] 没有 credential 泄露。

否则停止。

---

# 10. Phase 2 — Source-built LocalBridge App + Privileged Broker

目标：

> 不使用 LocalBridge 作者发布的 Installer，不通过“解包官方 Installer → 替换文件 → 重打包”的方式制作 Community Build。

必须从当前 Community Git checkout 构建。

正确链路：

```text
Community Git checkout
        ↓
npm ci
        ↓
prepare runtime resources
        ↓
frontend tests / build
        ↓
cargo test
        ↓
cargo clippy
        ↓
LocalBridge App build
        ↓
Privileged Broker build
```

---

## 10.1 安装依赖

```powershell
cd D:\LocalBridgeCommunity\LocalBridge

npm ci
```

然后使用**当前固定 LocalBridge commit 中实际存在、并由项目构建配置引用的 Runtime preparation script**。

当前基线如果确认仍为：

```powershell
node scripts/prepare-lb018-resources.mjs
```

则执行并记录该脚本名称。

规则：

- 不得仅根据文件名中的 `lb018` 推断其一定适用于或不适用于 0.1.5。
- Agent 必须通过当前 commit 的 `package.json`、构建脚本、Tauri 配置或相关源码确认真实入口。
- 实际使用的 resource preparation script 必须写入 TEST-REPORT 或 provenance。

不得加入：

```text
cloudflared.exe
cloudflared-manifest.json
```

---

## 10.2 前端测试

```powershell
npm test
npm run build
```

必须全部 PASS。

---

## 10.3 Rust 测试与静态检查

先运行 Rust 测试：

```powershell
cargo test `
  --manifest-path src-tauri/Cargo.toml `
  --locked `
  -- `
  --test-threads=1
```

重点关注：

```text
tunnel
bundle
runtime
credential
process supervisor
health
broker
```

与 pinned Runtime hash 有关的测试必须 PASS。

发现 hash mismatch 时：

> 修复 manifest/hash，不得绕过测试。

随后运行 Community Build 的 Rust 静态质量 Gate：

```powershell
cargo clippy `
  --manifest-path src-tauri/Cargo.toml `
  --locked `
  --all-targets `
  -- `
  -D warnings
```

规则：

- `cargo clippy` 属于 `BUILD_GATE`。
- 如果当前固定 LocalBridge commit 的 CI / 构建脚本使用了更严格的 Clippy 参数，应采用上游更严格的参数，不得降级。
- 如果上游 target/features 结构导致上述通用命令不适用，Agent 必须从当前 commit 确认真实参数，并在 `TEST-REPORT.md` 记录实际命令。
- 不得通过删除 `-D warnings`、大范围 `allow` 或修改业务代码来无理由掩盖已有警告；若必须做兼容性调整，应在 `UPSTREAM-DIFF.md` 中说明。

---

## 10.4 Privileged Broker

Broker 属于高风险组件。

在开始构建前，Agent 必须先从当前固定 LocalBridge commit 中确认：

```text
Broker 对应的实际 Cargo target / binary target
真实构建入口
最终 artifact 路径
Tauri packaging 是否会自动构建/复制 Broker
是否需要额外独立构建步骤
```

不得根据文件名、历史版本或文档示例猜测构建命令。

确认后的实际 target、build command、artifact path 必须写入 `provenance/broker.json`。

必须：

1. 从当前 LocalBridge/Community commit 源码构建。
2. 不使用作者发布的 Broker binary。
3. 单独记录 Broker SHA256。
4. 生成 `provenance/broker.json`。
5. 运行 Broker IPC 测试。
6. 验证 Named Pipe ACL。
7. 验证 Broker 没有公网 listener。
8. 验证普通模式不能直接取得管理员能力。
9. 验证 Elevated 能力仍受原有 capability 限制。
10. 不得将 arbitrary elevated shell 作为默认能力。

Broker provenance 至少记录：

```text
source commit
rustc version
target
build command
sha256
test result
```

---

## 10.5 Phase 2 Gate

- [ ] `npm ci` PASS。
- [ ] resource preparation PASS。
- [ ] `npm test` PASS。
- [ ] `npm run build` PASS。
- [ ] `cargo test --locked` PASS。
- [ ] `cargo clippy --locked --all-targets -- -D warnings` 或当前 commit 对应的更严格等价命令 PASS。
- [ ] 实际 Runtime preparation command 已记录。
- [ ] 实际 Broker target / build command / artifact path 已记录。
- [ ] LocalBridge App 来自当前源码。
- [ ] Privileged Broker 来自当前源码。
- [ ] Broker SHA256 已记录。
- [ ] Broker IPC/ACL 基础测试 PASS。
- [ ] 没有使用作者发布 Installer 中提取的 App/Broker。

---

# 11. Phase 3 — Runtime Supply Chain Lock

整理所有 bundled Runtime。

至少包括：

```text
OpenAI tunnel-client
coding-tools-mcp
Python embedded runtime
Document runtime
其他 bundled tools
```

为每个组件记录：

```text
name
source
version
commit/tag
download URL
SHA256
license
build_type
```

生成：

```text
provenance/runtime-lock.json
```

`runtime-manifest.toml` 与 `runtime-lock.json` 的职责必须明确区分：

```text
runtime-manifest.toml
→ 保持上游兼容，作为 LocalBridge 运行/构建所需的 manifest

provenance/runtime-lock.json
→ Community Build 在构建时根据实际 Runtime、manifest 和文件 SHA256 自动生成的供应链快照
```

规则：

- `runtime-lock.json` 不得作为第二份人工维护的真相源。
- Runtime 的 SHA256、build_type、来源等字段应优先由构建脚本自动生成。
- 不得要求维护者同时手工修改 `runtime-manifest.toml` 与 `runtime-lock.json` 中的同一 hash。
- 如果两者存在冲突，构建必须失败并指出冲突来源。

例如：

```json
{
  "name": "python-embedded",
  "source": "python.org",
  "version": "...",
  "sha256": "...",
  "license": "...",
  "build_type": "upstream-binary"
}
```

或：

```json
{
  "name": "openai-tunnel-client",
  "source": "github.com/openai/tunnel-client",
  "commit": "...",
  "sha256": "...",
  "build_type": "source-built"
}
```

规则：

> Runtime 中出现未登记 executable，构建必须失败。

---

# 12. Phase 4 — Unified Build Script

建立：

```text
scripts/build-community.ps1
```

目标：

> 将人工步骤变成一次可重复运行的 Community Build。

建议顺序：

```text
1. 检查 Git 工作区
2. 检查目标 upstream version / commit
3. 检查 Community commit
4. 从固定 commit 解析并检查工具链约束
5. 记录实际工具链版本
6. 构建 Tunnel Client
7. 验证 Tunnel Client SHA256 / module provenance
8. 从固定 commit 确认 Runtime preparation 真实入口
9. 准备/验证 Runtime
10. 确认最终 Runtime staging tree
11. 检查 staging tree 中未知 executable
12. npm ci
13. npm test
14. npm run build
15. cargo test
16. cargo clippy
17. 从固定 commit 确认 LocalBridge / Broker 的真实 build target 与 artifact path
18. 构建 LocalBridge App
19. 构建 Privileged Broker
20. Broker tests
21. Tunnel launch test
22. MCP Guard test
23. 从固定 commit 确认实际 Tauri release / bundle command
24. 执行实际 Tauri release / bundle command 并生成 Installer
25. 审计 Installer / 安装后 Runtime 内容
26. 生成 SHA256SUMS
27. 生成 provenance
28. 生成 UPSTREAM-DIFF
29. 生成 TEST-REPORT
```

任何一步失败：

```text
立即停止
```

禁止：

```text
失败后继续生成 Stable Release
```

对于 Runtime preparation、Broker 构建、Tauri release/bundle 等可能随上游变化的命令：

- 本文中的命令只能作为当前已知示例。
- `build-community.ps1` 必须优先从固定 LocalBridge commit 的实际配置/脚本确认真实入口。
- 实际执行命令必须记录到 `BUILD-PROVENANCE.json` 或 `TEST-REPORT.md`。
- 如果无法确认真实入口，Gate 必须为 `FAIL`，不得猜测后继续打包。

---

# 13. Community Build 自有验证

## Test 1 — 禁止作者 Tunnel Binary

最终 Runtime 的 Tunnel Client hash 必须等于本次 Community 构建产生的 hash。

---

## Test 2 — Tunnel Binary 来源验证

```powershell
go version -m tunnel-client.exe
```

确认 module 来源。

---

## Test 3 — Runtime 完整性

所有 Runtime 文件 hash 与：

```text
runtime-lock.json
```

一致。

---

## Test 4 — Unknown Runtime Reject

只扫描**最终 Installer 实际会打包进入 Runtime 的 staging tree**，不要扫描整个 Git 工作区。

推荐范围：

```text
扫描：
release-staging/runtime/**
或 Tauri 最终打包前的等价 Runtime staging 目录

不扫描：
target/**
node_modules/**
artifacts/**
.git/**
tests/fixtures/**
其他不会进入 Installer 的缓存/测试目录
```

最终 Runtime staging tree 中出现未登记 executable：

```text
BUILD FAIL
```

Agent 必须根据当前 LocalBridge/Tauri packaging 配置确认真实 staging/runtime 路径，不得凭目录名猜测。

---

## Test 5 — Sensitive Information Scan

扫描：

```text
installer
logs
provenance
build artifacts
```

禁止出现：

```text
真实 API Key
Tunnel Runtime Key
MCP Bearer
生产 Tunnel ID
测试账号真实凭据
```

凭据规则必须区分测试类型：

```text
Unit / Integration / Security Test
→ 只允许 synthetic/fake credential

Real Tunnel E2E
→ 允许使用真实、临时、授权的 Tunnel 凭据
→ 仅通过环境变量或安全凭据存储注入
→ 不得进入 argv、日志、截图、artifact、provenance、Git 或测试报告正文
```

---

# 14. Phase 5 — Integration / Security / E2E

## 14.1 Local MCP Runtime

确认：

```text
127.0.0.1
```

本地 MCP Guard 能正常启动。

---

## 14.2 Tunnel Client Process

确认：

```text
tunnel-client.exe
```

由 LocalBridge 正常拉起。

可检查：

```powershell
Get-Process tunnel-client |
Select-Object Id,Path
```

必须指向 Community Runtime。

---

## 14.3 Health

确认 LocalBridge 使用：

```text
--health.listen-addr
--health.url-file
```

并验证：

```text
/healthz
/readyz
```

正常。

---

## 14.4 MCP Server URL

Tunnel Client 必须连接：

```text
127.0.0.1:<LocalBridge MCP Guard port>
```

不得改成公网 MCP listener。

---

## 14.5 MCP Guard Bearer

保持原有：

```text
Authorization: env:LOCALBRIDGE_MCP_GUARD_BEARER
```

及 discovery header 行为。

测试：

```text
无正确 Bearer → 拒绝
正确注入 Bearer → 成功
```

---

## 14.6 Secret Injection

必须继续使用环境变量引用：

```text
LOCALBRIDGE_RUNTIME_API_KEY
CONTROL_PLANE_TUNNEL_ID
LOCALBRIDGE_MCP_GUARD_BEARER
```

禁止：

```text
真实 API Key 出现在 argv
Bearer Token 写配置文件
credential 进入 provenance
```

一旦发现真实 API Key 出现在命令行：

> 立即停止，视为安全回归。

---

## 14.7 Tunnel E2E

在合法 OpenAI Tunnel 凭据条件下：

```text
ChatGPT
   │
   ▼
OpenAI Tunnel Service
   │
   ▼
Community-built tunnel-client.exe
   │
   ▼
LocalBridge MCP Guard
   │
   ▼
coding-tools-mcp
```

测试顺序：

```text
1. tools/list
2. read-only MCP call
3. Edit MCP call
4. Full Mode 基础调用
5. 最后再测试 Elevated
```

不得一开始就测试 Elevated。

---

# 15. Phase 6 — Community Installer

所有前置测试 PASS 后才允许执行 Release Build。

禁止：

```text
下载 LocalBridge 官方 Installer
→ 解包
→ 替换 binary
→ 重新封装
```

最终 Installer 必须来自：

```text
Community LocalBridge source checkout
+
Community-built OpenAI tunnel-client
+
Community-built privileged broker
+
当前 Runtime lock
+
本机构建 Tauri application
```

---

## 15.1 安装后审计

在干净 VM 中安装。

检查：

```text
LocalBridge.exe

runtime/
├── tunnel-client/
│   ├── tunnel-client.exe
│   └── LICENSE
├── python/
├── coding-tools-mcp/
└── ...
```

重新计算安装后的：

```text
tunnel-client.exe SHA256
Broker SHA256
关键 Runtime SHA256
```

必须与构建记录一致。

---

# 16. Release Artifacts

`0.1.5-community.1` 至少发布：

```text
LocalBridge-Community-0.1.5-community.1.exe

SHA256SUMS.txt

BUILD-PROVENANCE.json

TEST-REPORT.md

UPSTREAM-DIFF.md

THIRD-PARTY-NOTICES.md
```

第一版如果已经具备能力，也生成：

```text
SBOM.spdx.json
```

SBOM 可以作为 `community.1` 推荐项，也可以在 `community.2` 完善。

---

# 17. UPSTREAM-DIFF.md

每个 Community Release 必须生成：

```text
UPSTREAM-DIFF.md
```

目的：

> 让审计者能够快速确认 Community Build 相比固定 LocalBridge upstream commit 到底修改了什么，尤其是是否修改了安全相关上游代码。

至少包含：

```text
Upstream version:
...

Upstream commit:
...

Community commit:
...

Community-only changes:
1. Source-built OpenAI tunnel-client
2. Runtime provenance / verification
3. Community build automation
4. Community version / release metadata
5. 其他实际修改

Security-sensitive upstream files modified:
- <file>
  Purpose: <why>
  Security behavior change: NONE / DESCRIBE
  Permission surface expanded: YES / NO
  Integrity/authentication behavior weakened: YES / NO
  Covered by tests: <tests>

Security behavior changes:
NONE / DESCRIBE
```

这里必须区分两个概念：

```text
“修改了安全敏感文件”
≠
“改变或削弱了安全语义”
```

例如本规范要求为自构建 Tunnel Client 更新：

```text
src-tauri/src/tunnel/bundle.rs
```

中的 pinned SHA256。即使完整性校验仍然保持 fail-closed，也应在：

```text
Security-sensitive upstream files modified
```

中如实列出该文件，而不是因为“没有削弱安全行为”就写成完全 `NONE`。

如果确实改变了安全行为，则必须逐项列出：

```text
文件
修改目的
原行为
新行为
对应测试
是否扩大权限面
是否削弱认证 / 完整性 / 最小权限
```

`Security behavior changes: NONE` 只有在确认**安全语义没有变化**时才允许填写。

推荐由脚本根据固定 upstream commit 与当前 Community commit 的 Git diff 自动生成基础文件清单，再由 Release 阶段补充人工语义审查。

---

# 18. BUILD-PROVENANCE

至少包含：

```text
Community version
LocalBridge upstream version
LocalBridge upstream commit
Community commit
Tunnel Client version
Tunnel Client commit
Tunnel Client SHA256
Broker SHA256
构建机器 OS
工具链版本
构建时间
Installer SHA256
测试结果
Runtime lock version/hash
```

禁止记录 credential。

---

# 19. TEST-REPORT.md

至少：

```text
LocalBridge upstream version:
...

LocalBridge upstream commit:
...

Community commit:
...

OpenAI tunnel-client version:
0.0.11

OpenAI tunnel-client commit:
8d55683eeef80bc5e360d95abf4692454fafc615

Go version:
...

Toolchain constraints source:
...

Runtime preparation command:
...

Tauri release / bundle command:
...

Broker target:
...

Broker build command:
...

Broker artifact path:
...

Tunnel Client SHA256:
...

Broker SHA256:
...

npm test:
PASS / FAIL

npm run build:
PASS / FAIL

cargo test:
PASS / FAIL

cargo clippy:
PASS / FAIL

Broker tests:
PASS / FAIL

Tunnel process launch:
PASS / FAIL

Health:
PASS / FAIL

MCP Guard auth:
PASS / FAIL

OpenAI Tunnel connection:
PASS / FAIL

tools/list:
PASS / FAIL

read-only MCP call:
PASS / FAIL

Edit MCP call:
PASS / FAIL

Full Mode basic test:
PASS / FAIL

Elevated basic test:
PASS / FAIL

Installed runtime checksum:
PASS / FAIL

Credential scan:
PASS / FAIL
```

禁止只写：

```text
Everything works.
```

---

# 20. Release Gate 分类与 Stable Release Gate

为避免把“测试失败”和“当前环境无法执行”混为一谈，所有 Gate 必须分类：

```text
BUILD_GATE
→ 可自动执行的构建/静态/单元/集成检查
→ 必须 PASS，否则不得生成 Release artifact

SECURITY_GATE
→ 与权限、认证、完整性、Broker、敏感信息等相关
→ 必须 PASS，否则不得标记 Stable

ENVIRONMENT_GATE
→ 依赖真实 OpenAI Tunnel、UAC 交互、干净 VM 等外部环境
→ 允许状态为 PASS / FAIL / NOT RUN
→ NOT RUN 时只允许发布 Pre-release，并必须写明未执行原因
```

Release Profile：

```text
Stable
→ BUILD_GATE 全部 PASS
→ SECURITY_GATE 全部 PASS
→ 必需 ENVIRONMENT_GATE 全部 PASS

Pre-release
→ BUILD_GATE 仍必须全部 PASS
→ SECURITY_GATE 不得存在 FAIL
→ ENVIRONMENT_GATE 可存在 NOT RUN，但必须明确原因

Development artifact
→ 允许存在尚未执行的后续 Gate
→ 不得以 Stable / Pre-release 名义对外发布
```

状态语义：

```text
PASS
→ 已执行并通过

FAIL
→ 已执行但失败

NOT RUN
→ 因缺少授权凭据、UAC/VM 环境或其他外部前置条件而未执行
```

禁止把 `NOT RUN` 写成 `PASS`，也禁止把 `FAIL` 描述成“环境未准备”。

正式 Stable 前必须全部满足：

- [ ] 上游版本明确。
- [ ] 上游 commit 明确。
- [ ] Community commit 明确。
- [ ] Tunnel Client 来自 OpenAI 官方源码。
- [ ] Tunnel Client 固定目标 commit。
- [ ] Tunnel Client 自行源码构建。
- [ ] Tunnel Client SHA256 已记录。
- [ ] Tunnel integrity check 未关闭。
- [ ] Privileged Broker 自行源码构建。
- [ ] Broker SHA256 已记录。
- [ ] 所有 Runtime 来源明确。
- [ ] 所有 executable 有 SHA256。
- [ ] 没有 unknown binary。
- [ ] 没有作者原 Tunnel binary 备份。
- [ ] 没有通过官方 Installer 改包。
- [ ] `npm test` PASS。
- [ ] frontend build PASS。
- [ ] `cargo test --locked` PASS。
- [ ] `cargo clippy` 对当前固定 commit 的实际 Gate PASS。
- [ ] Runtime preparation / Broker build / Tauri release 的实际命令均已从固定 commit 确认并记录。
- [ ] Broker tests PASS。
- [ ] Tunnel launch PASS。
- [ ] Health PASS。
- [ ] MCP Guard PASS。
- [ ] Tunnel E2E PASS。
- [ ] Read-only MCP PASS。
- [ ] Edit MCP PASS。
- [ ] Full Mode 基础测试 PASS。
- [ ] Elevated 基础测试 PASS。
- [ ] credential scan PASS。
- [ ] 安装后关键 binary checksum PASS。
- [ ] SHA256SUMS 已生成。
- [ ] Provenance 已生成。
- [ ] TEST-REPORT 已生成。
- [ ] UPSTREAM-DIFF 已生成并与实际 Git diff 一致。
- [ ] 所有 security-sensitive upstream file 修改均已列出。
- [ ] `Security behavior changes` 已完成语义审查，不得仅依据文件名自动判定。
- [ ] THIRD-PARTY-NOTICES 已生成。

任何关键项 `FAIL`：

```text
不得标记 Stable
```

任何必需的 `ENVIRONMENT_GATE` 为 `NOT RUN`：

```text
不得标记 Stable
仅允许 Pre-release
```

可以：

```text
Pre-release
```

但必须明确区分并记录：

```text
FAIL 项
NOT RUN 项
NOT RUN 原因
```

---

# 21. Phase 7 — community.2 / community.3 增强

第一版稳定后再进入。

## 21.1 GitHub Actions

流程：

```text
Git tag
  ↓
Windows Runner
  ↓
checkout
  ↓
构建 Tunnel Client
  ↓
构建 LocalBridge / Broker
  ↓
Tests
  ↓
SBOM
  ↓
Provenance
  ↓
SHA256
  ↓
Release artifacts
```

同时必须保留本地：

```text
scripts/build-community.ps1
```

不能让项目只能在 CI 中构建。

---

## 21.2 SBOM

优先：

```text
SPDX
```

或：

```text
CycloneDX
```

覆盖：

```text
LocalBridge
React
Tauri
Rust dependencies
OpenAI tunnel-client
coding-tools-mcp
Python Runtime
其他 bundled components
```

---

## 21.3 可重复构建

目标：

```text
相同源码 commit
+
相同工具链
+
相同构建参数
```

尽可能生成一致 binary。

优先保证：

```text
Tunnel Client
Privileged Broker
核心 Runtime
```

可重复。

Installer 如果因 Windows/Tauri timestamp、签名等产生差异：

> 允许，但必须解释差异来源。

---

# 22. 第二阶段安全增强

Community Build 第一版稳定后再考虑。

## 22.1 Full Mode 风险提示

UI 明确：

```text
Full Mode ≠ OS Sandbox
```

---

## 22.2 Workspace 外访问审计

记录：

```text
哪个 tool
什么时候
访问什么路径
是否允许
```

不得记录秘密数据。

---

## 22.3 Elevated 审计

记录：

```text
请求
capability
结果
```

禁止记录：

```text
password
token
secret
```

---

## 22.4 Runtime 权限面板

UI 明确显示：

```text
Edit
Full
Elevated
```

分别实际意味着什么能力。

---

# 23. 后续：真正 OS Sandbox

不是 v1 范围。

可研究：

```text
Windows Sandbox
AppContainer
Restricted Token
Job Object
ACL 临时隔离
```

目标：

> 即使 `exec_command` 启动普通程序，也尽量无法任意访问 workspace 之外的数据。

这是 Community Build 很有价值的后续方向，但禁止在第一版实施。

---

# 24. 高级模式

未来可以设计：

```text
Strict Mode
```

例如：

```text
默认 Edit
exec 每次确认
Elevated 每次确认
禁止 network
禁止 workspace 外读
```

以及：

```text
Developer Mode
```

允许高级用户放宽限制。

默认方向：

```text
Strict / Edit
```

---

# 25. Agent 执行规则

把本文交给编码 Agent 时，要求遵守以下规则。

## 25.1 规范性关键词

为避免 Agent 对措辞产生不同解释，本文使用：

```text
必须 / MUST
→ 不满足即 Gate FAIL，除非该项明确属于 ENVIRONMENT_GATE 并允许 NOT RUN

不得 / MUST NOT
→ 禁止绕过

应该 / SHOULD
→ 默认执行；若不执行，必须记录理由

可以 / MAY
→ 可选，不影响 Gate，除非 Release Profile 另有规定
```

任何脚本自动化都不得把 `SHOULD` 自行提升为修改上游行为的理由，也不得把 `MUST` 降级成 warning。

---

## 25.2 不扩大范围

Agent 不得自行：

- 升级依赖大版本。
- 升级 Tunnel Client。
- 重写协议。
- 修改认证模型。
- 降低安全检查。
- 引入与本阶段无关的功能。

---

## 25.3 Fail Closed

发现：

```text
commit 不匹配
hash 不匹配
test fail
unknown binary
credential 泄露
CLI 不兼容
Broker 权限异常
```

必须：

```text
停止
记录
修复
重新测试
```

不得通过关闭安全检查让流程继续。

---

## 25.4 每个 Phase 单独提交

推荐：

```text
commit 1
build: add source-built OpenAI tunnel-client

commit 2
build: build LocalBridge and privileged broker from source

commit 3
build: add runtime provenance and verification

commit 4
test: add Community Build validation

commit 5
build: add Community installer pipeline
```

不要把全部工作塞进一个巨大 commit。

---

## 25.5 Agent 必须输出

每个 Phase 完成后：

```text
1. 修改了哪些文件
2. 执行了哪些命令
3. 测试结果
4. 新增/变化的 SHA256
5. 是否存在未解决问题
6. 当前 Gate 状态（PASS / FAIL / NOT RUN）
7. 若为 NOT RUN，原因及缺少的外部前置条件
8. 是否允许进入下一 Phase
```

禁止仅回复：

```text
完成了
```

---

# 26. Community Build v1 最终成功标准

最终链路必须完整成立：

```text
LocalBridge upstream source
        │
        ▼
固定 upstream commit
        │
        ▼
Community Fork
        │
        ├───────────────┐
        ▼               ▼
LocalBridge App    Privileged Broker
source build          source build
        │               │
        └───────┬───────┘
                │
                ▼
OpenAI official tunnel-client source
                │
                ▼
fixed commit 8d55683...
                │
                ▼
Community Go build
                │
                ▼
Community tunnel-client.exe
                │
                ▼
Runtime Lock + SHA256
                │
                ▼
Tests + E2E
                │
                ▼
Provenance
                │
                ▼
Community Installer
                │
                ▼
Clean VM verification
```

第一版完成后，必须能够回答：

### LocalBridge 当前运行的 Tunnel Client 是否由 LocalBridge 作者提供？

```text
否。
```

### Tunnel Client 源代码来自哪里？

```text
OpenAI 官方 github.com/openai/tunnel-client
```

### 固定哪个 commit？

```text
8d55683eeef80bc5e360d95abf4692454fafc615
```

### 最终 Tunnel Client binary 是谁编译的？

```text
Community Build 从固定公开源码自行编译。
```

### LocalBridge 主程序和 Privileged Broker 是谁编译的？

```text
Community Build 从固定 LocalBridge 源码 commit 自行编译。
```

### Community Build 是否改变了上游安全语义？

必须能够通过 `UPSTREAM-DIFF.md` 回答：

```text
哪些 security-sensitive 文件被修改
这些修改是否改变权限、认证、完整性或最小权限行为
对应哪些测试
```

不能仅回答“改过”或“没改过”。

---

### 仍存在第三方预编译 Runtime 吗？

允许存在，但每一个都必须：

```text
来源明确
版本明确
SHA256 明确
许可证明确
build_type 明确
```

---

# 27. 一句话开发原则

> 不重新发明 LocalBridge。先把 LocalBridge 变成一个能够从公开源码完整追踪、关键组件自行构建、Runtime 可验证、测试可复核、最终安装包可解释的 Community Build。
