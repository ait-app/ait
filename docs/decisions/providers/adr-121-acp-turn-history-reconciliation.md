# ADR-121：ACP 轮次关联与原生历史结算

- 状态：Accepted，2026-10-10。
- 范围：Provider 会话事件、OpenCode ACP adapter 与既有 timeline 存储；补充 [ADR-115](adr-115-opencode-acp-provider.md)。

## 决策

OpenCode 的 ACP 实时输出可能不回显用户消息。实时到达顺序因此不能代表完整会话顺序。
提交成功后先发布有界的输入展示条目，沿用 Paseo 的 submitted user message 行为，
使运行中重新打开会话也能显示输入。条目使用明确的 `submitted:` 身份，由完整原生历史替换。
Adapter 在原生 prompt 完成后通过 `AgentTurnEvent::History` 交付完整原生重放；
应用层先调用现有 timeline reconcile，事务提交后才结算轮次。
普通单项完成事件与其他 Provider 的接入方式保持不变。

Reconcile 同时检查已完成条目和实时进度中，轮次初始输入是否先于本轮回复。
当原生用户消息位于已经发布的回复之前时，替换展示 generation；已有顺序正确时保留游标。
替换时保留原生条目的首次观察时间，用户输入通过 client message ID 保留提交时间。
原生历史仍由 OpenCode 所有，不在项目或原生数据库补写用户消息。

每次接受的输入用 client message ID（未提供时生成 UUID）关联 ACP prompt、实时输出和轮次状态。
结算后沿用持久化句柄中的原生用户 ID → 输入 ID 映射，将该用户消息及其回复归入同一轮次。
原生 message ID 和 tool call ID 保持不变；UI 使用统一的工具身份，不增加 Provider 专用排序逻辑。

## 验证

覆盖未回显与回显用户消息、连续输入、重放与恢复，以及应用层先提交历史再发布终态。
真实 OpenCode 1.18.4 / 2.0.26 验证使用隔离 XDG 目录和本地模型服务。
