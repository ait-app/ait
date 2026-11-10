# Plans 待办核查与修复（2026-11-04）

## 范围

基线 `58cae1677b3c02e1e278ef84b0cd44b580e0cb96` 加本次工作区修改，macOS arm64，
Rust 1.98.1、Node 26.12.0。核对 `docs/plans` 的 34 个未勾选条目，完成两份审查清单中的
21 项代码修复，保留 13 项协议、语义或真实环境待办。逐项触发条件、修复与回归名称见
[仓库审查清单](../../../plans/repository-review-2026-11-02.md)与
[daemon 审查清单](../../../plans/daemon-review-2026-11-03.md)。

主要行为变化：

- 桌面退出只交接已输出就绪日志的 daemon；启动实例锁覆盖令牌、诊断和运行时重启，状态写入离开异步线程。
- 发布协议错误会退避重试；一次 401 恢复成功后，后续独立拒绝可以重新恢复。
- OMP 输入回执不再污染下一条消息 ID；OpenCode 回收超时发现进程、更新旧子 Agent 终态，未知子事件不使父回合失败。
- 访客断线或撤销期间完成创建的资源仍登记并检查授权；撤销逐页取消 Agent，终端仅清理已退出的记录。
- 共享项目保护配置文件及独立分享设置，Forge 显式仓库身份绑定 checkout；节点访客的 Bot 与定时任务改为只读，客户端同步隐藏写入口并恢复只读页面。
- 计划查询覆盖共享主机全部授权，按压徽标与原生高亮同步，连接恢复时关闭对应重连提示。

边界修订记录在 ADR-201、ADR-203、ADR-207 和 ADR-208；架构与操作入口已同步。

## Rust 定向验证

下列命令使用默认 features 并通过，未运行完整 Rust workspace 测试。构建使用临时 CMake，daemon/API 的本地监听测试
在允许回环网络的环境运行；所有测试使用隔离目录，未调用用户模型或修改用户运行中的 daemon。

```sh
export PATH=/private/tmp/ait-plan-tools/bin:$PATH
export SHERPA_ONNX_ARCHIVE_DIR=/private/tmp/ait-plan-native
export TMPDIR=/private/tmp
```

| 命令                                                                                                                                       | 结果                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------- |
| `cargo test --locked --offline -p provider --lib local::omp::session::tests`                                                               | 11 通过                                   |
| `cargo test --locked --offline -p provider --lib local::opencode::`                                                                        | 117 通过，30 个原生 opt-in 用例忽略       |
| `cargo test --locked --offline -p provider --lib local::codex::subagents::`                                                                | 11 通过                                   |
| `cargo test --locked --offline -p provider --lib local::opencode::subagents::tests`                                                        | 最后调整后复验 6 通过                     |
| `CARGO_TARGET_DIR=/private/tmp/ait-hostlink-todo-target cargo test --locked --offline -p host-link controller::tests`                      | 50 通过                                   |
| `cargo test --locked --offline -p daemon --bin daemon -- host::mcp_socket host::tests config::tests instance::tests device::tests::status` | 46 通过，110 未选中                       |
| `cargo test --offline --locked -p api guest::`                                                                                             | 49 通过                                   |
| `cargo test --offline --locked -p api --lib shares::`                                                                                      | 35 通过，最终终端容器调整后同范围复验通过 |
| `cargo test --offline --locked -p api --lib connection::rpc::`                                                                             | 2 通过                                    |
| `cargo test --offline --locked -p api --lib tests::relay_rpc`                                                                              | 5 通过                                    |
| `cargo test --offline --locked -p api --lib guest::policy::tests::regressions`                                                             | 最后调整后复验 7 通过                     |
| `cargo test --offline --locked -p filesystem --lib guest_repository_identity_must_match_the_checkout_origin`                               | 1 通过                                    |

以下隔离 daemon 进程测试 6 通过，104 未选中，覆盖端口失败、实例锁冲突、信号退出、运行时重启、
凭据恢复与无效启动参数；重启保持端口不变并更换实例 ID。

```sh
cargo test --locked -p daemon --test process --offline -- \
  occupied_port_does_not_create_credentials_or_diagnostics \
  owned_data_directory_is_not_modified_before_lock_acquisition \
  signal_shutdown_releases_process_lock_and_preserves_identity \
  websocket_restart_rebuilds_server_and_shutdown_exits_process \
  daemon_process_restarts_with_saved_local_token_without_environment \
  help_and_invalid_credentials_do_not_create_state
```

受影响 crate 的 lint 均通过：

```sh
cargo clippy --locked --offline -p provider --all-targets -- -D warnings
cargo clippy --locked --offline -p host-link --all-targets -- -D warnings
cargo clippy --locked --offline -p daemon --all-targets -- -D warnings
cargo clippy --locked --offline -p api -p filesystem --all-targets -- -D warnings
```

创建 PR 前补充执行 `cargo build --locked --offline --workspace` 与
`cargo clippy --locked --offline --workspace --all-targets -- -D warnings`，均通过；未扩大测试范围。

## 客户端验证

```sh
npm exec --workspace=@ait/desktop -- vitest run \
  src/daemon/rust-daemon-readiness.test.ts \
  src/daemon/rust-daemon.test.ts src/daemon/quit-lifecycle.test.ts
```

首轮 35 通过，8 个依赖真实 daemon 二进制的用例按配置跳过。构建完成后补跑下列命令，
`rust-daemon.test.ts` 的 13 项全部通过，没有跳过。修正了测试把源码构建版本固定为发布版本的旧断言，
现在将握手版本与所测二进制的 `--version` 核对，并验证重启后版本不变。

```sh
AIT_SERVER_BIN="$PWD/target/debug/daemon" TMPDIR=/private/tmp \
  npm exec --workspace=@ait/desktop -- vitest run src/daemon/rust-daemon.test.ts
```

```sh
npm exec --workspace=@ait/mobile -- vitest run --project unit \
  src/runtime/use-online-service-connection-notice.test.tsx \
  src/components/toast-host.test.tsx src/hooks/use-schedules.test.ts \
  src/hooks/use-schedules.hosts.test.tsx src/schedules/aggregated-schedules.test.ts \
  src/components/sidebar-press-feedback.test.tsx \
  src/components/sidebar/use-sidebar-row-press.test.tsx src/components/ui/context-menu.test.tsx
```

8 个文件最终共 32 项通过。首轮有一处 StrictMode 测试断言不匹配 React 测试环境，修正后该文件
6 项通过；徽标回归补充实际传给行内容的 backdrop 断言后，所在文件 12 项通过。

```sh
npm run test --workspace=@ait/mobile -- \
  src/bots/bot-chat.test.tsx src/schedules/workspace-schedule-tab.test.tsx \
  src/schedules/schedules-table.test.tsx src/schedules/workspace-schedule-form.test.tsx \
  src/screens/schedules-screen.test.tsx src/hosts/background-task-access.test.ts
npm run test --workspace=@ait/mobile -- src/bots/workspace-bot-tab.test.tsx
```

后台自动化只读 UI 的 7 个文件共 49 项通过。`npm run build:ui-deps`、Desktop 与 Mobile 的
`typecheck`、变更文件 `oxlint --deny-warnings` 和 `oxfmt --check` 通过。
`cargo fmt --all --check`、`npm run check:docs`（509 份 Markdown 的本地链接）、
`npm run check:ui` 和 `git diff --check` 通过。

## PR #573 CI 回归修复

提交 `a3ac3508` 的[首轮 CI](https://github.com/ait-app/ait/actions/runs/41685395367)
发现固定调度次数断言与过时诊断夹具两处问题：

- host-link 的取票恢复测试用固定次数的 `advance` 与 `yield_now` 代替完成条件，可能在刷新或
  重连尚未完成时断言失败。旧测试在 8 个并发进程中重复 100 次，失败 32 次；改为等待预期
  刷新次数和两个连接恢复，保留 15 秒虚拟时间上限、刷新次数上界及任务存活断言后，同样
  重复 100 次全部通过。`cargo test --locked --offline -p host-link controller::tests`
  的 50 项也全部通过，没有降低测试并行度或改变生产重试逻辑。
- Codex 的诊断夹具把未知但结构合法的 `subAgentActivity.kind` 当成损坏事件，仍期待父回合
  报协议错误，与新的兼容行为冲突。将损坏夹具改为无效的子线程身份，保留错误上下文和
  关闭后的诊断断言；另用未知类型与状态验证父回合完成、已知子 Agent 状态保留及下一回合可用。
  `cargo test --locked --offline -p provider --lib local::codex::tests::diagnostics::` 的 5 项和
  `cargo test --locked --offline -p provider --lib local::codex::subagents::` 的 11 项全部通过，
  共用夹具的正常子 Agent 完成回归也通过。

该正常完成回归的命令为：

```sh
cargo test --locked --offline -p provider --lib \
  local::codex::tests::progress::child_completion_keeps_its_link_in_live_and_native_history_and_allows_next_turn -- --exact
```

提交 `927df258` 的[第二轮 CI](https://github.com/ait-app/ait/actions/runs/41688255647)
中，上述失败均通过，但持续 Hub 拒绝测试触发 10 秒超时。本地同条件重复 100 次失败 2 次，
进一步定位到生产调度：拒绝结果与续租同时就绪时，随机 `select!` 可以反复先选续租，且每次
成功后还会等待轮询间隔。现在优先处理已排队的拒绝，沿用原有 generation 过滤和单次刷新预算。
修复后该测试重复 100 次全部通过；恢复模块的 10 项测试整组重复 100 次也全部通过，
完整 controller 范围的 50 项通过。未放宽持续拒绝测试的超时上限或降低并行度。

## 未完成边界

- OpenCode 子会话恢复通知仍需支持对应扩展的真实版本确认；本机 2.0.32 不足以验收。
- 被拒访客后续是否仍被中心路由需要中心协议证据。
- 语音与定时任务的整体超时需先明确最长运行时间、取消语义和配置方式。
- 客户端用户会话续期的 5 项协议与发布工作仍待中心支持。
- 远程性能的 5 项采样、优化决策和平台验收仍待真实 TCP/SSH/Hub 与目标平台环境；没有性能分位数测量。
- Bot 的 Email、流式订阅、uncertain 投递恢复及真实平台验收仍属路线事项，不在上述 34 个复选框内。

未进行 Android/iOS 真机视觉验收或 Linux/Windows 平台验收。原生按压修复通过手势回调与行内容
状态的定向回归验证，不能替代安装包验收。

## Test coverage

**not measured**：用户未要求覆盖率，只执行受影响代码及直接相关行为的测试。没有当前覆盖率
百分比、可比覆盖率基线或覆盖率 artifact；通过用例数不代表覆盖率。后续如请求测量，使用
`cargo llvm-cov` 限定上述受影响 crate 与测试，另行生成并分享覆盖行数和 HTML 报告。
