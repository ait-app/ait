---
id: ADR-214
status: accepted
date: 2026-11-06
amends:
  - clients/adr-141-experimental-workspace-bots.md
paths:
  - apps/mobile
---

# ADR-214：客户端 Workspace Bot 标签页 v2

- 关系：修订 [ADR-141](adr-141-experimental-workspace-bots.md) 的标签身份与界面范围；延续 [ADR-213](../bot/adr-213-single-workspace-manager-bot.md)。
- 后续修订：[ADR-215](../bot/adr-215-bot-permissions-and-scoped-mcp.md) 在向导与设置中增加权限与模式上限；[ADR-221](../bot/adr-221-read-only-bot-session-inspection.md) 将对话界面对齐会话页并列出内部会话。
- 计划：[阶段 2 实施计划](../../archive/plans/workspace-bot-manager-phase-2.md)、[Workspace Bot 管理者重设计](../../plans/workspace-bot-manager.md)。

## 背景

ADR-213 把 Bot 改为每个 Workspace 至多一个、以 `workspaceId` 寻址的管理者，并以 `workspace-bots-v2` 取代 v1。
ADR-141 的客户端仍以 `botId` 标识标签页，提供创建、列表和人格编辑，无法使用新接口：早于本次变更的客户端连接 0.1.30 起的 Host（只支持 v2）时无法使用 Bot，只显示升级提示。

## 决策

1. **能力门控。** `workspaceBots` 要求服务器声明 `workspace-bots-v2` 且完整 Bot 方法清单可用；只有 v1 的旧 Host 显示 `bots.unsupported`。
   客户端请求一律以 `workspaceId` 寻址，客户端类型中不含 `botId`、`name`、`instructions`、`enabled`、`archivedAt`。
   实验开关行为沿用 ADR-141：关闭时隐藏新建入口，已恢复的 Bot 标签页显示 `bots.disabled` 且不发起请求，不影响服务器上的 Bot。
2. **单例标签。** 每个 Workspace 至多一个 Bot 标签页，标签 ID 固定为 `bot`，目标为 `{ kind: "bot" }` 或 `{ kind: "bot", view: "settings" }`。
   读取旧持久化目标（含 `botId` 及 `list`／`create`／`settings` 视图）时归一化：`view: "settings"` 保留为设置视图，其余变为默认视图；
   旧标签 ID 统一收敛为 `bot`；整个布局（跨所有窗格）只保留按窗格顺序的第一个 Bot 标签，其余移除，
   窗格焦点指向保留的旧 Bot 标签时落到 `bot`，失去焦点标签的窗格沿用常规焦点回退。
3. **界面状态。**
   - 未配置（`bot.get` 返回 `null`）：设置向导，选择 Provider、模型、权限模式与推理强度，可选“清空已保存的记忆”；访客只看到未配置提示。
     设置时若其他设备已配置 Bot（`BotConflict`），客户端立即刷新 `bot.get` 并切换到已有 Bot。
   - 已配置：对话或设置视图。`bot.get` 刷新失败时两个视图都显示错误与重新加载。对话显示轮次状态与错误。
   - 暂停（含 `directory_missing`，会显示原因说明）：对话历史只读可见，客户端以 `bot.conversation.list` 读取内置会话而不调用
     `open`（`open` 只接受 active Bot）；输入框停用，有管理权限者可恢复，恢复后无需重新打开标签页即可发送。
     记忆保存、Channel 新增与启停同样停用并提示先恢复 Bot（`bots.resumeToEdit`）；删除 Channel 与修改模型不受影响。
   - 设置视图：更新模型（对新对话生效）、暂停／恢复、带 revision 的记忆编辑、Telegram 与飞书 Channel 管理、确认后移除。
     Channel 凭据只接受 `env:AIT_SERVER_CREDENTIAL_*` 环境变量引用，并配置允许的发送者；新建 Channel 默认停用；
     同一次打开的表单重试复用同一 `channelId`，避免重复创建。
   - 对话与 Channel 的查询缓存键包含 Bot 的 `builtinChannelId`：Bot 在任一设备被移除并重新设置后，不会显示已退役 Bot 的对话或 Channel。
     记忆属于 Workspace、在重新设置后保留，只有设置时勾选清空记忆才丢弃其缓存。
   - 消息 16 KiB、记忆 32 KiB 的客户端校验不变。
4. **访客只读。** 没有后台任务管理权限的成员可查看对话与设置，但看不到向导提交、发送、暂停、恢复、移除、模型保存、记忆保存和 Channel 编辑入口。
5. **文案。** Bot 文案只在 `en.ts` 与 `zh-CN.ts` 维护，其他语言复用英文。

## 后果

- 旧 Host（只支持 v1）显示升级提示；新客户端不再创建或显示多 Bot 与人格。已保存的旧 Bot 标签页自动迁移，无需用户操作。
- 本阶段不含：权限与 Agent 启动默认值（阶段 3，向导将增加这些选项）、通知唤醒展示与在 Ait 中查看 Telegram／飞书会话（阶段 4）、退役 Bot 历史查看。
- 浏览器端到端测试 `apps/mobile/e2e/browser/workspace-bots.spec.ts` 重新启用，覆盖设置与对话主路径，以及暂停后重新加载仍可读历史、恢复后继续发送。
- 权限请求仍按 ADR-145 自动拒绝；客户端不提供审批入口。

## 验证

按仓库规则只运行受影响包的测试：协议与 SDK 能力门控、移动端 Bot 标签页组件、标签归一化与布局存储、浏览器端到端用例；不运行完整 Rust Workspace 测试。

## Test coverage

未测量覆盖率：本次未请求覆盖率统计。
