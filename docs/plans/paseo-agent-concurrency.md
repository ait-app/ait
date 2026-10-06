# Paseo 并发模型对齐修改清单

状态：C01–C11 已实施，C12 定向及提交前全量验证完成；真实远程采样待执行。日期：2026-10-07。
初期源码基线：`5df233ac2f87cecda5f1a1ee99c003a15522dc49`；PR 已整合最新 main `aa6d8d820c139f3f761d89c09a7608ecdc5629c9`，保留 ADR-088 的传输响应接纳与客户端连接就绪规则。

将 AgentExecution 从全局串行请求处理改为按 Agent 保序，把 Provider 发现和快照读取独立执行。目标是让一个慢 Provider 或 Agent 的启动、恢复、关闭不阻塞其他 Agent 的读取、控制与事件处理，同时保持单 writer、输入接纳和持久化顺序。

本清单覆盖服务端并发、相关客户端刷新行为和验证；它细化了[远程 Workspace 打开性能方案](remote-workspace-open-performance.md)中的执行器隔离工作。

## Paseo 参考与目标边界

参考[仓库记录的来源版本](../../paseo/README.md)：Paseo `0.9.0-beta.2`，提交 `2c8e8a826810337492cc5a38bb0bbd705b6fb632`。

| 边界             | Paseo 实现                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | AIT 对齐目标                                                          |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| 生命周期         | [runLifecycleMutation](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/agent-manager.ts#L2260)按 agentId 维护队列                                                                                                                                                                                                                                                                                                                                         | 同一 Agent 的恢复、关闭、归档、删除保序，不同 Agent 并发              |
| 前台操作         | [runForegroundMutation](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/agent-manager.ts#L2772)按 agentId 保序                                                                                                                                                                                                                                                                                                                                            | 输入接纳、steer、cancel 与配置提交在目标 Agent 内协调                 |
| session 事件     | [sessionEventTails](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/agent-manager.ts#L3721)及 drainSessionEvents                                                                                                                                                                                                                                                                                                                                          | 每个 Agent 独立处理事件，在状态提交前设置必要的事件屏障               |
| 冷加载           | [ensureAgentLoaded](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/agent-loading.ts#L63)共享同一 Agent 正在初始化的 Promise                                                                                                                                                                                                                                                                                                                              | 同一身份的恢复和历史加载分别去重，不同身份独立加载                    |
| Provider catalog | [后台快照读取](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/provider-snapshot-manager.ts#L738)、[每 Provider 并发上限 4](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/provider-snapshot-manager.ts#L834)、[并行加载及请求合并](https://github.com/getpaseo/paseo/blob/2c8e8a826810337492cc5a38bb0bbd705b6fb632/packages/server/src/server/agent/provider-snapshot-manager.ts#L879) | 独立服务、立即返回当前快照、后台发现、按 Provider 和 scope 去重及限流 |

Rust 中用按 Agent 的 actor 表达状态所有权：actor 是异步任务，不是每个 Agent 新建线程。生命周期、前台操作和事件可以有独立的排队规则，但共享 session 的可变操作仍由单一所有者提交；跨队列通过 session generation、turnId 与事件屏障协调。

Tokio current-thread 本身允许多个异步任务交错执行。拆分后，registry 和 SQLite 的阻塞操作仍需移到有界存储执行路径；不能让这些操作继续阻塞整个 Provider runtime，也不能让新的读取路径再次排入通用的单任务预算。

## 修改列表

以下文件均为当前入口；新增模块的名称在实施时按职责确定。

### C01 明确调度契约和状态所有权

- [x] 新增 Provider 并发调度 ADR，明确修订[ADR-032](../decisions/providers/adr-032-daemon-native-provider-execution.md)的全局串行范围，并更新文档索引。
- [x] 为 RPC 建立路由分类：Provider 查询、Agent 只读、Agent 生命周期、前台控制、跨 Agent 的目录操作。身份解析完成后按稳定 Agent ID 调度。
- [x] 明确 session、active turn、pending terminal、input receipt、history hydration 和可读 snapshot 的所有者。定义生命周期与前台操作的冲突、事件屏障和失败后的重试规则。

涉及：[service/agent_execution.rs](../../crates/provider/src/service/agent_execution.rs)、[rpc/agent_execution.rs](../../crates/provider/src/rpc/agent_execution.rs)、[service/agent_manager.rs](../../crates/provider/src/service/agent_manager.rs)。

验收：每个现有入口都有明确执行归属，所有修改同一 native session 的入口都经过其所有者；兼容 façade 不能同时通过旧 manager 和新 actor 操作同一 session。

### C02 共享 Provider client 并拆开依赖注入

- [x] 将注册后的 client 持有方式调整为可共享的 `Arc<dyn AgentClient>`，供 catalog、历史读取和 session factory 使用；现有 trait 已要求 `Send + Sync`。
- [x] client registry 共享同一组工厂引用，catalog 成为独立共享服务；AgentManager 保留兼容访问入口。
- [x] AgentExecution 保留可克隆入口，内部组合独立 catalog、Agent 调度器与读取服务。

涉及：[ports/agent_session.rs](../../crates/provider/src/ports/agent_session.rs)、[service/agent_manager.rs](../../crates/provider/src/service/agent_manager.rs)、[service/agent_execution.rs](../../crates/provider/src/service/agent_execution.rs)、[daemon host](../../bins/daemon/src/host.rs)。

验收：客户端工厂可并发使用，live session 仍保持独占所有权；各 Provider 的 adapter 契约测试通过。

### C03 Provider catalog 独立后台执行

- [x] 将 catalog RPC 从全局命令循环移到独立服务；不同 Provider 并发发现，每个 Provider 的发现并发上限初始设为 4，另设总任务预算。
- [x] 以 Provider 和规范化 scope 合并正在进行的发现；普通读取复用同一次任务。刷新请求合并为有界的后续刷新，避免连续点击产生无限任务。
- [x] `snapshot.get` 返回当前缓存或 loading 条目并触发后台刷新；`snapshot.refresh` 在任务被接纳后返回 ack，完成时逐项推送新快照。
- [x] 为模型、模式、features、available 等已有查询定义兼容行为：需要完整结果的查询可以等待目标发现，但等待发生在 catalog 服务内。
- [x] 发现设置超时；失败只更新对应 Provider。按条目维护有效期与发现 generation，选中一个 Provider 刷新不能把其他条目误标为新鲜。
- [x] 保留 scope 数量上限和内容 hash；scope 淘汰、刷新替换、停机后的旧结果不得重新写回缓存。

涉及：[service/provider_catalog.rs](../../crates/provider/src/service/provider_catalog.rs)、其 [scope](../../crates/provider/src/service/provider_catalog/scope.rs) 和 [draft](../../crates/provider/src/service/provider_catalog/draft.rs)、[protocol/provider.rs](../../crates/provider/src/protocol/provider.rs)、[rpc/agent_execution.rs](../../crates/provider/src/rpc/agent_execution.rs)。

验收：阻塞一个 discovery 时，另一个 Provider 能完成，Agent 查询、cancel 与事件处理能继续；相同 scope 的并发读取只执行一次发现。

### C04 按 Agent 调度生命周期

- [x] 将 live map 中每个 Agent 的 session 和运行状态交给独立 actor；全局调度器只负责身份注册、路由和预算接纳，不等待 native I/O。
- [x] create、resume、restore、close、archive、delete、refresh 和 rewind 的 writer 变更按 Agent 保序。创建中的身份先预留，失败时清理 session、身份和预算。
- [x] 目录元数据修改与 writer 生命周期协调，保留 `registry.update` 合并最新 title、labels、attention 的行为；删除后关闭不得重新插入记录。
- [x] Workspace 退役、自动归档和级联操作先确定目标集合，再逐 Agent 执行，避免持有多个 Agent 的执行权相互等待。创建提交时重查 Workspace 和 Agent 的有效状态。
- [x] 关闭失败保留 writer 所有权；归档或删除部分成功时仍清理相应 writer。没有 writer 和在途任务的空闲调度项可以回收；watch 观察记录独立保留。

涉及：[service/agent_manager.rs](../../crates/provider/src/service/agent_manager.rs)、[resume.rs](../../crates/provider/src/service/agent_manager/resume.rs)、[native_sessions.rs](../../crates/provider/src/service/agent_manager/native_sessions.rs)、[auto_archive.rs](../../crates/provider/src/service/agent_manager/auto_archive.rs)、[service/agent_runtime.rs](../../crates/provider/src/service/agent_runtime.rs)、[rpc/agent_execution.rs](../../crates/provider/src/rpc/agent_execution.rs)。

验收：Agent A 的慢恢复、关闭不影响 B；同一 Agent 的重复恢复只产生一个 writer；归档、删除与恢复竞争不会重新激活 Agent。

### C05 前台操作保序和跨队列屏障

- [x] send、steer、cancel、permission、configure 与待发送输入调度移入目标 Agent 的前台操作路径，保留输入 FIFO、claim 先落盘、接纳不确定时不自动重发的规则。
- [x] close、rewind、restore 等独占操作先阻止新的前台接纳，再协调在途操作与事件；队列之间通过显式屏障衔接。
- [x] voice 的取消继续绑定已接纳的 turnId，不能取消后续 turn；客户端断开不取消普通已接纳输入。
- [x] native 操作产生的配置事件先处理，再提交显式配置结果；本次 turn 使用的配置保持确定。

涉及：[agent_manager/delivery.rs](../../crates/provider/src/service/agent_manager/delivery.rs)、[controls.rs](../../crates/provider/src/service/agent_manager/controls.rs)、[rpc controls](../../crates/provider/src/rpc/agent_execution/controls.rs)、[rpc voice](../../crates/provider/src/rpc/agent_execution/voice.rs)、[execution voice](../../crates/provider/src/service/agent_execution/voice.rs)。

验收：send/cancel、permission/config、rewind/send 竞争时没有重复接纳、错误 turn 取消或旧配置覆盖。

### C06 每个 Agent 独立处理事件

- [x] 将全局 `poll → reconcile → dispatch_pending_inputs` 循环拆成按 Agent 的事件处理和输入唤醒，去掉每个 RPC 前的全局 poll。
- [x] 先保留 adapter 的 `poll_turn` 契约，每个 Agent 独立调度 drain；按事件到达顺序持久化，并限制单次处理量以保持公平。
- [x] pending runtime、handle、input 和 terminal 的持久化重试留在所属 Agent。一个 Agent 的写入错误或失败关闭不阻止其他 Agent 的 drain。
- [x] 以 session generation 和 turnId 隔离旧 session 的事件；terminal、config、permission、replacement 使用必要的提交屏障。只有持久化完成后更新快照和发布完成状态。
- [x] 标题生成、自动归档与待输入派发按所属 Agent 或 Workspace 唤醒，其慢外部工作不占全局事件处理路径。

涉及：[agent_manager.rs](../../crates/provider/src/service/agent_manager.rs)、[streaming.rs](../../crates/provider/src/service/agent_manager/streaming.rs)、[generated_titles.rs](../../crates/provider/src/service/agent_manager/generated_titles.rs)、[auto_archive.rs](../../crates/provider/src/service/agent_manager/auto_archive.rs)、[agent_execution.rs](../../crates/provider/src/service/agent_execution.rs)。

验收：慢 discovery 和其他 Agent 的关闭期间，运行中的流事件持续处理；落盘失败不提前公布 idle，恢复后能按顺序提交。

### C07 恢复与历史加载分别去重

- [x] 按稳定身份的会话任务合并重复恢复和 hydration，成功后共享已加载状态；失败允许后续请求重试，提交用 persistence 和 history epoch 校验。
- [x] 保留历史检查与交互式 writer 的区别：读取历史不默认恢复 writer；归档身份按已有 history purpose 处理。
- [x] 原生历史读取在有界后台任务执行，结果提交进入目标 Agent 的协调路径；提交前检查删除、归档、rewind、replacement 与 generation 变化。
- [x] close 与后续 resume 设置加载屏障；慢旧历史不能覆盖新 epoch 或在完成后重新创建已删除记录。

涉及：[AgentManager::load_timeline](../../crates/provider/src/service/agent_manager.rs)、[rpc timeline 入口](../../crates/provider/src/rpc/agent_execution.rs)、[native_sessions.rs](../../crates/provider/src/service/agent_manager/native_sessions.rs)、[storage/timeline.rs](../../crates/provider/src/storage/timeline.rs)。

验收：同一 Agent 并发历史请求只读一次原生历史，不同 Agent 可并发；rewind/delete 与 hydration 竞争时旧结果被拒绝。

### C08 目录 时间线和订阅读取脱离生命周期队列

- [x] 提供已提交的 Agent snapshot、目录投影和 timeline 读取接口；直接读取不隐式触发所有 Agent 的 poll 或 native resume。
- [x] 已加载 timeline 的查询直接进入读取服务；需要 hydration 的请求只等待目标 Agent 的共享加载任务。
- [x] 事件订阅的身份验证改为一次有界批量读取，保留身份解析、订阅激活与事件缓冲顺序。
- [x] Agent 目录订阅与 sync 使用读取投影，保留 generation、seq、删除与增量同步语义。存储读锁和序列化使用独立有界预算。

涉及：[connection.rs](../../crates/provider/src/connection.rs)、[connection/directory.rs](../../crates/provider/src/connection/directory.rs)、[rpc/agent_execution.rs](../../crates/provider/src/rpc/agent_execution.rs)、[rpc/agent_runtime/listing.rs](../../crates/provider/src/rpc/agent_runtime/listing.rs)、[storage/timeline.rs](../../crates/provider/src/storage/timeline.rs)。

验收：一个 Agent 的创建或关闭未完成时，另一个 Agent 的目录、订阅和已加载 timeline 请求可以完成，且订阅没有缺口或重复基线。

### C09 finish wait 改为观察已提交状态

- [x] 用每个 Agent 的 watch/通知观察完成状态，替换当前每 25 ms 调用 `agent.finish.wait.request` 并重新进入命令队列的轮询。
- [x] wait 同时监听状态、deadline、连接取消与停机；等待者不占生命周期或前台命令槽。
- [x] 保留 permission、error、timeout、lastMessage 和归档后完成文本的语义；只有 durable running、没有 live writer 时不能报告成功。

涉及：[agent_execution/waits.rs](../../crates/provider/src/service/agent_execution/waits.rs)、[dispatch/agent_execution.rs](../../crates/provider/src/dispatch/agent_execution.rs)、[rpc/agent_execution.rs](../../crates/provider/src/rpc/agent_execution.rs)。

验收：多等待者不制造全局命令流；terminal 落盘失败期间 wait 保持未完成；取消等待不取消已接纳 turn。

### C10 调整客户端后台刷新兼容行为

- [x] 复用现有 Provider loading 状态、snapshot hash 和推送更新；明确 refresh ack 代表接纳，界面在目标刷新结果到达后结束刷新状态。
- [x] 修正 refresh 后立即 GET 得到旧缓存或 loading 时的处理；刷新失败要有终态，断线重连能够重新取得结果。
- [x] 检查 scope 切换、并发 refetch 与推送的覆盖顺序，旧响应不能回滚已应用的新快照；需要新增协议字段时同步 Rust、TS schema、SDK 和能力协商。
- [x] 模型选择与创建表单继续验证实际 Provider 能力，不能把临时 loading 误当成永久 unavailable。

涉及：[providers-snapshot.ts](../../apps/mobile/src/data/providers-snapshot.ts)、[use-providers-snapshot.ts](../../apps/mobile/src/hooks/use-providers-snapshot.ts)、[rust-daemon/messages.ts](../../apps/mobile/src/runtime/rust-daemon/messages.ts)、[client daemon-client.ts](../../packages/client/src/daemon-client.ts)、[protocol/provider.rs](../../crates/provider/src/protocol/provider.rs)。

验收：首次 loading 后收到 ready/error；手动刷新有明确完成或失败状态；切换目录不串用模型，旧 GET 不覆盖新推送。

### C11 统一预算 存储执行与停机

- [x] 全局保留有界接纳预算：初始沿用 64 个待处理命令和 32 个 live session 的上限，创建或恢复中的 session 先占预算；actor 队列不能把上限按 Agent 数放大。
- [x] discovery、history、阻塞存储读取分别设置有界预算，避免慢 Provider 耗尽交互资源。沿用传输层 32 个 execution wait 的预算。
- [x] registry 和 SQLite 的阻塞操作在有界执行路径中完成，事务锁只覆盖必要存储操作；任务不得持有全局 manager 锁等待 native I/O。
- [x] 统一登记 catalog、hydration、actor、生成任务与 native child 的生命周期。停机先拒绝新工作，再按任务类型停止刷新或 drain 已接纳操作，关闭 session 并回收子进程，最后释放 data-directory lease。
- [x] daemon 组装与 API 的 daemon snapshot 查询使用新的 catalog 和读取入口，保留错误码、能力声明、voice/schedule 调用与断线后的任务所有权。

涉及：[daemon host](../../bins/daemon/src/host.rs)、[agent_execution.rs](../../crates/provider/src/service/agent_execution.rs)、[dispatch/agent_execution.rs](../../crates/provider/src/dispatch/agent_execution.rs)、[API connection dispatch](../../crates/api/src/connection/dispatch.rs)、[model runtime](../../crates/model/src/runtime.rs)。

验收：并发创建不会突破 session 上限，任务过载有明确拒绝；关闭失败不丢失所有权；停机、调用者取消和错误展开都不会留下失管子进程或提前释放实例锁。

### C12 回归验证和调度指标

- [x] 为每批修改增加行为测试，使用可控制的假 Provider 阻塞点验证并发；用同步屏障证明其他请求能完成，避免依赖固定 sleep 判断速度。
- [x] 核心场景覆盖慢 discovery、跨 Agent 恢复、同 Agent 去重、archive/delete/rewind 竞争、配置事件顺序、输入不确定接纳、terminal 落盘失败、wait、过载和停机。
- [x] 记录按操作类别的 queue wait、native I/O、history、事件提交延迟和在途任务数，区分 catalog、Agent 生命周期、前台控制与读取预算的等待。
- [x] 每批运行直接相关 Rust/客户端测试以及格式和 lint；准备提交时执行仓库规定的完整检查。同步更新架构、ADR 和本清单的完成状态。

测试入口：[catalog tests](../../crates/provider/src/service/provider_catalog/tests.rs)、[manager tests](../../crates/provider/src/service/agent_manager/tests.rs)、[execution tests](../../crates/provider/src/service/agent_execution/tests.rs)、[目录订阅 tests](../../crates/provider/src/connection/directory/tests.rs)、[daemon process tests](../../bins/daemon/tests/process/agent_execution.rs)、[Provider hook tests](../../apps/mobile/src/hooks/use-providers-snapshot.test.ts)。

验收：正确性场景通过，慢操作不再引入跨 Agent 的整段排队等待；指标能够分别定位任务执行时间与预算等待。

## 实施顺序

| 批次 | 修改项                                             | 可以独立评审的结果                                                        |
| ---- | -------------------------------------------------- | ------------------------------------------------------------------------- |
| 准备 | C01                                                | 调度 ADR、入口归属、冲突与提交屏障明确                                    |
| 1    | C02、C03、C10 的 catalog 部分、C11 的 catalog 部分 | Provider 后台发现不占 Agent worker，缓存、loading、刷新和停机形成闭环     |
| 2    | C04、C05、C06、C11 的 Agent 部分                   | session 所有权、全部写入口、事件、预算和关闭一起迁移，不同 Agent 独立运行 |
| 3    | C07、C08、C09、C10 的剩余兼容检查、C11 的读取部分  | 历史加载去重、只读与 wait 脱离全局执行队列                                |
| 验收 | C12，随每批执行并最终汇总                          | 并发行为、持久化一致性、资源上限与远程打开基线都有证据                    |

第一批优先解除慢 discovery 对 Workspace 打开的影响。第二批涉及 session 所有权，必须把生命周期、前台写入口、事件与停机协调一起切换；每批以根 Cargo workspace 持续可构建和直接相关行为通过为交付条件。

## 实施结果与待验收项

具体所有权、统一会话队列、跨会话屏障和预算见 [ADR-091](../decisions/providers/adr-091-independent-session-execution.md)，命令、测试结果及构建限制见[验证报告](../reports/providers/paseo-concurrency-validation.md)。Rust 使用同一个会话所有者协调生命周期、前台输入和事件，不照搬多条 Promise tail；根 Cargo workspace 的完整原生链接、daemon process 测试与行覆盖率已验证。

- [ ] 在实际 TCP、SSH 和 Relay 连接上采样 Workspace 首屏及历史加载的 p50/p95，确认网络和其他打开阶段的耗时。
- [x] 准备提交时完成完整 Rust 测试、workspace coverage、原生链接与 daemon process 验证，证据见验证报告。
- [ ] 在 Windows/Linux 上执行平台检查。
