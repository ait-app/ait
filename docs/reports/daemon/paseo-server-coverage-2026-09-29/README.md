# Paseo server 定向覆盖率证据

测量基线为 `b797e0d2f83ae57f62892c288a5d81776f8afa6a` 加本次修改。
`coverage.json` 保存每个测量源文件的 SHA-256、未覆盖行、修改行命中数及测试命令。
本次没有可比较的旧覆盖率基线；此处保留本地迭代定向测量。
提交前完整 workspace 测量另见[PR 验证报告](../paseo-server-pr-validation-2026-09-29.md)。

## Test coverage

| 测量口径                        | 覆盖率 | 已覆盖 / 总行数 |
| ------------------------------- | -----: | --------------: |
| 选定 crate 的全部已插桩生产文件 | 68.95% |   26317 / 38168 |
| 本次改动的生产文件（整个文件）  | 86.67% |   15864 / 18303 |
| 新增或修改的可执行行            | 95.14% |     4657 / 4895 |

不是逐接口语义覆盖率，也不是 workspace 覆盖率。macOS arm64，默认 features。
测试文件和 test_support 被排除；不可执行的类型定义、注释及未编译的平台代码不进入 LLVM 行分母。
Linux/Windows 和真实认证 Provider 未运行。3 个既有在线 Provider 测试保持 ignored。

复现：`python3 scripts/paseo-focused-coverage.py --run`。该脚本只运行下面列出的定向测试。
HTML 位于 `target/llvm-cov/html/index.html`；可评审的文件/行证据为 [coverage.json](coverage.json)。

| 测试目标   | 通过 | ignored | 覆盖率 | 已覆盖 / 总行数 |
| ---------- | ---: | ------: | -----: | --------------: |
| provider   |  427 |       3 | 88.51% |   15328 / 17318 |
| model      |   30 |       0 |  95.4% |       601 / 630 |
| metadata   |   99 |       0 | 66.74% |     4829 / 7235 |
| filesystem |   72 |       0 | 23.74% |     2147 / 9042 |
| terminal   |   28 |       0 | 86.83% |     1233 / 1420 |
| api        |   44 |       0 | 88.29% |     1418 / 1606 |
| daemon     |   46 |       0 | 83.16% |       721 / 867 |
| protocol   |    4 |       0 |  80.0% |         40 / 50 |

测试数与覆盖率分开统计；每个测试目标只统计此次干净测量中的一次执行。

## 重要未覆盖行为

部分持久化/队列故障分支，以及操作系统拒绝终止进程后保留清理责任的分支，仍需故障注入验证。
真实 Codex/Claude 认证推理、GitHub/GHES 网络和 push、Linux/Windows 需在相应环境另行验证。
全局 session event 生产者和普通 Agent 失败重试等剩余兼容差异见[主报告](../paseo-api-audit-2026-09-29.md)。
定向测试只覆盖改动及直接相关行为，未为提高比例运行其他未改动模块；因此所选 crate 的完整文件分母仍包含未执行的旧路径。

## 精确命令

```sh
cargo llvm-cov clean --workspace
cargo llvm-cov test --locked --offline -p provider --lib --no-report -- service::agent_execution:: service::agent_manager:: service::agent_runtime:: service::provider_catalog:: service::workspace_attention:: rpc::timeline:: rpc::fork_context:: rpc::agent_execution:: rpc::agent_runtime:: connection:: local::codex:: local::claude:: storage::timeline:: ports::environment:: ports::agent_session:: protocol::tests::
cargo llvm-cov test --locked --offline -p model --lib --no-report -- pagination:: directory_sync:: polling:: runtime:: events::
cargo llvm-cov test --locked --offline -p metadata --lib --no-report -- service::directory:: service::session:: service::creation:: workspace_automation:: protocol::worktree_source:: rpc::directory:: protocol::directory:: protocol::workspace::
cargo llvm-cov test --locked --offline -p filesystem --lib --no-report -- worktrees:: worktree_checkout::
cargo llvm-cov test --locked --offline -p terminal --lib --no-report -- service:: activity::
cargo llvm-cov test --locked --offline -p api --lib --no-report -- terminal_activity:: listener:: capabilities:: auth:: browser_auth:: tests::session:: tests::paseo::
cargo llvm-cov test --locked --offline -p daemon --test process --no-report -- agent_execution:: agent_controls:: agent_history:: terminal:: worktrees:: workspace_automation:: directory:: native_sessions:: schedule:: session::
cargo llvm-cov test --locked --offline -p protocol --lib --no-report -- methods::
cargo llvm-cov report --lcov --output-path target/paseo-focused-coverage/coverage.lcov --ignore-filename-regex '/(tests|test_support)(/|\.rs$)'
cargo llvm-cov report --html --ignore-filename-regex '/(tests|test_support)(/|\.rs$)'
```

## 改动文件

文件名和计数保留测量时的历史路径；已迁移文件的链接指向当前定义，本报告未重新测量覆盖率。

| 文件                                                                                                                                       | 已覆盖 / 总行数 | 新增修改行：已覆盖 / 总行数 |
| ------------------------------------------------------------------------------------------------------------------------------------------ | --------------: | --------------------------: |
| [bins/daemon/src/host.rs](../../../../bins/daemon/src/host.rs)                                                                             |       369 / 372 |                     41 / 41 |
| [bins/daemon/src/host/schedule.rs](../../../../bins/daemon/src/host/schedule.rs)                                                           |       225 / 248 |                       3 / 3 |
| [crates/api/src/capabilities.rs](../../../../crates/api/src/capabilities.rs)                                                               |         87 / 87 |                     22 / 22 |
| [crates/api/src/connection.rs](../../../../crates/api/src/connection.rs)                                                                   |       263 / 315 |                     11 / 14 |
| [crates/api/src/connection/creation_receipts.rs](../../../../crates/api/src/connection/creation_receipts.rs)                               |         41 / 41 |                     41 / 41 |
| [crates/api/src/connection/dispatch.rs](../../../../crates/api/src/connection/dispatch.rs)                                                 |        81 / 108 |                     18 / 18 |
| [crates/api/src/connection/workspace_archive.rs](../../../../crates/api/src/connection/workspace_archive.rs)                               |       130 / 134 |                   130 / 134 |
| [crates/api/src/connection/workspace_creation.rs](../../../../crates/api/src/connection/workspace_creation.rs)                             |       182 / 187 |                   182 / 187 |
| [crates/api/src/lib.rs](../../../../crates/api/src/lib.rs)                                                                                 |       302 / 320 |                     23 / 23 |
| [crates/api/src/listener.rs](../../../../crates/api/src/listener.rs)                                                                       |         31 / 31 |                       6 / 6 |
| [crates/api/src/terminal_activity.rs](../../../../crates/api/src/terminal_activity.rs)                                                     |         47 / 49 |                     47 / 49 |
| [crates/api/src/workspace_cleanup.rs](../../../../crates/api/src/workspace_cleanup.rs)                                                     |         27 / 35 |                     27 / 35 |
| [crates/filesystem/src/local/forge.rs](../../../../crates/filesystem/src/local/forge.rs)                                                   |      134 / 1076 |                       1 / 1 |
| [crates/filesystem/src/local/forge/worktree_checkout.rs](../../../../crates/filesystem/src/local/forge/worktree_checkout.rs)               |       139 / 162 |                   139 / 162 |
| [crates/filesystem/src/local/worktrees.rs](../../../../crates/filesystem/src/local/worktrees.rs)                                           |       663 / 727 |                     25 / 25 |
| [crates/filesystem/src/local/worktrees/change_request.rs](../../../../crates/filesystem/src/local/worktrees/change_request.rs)             |       101 / 112 |                   101 / 112 |
| [crates/filesystem/src/local/worktrees/directory.rs](../../../../crates/filesystem/src/local/worktrees/directory.rs)                       |         59 / 62 |                     59 / 62 |
| [crates/filesystem/src/ports/worktrees.rs](../../../../crates/filesystem/src/ports/worktrees.rs)                                           |           8 / 8 |                       8 / 8 |
| [crates/filesystem/src/protocol/worktrees.rs](../../../../crates/filesystem/src/protocol/worktrees.rs)                                     |         59 / 87 |                       1 / 1 |
| [crates/filesystem/src/rpc/worktrees.rs](../../../../crates/filesystem/src/rpc/worktrees.rs)                                               |       147 / 170 |                     23 / 24 |
| [crates/filesystem/src/service/worktrees.rs](../../../../crates/filesystem/src/service/worktrees.rs)                                       |       418 / 475 |                    98 / 101 |
| [crates/filesystem/src/service/worktrees/provisioning.rs](../../../../crates/filesystem/src/service/worktrees/provisioning.rs)             |       119 / 123 |                     45 / 45 |
| [crates/metadata/src/connection.rs](../../../../crates/metadata/src/connection.rs)                                                         |         32 / 35 |                       2 / 2 |
| [crates/metadata/src/connection/creation.rs](../../../../crates/metadata/src/connection/creation.rs)                                       |       111 / 117 |                       9 / 9 |
| [crates/metadata/src/connection/directory.rs](../../../../crates/metadata/src/connection/directory.rs)                                     |         49 / 52 |                     49 / 52 |
| [crates/metadata/src/connection/session.rs](../../../../crates/metadata/src/connection/session.rs)                                         |         81 / 85 |                       6 / 6 |
| [crates/metadata/src/dispatch.rs](../../../../crates/metadata/src/dispatch.rs)                                                             |       104 / 210 |                       8 / 8 |
| [crates/metadata/src/local/workspace_automation.rs](../../../../crates/metadata/src/local/workspace_automation.rs)                         |       478 / 534 |                     23 / 29 |
| [crates/metadata/src/local/workspace_automation/retirement.rs](../../../../crates/metadata/src/local/workspace_automation/retirement.rs)   |         52 / 61 |                     52 / 61 |
| [crates/metadata/src/model/workspace_activity.rs](../../../../crates/model/src/workspace/activity.rs)                             |           8 / 8 |                       8 / 8 |
| [crates/metadata/src/protocol/directory.rs](../../../../crates/model/src/workspace/protocol/directory.rs)                                         |         37 / 37 |                       0 / 0 |
| [crates/metadata/src/protocol/session.rs](../../../../crates/model/src/session/protocol.rs)                                             |         11 / 11 |                       4 / 4 |
| [crates/metadata/src/protocol/workspace.rs](../../../../crates/model/src/workspace/protocol/workspace.rs)                                         |         32 / 32 |                       0 / 0 |
| [crates/metadata/src/protocol/worktree_source.rs](../../../../crates/model/src/workspace/protocol/worktree_source.rs)                             |         16 / 16 |                     16 / 16 |
| [crates/metadata/src/rpc/directory.rs](../../../../crates/metadata/src/rpc/directory.rs)                                                   |       578 / 735 |                     77 / 84 |
| [crates/metadata/src/rpc/directory/listing.rs](../../../../crates/metadata/src/rpc/directory/listing.rs)                                   |       212 / 225 |                   212 / 225 |
| [crates/metadata/src/rpc/directory/pagination.rs](../../../../crates/metadata/src/rpc/directory/pagination.rs)                             |         81 / 88 |                     81 / 88 |
| [crates/metadata/src/service/creation.rs](../../../../crates/model/src/creation.rs)                                             |       255 / 257 |                     99 / 99 |
| [crates/metadata/src/service/directory.rs](../../../../crates/metadata/src/service/directory.rs)                                           |       718 / 778 |                     37 / 38 |
| [crates/metadata/src/service/directory/activity.rs](../../../../crates/metadata/src/service/directory/activity.rs)                         |         61 / 61 |                     61 / 61 |
| [crates/metadata/src/service/session.rs](../../../../crates/model/src/session.rs)                                               |       207 / 212 |                     10 / 10 |
| [crates/metadata/src/service/workspace_automation.rs](../../../../crates/metadata/src/service/workspace_automation.rs)                     |       129 / 134 |                       8 / 8 |
| [crates/model/src/directory_sync.rs](../../../../crates/model/src/directory_sync.rs)                                                       |         88 / 88 |                     88 / 88 |
| [crates/model/src/events.rs](../../../../crates/model/src/events.rs)                                                                       |       108 / 112 |                     16 / 16 |
| [crates/model/src/lib.rs](../../../../crates/model/src/lib.rs)                                                                             |           3 / 3 |                       0 / 0 |
| [crates/model/src/pagination.rs](../../../../crates/model/src/pagination.rs)                                                               |       108 / 110 |                   108 / 110 |
| [crates/model/src/pagination/collation.rs](../../../../crates/model/src/pagination/collation.rs)                                           |         25 / 25 |                     25 / 25 |
| [crates/model/src/polling.rs](../../../../crates/model/src/polling.rs)                                                                     |         62 / 65 |                     62 / 65 |
| [crates/model/src/runtime.rs](../../../../crates/model/src/runtime.rs)                                                                     |         72 / 73 |                     30 / 31 |
| [crates/model/src/server.rs](../../../../crates/model/src/server.rs)                                                                       |           8 / 8 |                       0 / 0 |
| [crates/provider/src/connection.rs](../../../../crates/provider/src/connection.rs)                                                         |       124 / 128 |                     10 / 10 |
| [crates/provider/src/connection/directory.rs](../../../../crates/provider/src/connection/directory.rs)                                     |       114 / 132 |                   114 / 132 |
| [crates/provider/src/dispatch.rs](../../../../crates/provider/src/dispatch.rs)                                                             |        72 / 108 |                     31 / 58 |
| [crates/provider/src/local/claude.rs](../../../../crates/provider/src/local/claude.rs)                                                     |       185 / 202 |                     17 / 21 |
| [crates/provider/src/local/claude/transport.rs](../../../../crates/provider/src/local/claude/transport.rs)                                 |       236 / 253 |                       4 / 4 |
| [crates/provider/src/local/codex.rs](../../../../crates/provider/src/local/codex.rs)                                                       |       769 / 845 |                     47 / 48 |
| [crates/provider/src/local/codex/controls.rs](../../../../crates/provider/src/local/codex/controls.rs)                                     |       212 / 216 |                       4 / 4 |
| [crates/provider/src/local/codex/discovery.rs](../../../../crates/provider/src/local/codex/discovery.rs)                                   |       217 / 228 |                       1 / 1 |
| [crates/provider/src/local/codex/metadata.rs](../../../../crates/provider/src/local/codex/metadata.rs)                                     |         64 / 70 |                       1 / 1 |
| [crates/provider/src/local/codex/native_sessions.rs](../../../../crates/provider/src/local/codex/native_sessions.rs)                       |       181 / 184 |                       2 / 2 |
| [crates/provider/src/local/codex/rewind.rs](../../../../crates/provider/src/local/codex/rewind.rs)                                         |         90 / 94 |                       1 / 1 |
| [crates/provider/src/local/codex/transport.rs](../../../../crates/provider/src/local/codex/transport.rs)                                   |       238 / 258 |                     15 / 15 |
| [crates/provider/src/ports/agent_session.rs](../../../../crates/provider/src/ports/agent_session.rs)                                       |        63 / 192 |                     24 / 24 |
| [crates/provider/src/ports/environment.rs](../../../../crates/provider/src/ports/environment.rs)                                           |         28 / 28 |                     28 / 28 |
| [crates/provider/src/protocol/agent_execution.rs](../../../../crates/provider/src/protocol/agent_execution.rs)                             |           9 / 9 |                       0 / 0 |
| [crates/provider/src/protocol/agent_lifecycle.rs](../../../../crates/provider/src/protocol/agent_lifecycle.rs)                             |           0 / 6 |                       0 / 0 |
| [crates/provider/src/protocol/resume.rs](../../../../crates/provider/src/protocol/resume.rs)                                               |         38 / 38 |                     38 / 38 |
| [crates/provider/src/rpc/agent_execution.rs](../../../../crates/provider/src/rpc/agent_execution.rs)                                       |       524 / 547 |                   156 / 160 |
| [crates/provider/src/rpc/agent_execution/native_sessions.rs](../../../../crates/provider/src/rpc/agent_execution/native_sessions.rs)       |       159 / 163 |                       1 / 1 |
| [crates/provider/src/rpc/agent_execution/placement.rs](../../../../crates/provider/src/rpc/agent_execution/placement.rs)                   |         77 / 79 |                     77 / 79 |
| [crates/provider/src/rpc/agent_execution/resume.rs](../../../../crates/provider/src/rpc/agent_execution/resume.rs)                         |         86 / 90 |                     86 / 90 |
| [crates/provider/src/rpc/agent_execution/workspace_creation.rs](../../../../crates/provider/src/rpc/agent_execution/workspace_creation.rs) |        84 / 105 |                    84 / 105 |
| [crates/provider/src/rpc/agent_execution/worktrees.rs](../../../../crates/provider/src/rpc/agent_execution/worktrees.rs)                   |       221 / 224 |                   221 / 224 |
| [crates/provider/src/rpc/agent_runtime.rs](../../../../crates/provider/src/rpc/agent_runtime.rs)                                           |       351 / 426 |                     51 / 52 |
| [crates/provider/src/rpc/agent_runtime/listing.rs](../../../../crates/provider/src/rpc/agent_runtime/listing.rs)                           |         45 / 46 |                     45 / 46 |
| [crates/provider/src/rpc/fork_context.rs](../../../../crates/provider/src/rpc/fork_context.rs)                                             |       108 / 113 |                     47 / 52 |
| [crates/provider/src/rpc/fork_context/tools.rs](../../../../crates/provider/src/rpc/fork_context/tools.rs)                                 |         80 / 80 |                     80 / 80 |
| [crates/provider/src/rpc/timeline.rs](../../../../crates/provider/src/rpc/timeline.rs)                                                     |       114 / 114 |                     25 / 25 |
| [crates/provider/src/rpc/timeline/projection.rs](../../../../crates/provider/src/rpc/timeline/projection.rs)                               |       130 / 141 |                   130 / 141 |
| [crates/provider/src/rpc/timeline/projection/page.rs](../../../../crates/provider/src/rpc/timeline/projection/page.rs)                     |         99 / 99 |                     99 / 99 |
| [crates/provider/src/service/agent_execution.rs](../../../../crates/provider/src/service/agent_execution.rs)                               |       131 / 140 |                     16 / 16 |
| [crates/provider/src/service/agent_execution/waits.rs](../../../../crates/provider/src/service/agent_execution/waits.rs)                   |         52 / 56 |                     52 / 56 |
| [crates/provider/src/service/agent_manager.rs](../../../../crates/provider/src/service/agent_manager.rs)                                   |       736 / 768 |                     51 / 52 |
| [crates/provider/src/service/agent_manager/auto_archive.rs](../../../../crates/provider/src/service/agent_manager/auto_archive.rs)         |         96 / 99 |                     96 / 99 |
| [crates/provider/src/service/agent_manager/controls.rs](../../../../crates/provider/src/service/agent_manager/controls.rs)                 |       469 / 518 |                     16 / 17 |
| [crates/provider/src/service/agent_manager/resume.rs](../../../../crates/provider/src/service/agent_manager/resume.rs)                     |       111 / 114 |                   111 / 114 |
| [crates/provider/src/service/agent_manager/streaming.rs](../../../../crates/provider/src/service/agent_manager/streaming.rs)               |       255 / 289 |                       7 / 7 |
| [crates/provider/src/service/agent_runtime.rs](../../../../crates/provider/src/service/agent_runtime.rs)                                   |       357 / 388 |                     95 / 99 |
| [crates/provider/src/service/agent_runtime/archive.rs](../../../../crates/provider/src/service/agent_runtime/archive.rs)                   |       116 / 116 |                   116 / 116 |
| [crates/provider/src/service/agent_runtime/search.rs](../../../../crates/provider/src/service/agent_runtime/search.rs)                     |       121 / 122 |                   121 / 122 |
| [crates/provider/src/service/provider_catalog.rs](../../../../crates/provider/src/service/provider_catalog.rs)                             |       189 / 190 |                     20 / 20 |
| [crates/provider/src/service/provider_catalog/draft.rs](../../../../crates/provider/src/service/provider_catalog/draft.rs)                 |         30 / 30 |                     30 / 30 |
| [crates/provider/src/service/provider_catalog/scope.rs](../../../../crates/provider/src/service/provider_catalog/scope.rs)                 |         31 / 31 |                     31 / 31 |
| [crates/provider/src/service/workspace_attention.rs](../../../../crates/provider/src/service/workspace_attention.rs)                       |       210 / 225 |                     66 / 68 |
| [crates/provider/src/storage/timeline.rs](../../../../crates/provider/src/storage/timeline.rs)                                             |       281 / 283 |                       1 / 1 |
| [crates/provider/src/storage/timeline/subagents.rs](../../../../crates/provider/src/storage/timeline/subagents.rs)                         |         66 / 69 |                     14 / 14 |
| [crates/terminal/src/activity.rs](../../../../crates/terminal/src/activity.rs)                                                             |         98 / 98 |                     98 / 98 |
| [crates/terminal/src/dispatch.rs](../../../../crates/terminal/src/dispatch.rs)                                                             |         66 / 68 |                     31 / 33 |
| [crates/terminal/src/lib.rs](../../../../crates/terminal/src/lib.rs)                                                                       |          5 / 10 |                       0 / 0 |
| [crates/terminal/src/ports.rs](../../../../crates/terminal/src/ports.rs)                                                                   |           8 / 8 |                       8 / 8 |
| [crates/terminal/src/protocol.rs](../../../../crates/terminal/src/protocol.rs)                                                             |         30 / 31 |                       0 / 0 |
| [crates/terminal/src/service.rs](../../../../crates/terminal/src/service.rs)                                                               |       330 / 331 |                     96 / 96 |
