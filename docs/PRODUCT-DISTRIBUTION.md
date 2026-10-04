# 产品发行源、配套扩展与安装引导

## 当前状态与入口

本项目的发行源是 `MYD548-svg/LocalBridge-Community`，统一配置于根目录 `product-release.json`。更新检查、本项目发布页、扩展附件查询和发行脚本共享这份配置。上游署名及第三方依赖的官方下载地址保留。

截至本轮核查，本仓库尚无正式 Release。候选代码的版本为 0.1.5，候选以来源提交 SHA 区分。尚未发布时应用会说明原因，不回退到其他项目；首次安装可以使用安装包内置的配套扩展。

安装操作见 [中文六步图解](../extensions/chatgpt-web/INSTALL.html)。网页接入、Codex 桌面接入和 Tunnel 高级兼容模式分别引导。

## 用户操作和实际状态

1. 选择项目、启用本地连接模式、启动本地服务。
2. 设置 → ChatGPT 网页，选择 Edge/Chrome，点击“准备配套扩展”。
3. 粘贴浏览器管理页地址并回车，开启开发者模式，加载应用显示的固定目录。
4. 在 ChatGPT 点击扩展连接这台电脑，回应用核对项目与权限并批准配对。
5. 回扩展启用当前聊天，以扩展的当前标签页状态为准。
6. 点击“准备首次只读调用”，检查填入内容后手动发送；核对 workspace_context 请求并确认执行，再查看并回填真实结果。

文件准备完成不表示浏览器安装、配对、聊天启用或工具执行已完成。应用的浏览器/聊天/成功次数是总计，不会自动把安装向导推进到“当前聊天已启用”。向导中的手动继续只切换提示；授权和执行仍由后端检查。

首次准备不要求关闭尚不存在的扩展。升级必须先在所有窗口停止并断开，关闭浏览器中的扩展开关，再明确确认替换；导入后重新启用/重新加载并刷新 ChatGPT。较旧内置包不能覆盖新版，重复准备同份文件返回明确的 alreadyPrepared 状态。

## 内置资源和配套下载

发行扩展生成后，经过 ZIP/身份/权限/文件校验，原样暂存为：

```
src-tauri/target/browser-extension-stage/extension.zip
src-tauri/target/browser-extension-stage/bundle.json
```

Tauri 将它们安装为 `browser-extension/extension.zip` 与 `browser-extension/bundle.json`。应用读取安装资源，核对编译时来源提交、版本、哈希及包内元数据后，再复用受管理目录导入流程。开发扩展不写入这两个发行资源。独立 ZIP 也携带原 MIT LICENSE，版权声明保持完整。

固定加载目录为 `%LOCALAPPDATA%\LocalBridge\browser-extension\current`。应用只把经校验的文件准备到这里，保留备份、互斥和活动连接检查；不会自动改变浏览器开发者模式或安装状态。

独立下载查询当前数字版本对应的正式 tag 和 `localbridge-release.json`，校验仓库、渠道、来源提交、应用版本、扩展身份、协议、兼容范围、附件名称/大小及地址。只打开与当前编译提交匹配的具体扩展附件。没有 Release、缺附件、版本不匹配、网络问题和发行数据错误分别返回状态；查询仅由用户触发，不加入两秒状态轮询。

发行包与安装包资源使用同一份 ZIP。Source code ZIP 和 Actions 外层附件不能作为扩展导入。现有合法旧元数据仍可手动校验导入；新内置包与正式发行清单必须携带本项目及来源提交信息。

## 版本规则与正式发行清单

正式 tag 格式为 `vX.Y.Z-community.1`。每次正式升级递增产品数字版本；社区后缀固定用于渠道标识，不用于在同一个数字版本下发行多次更新。候选可以保持相同数字版，通过来源提交区分。

应用数字版本在 npm、Cargo 和 Tauri 中必须一致，也必须满足 Chrome/Edge 的数字版本规则。更新比较读取发行清单的 applicationVersion，不能把带社区后缀的 tag 直接与 Cargo 版本比较。预发布/draft 不进入普通用户更新入口。

`localbridge-release.json` 包含 schemaVersion、repository、channel、tag、sourceCommit、applicationVersion、installer 与 extension。其中 installer/extension.asset 包含 name、size 和 sha256；extension 另含 version、protocol、extensionId 与 application 兼容范围。

正式附件包括安装 EXE、发行扩展 ZIP、清单、INSTALL.html、INSTALL.svg、SHA256SUMS.txt、完整 TEST-REPORT.json 和 BUILD-PROVENANCE.json。

## 构建与发布复用

现有十九阶段门禁保留。frontend-build 先生成扩展，runtime-resources 依赖该阶段；Rust 编译前扩展资源已存在。Tauri hook 再次调用构建时，只有提交、全部相关源文件/构建脚本/配置、ZIP 校验和及暂存资源一致才复用扩展；失配即重建，失败使旧 PASS 失效。

最终 NSIS 检查实际提取安装资源，核对内置 ZIP 和描述文件，继续核对 broker、MCP adapter、浏览器宿主与注册模板。成功后将同份 EXE/ZIP、清单和图解集中生成到 `tests/artifacts/release`，与最终 CI 报告组成完整候选。

- main/release 分支与指向它们的 PR 由 Community Build 处理，保留固定上游源码的完整 Go 测试。
- 其他分支与 PR 由 CI 处理；本次 codex 分支普通推送触发 CI。
- tag 推送不执行完整构建，不自动发布。
- 同工作流/同 ref 的过期构建可取消；必要测试和失败诊断仍保留。

后续正式发布由 `Publish verified community release` 手动触发，输入成功 Community Build 的 run_id、指向同一提交的既有 tag，并明确选中 publish。它核对源仓库、工作流、成功状态、提交、全部十九阶段报告、资源哈希与指南内容后，使用既有附件发布；不安装编译依赖、不重新执行 Windows 构建，不覆盖已有 Release。正式数字版本必须大于当前正式发行。

普通推送只提供候选，不触发这个手动发布流程。本轮实施的终点仍是一次普通推送并核对远端 SHA；不创建 tag/Release，不查询或等待新 Actions。

## 验证边界

本轮运行结果见 [网页检查报告](CHATGPT-WEB-TEST-REPORT.md)。Linux 下可检查 TypeScript、前端/扩展、打包合同及平台无关 Rust 逻辑；Windows 原生编译、实际 NSIS、干净安装、Edge/Chrome 实际加载和真实 ChatGPT 操作需要相应环境验证，不能由文件存在、模拟调用或单元测试推定通过。
