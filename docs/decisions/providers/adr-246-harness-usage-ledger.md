---
id: ADR-246
status: accepted
date: 2026-11-11
amends:
  - providers/adr-052-native-provider-capabilities.md
paths:
  - crates/provider
  - bins/daemon
  - packages/protocol
  - packages/client
  - apps/mobile
  - crates/provider-native
  - crates/provider-core
  - crates/provider-minimax
  - crates/provider-antigravity
  - crates/provider-claude
  - crates/provider-deepseek-harness
  - crates/provider-opencode
  - crates/provider-pi
---

# ADR-246：按 Host 记录的 Harness 用量账本

- 修订：[ADR-052](adr-052-native-provider-capabilities.md) 的用量条款。快照仍是替换值；本决定新增可累加的请求用量。
- 关系：端口位置沿用 [ADR-205](adr-205-provider-execution-contracts.md)；账号额度查询（`provider.usage.list`）不变。
- 方案：[Harness Usage Ledger 计划](../../archive/plans/harness-call-usage.md)。

## 问题

`AgentTurnEvent::Usage(AgentUsage)` 与 `lastUsage` 是上下文占用快照，重复观察会替换旧值，无法回答
“这台 Host 某段时间调用了多少次、花了多少 token”。需要一份只增不改的、可幂等重放的请求级记录。

## 决策

### 请求用量与互斥桶

- 一次模型请求是计量单位。一个调用（call）由若干请求组成，调用合计是请求之和，不是快照替换。
- 新增 `AgentTurnEvent::RequestUsage(RequestUsage)`，与 `Usage(AgentUsage)` 并存。适配器继续发布快照供上下文表使用
  （`lastUsage`、`usage_updated` 的行为与线格式不变）；账本只读取 `RequestUsage`。
- 桶互斥：`input` 不含缓存读写，提供方上报含缓存的输入时由适配器扣减
  （`input = max(0, input − cache_read − cache_write)`）。`output` 含推理；`reasoning` 与 `cache_write_1h`
  是子集，只作诊断，永不加入合计。
- 请求记录含 `segment_id`（同一原生事件稳定，同一 Agent 内唯一，含 provider 与 turn id）、`parent`（子 Agent 流键）、
  `model`、`boundary`、各桶、`cost_usd_reported`、`complete`；
  `AgentManager` 把它归入当前调用时补上 `agent_id` 与 `recorded_at`（写入时间）。
- 同一 `segment_id` 的分帧按字段取最大值合并，重复上报同一值不会重复计数。
- 累计计数器只做差分、不求和：任一字段回落视为进程重启并重置基线；重启后的第一个累计快照只作基线，计 0。
  差分助手放在共享的 `crates/provider-native/src/usage/cumulative.rs`。带运行总量的帧（如 Codex `total.totalTokens`）仅在总量大于
  上次已接受值时采纳，重复或乱序帧丢弃。
- `boundary`：`request`（提供方按请求给出）、`turn`（整轮只有一个数）、`cumulative_delta`（由累计值差分）。
- 不记录猜测的费用：只存提供方自己报告的 `cost_usd_reported`；价格表估算是读取时的后续功能。

### UsageLedger 端口

端口在 `crates/provider`，方法为 `begin_call`、`record_request`、`finish_call`、`recover_interrupted`、
`summarize(query)`。

- `finish_call` 携带 `token_usage_available: bool`。端口上的“上报 token 用量”能力默认 `false`，
  会发出 `RequestUsage` 的适配器覆盖为 `true`。
- `usage_status` 由 `finish_call` 计算：该调用的每个请求都满足 `boundary = request` 且 `complete = true`，
  且至少有一个请求，为 `exact`；没有请求或提供方不上报 token 用量，为 `missing`；其余为 `lower_bound`。
  失败、取消或被中断的调用可能丢失进行中或未刷出的请求，因此 `exact` 降为 `lower_bound`，`missing` 不变。
- 账本错误只用 `tracing::warn!` 记录，永不使轮次失败或延迟。写入经有界通道交给单一阻塞写线程，
  `AgentManager` 轮询循环不等待 SQLite。
- 调用来源：`user`、`bot`、`schedule`、`heartbeat`、`autonomous`、`summary`。启动被拒的调用仍按其来源记录，
  结果为 `rejected`（无请求）。向运行中轮次的 steer、`out_of_band` 命令、额度读取和历史/恢复读取不是新调用。

### 存储：`agents/usage.sqlite3`

经 `provider::storage::open` 打开，使用独立的存储族 id。三张表：

```text
calls(call_id PK, agent_id, provider, model, source, outcome, usage_status,
      started_at, ended_at, workspace_id)
      -- outcome: running | completed | failed | cancelled | rejected | interrupted
requests(agent_id, segment_id, call_id, parent, model, boundary,
         input, output, cache_read, cache_write, cache_write_1h, reasoning,
         cost_usd_reported, complete, recorded_at,
         PRIMARY KEY (agent_id, segment_id))
daily_usage(day, provider, model, source,
            calls, failed_calls, requests, input, output, cache_read, cache_write,
            cost_usd_reported, cost_reported_requests, lower_bound_calls,
            PRIMARY KEY (day, provider, model, source))
```

`day` 与所有时间戳使用 UTC；不使用本地时区日键。

幂等：请求以 `ON CONFLICT DO NOTHING` 插入，仅当确实插入了一行时才在同一事务内更新汇总，因此恢复或重放不会重复计数。
调用在开始时写为 `running`，在 `AgentManager::poll` 的终态分支（`publish_terminal` 旁）完成；启动时残留的
`running` 变为 `interrupted`。请求归属于它自报的模型，缺失时依次用调用的 `runtime_info.model`、`config.model`。

保留期：`calls` 与 `requests` 保留 90 天，`daily_usage` 无限期保留。v1 没有开关。

隐私：不存提示词、输出文本、路径或凭据；数据只留在本机，不进入 Hub outbox。

### `provider.usage.ledger` 契约

- 请求：`provider.usage.ledger.request { from, to, groupBy: ("provider"|"model"|"source"|"outcome"|"day")[] }`。
- 响应：`{ rows: { key, calls, failedCalls, requests, inputTokens, outputTokens, cacheReadTokens,
cacheWriteTokens, reportedCostUsd, costReportedRequests, lowerBoundCalls, missingCalls }[], since }`；
  `since` 是本 Host 账本的起点，客户端据此说明更早的历史不存在。
- 客户端按方法名协商（`features.providerUsageLedger`），与 `providerUsageList` 相同。
  模式放在 `packages/protocol/src/messages.ts`，客户端方法 `DaemonClient.listUsageLedger` 放在 `packages/client`。
- 客户端在用量弹窗新增 History 标签（24h、7d、30d）；含 `lower_bound` 或 `missing` 调用的数字显示“≥”并说明原因。
  “All hosts”视图逐 Host 汇总，无法连接的 Host 单独列出而不记为 0。

### 各适配器的请求边界

| 适配器                 | 映射                                                                                                                                    | 边界                  |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| Claude                 | 每个 Anthropic message id 一个请求，取最大值合并；缓存读、创建与 1 小时创建子集分开；子 Agent 帧带 `parent`                             | `request`             |
| Codex                  | 每个被接受的 `last` 一个请求，各线程按 `total.totalTokens` 游标去重；输入减缓存读写；子 Agent 线程带 `parent`；非当前轮次的帧只推进游标 | `request`             |
| Pi、OMP                | 每条 assistant 消息一个请求（OMP 在累加进快照前发出）；Pi 记录提供方费用                                                                | `request`             |
| MiniMax                | 每轮一个请求                                                                                                                            | `turn`                |
| Antigravity            | 经累计助手差分；重启后首值为基线                                                                                                        | `cumulative_delta`    |
| OpenCode/ACP、DeepSeek | 无 token 数据                                                                                                                           | `missing`             |
| 摘要/标题调用          | 计一次调用；仅适配器能返回时才有 token                                                                                                  | `missing`，扩展前如此 |

### 已接受的五项选择

1. Claude 子 Agent：流中带其帧时按带 `parent` 的请求计入；没有时该调用为 `lower_bound`。不引入本地代理。
2. 摘要与标题生成按来源 `summary` 计入。
3. 跨 Host：每台 Host 只计自己的调用，由客户端相加；不经 Hub outbox。
4. 费用：只存提供方报告的费用，价格表估算留作后续。
5. 保留：`calls`、`requests` 90 天，`daily_usage` 无限期，v1 无开关。

## 后果

- 账本是与 `AgentUsage` 快照并行的第二条用量通道；快照继续是替换值，请求用量是可累加的，二者不可混用。
- 不采用 Cindy 的自有网关价目表与计费路由、Claude 子 Agent 的回环代理、Vertex 输出滞后检测和本地时区日键。
- 相关的进行中草案 PR #650（ADR-238，只读的单 Agent 会话统计）展示 `lastUsage` 快照且不保存累计状态；本账本与之互补，记录可累加的逐请求事件，从不对快照求和。
- 无 token 数据的适配器（`missing`）只贡献调用次数。旧历史不回填，`since` 标明起点。
