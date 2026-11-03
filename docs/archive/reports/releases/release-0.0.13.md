# Ait 0.0.13 发布说明

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-10-02（Asia/Shanghai）。基于 main `90e8fb9117070d7a847cb485f3bd147f91e81ffc`
准备，正式来源以不可变标签 `v0.0.13` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **DeepSeek 模型发现修复。** Harness 未提供模型描述或默认推理等级时，server 省略
  对应字段，客户端兼容旧 server 返回的空描述，避免 Provider 快照校验失败。
  保留描述字段的可选类型，修复最新 main 的 SDK/桌面构建失败。
  [PR #144](https://github.com/ait-app/ait/pull/144)。
- **移动端手动发布流程。** 新增稳定桌面 Release 之后手动触发的 iOS TestFlight
  工作流，复用 EAS 托管凭据，检查重复构建并支持显式重试。Android `ait` 提交 profile
  固定为 Google Play Internal testing；遗留移动发布流程改为手动触发。
  [PR #145](https://github.com/ait-app/ait/pull/145)、
  [操作指南](../../../operations/releasing.md)、[ADR-070](../../../decisions/clients/adr-070-ios-testflight-release.md)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.13)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.13-macos-arm64.dmg`、`Ait-0.0.13-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.13-linux-x64.tar.gz` |

退出旧应用，更新完整安装包后重新打开；远端 Host 需更新 server 才能获得服务端模型
元数据修复。用户目录与认证继续沿用。此次 GitHub Release 分发桌面平台安装包，
移动端工作流需要独立触发。

## 发布验证

版本准备同步版本元数据、锁文件、CHANGELOG 和文档，并修复协议 schema 的可选字段
类型及添加缺失描述的回归测试。12 个 Cargo 包及本地 path
版本约束、根 npm 包与 6 个 workspace 统一为 `0.0.13`，第三方依赖保持锁定版本。
本地验证在 macOS arm64、Rust 1.98.1 上执行：

- `npm ci --offline --no-audit --no-fund --loglevel warn`：锁定依赖安装与项目补丁。
- `npm run verify:release -- v0.0.13`、`npm run verify:local-packages`：版本与本地包链接。
- Cargo/npm 锁文件语义比较：第三方依赖未变化。
- `npm run test:release`：12 passed；`npm run test:mobile-release`：16 passed。
- `npm run build:desktop-main`、
  `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：SDK、桌面构建与类型检查。
- `npm run test --workspace=@ait/protocol`：67 文件、749 passed，包括缺失描述和空描述回归。
- `npm run test --workspace=@ait/client`：8 文件、216 passed。
- `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 npm run test --workspace=@ait/mobile -- --project unit native-release-version.test.ts src/changelog`：
  3 文件、46 passed，原生版本与应用内发布说明。
- `cargo metadata --locked --offline --no-deps --format-version 1`、`cargo fmt --all --check`。
- 锁定依赖离线构建 `daemon`，`server --version` 输出 `server 0.0.13`；
  `AIT_SERVER_BIN` 资源暂存验证通过，仅包含该 server。
- 改动 TypeScript 的 `oxlint`、改动 JSON/Markdown/TypeScript 的格式检查与 `git diff --check`。

server 验证命令：

```sh
CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target \
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib \
cargo build --locked --offline -p daemon --bin daemon
/private/tmp/ait-git-fetch-target/debug/daemon --version
AIT_SERVER_BIN=/private/tmp/ait-git-fetch-target/debug/daemon node apps/desktop/scripts/prepare-server.mjs
```

基线 [main CI](https://github.com/ait-app/ait/actions/runs/36957170240) 在 SDK 构建时报
模型 `description` 可选类型错误，本次 schema 修复已通过同一构建命令。
正式发布仍须等待版本 PR 与合并后的 main CI 全部通过。

正式版本经 PR 合并到 main 并通过 CI 后创建标签。Release Ait 工作流在两个原生平台
构建、验证资源布局与成品启动，macOS 完成签名和 Apple 公证。两平台均成功后发布
安装包、blockmap、更新 YAML、`BUILD-INFO.json` 和 `SHA256SUMS`。发布后核对标签
来源、完整资产集合与摘要；实际状态及构建链接以 Release 页面为准。

## Test coverage

**未测量行覆盖率**：本次没有 Rust 运行代码变更，按仓库规则跳过本地 Rust 测试和
覆盖率。协议 schema 修复通过回归测试与真实 SDK/桌面构建验证，未测量前端行覆盖率。
测试通过数量不作为覆盖率百分比；功能修复的历史覆盖率见
[PR #144](https://github.com/ait-app/ait/pull/144)。后续运行代码变更按对应范围补充
测试和覆盖率测量。
