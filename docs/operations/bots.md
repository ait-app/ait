# Workspace Bot

每个 Workspace 至多一个管理者 Bot，使用独占 Channel 和隐藏 Provider session，由用户显式设置；未设置时不产生任何记录、会话、轮询或模型调用。
当前支持内置对话、Telegram、中国区飞书、微信（仅单聊）、QQ（单聊和群聊）与 iMessage（通过 Photon，仅单聊）；Email 尚未实现。设计见 [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)，权限与 Ait 工具见 [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)。
客户端 Bot 标签页见 [ADR-214](../decisions/clients/adr-214-workspace-bot-tab-v2.md)，使用方式见下文“在 Ait 中使用”。

## 使用与生命周期

通过 `bot.setup` 指定 `workspaceId` 与 Provider 执行配置设置 Bot，同时生成内置 Channel；Bot 行为由内置管理者指令决定，不再有自定义人格。
相同配置重复设置幂等（`created:false`），配置不同且已有非退役 Bot 返回 `BotConflict`；用 `bot.update` 修改执行配置。
`clearMemory:true` 仅在新建 Bot 时清空记忆，幂等重复调用时忽略。
打开内置对话后发送消息，以稳定 `messageId` 重试可避免重复接纳。相同 ID 的目标或内容不同会返回冲突。
新建外部 Channel 默认停用，配置完成后显式启用，并检查 Channel 状态。

- 同一 Conversation 串行执行，不同发送者/线程的外部对话分开维护上下文。
- Provider 会话从创建起隐藏，普通 Agent API 无法接管；Bot 标签页将其列为子 Agent，可按完整 ID 只读查看（[ADR-221](../decisions/bot/adr-221-read-only-bot-session-inspection.md)）；关闭界面不停止 Bot。
- 配置在新 Conversation 或显式 reset 后生效。reset 前先完成或取消正在执行的轮次；排队中的输入随 reset 取消；旧历史保留。
- 状态为 `active`、`paused`（原因 `user` 或 `directory_missing`）、`retired`。暂停取消排队和执行中的输入并停止外部轮询；
  恢复要求 Workspace 目录存在，否则返回 `WorkspaceNotFound`。Workspace 目录缺失会在轮次可运行时检测，Bot 暂停为 `directory_missing`，需用户显式恢复。
- `bot.remove` 使 Bot 退役、保留记忆，并释放其 Telegram／飞书账号（外部 Channel 标记为已删除）；Workspace 归档同样退役并删除记忆。
- 退役 Bot 的对话与历史仍留在磁盘上，但 v2 接口只读取当前未退役的 Bot，因此无法再查看；
  重新 `bot.setup` 从空的对话历史开始，记忆除非 `clearMemory:true` 否则保留。清理或只读访问退役历史留待后续工作。
- 所有 Provider 权限请求自动拒绝，不等待用户审批，见
  [ADR-145](../decisions/bot/adr-145-bot-permission-denial.md)；Bot 的 Ait 能力由下文“权限”授予。

输入落盘后才确认平台消息或推进游标；模型完成与平台投递是独立状态。
一次执行中的每条可见 assistant 消息都会进入同一个 turn 的 `messages` 数组并在运行期间更新；同一 `messageId` 的流式片段更新原消息，不同 `messageId` 保持独立。tool call、reasoning 和其他内部记录不会进入 Bot 对话。`reply` 仍保留为旧客户端读取最新消息的兼容字段。
重启时结果不明的运行输入标记 `uncertain`，阻止同 generation 的后续输入，需显式 reset；
不会自动重做可能产生副作用的输入。正在发送但未确认的回复也标记 `uncertain`，不盲目重发。
当前通过持久查询读取结果，尚无 Bot 流式事件订阅。

## 在 Ait 中使用

Bot 标签页仍在“设置 → 关于”的测试功能开关之后，且要求 Host 支持 `workspace-bots-v2`，否则显示升级提示。

1. 开启测试功能后，在 Workspace 新建菜单选择 Bot；每个 Workspace 只有一个 Bot 标签页，重复打开会聚焦已有标签页。
2. 未设置时显示设置向导：选择 Provider、模型、权限模式与推理强度后提交；可勾选“清空已保存的记忆”，否则保留之前移除的 Bot 留下的记忆。
   访客只看到未配置提示。
3. 设置完成后进入对话，轮次显示状态与错误。Bot 暂停时仍可查看对话历史（重新加载后也可），输入框停用，有管理权限的成员可点击恢复；
   目录缺失导致的暂停会说明原因。暂停期间记忆保存、Channel 新增与启停也停用，恢复后才能修改。
4. 在设置页可修改模型（对新对话生效）、暂停或恢复、按 revision 编辑记忆、管理 Telegram 与飞书 Channel、移除 Bot。
   飞书 Channel 默认扫码接入，也可手动填写凭据，见下文“飞书接入”；Telegram 按页面引导粘贴 Token 并点击 Start，见下文“Telegram 接入”。两者都可改用 `env:AIT_SERVER_CREDENTIAL_*` 环境变量引用并配置允许的发送者；通过环境变量引用新建的 Channel 默认停用，确认配置与状态后再启用。移除需要确认。
5. 访客在所有视图中只读。重新设置被移除的 Bot（包括在其他设备上）得到空的对话与 Channel，客户端不会显示旧 Bot 的缓存内容。

## 权限

Bot 通过 Ait MCP 工具管理 Workspace，可用工具由用户授予的**分组**决定。分组整体替换保存，在 Bot 的下一次 MCP 请求生效，
工具目录和指令在 Bot 下一轮刷新。

| 分组        | 内容                                                                    | 默认 |
| ----------- | ----------------------------------------------------------------------- | ---- |
| `observe`   | 查看 Agent、定时任务、脚本、终端、Provider 与模型（总是开启，不可关闭） | 是   |
| `agents`    | 创建 Agent、发送提示、修改 Agent 设置与模式                             | 是   |
| `stop`      | 取消、归档、终止 Agent                                                  | 是   |
| `schedules` | 创建、修改、暂停、恢复、运行、删除定时任务与心跳                        | 是   |
| `scripts`   | 启停 Workspace 脚本                                                     | 否   |
| `terminals` | 创建终端、发送按键、读取并终止终端                                      | 否   |

`create_workspace`、`respond_to_permission`、`archive_workspace`、`rename_workspace`、`list_workspaces` 永不授予 Bot，Bot 不能通过 Ait MCP 创建 worktree 或 Workspace（[ADR-222](../decisions/bot/adr-222-single-agent-mode-no-bot-workspaces.md)）。
`bot_memory_read`、`bot_memory_update`、`workspace_overview`（作用域概览，输出上限 24 KiB）只对 Bot 自己可见。

**Agent 模式**为每个 Provider 选择一个原生模式（至多 32 个 Provider），Bot 与它启动的 Agent 只能使用该模式；
未列出的 Provider 不能被使用。阶段 1／2 创建的 Bot 升级后得到默认分组但模式上限为空，不能启动 Agent，需在设置中配置。
上限约束 Bot 与子树设置或创建的每一个模式。允许能运行命令的模式或 `terminals` 分组时，这些 Agent 与终端命令可读取本机 daemon 凭据并绕过 Bot 的 Ait 限制，
设置页与向导在模式上限部分和终端分组下显示此警告。只读模式不阻止读取：即使 Codex `read-only` 下的 Bot 也能读取 `local-api.token`（见 ADR-215）。

**作用域**为 Bot 所在 Workspace，加上此前已创建且未归档的 worktree（升级前的 Bot 可能留有记录）。Bot 可向作用域内的任何 Agent
发送提示、修改设置，也可运行、暂停、恢复或删除任何定时任务（含用户启动和创建的），但受已授权分组与模式上限约束；
`update_schedule` 只能编辑 Bot 创建的定时任务，用户创建的任务需由用户自己修改；
向用户启动的 Agent 发送提示可能触及超出 Bot 自身限制的能力，这是已接受的风险（见 ADR-215）。

**Bot 模型的降级。** Bot 可使用任何 Provider。Codex 与 Claude 使用只读模式（Codex `read-only`；Claude `default`，仍把写入路由到审批的最严格模式，
用户与项目的允许规则照常适用）、系统提示和 Ait 工具预授权。其他 Provider 使用默认模式，指令前置到每一条输入，
需要审批的写入与 Ait 工具调用被自动拒绝；Provider 不经审批执行的写入不会被阻止，不支持 HTTP MCP 的 Provider 没有 Ait 工具。
设置页以能力说明显示这些差异。已有会话无法达到安全配置时，Bot 自动轮换一次对话。

客户端以 `bot.permissions.update.request` 是否实现决定是否显示权限设置；服务端 `bot.setup` 可携带可选 `permissions`，
`BotView.permissions` 返回当前配置。更新示例（参数整体替换，访客不可调用）：

```json
{
  "workspaceId": "<workspaceId>",
  "permissions": {
    "groups": ["observe", "agents", "stop", "schedules", "scripts"],
    "modes": { "codex": ["read-only", "auto"], "claude": ["default"] }
  }
}
```

### Bot 启动的 Agent

Bot 创建的 Agent 带保留标签 `ait.bot.scope`，其会话获得派生凭据，只能访问同一作用域并受同一模式上限约束，无 Bot 专属工具；
作用域调用者不能写入任何 `ait.` 前缀的标签。Bot 暂停期间这些 Agent 保留完整授权（只有 Bot 自身的凭据失效）；Bot 移除后降为只读观察；
同一 Workspace 设置新 Bot 后，新 Bot 的授权适用于该 Workspace 已有的这些 Agent 与定时任务。

Bot 创建的定时任务记录 `botScope`（根 Workspace，而不是某个 Bot），每次运行前按当前授权复核；不再被允许时该次运行失败，任务保持启用。
没有当前 Bot 时（例如 Bot 已移除），这些任务的每次运行都以 `REVOKED` 错误失败；同一 Workspace 设置新 Bot 后，它们按新 Bot 的授权运行。
Bot 运行或恢复模式超出上限的任务会被拒绝；没有存储模式的用户任务需由用户先设置允许的模式。
凭据细节见 [MCP](mcp.md#bot-与子树凭据)。

## Telegram 与飞书配置

飞书推荐扫码接入，Telegram 推荐引导接入，都无需下面的配置。使用环境变量时，凭据由启动 daemon 的宿主环境注入，Channel 仅保存引用。
以下为非秘密配置示例，替换允许的用户 ID 后再启用。

```json
{
  "kind": "telegram",
  "tokenRef": "env:AIT_SERVER_CREDENTIAL_TELEGRAM",
  "allowedSenders": ["123456"],
  "allowGroups": false
}
```

Telegram 使用 getUpdates 长轮询。已有 webhook 会报告冲突，不会自动删除；
当前只处理文本，不处理附件或编辑消息。

```json
{
  "kind": "feishu",
  "appId": "cli_example",
  "secretRef": "env:AIT_SERVER_CREDENTIAL_FEISHU",
  "allowedSenders": ["ou_example"],
  "allowGroups": false
}
```

### Telegram 接入

在 Bot 设置页添加 Telegram Channel，按页面步骤在 BotFather 创建机器人并粘贴 Token，daemon 向 Telegram 验证后显示一个二维码和链接。打开后在机器人对话中点击 Start，你会成为唯一允许的发送者，Channel 随后自动启用；群聊需在继续前勾选。链接 10 分钟后过期，daemon 重启后需重新开始。
配对期间发给该机器人的其他消息会被丢弃。Token 与飞书 App Secret 保存在同一位置，边界相同。

### 微信接入

在 Bot 设置页添加微信 Channel，点击“显示二维码”，用微信扫码并在手机上确认，Channel 随后自动启用。只有扫码者可以在单聊中给机器人发消息，不支持群聊，也没有手动接入。
微信结束会话后 Channel 显示已过期，需删除后重新扫码。此流程依赖未公开的 iLink 接口，边界见 [ADR-225](../decisions/bot/adr-225-weixin-and-qq-channels.md)。

### QQ 接入

在 Bot 设置页添加 QQ Channel，点击“显示二维码”，用 QQ 扫码并同意，Channel 随后自动启用，只有扫码者可以私聊机器人；群聊需在扫码前勾选。
扫码不可用时可选择手动填写 App ID 与 App Secret，daemon 向 QQ 验证后保存。

手动接入的 Channel 起初没有允许的发送者，开启群聊的 Channel 也可能无法在群里识别扫码者。此时在该 Channel 所在行点击“获取配对码”，把配对码私聊发给机器人，或在群里 @ 机器人并发送它，发送者即加入允许列表。配对码只能使用一次，10 分钟后过期，daemon 重启后失效。

QQ 限制被动回复的时间和次数，超出后 Ait 改用主动消息；在 QQ 中关闭机器人的“允许主动发送”后，这部分回复会发送失败。

### iMessage 接入

iMessage 通过 Photon Beta 2.0 的托管线路收发，消息内容经过 Photon 的云服务，不需要 Mac，边界见 [ADR-226](../decisions/bot/adr-226-photon-imessage-channel.md)。

先在 [Photon 控制台](https://app.beta.photon.codes)创建项目和 API Key，Key 需要 `project:read`、`project:write`、`platforms:read`、`platforms:write` 权限。然后在 Bot 设置页添加 iMessage Channel，填写 Project ID、API Key 和你自己的手机号（E.164）或 iMessage 邮箱。daemon 分配一条共享线路并显示号码和一次性代码，从填写的身份用 iMessage 把代码发给该号码，Channel 随后自动启用，只有该身份可以给 Bot 发消息。代码 10 分钟后过期，daemon 重启后需重新开始。

当前只处理单聊文本，不支持群聊和附件，也不支持 Photon 的专用线路。回复被 Photon 接受即显示为已投递，最终失败只能在 Photon 控制台看到。删除 Channel 不释放线路分配。已用真实 Photon Beta 项目验证单聊往返，移动端设置页尚未在真机上验证。

Channel 存在后回退到旧版 daemon 会导致 daemon 无法启动，即使 Channel 已删除，详见 ADR-226。

### 飞书接入

在 Bot 设置页添加飞书 Channel，点击“显示二维码”，用飞书 App 扫码并同意。daemon 直接向飞书创建应用，无需开发者后台操作；
完成后 Channel 自动启用，只有扫码者可以给机器人发消息，群聊需在扫码前勾选。二维码由 daemon 按飞书返回的有效期管理，daemon 重启后需重新扫码。
扫码不可用（例如企业禁止创建应用）时选择手动填写，按页面步骤创建应用，粘贴 App ID 与 App Secret，并填写发送者的 Open ID；daemon 向飞书验证后保存。也可填写 `env:` 引用。

扫码或粘贴的 App Secret 保存在 `<data>/workspace-bots/credentials/<channelId>`（Unix 上仅所有者可读），删除 Channel 或移除 Bot 时一并删除；飞书上的应用需在开发者后台自行删除。
同一用户运行的进程可以读取此文件，边界见 [ADR-224](../decisions/bot/adr-224-feishu-scan-onboarding-and-stored-secrets.md)。

飞书使用自建应用机器人，配置 IM 接收/发送权限，订阅 `im.message.receive_v1` 并选择长连接。
默认只接收允许列表中的发送者；群聊需显式开启并提及机器人。当前不支持 Lark 国际区域、富媒体和卡片流式回复。
同一 daemon 内限制平台账号独占，无法阻止其他服务使用同一账号。
实际平台凭据和应用权限需要单独验收，历史模拟服务测试不能证明真实账号已验证。

### Ait 中心 Channel

Ait 中心（ait-server）可以把飞书、Slack、Telegram 中发给中心机器人的消息转给 Workspace Bot，见 [ADR-233](../decisions/bot/adr-233-hub-channel-center-automation.md)。
用 `bot.channel.create.request` 创建 `{"kind":"hub"}` Channel 并启用，即允许本账号的中心访问这个 Bot；每个 Bot 至多一个，客户端添加入口尚未实现。
daemon 不为它运行传输：中心通过自己的 Hub 会话投递消息、轮询 `bot.turn.get` 取回复，再用 `bot.hub.delivery.ack` 报告投递结果，会话结束时用 `bot.hub.conversation.close` 删除会话。
中心会话只能使用这五个方法，且只能访问中心 Channel（Hub 与 Webhook）的轮次和会话；最后一个中心 Channel 停用、或 Bot 暂停或移除后，中心会话立即断开。

Webhook Channel 由中心托管一个 HTTPS 端点：在 Bot 设置页添加「Webhook」即创建，Host 需已接入支持 Webhook 的中心。
创建后只显示一次密钥；丢失后用「重新生成密钥」（`bot.channel.webhook.rotate`）换新，旧密钥立即失效。调用方 `POST` 文本到地址（`Authorization: Bearer <密钥>`），得到 202 和状态地址。回复有两种取法：用同一密钥请求状态地址（加 `?wait=30` 会等到回复就绪），或在调用里带上 `callback_url`，由中心把签名过的结果 POST 过去。同一 `thread` 的调用接着同一个会话。
删除 Channel 会同时请中心删除端点；中心不可达时只在本地删除，中心不再把消息投给这个 Channel。

## 记忆与接口

Bot 记忆位于 `<data>/workspace-bots/memory/<sha256(workspaceId)>.md`，限制 32 KiB，不再写入 Workspace 目录。
每轮读取并提供给原生会话；客户端通过 `bot.memory.update` 更新，revision 防止覆盖旧内容。
Bot 自身通过 `bot_memory_read`／`bot_memory_update` 读写记忆（携带 revision，冲突时重读）；内置指令把记忆放在 `<workspace-memory>` 围栏内并声明为用户数据。移除 Bot 保留记忆，Workspace 归档删除记忆。

接口通过认证 WebSocket 协商 `workspace-bots-v2`，所有方法以 `workspaceId` 寻址，客户端 JSON 不含内部 `botId`。常用流程为
`bot.setup.request` → `bot.conversation.open.request` → `bot.message.send.request`，
再以 `bot.turn.get.request` / `bot.history.get.request` 读取执行与投递状态；`bot.get` 在未配置时返回 `null`。
其余方法：`bot.update`、`bot.pause`、`bot.resume`、`bot.remove`、`bot.channel.*`（含 `bot.channel.feishu.scan.{start,status,cancel}`、`bot.channel.feishu.connect`、`bot.channel.telegram.pair.{start,status,cancel}`、`bot.channel.weixin.scan.{start,status,cancel}`、`bot.channel.qq.scan.{start,status,cancel}`、`bot.channel.qq.connect`、`bot.channel.qq.pair`、`bot.channel.imessage.pair.{start,status,cancel}` 与 `bot.channel.webhook.{create,rotate}`）、`bot.conversation.{list,reset}`、
`bot.turn.cancel`、`bot.memory.get`。`bot.create`、`bot.list`、`bot.archive`、`bot.permission.resolve` 已删除。
Channel 能力以 `bot.channel.types.request` 为准；完整参数见
[`protocol.rs`](../../crates/bot/src/protocol.rs)，实现归属见
[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md) 与
[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)。

## 从旧版升级

旧版多 Bot 数据不迁移：daemon 启动时关闭 `<data>/bots/` 中记录的隐藏会话并删除该目录，之后用 `bot.setup` 重新设置。
旧版写在 `<workspace>/.ait/bots/` 的记忆文件不再读取，可手动删除。

旧设计、完整 RPC 示例与历史验证见[归档](../archive/plans/bot-backend.md)，未实现能力见[计划索引](../plans/README.md)。
