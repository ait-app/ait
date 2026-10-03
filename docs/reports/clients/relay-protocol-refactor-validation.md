# Relay 协议重构验证

日期：2026-10-03。范围：[ADR-075](../../decisions/clients/adr-075-relay-protocol-modules.md)
所述的 Rust 中继协议与传输模块、daemon 单连接协商标识。

测量源码为 `6d0aee65`，基于 `204f94ac22772f9d98fa3e4371abade3f6d914d1`；
[覆盖率摘要](relay-protocol-refactor-coverage.json) 记录变更文件 SHA-256 和完整 Rust 源码
指纹，标识实际测量源码。验证平台为 Linux x86_64，Rust 1.98.1，使用默认 Cargo features。

本报告与[初版账户中继报告](account-host-relay-validation.md) 分别保留各自版本的测量结果，
不代表后续 Android 适配或界面修改已完成同等范围的验证。

## 测试结果

提交前普通全量测试：**1,587 通过、0 失败、3 忽略**；覆盖率全量测试独立得到相同结果，
不与普通测试数相加。忽略的是原有真实 Claude/Codex CLI 测试，其中两项需要认证并执行推理。

此前 27 项定向测试也全部通过：relay 11、protocol 10、API 6。
相关回归覆盖控制握手与独立数据连接、业务 hello 原样透传、消息格式和非法字段、配对会话 ID、
下载 headers/chunks/end/complete、错误 ACK 和 HTTP 失败、能力预算及普通连接协商。
本地网络测试在允许绑定回环端口的环境执行，不依赖线上中心。

执行命令：

```sh
cargo test --workspace
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --check
npm run check:docs
```

以上检查全部通过。

## Test coverage

Workspace 行覆盖率：**92.02%（44,649 / 48,520）**。

| 范围                          | 覆盖行 / 总行   | 行覆盖率 |
| ----------------------------- | --------------- | -------- |
| Rust workspace                | 44,649 / 48,520 | 92.02%   |
| `api`                         | 1,861 / 2,015   | 92.36%   |
| `relay`                       | 419 / 464       | 90.30%   |
| `protocol`                    | 75 / 75         | 100.00%  |
| 新增 `relay/src/protocol.rs`  | 38 / 38         | 100.00%  |
| 新增 `relay/src/transport.rs` | 38 / 38         | 100.00%  |

使用 `cargo-llvm-cov 0.9.1`，采用默认源码过滤，未额外排除生产文件；测试模块和 doctest
不计入覆盖率，纯常量文件没有可执行覆盖行。macOS/Windows 未运行。

对比 `a0ff647be14be76d6d35da84f7342a572b97fdb1` 中的
[workspace 基线](account-host-relay-coverage.json)：91.76%（44,517 / 48,513），
本次提高约 **0.26 个百分点**。两次使用相同工具版本、默认 features 和源码过滤，
本次最终测量限制为 8 个测试线程；该差值是观测结果，不单独归因于重构。

```sh
CARGO_LLVM_COV_TARGET_DIR=/tmp/ait-relay-protocol-cov-target cargo llvm-cov --workspace --html -- --test-threads=8
CARGO_LLVM_COV_TARGET_DIR=/tmp/ait-relay-protocol-cov-target cargo llvm-cov report --json --summary-only --output-path /tmp/ait-relay-commit-summary.json
CARGO_LLVM_COV_TARGET_DIR=/tmp/ait-relay-protocol-cov-target cargo llvm-cov report --lcov --output-path /tmp/ait-relay-commit.lcov
```

使用独立覆盖率构建目录，避免旧二进制的源码映射污染测量。
仓库内的[可审查 JSON 摘要](relay-protocol-refactor-coverage.json) 包含命令、源码哈希、
变更生产文件计数与未覆盖行、包汇总和基线；HTML 位于 `target/llvm-cov/html/index.html`。

首次默认并发的覆盖率运行中，原有
`filesystem::local::github_projects::tests::paseo::authentication_failure_during_protocol_lookup_is_not_hidden_by_https_fallback`
返回了 `SearchFailed`，而预期为 `Unauthenticated`。该测试已在普通全量测试中通过；
随后用相同插桩环境定向复查也通过，最终以 8 个测试线程运行的全量覆盖率全部通过。
根因未确定，未修改 filesystem 代码；若再次出现，应独立调查该测试的子进程执行失败。

尚未覆盖真实中心 TLS 互通、慢连接超时、并发会话达到上限及全部取消竞争路径；
行覆盖率为 100% 也不代表这些异步分支全部覆盖，后续应补充故障注入及真实中心互通验证。
