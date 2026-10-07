# Ait 0.0.21 发布准备与失败记录

**未发布。** [首次正式构建](https://github.com/ait-app/ait/actions/runs/37574574517)在 Linux 和 macOS 成品冒烟测试中均失败：daemon 仍以 `agent_stream` 发送 Timeline 通知，名称迁移后的客户端缺少映射，SDK 校验丢弃实时更新。没有生成 GitHub Release 或对外安装包。标签 `v0.0.21` 保留不动；修复后改用 [0.0.22](release-0.0.22.md) 发布。

日期：2026-10-07（Asia/Shanghai）。本次准备基于 main `f50408452e245d9dac7c6edfaf102459d82b2321`；版本 PR #201 合并后标签 `v0.0.21` 指向 `0fc1e3e399df86cf3947bac82243e70dbbc515c8`；发布门禁失败，未生成 Release 或 `BUILD-INFO.json`。

## 更新内容

- **工作区重置同步远端。** 托管 worktree 恢复初始分支并重置到 origin 最新默认分支后，存在同名远端分支时用一次带明确 lease 的强制推送同步到同一提交。远端分支不存在时跳过；并发更新、删除或推送拒绝会明确报错。主 checkout 保持不可重置。见 [ADR-092](../../decisions/workspace/adr-092-reset-same-named-remote-branch.md)。
- **独立会话并发。** 不同 Agent 会话独立执行，同一会话仍按顺序处理；Provider 发现和历史恢复在后台运行，避免阻塞主机连接。见 [ADR-091](../../decisions/providers/adr-091-independent-session-execution.md)。
- **Antigravity 原生支持。** 新增 Antigravity CLI 发现、流式输出、审批、取消与对话恢复。见 [使用说明](../../operations/antigravity.md)。
- **DeepSeek Harness 与 OpenCode。** 恢复 DSH 原生交互、权限切换、问题与持久化会话历史；改进 OpenCode v2 发现和持久化历史中的完成状态。见 [DSH 使用说明](../../operations/deepseek-harness.md) 和 [OpenCode Provider 设计](../../decisions/providers/adr-074-opencode-native-provider.md)。
- **大结果和历史恢复。** Timeline 单项支持 768 KiB，超大查询分页保留完整条目；恢复的历史与实时缓存覆盖层分离，减少重复或过期条目。见 [ADR-090](../../decisions/providers/adr-090-timeline-entry-and-page-budgets.md)。
- **共享客户端协议。** 客户端、SDK、协议包与 daemon 统一采用 Ait 标准方法名；仓库内 SDK 和协议包同步重建。见 [ADR-094](../../decisions/clients/adr-094-canonical-ait-client-methods.md)。

## 原计划产物（未发布）

以下为准备时的预期文件名；本版本没有可下载的 Release。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.21-macos-arm64.dmg`、`Ait-0.0.21-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.21-linux-x64.tar.gz` |

桌面标签发布由 `Release Ait` 工作流构建 Linux/macOS 安装包、自动更新资产、签名与公证，并验证打包应用和 daemon 启动。Android APK、Google Play 和 iOS TestFlight 使用独立的手动流程。

## 发布准备验证

本次只更新版本清单、锁文件和文档，没有修改 `bins/` 或 `crates/` 中的 Rust 源码；按仓库规范跳过本地 Rust 测试与覆盖率重测。13 个 Cargo 包、根 npm 包和 6 个 npm workspace 均为 `0.0.21`。

在 macOS arm64、Node 26.10.0、npm 11.19.1 上完成以下检查：

- `npm ci --no-audit --no-fund`：通过，锁定安装并执行仓库 postinstall 补丁。
- `npm run verify:release -- v0.0.21`、`npm run verify:local-packages`：通过。
- `npm run test:release`：18 passed，0 failed；`npm run test:mobile-release`：32 passed，0 failed。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- `cargo metadata --locked --offline --no-deps --format-version 1`：通过，全部 13 个 workspace 包版本一致。
- `cargo fmt --all --check`、变更清单和文档的 Oxfmt、`npm run check:docs`、`git diff --check`：通过。
- 结构化比较 Cargo/npm 锁文件确认只更新本地包版本，第三方依赖及其锁定信息未变。

发布准备经 [PR #201](https://github.com/ait-app/ait/pull/201) 验证并合并，已创建 `v0.0.21`。正式工作流的实时 Timeline 门禁失败，未进入资产发布；修复后的发布流程见 0.0.22。

## Test coverage

**Not applicable — 本次发布准备没有修改 Rust 源码或业务行为。** 本次不重新测量行覆盖率；普通发布脚本测试的通过数量不代表覆盖率。版本 PR CI 已验证清单和构建；真实成品验证发现实时 Timeline 回归并阻止发布。下一步在 0.0.22 修复并重新验证。

[工作区远端重置的历史验证](../workspace/workspace-reset-remote-pr-validation-2026-10-07.md)记录源码 `c4f340f72dbf9dccf79fb53e2e1cea9e8448aa38` 的 1,894 passed、7 ignored，以及 workspace 53,569/56,802（94.31%）、filesystem 11,366/11,988（94.81%）的测量和证据。该统计不代表最终 0.0.21 标签的重新测量；其后 main 的客户端和请求分发更新也由各自 PR 与 CI 验证。
