---
id: ADR-280
status: accepted
date: 2026-11-14
amends:
  - bot/adr-215-bot-permissions-and-scoped-mcp.md
  - bot/adr-271-bot-notification-inbox-and-wake-turns.md
  - bot/adr-274-bot-tickets-assignments-and-reports.md
  - bot/adr-213-single-workspace-manager-bot.md
paths:
  - crates/bot
  - crates/domain/src/bot/permissions.rs
  - crates/api/src/mcp
  - crates/provider/runtime/src/service/agent_mcp.rs
  - packages/protocol/src/bot
  - apps/mobile/src/bots
---

# ADR-280：Bot 按 id 回读（Chain of Recall）、内联与溢出指针

- 状态说明：接受。本 ADR 落地 Bot 工具 `bot_read`、工作者工具 `ticket_read`、外部对话之间的读取与 Agent 触达隔离、
  概览与唤醒输入的溢出指针，以及客户端在 Bot 页只读查看外部对话。
- 关系：扩展 [ADR-215](adr-215-bot-permissions-and-scoped-mcp.md) 的工具访问分类（新增一个 `BotOnly` 与一个 `Worker` 工具）；
  修订 [ADR-271](adr-271-bot-notification-inbox-and-wake-turns.md) 第 7 节唤醒输入的每行文本（仍在 [ADR-276](../providers/adr-276-ait-input-envelope-and-origin.md) 的信封内）；
  修订 [ADR-274](adr-274-bot-tickets-assignments-and-reports.md) 第 10 节的概览与 `bot_tickets`、`bot_ticket_update` 对外部对话的结果，
  以及工作者预授权列表（`worker_tools()` 多出 `ticket_read`）；扩展 [ADR-213](adr-213-single-workspace-manager-bot.md) 的
  `bot.conversation.list.request` 投影（两个可选字段）。

## 背景

ADR-274 之后，Bot 能看到工单、通知与 Agent，但只有两种读法：每轮塞进上下文的有界概览（超出预算时直接丢掉文本），
以及 `bot_tickets` 的摘要列表。它读不到一张工单的完整汇报历史、一条通知的全文、自己对话里因 reset 掉出 Provider 上下文的旧轮次，
也读不到别的对话里发生了什么：用户在 Telegram 交办的任务，回到 Ait 询问时 Bot 只知道工单标题。工作者读不到自己的工单，
同一工单上的第二个 Agent 只知道自己的提示词。客户端也只能看内置对话，在 Telegram 里回答的唤醒轮次在 Ait 中不可见。

DeepSeek 公布的 super agent 设计（`dsh-superagent-prototype` 的 `docs/007-architecture.md`，“Read resources”与“Messages delivered to Sessions”）
给出的做法是：路由会话按 id 读取一切资源，页面上出现的每个 id 都能再读（Chain of Recall）；投递给会话的消息只内联有界预览，
被截断的文本带完整长度与读取指针（Inline／Spill）；文本页 4,000 单位、至多 8,000，列表 20 条、至多 50 条，带 `next_offset`；
执行者只能读自己的工单。本 ADR 按 Ait 的记录与权限模型实现同样的读取方式。

## 决策

### 1. 读取入口与常量

所有读取都经 `bot::service::Bots::read(Read) -> Result<Value, ErrorCode>` 进入 Bot worker，在其状态上投影，
**不是**客户端 RPC：读取范围取决于调用者的凭据（下文第 4 节），而客户端 RPC 无法携带它。参数类型在 `bot::recall`：

```rust
pub struct Read { pub root: String, pub reader: Reader, pub resource: String,
                  pub offset: Option<usize>, pub limit: Option<usize>, pub agent: Option<Value> }
pub enum Reader { Conversation(ConversationId), Worker(TicketId) }
pub enum Origin { Conversation(ConversationId),
                  Agent { ticket: Option<TicketId>, conversation: Option<ConversationId> } }
pub enum Viewer { Owner(ConversationId), External(ConversationId), Worker(TicketId) }
pub struct Sight { pub viewer: Viewer, pub tickets: BTreeMap<TicketId, Option<ConversationId>> }
```

`bot::service::Bots::sight(root, Origin) -> Result<Option<Sight>, ErrorCode>` 按调用者的工作来自哪个对话解析它能触达什么（第 4 节）：
Bot 会话是自己的对话；子树中的 Agent 由 `crates/api` 传入它的工单与对话标签，Bot 取工单的来源对话，没有时取标签。
两者都没有时返回 `None`，即不受对话限制。外部对话的 `Sight` 另带 Bot 全部工单的来源对话，
`crates/api` 一次取得后按 Agent 标签判定，不再逐个询问 Bot。

`agent` 只用于 `agent/<id>`：Agent 目录归 `crates/api` 读取（ADR-274 第 2 节的同一划分），由它把 `agent.get.request` 的记录传入，
Bot 侧只做工单、对话与权限的投影。

| 常量（`bot::recall`） | 值   | 说明                                                                |
| --------------------- | ---- | ------------------------------------------------------------------- |
| `TEXT_PAGE`           | 4000 | 文本页默认长度（字符）                                              |
| `TEXT_PAGE_LIMIT`     | 8000 | 文本页最大长度                                                      |
| `LIST_PAGE`           | 20   | 列表页默认条数                                                      |
| `LIST_PAGE_LIMIT`     | 50   | 列表页最大条数                                                      |
| `PREVIEW`             | 400  | 列表项内每个文本字段的预览长度                                      |
| `DETAIL`              | 1200 | 单个对象页（`ticket/<id>`、`turn/<id>` 等）内每个文本字段的预览长度 |
| `WAKE_PREVIEW`        | 600  | 唤醒输入中每条通知摘要的预览长度                                    |

长度按字符（Unicode 标量值）计，而非 DeepSeek 的 UTF-16 码元：Rust 字符串按字符切分不会把一个字符切成两半，
`offset` 也就总能落在合法位置；对 BMP 文本两者相同。`limit` 超过所读资源的上限（列表 50、文本 8000）或为 0 返回 `InvalidMessage`。
JSON 字段沿用 Ait 的 camelCase（`nextOffset`），不用 DeepSeek 的 `next_offset`。

### 2. 页面形状与溢出

- **溢出字段**：被截断的文本字段不再是字符串，而是 `{"preview":"…","chars":<全文字符数>,"read":"<资源>"}`；`read` 用 `bot_read`（工作者为 `ticket_read`）读取全文。
  未截断的字段保持字符串。Agent 目录不带 Agent 的结果文本，概览与 `agent/<id>` 也就不给；Agent 的结果与转录由 `get_agent_activity`
  读取（`wait_for_agent` 的结果另带它刚结束的一轮的最后消息），不在 `bot_read` 中重复。
- **文本页**：`{"resource","text","offset","chars","nextOffset"}`，`nextOffset` 在读完时为 `null`。
- **列表页**：`{"resource","items","nextOffset"}`；对象页内嵌的列表为 `{"items","nextOffset","read"}`，`read` 是继续翻页的资源。
- 页面上的每个 id 都能经某个资源读取；读不到的 id（例如外部对话看到的别人的对话 id）不出现在页面上。

### 3. 资源

| 资源                                             | 内容                                                                                                                                                                                                                   |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tickets`、`tickets/closed`                      | 列表：未关闭工单（`updatedAt` 倒序）或已关闭工单（`closedAt` 倒序）。项为 `{id,title,status,waiting,thisConversation,conversation,updatedAt,closedAt,agents,latestReport,read}`                                        |
| `ticket/<id>`                                    | `request`、`outcome`（`DETAIL` 溢出）、状态与等待原因、`conversation`、`assignments`（每个 Agent 带最近一条汇报，并由 `crates/api` 合并**实时** `status`、`title`、`requiresAttention`、`archived`）、汇报首页（倒序） |
| `ticket/<id>/reports`                            | 汇报列表，最新在前；项为 `{sequence,at,source,kind,agentId,status,text}`                                                                                                                                               |
| `ticket/<id>/report/<n>`、`/request`、`/outcome` | 文本页；汇报页另带该汇报的元数据                                                                                                                                                                                       |
| `conversations`                                  | 列表：本 Bot 的对话，最近活动在前；项为 `{id,channel:{id,kind},label,peer,sender,thread,generation,lastActivityAt,openTickets,thisConversation,read}`                                                                  |
| `conversation/<id>`                              | 对话信息加轮次页（最新在前），每轮 `input` 与 `reply`（最后一条 assistant 消息）为 `PREVIEW` 溢出                                                                                                                      |
| `turn/<id>`                                      | 一个轮次：`input`、每条 assistant 消息（分页，`DETAIL` 溢出）、唤醒通知、状态、投递、`silent`、错误                                                                                                                    |
| `turn/<id>/input`、`turn/<id>/message/<n>`       | 文本页（`n` 从 0 开始）                                                                                                                                                                                                |
| `notifications`                                  | 日志列表，最新在前，与 `bot.notifications.list.request` 同源；`summary` 为 `PREVIEW` 溢出                                                                                                                              |
| `notification/<sequence>`                        | 一条日志条目：完整通知、唤醒状态，以及 `ticket`、`report`（工作者汇报产生的通知）、`turn`（已唤醒时）、`conversation` 指针                                                                                             |
| `agent/<id>`                                     | Agent 的实时状态与标题、所属工单卡片、来源对话、在该工单上的最近汇报；转录用 `get_agent_activity`；只对能触达该 Agent 的读取者（第 4 节）                                                                              |

- 通知以日志序号寻址，而不是通知 id：通知 id 是自由文本（可含 `/`），序号短且在 Bot 内唯一。被日志淘汰的条目读不到（`BotNotFound`）；
  唤醒轮次自己保存了它带的全部通知，`turn/<id>` 仍能读到它们。
- `label` 由 Channel 种类与 `peer`、`sender`、`thread` 拼成（内置对话为 `Ait`）。Ait 不保存外部平台的聊天或发送者显示名，
  因此 label 只能用这些 id；保存显示名需要改 Channel 适配器与对话记录，不在本阶段。
- 读取是纯读取：只读内存索引与工单、轮次文件，不写盘、不唤醒、不推进 ADR-271 的“未看过”游标（`notificationCursor` 只在轮次完成时推进）、
  不记录 ADR-271 第 14 节的观察。退役 Bot 的数据不可读（`current` 不返回退役 Bot，`BotNotFound`）。
- 未配置 Bot 的 Workspace 成本不变：作用域凭据只为有 Bot 的 Workspace 签发，`bot_read` 与 `ticket_read` 只对作用域凭据可见。

### 4. 谁能读什么、能触达哪些 Agent

一个 Bot 的全部对话属于同一 Workspace，但外部对话是不同的人（每个聊天中每个允许的发送者各自一个对话）。读取者在服务端由调用者凭据决定，
不由提示词决定：

- **所有者**：调用者对话所在 Channel 为内置 Channel（`ChannelConfig::Builtin`，即 Workspace 所有者在 Ait 中的对话）。可读本 Bot 的全部对话、轮次、工单与通知，
  可触达作用域内的全部 Agent。
- **外部对话 E**（其他 Channel，包括 Webhook；以及 Bot 已不再拥有的对话）：只能读 E 自己（`conversations` 只列 E）、E 的轮次、
  来源对话为 E 的工单（`conversationId == E`）与通知（`conversationId == E`），只能触达 E 的任务的 Agent（见下文）。
  别的工单只以存根出现（`{id,title,status,waiting,updatedAt,closedAt,restricted:true}`，`waiting` 只有 `reason` 与 `since`，不含让工单等待的 Agent），
  `ticket/<id>` 对它返回同样的存根；别的对话、轮次与报告文本返回 `Forbidden`。别人的通知与不存在的通知一样返回“未保留”：
  通知序号是小的递增整数，区分两者会让 E 数出别人的通知与它们的时间。
- **工作者**（带合法 `ait.bot.ticket` 标签的 Agent，即 ADR-274 的 `Holder::Worker`）：只能读自己的工单，见第 5 节。

**子树中的 Agent 能触达的 Agent**：与它的工作来自的对话相同（`Bots::sight` 的 `Origin::Agent`）。工作来自它的工单的来源对话，
Bot 保存该工单且工单有来源对话时由工单决定；否则来自它自己的 `ait.bot.conversation` 标签；两者都没有（例如 Bot 之外创建、只带作用域标签的 Agent）时不受对话限制。
因此外部对话 E 启动的工作者与它的子 Agent 只能触达 E 的任务的 Agent，所有者对话启动的则触达作用域内的全部 Agent。
它自己与它的子 Agent 带同一工单与同一对话标签，所以总能触达；同一工单上的其他 Agent 由工单触达。

**外部对话能触达的 Agent**（`bot::recall::Viewer::sees_agent`）：

- Agent 的 `ait.bot.ticket` 是 Bot 已知且有来源对话的工单时，由工单决定：来源为 E 才能触达，包括所有者加入 E 的工单的 Agent。
  别人的工单总是优先于 Agent 自己的 `ait.bot.conversation`：这样的 Agent 能用 `ticket_read` 读出整张别人的工单。
- 否则（没有工单标签、工单已不存在，或工单没有来源对话），由 `ait.bot.conversation == E` 决定。
- Bot 没有启动的 Agent 两个标签都没有，包括所有者自己的 Agent，E 不能触达。子 Agent 继承父 Agent 的两个标签，规则对它们同样成立。

同一规则用于每个读出或指挥 Agent 的面，避免一个外部对话借这些工具把另一个人的对话或工作结果带进自己的聊天：

- `bot_read agent/<id>`：不能触达的 Agent 返回引导文本，不给标题、状态、工单与汇报。对外部对话，未知、作用域外或只按前缀与标题解析到的 id
  也返回同一段文本，而不是“未保留”，回答不能证实某个 id 存在；所有者对话对这些 id 仍得到“未保留”。
- 作用域 Agent 工具 `get_agent_status`、`get_agent_activity`、`wait_for_agent`、`send_agent_prompt`、`update_agent`、`set_agent_mode`、
  `cancel_agent`、`archive_agent`、`kill_agent`：不能触达的 Agent 返回同一引导文本，不含标题或 id。对受限的调用者（外部对话，以及工作来自外部对话的子树 Agent），
  解析不到的 id（包括标题与前缀）也返回这段文本，拒绝不能证实某个 Agent 存在。拒绝发生在执行之前，因此不会按 ADR-274 重新打开工单，
  也不会记录 ADR-271 第 14 节的观察；E 对自己 Agent 的调用两者都照常。`list_agents` 与 `list_pending_permissions` 只列能触达的 Agent。
  对受限的调用者，`list_agents` 读目录中最新的至多 200 个 Agent，按标签过滤后保留至多 `limit` 个，不返回 `nextCursor`（为 `null`），
  而以 `truncated` 表示目录还有没读到的 Agent 或能触达的超过 `limit` 个。目录的游标编码它读到的最后一个 Agent 的 id 与更新时间，
  过滤后照常返回会逐页泄露别人的 Agent id。没有改为按标签在源头过滤，是因为规则以工单优先：能触达的 Agent 是“工单来源为 E”或
  “对话标签为 E 且工单不属于别人”，一个标签过滤表达不了，需要按 E 的每张工单各查一次再合并分页；概览已用同样的有界读取。
  不受限的调用者行为不变，`nextCursor` 照常续读。
- `create_agent` 带 `ticketId`：E 不能加入它读不了的工单，返回 `bot_ticket_update` 的同一引导文本。否则新 Agent 的 `ticket_read`
  会读出整张工单，它的 `blocked`、`done`、`failed` 汇报还会唤醒那张工单的对话。
- `workspace_overview`：`botAgents` 只列 E 能触达的 Agent，为此对外部对话读至多 200 个 Bot Agent 后过滤，没读全时 `truncated` 为真；
  `otherAgents`（Bot 没有启动的 Agent）对 E 只给数量，`attention` 为空；别人的工单去掉 `latestReport`，`agents` 只给数量，`waiting` 同存根；
  `notifications` 只保留来源对话为 E 的条目。
- `bot_tickets`：别人的工单为存根（去掉 `conversationId`、`outcome`、`assignments`、`latestReport`，`waiting` 同上，加 `restricted:true`）。
- `bot_ticket_update`：对别人的工单返回引导文本（结果会带完整工单）。

所有者对话不受这些限制。子树中的 Agent 按它的工作来自的对话受同样的限制（见上文）：Agent 工具、两个列表与 `create_agent`
都对它适用。子树调用者不能以 `ticketId` 加入工单，`create_agent` 只把它自己的工单传给子 Agent。作用域只把调用者限制在 Bot 的 Workspace 内，
不把它限制在自己的子树内；不受对话限制的子树调用者能触达作用域内的全部 Agent。
规则的判定在 `bot::recall`（`Viewer::sees`、`Viewer::sees_agent`、`Sight`）与 Bot 侧投影（`State::sight` 的来源解析）中，`crates/api` 只套用它。

**不在本规则内、按设计在对话之间共享**：

- Workspace 记忆：`bot_memory_read`、`bot_memory_update`，以及组合进每个对话指令的记忆。它保存所有者的偏好，每个对话都读得到、也能改，
  因此所有者对话写进记忆的内容会出现在每个外部对话的指令中。
- 定时任务与心跳（`list_schedules`、`inspect_schedule`、`schedule_logs` 等）、终端（`capture_terminal` 等）与 Workspace 脚本。
- 工单存根：每个对话都看得到所有工单的标题、状态与等待原因；Bot 启动的 Agent 的标题通常就是工单标题。

### 5. 工作者工具 `ticket_read`

参数 `{"resource"?: "ticket"|"ticket/reports"|"ticket/report/<n>"|"ticket/request"|"ticket/outcome","offset"?,"limit"?}`，默认 `ticket`。
资源路径不带工单 id，总是凭据中的工单。页面为 `ticket/<id>` 的工作者形态：请求、状态、结论、其他指派及其最近汇报（含实时状态）、汇报历史，
分页与溢出规则相同；不显示工单 id 以外的任何其他工单、对话或通知 id，也不显示 `thisConversation`、`conversation`。
访问类别 `ToolAccess::Worker`，与 `report_progress` 相同：只对工作者凭据可见，Bot 会话、普通子树与主机令牌的 `tools/list` 都不列出。

它是只读工具，因此加入工作者预授权：`worker_tools()` 按访问类别取列表，Claude 工作者的有效配置预授权
`report_progress` 与 `ticket_read`（`--allowedTools=…,mcp__ait__report_progress,mcp__ait__ticket_read`）。其余 Provider 不变（ADR-274 第 10 节）。

### 6. Bot 工具 `bot_read`

参数 `{"resource": string, "offset"?: integer ≥ 0, "limit"?: integer 1..8000}`。访问类别 `ToolAccess::BotOnly`（只对 Bot 自身凭据可见，进入 Bot 会话预授权）。
资源不存在返回引导文本（列出资源名），不可读返回说明它属于另一个对话的引导文本，被淘汰的通知说明已不在日志中。

### 7. 已有内联文本的溢出

- **概览**：`notifications[].summary` 与 `tickets[].latestReport.text`（512 字符）截断时按第 2 节溢出，
  通知条目新增 `read: "notification/<sequence>"`，工单条目新增 `read: "ticket/<id>"`。超出 24 KiB 预算时先把这些文本换成只有 `chars` 与 `read` 的对象
  （不再整段删除），再按最长列表优先删条目；删掉条目时 `more` 给出各列表的完整资源（`tickets`、`notifications`；Agent 列表为工具 `list_agents`）。长度从字节改为字符。
  `botAgents` 的条目只有 `id`、`title`、`provider`、`status`、`requiresAttention` 与 `updatedAt`：Agent 目录不带结果文本，Agent 的结果用 `get_agent_activity` 读取。
- **唤醒输入**：每行末尾附 `(read notification/<sequence>)`；摘要超过 `WAKE_PREVIEW` 时截断并标明全文字符数。仍在 ADR-276 的 `bot_wake` 信封内，转义规则不变。
  轮次的 `wake.notifications` 不变（存完整通知）。

### 8. 对话列表投影与客户端

`bot.conversation.list.request`（以及 `open`、`reset` 返回的对话）新增两个可选字段：`channelKind`（Channel 配置的 `kind`，Channel 已删除时仍给出）
与 `lastActivityAt`（最新轮次的 `createdAt`，没有轮次时为 `null`）。旧客户端忽略未知字段，客户端对缺少它们的旧 Host 退回用 Channel 列表查种类。

Bot 页的对话视图新增对话切换：默认仍是内置对话，也是唯一能输入的对话；选择外部对话时只读显示它的轮次（与内置对话相同的渲染，包括唤醒卡片与静默轮次），
不显示输入框、重置与取消。数据来自已有的 `bot.conversation.list.request` 与 `bot.history.get.request`，访客按现有读取规则（node 访客可读）。

### 9. 降级兼容

不新增存储格式与客户端 RPC。旧 daemon 不认识 `bot_read`、`ticket_read`；新字段只出现在 RPC 结果中。

## 后果

- Bot 可以从概览或唤醒输入中的任何 id 一路读到全文，不必猜；在所有者对话中可以查到外部对话里发生的事；工作者能读自己工单的历史与同伴的汇报。
- 外部对话之间的读取与 Agent 触达由服务端按凭据隔离，覆盖新工具、已有的概览与工单工具，以及作用域 Agent 工具；
  外部对话启动的 Agent 及其子 Agent 受同样的 Agent 触达限制。Workspace 记忆、定时任务、终端与脚本仍在对话之间共享（第 4 节）。
  每次 Bot 会话与子树 Agent 的 Agent 工具调用多一次 Bot worker 往返以取得 `Sight`。
- 受限的调用者用 `list_agents` 每次只读取 200 个 Agent 并过滤，结果不返回 `nextCursor`，只以 `truncated` 表示可能还有更多；
  它仍可传入自己构造的 `cursor` 越过最新的 200 个，过滤照常进行，不会泄露他人的 Agent。Workspace 中 Agent 很多时，
  它较早的 Agent 也可按 id 经 `get_agent_status` 读取（id 在它的工单与概览中）。
- 每次读取是一条 worker 命令，读工单与轮次各一次文件读取；概览不变的部分成本不变，只多出指针字段。
- 客户端可以只读查看 Telegram 等外部对话。

## 验证

`crates/bot` 的定向测试覆盖每个资源、分页与 `nextOffset`、溢出指针读回全文、所有者读外部对话、外部对话被拒读另一外部对话、
外部对话看到别人工单的存根（不含等待的 Agent）、别人的通知读作未保留、`agent/<id>` 的触达规则（工单优先于标签、Bot 未启动的 Agent）、
`Viewer::sees_agent` 的各分支、子树 Agent 的来源解析（工单优先于标签、两者都没有时不受限制）、外部对话读未知 `agent/<id>` 与读别人的 Agent 得到同一回答、
工作者只读自己的工单、读取不写盘也不移动未看过游标、退役 Bot 不可读，以及唤醒输入的指针。
`crates/api` 经真实服务的作用域 MCP 覆盖端到端读取、实时状态合并、各类调用者的工具可见性（Bot 会话、工作者、无工单子树、主机令牌）、
非所有者对话被拒读所有者对话与概览的过滤、被拒加入别人的工单、被拒以全部九个 Agent 工具触达所有者的 Agent（按 id、前缀与未知 id）、
两个列表只列自己的 Agent 且 `list_agents` 不返回游标、外部对话的工作者同样被拒触达所有者的 Agent 而能触达自己与自己的子 Agent、
所有者对话的工作者仍触达全部 Agent、`bot_read agent/<id>` 对未知与作用域外的 id 返回同一拒绝、被拒的提示不重新打开工单而对自己 Agent 的提示照常重新打开；概览的投影测试覆盖 `botAgents`、
`otherAgents` 与别人工单的 Agent 数量；`crates/domain` 覆盖访问类别；`crates/provider/*` 覆盖工作者预授权列表；
`packages/protocol` 与 `apps/mobile/src/bots` 覆盖对话投影解析与只读对话视图。按仓库规则只运行受影响 crate 与包的测试。

## Test coverage

未测量：本次未要求覆盖率统计。
