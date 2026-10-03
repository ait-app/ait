# 账户主机中继验证

- 测量日期：2026-10-03
- 源码：`a0ff647b`，基于主分支 `6ce645040c0ef03d9d677ce9224c0fa9cdcb028b`；精确源码哈希见[覆盖率摘要](account-host-relay-coverage.json)。
- 平台：Linux x86_64，Rust 1.98.1，cargo-llvm-cov 0.9.1，默认 features。
- 范围：桌面账户登录、主机注册与发现、显式选择、反向中继和独立下载；涉及 `apps/desktop`、`apps/mobile`、`bins/daemon` 与 `crates/relay`。

这是账户中继初版的历史验证记录。协议模块整理后的验证见
[协议重构报告](relay-protocol-refactor-validation.md)；Android 适配由
[ADR-076](../../decisions/clients/adr-076-android-account-relay.md) 记录，不在本次测量范围内。

## 实现范围

默认账户 API 地址为 `https://dash.ait-app.com:8443/api`。登录操作与注册、控制连接共用的
重试延迟提取为具名辅助函数。主分支整合保留 daemon 版本、后台 Git fetch 组装与已有能力标识。
目录测试区分 `connection.single.v1` 协议标记和 RPC 方法。
依赖守卫允许 `api` 持有 relay adapter，同时禁止 `relay` 依赖其他 workspace crate。
边界见 [ADR-074](../../decisions/clients/adr-074-account-host-relay.md)。

## 测试结果

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 和
  `cargo build --workspace --offline` 通过。
- `cargo test --workspace --offline`：1,578 通过、0 失败、3 忽略。
- 共享界面的登录与 Rust transport 测试：58 通过，包含真实 daemon 集成测试。
- 桌面账户、daemon 管理与 Rust 生命周期测试：19 通过，覆盖启动、重启、监听配置恢复、
  运行时身份与进程所有权。
- 共享界面和桌面 TypeScript 检查通过；变更界面的 ESLint 及桌面账户、运行时文件的
  Oxlint 检查无警告。
- 22 个变更 TypeScript 文件的 Oxfmt 检查及 `git diff --check` 通过。

真实 daemon 测试使用新构建的 `target/debug/daemon`，分别通过 `AIT_TEST_RUST_SERVER`
和 `AIT_SERVER_BIN` 指定。忽略的三项 Rust 测试要求本机已安装、已认证的 Claude 或 Codex
CLI，其中两项执行真实推理。上述检查不包含生产部署或线上账户写操作。

## Test coverage

工作区行覆盖率为 **91.76%（44,517 / 48,513）**。测量使用默认源码过滤；
源码文件哈希和完整工作区指纹保存在[覆盖率摘要](account-host-relay-coverage.json)，
文档编辑不改变其标识的测量源码。

```sh
cargo llvm-cov --workspace --html --locked --offline
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-pr138-current-summary.json
cargo llvm-cov report --lcov --output-path /tmp/ait-pr138-current.lcov
```

插桩测试独立得到 1,578 通过、3 忽略，不与普通测试数相加。doctest 未插桩。
macOS 和 Windows 未验证；没有测量可比较的 Linux 基线，因此不声明覆盖率增量。

| 范围           | 行覆盖率 |   覆盖行 / 总行 |
| -------------- | -------: | --------------: |
| Rust workspace |   91.76% | 44,517 / 48,513 |
| `api`          |   92.41% |   1,864 / 2,017 |
| `daemon`       |   94.31% |       879 / 932 |
| `model`        |   94.82% |       696 / 734 |
| `protocol`     |  100.00% |         73 / 73 |
| `relay`        |   60.83% |       278 / 457 |

本地 HTML 报告生成于 `target/llvm-cov/html/index.html`；仓库中的 JSON 摘要用于共享审查，
包含各 crate 统计、源码哈希和变更生产文件的未覆盖行。

本次测量中，下载路径为 0 / 134 行，单连接调度器为 150 / 205 行，relay bridge 为
53 / 67 行。此版本的缺口包括下载传输、失败与取消、调度饱和及停机竞争。
后续协议重构报告补充了下载回归和新的测量结果；早期人工端到端检查不计入本报告覆盖率。
