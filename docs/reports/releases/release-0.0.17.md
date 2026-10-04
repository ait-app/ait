# Ait 0.0.17 发布说明

日期：2026-10-04（Asia/Shanghai）。本次准备基于 `main` 的
`db115f38a1ba6cca5d562f0c2c449f7f4a843a53`；正式源码以合并后的不可变标签
`v0.0.17` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **Workspace 可空字段。** daemon 将缺失的可空 Workspace、Project 和目录运行时字段
  规范化为 `null`，让目录更新能清除客户端旧状态。Checkout 输入仍区分缺失与显式 `null`；
  字段名、请求方法和持久数据格式未改。见 [ADR-082](../../decisions/workspace/adr-082-canonical-nullable-workspace-fields.md)。
- **Rust 代码清理。** 简化 Agent thinking filter、OpenCode 事件投影和终端服务，移除多余克隆、
  不可达分支与无用 lint 例外。见 [PR #171](https://github.com/ait-app/ait/pull/171)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.17)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.17-macos-arm64.dmg`、`Ait-0.0.17-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.17-linux-x64.tar.gz` |

退出旧桌面应用，安装新版本后重新打开；远端 Host 也需升级 daemon 才能获得服务端改进。
Android APK、Google Play 和 iOS TestFlight 使用独立的手动发布流程。

## 发布验证

13 个 Cargo 包、根 npm 包和 6 个 workspace 同步到 `0.0.17`；Cargo/npm 锁文件仅更新本地包版本。
在 macOS arm64 上，以 `db115f38` 为基线完成以下准备检查：

- `npm ci --offline --no-audit --no-fund`、`npm run verify:release -- v0.0.17`：通过。
- `npm run test:release`：18 passed；`npm run test:mobile-release`：32 passed。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- `npm run check:docs`、Oxfmt、`cargo metadata --locked --offline --no-deps --format-version 1`、
  `cargo fmt --all --check`：通过。

包含 PR #171 的 [main CI](https://github.com/ait-app/ait/actions/runs/37198448356) 已通过。
Linux/macOS 正式安装包仍须由 `Release Ait` 工作流构建、签名、公证、验证并上传；
发布后核对资产、SHA-256、更新摘要和 `BUILD-INFO.json`。

## Test coverage

本次版本与文档准备未改 Rust 行为，未重新测量覆盖率。已合入的 PR #171 在其源码提交
`46980159e8ec289002adbc4d7224840159d01d3b` 上，以 macOS arm64、Rust 1.98.1、
默认 features 执行 `cargo llvm-cov --workspace --html --offline`，workspace 行覆盖率为
49,035/51,889（94.50%）；详细范围、逐 crate 结果、此前基线比较和可审查产物见
[PR 验证报告](../daemon/rust-code-smell-pr-validation-2026-10-04.md)。该测量早于合并与版本同步，
不代表最终标签源码的重新测量。3 项需要本地 Provider CLI 或认证的测试被忽略。
