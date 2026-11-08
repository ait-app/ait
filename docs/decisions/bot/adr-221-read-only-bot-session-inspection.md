# ADR-221：以子 Agent 形式只读查看 Bot 内部会话

- 状态：已实现
- 日期：2026-11-08
- 关系：修订 [ADR-213](adr-213-single-workspace-manager-bot.md) 的会话隐藏边界与 [ADR-214](../clients/adr-214-workspace-bot-tab-v2.md) 的对话界面；复用 [ADR-209](../providers/adr-209-observed-subagent-normalization.md) 的子 Agent 展示。

## 背景

Bot 的 Provider 会话从创建起隐藏，用户无法看到 Bot 一轮内实际调用了哪些工具。
Bot 标签页使用一套独立的卡片样式，把用户输入与回复放在同一个框内，与会话页不一致。

## 决策

1. **对话界面与会话页一致。** 用户输入使用会话页的用户气泡，Bot 回复使用助手消息，两者分开显示；输入框使用会话页的 `MessageInput`；取消当前轮次放在输入框的停止位置。
2. **内部会话列为子 Agent。** `BotConversation` 增加可选的 `sessionAgentId`。第一轮创建会话之后，Bot 标签页在输入框上方的子 Agent 胶囊中列出该会话，点击后以普通 Agent 标签页打开。
3. **只读，且只能按完整 ID 访问。** 只有 `agent.get`、`agent.timeline.get`、时间线搜索、提示列表和时间线订阅可以寻址 Bot 当前未归档的隐藏会话，且必须使用完整 ID。
   前缀和标题解析、Agent 列表、目录、attention 仍然排除隐藏会话；发送、取消、配置、归档、清除 attention 等写操作仍返回 `AgentNotFound`；隐藏会话不能作为新 Agent 的调用者，MCP 确认调用者身份时使用 `publicOnly` 排除隐藏会话。
4. **快照声明只读且不产生提醒。** 隐藏会话的 `requiresAttention` 始终为 `false`，客户端不会因它切换焦点；快照把 `supportsMessageSend`、`supportsCancel`、`supportsConfiguration`、`supportsDetach`、`supportsArchive` 置为 `false`，客户端据此不显示输入框和操作按钮。
5. **标签页生命周期。** 隐藏会话不进入 Agent 目录，其标签页以 `pin` 打开，避免被目录对账关闭；本设备重置对话或移除 Bot 时关闭已退役会话的标签页。

## 后果

- 能查看 Workspace 的访客也能按此路径只读查看 Bot 会话的时间线，包括工具调用的输入与输出，这与访客查看该 Workspace 其他 Agent 的权限一致。
- 隐藏会话的状态变化不通过 Agent 目录推送；子 Agent 胶囊的运行状态取自 Bot 轮次，已打开标签页的时间线通过订阅更新。
- 旧 Host 不返回 `sessionAgentId`，客户端不显示该子 Agent。
- 重置对话会轮换会话，旧会话归档后不可再访问；在其他设备上已打开的旧会话标签页会显示加载失败，需手动关闭。

## 验证

按仓库规则只运行受影响范围的测试：`provider` 的 Bot 会话用例、`bot` crate、移动端 Bot 组件测试、`workspace-bots.spec.ts` 浏览器用例；未运行完整 Rust Workspace 测试。

## Test coverage

未测量覆盖率：本次未请求覆盖率统计。
