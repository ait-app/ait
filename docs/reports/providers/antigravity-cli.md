# Antigravity CLI 原生 Provider 验证

日期：2026-10-06。源码范围：`8d3d8b8a3d837eb30acdd8a8a405d20fcd577fb0`
合并最新 `main` 的 `91102e13e2b549a135b7e2b8dd9f2a5546468afa`。
测量源码的聚合 SHA-256、变更文件哈希与逐文件指标见[覆盖率工件](antigravity-cli-coverage.json)。
平台：macOS arm64；本机 Homebrew `agy` 1.3.0。

## 结果与范围

新增 `antigravity` adapter 和 daemon 注册，使用官方 NDJSON 协议。
客户端补齐 Provider 定义、图标、权限模式和原生终端恢复命令。
设计与操作范围见 [ADR-087](../../decisions/providers/adr-087-antigravity-cli-provider.md)
和 [操作手册](../../operations/antigravity.md)。

| 验证 | 结果 |
| --- | --- |
| 新 adapter 针对性 Rust 测试 | 22 通过，0 失败，1 个在线测试默认 ignored |
| Provider catalog 直接相关回归 | 20 通过，0 失败 |
| 本机 AGY 在线测试（初版 `8d3d8b8a`） | 1 通过，0 失败；三轮短文本，不请求工具 |
| 协议包完整回归 | 765 通过，0 失败；69 个测试文件 |
| 客户端图标与恢复命令回归 | 13 通过，0 失败 |
| Rust workspace 完整测试 | 1857 通过，0 失败，7 个原生 CLI 测试默认 ignored；串行运行 |
| workspace 覆盖率插桩测试 | 1857 通过，0 失败，7 ignored；HTML 已生成 |
| workspace 严格 Clippy | 通过，`--all-targets -- -D warnings` |
| workspace 构建 | 通过，`cargo build --workspace`，无警告 |
| Rust 格式、TypeScript 格式/lint、协议包构建 | 通过 |
| 桌面类型检查 | 重建本地 SDK 后通过 |
| 移动端类型检查 | 65 项诊断，与同依赖环境的 `main` 基线完全一致；没有新增诊断 |

离线夹具覆盖官方/Homebrew 路径选择及显式错误路径、动态模型格式、多轮和模式切换、
resume identity、Unix 原生中断确认、环境值不持久化、异常协议/EOF/超限/超时、
失败结果、unsupported 输入拒绝、token 累计快照和 UTF-8 工具输出预览。
AgentManager 回归验证时间线持久化及 manager 重建后保持原生 ID 与已存显示历史。

真实测试使用 `AntigravityClient::installed()` 发现 `/opt/homebrew/bin/agy`，
动态选择 CLI 提供的低 effort 模型，在 Plan 模式完成两轮上下文记忆验证，关闭原生进程后
以 `--conversation` 恢复，再验证第三轮上下文。原生目录和认证未复制进仓库。

官方安装发现使用临时目录夹具验证；未在当前主机安装第二份 CLI。
Linux 和 Windows 未进行实机运行。在线测试未运行 shell/edit 工具，未测在线取消、额度或交互审批。
初次完整测试发现 daemon 旧列表断言仍假设四个 Provider，且会发现主机上真实的 Homebrew AGY。
已在进程夹具中指定独立的 `AIT_SERVER_ANTIGRAVITY_BIN`，更新五个 Provider 的诊断断言，
并按 Provider ID 检查原生历史错误，避免依赖列表位置。
并发进程回归曾出现一次目录同步超时；最终完整 suite 串行运行全部通过。

移动端类型检查的诊断包含本机缺少的 `expo-clipboard`、`@xterm/*`、
`react-native-keyboard-controller` 和 `node:sqlite` 类型等。
从最新 `main` revision 导出桌面和移动端源码，在相同依赖与本地 SDK 下重跑 `tsgo --noEmit`，
移动端的 65 项诊断逐项一致；不将这一环境限制记为类型检查通过。

PR #196 初版 CI 的 Rust、UI 与文档检查均通过，随后 `main` 新增提交导致文档索引冲突。
本次同步保留 Antigravity 条目和 ADR-088、DSH 原生 Host 说明；代码自动合并保留两侧的
Provider 注册、独立 catalog 通道和进程夹具隔离。完整测试在合并后的源码上重新运行。

## 命令

```bash
cargo test -p provider local::antigravity --no-fail-fast
cargo test -p provider service::provider_catalog --no-fail-fast
cargo test -p provider local::antigravity::tests::installed::authenticated_multi_turn_and_resume -- --ignored
cargo test --workspace -- --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo fmt --all --check
npm run build:protocol
npm run test --workspace=@ait/protocol
npm run build:sdk
npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile
```

客户端命令在 `apps/mobile` 运行：

```bash
../../node_modules/.bin/vitest run --project unit src/components/provider-icons.test.ts src/components/provider-icon-name.test.ts src/utils/provider-command-templates.test.ts
```

对本次修改的八个 TypeScript 文件运行 `oxlint` 与 `oxfmt --check`；
本次文档运行 `npm run check:docs`；工作区运行 `git diff --check`。
daemon 首次 lint 需要下载项目已有的 sherpa-onnx 1.13.8 静态库；下载后相同命令正常通过。

## Test coverage

| 测量范围 | 覆盖行 / 总行 | 行覆盖率 |
| --- | ---: | ---: |
| Cargo workspace | 51874 / 54974 | **94.36%** |
| `provider` | 23884 / 25472 | **93.77%** |
| `daemon` | 928 / 973 | **95.38%** |
| Antigravity 生产 adapter | 869 / 911 | **95.39%** |

测量使用 Rust 1.98.1、LLVM 22.1.8 和 cargo-llvm-cov 0.8.4，
平台为 macOS 27.0.1 (26A434) arm64；源码 revision 与哈希见本文开头及工件。
完整 workspace 启用默认 features，使用工具默认源文件过滤，没有额外排除。
同命令基线为初版 `8d3d8b8a`：workspace 50309 / 53243（94.4894%），
本次为 94.3610%，变化 -0.1284 个百分点。差值包含合入 `main` 的全部源码变化，
不能归因于 AGY；AGY 生产源码哈希未变，覆盖率运行中的分支执行略有差异。
没有接入 Antigravity 前的覆盖率基线。
未插桩 doctest 或 TypeScript/UI；七项原生 Claude/Codex/AGY/DSH/OpenCode CLI 测试
按默认 ignored 设置跳过，分别需要认证或显式安装测试 CLI。
此前单独通过的 AGY 三轮在线测试不计入这些覆盖率数据。

```bash
cargo llvm-cov --workspace --html --no-fail-fast -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path target/antigravity-pr-sync-validation/coverage-summary.json
cargo llvm-cov report --lcov --output-path target/antigravity-pr-sync-validation/coverage.lcov
```

可共享的[覆盖率工件](antigravity-cli-coverage.json)包含 workspace/各 crate 计数、
adapter 逐文件指标与未覆盖行、源码哈希、测试数量和测量命令。
本机完整 HTML 位于 `target/llvm-cov/html/index.html`；已结合 LCOV 审查新增代码的未覆盖行。
主要未覆盖行为包括 Windows 用户安装路径回退、subagent 元信息、累计输出预算边界、
模型查询超限/超时、stdin 写入失败/超时，以及取消未获原生确认的分支。
扩展平台和工具支持前，应补充畸形协议与 I/O 异常夹具，并在对应宿主实测。
未实测的平台和在线工具/取消行为如上所列，后续实机验证应单独记录。
