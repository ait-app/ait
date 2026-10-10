# Ait 0.0.26 发布说明

日期：2026-10-10（Asia/Shanghai）。发布准备基于 main `5d6a9e9dfa583ad4f3903267cf38099595b18e64`。
最终源码以不可变标签 `v0.0.26` 和发布资产 `BUILD-INFO.json` 为准。

## 更新内容

- **终端崩溃修复。** 修复宽字符在缩窄终端后被裁剪、后续写入或清行时引发的 panic，
  同时覆盖单行、单列终端与截图恢复。沿用已合并的固定 vt100 上游修复提交。
- **启动请求突发。** 每个能力组允许有界的 16 个入站请求，接纳正常启动读取突发；
  超量请求继续拒绝，Ping 保持独立响应。见
  [ADR-120](../../decisions/daemon/adr-120-single-connection-startup-admission.md)。
- **OpenCode 目录校验。** 模型没有默认思考选项时省略对应字段，避免 Provider snapshot
  和主机诊断因不接受 `null` 而校验失败。

上述修复均来自 [PR #250](https://github.com/ait-app/ait/pull/250)。复现、源码验证和观察范围见
[daemon 修复报告](../daemon/daemon-startup-panic-fix-2026-10-10.md)。

## 安装与更新

[正式版下载页](https://github.com/ait-app/ait/releases/tag/v0.0.26)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.26-linux-x64.tar.gz` |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.26-macos-arm64.dmg`、`Ait-0.0.26-macos-arm64.zip` |

退出旧桌面应用后安装，或使用正式通道自动更新。正式工作流完成 macOS 签名、公证及两个平台的
成品 daemon 生命周期验证后，上传安装包、`latest-linux.yml`、`latest-mac.yml`、
`BUILD-INFO.json` 与 `SHA256SUMS`。

根 `PKGBUILD` 源码配方同步为 `0.0.26-1`。Android APK、Google Play、iOS TestFlight
和 AUR 二进制包使用各自独立流程。

## 发布准备验证

版本与锁文件仅更新 13 个本地 Cargo 包、根 npm 包及 6 个 npm workspace，第三方依赖沿用
main 中已锁定的版本，包括终端修复使用的 vt100 Git 提交。

- `npm ci --no-audit --no-fund`：通过，保留 lockfile 既有格式。
- `npm run verify:release -- v0.0.26`、`npm run verify:local-packages`：通过。
- `npm run test:release`：45 passed；`npm run test:mobile-release`：40 passed。
- `npm run build:desktop-main`、
  `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- 变更 JSON、Markdown 的 Oxfmt 检查、`npm run check:docs`：通过，
  检查 298 个 Markdown 文档的本地链接。
- `cargo metadata --locked --offline --format-version 1`、`cargo fmt --all --check`、
  `bash -n PKGBUILD`、`git diff --check`：通过。
- 本地环境：macOS arm64、Node 26.10.0、npm 11.19.1、Cargo 1.98.1；
  正式流水线沿用 Node 24 / Rust 1.98.1。没有在本机进行 Arch 安装包验证。

远端构建、签名、公证和 Web 导出结果以 GitHub Actions 和 Release 资产为准，
版本准备完成不代表安装包已发布。

## Test coverage

**Not measured。** 本次仅同步版本、锁文件与发布文档，没有修改 `bins/` 或 `crates/`
的 Rust 源码，按仓库规则跳过本地 Rust 测试及覆盖率重测。PR #250 的普通测试和覆盖率限制
见上述修复报告；发布准备测试不视为覆盖率百分比。
