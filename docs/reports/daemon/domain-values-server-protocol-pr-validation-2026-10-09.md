# Domain 数据与 Server 协议归属：PR 验证

日期：2026-10-09。测量源码：`9d52ff62888288881b0be5c0ab7fcf546433b345`，基于 main
`fdd6ce6375b9f40c9f5d5eb22216f11e238bf736`。完整检查在同一源码快照上运行后提交；后续验证文档提交不改变 Rust 源码。
[JSON 证据](domain-values-server-protocol-pr-coverage-2026-10-09.json)保存 13 个 crate、398 个源文件的行数与 SHA-256、精确命令、
测试计数、忽略原因及变更文件的 LCOV 零命中行。

## 结果

独立 protocol crate 已并入 `model::server`，domain 拥有纯业务值、持久记录、协议数据和纯投影。
model 保留协作端口、运行资源与创建协调，消费者直接导入类型所属 crate。
测试和 Paseo 夹具随定义迁移，依赖守卫约束 domain 的向内边界并拒绝 protocol 回归。
边界决策见 [ADR-111](../../decisions/daemon/adr-111-domain-values-and-server-protocol.md)。

普通 workspace 测试：**2023 通过、0 失败、15 忽略**，包括 file 的 1 个 doctest。
插桩 workspace 测试：**2022 通过、0 失败、15 忽略**；覆盖率命令不插桩 doctest。
完整构建、全目标 Clippy、格式、文档链接和相关脚本检查均通过。

检查期间修正了已有诊断测试的 Unix 路径：macOS 没有 `/bin/false`，实际可执行文件是
`/usr/bin/false`。修正后该用例和普通完整测试通过。
首次四线程插桩运行中，目录同步 WebSocket 用例接收超时（该次只运行到 261 通过、1 失败）；
单独插桩复查及最终完整串行插桩运行均通过，未修改该用例或生产代码。

## Test coverage

范围：全部 13 个 Rust workspace package，默认 features、默认文件过滤，无额外文件排除；
macOS arm64、rustc 1.98.1、cargo-llvm-cov 0.8.4。15 项原有原生 Provider 用例需要额外安装、
环境变量、既有会话或认证，按默认配置忽略，逐项原因见 JSON。Linux/Windows 和 TypeScript
覆盖率未测量。

| 范围       |  行覆盖率 | 已覆盖 / 总行数 |
| ---------- | --------: | --------------: |
| workspace  |  94.1797% |   56667 / 60169 |
| api        |  94.2794% |     2126 / 2255 |
| browser    |  97.5443% |       715 / 733 |
| daemon     |  95.1020% |     1165 / 1225 |
| domain     | 100.0000% |       549 / 549 |
| file       |  95.4317% |     1901 / 1992 |
| filesystem |  94.8569% |   11564 / 12191 |
| metadata   |  94.1034% |     5426 / 5766 |
| model      |  96.2154% |     1322 / 1374 |
| provider   |  93.2431% |   27268 / 29244 |
| relay      |  91.8489% |       462 / 503 |
| schedule   |  97.5980% |       772 / 791 |
| terminal   |  96.5888% |     1529 / 1583 |
| voice      |  95.1605% |     1868 / 1963 |

没有相同 13-crate 归属与本次 rebase 源码范围的直接可比基线，之前的报告早于 protocol 合并
和 domain 数据迁移，因此不计算百分比差值。

`model::server` 为 **93.8053%（106 / 113）**，未覆盖的 7 行是部分 Project/Daemon/标签错误码的
安全文本映射。相关剩余分支还包括创建协调器的无效阶段与锁异常转换、排序值 Number/Text
混合方向，以及 SessionSubscription 的 Debug 格式。旧原生 Provider、辅助通道及部分 I/O
故障分支仍未覆盖；修改这些行为时补充可控故障用例，并在对应原生环境与 Linux CI 复查。
本次迁入 domain 的可执行行全部覆盖。

[可审阅 JSON 证据](domain-values-server-protocol-pr-coverage-2026-10-09.json)使用 LLVM JSON summary 作为权威行总数，LCOV
用于零命中行诊断。HTML 已生成到 `target/llvm-cov/html/index.html`；HTML、日志与原始 profile
保留在忽略的 target 目录中。

## 精确命令

构建、测试与插桩使用 `RUST_BACKTRACE=1`，`SHERPA_ONNX_LIB_DIR` 指向已有的
`target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib`。
完整普通测试采用四个线程；最终覆盖率采用单线程。

```sh
cargo fmt --all --check
cargo build --workspace --offline --locked
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
cargo test --workspace --offline --locked --no-fail-fast -- --test-threads=4
cargo llvm-cov --workspace --html --offline --locked -- --test-threads=1
cargo llvm-cov -p daemon --test process --no-report --offline --locked -- unix::agent_execution::directory_sync::websocket_directory_streams_keep_sequences_ownership_and_reconnect_checkpoints --exact --test-threads=1
cargo llvm-cov report --json --summary-only --output-path target/domain-boundaries-coverage-raw.json
cargo llvm-cov report --lcov --output-path target/domain-boundaries-coverage.lcov
npm run check:docs
python3 scripts/rust_method_specs_test.py
python3 scripts/check-crate-coverage.test.py
node --check scripts/paseo-registry-fixtures.mjs
python3 -m py_compile scripts/paseo-focused-coverage.py
cargo test -p provider --lib --offline --locked diagnostics::tests::probes_fail_without_hanging_and_collection_reports_missing_sources -- --exact --test-threads=1
git diff --check
```
