# Ait 0.0.16 发布说明

日期：2026-10-04（Asia/Shanghai）。本次准备基于 `main` 的
`151ce454867d915b5f80823463d9b8eafd0283ae`；正式源码以不可变标签 `v0.0.16`
和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **工作区与 Agent 目录更新。** 连接中的客户端可及时收到目录、会话和运行状态变更，
  减少依赖轮询产生的延迟。[PR #166](https://github.com/ait-app/ait/pull/166)。
- **Codex 与 Agent 稳定性。** 显示 Codex 模型发现和容量错误，继续排除隐藏模型；
  MCP 工具调用按 `server.tool` 显示，分别呈现参数、结果和原生错误；修复任务预算竞争下的
  Agent 创建，并隔离后台 Git fetch 失败。
  [PR #164](https://github.com/ait-app/ait/pull/164)、
  [PR #162](https://github.com/ait-app/ait/pull/162)、
  [PR #167](https://github.com/ait-app/ait/pull/167)。
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

包含此前功能变更的 [main CI](https://github.com/ait-app/ait/actions/runs/37164338448)
已通过。本分支另增补 Codex MCP 工具调用显示修复，其提交前验证见下节。
正式 Linux/macOS 安装包仍须由 `Release Ait` 工作流构建、签名、公证、验证并上传；
发布后核对资产、SHA-256、更新摘要和 `BUILD-INFO.json`。

## Test coverage

增补的 Codex MCP 显示修复以 `ec2ca859` 为基线；准确的 Rust 源码哈希、命令、
逐范围数据和验证限制见[本次可审阅覆盖率记录](../providers/codex-mcp-tool-call-coverage.json)。
在 macOS arm64、Rust 1.98.1、默认 features 下，先运行
`cargo llvm-cov clean --profraw-only --offline` 清除旧采样，再运行
`RUST_TEST_THREADS=1 CARGO_INCREMENTAL=0 cargo llvm-cov --workspace --html --no-clean --offline --no-fail-fast -j2`，
最后导出 JSON 摘要。13 个 Cargo workspace 包的行覆盖率为 **49,050/51,906（94.4977%）**，
其中 `provider` 为 **21,325/22,701（93.9386%）**；使用 cargo-llvm-cov 默认文件过滤，
无额外排除，doctests 未插桩。相比最近的
[目录推送覆盖率记录](../workspace/workspace-change-push-coverage.json)，workspace 变化
约 -0.0003 个百分点；中间还有其他源码变更，不能将差值全部归因于此修复。

测试执行结果单独统计：最终单线程覆盖率运行 **1,771 passed、0 failed、3 ignored**。
3 个 ignored 测试需要本地 CLI 或真实认证。此前沙箱运行无法绑定测试监听端口，
并行插桩运行有一个目录流测试超时；该测试单独运行及最终单线程完整运行均通过。
HTML 报告位于本地 `target/llvm-cov/html/index.html`，可共享的统计与源码指纹已写入上述
JSON。真实认证 Codex MCP 调用和 GitHub 连接器、Linux、Windows 未在本次运行中覆盖。
