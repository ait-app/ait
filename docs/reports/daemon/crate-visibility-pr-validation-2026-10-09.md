# Crate 可见性与最新 main 整合验证

2026-10-09；PR #239，源码提交 `fa5fc947a95a18bf0a95b606c8312b172e875d1f`，基于 main `129acd53700b39bc03b3ee6180b2e4c1b60a9db7`。
测量树为 `0064f6c93ad0cadb538efe51d6cd24edf6f02684`，Rust 源码指纹和逐文件计数见 [JSON 证据](crate-visibility-pr-coverage-2026-10-09.json)。
随后添加的报告和索引不改变测量源码。边界决策见 [ADR-116](../../decisions/daemon/adr-116-crate-visibility.md)。

本分支已整合 main 的 OpenCode 官方 ACP 实现，沿用其模块、能力探测和历史重放；
保留旧 HTTP/SSE 文件删除，收缩 ACP client 可见性。新 ACP 重放测试直接比较完整 `Row`，
沿用本分支对未使用 `Row::value` 的删除。文档保留 OpenCode ADR-115，可见性 ADR 改为 116。

## 验证

- ACP 定向测试：31 通过、0 失败、5 忽略。
- `cargo test --workspace -- --test-threads=1`：2000 通过、0 失败、14 忽略；doctest 阶段通过，当前 0 个用例。
- Instrumented workspace：2000 通过、0 失败、14 忽略，不含 doctest。
- `cargo fmt --all --check`、`cargo build --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、文档本地链接和 diff 检查通过。
- 初次沙箱执行不允许 loopback 监听；完整测试与覆盖率在允许本地监听的环境重跑，环境限制未导致源码修改。

## Test coverage

Workspace 行覆盖率 **94.6999%（55854 / 58980）**；provider **94.2613%（26445 / 28055）**。
OpenCode 生产实现 **91.7611%（2105 / 2294）**；共享 ACP transport 生产实现 **89.8551%（186 / 207）**。
无同一基准提交、同一测量范围的可比基线；历史 ACP 报告额外运行了原生安装测试，不能直接比较。

测量命令：`cargo llvm-cov --workspace --html -- --test-threads=1`。
范围为全部 13 个 workspace package、默认 features、macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4，
无额外文件排除。默认忽略的 14 项原生 Provider 测试没有执行；清单、精确导出命令、
源码哈希和未覆盖行在上述 JSON 中。Doctest、shell/Python fixture、上游二进制与未接入模块未被 Rust instrumentation 测量。
本地 HTML：`target/llvm-cov/html/index.html`；可共享产物为本仓库 JSON 和本报告。

| Package | 行覆盖率 |
| --- | --- |
| api | 94.4149%（2130 / 2256） |
| browser | 97.5443%（715 / 733） |
| daemon | 94.8117%（1334 / 1407） |
| domain | 100.0000%（549 / 549） |
| filesystem | 94.9165%（11595 / 12216） |
| metadata | 94.1420%（5464 / 5804） |
| model | 96.1933%（1314 / 1366） |
| persistence | 95.7143%（1675 / 1750） |
| provider | 94.2613%（26445 / 28055） |
| relay | 91.8489%（462 / 503） |
| schedule | 97.5980%（772 / 791） |
| terminal | 96.4713%（1531 / 1587） |
| voice | 95.1605%（1868 / 1963） |

ACP 运行中的原生 `config_option_update`、`$/cancel_request` 通知处理和部分协议/进程 I/O 错误仍未覆盖；
后续补充通知回调与故障注入测试，并在 Linux/Windows 和隔离的原生 Provider 环境复查。

`summary_config.rs` 及其 tests.rs 仍未接入模块树，引用的摘要接口不存在，因此没有编译或测量；
main 中重复编号的摘要 ADR-109 与历史审计路径问题保持现有状态。
