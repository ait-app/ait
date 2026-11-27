# 审查遗留事项

三次代码审查和一次文档整理中确认、但尚未处理的条目。已修复的条目及其回归测试留在原清单：
[2026-11-02 与 2026-11-13 仓库审查](../archive/plans/repository-review-2026-11-02.md)、
[2026-11-03 daemon 审查](../archive/plans/daemon-review-2026-11-03.md)、
[2026-11-10 文档后续事项](../archive/plans/docs-followups-2026-11-10.md)。

每项保留原始触发条件和建议做法。修复后从本页删除该项，修复与回归测试写进 PR 描述。
2026-11-27 合并三份清单时对照源码抽查了权限答复顺序、lane 出错分支、`agent.items.close`、
排队 Agent 上限、DSH 取消和创建回执，均仍成立；其余条目没有重新核对。

## 需要先确认

- [ ] **OpenCode（待确认）：恢复后的子会话通知可能使回合失败。**
      `reload` 在同一连接上加载子会话；回合中来自这些子会话的 `session/update` 只接受命令或
      配置更新，`usage_update` 等会使回合失败，子会话的权限请求在 `capture` 与 `poll_idle`
      中也会被拒绝。需先用真实 OpenCode 确认加载后是否会发送这些通知。
      2026-11-04：本机 OpenCode 为 2.0.32，早于 ADR-206 所需的子会话扩展版本，不能据此关闭该项。
      2026-11-10：本机 OpenCode 已升级到 2.2.5，晚于 ADR-206 引用的 2.1.4 子会话事件，版本不再是阻碍；
      尚未用真实会话复现 `reload` 后的子会话通知，仍不能关闭。

- [ ] **（待确认）被拒访客可能在下一次结果中被重新放行。**
      `shares.rs` 的 `apply_outcome` 用每次新结果替换 `book.denied`，被拒授权又已从发布的
      `visitors` 中移除，下一次结果可能清空拒绝记录。仅当中心仍会为被拒访客路由会话时才有影响，
      需确认中心行为。

## 2026-11-13 仓库审查

### 定时任务

- [ ] **定时任务的运行记录不会裁剪，文档可能超过存储上限后无法再写。**
      `crates/schedule/src/engine.rs` 只限制单个任务的运行数量（4096），不裁剪历史；一次运行最多
      保留 64 KiB 输出。文档超过 `crates/persistence/src/storage/schedule.rs` 的 16 MiB 上限后，
      `save` 返回 `Error::Conflict`，该任务的运行停在 `Running`，删除与修改也被拒绝。
      建议：保存冲突时丢弃最旧的已结算运行（含输出）后重试一次。

- [ ] **一条损坏的定时任务记录会让 daemon 无法启动。**
      `Engine::open` 是全有或全无：提示超过 64 KiB、`maxRuns` 为 0、Agent id 不是 UUID 或
      时区无法解析的记录会让整个打开失败，错误经 `Schedules::spawn` 冒泡到 daemon 启动，
      此时 API 尚未起来，没有任何修复入口。
      建议：隔离这一条记录（标记为已结束）并保留其余记录。

### Provider 运行时与适配器

- [ ] **权限答复在追问失败时不会送达 Provider。**
      `crates/provider/runtime/src/service/agent_manager/controls.rs` 先解析追问、再把追问答复
      投递给 Provider，最后才调用 `respond_permission`。追问投递失败（队列忙、Provider 拒绝）
      时提前返回，Provider 永远收不到这次权限答复，Agent 会一直停在 `permission` 状态。
      建议：先答复 Provider，再投递追问；追问失败不应跳过答复。

- [ ] **lane 失败时不会关闭其原生会话。**
      `crates/provider/runtime/src/service/agent_execution/lane.rs` 只在关停、通道关闭和空闲
      过期时调用 `close(state)`；出错分支直接跳出，`ExecutionState` 连同其中的原生会话被丢弃。
      子进程会随 `Drop` 结束，但持久状态仍是 `Running`，lane 的路由别名也不会释放。
      建议：出错分支同样关闭 lane 的状态。

- [ ] **`agent.items.close.request` 会吞掉归档失败。**
      `crates/provider/runtime/src/rpc/agent_runtime.rs` 用 `filter_map` 丢弃归档错误，答复里
      的 `agents` 列表因此可能短于请求的 id；调用方无法区分「没有可关闭的」和「关闭失败」。
      同一路径还直接使用请求里的原始 id，而目录查询支持前缀解析，短 id 会静默关闭不到任何东西。
      建议：先解析 id，再逐个报告失败。

- [ ] **运行时为每个被改动的 Agent 保留一条永久别名。**
      `Owners::agent` 会在每次改动时插入 `id -> agent:{id}`，只有 lane 拆除时才会移除；
      `loaded_timelines` 也只在硬删除时清理，存活进程内的 Agent 越多，两个集合越大。
      建议：作用域查询不生成路由别名；归档时一并清理历史标记。

- [ ] **排队输入的 Agent 列表被截断在 1024 个。**
      `crates/provider/runtime/src/storage/timeline/inbox.rs` 的 `queued_agents` 用
      `ORDER BY agent LIMIT 1024`，超出部分在没有活跃 lane 时不会获得 lane，其排队输入一直
      得不到投递。
      建议：对身份查询分页，或去掉该上限（它不读取提示正文）。

- [ ] **DSH 的 `cancel_turn` 在回合真正结束前就返回。**
      `crates/provider/deepseek-harness/src/native/session.rs` 只发送 `session/cancel`，
      `self.active` 要等原生 `turn/end` 才清空；原生不再发这个事件时，运行时又会因为
      `interruption_requested` 放弃重试，该会话此后的每次 `start` 都被拒绝。
      建议：在有界超时内等待回合结束，超时后清空 `self.active` 并给出取消事件。

- [ ] **ACP 未答复的权限请求被丢弃，却告诉宿主已经解决。**
      `crates/provider/native/src/acp_streaming.rs` 的 `settle` 把待答复的权限请求直接转成
      `PermissionResolved` 事件，没有给原生会话发送 ACP 答复；原生若在权限未决时中止了回合，
      它的请求 id 永远收不到答复。OpenCode 适配器发送 `cancelled` 才是正确做法。
      建议：结算前先撤回待答复的权限。

### 自动化与持久化

- [ ] **自动化运行时从不释放每个工作区的状态。**
      `crates/metadata/src/automation/local.rs` 的 `scripts`、`runs`、`setups` 与
      `workspace_ports` 只有插入没有删除；工作区归档或删除后，其运行历史（每个运行最多 64 KiB
      输出缓冲）和脚本进程仍然常驻，daemon 的内存随见过的工作区数量单调增长。
      建议：工作区归档或删除时调用一个释放该工作区状态的入口。

- [ ] **创建回执永不淘汰。**
      `crates/persistence/src/storage/creation.rs` 的 `put` 只写入不删除，回执随 daemon 存活时间单调增长。
      2026-11-27：回执已迁入 SQLite 并按 id 写入，原清单中“每次创建重写整个文件”不再成立；仍没有淘汰。
      建议：给回执加上淘汰策略，并在写入时顺带清理过期项。

## 2026-11-03 daemon 审查

- [ ] **等待循环没有整体超时。**
      `host/voice.rs` 与 `host/schedule.rs` 的等待仍可无限期进行，依赖取消令牌结束。定时运行可能
      合理地持续很久，加入上限前需要先确定产品语义。
      2026-11-04：保留已有取消令牌机制；还需明确语音与定时任务各自的上限、超时是否取消
      Agent 回合，以及用户可配置方式，不能以任意固定时限中断合法的长任务。

## 2026-11-10 文档整理

由执行整理的 Agent 对照源码提出，尚未经第二人核实。

- [ ] **四份现行规格未经熟悉该领域的人审阅。**
      [provider](../architecture/provider.md)、[workspace](../architecture/workspace.md)、
      [bot](../architecture/bot.md) 与[连接](../architecture/connection.md)各约 30 条不变量，由 Agent 逐条对照源码写成，
      只抽查过少数常量。在被当作权威之前需要逐份审阅。
- [ ] **ADR 元数据中的推断值。**
      十篇 ADR 的日期取自 Git 历史；约 25 篇的所属路径把握不大（如 `daemon/adr-107`、`adr-116` 标为整个 `crates`）；
      `clients/adr-172`、`daemon/adr-038` 由 Accepted 改标为 superseded。需要确认或修正。
