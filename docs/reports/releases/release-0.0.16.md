# Ait 0.0.16 发布说明

日期：2026-10-04（Asia/Shanghai）。本次准备基于 `main` 的
`151ce454867d915b5f80823463d9b8eafd0283ae`；正式源码以不可变标签 `v0.0.16`
和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **工作区与 Agent 目录更新。** 连接中的客户端可及时收到目录、会话和运行状态变更，
  减少依赖轮询产生的延迟。[PR #166](https://github.com/ait-app/ait/pull/166)。
- **Codex 与 Agent 稳定性。** 显示 Codex 模型发现和容量错误，继续排除隐藏模型；
  修复任务预算竞争下的 Agent 创建，并隔离后台 Git fetch 失败。
  [PR #164](https://github.com/ait-app/ait/pull/164)、
  [PR #162](https://github.com/ait-app/ait/pull/162)。
- **终端选择。** 修复相同尺寸的终端 resize 声明导致选择内容丢失。
  [PR #165](https://github.com/ait-app/ait/pull/165)。
- **Android 发布入口。** Android APK 改为独立手动 EAS 工作流，桌面版本标签仍只触发
  Linux 和 macOS 构建。本次仅发布桌面安装包。
  [PR #163](https://github.com/ait-app/ait/pull/163)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.16)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.16-macos-arm64.dmg`、`Ait-0.0.16-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.16-linux-x64.tar.gz` |

退出旧桌面应用，安装新版本后重新打开；远端 Host 也需升级 daemon 才能获得服务端改进。
Android APK、Google Play 和 iOS TestFlight 使用独立的手动发布流程，不属于本次发布。

## 发布验证

13 个 Cargo 包及本地 path 依赖约束、根 npm 包和 6 个 workspace 均同步到 `0.0.16`；
Cargo/npm 锁文件只修改本地包版本，第三方依赖未变。

在 macOS arm64 上，以 `151ce454` 为源码基线，完成以下准备检查：

- `npm ci --offline --no-audit --no-fund`：通过。
- `npm run verify:release -- v0.0.16`：通过，确认 Rust、npm 及锁文件版本一致。
- `npm run test:release`：18 passed；`npm run test:mobile-release`：32 passed。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- `cargo metadata --locked --offline --no-deps --format-version 1`、`cargo fmt --all --check`、
  `git diff --check`：通过。

包含本次功能变更的 [main CI](https://github.com/ait-app/ait/actions/runs/37164338448)
已通过。正式 Linux/macOS 安装包仍须由 `Release Ait` 工作流构建、签名、公证、验证并上传；
发布后核对资产、SHA-256、更新摘要和 `BUILD-INFO.json`。

## Test coverage

**未测量。** 本次发布准备只修改版本清单、锁文件、更新日志和文档，没有修改 `bins/` 或
`crates/` 的 Rust 源码，按仓库规则跳过 Rust 测试和覆盖率复测。版本包含的既有行为变更已在
[目录推送覆盖率证据](../workspace/workspace-change-push-coverage.json)、
[Codex 容量覆盖率证据](../providers/codex-capacity-coverage.json)及
[Agent 创建覆盖率证据](../providers/agent-creation-resource-contention-coverage.json)
中分别记录；这些是各自提交的历史测量，不代表本发布提交的覆盖率。
以后修改 Rust 行为时，按 Rust style guide 重测对应 crate，并在提交准备阶段测量完整 workspace。
