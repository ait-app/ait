# Ait 0.0.24 发布说明

日期：2026-10-09（Europe/London）。发布准备基于 main `938a98f548e216cacf9b7caecee62383d4a27442`。
最终源码以不可变标签 `v0.0.24` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

- **OpenCode 原生权限。** 当前会话支持 Allow / Ask / Deny，未选择时继承原生规则；
  权限写入后回读核对，异常时阻止提交。Build / Plan 保留原生 agent 语义，权限按钮与菜单使用
  对应图标。见 [PR #241](https://github.com/ait-app/ait/pull/241)、
  [PR #242](https://github.com/ait-app/ait/pull/242)。
- **OpenCode 会话兼容。** 收口 v1/v2 私有协议差异，恢复保存模型偏好，区分原生权限拒绝与主动
  取消，并保留具体错误原因和 HTTP 状态码。见 [PR #231](https://github.com/ait-app/ait/pull/231)、
  [PR #228](https://github.com/ait-app/ait/pull/228)。
- **故障证据导出。** 后台采集有界、脱敏的错误证据及原生 Harness 日志，支持文本附件下载与分享。
  见 [使用说明](../../operations/diagnostics.md)、[PR #227](https://github.com/ait-app/ait/pull/227)。
- **模型选择与草稿重试。** 隐藏未安装的 Provider；创建失败后修改配置可重新发送，切换页面仍等待
  已发出的创建请求，并修复 PDF 类型识别。见 [PR #226](https://github.com/ait-app/ait/pull/226)、
  [PR #228](https://github.com/ait-app/ait/pull/228)。
- **Codex 与 Antigravity。** 历史重载后保持 Codex 异步问题答案顺序；解释 Antigravity headless
  权限拒绝与原生失败，结算受影响工具。见 [PR #233](https://github.com/ait-app/ait/pull/233)、
  [PR #234](https://github.com/ait-app/ait/pull/234)。
- **iOS 终端中文输入。** UIKit 管理输入法组合文本，仅提交确认后的 Unicode。
  该原生修复需要另行重建并发布 iOS 应用，桌面标签不会触发 TestFlight。
  见 [PR #238](https://github.com/ait-app/ait/pull/238)。
- **内部能力归属整理。** 独立持久化适配器，收口共享业务值与服务端协议，按能力组织 filesystem。
  见 [PR #235](https://github.com/ait-app/ait/pull/235)、[PR #237](https://github.com/ait-app/ait/pull/237)。

## 安装与更新

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.0.24)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.24-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.24-macos-arm64.dmg`、`Ait-0.0.24-macos-arm64.zip` |

退出旧桌面应用后安装，或使用正式通道自动更新。稳定版自动更新摘要为
`latest-linux.yml` 和 `latest-mac.yml`。正式工作流负责 macOS 签名、公证和两个平台的
成品 daemon 生命周期验证，成功后自动创建 Release 并上传校验和。

Android APK、Google Play、iOS TestFlight 和 AUR 二进制包使用各自的独立流程。
本次同步根 `PKGBUILD` 源码配方版本为 `0.0.24-1`。

## 发布准备验证

- `npm ci --no-audit --no-fund`：通过，第三方锁定依赖未改变。
- `npm run verify:release -- v0.0.24`、`npm run verify:local-packages`：通过。
  13 个本地 Cargo 包、根 npm 包和 6 个 npm workspace 的版本一致。
- `npm run test:release`：45 passed；`npm run test:mobile-release`：40 passed。
- `npm run build:desktop-main`、`CI=1 EXPO_NO_TELEMETRY=1 npm run build:desktop-assets`、
  `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- 变更 JSON、Markdown 的 Oxfmt 检查：通过；package-lock 保留既有格式。
- `npm run check:docs`：通过，检查 286 个 Markdown 文档的本地链接。
- `cargo metadata --locked --offline --no-deps --format-version 1`、
  `cargo metadata --locked --format-version 1`、`cargo fmt --all --check`、
  `bash -n PKGBUILD`、`git diff --check`：通过。完整 metadata 首次离线检查因缺少
  Windows 平台依赖缓存失败，下载锁定的 `winapi-util 0.1.11` 后通过，锁文件未改变。
- 本地环境：macOS arm64，Node 22.23.1、npm 10.9.8、Cargo 1.97.0。
  正式流水线使用既有 Node 24 / Rust 1.98.1 配置；本次没有在 Arch Linux 构建安装包。

本次只同步版本、锁文件与发布文档。远端发布结果以 GitHub Actions 和 Release 资产为准，
不把版本准备完成视为安装包已发布。

## Test coverage

**Not measured。** 本次没有修改 `bins/` 或 `crates/` 的 Rust 源码，按仓库规范跳过
Rust 测试和覆盖率重测。发布相关测试的通过数量单独记录，不视为覆盖率百分比。
合并后由现有 CI 与正式 Linux/macOS 成品门禁验证最终发布源码。
