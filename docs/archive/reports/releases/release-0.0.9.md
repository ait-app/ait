# Ait 0.0.9 发布说明

> 历史归档（2026-11-03）：保留原始设计、命令与验证结论，不能作为当前操作说明。当前入口和后续工作见[归档索引](../../README.md)。

日期：2026-09-28（Asia/Shanghai）。准备基线为 main
[`6ecfb171`](https://github.com/necokeine/ait/commit/6ecfb1718c3684d61be256f6c90cf4034ae1c6ee)。
本版承接已发布的 0.0.8，新增下列已合并修复。
正式源码对应新标签 `v0.0.9`，安装包的实际构建提交及运行链接随 `BUILD-INFO.json` 发布。

## 更新内容

- **默认离线语音。** 听写和语音合成接入 Sherpa ONNX，后台自动准备 SenseVoice / Kokoro 模型，
  不阻塞 Host 连接；模型就绪后可在本机识别和合成，已有显式 Provider 配置继续生效。
  [PR #128](https://github.com/necokeine/ait/pull/128)
- **终端连接与显示。** 支持更宽的终端面板，避免合法的大尺寸 resize 被当作连接错误；
  应用事件错误与实际连接错误分开处理。断线时保留终端画面、显示连接状态并禁止输入，
  重连后恢复输入，减少销毁重建导致的闪烁。
  [PR #128](https://github.com/necokeine/ait/pull/128)
- **修复文件面板持续加载。** 保留 SDK 请求 ID，使 JSON 与二进制文件数据正确关联；
  完成传输后释放等待记录，拒绝重复的未完成请求，并忽略迟到或错误连接的数据。
  此修复位于客户端适配层，需要更新完整桌面应用。
  [PR #129](https://github.com/necokeine/ait/pull/129)

## 升级与兼容

[下载 Release](https://github.com/necokeine/ait/releases/tag/v0.0.9)。正式资产包含：

| 平台                           | 安装包                                                    |
| ------------------------------ | --------------------------------------------------------- |
| macOS Apple Silicon，macOS 13+ | `Ait-0.0.9-macos-arm64.dmg`、`Ait-0.0.9-macos-arm64.zip`  |
| Linux x86_64                   | `Ait-linux-x86_64.AppImage`、`Ait-0.0.9-linux-x64.tar.gz` |

更新前退出旧应用，安装后重新打开；仅替换或重启 server 不会加载文件面板的客户端修复。
本版沿用 Ait 用户数据目录和应用标识，不自动迁移或删除 Paseo 数据。

首次准备语音模型需要联网，安装后约占 **637 MiB**，默认缓存于
`<server-data-dir>/models/local-speech`；准备期间稍后重试即可。模型不内置于安装包，
已有完整缓存可复用。识别与合成可离线运行，语音对话仍取决于所选 Agent Provider 的可用性。
中英文混说可能出现识别误差。配置和错误说明见[语音操作指南](../../../operations/speech.md)。

## 版本与验证

本次发布提交仅更新版本元数据、锁文件和文档：12 个 Cargo 包、28 处本地版本约束、
根 npm 包及 6 个 `@ait/*` workspace 一致为 `0.0.9`；第三方依赖保持 main 的锁定版本。
基线的 [main CI](https://github.com/necokeine/ait/actions/runs/36430367018) 已通过。

以下检查在 macOS arm64、`6ecfb171` 加本次版本元数据与文档变更上通过：

| 检查                                                                                                             | 结果                                      |
| ---------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `npm ci --offline --no-audit --no-fund`                                                                          | 锁定依赖安装及项目补丁成功                |
| `npm run verify:release -- v0.0.9`、`npm run verify:local-packages`                                              | 版本一致，6 个私有 workspace 本地链接正确 |
| Cargo/npm 锁文件比较                                                                                             | 仅本地包版本变化，第三方依赖未变          |
| `npm run test:release`                                                                                           | 12 passed，0 failed                       |
| `npm run build:desktop-main`                                                                                     | 共享库与 Electron 主进程构建通过          |
| `npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`                                             | 通过                                      |
| `npm run test --workspace=@ait/mobile -- --project unit native-release-version.test.ts src/changelog`            | 3 文件、46 passed，0 failed               |
| `CARGO_TARGET_DIR=/private/tmp/ait-release-0.0.9-target cargo build --locked --offline -p daemon --bin daemon`   | 通过，实际版本输出 `server 0.0.9`         |
| `otool -L /private/tmp/ait-release-0.0.9-target/debug/daemon`                                                    | 仅依赖 macOS 系统库，无外置推理 dylib     |
| `AIT_SERVER_BIN=/private/tmp/ait-release-0.0.9-target/debug/daemon node apps/desktop/scripts/prepare-server.mjs` | 版本与单 server 资源暂存通过              |
| `cargo fmt --all --check`、包 manifest/Markdown 格式、文档链接与 `git diff --check`                              | 通过                                      |

本地 server 为 debug 构建，仅用于发布前验证，正式资产由下述工作流重新构建。

合并版本 PR 后，从通过 CI 的 main 创建 `v0.0.9` 标签。Release Ait 工作流在原生 Linux x86_64
和 macOS arm64 runner 构建安装包、检查资源布局和成品启动；macOS 还要求签名、公证。
两个平台均成功后上传安装包、更新 YAML、blockmap、`BUILD-INFO.json` 和 `SHA256SUMS`。
最终发布状态以 [Release 页面](https://github.com/necokeine/ait/releases/tag/v0.0.9)和关联工作流为准。
完整步骤见[发布操作指南](../../../operations/releasing.md)。

## Test coverage

**本次未测量行覆盖率**：发布提交没有修改 Rust 或前端运行代码；按仓库规则跳过本地 Rust
测试与覆盖率。版本校验、构建和测试执行结果不作为覆盖率百分比。
此次合入功能的原始验证分别见[语音与终端报告](../../../reports/clients/desktop-speech-terminal.md)及
[文件读取报告](../../../reports/clients/desktop-file-read.md)，其中的历史测量不标记成本次发布测量。
后续运行代码变更按仓库要求补充针对性测试与提交阶段覆盖率；真实麦克风、用户文件面板
和各平台界面的人工验收不由自动打包与启动测试代替。
