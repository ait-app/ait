---
id: ADR-276
status: accepted
date: 2026-11-13
amends:
  - bot/adr-271-bot-notification-inbox-and-wake-turns.md
paths:
  - crates/domain/src/input_envelope.rs
  - crates/domain/src/lib.rs
  - bins/daemon/src/host/schedule.rs
  - crates/bot/src/instructions.rs
  - crates/provider/runtime/src/timeline_projection.rs
  - crates/provider/runtime/src/storage/timeline.rs
  - crates/provider/runtime/src/service/agent_manager/titles.rs
  - packages/protocol/src
  - apps/mobile/src/types
  - apps/mobile/src/components/message.tsx
  - apps/mobile/src/runtime/replica-cache
---

# ADR-276：Ait 生成的输入使用统一信封并在投影中拆出 origin

- 关系：修订 [ADR-271](../bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 11 节描述的
  Bot 指令与唤醒输入的文本形式；定时任务运行提示词的包装随之统一，调度语义不变。

## 背景

Ait 会代替人写入 Agent 输入：定时任务与心跳的每次运行、Bot 指令前缀和 Bot 唤醒通知。
这些输入此前各自使用一种标签（`<ait-system>`、`<ait-bot-instructions>`、`<ait-notifications>`），
或者完全没有标记（新 Agent 目标的定时任务），并且定时任务提示词不转义，提示词中出现
`</ait-system>` 会提前闭合信封。provider 时间线把这些标签当作普通 `user_message` 文本，
客户端因此显示原始 XML，会话标题也取到标签开头；而 Claude、Codex、OMP 的原生信封已有
各自的解析，说明结构化投影可行，但 Ait 自己的输入没有入口。

## 决策

1. **唯一信封**：`domain::input_envelope` 负责写入与解析。格式为
   `<ait-input kind="schedule|bot_wake|bot_instructions" …>`；定时任务额外带
   `schedule-id`、`run-id` 与可选 `name`。属性值做 `&`、`"`、`<`、`>` 转义，
   正文把 `</` 写成 `<\/`，因此调度提示词与通知文本都无法闭合信封。
2. **解析只在投影层发生一次**：provider runtime 在写库前的发布路径
   （`storage/timeline.rs` 的 `publish`）、读取投影（`timeline_projection.rs` 的
   `normalize_item`）与会话标题（`service/agent_manager/titles.rs`）调用同一个
   `ait_input` 投影：把 `text` 换成信封内的正文，并附加
   `origin: { kind, scheduleId?, runId?, name? }`。存储行保持发送时的原文，展示与索引使用投影。
   信封必须从开头覆盖整条输入（Bot 指令除外，其后是用户正文），引用标签的普通消息不受影响。
3. **兼容旧记录**：解析器同时识别 `<ait-system>`、`<ait-bot-instructions>`、
   `<ait-notifications>` 三种旧形式，因为 provider 原生会话里已经存在这些文本。
4. **协议与客户端**：`user_message` 增加可选 `origin`（`packages/protocol`）；客户端按
   `origin` 显示“Schedule … · run …”“Ait notifications”“Sent by Ait”等来源行，正文只渲染投影后的文本。
   旧客户端忽略未知字段，旧 daemon 不下发该字段。

## 后果

Ait 写入的输入在模型侧有稳定、不可注入的边界，在客户端与时间线搜索、会话标题、fork 上下文中
显示为人的消息加来源说明；新增一种 Ait 输入只需增加一个 `kind`，不新增解析器。存储行仍保留原始
信封，重放和原生历史导入得到同样投影，但依赖 `<ait-system>` 等旧标签的读取方必须改用
`domain::input_envelope` 或投影后的 `origin`。定时任务新 Agent 目标仍发送裸提示词，使其中的
slash 命令继续生效，因此该路径不在信封内。
