# Workspace 侧边栏信息与运行标记修复

日期：2026-10-02
基线：`84a07aafd3a671e47abee49612f98553d9997c72` 加本次 PR 修改。
Rust 源码指纹：`34b4210319de1b25a1caeaa6f8560ba7532dd36cf17e0632dc82c7fc517ce31f`。
[覆盖率证据](workspace-sidebar-coverage.json) 记录完整源码列表及逐文件 SHA-256，
用于确认测量对应的代码；测量后只更新报告。

## 结果

侧边栏原有的增删行数、PR/MR 合并状态、CI 结果和 Git hover 信息现在由 Rust server
填充，编辑文件后通过现有 workspace 订阅持续更新。Git 与 Forge 独立缓存和排队刷新，
慢远端不会阻塞本地 Git 信息或目录请求。

运行标记会尊重 workspace 目录中较新的完成状态，旧客户端 Agent 活动不能重新点亮
已完成的行；下一轮任务仍可正常显示运行。原生会话空闲时明确返回 `activeTurn: null`。
真实 WebSocket 回归验证了未打开聊天页、未订阅 timeline 时的正常完成、取消和再次运行。

实现边界见 [ADR-068](../../../decisions/workspace/adr-068-workspace-runtime-summaries.md)。本次 PR 仅包含
侧边栏摘要和运行状态修复，基于最新 main 验证；共享工作区的其他任务没有纳入。
本轮没有发布或替换已安装的桌面包，客户端需重新构建启动后生效。

## 合并后 Diff 归零修复（2026-10-02）

测量版本：`274fd55e` 加本 PR 的 Rust 修改，macOS arm64、Rust 1.98.1、默认 features；
完整源码指纹及四个修改文件的 SHA-256 记录在本次覆盖率证据中。
实机右侧“已 commit”为 `+0 -0`，侧边栏仍为 `+3.5k -53`。同一 checkout 的本地 main
比较得到 `+3513 -53`，origin/main 比较为零：远端已包含工作区提交，本地 main 未同步。

侧边栏 numstat 现在复用 base diff 的 `comparison_base`，优先读取已 fetch 的 origin
跟踪引用。统计、缓存到期后的更新及真实 WebSocket 推送均覆盖“HEAD 不变、远端基准
移动”后的清空行为；后续未提交和未跟踪改动仍正常计数。规则与 Paseo v0.10.2 的
[`resolveBestComparisonBaseRef` 和 `resolveShortstatComparisonRef`](https://github.com/getpaseo/paseo/blob/v0.10.2/packages/server/src/utils/checkout-git.ts#L1632)
一致。本轮没有替换正在运行的桌面包或内置 server。

定向测试 **18 通过、0 失败**，命令如下，均使用
`CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-pr-target`：

```sh
cargo test --locked --offline -p filesystem --lib sidebar_ -- --nocapture
cargo test --locked --offline -p filesystem -p metadata --lib workspace_runtime -- --nocapture
cargo test --locked --offline -p daemon --test process binary_workspace_runtime -- --nocapture
```

WebSocket 测试首次因 sandbox 禁止回环监听而失败，允许本机监听后通过。
Git 使用临时仓库和受控远端引用，Forge 使用本地 CLI fixture。
`cargo fmt --all --check` 和
`cargo clippy --locked --offline -p filesystem -p daemon --all-targets -- -D warnings`
通过。

提交准备阶段的普通全量测试 **1557 通过、0 失败、3 ignored**；构建、格式和完整
Clippy 检查通过。普通检查命令如下，除格式检查外均使用上述普通检查 target：

```sh
cargo fmt --all --check
cargo build --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline -- --test-threads=4
```

### Test coverage

使用 cargo-llvm-cov 0.8.4，测量整个 Cargo workspace，默认 features，macOS arm64，
Rust 1.98.1。采用工具默认源文件过滤，没有额外排除；不包含 TypeScript/UI 或 doctest
插桩。workspace 行覆盖率 **92.15%（43,992 / 47,739）**；本次修改的业务可执行行
**100%（2 / 2）**。相关 crate 数值记录在
[本次覆盖率证据 JSON](workspace-sidebar-diff-base-coverage.json) 中。
`filesystem` 为 **90.17%（10,547 / 11,697）**，`daemon` 为
**94.38%（890 / 943）**。

可比较的 [PR #147 基线](diff-pr-coverage.json) 为 92.1467%（43,989 / 47,738），
本轮为 92.1511%，变化 **+0.0044 个百分点**。基线使用相同范围、平台、features 和
工具链；其 763 个源码文件哈希与本 PR 基线全部一致。

```sh
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-sidebar-diff-coverage-summary.json
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --lcov --output-path /private/tmp/ait-sidebar-diff-coverage.lcov
```

插桩全量测试独立运行，**1557 通过、0 失败、3 ignored**，不能与普通测试数相加。
共享证据包含 workspace/crate 数值、修改行命中、源码指纹和测试日志哈希；本地 HTML
位于 `/private/tmp/ait-workspace-sidebar-cov-target/llvm-cov/html/index.html`，生成的
HTML、LCOV 和运行文件未纳入提交。

本轮未验证 Linux/Windows 或真实网络 fetch 的时间与失败恢复；回归测试直接移动
受控的 origin 跟踪引用。3 项依赖已安装且已认证的真实 Codex/Claude Provider 测试
保留 ignored。后续可在其他平台和真实远端补充验证。

## 原侧边栏修复的测试执行

平台为 macOS arm64，Rust 1.98.1，默认 features。Git/Forge 使用临时仓库和受控 CLI，
Provider 使用现有离线 Codex app-server fixture；3 项依赖已安装且已认证的真实
Codex/Claude 测试按原配置 ignored。

Rust 普通检查的 `CARGO_TARGET_DIR` 为 `/private/tmp/ait-workspace-sidebar-pr-target`：

```sh
cargo fmt --all --check
cargo build --workspace --locked --offline
cargo clippy --workspace --locked --offline --all-targets -- -D warnings
cargo test --workspace --locked --offline -- --test-threads=4
```

全量构建、格式和 Clippy 检查通过；全量测试 **1516 通过、0 失败、3 ignored**。
测试包含目录快照与订阅、文件编辑后摘要更新、PR/MR 与 CI 投影、身份失效与网络错误，
以及 Provider 完成、取消、重连和再次运行。

客户端命令：

```sh
npm exec --workspace=@ait/mobile -- vitest run --project unit src/hooks/sidebar-workspaces-view-model.test.ts src/runtime/directory-sync/agent-replica.test.ts src/utils/agent-snapshots.test.ts src/utils/workspace-agent-activity.test.ts
```

客户端 4 个相关文件 **64 项测试通过**。两个修改的 TypeScript 文件通过 oxfmt；
ESLint 为 0 错误，保留原有的 4 个 `array-type` 警告。

## Test coverage

使用 cargo-llvm-cov 0.8.4，测量整个 Cargo workspace，默认 features，macOS arm64。
采用工具默认源文件过滤，没有额外排除；不包含 TypeScript/UI 覆盖率和 doctest 插桩。
测量版本由本报告基线和覆盖率证据中的源码指纹确定。

| 范围                               | 行覆盖率   | covered / total     |
| ---------------------------------- | ---------- | ------------------- |
| 整个 workspace                     | **92.01%** | **43,160 / 46,907** |
| daemon                             | 94.32%     | 880 / 933           |
| filesystem                         | 89.51%     | 9,799 / 10,947      |
| metadata                           | 90.72%     | 6,980 / 7,694       |
| provider                           | 93.41%     | 18,347 / 19,641     |
| 本次新增或修改的 Rust 业务可执行行 | 89.96%     | 466 / 518           |

workspace 和 crate 数值来自 LLVM JSON summary。新增或修改行数取 LCOV 的 `DA` 记录
与基于 main 的 diff 相交，排除测试源码；LCOV 与 JSON summary 对宏展开的行统计可能不同。
可比较的 [DeepSeek 基线](../providers/deepseek-harness-acp-coverage.json) 同样使用 Rust 1.98.1、
macOS arm64、默认 features 和 workspace 范围：91.99%（42,649 / 46,362），
本次提升 **0.02 个百分点**。

完整测量命令：

```sh
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-workspace-sidebar-coverage-summary.json
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --lcov --output-path /private/tmp/ait-workspace-sidebar-coverage.lcov
```

插桩全量测试 **1516 通过、0 失败、3 ignored**，这是独立运行，不能与普通测试数相加。
首次使用 4 个测试线程的插桩运行触发已有
`setup_thread_does_not_retain_the_runtime_across_restart` 的即时 weak 引用断言竞态；
未修改源码，完整串行重跑通过。

可共享的 [覆盖率证据 JSON](workspace-sidebar-coverage.json) 包含 workspace/crate 数值、
覆盖文件摘要、修改行命中及未覆盖行号、工具链、命令、源码和测试日志哈希。
本地 HTML 为 `/private/tmp/ait-workspace-sidebar-cov-target/llvm-cov/html/index.html`；
生成的 HTML、LCOV 和运行文件未纳入提交。

已审查的重要缺口包括缓存容量淘汰、过期排队需求、线程创建失败、部分 PR/CI 枚举映射
和文件系统异常分支。后续适合增加可控时钟和线程启动注入的测试；本次未为覆盖率指标
新增生产代码。Linux/Windows、真实已认证 gh/glab 远端和 Provider 模型调用未执行。
