# ADR-214：客户端 Workspace Bot 标签页 v2

- 状态：已实现
- 日期：2026-11-06
- 关系：修订 [ADR-141](adr-141-experimental-workspace-bots.md) 的标签身份与界面范围；延续 [ADR-213](../bot/adr-213-single-workspace-manager-bot.md)。
- 计划：[阶段 2 实施计划](../../plans/workspace-bot-manager-phase-2.md)、[Workspace Bot 管理者重设计](../../plans/workspace-bot-manager.md)。

## 背景

ADR-213 把 Bot 改为每个 Workspace 至多一个、以 `workspaceId` 寻址的管理者，并以 `workspace-bots-v2` 取代 v1。
ADR-141 的客户端仍以 `botId` 标识标签页，提供创建、列表和人格编辑，无法使用新接口，旧 Host 之外的 Host 都会显示升级提示。

## 决策

1. **能力门控。** `workspaceBots` 要求服务器声明 `workspace-bots-v2` 且完整 Bot 方法清单可用；只有 v1 的旧 Host 显示 `bots.unsupported`。
   客户端请求一律以 `workspaceId` 寻址，客户端类型中不含 `botId`、`name`、`instructions`、`enabled`、`archivedAt`。
   实验开关行为沿用 ADR-141：关闭时隐藏新建入口，已恢复的 Bot 标签页显示 `bots.disabled` 且不发起请求，不影响服务器上的 Bot。
2. **单例标签。** 每个 Workspace 至多一个 Bot 标签页，标签 ID 固定为 `bot`，目标为 `{ kind: "bot" }` 或 `{ kind: "bot", view: "settings" }`。
   读取旧持久化目标（含 `botId` 及 `list`／`create`／`settings` 视图）时归一化：`view: "settings"` 保留为设置视图，其余变为默认视图；
   旧标签 ID 统一收敛为 `bot`，同一窗格内多个旧 Bot 标签合并为一个，窗格焦点指向旧 Bot 标签时落到 `bot`。
3. **界面状态。**
   - 未配置（`bot.get` 返回 `null`）：设置向导，选择 Provider、模型、权限模式与推理强度，可选“清除已保存记忆”；访客只看到未配置提示。
   - 已配置：对话或设置视图。暂停时输入框停用，有管理权限者可恢复；`directory_missing` 显示原因说明；对话显示轮次取消原因。
   - 设置视图：更新模型（对新会话生效）、暂停／恢复、带 revision 的记忆编辑、Telegram 与飞书 Channel 管理、确认后移除。
     Channel 凭据只接受环境变量引用，并配置允许的发送者；新建 Channel 默认停用。
   - 移除后重新设置会丢弃该 Workspace 缓存的对话、记忆与 Channel，避免显示已退役 Bot 的内容。
   - 消息 16 KiB、记忆 32 KiB 的客户端校验不变。
4. **访客只读。** 没有后台任务管理权限的成员可查看对话与设置，但看不到向导提交、发送、暂停、移除、记忆保存和 Channel 编辑入口。
5. **文案。** Bot 文案只在 `en.ts` 与 `zh-CN.ts` 维护，其他语言复用英文。

## 后果

- 旧 Host（只支持 v1）显示升级提示；新客户端不再创建或显示多 Bot 与人格。已保存的旧 Bot 标签页自动迁移，无需用户操作。
- 本阶段不含：权限与 Agent 启动默认值（阶段 3，向导将增加这些选项）、通知唤醒展示与在 Ait 中查看 Telegram／飞书会话（阶段 4）、退役 Bot 历史查看。
- 浏览器端到端测试 `apps/mobile/e2e/browser/workspace-bots.spec.ts` 重新启用，覆盖设置与对话主路径。
- 权限请求仍按 ADR-145 自动拒绝；客户端不提供审批入口。

## 验证

按仓库规则只运行受影响包的测试：协议与 SDK 能力门控、移动端 Bot 标签页组件、标签归一化与布局存储、浏览器端到端用例；不运行完整 Rust Workspace 测试。

## Test coverage

未测量覆盖率：本次未请求覆盖率统计。
