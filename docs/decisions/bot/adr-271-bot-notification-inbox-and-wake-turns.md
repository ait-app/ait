---
id: ADR-271
status: accepted
date: 2026-11-12
amends:
  - bot/adr-213-single-workspace-manager-bot.md
  - bot/adr-215-bot-permissions-and-scoped-mcp.md
  - bot/adr-227-bot-multi-message-output.md
  - clients/adr-177-native-local-and-remote-notifications.md
  - workspace/adr-138-workspace-owned-schedules.md
paths:
  - crates/bot
  - crates/domain/src/bot.rs
  - crates/domain/src/schedule.rs
  - crates/model/src/bot.rs
  - crates/model/src/session.rs
  - crates/persistence/src/storage/bot.rs
  - crates/schedule/src/engine.rs
  - crates/schedule/src/ports.rs
  - crates/schedule/src/service.rs
  - crates/api/src/mcp
  - bins/daemon/src/host/notifications.rs
  - bins/daemon/src/host/schedule.rs
  - packages/protocol/src/bot
  - packages/client/src/protocol
  - apps/mobile/src/bots
---

# ADR-271：Bot 通知收件箱与合并唤醒轮次

- 状态说明：接受。本 ADR 落地 Bot 能力内的收件箱、唤醒与路由（工作包 A）与 daemon 事件源接线（工作包 B，第 13 节）；客户端协议、展示与唤醒范围设置（工作包 C）按本文契约实现，规则见 [Bot 规格](../../architecture/bot.md#访客与客户端契约)。
  合入前的修复轮修订了第 2–5、11–13 节并新增第 14 节：已观察的 Agent 不再重复唤醒、卡住的目标不再占满收件箱、被重启打断的唤醒轮次重试、
  关闭与重启时的定时任务结果不再丢失、去重覆盖重放窗口，以及心跳与定时任务 Agent 的审批请求不再重复通知。
- 关系：实现[管理者重设计](../../plans/workspace-bot-manager.md)阶段 4“通知与唤醒”；扩展 [ADR-213](adr-213-single-workspace-manager-bot.md) 的生命周期与 v2 方法、
  [ADR-215](adr-215-bot-permissions-and-scoped-mcp.md) 第 3 节的保留标签与 `botScope`、[ADR-227](adr-227-bot-multi-message-output.md) 的轮次记录。

## 背景

Bot 只在用户发消息时执行。它启动的 Agent 完成、失败或等待审批时没有任何东西告诉它，从 Telegram 交办的任务因此不会回报。
设计要求：通知进入持久的每 Bot 收件队列，短窗口合并后在发起该工作的对话中唤醒一轮；未配置或暂停的 Bot 零调用；
Bot 可判断无需告知；不得因 Agent 连续完成而无限循环。通知记录同时是今后 Router／Worker 形态的触发器，因此保持通用：
稳定 id、来源、主体与来源对话，不对 Agent 特化。

## 决策

### 1. 通知记录

`domain::bot::notification::Notification`（JSON camelCase）：

| 字段                                           | 说明                                                                                                                                                              |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                           | 稳定、确定性的去重键，1–256 字节、无控制字符。daemon 使用 `agent:<agentId>:<attention 时间戳>`、`schedule:<scheduleId>:<runId>`、`terminal:<terminalId>:<时间戳>` |
| `workspaceId`                                  | 事件发生的 Workspace                                                                                                                                              |
| `scope`                                        | 可选；主体带 `ait.bot.scope` 标签（Agent）或 `botScope`（定时任务）时为该根 Workspace                                                                             |
| `kind`                                         | `agent_finished`、`agent_failed`、`agent_permission`、`schedule_succeeded`、`schedule_failed`、`terminal_attention`                                               |
| `agentId`、`scheduleId`、`runId`、`terminalId` | 可选主体 id                                                                                                                                                       |
| `title`                                        | 简短标题，超过 256 字节时在字符边界截断                                                                                                                           |
| `summary`                                      | 可选摘要，超过 1024 字节时在字符边界截断                                                                                                                          |
| `at`                                           | 事件时间（RFC 3339）                                                                                                                                              |
| `conversationId`                               | 可选来源 Bot 对话（来自下文第 9 节的来源标签或字段）                                                                                                              |

id、`workspaceId`、`scope` 与主体 id 不合法时整条拒绝（`InvalidMessage`）。

### 2. 入口与准入

daemon 调用 `bot::service::Bots::notify(&self, Notification) -> Result<Notified, ErrorCode>`，
`Notified` 为 `Admitted`、`Duplicate`、`Ignored`。

- 先在调用方线程按 `Scopes` 已发布的快照检查 `workspaceId` 与 `scope` 是否有处于 `active` 的当前 Bot；都没有时直接返回 `Ignored`，
  不进入 Bot worker、不写盘、不建会话、不调用模型。随后 worker 按权威目录再判一次。
- 候选 Bot 为 `workspaceId` 的当前 Bot 与 `scope` 的当前 Bot（去重）；只有 `active` 的接收，`paused`、`retired` 或不存在则丢弃且无副作用。
- 同一 Bot 的日志中已有相同 `id`，或该 Bot 在 `WAKE_WINDOW`（1 小时）内创建的唤醒轮次携带过该 `id` 时，返回 `Duplicate`，不写盘。
  后者在内存中维护，启动时从唤醒轮次记录重建，因此已唤醒过的通知即使已被日志淘汰（第 3 节），在 daemon 的 15 分钟重放窗口（第 13 节）内重放也不会再次唤醒。
  已被淘汰的 `logged` 或 `skipped` 条目重放时按新通知重新接纳；daemon 不应重放早于 15 分钟的通知。
- 写入持久化后才返回 `Admitted`；写入失败按现有规则冻结（`frozen`）。方法只等待一次文件写入，从不等待模型执行。

### 3. 日志、收件箱与存储格式

每个 Bot 一个原子 JSON 文件 `<data>/workspace-bots/notifications/<botId>.json`（`domain::bot::notification::NotificationLog`）：

```json
{"botId":"…","nextSequence":8,"entries":[
  {"sequence":7,"admittedAt":"2026-11-12T08:00:00Z","notification":{…},"wake":{"state":"pending"}}]}
```

- 每条被接纳的通知获得 Bot 内单调递增的 `sequence`（从 1 开始），进入日志（供概览）。
- `wake.state`：`logged`（不在唤醒范围，只记日志）、`pending`（在收件箱中）、
  `woke`（`conversationId`、`turnId`：已并入该唤醒轮次）、`skipped`（`reason`：`over_budget`、`inbox_full`、`paused`，
  以及 `observed`（第 14 节）与 `blocked`（第 5 节））。客户端对未知的 `state` 与 `reason` 按通用文本展示，不拒绝整个列表。
- 收件箱即 `pending` 条目，至多 `INBOX_LIMIT = 64`；满时新的范围内通知记为 `skipped/inbox_full`。
  非 `pending` 条目至多保留 `LOG_LIMIT = 128`，超出时淘汰最旧的；`pending` 条目从不被淘汰。

### 4. 唤醒范围

Bot 新增设置 `wakeScope`：`own`（默认）只让 `scope` 等于本 Bot 根 Workspace 的通知进入收件箱；`all` 另让 `workspaceId` 等于根 Workspace 的所有通知进入收件箱。
`bot.setup` 与 `bot.update` 接受可选 `wakeScope`（`update` 省略时不变；`setup` 省略按 `own` 参与幂等比较），`bot.get` 返回 `wakeScope`。立即作用于之后的通知。
`bot.update` 的 `execution` 也改为可选，省略时不变、提供时行为不变；客户端只修改唤醒范围时不发送 `execution`，
以免覆盖其他设备在此期间对模型的修改。不支持通知的旧 Host 仍要求 `execution`，客户端只对报告了通知特性的 Host 省略它。

### 5. 目标对话与合并

- 目标对话：来源 `conversationId` 仍存在、属于当前 Bot、所在 Channel 已启用未删除、不是 Webhook Channel，且其当前 generation 没有 `uncertain` 轮次时，使用它；
  否则使用内置 Channel 的 `owner` 对话（不存在时按 `bot.conversation.open` 的方式创建）。Webhook Channel 只能在中心发起的轮次上回传结果，
  无法投递主动消息，因此来源为 Webhook 的通知回到 `owner` 对话。目标在触发时解析。
- 合并：同一目标的 `pending` 条目在最后一条到达后静默 `WAKE_QUIET = 3 s`，或最早一条已等待 `WAKE_MAX_DELAY = 30 s` 时，合并为一个唤醒轮次；
  目标对话有执行中或排队的轮次时继续累积，待其结束后触发。每个唤醒轮次至多并入 `WAKE_BATCH = 16` 条（按序号），其余留在收件箱等待下一次。
- 解析出的目标（包括回退后的 `owner` 对话）当前 generation 有 `uncertain` 轮次、在用户 reset 前无法执行时，到期条目不再保留，
  而是记为 `skipped/blocked`：它们仍在日志与该对话之后的“未看过”通知中。收件箱上限按 Bot 计，保留这些条目会让一个卡住的对话占满收件箱，
  使其他对话的通知都记为 `inbox_full`；按目标分别限额则会让日志超出存储上限，且只会在 reset 后集中唤醒。`blocked` 让设置列表与概览说明原因。
- 唤醒轮次走同一对话的串行管道与积压上限：全局积压满时条目留在收件箱，下一次检查（每秒及每条命令后）重试，不静默丢弃。

### 6. 成本护栏

每个对话在滚动 `WAKE_WINDOW = 1 h` 内至多 `WAKE_BUDGET = 12` 个唤醒轮次。超出时该对话的待触发条目改为 `skipped/over_budget`，只留在日志中，
通过 `bot.notifications.list` 可见，并会出现在该对话下一轮的“未看过”通知里。预算按已存在的唤醒轮次的 `createdAt` 计算，重启后从轮次记录恢复。

### 7. 轮次记录

`Turn` 新增三个可选字段，缺省时不序列化，旧记录照常加载：

- `wake: {"notifications":[Notification…]}`：唤醒轮次的来源，至多 16 条；用户输入的轮次没有该字段。唤醒轮次 `messageId` 为 `notification:<turnId>`，
  `replyTo` 为 `null`，`text` 是围栏 `<ait-notifications>` 包住的通知文本（内容中的 `</` 转义为 `<\/`，与记忆围栏相同）。
- `silent: true`：Bot 判断无需告知时为真。
- `notificationCursor`：轮次开始执行（进入 `sending`）时 Bot 日志的最大序号；日志为空时不写。

### 8. 静默回复

唤醒轮次完成时最后一条 assistant 消息去掉首尾空白后恰为 `NO_REPLY`（`crate::instructions::SILENT_REPLY`），该轮次标记 `silent: true`，
投递直接记为 `delivered`，任何外部 Channel 都不发送。用户输入的轮次不适用。内置指令说明唤醒轮次与该哨兵。

### 9. 来源标签与字段（由接线工作包写入）

- Agent 标签 `ait.bot.conversation = <Bot 对话 id>`（`domain::bot::BOT_CONVERSATION_LABEL`，位于保留前缀 `ait.bot.` 下，与 `ait.bot.scope` 并列），
  记录发起该 Agent 的 Bot 对话；ADR-215 对 `ait.bot.` 前缀的写入限制同样适用。
- 定时任务记录在 `botScope` 旁新增 `botConversation`（Rust `bot_conversation: Option<ConversationId>`），记录创建它的 Bot 对话；运行产生的通知以它为 `conversationId`。

### 10. 概览读取：`bot.notifications.list.request`

请求 `{"workspaceId":"…","unseenBy":"<conversationId>"?,"limit":1..128?}`（默认 50）；
响应 `{"headSequence":n,"seenSequence":m|null,"wakeScope":"own","notifications":[日志条目…]}`，条目按序号倒序。

- 不带 `unseenBy` 时返回最近的日志。
- 带 `unseenBy` 时只返回序号大于该对话最近一个 `completed` 轮次的 `notificationCursor`（即 `seenSequence`）的条目。
  该读取是纯读取：游标只在轮次完成时推进，失败、取消或 `uncertain` 的轮次不消耗通知。
  轮次开始后才到达、又在本轮被概览读到的通知，下一轮仍会再出现一次，宁可重复也不遗漏。
- 未配置 Bot 返回 `BotNotFound`。方法加入 `bot::capabilities::METHODS`。

### 11. 生命周期与崩溃一致性

- 暂停（用户或目录缺失）把所有 `pending` 条目改为 `skipped/paused` 并停止准入；恢复不补发。移除与 Workspace 归档删除该 Bot 的通知文件；
  启动时发现退役或不存在的 Bot 的通知文件也会删除。
- 触发顺序：先提交轮次（携带全部并入的通知），再把条目改为 `woke` 并保存日志。启动时按唤醒轮次携带的通知 id 把仍为 `pending` 的条目改为 `woke`，
  因此任一时刻重启都不会丢失已接纳的通知，也不会生成第二个唤醒轮次；轮次写入失败时条目仍为 `pending`。
  该补标记只作用于仍为 `queued` 的唤醒轮次，这是提交轮次与保存日志之间崩溃时唯一可能的状态。
- 用户取消或因 reset 取消的唤醒轮次不会重新唤醒，其通知保持 `woke`，并因该轮次未完成而仍出现在 `unseenBy` 结果中。
- 被 daemon 打断的唤醒轮次会重试：服务停止期间（`Bots::stop` 之后）被取消或因此记为 `uncertain` 的唤醒轮次，
  以及启动时发现仍为 `sending`、`running` 或 `needs_approval` 的唤醒轮次（崩溃），其条目回到 `pending`，下一次检查重新唤醒。
  崩溃时该对话照旧记为 `uncertain`，因此重试按第 5 节回退到 `owner` 对话；`owner` 也 `uncertain` 时记为 `skipped/blocked`。
  条目同一时刻只属于一个唤醒轮次，每次中断只产生一个重试轮次；被打断的唤醒轮次照常计入第 6 节的预算，因此反复中断不会无限循环。
  写入顺序保证任一时刻崩溃都只重试一次：关闭时先把条目改回 `pending` 并保存日志，再提交该轮次的终态；
  启动时先保存把条目改回 `pending` 的日志，再写回改为 `uncertain` 的轮次。两次写入之间崩溃时，该轮次在磁盘上仍为执行中，
  下次启动照常视为被打断，而条目已是 `pending`，不会再改动，因此既不丢失也不重复重试。
- 外部 Channel 对唤醒轮次发送普通消息：Telegram 发到原 chat 与话题；飞书发到原 chat（不在话题内回复）；
  QQ 唤醒轮次没有可用的原始消息锚点，投递记为 `uncertain`；直接回复入站 QQ 消息时只使用仍有效的被动回复锚点；
  微信不带 context token 发送；iMessage 不受影响。

### 12. 降级兼容

`Bot`、`Turn` 均为 `deny_unknown_fields`。旧 daemon 读到 `wakeScope: "all"` 的 Bot，或带 `wake`、`silent`、`notificationCursor` 的轮次时，
Bot 存储加载失败，daemon 启动报错 `start Bot service`。新字段在默认值时不序列化，所以未收到过通知、`wakeScope` 为 `own` 的 Bot 仍可降级；
`notifications/` 目录被旧 daemon 忽略。新的通知记录类型（`Notification`、`NotificationLog`、`Wake` 等）不拒绝未知字段，
以后在其中加字段不会让本版本拒绝加载。降级前需把 `wakeScope` 设回 `own`，并删除含唤醒轮次的对话或整个 `<data>/workspace-bots/`。这与 ADR-215、ADR-227 增加字段时的约束相同。

实际后果比上一句更宽：日志包含 Workspace 中任何 Agent 的完成（`wakeScope: own` 时也记为 `logged`），日志一旦非空，Bot 的下一个轮次就会写入
`notificationCursor`；任何被唤醒过的 Bot 都有 `wake` 轮次。因此在有活动的 Workspace 中，几乎每个 Bot 在升级后很快就无法被旧 daemon 加载，
降级实际上意味着删除 `<data>/workspace-bots/`（或删除该 Bot 的全部对话）。只在游标与已看过的值不同时才写入并不能改变这一点：
新的日志条目几乎总会让游标变化，所以保留现有写法。

### 13. 事件源接线

- **来源记录**：`bot::mcp::CallerKind::Bot` 携带会话属主（`SessionOwner`）中的 `conversation_id`，会话轮换后仍指向同一对话。
  作用域 `create_agent` 对 Bot 调用者写入 `ait.bot.conversation = <该对话>`；子树调用者复制其自身 Agent 记录上的该标签（不是合法对话 id 时不写），
  因此孙代 Agent 回到同一对话。作用域 `create_schedule` 与 `create_heartbeat` 以同样来源写入 `botConversation`（工具 schema 不接受该字段），
  定时任务创建后不可修改；运行创建的 Agent 与 `ait.bot.scope` 一起带上该标签。ADR-215 第 3 节对 `ait.bot.` 前缀的写入限制不变。
- **Agent 与终端 attention**：`model::session::SessionEvents::subscribe_committed_attention()` 是新的有界（64）广播，
  在焦点判断与中心投递之前收到每个已提交的 attention 事件；ADR-177 的推送通道（`subscribe_attention`）行为不变。
  daemon 的转发任务（`bins/daemon/src/host/notifications.rs`）按 Agent 当前记录生成通知，id 为 `agent:<agentId>:<事件 timestamp>`，
  `scope`、`conversationId` 取自标签，失败时 `summary` 为 `lastError`。跳过：内部会话（Bot 自身会话）、已归档、记录中已有不同 attention 时间戳（被更新的 attention 取代）、
  没有 Workspace，以及带 `paseo.schedule-run` 的 Agent 的全部 attention（完成、失败与等待审批）：其运行结果就是通知，
  而运行器在该 Agent 等待审批时会取消它并使运行失败（错误为 “Scheduled agent is waiting for permission”，默认随后归档），单独的审批通知只会让用户去批准一个已归档的 Agent。
  客户端已清除的 attention（时间戳为空）仍通知。
  终端 attention 的 id 为 `terminal:<terminalId>:<接收毫秒>`，没有来源，只在 `wakeScope: all` 时唤醒。
- **重放边界**：转发任务先订阅再扫描；启动时与接收方报告 `Lagged` 时扫描 Agent 注册表，只重放当前仍需 attention 且时间戳在
  `RESCAN_WINDOW = 15 min` 内的记录，依靠 Bot 侧按 id 去重。更早的 attention 不重放，因此长期等待的 Agent 不会在每次重启后再次唤醒。
- **定时任务结果**：`schedule::ports::Runner` 新增默认空实现的 `settled(&Schedule, &Run)`，在每次运行结算写盘后调用，
  包括打开存储时把中断的运行记为失败；`crates/schedule` 不依赖 `crates/bot`。daemon 的运行器生成 id 为 `schedule:<scheduleId>:<runId>` 的通知
  （`schedule_succeeded`／`schedule_failed`，`summary` 为输出或错误的前 1024 字节），包括创建 Agent 之前的失败（如 `REVOKED`），并经 `Bots::offer` 在后台提交。
  以已有 Agent 为目标的定时任务（心跳）运行成功时不生成通知，该 Agent 自己的完成 attention 已经报告；失败仍通知，因为 Agent 运行前的失败
  （`REVOKED`、Agent 不存在或已归档）没有 attention。
- **关闭与重启时的运行结果**：`Bots::stop` 只停止 Channel 与执行中的轮次，Bot worker 继续接纳通知（只记日志、不唤醒），直到 `Bots::shutdown`；
  `shutdown` 先等待所有经 `Bots::offer` 提交、尚未完成的通知。`Api::wait_closed` 先关闭定时任务（结算被取消的运行）再关闭 Bot，
  因此关闭时结算的运行结果会写入日志，重启后唤醒。`Schedules::spawn` 打开存储后，对 `endedAt` 在 15 分钟（`REOFFER_WINDOW`，与 attention 重放一致）
  内的每个已结算运行再调用一次 `settled`（包括刚记为中断的运行），依靠确定性 id 与 Bot 去重；因此 `settled` 是“至少一次”。
  仍会丢失的只有：结算写盘后、通知写入日志前进程崩溃，且 15 分钟后才重新启动；或 Bot 存储写入失败（冻结）。
- **概览**：`workspace_overview` 以调用者会话的 `conversation_id` 调用 `bot.notifications.list.request`（`unseenBy`），
  最多保留 20 条（`summary` 截到 512 字节），按序号倒序，计入 24 KiB 预算：超限时先去掉 `summary`，再与其他列表一起截断；
  该对话已不属于 Bot 时为 `null`。节点级访客可读 `bot.notifications.list.request`，Workspace／项目访客与 Webhook 自动化不可读。

### 14. 已观察的 Agent 不重复唤醒

Bot 会按指令用 `wait_for_agent` 等待交办的 Agent 并在同一轮次回报；该 Agent 的完成通知随后到达，若再唤醒一次，就会多一次模型调用或重复回报。
抑制只依据证据，从不只看时间，因为漏报用户没收到的结果比重复更糟：

- Bot 会话（`CallerKind::Bot`，带其对话 id）经作用域 MCP 调用 `wait_for_agent`、`get_agent_status` 或 `get_agent_activity`，
  结果显示该 Agent 已不在执行（状态 `idle`、`error` 或 `permission`）时，`crates/api` 调用 `Bots::observe(conversationId, agentId, at, readStarted)`，
  其中 `at` 是结果中 Agent 快照的最后变更时间 `updatedAt`（`get_agent_activity` 的结果为此新增 `status` 与 `updatedAt`，取自读取时间线之前的 Agent 记录），
  快照带有更晚的 `attentionTimestamp` 时取后者：审批请求只写 attention、不改 `updatedAt`，而该时间戳与 `agent_permission` 通知的 `at` 取自同一注册表字段。
  `wait_for_agent` 与 `get_agent_status` 的快照带该字段；`get_agent_activity` 的结果不带，只按 `updatedAt` 计，且只有 `wait_for_agent` 会报告 `permission` 状态，
  因此只有 `wait_for_agent` 读到的审批请求会被抑制，其余情况最坏是重复唤醒。
  Agent 的完成状态与 attention 时间戳在同一次注册表提交中写入，二者都是毫秒精度，因此该时间不早于它所反映的任何 attention。
- 读取不等于回报，完成的轮次才是证据：观察先挂在该对话正在执行的轮次上，该轮次 `completed` 时才生效；以失败、取消、`uncertain` 或 daemon 重启结束时丢弃，
  相关通知照常唤醒。该对话没有执行中的轮次时的读取不记录。观察轮次仍在执行时，条目不会被记为 `skipped/observed`。
- 观察属于发起读取的轮次，而不是读取返回时恰好在执行的轮次：`crates/api` 在分派工具调用之前记下调用开始时间 `readStarted`，
  worker 在轮次开始执行（`ready`）时记下开始时间，调用开始早于该对话当前轮次开始时间的读取不记录。
  例如已取消轮次的 `wait_for_agent`（最长等待 45 秒）在下一轮执行时才返回，它不会让下一轮的完成抑制一份没人回报过的通知。
  两个时间都取自同一 daemon 进程的单调时钟，前一轮次结束后下一轮次才可能开始，因此判断不受系统时钟调整影响。
- 生效后，关于该 Agent 的 `agent_finished`、`agent_failed`、`agent_permission` 待唤醒条目，若解析出的目标就是该对话且 `at` 不晚于观察时间，
  记为 `skipped/observed`，不再唤醒。更晚的通知、其他 Agent、目标为其他对话的通知、定时任务通知都不受影响；子树 Agent 的读取不算观察。
- 观察只保存在 Bot worker 内存中：已生效的至多 256 条，每个执行中轮次挂起的也至多 256 条，超出时淘汰最旧的；
  重启后最坏情况是再唤醒一次（重复而不遗漏）。`crates/api` 依赖 `crates/bot`，方向不变。

## 后果

- Bot 能在通知到达时回报，且回到交办任务的对话；无需告知时不打扰用户。
- 未配置与暂停的 Workspace 仍为零调用；连续通知最多每对话每小时 12 次模型调用。
- 每条接纳的通知重写一次该 Bot 的日志文件（上限约数百 KiB）；通知频率低，可接受。
- Webhook 来源的通知不回到 Webhook 调用方，因为中心协议没有主动推送。

## 验证

`crates/domain`、`crates/persistence`、`crates/bot` 中的定向测试覆盖：未配置与暂停零成本、去重、`own`／`all`、合并为一轮、执行中累积、
目标回退、预算耗尽、带收件箱与带已提交唤醒轮次的重启恢复、静默回复不外发、旧轮次与目录加载。修复轮另行覆盖：`uncertain` 目标记为 `blocked`
且不阻塞其他对话、关闭与崩溃打断的唤醒轮次各重试一次而用户取消的不重试、关闭与启动时在每次写入后崩溃仍只重试一次、日志淘汰后的重放仍去重、
已观察的 Agent 不唤醒且观察轮次未完成时照常唤醒（Bot 状态与经作用域 MCP 的端到端）、快照中审批请求的 attention 时间计入观察、
停止后提交的通知在关闭前写入、启动时重报近期结束的运行、心跳与定时任务 Agent 的规则，以及 daemon 端到端测试中真实重启后的重放。
按仓库规则只运行受影响 crate 的测试。

## Test coverage

未测量：本次未要求覆盖率统计。
