# Ait 0.0.19 发布说明

日期：2026-10-05（Asia/Shanghai）。本次准备基于最新 `main` 的
`34dd1a8a0164b28168051cfe579282e0aaf734a3`；正式源码以合并后的不可变标签
`v0.0.19` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **Daemon 同步注册。** 发布节点使用 daemon 的稳定身份，重新开启同步、daemon 重启或
  另一客户端接管时复用同一节点与 Host；注册 ID 已关闭或过期时，下次同步重试使用新 ID。
  见 [PR #183](https://github.com/ait-app/ait/pull/183) 与
  [ADR-085](../../decisions/clients/adr-085-stable-daemon-publication.md)。
- **桌面 IPC 命名。** preload 调用和主进程 handler 同步使用 `ait:invoke`，替换导入的
  Paseo 通道名称。注册请求的服务端错误原因仍由在线服务返回。
- **工作区重置。** 按路径组件比较保存路径与请求路径，避免末尾分隔符差异造成
  `Workspace identity or initial branch mismatch`。见
  [PR #182](https://github.com/ait-app/ait/pull/182) 与
  [验证报告](../workspace/workspace-reset-path-pr-validation-2026-10-05.md)。
- **iOS E2E 验证。** 同步模拟器与 Web 下载行为的测试契约。见
  [PR #181](https://github.com/ait-app/ait/pull/181)。

## 在线服务兼容条件

已有随机或旧客户端节点绑定，需要配套部署 `ait-app/ait-server` 的旧绑定兼容修复；
单独更新桌面不能迁移这些记录。已有活动 runtime 租约须先停止同步或等待到期才能接管。

删除或禁用旧 Host 后，旧客户端绑定导致的
`403 / Insufficient permission or inactive account` 发生在密码登录后的节点注册。
配套服务端修复会解除纯客户端的失效绑定并撤销旧 runtime 租约，保留 Host 删除/禁用状态。
该登录修复只需更新服务端。服务端源码目前位于本地 `Documents/ait-server`，
尚未提交或部署，不包含在本次桌面安装包中。
详细结果见 [daemon 注册验证报告](../clients/daemon-registration-validation.md)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.19)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.19-macos-arm64.dmg`、`Ait-0.0.19-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.19-linux-x64.tar.gz` |

退出旧桌面应用，安装新版本后重新打开。Android APK、Google Play 和 iOS TestFlight 使用
独立的手动发布流程。

## 发布验证

13 个 Cargo 包、根 npm 包和 6 个 workspace 同步到 `0.0.19`；Cargo/npm 锁文件仅更新本地包版本。
本次版本与文档准备未修改 Rust 源码；按仓库规范跳过本地 Rust 测试与覆盖率重测。

在 macOS arm64 上，以 `34dd1a8a` 为基线完成以下准备检查：

- `npm ci --offline --no-audit --no-fund`：通过。
- `npm run verify:release -- v0.0.19`、`npm run verify:local-packages`：通过。
- `npm run test:release`：18 passed；`npm run test:mobile-release`：32 passed。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- `npm run check:docs`、变更清单与文档的 Oxfmt、
  `cargo metadata --locked --offline --no-deps --format-version 1`、
  `cargo fmt --all --check`、`git diff --check`：通过。

最新 [main CI](https://github.com/ait-app/ait/actions/runs/37244909937) 已通过。
正式安装包由 `Release Ait` 工作流构建、签名、公证、成品启动验证并上传；
发布后核对资产、SHA-256、更新摘要和 `BUILD-INFO.json`。

## Test coverage

本次版本与文档改动没有新的行覆盖率测量，不改变 Rust 行为。历史测量只对应各报告注明
的源码和范围，不代表最终标签重新测量。工作区重置修复的 workspace 行覆盖率为
94.48%（49,431 / 52,321），见
[验证报告与可审查统计](../workspace/workspace-reset-path-pr-validation-2026-10-05.md)。
Daemon 注册的 39 项客户端测试通过，TypeScript 行覆盖率未测量；配套服务端 7 项定向
测试通过，注册模块行覆盖率为 92.31%（264 / 286），包在该组测试下为
43.60%（1,118 / 2,564）。精确命令、修订、文件哈希、过滤范围及未覆盖行为见
[注册验证报告](../clients/daemon-registration-validation.md)与
[覆盖率摘要](../clients/daemon-registration-coverage.json)。服务端无可比覆盖率基线，
完整覆盖率留待其提交准备。本次桌面发布不新增服务端源码提交。
