# ADR-213：单 Workspace 管理者 Bot 与 v2 接口

- 状态：接受（阶段 1 已随 0.1.30 发布；阶段 2 客户端见 ADR-214；阶段 3 权限与 MCP 见 ADR-215）
- 日期：2026-11-05
- 关系：取代 [ADR-139](adr-139-workspace-bots-and-owned-channels.md) 中多 Bot、人格和 `botId` 寻址（第 1、2、5 条相关部分）以及记忆位置（第 7 条）；
  修订 [ADR-141](../clients/adr-141-experimental-workspace-bots.md) 的标签页身份与能力名（客户端见 [ADR-214](../clients/adr-214-workspace-bot-tab-v2.md)）；
  取代 [ADR-145](adr-145-bot-permission-denial.md) 第 3 条（删除 `bot.permission.resolve`），其第 1、2、4、5 条的自动拒绝规则继续有效。
- 后续修订：[ADR-215](adr-215-bot-permissions-and-scoped-mcp.md) 增加权限与作用域 MCP 工具，修订 ADR-145 第 4 条，并让 Bot 可写记忆（第 6 条）；[ADR-221](adr-221-read-only-bot-session-inspection.md) 允许按完整 ID 只读查看隐藏会话。
- 计划：[Workspace Bot 管理者重设计](../../plans/workspace-bot-manager.md)、[阶段 1 计划](../../plans/workspace-bot-manager-phase-1.md)。

## 背景

原设计允许一个 Workspace 创建多个带人格的 Bot，客户端以 `botId` 寻址，记忆写在 Workspace 目录的 `.ait/bots/` 下。
这使 Bot 更像可任意创建的聊天实例：Workspace 与 Bot 的关系不唯一，记忆随 worktree 删除或误提交，
目录缺失时的行为不明确。新方向把 Bot 定位为 Workspace 的管理者：每个 Workspace 至多一个，由用户显式设置，
未设置时不占用任何资源。

## 决策

1. **单 Bot 约束。** 每个 Workspace 同时至多一个非 `retired` 的 Bot。未配置 Bot 的 Workspace 不产生 Bot 记录、
   隐藏会话、Channel 轮询或模型调用。客户端 JSON 以 `workspaceId` 寻址，不再携带内部 `botId`。
2. **`BotState`。** `active`、`paused`（原因 `user` 或 `directory_missing`）、`retired`。
   - 暂停取消排队和执行中的输入并停止外部轮询；恢复要求 Workspace 目录存在，否则返回 `WorkspaceNotFound`。
   - 用户移除：进入 `retired`，保留记忆，释放该 Bot 的 Telegram／飞书账号，外部 Channel 标记为已删除。
   - Workspace 归档：进入 `retired` 并删除该 Workspace 的 Bot 记忆。
   - 目录缺失在某个轮次变为可运行时检测，Bot 进入 `paused(directory_missing)`，由用户显式恢复。
   - 退役 Bot（移除或归档）的 Conversation 与历史仍保存在磁盘上，但 v2 接口的所有读取只解析当前非 `retired` 的 Bot，
     因此无法再读取退役 Bot 的历史。重新设置得到新的 Bot，对话历史为空；记忆除非 `clearMemory:true` 否则保留。
3. **`bot.setup` 语义。** 参数为 `workspaceId`、`execution` 和可选 `clearMemory`。执行配置相同时幂等返回 `created:false`；
   已有非 `retired` Bot 且配置不同返回 `BotConflict`，因此并发设置不会产生两个活跃 Bot。
   `clearMemory:true` 仅在确实新建 Bot 时生效，幂等重复调用时忽略。重新设置时默认保留已移除 Bot 的记忆。
4. **v2 方法。** 能力名 `workspace-bots-v2` 取代 `workspace-bots-v1`。方法：`bot.get`（未配置返回 `null`）、
   `bot.setup`、`bot.update`（仅修改 `execution`，对新会话生效）、`bot.pause`、`bot.resume`、`bot.remove`、
   `bot.channel.{list,types,create,update,delete,status}`、`bot.conversation.{list,open,reset}`、`bot.message.send`、
   `bot.history.get`、`bot.turn.{get,cancel}`、`bot.memory.{get,update}`。删除 `bot.create`、`bot.list`、
   `bot.archive`、`bot.permission.resolve`。访客对 `bot.get`、`bot.history.get`、`bot.memory.get` 只读，其余写入方法被拒绝。
5. **内置管理者指令。** 内置指令（`crates/bot/src/instructions.rs`）取代用户编写的人格；Conversation 不再保存 `instructions`。
6. **记忆。** 存储于 `<data>/workspace-bots/memory/<sha256(workspaceId) 十六进制>.md`，上限 32 KiB，不再写入 Workspace 目录。
   客户端通过 `bot.memory.update` 并携带 revision 更新。Bot 自身通过 `bot_memory_read`／`bot_memory_update` 读写记忆（见 ADR-215）。
7. **旧数据丢弃。** 新存储根为 `<data>/workspace-bots/`。daemon 启动时关闭旧根 `<data>/bots/` 中记录的隐藏会话并删除该目录，
   不做迁移。Workspace 内旧的 `.ait/bots/` 记忆文件不再读取，用户可手动删除。

## 后果

- 这是不兼容变更：升级后旧 Bot、Channel、Conversation 与历史全部丢失，需要通过 `bot.setup` 重新配置。
- 客户端 Bot 标签页已按 `workspace-bots-v2` 重写，见 [ADR-214](../clients/adr-214-workspace-bot-tab-v2.md)；旧 Host 只提供 v1 时客户端显示升级提示。
- 阶段 2（客户端标签页与设置向导）与阶段 3（权限与 MCP 工具、Bot 写记忆，见 [ADR-215](adr-215-bot-permissions-and-scoped-mcp.md)）已实现；阶段 4（来源标记与通知）尚未实现。
  [ADR-145](adr-145-bot-permission-denial.md) 的权限请求自动拒绝仍然适用。
- 记忆脱离 Workspace 目录后不再随 worktree 删除，也不会进入版本控制；其生命周期由移除与归档规则管理。
- 退役 Bot 的历史留在磁盘上但不可通过接口访问；清理这些历史或提供只读访问留待后续工作。

## 验证

定向测试覆盖单 Bot 与幂等设置、未配置无副作用、暂停／恢复／移除／归档删除记忆、目录缺失暂停、访客只读策略和旧数据丢弃。
按仓库规则只运行受影响 crate 的测试，不运行完整 Workspace 测试。

## Test coverage

未测量覆盖率：本次未请求覆盖率统计。
