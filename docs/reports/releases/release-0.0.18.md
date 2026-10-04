# Ait 0.0.18 发布说明

日期：2026-10-05（Asia/Shanghai）。本次准备基于最新 `main` 的
`c5fef988002f7f26009e1c7d9d5b1cb38104a04c`；正式源码以合并后的不可变标签
`v0.0.18` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **Codex 截图时间线。** 过滤计算机操作截图中冗长的内嵌图片元数据，避免耗尽时间线行数、
  挤掉后续回复；保持截图与原生会话恢复。见 [PR #173](https://github.com/ait-app/ait/pull/173)。
- **在线服务与主机同步。** 分离应用账户登录与每台主机的同步开关，增加独立的主机控制和
  中继租约。见 [PR #175](https://github.com/ait-app/ait/pull/175) 与
  [ADR-083](../../decisions/clients/adr-083-online-service-host-sync.md)。
- **工作区 Git 操作。** 新增重置工作区到 origin 最新默认分支的操作，并按改动、提交、推送、
  PR 和归档状态引导主按钮。重置前明确提示覆盖本地改动。见
  [PR #176](https://github.com/ait-app/ait/pull/176) 与
  [验证报告](../workspace/workspace-git-actions-pr-validation-2026-10-05.md)。
- **iOS 在线主机支持。** 源码加入账户登录、主机发现、中继连接、安全凭据存储和原生下载；
  安装包由独立的 TestFlight 工作流发布。见 [PR #177](https://github.com/ait-app/ait/pull/177)
  与 [ADR-084](../../decisions/clients/adr-084-ios-account-relay.md)。
- **许可证与测试。** Ait 使用 Apache-2.0 许可证；更新品牌文案与本地化日期测试。
  见 [PR #178](https://github.com/ait-app/ait/pull/178) 与
  [PR #179](https://github.com/ait-app/ait/pull/179)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.18)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.18-macos-arm64.dmg`、`Ait-0.0.18-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.18-linux-x64.tar.gz` |

退出旧桌面应用，安装新版本后重新打开；远端 Host 也需升级 daemon 才能获得服务端改进。
Android APK、Google Play 和 iOS TestFlight 使用独立的手动发布流程。

## 发布验证

13 个 Cargo 包、根 npm 包和 6 个 workspace 同步到 `0.0.18`；Cargo/npm 锁文件仅更新本地包版本。
本次版本与文档准备未修改 Rust 源码；按仓库规范跳过本地 Rust 测试与覆盖率重测。

在 macOS arm64 上，以 `c5fef988` 为基线完成以下准备检查：

- `npm ci --offline --no-audit --no-fund`：通过。
- `npm run verify:release -- v0.0.18`、`npm run verify:local-packages`：通过。
- `npm run test:release`：18 passed；`npm run test:mobile-release`：32 passed。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- `npm run check:docs`、Oxfmt、`cargo metadata --locked --offline --no-deps --format-version 1`、
  `cargo fmt --all --check`、`git diff --check`：通过。

最新 [main CI](https://github.com/ait-app/ait/actions/runs/37238138136) 已通过。
正式 Linux/macOS 安装包须由 `Release Ait` 工作流构建、签名、公证、验证并上传；
发布后核对资产、SHA-256、更新摘要和 `BUILD-INFO.json`。

## Test coverage

本次没有新的覆盖率测量。已合入功能的覆盖率只对应各自报告注明的源码、平台和工具链，
不代表最终标签的重新测量。工作区 Git 操作的 Rust workspace 行覆盖率为
49,427/52,321（94.47%），详见
[验证报告和可审查统计](../workspace/workspace-git-actions-pr-validation-2026-10-05.md)；
在线服务与主机同步的 Rust workspace 行覆盖率为 49,214/52,090（94.48%），详见
[验证报告和可审查统计](../clients/online-service-host-sync-validation.md)。
两次测量各忽略 3 项需要真实 Provider CLI 或认证的既有测试。
