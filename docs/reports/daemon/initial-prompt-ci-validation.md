# PR #169 Rust CI：首次输入重试测试的会话隔离

日期：2026-10-04。本轮父提交 `aa438db58dffc081bad1489c5650c67bb2a8893f`。
仅修改 daemon 进程测试，不修改 DSH、Codex 或共享会话服务的运行逻辑。

## 根因与修复

[失败的 Ubuntu CI](https://github.com/ait-app/ait/actions/runs/37175370929/job/111356794938)
在 fmt 和 clippy 通过后，`attempted_initial_prompt_is_not_replayed_after_failure` 断言失败：
`turn/start` 次数为 2，预期为 1。其余 90 项 daemon 进程测试通过。

原断言统计同一 cwd 的整个 fixture 请求日志，包含用户会话和后台标题生成的临时会话。
后台调用是否已写入日志取决于调度，因此本机原全量测试能通过，CI 却可能失败。
本地在读取日志前临时延迟 1.5 秒，复现相同的 `2 != 1`；该诊断延迟未保留。

测试现在等待另一个 thread 的结构化 metadata 调用实际出现，再按失败 Agent 的持久化 session ID
统计用户 `turn/start`，仍严格断言恰好一次。创建重试的 receipt 相等断言保留。
只读取换行完整的日志记录，避免并发追加时解析半条 JSON。
没有禁用标题生成、放宽发送次数、跳过测试或修改原生 Codex 行为。

## 验证

- 相关 workspace creation concurrency 测试：3 passed、0 failed。
- fmt、workspace clippy（all-targets，warnings denied）和 build 通过。
- 普通 workspace 测试：1786 passed、0 failed、4 ignored；覆盖率执行相同全量测试通过。
- 文档链接检查通过。推送后由 PR checks 记录 Ubuntu CI 复测结果。

## Test coverage

本轮使用 `aarch64-apple-darwin`、Rust 1.98.1、cargo-llvm-cov 0.8.7、默认 features/default file filters，
无额外文件排除；不包含 doctest 行覆盖率。4 项既有外部 CLI/推理测试仍 ignored。
本轮没有重新运行真实 CLI 或桌面验收；原 DSH smoke 证据属于 `aa438db5`。

| 范围 | 覆盖行 / 总行 | 行覆盖率 |
| --- | ---: | ---: |
| Workspace | 50,385 / 53,391 | 94.37% |
| Daemon | 921 / 965 | 95.44% |
| Provider | 22,659 / 24,185 | 93.69% |

与 `aa438db5` 同工具/平台/范围基线相比，workspace 变化 +0.00 个百分点。
测试数量与行覆盖率分别报告；本次只修改测试，覆盖率衡量全仓库生产 Rust 代码。
源码由父提交和附件中唯一 Rust 修改文件的 SHA-256 确定，之后仅补报告。

在 Nix dev shell 中设置 `CARGO_TARGET_DIR=/Users/lonnetkirisame/Documents/Developer/ait/target`，执行：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline
cargo llvm-cov --workspace --locked --offline --html -- --test-threads=1
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-dsh-ci-coverage.json
```

[覆盖率附件](initial-prompt-ci-coverage.json)记录测量范围、逐 crate 指标、源码和日志校验值。
HTML 位于共享 target 的 `llvm-cov/html/index.html`。
未覆盖部分启动失败、验证上限及外部 provider 行为；本轮不扩展这些功能。
DSH 修复后的桌面复测仍参考[原生 Host 交接报告](../providers/deepseek-harness-native-host.md)。
