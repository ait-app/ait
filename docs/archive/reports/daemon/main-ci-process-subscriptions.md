# main CI 进程与文件订阅修复

- 日期：2026-09-27。
- 分支 `brown-liger` 已 rebase 到 `origin/main`：`db506dd4d5137211782d59e4d419104095e97f94`。
- rebase 前的 session 标题修复通过包含新增文件的 stash 完整保存并恢复，没有冲突。
- 本次修改没有改变领域、协议或存储 schema 边界。

## 远端失败与处理

[最新 main CI](https://github.com/necokeine/ait/actions/runs/36321844276) 的 desktop job 成功，
Rust job 在 `local::claude::tests::process::native_result_errors_and_initialize_deadlines_are_not_successes`
首次 `create_session` 时返回 `Unavailable`，尚未执行该测试后半部分的 100 ms 初始化超时断言。

Claude 测试此前为每个用例写入临时可执行脚本后立即启动；现在使用 checkout 中固定的
`tests/fixtures/claude_code.py`（Git 可执行位 `100755`），只把 cwd 和配置目录留在各用例的
临时目录中。生产启动逻辑、错误传播和超时断言均保留，没有增加失败重试或串行化测试。
这消除了测试期间新脚本写入与并行进程启动的竞争窗口。

远端日志只保留了安全错误 `Unavailable`，没有底层 OS errno，因此不能确认具体是
`ETXTBSY`。这是根据失败阶段和临时 executable 写入方式的推断；Linux 对仍被写入打开的
executable 会返回该错误，见 [execve(2)](https://man7.org/linux/man-pages/man2/execve.2.html)。
本机没有运行中的 Linux 容器，具体 Linux 启动失败需要提交后的 CI 继续验证。

[前一次 main CI](https://github.com/necokeine/ait/actions/runs/36310638510) 则在
`identical_file_observations_keep_independent_release_and_idle_peers_receive_no_updates`
等待两个独立文件订阅更新时超过 10 秒。

文件订阅每 200 ms 用 `try_acquire_owned` 争用同一个许可，失败直接跳过本轮。
当订阅的轮询周期对齐时，没有排队公平性保障，同一订阅可能反复错失读取机会。
新增确定性回归用例持有许可并按顺序 poll 两个等待者，旧抢占实现立即失败；修复后两者
公平排队，订阅释放、server shutdown、outbound 断线及 semaphore 关闭均取消等待。
已有 blocking 任务追踪和连接所有权保护继续有效。

## 验证

所有测试使用离线替身和临时数据；本次没有修改正在运行的用户 Server。

- `cargo test -p filesystem connection::`：15 通过，包括新增 2 项竞争/取消回归。
- `cargo test -p provider local::claude::tests::`：33 通过，2 项既有真实 Provider 测试 ignored。
- `cargo test -p daemon --test process unix::files::`：8 通过，包括远端失败的 WebSocket 回归。
- `cargo test -p provider titles`：7 通过，验证保留的标题修复与最新 main 兼容。
- `cargo fmt --all --check`、两个修改 crate 的 `cargo clippy --all-targets -- -D warnings`、`git diff --check` 通过。

上述普通测试使用 `CARGO_TARGET_DIR=/private/tmp/ait-agent-interface-target`。
WebSocket 集成测试获准在沙箱外运行，以绑定本地回环端口。
准备提交时已追加完整 workspace 验证：普通测试 1717 通过、0 失败、8 ignored；
完整构建无警告，workspace Clippy、格式检查均通过。本报告不将本地通过等同于远端 CI 已经变绿。

## Test coverage

提交前测量 revision 为本文 `origin/main` 基线加本次修改，平台 macOS arm64、workspace
默认 features，没有额外文件排除。工具默认不计测试源码且不插桩 doctest；8 项既有 ignored
测试未启用，Linux 和 Windows 未在本地测量。没有同范围 baseline，因此不报告覆盖率增减。

| 测量范围                                  | 已覆盖 / 总行数 | 行覆盖率 |
| ----------------------------------------- | --------------- | -------- |
| workspace                                 | 57,796 / 67,756 | 85.30%   |
| `filesystem`                              | 7,576 / 8,629   | 87.80%   |
| `provider`                                | 14,537 / 15,679 | 92.72%   |
| 改动文件 `connection/files/connection.rs` | 241 / 256       | 94.14%   |

完整插桩测试 1716 通过、0 失败、8 ignored（普通测试额外包含 1 项 doctest）。
新增排队顺序与四类退出路径均有回归断言；改动文件仍有异常、取消和传输路径未执行。
当前测量覆盖本地离线夹具，Linux 进程启动错误与真实 Provider 行为仍需对应环境验证。

```sh
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov clean --workspace
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov --workspace --html
CARGO_TARGET_DIR=/private/tmp/ait-session-titles-coverage cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-pr-workspace-coverage.json
```

可评审产物：[workspace 与逐 crate/文件行覆盖率、源码 SHA-256 和执行结果](../providers/server-session-titles-coverage.json)。
本地 HTML：`/private/tmp/ait-session-titles-coverage/llvm-cov/html/index.html`。
远端 Linux CI 未重跑；Claude 具体启动错误的 errno 仍待该环境验证。
