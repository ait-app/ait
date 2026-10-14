# Server 与本地 Paseo API 对照

- Ait 基线：`b797e0d2f83ae57f62892c288a5d81776f8afa6a`，加本次未提交修改。
- Paseo：`/Users/necokeine/Documents/paseo`，`30178c4f58b67f8472901356e1484022bd835de0`，0.10.0-beta.1。
- 范围：server 的当前业务 API；保留已经明确移除的能力组和 Ait transport 认证边界。
- 实现阶段采用定向测试；提交前完整验证见 [PR 验证报告](paseo-server-pr-validation-2026-09-29.md)。未调用真实付费 Provider。
- [205 项逐接口索引](paseo-api-matrix-2026-09-29.md)包含原始名称、canonical 方法、输入字段和实现入口；
  完整输入 schema、源码指纹、关联测试在 `paseo-api-contracts.json`。路由索引不等同于全部语义兼容。

## 已修复

1. Workspace / Agent 列表使用保存排序值与身份的游标，替代数字偏移。补齐默认 200 条、
   大小写排序、稳定并列排序、排序变更拒绝及删除前页记录后的续页。
2. Workspace 列表过滤归档项目，按名称和身份查询；空项目不被 Workspace 文本查询隐藏。
   只为受管理 worktree 返回 worktreeSlug。
3. Workspace 状态从 Agent、同工作区子 Agent 和原生子任务聚合，保留状态优先级及进入时间。
   同 cwd 的不同 Workspace 保持隔离。新增 metadata 只读 port，见 ADR-065。
4. 时间线合并助手/推理片段和工具生命周期，按投影分页；增量响应包含完整投影与准确源区间。
   canonical 请求按当前上游返回 projected。搜索使用相同投影及 seqEnd 定位。
5. 分叉导出按完整投影选择检查点；拒绝已被后续状态改变的旧边界，恢复工具摘要及子任务日志。
6. 历史搜索支持分散在不同名称字段的关键词、词内缩写、短词换位和长词有界纠错。
7. 等待接口在审批时返回 permission，支持显式长超时及无超时等待；超时不带上一次回复，
   取消过程中不让旧审批提前结束等待。未知身份以内联 error 返回；自动归档期间仍返回该次
   等待观察到的最后回复，不为已关闭的原生会话保留进程。
8. Project / Workspace / Agent 支持目录 sync，Workspace / Agent 支持连接所有权订阅、
   序列化变化和按游标重新连接。ServerInfo 发布显式特性标记，SDK 仅对支持的 server 启用。
9. Provider 功能预览接收 `draftConfig` 并反映模型、工作流和选中的功能值；隐藏模型不进入
   picker。全局目录与显式 cwd 分开缓存，支持空白、相对路径与 `~` 展开。
10. 显式恢复支持 overrides、归档恢复和继续输入；失败不会提交新配置或取消旧归档。
    内部历史读取保持只读；新标题清除自动命名来源，防止后续生成覆盖手工标题。
11. 目录使用 ICU 语言排序，修复重音、中文及标点排序和 keyset 续页顺序差异。
12. 创建 Agent 按显式 Workspace、caller、无上下文创建的顺序解析归属；不再让旧 cwd
    覆盖显式 Workspace，caller 关联不能被请求 labels 中的伪造父标识覆盖。
13. 未知原生 handle 可直接 resume，保留原生身份、历史和配置而登记新的本地 Agent；
    cwd overrides 恢复后的会话仍可继续输入，非法 handle 不创建本地记录。
14. 创建 env 只传入对应 Codex/Claude 子进程，兄弟 Agent 之间隔离；幂等回执比较环境摘要，
    不保存明文。覆盖非法环境、配置目录及 SDK 强制环境参数的测试。
15. autoArchive 等待 completed/failed/cancelled，审批不归档；清理原生进程失败可重试。
    新建 worktree 时归档整个所属 Workspace 的 Agent，再清理无其他 Workspace 引用的 checkout。
16. 创建 Agent 支持 modern branch-off/checkout-branch、legacy worktreeName/createWorktree、
    原目录 createNewBranch/baseBranch；重试不重复建树、setup 只启动一次、原生创建失败回滚。
    新分支不继承 upstream，脏目录拒绝切换；源仓库和其他 Workspace 保留。
17. 终端支持回环 HTTP hook 上报、进程专属 token、Workspace 状态聚合、可见焦点清除及
    Ctrl-C/Esc 中断清除；完成和输入提示事件支持连接订阅、去重和焦点抑制。
18. Workspace、Project 和 worktree 归档先标记目录记录，再归档所属 Agent、关闭原生进程、
    终端及 setup/script，最后删除无活动引用的受管理 checkout。失败保留资源以便重试；
    同 cwd 的其他 Workspace 保留。自动归档也复用相同的终端和 automation 清理。
19. 会话订阅补齐审批请求、审批结果及原生子 Agent 更新，不依赖时间线订阅。
    审批记录成功持久化后才发布事件；SDK 通过 agent-session-events-v1 检查生产者。
20. checkout 分支默认以分支名称派生目录名，并保留原分支为比较基线，包括分支已被其他
    worktree 使用而创建副本的情况。
21. Workspace、独立 worktree、modern/legacy Agent 创建支持 GitHub/GHES PR 来源：解析
    head/base、检出 PR ref、避免覆盖同名分支、保留比较基线；fork 使用专属 push remote/refspec。
    外部 fork 默认不可信，setup 在批准前不运行。失败清理临时 ref，未执行真实 push。
22. Workspace 创建可同时创建初始 Agent，复用一个回执与预留身份，保留相对 cwd、env 和首条
    输入。调用方断线后继续；并发相同键只执行一次。原生失败保留 Workspace，仅在确认未登记
    Agent 且进程已关闭后允许安全重试；已经尝试的首条输入不因重试再次提交。
23. 创建请求的 subscribe 在操作期间实时报告进度并自动释放；显式 creation.subscribe
    保持独立连接所有权。连续创建和幂等重放不再耗尽订阅名额。SDK 通过 creation-lifecycle-v1
    与所需方法共同判断是否启用初始 Agent 和创建回执。
24. 创建回执跨幂等键、跨 Agent/Workspace 创建种类预留资源身份，重启后仍保留冲突检查。
    已存在的显式 Workspace ID 不再覆盖原记录；已有 Agent 即使没有旧创建回执，也会在新建
    Workspace 前被拒绝。首条输入失败标记 outcomeUnknown，并记录 Agent 的实际 Workspace ID。
25. 完成回执在重放和查询时验证资源仍存在；删除 Agent 或移走工作目录后返回缺失错误，
    不返回虚假的成功或重新创建。恢复目录后可继续重放原回执，归档本身不使目录回执失效。
26. 真实 SDK 验证发现创建操作的进度不能带长期订阅 ID，否则客户端忽略中间阶段。
    现在操作进度和最终响应都省略 subscriptionId，独立回执订阅仍携带自己的 ID。
    已接纳创建的 setup/回执收尾在现有工作配额内排队，避免并发读取使已执行的创建报配额错误；
    排队会响应 shutdown，已开始的工作继续受任务追踪约束。

## 上游测试来源

| 来源                                                                                     | 本次验证                                                                                                                                                                                            |
| ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pagination/cursor.test.ts`、`pagination/sortable-pager.test.ts`                         | 通用游标及目录 API 回归                                                                                                                                                                             |
| `workspace-directory.test.ts`                                                            | Workspace 过滤、身份归属、状态优先级及状态时间回归                                                                                                                                                  |
| `agent/timeline-projection.test.ts`                                                      | 执行原始 20 个测试，保存 18 组投影/分页对照数据                                                                                                                                                     |
| `agent/activity-curator.test.ts`                                                         | 执行原始 16 个测试，保存 6 组分叉成功/拒绝对照数据                                                                                                                                                  |
| `agent-history-search.test.ts`                                                           | 执行原始 9 个测试，保存 12 组搜索对照数据                                                                                                                                                           |
| `protocol/search/text-match.ts`                                                          | 固定生成 160 组编辑距离对照数据，验证优化后的 Rust 算法                                                                                                                                             |
| `session.ts` wait-for-finish、原生取消/审批测试                                          | 离线 Provider 的审批等待、超时与取消回归                                                                                                                                                            |
| `directory-sync/index.test.ts`、`internal/versioned-collection.test.ts`                  | 序列、压缩、墓碑过期、generation、跨连接订阅和断线补取                                                                                                                                              |
| `session/provider/provider-catalog-session.test.ts`                                      | 草稿功能输入、内联失败、隐藏模型和全局作用域                                                                                                                                                        |
| `agent/agent-manager.test.ts`、`session.ts` resume                                       | 原生覆盖配置、注册失败、归档恢复、重启与运行中拒绝                                                                                                                                                  |
| `agent/create-agent/intent.test.ts`                                                      | Workspace/caller 归属优先级、父标签与新建 Workspace                                                                                                                                                 |
| `pagination/sortable-pager.ts`                                                           | 3 个 locale、33 个值的实际 Node/ICU 排序 oracle                                                                                                                                                     |
| `agent/create-agent-lifecycle-dispatch.test.ts`、`worktree-session.test.ts`              | Agent worktree、自动归档、原目录分支、失败清理和 setup                                                                                                                                              |
| `terminal/activity/terminal-activity-tracker.test.ts`、`terminal-activity-route.test.ts` | 活动状态、鉴权、退出撤销、真实 HTTP/PTY/Workspace                                                                                                                                                   |
| `websocket-server.terminal-notifications.test.ts`                                        | 通知去重、连接订阅及终端焦点抑制                                                                                                                                                                    |
| `session.ts` permission / provider_subagent 转发                                         | 独立于时间线的会话事件、审批撤回、显式结果和子任务身份                                                                                                                                              |
| `worktree-core.posix.test.ts`、`utils/worktree.test.ts`                                  | checkout 比较基线、归档资源关闭、共享引用与失败重试                                                                                                                                                 |
| `worktree-session.test.ts`、`worktree-core.posix.test.ts` 的 PR checkout 分支            | 本地 bare Git + gh fixture、同仓库/fork、分支冲突、临时 ref 清理及信任批准                                                                                                                          |
| `creation/creation.e2e.test.ts`、`creation/index.ts`、`session.ts` Workspace 创建        | 初始 Agent、失败后相同身份重试、首条输入至多一次、断线与并发、实时进度和订阅释放                                                                                                                    |
| `creation/index.test.ts`、`websocket-server.ts::validateCompletedCreation`               | 跨回执资源冲突、结果不确定标记、缺失目录/Agent 的重放和查询校验                                                                                                                                     |
| 现有 SDK `CreationClient` 与 Rust transport 联合回归                                     | 真实本地 server + 离线 Codex，验证普通 Agent 和 Workspace 初始 Agent 的全部实时阶段、完成回复、分页/搜索/同步、审批事件与答复、归档恢复后继续输入，以及真实 PTY 的活动 HTTP、通知、列表、捕获和清理 |

生成器：`scripts/paseo-timeline-fixtures.mjs`、`scripts/paseo-history-fixtures.mjs`、
`scripts/paseo-collation-fixtures.mjs`。传入 Paseo checkout 和 `--check` 可只读校验已有数据。
JSON fixture 含 revision 与逐源文件 SHA-256。生成时执行的上游测试数与 Rust 测试数量分开统计；
已有测试没有计作本次新增测试。

## App 适配改动与上游的关系

`apps/mobile` 的生产改动仅在 `runtime/rust-daemon/messages.ts`：把 server 公告的能力转换为
现有 SDK 使用的能力字段；另外两处文件为能力判断测试和真实 SDK/server 集成测试。
上游 `packages/app` 没有这个 Rust transport 目录，这部分属于 Ait 的接入代码。

`directorySync`、`creationLifecycle`、`agentRequestReceipts`、`workspaceRequestReceipts`
以及新增公告的审批、子 Agent、终端通知事件均来自固定版本 Paseo 的协议。
`*-v1` 是本轮新增的 Rust 能力标记；`directorySubscriptions` 和 `sessionEventTypes`
则是 Ait 之前已有的扩展，本轮根据 server 实际支持的能力启用。
没有修改 SDK 的创建状态机；其实现及单元测试除包名替换外与该上游版本一致。
这些标记描述已安装的行为，不能作为下文剩余语义差异已经消除的证明。

## 仍需区分的范围

Hub / Chat / Loop / Plugin、relay 的既有移除决定不变。Ait 的 Bearer/ticket 认证及 dotted envelope
继续由 SDK transport adapter 适配。当前安装的原生 Provider 为 Codex / Claude，forge 为
GitHub / GHES；OpenCode/Pi 与 GitLab/Gitea 不在现有安装范围。本次没有调用付费模型、远端账号
或真实 push 服务，GitHub/GHES PR 测试使用本地 Git 和 gh 协议 fixture。

以下兼容差异仍未完成，不能将当前 167 个已路由方法等同于 167 个已完全兼容方法。
配对 offer RPC 已按 [ADR-127](../../decisions/daemon/adr-127-remove-daemon-pairing-offer.md) 删除。

- `checkout_status_update`、`script_status_update`、`workspace_setup_progress`、`project.update`
  和 `activity_log` 等全局 session event 生产者未全部接入；已实现的按资源查询/订阅接口继续有效。
- 全局 Agent hook 自动安装未实现；本轮新增的是每个 PTY 的环境及活动 HTTP 接口。
- 普通 Agent 创建失败和进程在中间阶段退出后的回执恢复规则仍比上游保守。
  本轮实现的是 Workspace 初始 Agent 已确认无原生副作用后的重试，未实现全部阶段的自动恢复。
- 原生历史 cwd 校验、等待跨排队 turn 的事件边界还需要更多实际 Provider 对照。

协议矩阵、固定上游测试 oracle、离线原生进程与真实本地传输回归分别提供不同层面的证据；
它们不替代真实账号、远端 forge、Linux/Windows 或整套 Paseo 产品的端到端验证。

## Test coverage

提交前另行完成完整 workspace 测量：**91.71%（40,580 / 44,250）**；
改动生产文件整体为 **91.63%（17,820 / 19,448）**，新增/修改行按 LCOV DA 统计为
**95.16%（4,658 / 4,895）**。普通测试和覆盖率测试均为 1,460 通过、3 ignored、0 失败。
精确命令、各 crate 行数、历史比较及统计口径见[提交前验证](paseo-server-pr-validation-2026-09-29.md)；
以下保留此前实现阶段的定向测量，不与完整测量混用。

2026-09-28 22:04 UTC 以 `cargo llvm-cov` 完成一次清空旧 profile 后的定向测量。
版本为本文 Ait 基线加未提交修改；[覆盖率证据](paseo-server-coverage-2026-09-29/coverage.json)
保存源码 SHA-256、逐文件未覆盖行、修改行命中次数、精确测试命令及日志指纹。
测量后已核对 358 个源码指纹与当前文件一致。没有可比较的旧覆盖率基线。

| 测量口径                        | 覆盖率 | 已覆盖 / 总行数 |
| ------------------------------- | -----: | --------------: |
| 新增或修改的可执行行            | 95.14% |   4,657 / 4,895 |
| 本次改动的生产文件整体          | 86.67% | 15,864 / 18,303 |
| 选定 crate 的全部已插桩生产文件 | 68.95% | 26,317 / 38,168 |

范围为 model、provider、metadata、filesystem、terminal、api、protocol 及 daemon 的
改动和直接相关行为，macOS arm64、默认 features；排除 tests/test_support 文件。
完整文件分母仍包含未由本轮定向测试执行的旧路径，特别是 filesystem 的其他接口。
以上不是逐接口语义覆盖率，也不是 workspace 覆盖率；后续提交准备已另行完成完整验证。

测试执行结果单独记录：750 项 Rust 测试通过，3 项既有在线 Provider 测试 ignored；
43 项 SDK adapter 测试、1 项真实本地 server 集成测试、9 项 SDK creation 测试通过。
上游生成器另外执行 45 项原始测试，并核对 3 个 locale 的排序对照；这些数量不代表新增测试数。
`cargo fmt --all --check`、workspace check、Clippy `-D warnings`、改动 JS/TS 的 oxfmt/oxlint、
集成测试单文件 TypeScript 检查、5 个 Python 文件语法检查和 `git diff --check` 均通过。

复现主要验证：

```sh
python3 scripts/paseo-focused-coverage.py --run
cargo fmt --all --check
cargo check --locked --offline --workspace --all-targets
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
node scripts/paseo-api-audit.mjs ../paseo --check
python3 scripts/check-paseo-protocol.py ../paseo
node scripts/paseo-timeline-fixtures.mjs ../paseo --check
node scripts/paseo-history-fixtures.mjs ../paseo --check
node scripts/paseo-collation-fixtures.mjs ../paseo --check
npm run test --workspace=@ait/client -- src/creation/index.test.ts
npm run build:clean --workspace=@ait/client
cargo build --locked --offline -p daemon --bin daemon
AIT_TEST_RUST_SERVER="$PWD/target/debug/daemon" npm run test --workspace=@ait/mobile -- src/runtime/rust-daemon/
```

真实 SDK 集成测试默认跳过，需要上面的 `AIT_TEST_RUST_SERVER`；它为 server 创建临时数据目录、
随机本地端口和 Bearer token，原生 Provider 使用仓库离线 Python fixture，结束后清理。
覆盖率各 crate 命令和行数见[详细报告](paseo-server-coverage-2026-09-29/README.md)，
本地 `target/llvm-cov/html/index.html` 现在对应最新的提交前完整测量；定向结果保存在上述 JSON。

尚未覆盖部分持久化/队列故障和操作系统拒绝终止进程后的保留责任分支；需要进一步故障注入。
未运行真实账号的 Codex/Claude 推理、GitHub/GHES 网络/push、Linux/Windows；
这些环境验证和上文列出的兼容差异仍然待办。
