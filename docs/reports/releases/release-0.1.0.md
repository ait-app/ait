# Ait 0.1.0 发布说明

日期：2026-10-11（Asia/Shanghai）。发布准备基于 main `22f12e0f1927b7743a88775b78d9eb8c39c54148`，
该提交也是 `v0.0.29` 的发布源码。最终构建来源以不可变标签 `v0.1.0` 和资产
`BUILD-INFO.json` 为准。

## 更新内容

本次将已发布的 0.0.29 功能升级为 0.1.0 正式版本。两个版本之间的应用行为保持一致；
完整功能说明见 [0.0.29 发布说明](release-0.0.29.md)。

- Rust workspace、13 个本地 Cargo 包、根 npm 包及 6 个 npm workspace 统一使用 `0.1.0`。
- Cargo 与 npm 锁文件只更新本地包版本，第三方依赖沿用发布准备基线的锁定条目。
- 根 `PKGBUILD` 本地源码配方同步为 `0.1.0-1`。
- 重新构建 Linux x86_64 和 macOS Apple Silicon 安装包，发布正式通道更新元数据。

## 安装与兼容性

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.1.0)。

| 平台                           | 安装包                                                    |
| ------------------------------ | --------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.1.0-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.1.0-macos-arm64.dmg`、`Ait-0.1.0-macos-arm64.zip`  |

退出旧桌面应用后安装，或使用正式通道自动更新。现有账号、工作区与配置沿用 0.0.29 的行为。
OpenCode 2.x 最低支持 `2.0.26`，1.x 保留既有支持；Ait 不替换原生 CLI，
也不改写其认证或全局配置。

官方 API 继续使用 `https://aitapp.cn:8443/api`，服务端与 Authing 配套配置由独立部署管理。
Android APK、Google Play、iOS TestFlight 与 AUR 二进制包继续使用各自的独立发布流程。

## 发布验证

本地验证通过：

- `npm ci --no-audit --no-fund`、发布版本一致性与 6 个本地 workspace 检查。
- 两份锁文件仅更新本地包版本，第三方依赖记录保持一致。
- `npm run test:release` 47 项、`npm run test:mobile-release` 41 项。
- `npm run build:desktop-main`、桌面与移动端类型检查。
- 全仓 Oxfmt 检查，共 3281 个文件；变更文件 UI lint 无适用源文件。
- 文档检查的 1 项测试及 330 份 Markdown 的本地链接验证。
- `cargo metadata --locked --offline`、`cargo fmt --all --check`、`bash -n PKGBUILD`
  与 `git diff --check`。

适用的 PR 检查与合并后的 main CI 全部成功后创建正式标签。发布工作流执行 macOS 签名、公证和
两平台成品 daemon 生命周期门禁，校验安装包与更新摘要；正式发布后核对公开资产摘要及构建来源。

## Test coverage

**Not measured。** 用户未请求覆盖率。本次仅修改版本、锁文件和发布文档，没有修改
`bins/` 或 `crates/` 的 Rust 源码，按仓库规则跳过本地 Rust 测试与覆盖率测量。
仓库既有 CI 门禁继续自动执行；历史功能测试和覆盖率不代表本次发布源码的覆盖率。
如后续请求测量，按目标功能选择相关 crate 和定向测试并提供可共享报告；工作区覆盖率另需明确请求。
