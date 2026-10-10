# Ait 0.0.25 发布说明

日期：2026-10-10（Asia/Shanghai）。发布准备基于 main `2ce787fe686a2a8cb96a6bfba5e3c4dd5e3d2c2d`。
最终源码以不可变标签 `v0.0.25` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

- **OpenCode 官方 ACP。** 原生问答、权限审批、取消、会话恢复与模型选择统一使用
  `opencode acp`，保留原生认证、工具和历史；完整工具结果留在原生 transcript。
  见 [PR #243](https://github.com/ait-app/ait/pull/243)。
- **用量与 Codex 速度。** 增加原生账号用量卡片、固定窗口、已用/剩余显示、主机选择与刷新。
  Codex 按原生模型目录提供 Normal、Fast、Ultrafast 等速度，选择 Normal 清除上一轮速度。
  见 [PR #248](https://github.com/ait-app/ait/pull/248)。
- **聊天与桌面体验。** 稳定聊天阅读位置、搜索及 Markdown block 身份，改进选区复制、图片尺寸、
  文件链接与内容宽度设置；统一 Explorer/Workspace 标签并改善子 Agent 分栏、上下文详情与叠层。
  修复窗口拖动、浏览器自动化目标、缩放后截图坐标、重连同步及缓存写失败后的忙循环。
  新增 Vue 高亮并修复 Astro 解析。见 [PR #248](https://github.com/ait-app/ait/pull/248)。
- **终端主题与内部整理。** OpenCode 终端配色跟随桌面主题；收口摘要契约、隔离目录读取预算、
  整理 crate 可见性并移除未使用的持久化监听。
  见 [PR #246](https://github.com/ait-app/ait/pull/246)、
  [PR #247](https://github.com/ait-app/ait/pull/247)、
  [PR #239](https://github.com/ait-app/ait/pull/239)。

## 安装与更新

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.0.25)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.25-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.25-macos-arm64.dmg`、`Ait-0.0.25-macos-arm64.zip` |

退出旧桌面应用后安装，或使用正式通道自动更新。稳定版自动更新摘要为
`latest-linux.yml` 和 `latest-mac.yml`。正式工作流负责 macOS 签名、公证与两个平台的
成品 daemon 生命周期验证，成功后创建 Release 并上传校验和。

Android APK、Google Play、iOS TestFlight 和 AUR 二进制包继续使用各自独立流程。
根 `PKGBUILD` 源码配方同步为 `0.0.25-1`。移动端原生改进需要对应平台另行构建发布。

## 发布准备验证

- `npm ci --no-audit --no-fund`：通过，第三方 npm/Cargo 锁定依赖未改变，lockfile 保留既有格式。
- `npm run verify:release -- v0.0.25`、`npm run verify:local-packages`：通过。
  13 个本地 Cargo 包、根 npm 包和 6 个 npm workspace 的版本一致。
- `npm run test:release`：45 passed；`npm run test:mobile-release`：40 passed。
- `npm run build:desktop-main`、`CI=1 EXPO_NO_TELEMETRY=1 npm run build:desktop-assets`、
  `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- 变更 JSON、Markdown 的 Oxfmt 检查：通过；`npm run check:docs` 等价命令通过，
  检查 295 个 Markdown 文档的本地链接。
- `cargo metadata --locked --offline --no-deps --format-version 1`、
  `cargo metadata --locked --offline --format-version 1`、`cargo fmt --all --check`、
  `bash -n PKGBUILD`、`git diff --check`：通过。本次没有在 Arch Linux 构建安装包。
- 本地环境：macOS arm64、Node 26.10.0、npm 11.19.1、Cargo 1.98.1；
  正式流水线使用既有 Node 24 / Rust 1.98.1 配置。

准备期间发现基线 main [CI](https://github.com/ait-app/ait/actions/runs/38009285494)
的文件订阅集成测试收到文件截断过程中的空内容，期望最终大小 18 而实际为 0。
该测试此前已存在，格式、Clippy 与 UI 检查通过；发布准备 PR 的 CI 将再次验证相同源码行为。

远端构建结果以 GitHub Actions 和 Release 资产为准，版本准备完成不代表安装包已发布。

## Test coverage

**Not measured。** 本次仅同步版本、锁文件与发布文档，没有修改 `bins/` 或 `crates/`
的 Rust 源码，按仓库规则跳过本地 Rust 测试及覆盖率重测。发布相关测试结果单独记录，
不视为覆盖率百分比；合并后由现有 CI 和 Linux/macOS 正式成品门禁验证最终发布源码。
