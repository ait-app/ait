---
id: ADR-227
status: accepted
date: 2026-11-10
amends:
  - bot/adr-213-single-workspace-manager-bot.md
paths:
  - crates/bot
  - crates/provider/runtime
  - apps/mobile
---

# ADR-227：Bot 按消息身份保留 Provider 的多条回复

- 扩展：[ADR-213](adr-213-single-workspace-manager-bot.md) 的 Bot 执行与对话回执模型。

## 背景

Bot 过去只从 Provider session 的 `lastMessage` 生成一条回复。一次执行中可能包含多条有意义的 assistant 消息，中间还会出现 tool call、reasoning 和其他内部记录；只取最后一条会丢失上下文。

把一次执行的所有文本简单拼成一条消息也会破坏消息边界。Provider 的流式增量属于同一个 `messageId` 时应更新同一条消息；出现新的 `messageId` 时应立即创建独立消息。

## 决策

1. Provider timeline 是 Bot 输出的唯一投影来源。Bot 在发送输入前记录当前 timeline generation 和序号，轮询时只读取该游标之后的 `assistant_message`，过滤 tool call、reasoning 和其他记录。
2. timeline projection 按 `messageId` 合并同一条消息的连续流式片段；不同 `messageId` 保持独立，并按 Provider 顺序返回。没有稳定身份的记录使用其 timeline 序号作为确定性 ID。
3. Bot 的 `Turn` 增加 `messages`，每项包含 `id`、当前完整 `text` 和 `streaming` / `completed` 状态。轮询得到的新消息立即持久化，内置对话在运行期间显示这些更新；完成时所有消息标记为 `completed`。
4. `reply` 保留为兼容字段，值为最新 assistant 消息。新客户端和外部 Channel 使用 `messages`，逐条展示或发送；旧记录没有 `messages` 时回退到 `reply`。
5. Provider 不支持游标轮询的旧执行器继续走 `Status` 和 `lastMessage` 兼容路径，并生成一条确定性消息，保证升级后已有适配器仍可运行。

## 后果

- 一次 Bot 执行可以在 conversation 中显示多条 assistant 消息，且同一条流式消息不会重复创建气泡。
- 外部 Channel 按独立消息逐条发送，并在长文本切分时保持每个分段的幂等键。
- `Turn.messages` 是新的持久化字段，使用默认空数组兼容旧数据；`reply` 在过渡期继续对外提供。
- Bot 仍通过已提交的轮询快照更新状态，不新增 Provider 公共事件订阅；轮询间隔决定可见更新的最大延迟。
