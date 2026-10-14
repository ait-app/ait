# Rust 测试提速：提交验证

2026-10-10；基于 main `d2699356d2f585cf17e8e221f278863b104d8d2a`（最初在 `d8a5fb8e` 上测量，随后 rebase）。
平台 macOS 27.0.1、Apple M4（10 核）、Rust 1.98.1、cargo-nextest 0.9.148、cargo-llvm-cov 0.8.4。
所有耗时均为单机单次测量，只作量级参考。

## 改动

- 完整测试改用 `cargo nextest run --workspace`，并用 `cargo test --workspace --doc` 补跑 doctest。
  新增 `.config/nextest.toml`：失败后继续运行，超过 30 秒标记为慢测试，超过 120 秒终止。
- `[profile.dev] debug = "line-tables-only"`：panic 回溯仍保留文件与行号。需要在调试器里查看变量时，
  用 `CARGO_PROFILE_DEV_DEBUG=full` 构建。
- CI 的 rust job 增加 `Swatinem/rust-cache` 缓存，改用 nextest；`.config/nextest.toml` 变更也会触发该 job。
- Rust 规范更新完整测试与覆盖率命令，禁止靠降低并发让测试通过，并补充新 worktree 复用 target 的做法。
- flake 开发环境加入 cargo-nextest。

## 并发不稳定的测试

nextest 并发度高于 `cargo test`，在 main 上暴露出以下依赖时序的测试，均只改测试代码：

| 测试                                                                                                        | 原因                                                                                                                                      | 修复                                                                |
| ----------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `voice` `dictation_audio_budget_rejects_only_the_extra_chunk`                                               | 发送 256 个分块期间，真实时钟越过 2 秒中间转写周期，第一次引擎调用变成中间转写                                                            | 断言最后一次调用（最终转写）收到完整 16 MiB                         |
| `filesystem` `workspace_runtime_refreshes_every_queued_checkout_without_repeated_snapshot_reads`            | 等待期间超过 5 秒需求 TTL，排队的读取被当作无人需要而丢弃                                                                                 | 轮询时刷新访问时间，只保持需求，不新增读取                          |
| `daemon::process` `opencode_workspace_creation_returns_frontend_compatible_resume_handles` 等 OpenCode 用例 | 发现、标题生成与 Agent 进程共用 fixture 根目录，`session/new` 以“文件是否存在”判断，可能同时认领 `ses_one`，临时会话删除后 Agent 回放为空 | fixture 用 `open("x")` 原子认领 `ses_one`，其他进程使用独立会话文件 |
| `api` `single_connection_rejects_an_excessive_burst_without_blocking_ping`（#250 新增）                     | files worker 取走上传帧的时机不定，被拒绝的请求不一定是突发末尾的连续区间；测试却假定接纳的是前 N 个。`cargo test` 下也约 40% 失败        | 记录被拒绝的 ID，按顺序断言其余请求全部成功                         |
| `daemon::process` `large_diff_snapshot_limits_stay_inline_and_recover`                                      | 负载下偶发 10 秒等待超时；前述修复后未再出现                                                                                              | 未改动，继续观察                                                    |

ADR-116 记录的 `websocket_directory_streams_keep_sequences_ownership_and_reconnect_checkpoints`
在本次 18 轮完整 nextest 中均通过。

## 验证

- rebase 前基于 `d8a5fb8e`：`cargo nextest run --workspace` 共 8 轮压力运行，每轮 2009 通过、0 失败；修复前 3 轮中有 2 轮失败。
- rebase 后基于 `d2699356`：`--stress-count 5` 每轮 2021 通过、0 失败、15 跳过。
- OpenCode/ACP 相关用例 `--stress-count 30`（rebase 前）与 `--stress-count 20`（rebase 后）全部通过；修复前 20 轮中失败 2 轮。
- `api` 突发测试 `--stress-count 100` 全部通过，`cargo test` 连续 20 次通过；修复前 40 轮中失败 26 轮。
- `cargo test --workspace --doc` 通过；`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `npm run check:docs`、`node --test scripts/nightly-workflow.test.mjs`（22 通过）、oxfmt 检查通过。

## 耗时

在 `d8a5fb8e` 上测量：

| 场景                                                                   | 修改前           | 修改后             |
| ---------------------------------------------------------------------- | ---------------- | ------------------ |
| 已编译，完整运行测试（`cargo test -- --test-threads=1`，以往报告常用） | 327s             | —                  |
| 已编译，完整运行测试（`cargo test`）                                   | 78–89s           | —                  |
| 已编译，完整运行测试（`cargo nextest run` + doctest）                  | —                | 67–75s + 10s       |
| 冷编译测试二进制（墙钟 / CPU 用户时间）                                | 88s / 444s       | 84s / 357s         |
| 冷编译 target 体积                                                     | 5.7G             | 4.3G               |
| 修改 `model` 后增量编译                                                | 14.0s            | 12.3s              |
| 新 worktree，APFS 克隆 4.3G target 后编译                              | 约 85s（冷编译） | 6s 克隆 + 45s 编译 |

主要收益来自去掉单线程运行，以及修复并发问题后可以稳定地并发跑测试。nextest 比默认并发的
`cargo test` 快约 15%。调试信息降级对 10 核机器的墙钟时间影响小，但减少约 20% CPU 时间和
25% 磁盘占用；在核数较少的 CI runner 上收益会更明显。
APFS 克隆耗时与源 target 体积成正比：克隆一个积累到 55G 的 target 需要 51s，因此规范建议定期
`cargo clean` 后再作为种子。

## Test coverage

Workspace 行覆盖率 **94.6512%（56273 / 59453）**，可比基线 main `d2699356` **94.6479%（56271 / 59453）**，
**+0.0034 个百分点**。本变更只修改测试代码和 fixture，不改变生产代码的可覆盖行数；差异来自两个依赖时序的
分支（`opencode/summary.rs`、`generated_titles.rs` 各 +1 行）。

`deepseek_harness/metadata.rs` 的覆盖在不同运行间相差 24 行：它只在后台标题生成恰好调用到 DSH 时执行，
同一基线的两次运行分别为 81 / 103 和 57 / 103，因此两边都取了未命中的那次运行作比较。

命令：`cargo llvm-cov clean --workspace` 后运行 `cargo llvm-cov nextest --workspace --html`，再运行
`cargo llvm-cov report --json --summary-only`。基线在同一机器上对 `d2699356` 使用完全相同的命令。
范围：全部 13 个 workspace package、默认 features、无额外文件排除；15 项原生 Provider 用例按默认配置跳过。
覆盖率运行 2021 通过、0 失败。Doctest、Python/shell/Node fixture 与上游二进制不在插桩范围；Linux/Windows 未测量。
本地 HTML 位于 `target/llvm-cov/html/index.html`，CI 产物尚未提供。

| Package     | 基线                      | 本变更                    |
| ----------- | ------------------------- | ------------------------- |
| api         | 94.5479%（2133 / 2256）   | 94.5479%（2133 / 2256）   |
| browser     | 97.5443%（715 / 733）     | 97.5443%（715 / 733）     |
| daemon      | 94.7520%（1318 / 1391）   | 94.7520%（1318 / 1391）   |
| domain      | 100.0000%（549 / 549）    | 100.0000%（549 / 549）    |
| filesystem  | 94.8674%（11589 / 12216） | 94.8674%（11589 / 12216） |
| metadata    | 94.1562%（5462 / 5801）   | 94.1562%（5462 / 5801）   |
| model       | 96.2536%（1336 / 1388）   | 96.2536%（1336 / 1388）   |
| persistence | 95.8734%（1696 / 1769）   | 95.8734%（1696 / 1769）   |
| provider    | 94.1556%（26840 / 28506） | 94.1626%（26842 / 28506） |
| relay       | 91.8489%（462 / 503）     | 91.8489%（462 / 503）     |
| schedule    | 97.5980%（772 / 791）     | 97.5980%（772 / 791）     |
| terminal    | 96.4713%（1531 / 1587）   | 96.4713%（1531 / 1587）   |
| voice       | 95.1605%（1868 / 1963）   | 95.1605%（1868 / 1963）   |

## 限制与后续

- 仅在 macOS arm64 上测量；Linux CI 上的首次运行会建立 rust-cache，之后的 PR 才能体现缓存收益。
- `large_diff_snapshot_limits_stay_inline_and_recover` 依赖 10 秒的接收超时，未定位到确定原因；
  若再次出现，需检查 checkout 轮询预算（容量 1）在高并发下的排队情况。
- DSH 标题生成路径只有后台调度偶然命中时才有覆盖，需要一个确定性的进程级测试。
- `provider` 单个 crate 的类型检查约占冷编译关键路径的一半，拆分属于边界变更，需另立 ADR。
