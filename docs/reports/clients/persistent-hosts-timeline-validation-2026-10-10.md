# 在线服务主机持久保存与 OpenCode 时间线：提交验证

2026-10-10。源码提交 `1f755b47ec296ab195252c20528f9af2b8121d8a`，基于 main
`4fecd83a`；本报告及统计文件在后续文档提交中添加。测量前后的 997 个 Rust、Cargo、nextest
配置及 Python fixture 文件指纹一致，完整 SHA-256 见[覆盖率证据](persistent-hosts-timeline-coverage-2026-10-10.json)。
平台为 macOS 27.0.1 / aarch64，Rust 1.98.1，cargo-nextest 0.9.148，cargo-llvm-cov 0.8.4。

## 改动与验收范围

- 在线服务主机显式添加后写入客户端注册表，重启恢复多台主机；登录状态决定连接可用性。
  登出保留记录，删除独立持久化，损坏记录逐条过滤。连接携带规范化服务地址，访问和下载校验当前账户服务。
  参见 [ADR-122](../../decisions/clients/adr-122-persistent-online-service-hosts.md)。
- 同一 daemon 合并中继与直连配置并保留用户名称、外观；切换账户选择不关闭其他主机的传输。
  下载使用当前连接对应的中继服务。
- 沿用 main 在 PR #255、#256、#258 中实现的 OpenCode 输入先行、原生历史结算与错误解释行为，
  补充多轮顺序、`clientMessageId`、时间保留、SQLite 重开及 AgentManager 完成前结算回归。
- 默认并行 nextest 首次发现 daemon 退出后立即检查原生 PID 的竞争：2038 通过、1 失败。
  退出验收改为两秒内有界等待系统回收，归档操作的即时退出断言保留。修正后相关 20 项、
  完整 nextest 和覆盖率测试均通过，全程使用默认并行度。

## 测试执行

| 检查                                                                       | 结果                                                |
| -------------------------------------------------------------------------- | --------------------------------------------------- |
| `cargo nextest run --workspace --locked --offline`                         | 2039 通过，16 跳过                                  |
| `cargo test --workspace --doc --locked --offline`                          | 12 个 crate 的文档测试发现成功，当前共 0 个 doctest |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 通过                                                |
| `cargo build --workspace --locked --offline`                               | 通过，无编译警告                                    |
| `cargo fmt --all --check`                                                  | 通过                                                |
| 真实 OpenCode 2.0.25 多轮、时间顺序、关闭恢复及发现                        | 1 通过；隔离 XDG 目录和回环模型                     |
| Mobile 主机、账户、中继、下载及 schema 相关 Vitest                         | 12 文件，185 通过                                   |
| Desktop 账户下载、会话及 IPC 相关 Vitest                                   | 3 文件，25 通过                                     |
| Client SDK 账户会话 Vitest                                                 | 1 文件，35 通过                                     |
| SDK 构建及 Client / Desktop / Mobile 类型检查                              | 通过                                                |
| 改动文件 oxfmt / oxlint、`npm run check:docs`、`git diff --check`          | 通过                                                |

真实 OpenCode 验收命令：

```sh
AIT_TEST_OPENCODE_BIN=/opt/homebrew/bin/opencode cargo test -p provider installed_acp_multi_turn_native_history_resume_and_discovery --locked --offline -- --ignored
```

客户端相关检查命令：

```sh
npm run build:sdk
npm run typecheck --workspace=@ait/client --workspace=@ait/desktop --workspace=@ait/mobile
npm run test --workspace=@ait/client -- src/account-session.test.ts
npm run test --workspace=@ait/desktop -- src/daemon/account-download.test.ts src/daemon/account-session.test.ts src/daemon/account-ipc.test.ts
npm run test --workspace=@ait/mobile -- --project unit src/runtime/host-runtime.test.ts src/runtime/account-state.native.test.ts src/runtime/account-state.desktop.test.ts src/runtime/rust-daemon/account-relay-target.test.ts src/runtime/rust-daemon/account-transport.desktop.test.ts src/runtime/rust-daemon/account-transport.native.test.ts src/runtime/rust-daemon/native-account-transport.test.ts src/runtime/rust-daemon/native-account-download.test.ts src/runtime/rust-daemon/transport.test.ts src/stores/download-store.test.ts src/stores/download-store.native.test.ts src/types/host-connection.test.ts
```

初次 Mobile 类型检查遇到本地已安装 lucide-react-native 0.546.0、锁文件要求 1.50.0 的差异。
按锁文件 SHA-512 校验并同步本地依赖后检查通过；依赖清单和锁文件没有新增改动。

## Test coverage

| Rust 范围                   | 已覆盖 / 总行数 | 行覆盖率 |
| --------------------------- | --------------- | -------- |
| Workspace，13 个包          | 56524 / 59701   | 94.68%   |
| Provider                    | 27028 / 28697   | 94.18%   |
| Daemon                      | 1318 / 1391     | 94.75%   |
| OpenCode `session.rs`       | 568 / 631       | 90.02%   |
| AgentManager `streaming.rs` | 321 / 333       | 96.40%   |
| Timeline `timeline.rs`      | 288 / 292       | 98.63%   |
| Timeline `progress.rs`      | 246 / 251       | 98.01%   |

测量上述源码提交的完整 workspace、默认 features，使用 llvm-cov 默认排除规则，
没有额外文件或包排除。覆盖率运行另有 2039 项通过、16 项跳过；通过用例数与行覆盖率分别记录。
本次未重新测量相同 runner 和范围下的当前 main，无可比基线；历史报告不用于声明覆盖率增减。

```sh
cargo llvm-cov nextest --workspace --locked --offline --html
cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-submit-coverage-summary.json
cargo llvm-cov report --lcov --output-path /private/tmp/ait-submit-coverage.lcov
```

可审阅[统计证据](persistent-hosts-timeline-coverage-2026-10-10.json)记录各包计数、源码指纹及相关生产文件的
LCOV 未覆盖行；本地完整 HTML 位于 `target/llvm-cov/html/index.html`。OpenCode ACP
`session.rs` 456–475 行的远端 `$/cancel_request` 和未知客户端 RPC 响应路径未被本次测量执行，
后续修改这些路径时需补充对应 fixture 通知与响应断言。

范围限制：覆盖率未包含 ignored 原生验收、doctest 或原生 CLI 二进制；真实 OpenCode 测试单独执行。
TypeScript 相关测试的覆盖率未测量。在线服务中心票据请求、原生安全存储及下载依赖使用 mock，
未运行生产中心、打包 Electron 或 iOS / Android 真机端到端验收，也未在 Linux / Windows 执行本次 Rust 测量。
