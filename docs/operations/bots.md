# Workspace Bot

Bot 是 Workspace 内长期存在的对话实体，使用独占 Channel 和隐藏 Provider session。
当前支持内置对话、Telegram 与中国区飞书；Email 尚未实现。通过实验性 Workspace Bot tab 使用，
入口见 [ADR-141](../decisions/clients/adr-141-experimental-workspace-bots.md)。

## 使用与生命周期

创建 Bot 时指定 Workspace、名称、人格 instructions 和 Provider 配置；创建同时生成内置 Channel。
打开内置对话后发送消息，以稳定 `messageId` 重试可避免重复接纳。相同 ID 的目标或内容不同会返回冲突。
新建外部 Channel 默认停用，配置完成后显式启用，并检查 Channel 状态。

- 同一 Conversation 串行执行，不同发送者/线程的外部对话分开维护上下文。
- Provider 会话从创建起隐藏，普通 Agent API 无法接管；关闭界面不停止 Bot。
- 配置在新 Conversation 或显式 reset 后生效。reset 前先完成或取消正在执行的轮次；排队中的输入随 reset 取消；旧历史保留。
- 暂停取消排队和执行中的输入，保留可恢复会话；归档才退役会话。Workspace 归档先停用 Bot。
- 所有 Provider 权限请求自动拒绝，不等待用户审批；Bot 内禁用原生自动审核，见
  [ADR-145](../decisions/bot/adr-145-bot-permission-denial.md)。

输入落盘后才确认平台消息或推进游标；模型完成与平台投递是独立状态。
重启时结果不明的运行输入标记 `uncertain`，阻止同 generation 的后续输入，需显式 reset；
不会自动重做可能产生副作用的输入。正在发送但未确认的回复也标记 `uncertain`，不盲目重发。
当前通过持久查询读取结果，尚无 Bot 流式事件订阅。

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

每个 Bot 的共享记忆位于 `<workspace.cwd>/.ait/bots/<sha256(workspaceId)>/<botId>/MEMORY.md`，
限制 32 KiB。每轮读取并提供给原生会话；客户端更新使用 revision 防止覆盖旧内容。
外部文件工具直接编辑不受这个 revision 保护；删除 worktree 也会删除其中的记忆。

接口通过认证 WebSocket 协商 `workspace-bots-v1`。常用流程为
`bot.create.request` → `bot.conversation.open.request` → `bot.message.send.request`，
再以 `bot.turn.get.request` / `bot.history.get.request` 读取执行与投递状态。
Channel 能力以 `bot.channel.types.request` 为准；完整参数见
[`protocol.rs`](../../crates/bot/src/protocol.rs)，实现归属见
[ADR-139](../decisions/bot/adr-139-workspace-bots-and-owned-channels.md)。

旧设计、完整 RPC 示例与历史验证见[归档](../archive/plans/bot-backend.md)，未实现能力见[计划索引](../plans/README.md)。
