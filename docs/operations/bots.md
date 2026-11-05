# Workspace Bot

每个 Workspace 至多一个管理者 Bot，使用独占 Channel 和隐藏 Provider session，由用户显式设置；未设置时不产生任何记录、会话、轮询或模型调用。
当前支持内置对话、Telegram 与中国区飞书；Email 尚未实现。设计见 [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)。
客户端 Bot 标签页见 [ADR-214](../decisions/clients/adr-214-workspace-bot-tab-v2.md)，使用方式见下文“在 Ait 中使用”。

## 使用与生命周期

通过 `bot.setup` 指定 `workspaceId` 与 Provider 执行配置设置 Bot，同时生成内置 Channel；Bot 行为由内置管理者指令决定，不再有自定义人格。
相同配置重复设置幂等（`created:false`），配置不同且已有非退役 Bot 返回 `BotConflict`；用 `bot.update` 修改执行配置。
`clearMemory:true` 仅在新建 Bot 时清空记忆，幂等重复调用时忽略。
打开内置对话后发送消息，以稳定 `messageId` 重试可避免重复接纳。相同 ID 的目标或内容不同会返回冲突。
新建外部 Channel 默认停用，配置完成后显式启用，并检查 Channel 状态。

- 同一 Conversation 串行执行，不同发送者/线程的外部对话分开维护上下文。
- Provider 会话从创建起隐藏，普通 Agent API 无法接管；关闭界面不停止 Bot。
- 配置在新 Conversation 或显式 reset 后生效。reset 前先完成或取消正在执行的轮次；排队中的输入随 reset 取消；旧历史保留。
- 状态为 `active`、`paused`（原因 `user` 或 `directory_missing`）、`retired`。暂停取消排队和执行中的输入并停止外部轮询；
  恢复要求 Workspace 目录存在，否则返回 `WorkspaceNotFound`。Workspace 目录缺失会在轮次可运行时检测，Bot 暂停为 `directory_missing`，需用户显式恢复。
- `bot.remove` 使 Bot 退役、保留记忆，并释放其 Telegram／飞书账号（外部 Channel 标记为已删除）；Workspace 归档同样退役并删除记忆。
- 退役 Bot 的对话与历史仍留在磁盘上，但 v2 接口只读取当前未退役的 Bot，因此无法再查看；
  重新 `bot.setup` 从空的对话历史开始，记忆除非 `clearMemory:true` 否则保留。清理或只读访问退役历史留待后续工作。
- 所有 Provider 权限请求自动拒绝，不等待用户审批；Bot 内禁用原生自动审核，见
  [ADR-145](../decisions/bot/adr-145-bot-permission-denial.md)。

输入落盘后才确认平台消息或推进游标；模型完成与平台投递是独立状态。
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
   Channel 的凭据只填写 `env:AIT_SERVER_CREDENTIAL_*` 环境变量引用，并配置允许的发送者；新建 Channel 默认停用，确认配置与状态后再启用。移除需要确认。
5. 访客在所有视图中只读。重新设置被移除的 Bot（包括在其他设备上）得到空的对话与 Channel，客户端不会显示旧 Bot 的缓存内容。

## Telegram 与飞书配置

凭据由启动 daemon 的宿主环境注入；Channel 仅保存引用，不保存明文 secret。
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

飞书使用自建应用机器人，配置 IM 接收/发送权限，订阅 `im.message.receive_v1` 并选择长连接。
默认只接收允许列表中的发送者；群聊需显式开启并提及机器人。当前不支持 Lark 国际区域、富媒体和卡片流式回复。
同一 daemon 内限制平台账号独占，无法阻止其他服务使用同一账号。
实际平台凭据和应用权限需要单独验收，历史模拟服务测试不能证明真实账号已验证。

## 记忆与接口

Bot 记忆位于 `<data>/workspace-bots/memory/<sha256(workspaceId)>.md`，限制 32 KiB，不再写入 Workspace 目录。
每轮读取并提供给原生会话；客户端通过 `bot.memory.update` 更新，revision 防止覆盖旧内容。
Bot 自身暂时不能写记忆（工具在后续阶段加入）。移除 Bot 保留记忆，Workspace 归档删除记忆。

接口通过认证 WebSocket 协商 `workspace-bots-v2`，所有方法以 `workspaceId` 寻址，客户端 JSON 不含内部 `botId`。常用流程为
`bot.setup.request` → `bot.conversation.open.request` → `bot.message.send.request`，
再以 `bot.turn.get.request` / `bot.history.get.request` 读取执行与投递状态；`bot.get` 在未配置时返回 `null`。
其余方法：`bot.update`、`bot.pause`、`bot.resume`、`bot.remove`、`bot.channel.*`、`bot.conversation.{list,reset}`、
`bot.turn.cancel`、`bot.memory.get`。`bot.create`、`bot.list`、`bot.archive`、`bot.permission.resolve` 已删除。
Channel 能力以 `bot.channel.types.request` 为准；完整参数见
[`protocol.rs`](../../crates/bot/src/protocol.rs)，实现归属见
[ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md) 与
[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)。

## 从旧版升级

旧版多 Bot 数据不迁移：daemon 启动时关闭 `<data>/bots/` 中记录的隐藏会话并删除该目录，之后用 `bot.setup` 重新设置。
旧版写在 `<workspace>/.ait/bots/` 的记忆文件不再读取，可手动删除。

旧设计、完整 RPC 示例与历史验证见[归档](../archive/plans/bot-backend.md)，未实现能力见[计划索引](../plans/README.md)。
