# Ait 0.0.23 发布说明

日期：2026-10-08（Asia/Shanghai）。发布准备基于 main `9fadd2d11c3529161c238b4ef94bc10013118da8`。
最终源码以不可变标签 `v0.0.23` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

将 0.0.23-beta.1 的桌面改进发布到正式通道，包括本机连接失败后的重连、流式 Markdown
空白保留、Composer 模型选择和延迟切换提示、PDF 预览、外部文件打开、目录附件、
分块消息与带进度和背压的文件上传、登录后的内置 daemon 同步、工作区 fork 和无初始
Agent 创建，以及 Provider 辅助生成和原生会话兼容性改进。

在 beta 基础上移除主机设置中已废弃的“配对设备”区域。main CI 自动发布带提交 hash
和日期的 nightly，PR CI 提供 Linux/macOS 测试安装包。

## 安装与更新

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.0.23)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.23-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.23-macos-arm64.dmg`、`Ait-0.0.23-macos-arm64.zip` |

退出旧桌面应用后安装，或使用正式通道自动更新。稳定版自动更新摘要为
`latest-linux.yml` 和 `latest-mac.yml`。正式工作流负责 macOS 签名、公证和两个平台的
成品 daemon 生命周期验证，成功后自动创建 Release 并上传校验和。

Android APK、Google Play、iOS TestFlight 和 AUR 二进制包使用各自的独立流程。
本次同步根 `PKGBUILD` 源码配方版本为 `0.0.23-1`。

## 发布准备验证

- `npm ci --no-audit --no-fund`：通过，第三方锁定依赖未改变。
- `npm run verify:release -- v0.0.23`、`npm run verify:local-packages`：通过。
  14 个本地 Cargo 包、根 npm 包和 6 个 npm workspace 的版本一致。
- `npm run test:release`：42 passed；`npm run test:mobile-release`：40 passed。
- `npm run build:desktop-main`、`npm run build:desktop-assets`、desktop/mobile 类型检查：通过。
- 变更 JSON、Markdown 的 Oxfmt 检查：通过。
- `npm run check:docs`：通过，检查 270 个 Markdown 文档的本地链接。
- `cargo metadata --locked --offline --no-deps --format-version 1`、`bash -n PKGBUILD`、
  `git diff --check`：通过。本次没有在 Arch Linux 构建安装包。

本次只同步版本、锁文件与发布文档。远端发布结果以 GitHub Actions 和 Release 资产为准，
不把版本准备完成视为安装包已发布。

## Test coverage

**Not measured。** 本次没有修改 `bins/` 或 `crates/` 的 Rust 源码，按仓库规范跳过
Rust 测试和覆盖率重测。发布相关测试的通过数量单独记录，不视为覆盖率百分比。
