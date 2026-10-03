# Ait 0.0.15 发布说明

日期：2026-10-03（Asia/Shanghai）。基于 main `0e8142ac0340257bf5900386b6f7efbf9a1f0a9f`
准备，正式来源以不可变标签 `v0.0.15` 和 Release 的 `BUILD-INFO.json` 为准。

## 更新内容

- **OpenCode 原生 Provider。** 接入本机 `opencode serve`，支持模型发现、流式对话、工具审批、
  取消和会话恢复，修复用户输入顺序与工具历史兼容性。
  [PR #155](https://github.com/ait-app/ait/pull/155)。
- **账户与跨主机连接。** 桌面和 Android 支持账户登录、发现账户下的 Host 和反向中继连接。
  [PR #138](https://github.com/ait-app/ait/pull/138)、
  [验证说明](../clients/account-host-relay-validation.md)。
- **可选 Android APK。** 增加手动构建 ARM64、ARMv7 的入口，验证版本、架构、签名和原生库对齐。
  推送标签默认只发布桌面；需要 APK 时手动勾选 `build_android`，本次发布保持默认关闭。
  当前使用测试签名，安装及后续签名兼容性见 [Android APK 发布](../../operations/android-releases.md)。
  [PR #158](https://github.com/ait-app/ait/pull/158)。
- **大 Diff 与 Codex 选项。** 改善大 Diff 加载，超出响应预算时保留 Host 连接；按模型展示
  Codex 推理等级，包括可用时的 `max`、`ultra`。
  [PR #153](https://github.com/ait-app/ait/pull/153)、[PR #152](https://github.com/ait-app/ait/pull/152)。
- **稳定性与边界修复。** 收紧桌面导航，修复连接重试、过期通知操作、Git 特殊文件名、目录链接、
  运行中定时配置、空仓库历史、过大的目录与 setup 输出，以及离线语音容量错误分类。
  [PR #154](https://github.com/ait-app/ait/pull/154)、[PR #156](https://github.com/ait-app/ait/pull/156)。

## 安装与升级

[Release 下载页](https://github.com/ait-app/ait/releases/tag/v0.0.15)。

| 平台                           | 安装包                                                     |
| ------------------------------ | ---------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.15-macos-arm64.dmg`、`Ait-0.0.15-macos-arm64.zip` |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.15-linux-x64.tar.gz` |

退出旧桌面应用，安装新版本后重新打开；远端 Host 也需升级 daemon 才能获得服务端改进。
Android APK 仅在另行手动选择构建后提供。Google Play 和 iOS TestFlight 使用独立的手动发布流程。

## 发布验证

13 个 Cargo 包及本地 path 依赖约束、根 npm 包和 6 个 workspace 统一为 `0.0.15`，
Cargo/npm 锁文件语义比较确认第三方依赖未变。

发布准备同时修复 [main Rust CI](https://github.com/ait-app/ait/actions/runs/37115651052)
暴露的测试竞态：终端 shell 重定向先创建 PID 文件，再写入 PID；原测试只等文件存在，
可能读到空内容并触发 `ParseIntError { kind: Empty }`。现在由子进程在写完后输出
`PID_READY`，测试通过终端 capture 等待该信号，继续验证 daemon 关闭后终端进程已退出。
未跳过测试、延长超时或修改终端生产逻辑。

合并后 [main CI](https://github.com/ait-app/ait/actions/runs/37116836917) 的 Rust job 已通过，
桌面 job 则暴露 ASAR 测试夹具的写入竞态：`@electron/asar` 3 的 `createPackage` 在
输出流完成前返回，立即读取可能得到零字节内容。Linux launcher 与资源校验测试改为
等待 ASAR CLI 进程结束，再读取生成文件；生产打包和校验逻辑不变。
本地执行 `npm exec --workspace=@ait/desktop -- vitest run src/daemon/linux-launcher.posix.test.ts`：
1 passed，9 项 Linux 专用测试按原条件跳过；桌面 typecheck 和发布脚本测试通过，Linux 用例由 CI 验证。

Android 发布输入校验增加显式失败退出，避免 macOS Bash 3.2 的 `errexit` 行为使非法
标签检查被后续成功命令覆盖；现有非法标签/源码回归用例已验证。

本地环境：macOS arm64、Rust 1.98.1，基线加本发布分支的改动。

- `npm ci --no-audit --no-fund`：锁定依赖安装通过；初次离线安装因新依赖未缓存而失败，联网安装后通过。
- `npm run verify:release -- v0.0.15`、`npm run verify:local-packages`：版本与本地依赖验证通过。
- `npm run test:release`：17 passed；`npm run test:mobile-release`：29 passed（包含 Android 可选发布回归）。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`：通过。
- `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 npm run test --workspace=@ait/mobile -- --project unit native-release-version.test.ts src/changelog`：3 文件、46 passed。
- `cargo metadata --locked --offline --no-deps --format-version 1`、`cargo fmt --all --check`：通过。
- `AIT_SERVER_BIN=/private/tmp/ait-git-fetch-target/debug/daemon node apps/desktop/scripts/prepare-daemon.mjs`：只暂存 `daemon 0.0.15`。
- 修改 JSON/Markdown/YAML 的 Oxfmt、Android 工作流 YAML/Shell 语法、文档链接及 `git diff --check`：通过。

Rust 完整测试 **1,751 passed、0 failed、3 ignored**；workspace build、Clippy（`-D warnings`）
全部通过。以下命令均使用 `CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target` 和
`SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib`：

```sh
cargo test --locked --offline -p daemon --test process unix::terminal::terminal_reconnect_archive_batch_close_and_shutdown_cleanup_are_observable -- --exact
cargo test --locked --offline --workspace
cargo build --locked --offline --workspace
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
```

正式安装包由 Release Ait 工作流生成；Linux、macOS 全部成功后才创建 GitHub Release。
Android 默认跳过，仅手动选中时要求两个 ABI 的 APK 检查通过。
macOS 需通过签名、公证和成品启动检查。发布后核对资产完整性、SHA-256、桌面更新摘要和构建来源。

## Test coverage

后续 Android 可选发布与 ASAR 夹具调整只改工作流、发布脚本、测试与文档，没有 Rust 改动，按仓库规则
未重复 Rust 测试或覆盖率。定向回归覆盖默认桌面发布、显式 Android、缺失 APK、意外 APK、
选定平台失败/跳过/取消和实际工作流向校验脚本传参；Oxlint、Oxfmt 与文档检查通过。
该调整的 JavaScript 行覆盖率未测量，后续扩展发布脚本行为时继续补充相应回归。

本次提交准备实测 workspace 行覆盖率 **94.5328%（48,587 / 51,397）**；
相关 daemon 为 **95.2991%（892 / 936）**，terminal 为 **96.3528%（1,453 / 1,508）**。
插桩运行独立执行 **1,751 passed、0 failed、3 ignored**，测试数量不作为覆盖率百分比。

测量对应上述基线加版本同步及终端测试修复，精确输入指纹、改动测试 SHA-256、逐 crate
计数和原始报告摘要保存在[可审阅覆盖率证据](release-0.0.15-coverage.json)。
平台为 macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4，完整 13-crate workspace，
默认 features 和文件过滤，无额外排除或新增 ignored；不含 doctest 插桩。

```sh
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov --locked --offline --workspace --html
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-release-0.0.15-coverage-summary.json
```

HTML 位于 `/private/tmp/ait-workspace-sidebar-cov-target/llvm-cov/html/index.html`，共享证据为上述 JSON。
同平台历史[rebase 测量](../daemon/crate-coverage-rebase.md)为 94.5483%（48,595 / 51,397），
本次低 0.0156 个百分点；未重测旧基线，生产 Rust 未改动，不将本次差值解释为功能覆盖退化。
3 项原有 ignored 依赖真实 Provider 安装或认证；原生语音模型、真实 OpenCode 推理和移动真机
端到端流程未在本次验证。Linux 运行由 PR/main CI 验证；真实 Provider 和设备行为需相应环境复验。
