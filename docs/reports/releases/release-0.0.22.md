# Ait 0.0.22 发布说明

日期：2026-10-07（Asia/Shanghai）。发布准备基于 main `55cf20e990f002cf52014a13d355dfe0628fd9ec`；正式源码以合并后的不可变标签 `v0.0.22` 和 Release 的 `BUILD-INFO.json` 为准。

上一稳定版本为 0.0.20。0.0.21 在成品实时 Timeline 门禁失败，没有发布安装包；[失败记录](release-0.0.21.md)保留原标签和工作流证据。本次修复通知适配后使用新版本发布。

## 更新内容

- **恢复实时 Timeline 更新。** 在现有协议适配边界把 Rust Timeline producer 的 `agent_stream` 转为 SDK 标准事件 `agent.stream`，保留订阅 ID、序号和 epoch。Timeline 不被加入 SessionEventKind 订阅类别，应用仍统一使用标准事件名。
- **工作区重置同步远端。** 托管 worktree 恢复初始分支并重置到 origin 最新默认分支后，存在同名远端分支时用一次带明确 lease 的强制推送同步到同一提交。远端分支不存在时跳过；并发更新、删除或推送拒绝会明确报错。主 checkout 保持不可重置。见 [ADR-092](../../decisions/workspace/adr-092-reset-same-named-remote-branch.md)。
- **独立会话并发。** 不同 Agent 会话独立执行，同一会话仍按顺序处理；Provider 发现和历史恢复在后台运行，避免阻塞主机连接。见 [ADR-091](../../decisions/providers/adr-091-independent-session-execution.md)。
- **Antigravity 原生支持。** 新增 Antigravity CLI 发现、流式输出、审批、取消与对话恢复。见 [使用说明](../../operations/antigravity.md)。
- **原生会话导入与权限。** 导入 OpenCode 和 DSH 的既有会话，保留原生身份、历史、模型、权限与恢复状态；OpenCode 支持原生 Build/Plan agent，审批可按原生规则选择一次、始终或拒绝。见 [PR #202](https://github.com/ait-app/ait/pull/202)。
- **Arch Linux 配方。** 新增本地工作区源码构建和 AUR 二进制配方，见 [发布指南](../../operations/releasing.md#arch-linux-本地源码安装)。本地 `PKGBUILD` 版本同步为 `0.0.22-1`；AUR 配方更新和独立 AUR 仓库发布依照指南另行处理。
- **DeepSeek Harness 与 OpenCode。** 恢复 DSH 原生交互、权限切换、问题与持久化会话历史；改进 OpenCode v2 发现和持久化历史中的完成状态。见 [DSH 使用说明](../../operations/deepseek-harness.md) 和 [OpenCode Provider 设计](../../decisions/providers/adr-074-opencode-native-provider.md)。
- **大结果和历史恢复。** Timeline 单项支持 768 KiB，超大查询分页保留完整条目；恢复的历史与实时缓存覆盖层分离，减少重复或过期条目。见 [ADR-090](../../decisions/providers/adr-090-timeline-entry-and-page-budgets.md)。
- **共享客户端协议。** 客户端、SDK、协议包与 daemon 统一采用 Ait 标准方法名；仓库内 SDK 和协议包同步重建。见 [ADR-094](../../decisions/clients/adr-094-canonical-ait-client-methods.md)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.22)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.22-macos-arm64.dmg`、`Ait-0.0.22-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.22-linux-x64.tar.gz` |

退出旧桌面应用，安装新版本后重新打开。正式工作流构建 Linux/macOS 桌面和自动更新资产，完成 macOS 签名、公证，并验证成品应用与 daemon 生命周期。Android APK、Google Play 和 iOS TestFlight 使用独立手动流程。

## 修复与发布准备验证

- 回归测试先在未修复适配器上复现 SDK discriminator 校验失败、Timeline 回调 0 次；补上映射后通过，并核对订阅、序号和 epoch 完整保留。
- `npm run test --workspace=@ait/protocol -- src/session-event-kinds.test.ts`：8 passed，0 failed。
- `npm run test --workspace=@ait/mobile -- src/runtime/rust-daemon/messages.test.ts src/runtime/rust-daemon/transport.test.ts`：52 passed，0 failed。
- `npm run verify:release -- v0.0.22`、`npm run verify:local-packages`：通过，13 个 Cargo 包、根 npm 包和 6 个 npm workspace 版本一致。
- `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`、变更 TypeScript 的 Oxlint：通过。
- `npm run build:desktop-assets`、桌面 `build:main`、`cargo build --locked --offline -p daemon --bin daemon`：通过。复用本会话已完成的锁定 npm 安装，第三方依赖没有变化。
- `npm run test:release`：18 passed；`npm run test:mobile-release`：32 passed。
- 变更 JSON/TypeScript/Markdown 的 Oxfmt、`npm run check:docs`、`git diff --check`：通过；标准方法目录校验通过（168 项）。
- 本机开发态桌面 E2E 未完成：Node 26.10.0 与 22.23.3 的 Playwright 均在 Electron 启动时因 `Runtime.evaluate: Cannot find context with specified id` 失败，尚未执行应用断言。不把它计为通过；实际成品启动、实时对话、重启与历史恢复由正式 Linux/macOS 发布门禁验证。
- 本地 `PKGBUILD` 的 `bash -n` 语法检查通过；本次没有在 Arch Linux 构建或发布 AUR 包。
- `cargo metadata --locked --offline --no-deps --format-version 1`、`cargo fmt --all --check`：通过；第三方锁定依赖未变。

发布准备经 PR 验证并合并后创建 `v0.0.22`；正式工作流完成后核对全部安装包、SHA-256、自动更新摘要和 `BUILD-INFO.json` 的源码提交。

## Test coverage

**Not measured。** 本次修复仅改变 TypeScript 协议通知适配，没有修改 `bins/` 或 `crates/` 中的 Rust 源码；按仓库规范不重跑本地 Rust 测试和行覆盖率。TypeScript 测试数量不是覆盖率百分比，本次没有可比的行覆盖率基线。回归测试验证实际 Rust wire 通知经 transport 与 SDK 校验到达拥有订阅的消费者；后续 PR CI 和正式 Linux/macOS 成品门禁验证最终发布源码。历史 Rust 覆盖率仅作为[此前版本准备记录](release-0.0.21.md#test-coverage)中的历史证据。
