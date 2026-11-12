---
id: ADR-274
status: accepted
date: 2026-11-13
amends:
  - bot/adr-213-single-workspace-manager-bot.md
  - bot/adr-215-bot-permissions-and-scoped-mcp.md
  - bot/adr-271-bot-notification-inbox-and-wake-turns.md
  - providers/adr-202-agent-mcp-injection.md
paths:
  - crates/bot
  - crates/domain/src/bot.rs
  - crates/domain/src/bot/ticket.rs
  - crates/domain/src/lib.rs
  - crates/model/src/bot.rs
  - crates/persistence/src/storage/bot.rs
  - crates/api/src/mcp
  - crates/api/src/guest/policy/node.rs
  - crates/domain/src/bot/permissions.rs
  - bins/daemon/src/host/notifications.rs
  - packages/protocol/src/bot
  - packages/client/src/protocol
  - apps/mobile/src/bots
  - crates/provider/runtime/src/ports/mcp_scope.rs
  - crates/provider/runtime/src/service/agent_mcp.rs
  - crates/provider/core/src/ports/agent_session.rs
  - crates/provider/claude/src/lib.rs
---

# ADR-274：Bot 工单、指派与汇报

- 状态说明：接受。本 ADR 落地 Bot 能力内的记录、存储、状态机、通知挂接、入口与客户端 RPC（工作包 A），
  作用域 MCP 的工单创建流程、工具、概览、访客规则与 daemon 的 `ticketId` 填写（工作包 B，第 6、7、9、10 节）；
  客户端展示（工作包 C）按第 9 节的契约实现。修复轮按评审修订了多个受阻者的等待、达到上限时自动关闭、给 Agent 更多工作时重新打开、
  重复关闭时保存结论、不需指派即可汇报、崩溃后补发汇报唤醒、工单读取失败不阻止接纳、同一轮的汇报与结束只唤醒一次，
  以及作用域 `create_agent` 不因调用超时丢掉收尾（第 2、4–8、10–14 节）。
  第二修复轮收窄了“同一轮只唤醒一次”的规则，让自动关闭只依据最近一次重新打开之后的汇报、并取消全部 Agent 已归档的等待中工单，
  在 `send_agent_prompt` 的重新打开受上限所阻时先自动关闭再重试并告知 Bot，让分离执行的 `create_agent` 一直占用调用额度并有自身上限，
  并在预授权为追加式的 Provider（Claude）上为工作者预授权 `report_progress`（第 2、4–7、10 节）。
- 关系：扩展 [ADR-271](adr-271-bot-notification-inbox-and-wake-turns.md) 的通知记录、通知类型、唤醒输入与第 14 节的观察规则；
  扩展 [ADR-215](adr-215-bot-permissions-and-scoped-mcp.md) 第 3 节的保留标签与工具访问分类；扩展 [ADR-213](adr-213-single-workspace-manager-bot.md) 的生命周期与 v2 方法；
  扩展 [ADR-202](../providers/adr-202-agent-mcp-injection.md) 的会话绑定：工作者凭据在有效配置中追加 `report_progress` 的预授权（第 10 节），
  并为 [ADR-205](../providers/adr-205-provider-execution-contracts.md) 的 `AgentClient` 端口增加一个默认关闭的能力。

## 背景

ADR-271 之后，Bot 启动的 Agent 完成、失败或等待审批时会在发起对话中唤醒 Bot。但仍没有持久的“工作单元”：
用户交办的任务是什么、哪些 Agent 在做、最近汇报了什么、是否仍未结束，都只存在于模型上下文里。
于是 Bot 倾向于在一轮中 `wait_for_agent` 等 Agent 结束（阻塞对话），完成的工作无人关闭，用户也看不到 Bot 正在为自己做哪些事。

本决策引入该单元：**工单**（Ticket，一项交办任务）、**指派**（Assignment，在做它的 Agent）与**汇报**（Report，工作者与系统的进展）。
目标形态是“路由与工作者”：Bot 对话负责分派与总结，Agent 并行工作并回报；Bot 交出工作后即结束本轮，值得告知时再被唤醒。

## 决策

### 1. 记录

模块 `domain::bot::ticket`，JSON camelCase。新记录不拒绝未知字段，以后在其中加字段不会让本版本拒绝加载（与 ADR-271 的通知记录相同）。

**`TicketId`**：与 `BotId` 相同形态的 UUID 身份（`domain::TicketId`，非 nil）。

**`Ticket`**：

| 字段                     | 说明                                                                                                                      |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------- |
| `id`                     | `TicketId`                                                                                                                |
| `botId`                  | 所属 Bot（内部身份，客户端投影不返回）                                                                                    |
| `title`                  | 简短标题，1–256 字节，超出在字符边界截断；去掉首尾空白后为空则拒绝                                                        |
| `request`                | 交办内容，至多 4096 字节，超出截断                                                                                        |
| `conversationId`         | 可选，来源 Bot 对话                                                                                                       |
| `status`                 | `open`、`waiting`、`done`、`failed`、`cancelled`（第 4 节）                                                               |
| `waiting`                | `status` 为 `waiting` 时为 `{"reason":"permission"\|"blocked","agentId":"…","since":时间}`，否则 `null`                   |
| `blocked`                | 最近一次汇报为 `blocked` 的工作者，按受阻时间先后，形同 `waiting`，至多 16 个（内部字段，为空时不写出，客户端投影不返回） |
| `createdAt`、`updatedAt` | RFC 3339                                                                                                                  |
| `closedAt`               | 关闭时间，未关闭为 `null`                                                                                                 |
| `outcome`                | 关闭时的结论，至多 2048 字节，超出截断；未关闭为 `null`                                                                   |
| `key`                    | 打开它的 Agent 创建的幂等键，1–256 字节、无控制字符（内部字段，客户端投影不返回）                                         |
| `assignments`            | 指派，至多 `ASSIGNMENT_LIMIT = 16` 条，按时间顺序                                                                         |
| `nextReport`             | 下一条汇报的序号（从 1 开始，内部字段）                                                                                   |
| `reports`                | 最近 `REPORT_LIMIT = 32` 条汇报，按序号升序；超出时淘汰最旧的                                                             |

**`Assignment`**：`{"agentId":"…","workspaceId":"…","assignedAt":时间}`。两个 id 按 `model::valid_id` 校验（1–128 字节、无控制字符）。
指派只记录持久关系，不复制 Agent 的实时状态；读取方按 `agentId` 从 Agent 目录合并。同一 Agent 只记一次。

**`Report`**：`{"sequence":n,"at":时间,"source":"agent"|"bot"|"user"|"system","kind":…,"agentId":"…"|null,"text":"…","status":…|null}`。

- `kind`：工作者汇报 `progress`、`blocked`、`done`、`failed`；系统汇报 `agent_finished`、`agent_failed`、`agent_permission`；
  Bot 备注 `note`；Bot、用户或 Ait（`source: "system"`，第 2、7 节的自动关闭与重新打开）改变状态时为 `status`。
- `agentId`：工作者汇报与系统汇报所涉 Agent。
- `text`：至多 `TEXT_LIMIT = 2048` 字节，超出截断；系统汇报取通知的 `summary`（没有时为空）。
- `status`：该汇报使工单状态改变时，为改变后的状态；等待从一个原因或 Agent 换到另一个时（第 4 节）为 `waiting`。

### 2. 上限

| 常量               | 值   | 说明                                                                                           |
| ------------------ | ---- | ---------------------------------------------------------------------------------------------- |
| `OPEN_LIMIT`       | 32   | 每个 Bot 未关闭（`open`、`waiting`）的工单上限；打开或重新打开第 33 个返回 `ResourceExhausted` |
| `CLOSED_LIMIT`     | 64   | 每个 Bot 保留的已关闭工单；超出时删除 `closedAt` 最早的                                        |
| `ASSIGNMENT_LIMIT` | 16   | 每个工单的指派；超出返回 `ResourceExhausted`                                                   |
| `REPORT_LIMIT`     | 32   | 每个工单保留的最近汇报                                                                         |
| 文本               | 见上 | 标题 256、请求 4096、结论与汇报 2048 字节，均在字符边界截断                                    |

达到 `OPEN_LIMIT` 时从不静默丢弃，但上限能自行恢复：只靠模型收尾，忘记关闭工单的 Bot 会在第 33 项任务停住。
作用域 `create_agent` 打开工单遇到 `ResourceExhausted` 时（只在这时，不在常规路径上）先关闭“工作显然已结束”的未关闭工单，再重试一次打开：

- 候选是状态为 `open` 或 `waiting` 且至少有一个指派的工单（`bot.ticket.list.request` 的未关闭页，32 个都在默认页内）。
  等待中的工单在全部 Agent 归档后已无人能回答其受阻或审批请求，同样处理。
- 对每个候选以 `agent.list.request` 按标签 `ait.bot.ticket = <id>` 读未归档 Agent；结果为空且没有下一页（完整读取）时才算“全部 Agent 已归档”，
  包括带标签而未被指派的 Agent。读取失败或不完整时不关闭（未知状态从不关闭任何工单）。已删除的 Agent 不在目录中，与归档同样计。
- 关闭经 `Bots::close_finished_ticket`（系统来源）。`open` 工单只看最近一次重新打开（已关闭状态之后的 `status: open`；结束等待不算）之后保留的工作者汇报：
  每个被指派的 Agent 以及其间汇报过的每个工作者，最近一条都是 `done` 时关为 `done`，否则为 `cancelled`。
  因此重新打开之前的 `done`、或只有一个工作者完成而另一个无声归档，都不算完成。`waiting` 工单一律关为 `cancelled`。
  结论与系统 `status` 汇报的文本都说明是 Ait 自动关闭及原因：`open` 为全部 Agent 已归档且已达未关闭上限；等待受阻者与等待审批的分别说明在等哪一种、
  全部 Agent 已归档所以无人能继续或回答。入口在 Bot 内再次检查状态，期间已关闭或没有指派的工单不动。
- 没有能关闭的工单时照旧拒绝：工具错误 `bot::tickets::OPEN_LIMIT_MESSAGE`
  （“Too many open tickets, and none could be closed automatically. Call bot_tickets and close the tickets whose work is finished or no longer needed with bot_ticket_update (status done or cancelled), then try again”），
  告诉 Bot 关闭哪一类工单。第 7 节的 `send_agent_prompt` 重新打开达到上限时同样先自动关闭再重试一次；
  Bot 与用户的显式重新打开达到上限时不做自动关闭，直接返回同一错误。

关闭依据在 `crates/api`（Agent 目录归它读），`crates/bot` 只提供系统来源的关闭入口，不读 Agent 目录。

### 3. 存储

每个工单一个原子 JSON 文件：`<data>/workspace-bots/tickets/<botId>/<ticketId>.json`，经 `persistence::storage::bot::FileStore` 写入，
记录上限仍为 2 MiB。选择每工单一个文件，而不是每 Bot 一个文件：

- 单个工单正常上限约 100 KiB（汇报 32 × 2 KiB ≈ 64 KiB，指派 16 × 约 350 B，请求 4 KiB，结论 2 KiB，其余字段与 JSON 开销）；
  即使文本全为需要 `\u00XX` 转义的控制字符（最坏 6 倍）也约 600 KiB，低于记录上限。
- 每个 Bot 至多 96 个工单（32 未关闭 + 64 已关闭），合并为一个文件最坏约 9.6 MiB，超出 2 MiB 上限；
  且每条汇报都要重写全部工单。每工单一个文件只重写被修改的工单。
- 以 Bot 分目录，退役时整目录删除。

Bot 服务在内存中只保留每个工单的索引：除 `reports` 外的全部字段，加最新一条汇报（供列表）。
修改时先读该工单文件，在内存中应用变化，原子写回后再更新索引，与轮次记录的读写方式相同。
写入失败按现有规则冻结（`frozen`），只有写入成功后才返回结果；启动时扫描 `tickets/` 重建索引。

`model::bot::Store` 新增：`scan_tickets`（逐条访问，启动用）、`ticket(bot, id)`、`save_ticket`、`delete_ticket(bot, id)`、`delete_tickets(bot)`（整目录，缺失不报错）。

### 4. 状态与转换

`open`（进行中）、`waiting`（在等用户：Agent 等待审批，或工作者汇报 `blocked`）、`done`、`failed`、`cancelled`（后三者为“已关闭”）。
只有 Bot（工具 `bot_ticket_update`）与用户（`bot.ticket.update.request`）能关闭或重新打开工单。
Agent 结束一轮只是汇报，不是关闭，因为 Bot 可能还会继续给它指令。自动转换只有 `open` 与 `waiting` 之间的往返。

| 从                       | 到                            | 触发者                        | 条件与说明                                                              |
| ------------------------ | ----------------------------- | ----------------------------- | ----------------------------------------------------------------------- |
| （无）                   | `open`                        | 系统（作用域 `create_agent`） | 第 6 节 `open_ticket`                                                   |
| `open`                   | `waiting(permission, A)`      | 系统                          | 关于 Agent A 的 `agent_permission` 通知                                 |
| `open`                   | `waiting(blocked, A)`         | 工作者 A                      | A 汇报 `blocked`                                                        |
| `waiting(permission, B)` | `waiting(blocked, A)`         | 工作者 A（可以就是 B）        | A 汇报 `blocked`：受阻优先于审批，A 越过审批也不再结束这个等待          |
| `waiting(permission, A)` | `open`                        | 系统                          | 关于 A 的 `agent_finished` 或 `agent_failed` 通知（A 已越过审批）       |
| `waiting(*, A)`          | `open`                        | 工作者 A                      | A 汇报 `progress`、`done` 或 `failed`，且没有其他受阻的工作者           |
| `waiting(blocked, A)`    | `waiting(blocked, B)`         | 工作者 A                      | 同上，但 B 仍受阻：等待交给受阻最早的 B，`since` 为 B 受阻的时间        |
| `waiting`                | `open`                        | Bot、用户                     | 更新 `status: "open"`（例如已把用户的回答转给 Agent）                   |
| `open`、`waiting`        | `done`、`failed`、`cancelled` | Bot、用户                     | 写入 `closedAt`，可附 `outcome`；清除 `waiting`                         |
| `open`、`waiting`        | `done`、`cancelled`           | 系统（第 2 节）               | 达到上限时自动关闭全部 Agent 已归档的工单；`waiting` 只关为 `cancelled` |
| 已关闭                   | `open`                        | Bot、用户                     | 重新打开：清除 `closedAt` 与 `outcome`；受 `OPEN_LIMIT` 限制            |
| 已关闭                   | `open`                        | 系统（第 7 节）               | Bot 会话向该工单的 Agent `send_agent_prompt`；受 `OPEN_LIMIT` 限制      |

- 工单记下每个最近一次汇报为 `blocked` 的工作者（`blocked`）。工作者汇报 `blocked` 时：工单为 `open` 或在等审批（任何 Agent 的）时等待改为该工作者受阻；
  已因另一工作者受阻而等待时保持原等待，只把该工作者加入 `blocked`。工作者汇报其他种类时移出 `blocked`；工单正等它时交给 `blocked` 中最早的一个，
  没有则回到 `open`。因此只要有工作者报告受阻，工单在该工作者再次汇报、或 Bot／用户处理之前不会显示为进行中：
  `waiting(blocked, A)` 不会因 A 结束这一轮（`agent_finished`）而回到 `open`，A 先请求审批、再汇报受阻也是如此，另一个 Agent 越过审批同样不会。
- 审批造成的等待只从 `open` 进入；已在等待时保留原因。Bot／用户设回 `open`、关闭或重新打开时清空 `blocked`，所有受阻一并解除。
- 拒绝的转换返回 `BotConflict`：关闭为另一种已关闭状态（需先重新打开）、目标为 `waiting`（返回 `InvalidMessage`）。
  同目标的更新幂等：关闭为当前已关闭状态、或对 `open` 工单设 `open`，返回 `changed:false`，不追加汇报。
  例外：关闭为当前已关闭状态但带有不同的 `outcome` 时保存结论（`closedAt` 不变）并追加 `status` 汇报，返回 `changed:true`，
  例如用户先在客户端关闭，Bot 随后按指令以总结关闭。
- 每次状态变化都追加一条汇报（自动转换为触发它的系统或工作者汇报，Bot／用户为 `kind: "status"`），其 `status` 为新状态。
- 已关闭的工单仍追加系统汇报（历史），但状态不变；工作者对已关闭工单的汇报被拒绝（`BotConflict`）。

### 5. 通知挂接

- `Notification` 新增两个可选字段（缺省时不序列化）：`ticketId`（daemon 从 Agent 的 `ait.bot.ticket` 标签填写）与 `ticketTitle`。
- 接纳时（ADR-271 第 2 节的去重之后、写入日志之前）按接纳它的每个 Bot 处理：工单属于该 Bot 时，`ticketTitle` 写为工单当前标题；
  否则清除两个字段（调用方提供的 `ticketTitle` 一律被覆盖）。属于该 Bot 且类型为 `agent_finished`、`agent_failed`、`agent_permission` 时，
  先向工单追加对应的系统汇报并应用第 4 节的自动转换（写工单文件），再写日志。写工单与写日志之间崩溃时，重放会再次接纳并再追加一次汇报，
  宁可重复也不遗漏。不要求该 Agent 已在指派中：标签由 daemon 写入，调用者不能伪造（ADR-215 第 3 节）。
  工单文件读不出时（I/O 错误或被外部删除）记 `warn` 日志，照常接纳该通知（带索引中的标题），只是不追加系统汇报：接纳从不因工单而失败。写工单失败仍按规则冻结。
- `Notification` 另有可选字段 `turnStartedAt`（缺省时不序列化）：`agent_finished`、`agent_failed` 时由 daemon 从 Agent 记录的 `lastUserMessageAt`
  （最近一条用户消息的时间，`activeTurn.startedAt` 用的同一字段）填写，其他类型与未知时不填。它只在用户、Bot 或定时任务发送输入（以及插话）时前移；
  Provider 自行开始的一轮（例如 Bot 发送 `/goal` 后 Codex 的目标续跑、Claude 未经提示由 `system/init` 开始的主轮次）不改变它，
  所以它是这一轮开始时间的下界，可能早于这一轮，属于更早的一轮。
- **同一轮只唤醒一次**：工作者在一轮中先 `report_progress(done|failed|blocked)`，几秒后结束这一轮，会产生汇报与结束两条通知；
  超出 3 秒合并窗口就是两个唤醒轮次、两次模型调用。本规则只能省掉 Bot 已为同一事件得到的唤醒，有疑问时一律唤醒（`Ticket::spares_turn_end`）：
  `agent_finished` 或 `agent_failed` 带 `turnStartedAt`；取该 Agent 在工单上已记录的最近一次一轮结束（系统汇报 `agent_finished`／`agent_failed`，
  被跳过的结束也会记录）之后的汇报，其中该 Agent 最近的一条工作者汇报须在 `[turnStartedAt, at]`（含两端）内，且为 `blocked`、`done` 或 `failed`
  （结束为 `agent_failed` 时只认 `failed`）。满足时通知照常写日志并追加到工单，但不再唤醒：在唤醒范围内时直接记为 `skipped`，原因 `reported`。
  于是窗口下界实为 `turnStartedAt` 与上一次一轮结束中较晚者，更早一轮的汇报不会掩盖之后由 Provider 自行开始的一轮；
  汇报 `blocked` 后又汇报 `progress` 并继续工作的一轮照常唤醒；汇报 `done` 或 `blocked` 后失败的一轮照常唤醒。
  没有汇报的一轮、`turnStartedAt` 未知、汇报已被 32 条上限淘汰时同样唤醒。`agent_permission` 从不因此跳过。
  仍有的窗口：上一次一轮结束没有记录到工单且下一轮由 Provider 自行开始时，下界退回 `turnStartedAt`，上一轮的汇报仍可能掩盖这一轮的结束。
  一轮结束不被记录的情形有三种：Bot 暂停时；daemon 处理它之前同一 Agent 已有更新的 attention，旧事件按“已被取代”丢弃；
  attention 广播滞后，重扫只看当前 attention。后两种来自代码阅读，没有复现。
- 准入、去重、唤醒范围、合并、预算与路由规则不变。暂停或未配置的 Bot 仍在调用方线程被忽略，不读写工单。
- 新的通知类型 `ticket_blocked`、`ticket_done`、`ticket_failed` 由工作者汇报产生（第 7 节）。ADR-271 第 14 节的观察规则同样适用于这三种类型：
  它们都带 `agentId`，Bot 在完成的轮次中已读到该 Agent 不早于汇报时间的状态时记为 `skipped/observed`。
- 唤醒输入的每行在主体之后增加 ` [ticket <ticketId> "<ticketTitle>"]`（标题按围栏规则转义），轮次的 `wake.notifications` 条目带 `ticketId` 与 `ticketTitle`，
  Bot 与客户端都能看出是哪个工单唤醒了它。

### 6. 工单如何产生：作用域 `create_agent` 的入口

所有入口都是 `bot::service::Bots` 的异步方法，参数类型在 `bot::tickets`；先检查服务未停止（否则 `ServerDraining`），再等待命令容量（不因队列满返回 `ResourceExhausted`）。
`root` 是调用者的根 Workspace（`Caller.root`）；工单必须属于 `root` 的当前 Bot，否则 `BotNotFound`。
除用户更新外，Bot 不为 `active` 时一律 `BotConflict`，不写盘。

```rust
pub async fn open_ticket(&self, request: OpenTicket) -> Result<OpenedTicket, ErrorCode>;
pub async fn assign_ticket(&self, request: AssignTicket) -> Result<(), ErrorCode>;
pub async fn abandon_ticket(&self, root: String, ticket: TicketId) -> Result<(), ErrorCode>;
pub async fn report_progress(&self, report: WorkerReport) -> Result<TicketStatus, ErrorCode>;
pub async fn update_ticket(&self, root: String, ticket: TicketId, update: TicketUpdate) -> Result<Value, ErrorCode>;

// bot::tickets
pub struct OpenTicket { pub root: String, pub draft: TicketDraft }
pub struct OpenedTicket { pub ticket_id: TicketId, pub created: bool } // JSON {"ticketId","created"}
pub struct AssignTicket { pub root: String, pub ticket_id: TicketId, pub agent_id: String, pub workspace_id: String }
pub struct WorkerReport { pub root: String, pub ticket_id: TicketId, pub agent_id: String, pub kind: WorkerReportKind, pub text: String }
pub const OPEN_LIMIT_MESSAGE: &str;

// domain::bot::ticket
pub struct TicketDraft { pub conversation_id: Option<ConversationId>, pub key: String, pub title: String, pub request: String }
// Deserialize（camelCase、拒绝未知字段），可直接解码 bot_ticket_update 的参数（去掉 ticketId 后）
pub struct TicketUpdate { pub status: Option<TicketStatus>, pub outcome: Option<String>, pub title: Option<String>, pub note: Option<String> }
// Deserialize（snake_case），可直接解码 report_progress 的 kind
pub enum WorkerReportKind { Progress, Blocked, Done, Failed }
```

`open_ticket` 的 `conversation_id` 不属于该 Bot 时返回 `InvalidMessage`。

作用域 `create_agent` 的流程（`crates/api/src/mcp/tools/tickets.rs`，在 ADR-215 的作用域改写之后执行，因此被拒绝的调用不会打开工单）：

1. **Bot 会话未提供 `ticketId`**：先 `open_ticket`（`title` 为 Agent 标题，`request` 为初始提示，`conversation_id` 为调用者对话，
   `key` 为该次创建的 `idempotencyKey`；调用未提供时由 MCP 层生成一个 UUID 并同时写入本次 Agent 创建的参数）。同一 Bot 的同一 `key` 返回已有工单（`created:false`），
   标题、请求或对话不同则返回 `IdempotencyConflict`，因此重试的创建不会打开两个工单，Agent 创建也按同一键幂等。
   达到上限返回工具错误 `OPEN_LIMIT_MESSAGE`，此时不创建 Agent。`open_ticket` 返回 `InvalidMessage` 时（会话的对话已不属于该 Bot，
   例如对话关闭后会话仍在回答）以无来源对话重试一次；标题全为空白或键不合法时重试同样失败并返回该错误。
2. **Bot 会话提供 `ticketId`**：先用 `bot.ticket.get.request` 确认工单属于该 Bot、为 `open` 或 `waiting`，且指派少于 `ASSIGNMENT_LIMIT`（16，现为 `pub`）。
   不合法或不属于该 Bot 的 id、已关闭、已满分别在创建前返回引导文本（提示 `bot_tickets`、先以 `bot_ticket_update` 重新打开、改用 `send_agent_prompt`），不创建 Agent。
3. **子树调用者**（属于某工单的 Agent 启动子 Agent）：使用调用者凭据中的工单（见第 10 节；凭据在会话启动时从该 Agent 的 `ait.bot.ticket` 读取）。
   不带工单的子树调用者照旧创建，不写标签。
4. 创建 Agent 时写入 `ait.bot.ticket = <ticketId>`；结果带 `agent.id` 即视为已创建，随后 `assign_ticket`。
   指派失败（Bot 暂停返回 `BotConflict`、并发越过 16 条返回 `ResourceExhausted`、存储错误）只记 `warn` 日志，不使创建失败：Agent 已在运行，标签仍在。
   成功的结果附加 `ticketId`。
5. 结果没有 `agent.id`（RPC 失败，或创建回执带错误而没有 Agent）且工单由本次调用新开（`created:true`）时调用 `abandon_ticket`；
   `created:false` 的工单可能属于仍在进行的同键调用，不删除。`abandon_ticket` 失败只记日志。

作用域 `create_agent`（带作用域凭据的全部调用，包括不涉及工单的）在 MCP 层单独的任务中执行（`api::mcp::bounded`）：
60 秒调用超时只停止等待并向调用者返回 `ResourceExhausted`，打开、创建、指派或删除工单的收尾照常完成，调用被取消时也不会被丢弃。
分离的任务一直持有该调用的 MCP 调用额度（`connections` 许可）直到结束，所以卡住的创建不能在额度之外累积；
它另有 5 分钟的总上限（`DETACHED_TIMEOUT`），超过时被丢弃并记 `warn` 日志。
这个上限不保护 daemon 关闭：分离的任务不随关闭取消，而 Host 的关闭期限是 15 秒（`bins/daemon/src/host.rs`），
关闭时仍卡住的创建会让 `wait_closed` 等到该期限后以“server shutdown exceeded 15 seconds”失败，并跳过其后的定时任务、Bot 与终端关闭步骤。
这来自代码阅读，没有复现（需要创建恰在关闭时卡住超过 15 秒）。
被丢弃时收尾可能做了一半（与进程在该点崩溃相同，见下列窗口）。其他工具仍在超时时取消。

本流程留下的不一致窗口：

- 第 1 步之后、Agent 创建前进程崩溃或 `abandon_ticket` 失败：留下没有指派的 `open` 工单（键是生成的时，Bot 不会以同一键重试）。
  它出现在 `bot_tickets` 与概览中（`agents` 为空），由 Bot 或用户关闭；第 2 节的自动关闭不处理没有指派的工单。
- 调用超时后 Bot 收到 `ResourceExhausted`；键由 MCP 层生成时，Bot 的重试会用新键再开一个工单，而第一次调用仍会完成创建。
- 自动关闭（第 2 节）读完 Agent 目录后、关闭前，Bot 以 `ticketId` 给该工单新加的 Agent：工单仍被关闭，新 Agent 的汇报被拒绝，
  直到 Bot 以 `send_agent_prompt` 给它更多工作（第 7 节）或重新打开。需要并发的同工单创建与达到上限同时发生。
- Agent 创建后、指派前崩溃或指派失败：Agent 带标签而工单没有该指派。系统汇报照常追加（标签可信），概览与列表不显示该 Agent；
  该 Agent 的汇报照常记录，并在工单还有空位时补上指派（第 7 节）。
- 定时任务运行创建的 Agent 不属于工单（本阶段不处理）。

### 7. 工作者汇报

`report_progress`：Agent A 报告其工单 T 的进展。T 必须属于 `root` 的当前 `active` Bot（否则 `BotNotFound`／`BotConflict`），
T 未关闭（否则 `BotConflict`）。不要求 A 在指派中：指派只是记录，授权来自 daemon 写入的标签（作用域 MCP 只以凭据中的工单调用本入口，
系统汇报本就信任标签）。A 未被指派而工单还有空位时，先以 `WorkerReport.workspace_id`（A 自身记录的 Workspace）指派再记录；
已满 16 个时不指派也照常记录，之后有空位的汇报会再试。返回汇报后的工单状态。

- `progress` 只追加；`blocked` 追加并按第 4 节进入 `waiting`；`done`、`failed` 追加（只把同一 Agent 造成的 `waiting` 改回 `open`，不关闭）。
- `blocked`、`done`、`failed` 在写入工单之后经 ADR-271 收件箱提交一条通知：id `report:<ticketId>:<sequence>`，`kind` 为 `ticket_blocked`、`ticket_done`、`ticket_failed`，
  `workspaceId` 为该指派的 Workspace，`scope` 为根 Workspace，`agentId` 为 A，`title` 为工单标题，`summary` 为汇报文本（截到 1024 字节），
  `conversationId` 为工单的来源对话，`ticketId`、`ticketTitle` 为该工单，`at` 为汇报时间。因此 Bot 在工单的对话中被唤醒，合并、预算与路由不变。
  写入工单与写入日志之间崩溃（或日志写入失败而冻结）时汇报已保存：启动时从工单文件为每条 `WAKE_WINDOW`（1 小时，与 ADR-271 的重放去重相同）内的
  工作者 `blocked`、`done`、`failed` 汇报重建同一通知，在载入轮次与对账唤醒之后再次提交；日志已有该 id 或近期唤醒记录（由存储的唤醒轮次重建）带它时丢弃，
  因此恰好唤醒一次。日志已淘汰、又不在近期唤醒记录中的条目（例如 1 小时内超过 128 条已结算通知）可能再唤醒一次。
- `progress` 不产生通知，也不唤醒。

作用域工具 `report_progress`（第 10 节）以凭据中的根 Workspace、Agent 与工单，以及该 Agent 记录的 Workspace 调用本入口，参数中没有工单或 Agent。
工单已关闭或 Bot 暂停（`BotConflict`）、工单不存在（`BotNotFound`）时返回引导文本（汇报未记录，继续完成工作并正常回复）。
成功结果为 `{"ticketId":"…","ticketStatus":"open"|"waiting"|…}`。

**给 Agent 更多工作会重新打开它的工单**：指令要求 Bot 告知结果后关闭工单，同时用 `send_agent_prompt` 继续交代；
若不处理，被复用的 Agent 的汇报会因工单已关闭而被拒绝。Bot 会话的 `send_agent_prompt` 在作用域改写之后、发送之前读取目标 Agent 的 `ait.bot.ticket`，
工单属于该 Bot 且已关闭时经 `Bots::reopen_ticket` 重新打开，追加系统 `status` 汇报说明是 Bot 给该 Agent 更多工作；不依赖模型。
达到上限（`ResourceExhausted`）时先做第 2 节的自动关闭，有关闭的再重试一次；仍被拒绝时工单保持关闭，提示照常发送，
工具结果另带 `ticketNote`：说明该工单未能重新打开、Agent 对这项工作的汇报会被拒绝，并请 Bot 先关闭已结束的工单再以 `status: "open"` 重新打开它。
工单不属于该 Bot 时什么都不做；其他错误只记日志。重新打开发生在发送之前：发送失败（例如 Agent 忙或已归档）时工单仍保持重新打开，
这是可以接受的，Bot 或用户可以关闭它，全部 Agent 归档后自动关闭也会处理。子树调用者与主机令牌的 `send_agent_prompt` 不触发。

### 8. Bot 的更新入口

`update_ticket` 供 Bot 工具 `bot_ticket_update` 使用，`TicketUpdate` 至少有一项：

- `status`：`open`、`done`、`failed`、`cancelled`（第 4 节）；`outcome` 只能与关闭状态同时提供，否则 `InvalidMessage`。
- `title`：改名（截断规则同上），不追加汇报。
- `note`：追加 `source: "bot"`、`kind: "note"` 的汇报。

返回 `{"ticket":<完整投影>,"changed":bool}`。关闭使已关闭工单超过 `CLOSED_LIMIT` 时删除最早关闭的工单文件。
对已关闭工单以相同状态与新的 `outcome` 关闭时保存结论（第 4 节）。

### 9. 客户端 RPC

三个方法加入 `bot::capabilities::METHODS`，以 `workspaceId` 寻址当前非 `retired` Bot，不暴露 `botId` 与 `key`；未配置 Bot 返回 `BotNotFound`。

- `bot.ticket.list.request`：`{"workspaceId":"…","includeClosed":false?,"limit":1..128?}`（默认 50）→ `{"tickets":[摘要…]}`。
  先列未关闭工单（`updatedAt` 倒序），`includeClosed` 时再列已关闭工单（`closedAt` 倒序）。摘要为完整投影去掉 `request` 与 `reports`，加 `latestReport`（最新一条汇报或 `null`）：

  ```json
  {
    "id": "…",
    "title": "修复登录",
    "status": "waiting",
    "waiting": { "reason": "permission", "agentId": "agt_1", "since": "2026-11-13T08:00:00Z" },
    "conversationId": "…",
    "createdAt": "…",
    "updatedAt": "…",
    "closedAt": null,
    "outcome": null,
    "assignments": [{ "agentId": "agt_1", "workspaceId": "wks_…", "assignedAt": "…" }],
    "latestReport": {
      "sequence": 2,
      "at": "…",
      "source": "system",
      "kind": "agent_permission",
      "agentId": "agt_1",
      "text": "",
      "status": "waiting"
    }
  }
  ```

- `bot.ticket.get.request`：`{"workspaceId":"…","ticketId":"…"}` → `{"ticket":完整投影}`，完整投影为摘要的字段（不含 `latestReport`）加 `request` 与 `reports`（升序）。不存在为 `BotNotFound`。
- `bot.ticket.update.request`：`{"workspaceId":"…","ticketId":"…","status":"open"|"done"|"failed"|"cancelled","outcome":"…"?}`
  → `{"ticket":完整投影,"changed":bool}`。用户不能改名或备注；汇报的 `source` 为 `user`。暂停的 Bot 也可更新（用户的显式操作，与 `bot.update` 相同）。

访客：node 范围访客可调用 `bot.ticket.list.request` 与 `bot.ticket.get.request`，不能更新；Workspace／项目访客与 Webhook 自动化都不能调用
（`crates/api/src/guest/policy/node.rs` 的只读白名单；Workspace／项目访客与自动化策略本就拒绝未列出的 `bot.*`）。

### 10. 标签与工具名（工作包 B 实现）

- Agent 标签 `ait.bot.ticket = <ticketId>`（`domain::bot::BOT_TICKET_LABEL`），在保留前缀 `ait.bot.` 下，与 `ait.bot.scope`、`ait.bot.conversation` 并列；
  ADR-215 对该前缀的写入限制不变，调用者不能提供或修改它。
- Bot 专用工具（`ToolAccess::BotOnly`，与 `bot_memory_read` 相同，只对 Bot 自身凭据可见，总是授予，进入 Bot 会话的工具预授权）：
  - `bot_tickets`：参数 `{"includeClosed":bool?}`，结果为 `bot.ticket.list.request` 的结果（默认上限 50）。
  - `bot_ticket_update`：参数 `{"ticketId":"…","status":"open"|"done"|"failed"|"cancelled"?,"outcome"?,"title"?,"note"?}`（`outcome`、`note` 至多 2048 字符，`title` 至多 256 字符），
    调用 `update_ticket`，结果 `{"ticket":完整投影,"changed":bool}`。未知工单、关闭为另一种已关闭状态（`BotConflict`）、空更新或 `outcome` 不配关闭状态（`InvalidMessage`）、
    重新打开超过上限（`ResourceExhausted`）都返回引导文本。
- 工作者工具 `report_progress`：参数 `{"kind":"progress"|"blocked"|"done"|"failed","text":"…"}`（`text` 1–2048 字符），访问类别为新的 `ToolAccess::Worker`：
  `domain::bot::permissions::BotPermissions::allows(tool, Holder)` 以 `Holder::{Bot, Subtree, Worker}` 区分调用者，`Worker` 工具只授予 `Holder::Worker`，
  不进入 `grantable_tools()`（Bot 会话预授权），主机令牌的 `tools/list` 也不列出它。`bot::mcp::CallerKind::Subtree` 新增 `ticket: Option<TicketId>`，
  由 `Scopes::access` 在签发子树凭据时从会话标签 `ait.bot.ticket` 解析（不合法时为 `None`，即普通子树）；标签只有 daemon 能写，所以凭据中的工单与 Agent 记录一致。
  工具说明面向工作者 Agent：受阻需要用户时报 `blocked`、完成报 `done`、放弃报 `failed`、值得记录的里程碑报 `progress`；前三者通知 Bot，汇报不结束本轮。
- **工作者的 `report_progress` 预授权（只在 Claude 上）**：Bot 会话的 `ait` 工具经 `tool_policy` 预授权（`preapproved_ait_tools`）。
  工作者凭据（`McpAccess::Scoped` 的 `worker`：`Scopes::access` 为带合法 `ait.bot.ticket` 标签的 Agent 签发子树凭据时置位）
  在适配器声明追加式预授权时，由会话绑定（ADR-202 的 `Binding::apply`，启动、后续输入与配置更新校验每次都套用）在**有效配置**的 `tool_policy.preapproved`
  中追加且只追加 `{"kind":"mcp","server":"ait","tool":"report_progress"}`（`domain::bot::permissions::worker_tools()`），保留用户已有的预授权；
  持久的 Agent 记录不变。用户的 `allowedTools` 照旧与预授权合并（与 Bot 会话相同）。能力是 `AgentClient::additive_tool_preapproval()`，默认 `false`，
  只有 `provider-claude` 为 `true`：Claude 的预授权是追加的 `--allowedTools mcp__ait__<tool>`。Codex 上同一机制是排他白名单
  （`enabled_tools` 只保留预授权的工具，工作者会失去其他 `ait` 工具），其余 Provider（OpenCode、Pi、OMP、DSH、MiniMax、Antigravity）拒绝任何 `tool_policy`，
  因此它们保持默认，工作者配置与以前完全相同。这不扩大工作者的能力：该工具本就只授予工作者凭据，非工作者会话不预授权，也看不到它。
- **其余情况仍可能请求审批（已知限制）**：Codex 的 `auto`、`auto-review`（审批策略 `on-request`）以 `mcpServer/elicitation/request`
  （`codex_approval_kind: mcp_tool_call`）请求批准非只读的 MCP 工具（`report_progress` 的 `readOnlyHint` 为 `false`），`full-access`、`read-only`（审批策略 `never`）不请求；
  其余 Provider 按各自的模式。需要审批时，`done` 汇报先产生一次 `agent_permission` 唤醒并使工单进入 `waiting(permission)`，用户批准后汇报才记录。
  用户可让 Bot 以不请求审批的模式启动这些工作者。以上 Provider 行为只按代码判断，未用真实 Claude 或 Codex 会话验证。
- 作用域 `create_agent` 对 Bot 会话新增可选参数 `ticketId`（第 6 节第 2 步）。参数只在 Bot 会话看到的输入 schema 中（`api::mcp::catalog::Tool::schema`）；
  子树调用者与主机令牌的 schema 不含该属性，传入时按 schema 校验返回 `-32602`；作用域路径另对子树调用者返回 `Forbidden` 作为第二道防线。
  访客创建 Agent 本无此参数，`ait.bot.` 标签照旧被拒。
- `workspace_overview` 增加 `tickets`（`bot.ticket.list.request` 取未关闭工单，至多 20 条；Bot 已不存在时为 `null`）：

  ```json
  {
    "id": "…",
    "title": "修复登录",
    "status": "waiting",
    "waiting": { "reason": "blocked", "agentId": "…", "since": "…" },
    "thisConversation": true,
    "agents": [
      { "id": "…", "status": "running", "requiresAttention": false },
      { "id": "…", "status": "archived" }
    ],
    "latestReport": {
      "kind": "blocked",
      "source": "agent",
      "agentId": "…",
      "at": "…",
      "text": "…"
    },
    "updatedAt": "…"
  }
  ```

  `thisConversation` 表示工单来自调用的对话；`agents` 的状态从概览已读的 Agent 目录（作用域内最近更新的 200 个未归档 Agent）合并，
  目录读完整而找不到时为 `"archived"`，读不完整时为 `null`（未知）；`latestReport.text` 截到 512 字节，没有汇报时为 `null`。
  计入 24 KiB 预算：超限时与 Agent 的 `lastMessage`、通知的 `summary` 一起先去掉 `latestReport.text`，再参与最长列表优先的截断，并置 `truncated`。

### 11. 生命周期

- 暂停保留工单，不读写；入口（用户更新除外）返回 `BotConflict`，通知按 ADR-271 被忽略。恢复后继续。
- 移除与 Workspace 归档使 Bot 退役时删除 `tickets/<botId>/`；启动时发现退役或不存在的 Bot 的工单目录也会删除。
- 未配置 Bot 的 Workspace 不触及工单存储：`Bots::notify` 在调用方线程直接忽略，入口在找不到当前 Bot 时返回 `BotNotFound`，不写盘。
- 重启时从文件重建索引，工单状态不变；内存中的观察（ADR-271 第 14 节）照旧丢失，最坏重复唤醒一次。
  载入时补发 1 小时内工作者结果汇报的唤醒（第 7 节），已记录或已唤醒的按重复丢弃。

### 12. 指令

内置工作方式改为路由与工作者：Bot 把工作交给 Agent，告诉用户已开始，然后结束本轮；长时间的工作不在 `wait_for_agent` 中等待，
Agent 完成、失败、需要审批或汇报时会唤醒它；预计一分钟内结束的工作可以短暂等待。Bot 在 Agent 的提示中要求它在受阻或完成时调用 `report_progress`；
为同一任务再启动 Agent 时提供已有的 `ticketId`；把结果告诉用户后用 `bot_ticket_update` 关闭工单。
指令另说明：用 `send_agent_prompt` 给 Agent 更多工作会重新打开它的工单，所以只用于继续该工单的任务，新任务需要新的 Agent（与第 7 节的自动重新打开一致）。
权限、记忆、通知与 `NO_REPLY` 的规则不变。

### 13. 降级兼容

- `tickets/` 目录被旧 daemon（0.2.10 及更早）忽略。
- 通知的 `ticketId`、`ticketTitle` 被旧 daemon 忽略（`Notification` 与 `NotificationLog` 不拒绝未知字段）。
- 新的通知类型 `ticket_blocked`、`ticket_done`、`ticket_failed` 会使 0.2.10 无法解析含有它们的通知日志（`notifications/<botId>.json`）
  与携带它们的唤醒轮次，Bot 存储加载失败，daemon 启动报错 `start Bot service`。降级前需删除这些日志与对话，实际上与 ADR-271 第 12 节相同：删除 `<data>/workspace-bots/`。
- 新的跳过原因 `reported`（第 5 节）同样会使 0.2.10 无法解析含有它的通知日志，降级后果与上一条相同。
  通知的 `turnStartedAt` 与工单的 `blocked` 是新增字段，旧 daemon 忽略。
- 本版本的工单记录同样不能容忍未来新增的枚举值（状态、汇报来源与类型、等待原因）；以后新增值时需同样说明降级后果。

## 后果

- Bot 不必在一轮中等待 Agent；交办的工作有持久记录，可以并行、重试与收尾，用户能看到 Bot 正在做的事。
- 每条汇报重写一个工单文件（通常数 KiB）；系统汇报只在工单所属 Bot 接纳通知时发生。
- 未配置与暂停的 Workspace 成本不变；工单不增加模型调用，工作者汇报的唤醒受 ADR-271 的合并与预算约束。
- 一个 Agent 的工单只由标签决定；没有指派的 Agent 的系统汇报与工作者汇报都照常记录，指派只是记录（列表、概览与自动关闭依据它）。
- 未关闭上限在常规使用中能自行恢复，但只限全部 Agent 已归档的工单；仍有未归档 Agent 而工作已结束的工单仍要 Bot 或用户关闭。

## 验证

`crates/domain`、`crates/persistence`、`crates/bot` 中的定向测试覆盖：未配置零成本、同键幂等打开、上限及其错误、每个状态转换与被拒绝的转换、
带工单的通知追加汇报并在 `open` 与 `waiting` 之间往返、工作者汇报唤醒工单对话而 `progress` 不唤醒、对已关闭或他人工单的汇报被拒绝、
唤醒轮次标明工单、带未关闭工单的重启、暂停保留与退役删除，以及当前 `main` 写入的存储仍可加载。
修复轮另覆盖：先审批后受阻、另一 Agent 受阻接管审批等待、多个受阻者依次交接与 Bot 清除、重复关闭保存新结论、系统自动关闭（`done`／`cancelled`／不动）与重新打开、
未指派工作者的汇报与有空位时补指派、工单文件缺失时照常接纳并唤醒、同一轮的结果汇报使结束通知 `skipped/reported` 而更早一轮、开始未知与审批不受影响、
以及“写工单后、写日志前”在每个写入点崩溃都恰好唤醒一次（`notifications/tests/crash.rs`）与超过 1 小时的汇报不再补发；
daemon 从 `lastUserMessageAt` 填写 `turnStartedAt` 有单元测试。按仓库规则只运行受影响 crate 的测试。

`crates/api` 经真实服务的作用域 MCP（`tests/mcp/scoped/tickets.rs`）覆盖：Bot 启动的 Agent 获得工单、标签与指派，同键重试只有一个工单，
Agent 创建失败不留工单，`ticketId` 为同一工单加 Agent、已关闭与他人工单被拒，子 Agent 继承工单、暂停时创建的子 Agent 在首次汇报时补指派，
子树与主机不能传 `ticketId` 或写标签，各类调用者的工具可见性与 `create_agent` schema，`report_progress` 追加汇报且只有 `blocked`、`done`、`failed`
产生带工单对话的通知，概览列出未关闭工单及其 Agent 的实时状态，以及一个端到端循环：Bot 轮次启动 Agent、Agent 汇报 `done`、
发起对话中出现标明该工单的唤醒轮次（执行器为假实现）。概览的上限与截断、访客与 Webhook 自动化规则、`bins/daemon` 从标签填写 `ticketId` 有单元测试。
修复轮另经真实服务覆盖：达到上限时自动关闭全部 Agent 已归档的工单（已汇报 `done` 的为 `done`，其余 `cancelled`），`waiting`、仍有未归档 Agent 与没有指派的工单不动，
无可关闭时返回新的引导文本；`send_agent_prompt` 重新打开已关闭的工单而达到上限时提示照常发送；满 16 个指派的工单上的子 Agent 的汇报照常记录并唤醒。
`api::mcp::bounded` 有单元测试证明分离执行在等待超时后仍完成、未分离的随超时取消。工作者审批行为（第 10 节）只按代码判断，未用真实 Provider 验证。
第二修复轮另覆盖：Provider 自行开始的下一轮、汇报 `blocked` 后 `progress` 的一轮、汇报 `done` 后失败的一轮都照常唤醒而同一轮的结束仍跳过
（`crates/domain`、`crates/bot`）；重新打开之前的 `done` 与只有部分工作者完成都关为 `cancelled`，等待受阻或审批且全部 Agent 已归档的工单关为 `cancelled`
（`crates/domain`、`crates/bot`、`crates/api`）；`send_agent_prompt` 在上限时先自动关闭再重新打开，仍不能时结果带 `ticketNote`（`crates/api`）；
分离执行持有许可直到结束、卡住的创建在自身上限时被丢弃并释放许可（`api::mcp::bounded`）；工作者凭据只在追加式适配器上预授权 `report_progress`、
保留用户的预授权与 `allowedTools`、不改持久记录（`crates/provider/runtime`），经离线 Claude 得到 `--allowedTools=Read,mcp__ait__report_progress`（`crates/provider/claude`），
经离线 Codex 的工作者 `ait` 配置不含 `enabled_tools` 等（`crates/provider/codex`），不具备该能力的 Pi、OpenCode、MiniMax、DSH 照常启动工作者（共享的 `provider::testing::agent_mcp::verify`）。

## Test coverage

未测量：本次未要求覆盖率统计。
