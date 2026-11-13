# Workspace Bot 现行规格

本页列出 Workspace Bot 当前生效的规则，范围为 `crates/bot`、`crates/persistence` 中的 Bot 存储、
Bot 对 `crates/provider` 的使用、`crates/api` 与 `bins/daemon` 的作用域 MCP 凭据，以及构成契约的客户端标签页。
决策历史与取舍见 ADR（[索引](../decisions/README.md)），使用方式见 [Bot 手册](../operations/bots.md)。

## 身份与生命周期

- `bot` 的生产依赖只有 `domain`、`model`、`provider`，`persistence` 仅用于测试；Provider 不依赖 Bot。
  daemon 注入文件存储、Workspace registry 与凭据解析
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)）。
- 每个 Workspace 至多一个非 `retired` Bot，目录校验拒绝第二个；未设置时不产生记录、隐藏会话、轮询或模型调用。
  客户端 JSON 以 `workspaceId` 寻址，投影不暴露内部 Bot 身份
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)，取代
  [ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md) 的多 Bot 与 `botId`）。
- `bot.setup` 在执行配置与权限都相同时幂等返回 `created:false`，否则对已有非 `retired` Bot 返回 `BotConflict`；
  `clearMemory:true` 只在新 Bot 落盘后生效
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 状态为 `active`、`paused`（`user` 或 `directory_missing`）、`retired`。暂停取消执行中与排队的轮次并释放原生会话；
  恢复要求 Workspace 未归档且目录存在，目录缺失返回 `WorkspaceNotFound`。轮次就绪时发现目录缺失，
  Bot 转为 `paused(directory_missing)`，需显式恢复
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)）。
- 只有 `active` Bot 能打开或重置对话、接纳输入、更新记忆、创建或修改 Channel，否则返回 `BotConflict`；
  `bot.update`、权限更新与 Channel 删除在暂停时仍可用
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、
  [ADR-214](../decisions/clients/adr-214-workspace-bot-tab-v2.md)）。
- `bot.remove` 与 Workspace 归档都使 Bot 进入 `retired`：所有外部 Channel 停用并软删除，隐藏会话关闭，内置 Channel 保留。
  移除保留记忆；归档删除该 Workspace 的记忆，并从所有 Bot 的 `workspaces` 记录中移除它
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 退役 Bot 的对话与轮次留在磁盘，但所有 `bot.*` 读取只解析当前非 `retired` Bot；重新设置得到新 Bot 与空历史。
  旧 `<data>/bots/` 不迁移，daemon 启动时关闭其中记录的隐藏会话后删除该目录
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)）。

## 对话与执行

- Conversation 以 Channel、peer、sender、thread 区分，内置 Channel 使用固定 `owner` 对话；同一 Conversation 串行执行。
  Conversation 创建时复制 Bot 执行配置，`bot.update` 只对新 Conversation 或 reset 后的新 generation 生效
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)、
  [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)）。
- reset 要求没有执行中的轮次（否则 `BotConflict`），取消排队输入、关闭旧隐藏会话并递增 generation；历史保留
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)、[Bot 手册](../operations/bots.md)）。
- 输入按 Channel 与 `messageId` 去重：内容、目标相同返回 `duplicate:true`，不同返回 `IdempotencyConflict`。
  轮次落盘后才返回接纳；外部消息在持久接纳后才确认或推进游标，全局积压时不确认、等待重投，
  单个 Conversation 积压时确认并丢弃该条
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)）。
- daemon 重启时，`sending`、`running`、`needs_approval` 轮次改为 `uncertain`，正在发送的投递也改为 `uncertain`；
  执行结果无法确认时同样记为 `uncertain`。`uncertain` 阻止同 generation 的后续输入，需显式 reset，不自动重做或重发
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)、[Bot 手册](../operations/bots.md)）。
- 每轮正常结束后释放（`Release`）原生会话但保留其身份，下一轮重新打开；reset 与退役改为关闭（`Close`）并归档会话
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)、
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 输出只来自 Provider timeline 中输入游标之后的 `assistant_message`：同一 `messageId` 的片段合并为一条，
  无身份的记录用 timeline 序号作 ID。`Turn.messages` 保存每条消息，`reply` 是最新一条的兼容字段；
  不支持游标的执行器回退到 `lastMessage`
  （[ADR-227](../decisions/bot/adr-227-bot-multi-message-output.md)）。
- 内置 Channel 的轮次完成即为已投递；外部 Channel 按 `messages` 逐条发送（旧记录回退到 `reply`），
  发送失败或中断记为投递 `uncertain`，不重试
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)、
  [ADR-227](../decisions/bot/adr-227-bot-multi-message-output.md)）。

## 通知与唤醒

- daemon 以 `bot::service::Bots::notify` 提交 Workspace 通知（`domain::bot::notification::Notification`），持久化后返回
  `Admitted`、`Duplicate` 或 `Ignored`，从不等待模型。`workspaceId` 与 `scope` 都没有 `active` 的当前 Bot 时直接 `Ignored`：
  不进入 Bot worker、不写盘、不建会话
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md)）。
- 两个根 Workspace 的当前 `active` Bot 各自接纳，按通知 `id` 去重：日志中已有，或 1 小时内的唤醒轮次携带过（启动时从轮次记录重建），
  因此 15 分钟重放不会因日志淘汰而再次唤醒。全部进入有界日志
  （128 条已结算 + 至多 64 条待唤醒）；`scope` 为本 Bot 根 Workspace，或 `wakeScope` 为 `all` 且 `workspaceId` 为根 Workspace 时进入收件箱。
  `wakeScope` 默认 `own`，由 `bot.setup`／`bot.update` 设置、`bot.get` 返回
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md)）。
- 目标对话为仍属于当前 Bot、Channel 启用未删除、非 Webhook、当前 generation 无 `uncertain` 的来源对话，否则为内置 `owner` 对话（按需创建）。
  同一目标的待唤醒通知静默 3 秒或最早一条等满 30 秒后合并为一个唤醒轮次（至多 16 条），目标有执行中或排队轮次时等待；
  全局积压满时留在收件箱重试。每个对话每小时至多 12 个唤醒轮次，超出记为 `skipped/over_budget`。
  解析出的目标（含 `owner`）当前 generation 有 `uncertain` 轮次时，到期条目记为 `skipped/blocked`，不再占用收件箱
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 5 节）。
- Bot 会话经作用域 MCP 用 `wait_for_agent`、`get_agent_status` 或 `get_agent_activity` 读到 Agent 已不在执行时，`crates/api` 以
  `Bots::observe` 记录该对话看到的 Agent 最后变更时间（快照中更晚的 `attentionTimestamp` 优先，以覆盖审批请求）。观察挂在发起读取的轮次上
  （工具调用开始早于该对话当前轮次开始执行的读取不记录，例如已取消轮次迟返的 `wait_for_agent`），
  该轮次 `completed` 后才生效，以其他方式结束则丢弃；生效后关于该 Agent、目标为该对话且不晚于该时间的 `agent_*` 待唤醒条目记为
  `skipped/observed`。观察只在内存中保留，有界
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 14 节）。
- 唤醒轮次带 `wake.notifications`，`messageId` 为 `notification:<turnId>`，无 `replyTo`，输入是 `<ait-input kind="bot_wake">` 围栏；
  Bot 指令前缀的围栏同样是 `<ait-input kind="bot_instructions">`，时间线投影拆出来源与用户正文
  （[ADR-276](../decisions/providers/adr-276-ait-input-envelope-and-origin.md)）。
  先提交轮次再标记日志，启动时按仍排队的唤醒轮次携带的通知 id 补标记，因此重启不丢失也不重复唤醒。用户取消或 reset 的唤醒轮次不再重新唤醒；
  服务停止时被取消（或因此 `uncertain`）以及启动时发现仍在执行的唤醒轮次，其通知回到收件箱重试一次，被打断的轮次计入预算；
  两种情况都先保存改回 `pending` 的日志，再写入轮次的新状态，因此其间崩溃也只重试一次
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 11 节）。
- 来源对话由保留标签 `ait.bot.conversation`（`domain::bot::BOT_CONVERSATION_LABEL`）与定时任务字段 `botConversation` 记录。
  Bot 会话的作用域凭据携带其对话 id；作用域 `create_agent` 为 Bot 调用者写入该对话，子树调用者复制自身 Agent 的标签；
  作用域 `create_schedule`／`create_heartbeat` 以同样来源写入 `botConversation`，运行创建的 Agent 也带该标签。
  调用者不能自行提供或修改二者（[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 13 节）。
- 事件源：daemon 订阅 `SessionEvents::subscribe_committed_attention()`（焦点与中心投递之前的全部 attention），
  按 Agent 当前记录生成 `agent:<agentId>:<timestamp>` 通知，跳过内部会话、已归档、被更新 attention 取代、无 Workspace，
  以及定时任务运行创建的 Agent 的全部 attention（含等待审批）；终端 attention 生成 `terminal:<terminalId>:<毫秒>`，只在 `wakeScope: all` 时唤醒。
  每次定时任务运行结算（含创建 Agent 前的失败与重启中断）经 `schedule::ports::Runner::settled` 生成一条 `schedule:<scheduleId>:<runId>` 通知，
  心跳（已有 Agent 目标）的成功运行除外；通知经 `Bots::offer` 提交
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 13 节）。
- 重放：启动与广播滞后时只重放当前仍需 attention、时间戳在 15 分钟内的 Agent，由 Bot 侧按 id 去重；更早的 attention 不再唤醒。
  定时任务 worker 启动时把 15 分钟内结束的运行再报告一次，`settled` 因此是“至少一次”
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 13 节）。
- 关闭：`Bots::stop` 只停止 Channel 与执行中的轮次，worker 继续把通知写入日志（不唤醒）直到 `Bots::shutdown`；`shutdown` 先等待已提交的
  `Bots::offer`。`Api::wait_closed` 先关闭定时任务再关闭 Bot，关闭时结算的运行结果因此在重启后唤醒
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 13 节）。
- `workspace_overview` 的 `notifications` 是调用会话尚未在完成轮次中看到的通知（`bot.notifications.list` 的 `unseenBy`），
  至多 20 条、`summary` 至多 512 字节，计入 24 KiB 预算；对话已不属于 Bot 时为 `null`
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 13 节）。
- 唤醒轮次的最后一条回复恰为 `NO_REPLY` 时记为 `silent` 并直接 `delivered`，不向任何 Channel 发送；外部 Channel 对唤醒轮次发普通消息
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md)）。
- 轮次进入 `sending` 时记录 `notificationCursor`；`bot.notifications.list` 带 `unseenBy` 时只返回该对话最近完成轮次之后的通知，
  失败的轮次不推进。暂停把待唤醒通知记为 `skipped/paused`；移除与归档删除该 Bot 的通知状态
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md)）。

## 工单、指派与汇报

- 工单（`domain::bot::ticket::Ticket`，id 为 `TicketId`）是 Bot 交办的一项任务：标题、请求、来源对话、状态、指派（至多 16 个 Agent，
  只记 Agent 与 Workspace，实时状态从 Agent 目录读取）与最近 32 条汇报。每个 Bot 至多 32 个未关闭工单，已关闭的保留 64 个，超出删除最早关闭的。
  作用域 `create_agent` 打开工单达到上限时，先关闭全部 Agent 已归档（按 `ait.bot.ticket` 标签完整读取未归档 Agent 目录为空）且有指派的 `open` 或 `waiting` 工单
  （`Bots::close_finished_ticket`，系统来源：`open` 工单只在最近一次重新打开之后，每个被指派的 Agent 与其间汇报过的工作者最近一条都是 `done` 时为 `done`，
  否则 `cancelled`；`waiting` 工单无人能再回答，一律 `cancelled`；结论说明自动关闭的原因），再重试一次；读取失败或不完整时不关闭。
  仍无空位时返回 `ResourceExhausted`（工具错误 `bot::tickets::OPEN_LIMIT_MESSAGE`，说明应关闭哪类工单）；
  Bot 与用户的重新打开到上限不做自动关闭（[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 1、2 节）。
- 状态为 `open`、`waiting`、`done`、`failed`、`cancelled`。Agent 等待审批使 `open` 进入 `waiting(permission)`，由该 Agent 完成或失败一轮结束。
  工单记下每个最近一次汇报 `blocked` 的工作者（内部字段 `blocked`）：受阻优先于审批（同一或另一 Agent 的审批等待都改为受阻），
  已因他人受阻而等待时只记下；受阻者再汇报其他种类时解除，正等它时交给最早的其他受阻者，没有则回到 `open`。因此报告过受阻的工单在该工作者再汇报、
  或 Bot／用户处理之前不显示为进行中。Bot 与用户能关闭、重新打开或结束等待（清除全部受阻）；Ait 只在上限自动关闭与 `send_agent_prompt` 时以系统来源关闭或重新打开。
  关闭为另一种已关闭状态返回 `BotConflict`，重复同一目标为 `changed:false`，但以相同状态带新结论关闭时保存结论
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 4、8 节）。
- 作用域 MCP 经 `Bots::open_ticket`（按 Agent 创建的幂等键幂等）、`assign_ticket`、`abandon_ticket` 让 Bot 启动的每个 Agent 属于一个工单，
  Agent 带保留标签 `ait.bot.ticket`（`domain::bot::BOT_TICKET_LABEL`）。带标签的 Agent 经 `Bots::report_progress` 汇报，不要求已被指派：
  未被指派而工单有空位时先指派，满 16 个时照常记录；对已关闭工单返回 `BotConflict`。Bot 经 `Bots::update_ticket` 关闭、重新打开、改名或备注
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 6–8 节）。
- 作用域 `create_agent`（`api::mcp::tools::tickets`）：Bot 会话不带 `ticketId` 时，先以该次创建的 `idempotencyKey`（缺省时生成并传给创建）
  打开工单，再以 `ait.bot.ticket` 创建 Agent，最后指派；没有创建出 Agent 时删除本次新开的工单。会话的对话已不属于该 Bot 时，工单不记来源对话。
  带 `ticketId` 时先确认工单属于该 Bot、未关闭且指派未满 16 个，否则在创建前以引导文本拒绝。带工单标签的子树调用者把标签传给子 Agent 并指派，
  指派失败只记日志，不使创建失败。结果带 `ticketId`。`ticketId` 只出现在 Bot 会话看到的 `create_agent` 输入 schema 中，
  其他调用者传入时 `-32602`；定时任务运行创建的 Agent 没有工单。作用域 `create_agent` 在单独的任务中执行（`api::mcp::bounded`），
  60 秒调用超时只停止等待，工单收尾照常完成；该任务持有调用额度直到结束，总时长至多 5 分钟
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 6 节）。
- Bot 会话的 `send_agent_prompt` 在发送前读取目标 Agent 的 `ait.bot.ticket`，该 Bot 的工单已关闭时经 `Bots::reopen_ticket` 重新打开并追加系统汇报；
  达到上限时先做上述自动关闭再重试，仍不能时提示照常发送，结果带 `ticketNote` 告诉 Bot 工单未重新打开；发送失败时工单保持重新打开
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 7 节）。
- `bot_tickets`、`bot_ticket_update` 只对 Bot 会话可见；`report_progress` 属于新的访问类别 `ToolAccess::Worker`，
  只对带合法 `ait.bot.ticket` 标签的子树凭据可见（凭据签发时从会话标签读取，`bot::mcp::CallerKind::Subtree.ticket`），
  工单与汇报者都取自凭据。标签在、指派不在（指派失败、工单已满或进程在指派前退出）时汇报照常记录，有空位时补上指派。
  签发这类凭据时 `McpAccess::Scoped.worker` 置位；适配器声明 `AgentClient::additive_tool_preapproval()`（只有 Claude）时，会话绑定在有效配置中
  追加 `report_progress` 的预授权（与用户的预授权和 `allowedTools` 合并，持久记录不变），所以 Claude 工作者不再就它请求审批。
  Codex 的预授权是排他白名单、其他 Provider 拒绝 `tool_policy`，它们保持默认：Codex 在 `auto`、`auto-review` 中与其他 Provider 在会请求审批的模式中
  仍可能先就 `report_progress` 请求审批；未用真实 Provider 验证（[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 7、10 节）。
- `workspace_overview.tickets`：至多 20 个未关闭工单，每项 `{id,title,status,waiting,thisConversation,agents,latestReport,updatedAt,read}`；
  `agents` 的状态从概览已读的 Agent 目录合并，目录读完整而找不到时为 `archived`，不完整时为 `null`；最新汇报文本超过 512 字符时溢出为读取指针，
  超出 24 KiB 时与其他文本一起先缩为只有长度与指针，再参与列表截断（[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 10 节，[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 7 节）。
- 通知带 `ticketId` 时，只有拥有该工单的 Bot 保留它并填入 `ticketTitle`，其余 Bot 的副本清除两者；`agent_*` 通知先向工单追加系统汇报再写日志，
  工单文件读不出时记日志并照常接纳（不追加汇报）。
  `blocked`、`done`、`failed` 汇报产生 `ticket_blocked`、`ticket_done`、`ticket_failed` 通知，在工单的对话中唤醒 Bot，`progress` 不唤醒；
  三者同样适用观察规则。唤醒输入每行带 `[ticket <id> "<标题>"]`。
- daemon 为 `agent_finished`、`agent_failed` 填写 `turnStartedAt`（Agent 的 `lastUserMessageAt`，只随发送的输入前移，Provider 自行开始的一轮不更新，
  所以只是这一轮开始的下界）。该 Agent 在工单上已记录的上一次一轮结束之后、且在 `[turnStartedAt, at]` 内的最近一条工作者汇报为 `blocked`、`done` 或 `failed`
  （失败的一轮只认 `failed`）时，结束通知照常记录到日志与工单，但记为 `skipped/reported`，不再唤醒（`Ticket::spares_turn_end`）；
  之后改报 `progress` 的一轮、更早一轮的汇报、开始时间未知与审批通知都照常唤醒（[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 5 节）。
- 汇报写入工单后、写日志前崩溃时，启动时从工单为 1 小时内的工作者结果汇报重建通知并再次提交，日志与近期唤醒记录去重，恰好唤醒一次
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 5、7、11 节）。
- 客户端以 `bot.ticket.list.request`（默认只列未关闭工单，摘要带 `latestReport`）、`bot.ticket.get.request`（含请求与汇报）、
  `bot.ticket.update.request`（用户关闭、取消或重新打开，暂停时也可用）读写，投影不含 `botId` 与幂等键
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 9 节）。
- Bot 不为 `active` 时，除用户更新外的入口返回 `BotConflict` 且不写盘；暂停保留工单，移除与归档删除
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 11 节）。
- 内置指令要求 Bot 交出工作后结束本轮、只短暂等待预计一分钟内结束的工作、在 Agent 提示中要求其调用 `report_progress`、
  同一任务的新 Agent 提供 `ticketId`、回报结果后关闭工单，并说明 `send_agent_prompt` 会重新打开 Agent 的工单、新任务需要新 Agent
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 12 节）。

## 按 id 回读与对话隔离

- Bot 工具 `bot_read`（`ToolAccess::BotOnly`）与工作者工具 `ticket_read`（`ToolAccess::Worker`，与 `report_progress` 一起进入 Claude 工作者的预授权）
  都经 `bot::service::Bots::read(bot::recall::Read)` 在 Bot worker 中投影，不是客户端 RPC：读取范围由调用者凭据决定。
  `agent/<id>` 的 Agent 记录与工单指派的实时状态由 `crates/api` 从 Agent 目录读取并合并，Bot 侧只投影工单、对话、轮次与通知（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 1、5、6 节）。
- 资源：`tickets`、`tickets/closed`、`ticket/<id>`（及 `/reports`、`/report/<n>`、`/request`、`/outcome`）、`conversations`、`conversation/<id>`、
  `turn/<id>`（及 `/input`、`/message/<n>`）、`notifications`、`notification/<序号>`、`agent/<id>`；工作者的资源不带工单 id（`ticket`、`ticket/reports` 等）。
  页面上的每个 id 都能再读，通知以日志序号寻址，自由文本的通知 id 不出现在页面上（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 3 节）。
- 常量（`bot::recall`）：文本页 `TEXT_PAGE = 4000`、至多 `TEXT_PAGE_LIMIT = 8000` 字符；列表 `LIST_PAGE = 20`、至多 `LIST_PAGE_LIMIT = 50` 条，以 `nextOffset` 续读；
  列表项内文本预览 400 字符、单个对象页内 1200 字符、唤醒输入摘要 600 字符。长度按字符计。截断的文本为 `{"preview","chars","read"}`；
  Agent 目录不带 Agent 的结果文本，Agent 的结果与转录用 `get_agent_activity` 读取（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 1、2 节）。
- 读取是纯读取：不写盘、不唤醒、不记录观察、不推进 `notificationCursor`；退役 Bot 不可读（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 3 节）。
- 读取者：调用者对话在内置 Channel 上时为所有者，可读全部、可触达作用域内全部 Agent；其他对话（含 Webhook 与 Bot 已不拥有的对话）只读自己、来源为自己的工单与通知，
  别人的工单只给存根（标题、状态、不含 Agent 的等待原因、`restricted:true`），别人的通知读作未保留，其余返回 `Forbidden`；工作者只读自己的工单，页面不含其他工单、对话或通知 id
  （[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 4 节）。
- Agent 触达：`bot::recall::Viewer::sees_agent` 按标签判定，`ait.bot.ticket` 是有来源对话的已知工单时由工单来源决定，否则由 `ait.bot.conversation` 决定，
  Bot 未启动的 Agent 对外部对话不可触达。`Bots::sight(root, Origin)` 为外部对话返回 `Sight`（读取者加 Bot 全部工单的来源）；子树调用者的
  `Origin::Agent` 带它的工单与对话标签，Bot 取工单的来源对话，没有时取标签，两者都没有时返回 `None`（不受限）。`crates/api` 一次取得后套用于
  `bot_read agent/<id>`、`scope::confine` 的 Agent 目标（九个 Agent 工具，拒绝在执行前，不触发重新打开工单与观察）、`list_agents`、`list_pending_permissions`、
  `create_agent` 的 `ticketId`、`workspace_overview` 的 `botAgents`、`otherAgents` 与别人工单的 `agents`（只给数量）。受限的调用者的 `list_agents`
  读最新 200 个 Agent 后过滤，不返回目录游标（它编码最后读到的 Agent 的 id），以 `truncated` 表示没有列全。所有者对话及其启动的 Agent 不受限制。
  Workspace 记忆、定时任务、终端与脚本按设计在对话之间共享（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 4 节）。
- 唤醒输入每行以 `(read notification/<序号>)` 结尾，摘要超过 600 字符时截断并标明全文长度，仍在 `bot_wake` 信封内；内置指令要求 Bot 按指针读取、
  跟进工单前先读它，并在所有者对话中查询其他对话（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 7 节）。

## 原生权限与会话配置

- Provider 的 AgentManager 按持久化的 `internal` 与 `owner` 识别 Bot 会话，在发布前向原生 Provider 回复
  `{"behavior":"deny"}`，拒绝失败则本轮失败；对 Bot 会话只接受精确的拒绝响应。Bot 层也拒绝快照中残留的请求，
  失败时取消并标记失败。`bot.permission.resolve` 已删除
  （[ADR-145](../decisions/bot/adr-145-bot-permission-denial.md) 第 1、2 条；第 3 条由
  [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md) 取代）。
- `execution.modeId` 在设置、更新与加载时归一化为 `null`。Bot 会话使用 Provider 声明的只读模式（Codex `read-only`、
  Claude `default`），没有则用默认模式；Codex、Claude 写入系统提示并预授权全部可授予的 `ait` 工具，
  其他 Provider 把指令前置到每一条输入、不预授权
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 第 6 节，取代
  [ADR-145](../decisions/bot/adr-145-bot-permission-denial.md) 第 4 条的模式改写）。
- 已有会话无法收敛到上述配置时 Provider 返回 `resetRequired`，Bot 轮换一次 generation 后重试，仍不行则本轮失败
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 内置指令按已授予的分组生成；记忆放在 `<workspace-memory>` 围栏内并声明为用户数据，其中每个 `</` 转义为 `<\/`
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 第 7 节）。

## 工具分组与模式上限

- 工具访问由 `domain::bot::permissions::TOOLS` 唯一定义：`observe` 总是授予，新 Bot 默认
  `{observe, agents, stop, schedules}`；`create_workspace`、`respond_to_permission`、`archive_workspace`、
  `rename_workspace`、`list_workspaces` 及未登记的工具名永不授予。`workspaces` 分组仍可解析，但不授予工具且保存时移除
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)、
  [ADR-222](../decisions/bot/adr-222-single-agent-mode-no-bot-workspaces.md)）。
- `bot_memory_read`、`bot_memory_update`、`workspace_overview`、`bot_tickets`、`bot_ticket_update`、`bot_read` 只对 Bot 自身凭据可见，
  子树与主机令牌都看不到；`report_progress`、`ticket_read`（`ToolAccess::Worker`）只对带工单标签的子树凭据可见，不进入 Bot 会话的工具预授权
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)、[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md)、[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md)）。
- `modes` 线上形状仍为 `{ [provider]: string[] }`，保存时截断为第一项，执行时也只放行第一项；未列出的 Provider
  不可使用。`create_agent` 未指定模式时取该项；`set_agent_mode` 与 `update_agent` 按目标 Agent 当前 Provider 校验，
  `update_agent` 的 features 只允许 `fast_mode`
  （[ADR-222](../decisions/bot/adr-222-single-agent-mode-no-bot-workspaces.md)，修订
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。

## 作用域 MCP 凭据

- 凭据由 `bot::mcp::Scopes` 在 daemon 内存签发：64 个十六进制字符，只保存 SHA-256 摘要，容量满时淘汰最早一项，
  每次启动重新生成。同一 Agent 再次打开会话时先撤销旧凭据
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- Provider 绑定为 `Shared`、`Scoped` 或 `Strip`。被拒绝的会话和无法交付作用域凭据的会话一律 `Strip` 并撤销刚签发的凭据，
  不回退到共享令牌；Bot 自身会话不受 `injectIntoAgents` 控制，子树会话受其控制
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)，修订
  [ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)）。
- 授权在每次 MCP 请求时按根 Workspace 当前非 `retired` Bot 读取。Bot 凭据只在其 Bot 仍是当前 Bot 且为 `active` 时有效；
  暂停只使 Bot 自身凭据失效，子树保留完整授权；没有非 `retired` Bot 时子树降为只读观察，作用域只含根 Workspace
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 未授权工具不出现在 `tools/list`，调用返回 `-32602`。作用域凭据的 `X-Ait-Agent-Id` 必须是其自身身份；
  Bot 会话没有调用者上下文，子树以自身为上下文。共享令牌经 `X-Ait-Agent-Id` 指向带 `ait.bot.scope` 的 Agent 时返回 `-32602`
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)，修订
  [ADR-148](../decisions/daemon/adr-148-daemon-mcp.md)）。
- 作用域调用的每个工具必须在 `api::mcp::scope` 登记规则才会执行；`cwd` 一律拒绝，目标解析为规范 ID 后回写。
  作用域为根 Workspace 加上 `Bot.workspaces` 中已记录且未归档的 worktree；违规以 `isError` 工具结果返回 `Forbidden`
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)、
  [ADR-222](../decisions/bot/adr-222-single-agent-mode-no-bot-workspaces.md)）。
- 作用域调用者创建的 Agent 被写入 `ait.bot.scope = <根 Workspace>`，其会话获得子树凭据。MCP `create_agent`、`update_agent` 与访客创建
  Agent 都不能写入 `ait.bot.` 前缀的标签，作用域调用者不能写入任何 `ait.` 前缀的标签
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 第 3 节）。
- 作用域调用者创建的定时任务与心跳记录 `botScope`。每次运行前按当前授权复核 `schedules` 分组与模式，不满足时以
  `REVOKED` 失败、任务保持启用；`update_schedule` 只接受带 `botScope` 的任务，运行与恢复要求任务当前模式在上限内
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 第 5 节、
  [ADR-222](../decisions/bot/adr-222-single-agent-mode-no-bot-workspaces.md)）。

## Channel 与凭据

- 每个 Bot 恰有一个内置 Channel，不能创建、修改或删除；外部 Channel 删除为软删除。同一外部账号（Telegram token 引用、
  飞书、Lark 或 QQ App ID、微信账号、Photon 项目）在同一 daemon 内只能属于一个未删除的 Channel，停用的也算
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)、
  [ADR-226](../decisions/bot/adr-226-photon-imessage-channel.md)）。
- 凭据引用只能是 `env:AIT_SERVER_CREDENTIAL_*` 环境变量，或只指向所在 Channel 自身的
  `env:AIT_SERVER_CREDENTIAL_STORED_<channelId>`。后者的 Secret 存于 `<data>/workspace-bots/credentials/<channelId>`，
  Unix 上目录 `0700`、文件属主专用；Channel 删除或 Bot 退役后删除，已删除 Channel 的清理在每次启动时重试
  （[ADR-224](../decisions/bot/adr-224-feishu-scan-onboarding-and-stored-secrets.md)，修订
  [ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md) 第 8 条）。
- 除 QQ 外，外部 Channel 至少需要一个允许的发送者。`bot.channel.types` 只返回 `builtin`、`telegram`、`feishu`，
  客户端按接入方法是否实现显示 Lark、微信、QQ 与 iMessage
  （[ADR-225](../decisions/bot/adr-225-weixin-and-qq-channels.md)、
  [ADR-226](../decisions/bot/adr-226-photon-imessage-channel.md)）。
- Lark 以 `kind: "lark"` 保存，使用 `open.larksuite.com` 验证凭据和收发消息，WebSocket 仅接受
  `wss://*.larksuite.com`；飞书保留中国区端点。账号独占按区域区分，类型与 App ID 不可原地更换。
  `bot.channel.lark.connect` 验证并保存手动输入的凭据；客户端通过该方法发现 Lark 能力，
  飞书扫码流程仍只接入中国区应用（[ADR-272](../decisions/bot/adr-272-lark-channel.md)）。
- QQ 群聊接收 `GROUP_AT_MESSAGE_CREATE`；开启 QQ 的群全量消息能力后，也兼容
  `GROUP_MESSAGE_CREATE`，将群内普通消息一并接纳。

### 中心托管的 Webhook

- Webhook Channel 由 `bot.channel.webhook.create` 经 Host serve 链路创建；daemon 只保存端点和 URL，密钥由中心加密保存。配置只可切换启用状态，轮换使用专门方法。
- 中心只收到已启用 Webhook Channel 的发布状态；自动化会话仅能投递输入、读取或取消对应轮次、确认送达并释放对话。关闭最后一个可用 Channel 时立即结束会话。
- Bot 完成轮次后保持 pending，由中心按 poll 首次读取或 push 的 2xx 回报送达。该入口不创建独立 Hub Channel，传输和权限边界见 [ADR-233](../decisions/bot/adr-233-webhook-channel-center-endpoints.md)。

## 持久化

- 存储根为 `<data>/workspace-bots/`，由 `persistence::storage::bot::FileStore` 原子写入目录、Conversation 与轮次记录。
  记忆位于 `memory/<sha256(workspaceId) 十六进制>.md`，上限 32 KiB，不写入 Workspace 目录；revision 为内容的 SHA-256，
  客户端与 Bot 工具更新时都需携带
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 通知日志位于 `notifications/<botId>.json`，每个 Bot 一个原子文件；旧 daemon 忽略该目录。`Bot.wakeScope` 与轮次的 `wake`、
  `silent`、`notificationCursor` 在默认值时不写入，非默认值会使旧 daemon 拒绝加载 Bot 存储。日志非空后几乎每个轮次都会写入
  `notificationCursor`，因此有活动的 Workspace 降级实际上需要删除 `<data>/workspace-bots/`
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 12 节）。
- 工单位于 `tickets/<botId>/<ticketId>.json`，每个工单一个原子文件；内存只保留索引（每个工单只带最新一条汇报），修改时读写该文件。
  旧 daemon 忽略该目录与通知的 `ticketId`、`ticketTitle`、`turnStartedAt`，但无法解析 `ticket_*` 通知类型与跳过原因 `reported`，含有它们的通知日志或唤醒轮次会使 Bot 存储加载失败
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 3、13 节）。

## 内部会话可见性

- Conversation 投影返回 `sessionAgentId`。只有 `agent.get`、时间线读取、时间线搜索与提示列表能按完整 ID
  解析未归档的 Bot 隐藏会话；前缀与标题解析、列表和写操作仍找不到它，MCP 调用者解析使用 `publicOnly`
  （[ADR-221](../decisions/bot/adr-221-read-only-bot-session-inspection.md)，修订
  [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)）。
- 隐藏会话的 `requiresAttention` 始终为 `false`，快照中 `supportsMessageSend`、`supportsCancel`、
  `supportsConfiguration`、`supportsDetach`、`supportsArchive` 为 `false`
  （[ADR-221](../decisions/bot/adr-221-read-only-bot-session-inspection.md)）。

## 访客与客户端契约

- node 范围的访客只能调用 `bot.get`、`bot.channel.{list,types,status}`、`bot.conversation.list`、`bot.history.get`、
  `bot.turn.get`、`bot.memory.get`、`bot.notifications.list` 与 `bot.ticket.{list,get}`，其余 `bot.*`（包括 `bot.ticket.update`）返回 `Forbidden`；
  workspace／project 范围访客与 Webhook 自动化不能调用 `bot.*` 工单方法
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、
  [ADR-208](../decisions/daemon/adr-208-workspace-share-guests.md)、[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 9 节）。
- 客户端只在 Host 声明 `workspace-bots-v2` 且完整 Bot 方法可用时启用 Bot，权限设置另要求
  `bot.permissions.update.request`；整个功能仍受 `experimentalFeatures` 开关控制。每个 Workspace 至多一个 Bot 标签页，
  标签 ID 固定为 `bot`
  （[ADR-214](../decisions/clients/adr-214-workspace-bot-tab-v2.md)，修订
  [ADR-141](../decisions/clients/adr-141-experimental-workspace-bots.md)；权限门控见
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 唤醒范围设置与最近通知列表另要求 `bot.notifications.list.request`（SDK 特性 `workspaceBotNotifications`），
  且 `bot.get` 返回 `wakeScope`；不满足时隐藏，`bot.setup` 不发送 `wakeScope`。`bot.update` 修改唤醒范围时不发送 `execution`
  （支持通知的 Host 省略时保持不变），不会覆盖其他设备对模型的修改。列表对未知的唤醒状态与跳过原因显示通用文本
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 3、4 节）。
- `bot.conversation.list`（及 `open`、`reset` 返回的对话）带可选的 `channelKind` 与 `lastActivityAt`。Bot 标签页在 Bot 有外部对话时显示对话选择，
  默认且唯一可输入的是内置对话；外部对话只读显示，轮次渲染与内置对话相同，不显示输入框、重置、取消与会话入口；访客按现有读取规则可查看（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 8 节）。
- 带 `wake` 的轮次以通知列表代替用户消息展示，不显示围栏输入；同一 Workspace 的 Agent 与定时任务可打开。
  `silent` 轮次显示为一行说明；唤醒轮次在任何状态下都不显示恰为 `NO_REPLY` 的消息，执行中也不显示可能成为该标记的前缀。
  协议中通知 `kind` 接受未知值，旧 Host 的轮次与 Bot 视图解析不变
  （[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md)）。
- 工单另要求 `bot.ticket.{list,get,update}.request`（SDK 特性 `workspaceBotTickets`）；不满足时对话中没有任务入口，
  唤醒卡片的任务名为纯文本，客户端不发送任何 `bot.ticket.*` 请求，退役的 Bot 也不读取工单。界面称工单为“任务”。
  对话输入框上方的任务标签每 10 秒读取未关闭工单（`limit` 50），等待用户的排在前面，已关闭工单只在用户要求时以 `includeClosed`（`limit` 64）读取；
  打开的工单每 5 秒以 `bot.ticket.get` 刷新。只有 Host 所有者（`canManageBackgroundTasks`）看到关闭、取消与重新打开，
  `update` 只发送 `open`、`done`、`failed`、`cancelled` 与非空的 `outcome`；取消前确认，`ResourceExhausted` 与 `BotConflict` 显示说明。
  协议中通知的 `ticketId`、`ticketTitle` 可选，工单的状态、等待原因、汇报来源与类型都接受未知值并显示通用文本，
  一个新值不会使整个列表无法解析；没有工单的 Host 解析结果不变
  （[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md) 第 5、9 节）。
