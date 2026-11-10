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
- `bot_memory_read`、`bot_memory_update`、`workspace_overview` 只对 Bot 自身凭据可见，子树与主机令牌都看不到
  （[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
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
  飞书或 QQ App ID、微信账号、Photon 项目）在同一 daemon 内只能属于一个未删除的 Channel，停用的也算
  （[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)、
  [ADR-226](../decisions/bot/adr-226-photon-imessage-channel.md)）。
- 凭据引用只能是 `env:AIT_SERVER_CREDENTIAL_*` 环境变量，或只指向所在 Channel 自身的
  `env:AIT_SERVER_CREDENTIAL_STORED_<channelId>`。后者的 Secret 存于 `<data>/workspace-bots/credentials/<channelId>`，
  Unix 上目录 `0700`、文件属主专用；Channel 删除或 Bot 退役后删除，已删除 Channel 的清理在每次启动时重试
  （[ADR-224](../decisions/bot/adr-224-feishu-scan-onboarding-and-stored-secrets.md)，修订
  [ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md) 第 8 条）。
- 除 QQ 外，外部 Channel 至少需要一个允许的发送者。`bot.channel.types` 只返回 `builtin`、`telegram`、`feishu`，
  客户端按接入方法是否实现显示微信、QQ 与 iMessage
  （[ADR-225](../decisions/bot/adr-225-weixin-and-qq-channels.md)、
  [ADR-226](../decisions/bot/adr-226-photon-imessage-channel.md)）。

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
  `bot.turn.get` 与 `bot.memory.get`，其余 `bot.*` 返回 `Forbidden`；workspace／project 范围访客不能调用 `bot.*`
  （[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、
  [ADR-208](../decisions/daemon/adr-208-workspace-share-guests.md)）。
- 客户端只在 Host 声明 `workspace-bots-v2` 且完整 Bot 方法可用时启用 Bot，权限设置另要求
  `bot.permissions.update.request`；整个功能仍受 `experimentalFeatures` 开关控制。每个 Workspace 至多一个 Bot 标签页，
  标签 ID 固定为 `bot`
  （[ADR-214](../decisions/clients/adr-214-workspace-bot-tab-v2.md)，修订
  [ADR-141](../decisions/clients/adr-141-experimental-workspace-bots.md)；权限门控见
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
