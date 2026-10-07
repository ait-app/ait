# Ait 0.0.23-beta.1 发布说明

日期：2026-10-08（Asia/Shanghai）。发布准备基于 main `e41e7ab1da5fd56744744056346754f2f489abd3`。
最终源码以不可变标签 `v0.0.23-beta.1` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

这是 0.0.22 之后的首个桌面 beta，包含最新 main 的更新：本机传输断开后重连、流式
Markdown 空白保留、Composer 模型选择与延迟切换提示、PDF 预览及外部文件打开、目录
附件、分块消息与带背压的文件上传、登录后的内置 daemon 同步、工作区 fork 与无初始
Agent 创建、Provider 辅助生成及原生会话兼容性，以及 crate 服务和持久化边界整理。
另包含独立的 Google Play 内部测试流水线；本次 beta 标签只发布桌面。

发布工具接受稳定版和编号 beta；Electron 使用版本推导的 beta 更新摘要，GitHub
创建和修复 Release 均设置预发布且不成为 Latest。安装包和摘要仍经过同样的校验、
macOS 签名、公证及真实 daemon 成品门禁。

## 安装与更新

[Beta 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.23-beta.1)。

| 平台                           | 安装包                                                                   |
| ------------------------------ | ------------------------------------------------------------------------ |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.23-beta.1-linux-x64.tar.gz`        |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.23-beta.1-macos-arm64.dmg`、`Ait-0.0.23-beta.1-macos-arm64.zip` |

退出旧桌面应用后安装，或在桌面更新设置选择 beta 通道。正式通道继续使用稳定版；
beta 资产仅包含 `beta-linux.yml` 和 `beta-mac.yml`。Android、Play、TestFlight、AUR
二进制包不随本次桌面标签发布。本地 Arch 源码配方版本为 `0.0.23beta1-1`。

## 发布准备验证

- `npm ci --no-audit --no-fund`：通过，第三方锁定版本未改变。
- `npm run test:release`：21 passed；`npm run test:mobile-release`：40 passed。
- 桌面 `auto-updater`、`app-update-service`、`app-update-rollout` 定向测试：44 passed；
  移动端 `native-release-version` 定向测试：5 passed。
- `npm run verify:release -- v0.0.23-beta.1`、`npm run verify:local-packages`：通过。
  14 个本地 Cargo 包、根 npm 包和 6 个 npm workspace 的版本一致。
- `npm run build:desktop-main`、`npm run build:desktop-assets`、desktop/mobile 类型检查：通过。
- 变更 JavaScript 的 Oxlint、变更配置与文档的 Oxfmt、`npm run check:docs`：通过。
- `cargo metadata --locked --offline --no-deps --format-version 1`、`cargo fmt --all --check`、
  `bash -n PKGBUILD`、`git diff --check`：通过。本次未在 Arch Linux 构建安装包。

PR 和 Linux/macOS 正式构建成功后才创建预发布，并下载全部资产核对 SHA-256、
自动更新 SHA-512 和 `BUILD-INFO.json`。

## Test coverage

**Not measured。** 本次仅修改发布脚本、配置、版本和文档，没有修改 `bins/` 或
`crates/` 的 Rust 源码，按仓库规范跳过本地 Rust 测试及覆盖率测量。脚本回归测试
验证真实 Electron Builder 的 stable/beta 产物命名、摘要隔离和 GitHub 创建/修复参数；
通过数量单独记录，不视为覆盖率百分比。下一步由 PR CI 和正式成品门禁验证最终源码；
新增 Rust 行为的覆盖率仍由其功能 PR 提供。
