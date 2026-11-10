# Ait 0.0.14 发布说明

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-10-03（Asia/Shanghai）。基于 main `3ab58e57cd55a9140b4a6ef7f11bcdf1774e2116`
准备，正式来源以不可变标签 `v0.0.14` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **Diff 语法高亮与解析修复。** 工作区、base 和 commit Diff 支持按主题展示语法颜色，
  保留多行注释、字符串和 rename 两侧文件上下文；源码中的 `diff --git` 字符串不会再
  被误识别成文件头，避免出现假文件项。超出解析或输出预算时保留文本、省略高亮。
  [PR #147](https://github.com/ait-app/ait/pull/147)、[实施报告](../workspace/diff-syntax-highlighting.md)、
  [提交验证](../workspace/diff-pr-validation.md)。
- **侧边栏统计归零。** 增删行数与 base Diff 复用已获取的 origin 基准，修复远端已包含
  工作区提交、本地 main 尚未同步时仍显示旧增删统计的问题；未提交与未跟踪改动继续
  正常计数。[PR #148](https://github.com/ait-app/ait/pull/148)、
  [验证报告](../workspace/workspace-sidebar-runtime.md#合并后-diff-归零修复2026-10-02)。
- **Codex 推理等级。** 服务端接受 `max` 和 `ultra`，支持新建、恢复和发送任务时传递
  这些等级；实际可用选项仍由本机 Codex 与模型提供。
  [PR #149](https://github.com/ait-app/ait/pull/149)。
- **服务版本与项目链接。** 握手信息包含正在运行的 server 版本；文档、反馈、社区、
  Release 和桌面自动更新链接统一指向 `ait-app/ait`。
  [PR #147](https://github.com/ait-app/ait/pull/147)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.14)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.14-macos-arm64.dmg`、`Ait-0.0.14-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.14-linux-x64.tar.gz` |

退出旧应用，更新完整安装包后重新打开。远端 Host 也需更新 server 才能获得 Diff 高亮、
统计和 Codex 推理等级修复。继续使用 Ait 用户目录与认证。
本次 GitHub Release 分发桌面安装包；移动端测试发布按独立流程手动触发。

## 发布验证

版本提交只同步元数据、锁文件、CHANGELOG 和文档。12 个 Cargo 包及本地 path 约束、
根 npm 包和 6 个 workspace 统一为 `0.0.14`；第三方依赖保持最新 main 的锁定版本。
基线 [main CI](https://github.com/ait-app/ait/actions/runs/37015310879) 已通过。

本地验证范围为 macOS arm64、Rust 1.98.1、上述基线加本次发布元数据与文档修改：

- `npm ci --offline --no-audit --no-fund`：干净依赖安装与项目补丁。
- `npm run verify:release -- v0.0.14`、`npm run verify:local-packages`：版本和本地包链接。
- Cargo/npm 锁文件语义比较：第三方依赖未变。
- `npm run test:release`：12 passed；`npm run test:mobile-release`：16 passed。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/app`：
  共享包、桌面主进程构建与类型检查。
- `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 npm run test --workspace=@ait/app -- --project unit native-release-version.test.ts src/changelog`：
  3 文件、46 passed；英文环境与已有日期断言一致。
- `cargo metadata --locked --offline --no-deps`、锁定依赖离线 `server` 构建、版本检查及单 server 资源暂存。
- `cargo fmt --all --check`、改动 JSON/Markdown 格式、文档链接与 `git diff --check`。

上述本地检查全部通过。离线 server 构建和暂存的实际命令：

```sh
CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target \
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib \
cargo build --locked --offline -p server-bin --bin server
/private/tmp/ait-git-fetch-target/debug/server --version
AIT_SERVER_BIN=/private/tmp/ait-git-fetch-target/debug/server node apps/paseo/scripts/prepare-server.mjs
```

版本输出 `server 0.0.14`，资源暂存只包含该 server。

本地 debug server 只用于验证，正式资产由 Release Ait 工作流生成。版本 PR 和 main CI
通过后创建不可变标签，在 Linux x86_64/macOS arm64 原生 runner 构建并验证成品启动；
macOS 必须完成签名与 Apple 公证。双平台全部成功后才上传四个安装包、两个 macOS
blockmap、两个更新 YAML、`BUILD-INFO.json` 和 `SHA256SUMS`。
发布后下载全部资产，核对 SHA-256、更新文件 SHA-512/大小及标签与构建来源。
实际发布状态和构建链接以 Release 页面为准；完整步骤见[发布指南](../../../operations/releasing.md)。

## Test coverage

**未测量行覆盖率**：本次发布提交只改版本元数据与文档，没有 Rust 或前端运行代码变更，
按仓库规则跳过本地 Rust 测试和覆盖率。测试通过数量不作为覆盖率百分比。
Diff、侧边栏统计和 Codex 推理等级的历史验证分别见 [Diff PR 报告](../workspace/diff-pr-validation.md)、
[侧边栏报告](../workspace/workspace-sidebar-runtime.md)及[Codex 覆盖率证据](../providers/codex-reasoning-options-coverage.json)，
不将历史测量标记为本次发布结果。后续行为变更补充定向测试，Rust 源码提交准备时再运行
完整测试与覆盖率。
