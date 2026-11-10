# Ait 0.0.12 发布说明

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-10-02（Asia/Shanghai）。基于 main `220f1abd2d69c9a062fe06a1afb53bf56c9b22a0`
准备，正式来源以不可变标签 `v0.0.12` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **DeepSeek Harness。** 新增本机 ACP Provider，支持模型与推理等级发现、流式输出、
  工具审批、取消、上下文用量及会话恢复。使用前安装并配置 `dsh`；凭据由 Harness 管理。
  当前不支持原生会话导入、其他前端历史同步及 steer/rewind。
  [PR #139](https://github.com/ait-app/ait/pull/139)、[操作说明](../../../operations/deepseek-harness.md)。
- **后台 Git 刷新。** 活跃目录订阅首次观察仓库时立即 fetch，随后每 180 秒刷新 origin，
  按仓库去重并在最后一个观察者退出后取消；刷新后推送 checkout 状态。
  “从 main 更新”与基准比较能够使用已获取的远端提交。网络失败时保留已有引用。
  [PR #142](https://github.com/ait-app/ait/pull/142)、[验证报告](../workspace/background-git-fetch.md)。
- **侧边栏与运行状态。** 恢复增删行数、PR/MR 状态、CI 结果及 Git hover 信息，并持续
  更新；任务完成和取消后清除运行标记，下一轮任务仍正常显示运行状态。
  [PR #141](https://github.com/ait-app/ait/pull/141)、[验证报告](../workspace/workspace-sidebar-runtime.md)。
- **本地化与应用配置。** 补齐设置、Provider 用量、终端、浏览器工具及工作区导航翻译，
  移除过时的 Paseo 文档链接，并新增 Ait iOS EAS release profile。
  [PR #135](https://github.com/ait-app/ait/pull/135)、[PR #136](https://github.com/ait-app/ait/pull/136)、
  [PR #137](https://github.com/ait-app/ait/pull/137)、[PR #140](https://github.com/ait-app/ait/pull/140)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.12)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.12-macos-arm64.dmg`、`Ait-0.0.12-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.12-linux-x64.tar.gz` |

退出旧应用，更新完整安装包后重新打开；远端 Host 也需更新 server 才能使用新 Provider
和后台 Git 刷新。应用继续使用 Ait 的用户目录与认证。DeepSeek Harness 和 GitLab 的
`glab` 需在运行 server 的 Host 上另行安装、配置。iOS profile 是构建配置更新，本次
GitHub Release 仍只分发上述桌面平台。

## 发布验证

版本提交仅同步版本元数据、锁文件、CHANGELOG 和文档。12 个 Cargo 包、本地 path
版本约束、根 npm 包及 6 个 workspace 统一为 `0.0.12`，第三方依赖保持锁定版本。

基线 [main CI](https://github.com/ait-app/ait/actions/runs/36923952025) 已通过。
本地验证在 macOS arm64、Rust 1.98.1，基线加本次版本元数据与文档修改上执行：

- `npm ci --offline --no-audit --no-fund`：锁定依赖安装与项目补丁。
- `npm run verify:release -- v0.0.12`、`npm run verify:local-packages`：版本与本地包链接。
- Cargo/npm 锁文件语义比较：第三方依赖未变化。
- `npm run test:release`：12 passed，资源布局、错版 server、更新摘要与资产集合。
- `npm run build:desktop-main`：共享包与 Electron 主进程构建。
- `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：类型检查。
- `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 npm run test --workspace=@ait/mobile -- --project unit native-release-version.test.ts src/changelog`：
  3 文件、46 passed，原生版本与应用内发布说明。首次默认中文环境运行有 1 项既有测试
  因固定期待英文日期失败；英文环境重跑通过，未修改运行代码或测试。
- `cargo metadata --locked --offline --no-deps`、锁定依赖离线 `server` 构建与版本检查：
  Cargo workspace 与单 server 资源暂存。
- `cargo fmt --all --check`、改动 JSON/Markdown 格式与 `git diff --check`。

上述最终检查全部通过。离线 server 构建命令为：

```sh
CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target \
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib \
cargo build --locked --offline -p daemon --bin daemon
/private/tmp/ait-git-fetch-target/debug/daemon --version
AIT_SERVER_BIN=/private/tmp/ait-git-fetch-target/debug/daemon node apps/desktop/scripts/prepare-server.mjs
```

构建版本输出 `server 0.0.12`，资源暂存只包含该 server。

本地 debug server 仅用于验证。正式版本先经 PR 合并到 main 并通过 CI，再创建标签；
Release Ait 工作流在两个原生平台构建、验证资源布局及成品启动，macOS 完成签名与
Apple 公证。两个平台均成功后发布四个安装包、两个 macOS blockmap、两个更新 YAML、
`BUILD-INFO.json` 和 `SHA256SUMS`。发布后核对标签来源、完整资产集合、SHA-256 与
更新文件摘要，实际状态及构建链接以 Release 页面为准。
完整步骤见[发布指南](../../../operations/releasing.md)。

## Test coverage

**未测量行覆盖率**：本次只改版本元数据与文档，没有 Rust 或前端运行代码变更，按仓库
规则跳过本地 Rust 测试和覆盖率；测试通过数量不作为覆盖率百分比。原功能范围与历史
覆盖率见 [DeepSeek 验证报告](../providers/deepseek-harness-acp.md)、[侧边栏报告](../workspace/workspace-sidebar-runtime.md)
和 [Git 刷新报告](../workspace/background-git-fetch.md)，不将历史测量标记为本次发布测量。
后续行为变更应补充定向测试，涉及 Rust 源码的提交再运行完整测试与覆盖率。
