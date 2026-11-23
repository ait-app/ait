---
id: ADR-237
status: accepted
date: 2026-11-11
amends:
  - providers/adr-206-opencode-acp-subagents.md
paths:
  - crates/provider
  - apps/mobile
---

# ADR-237：OpenCode 子会话终态只读核对

OpenCode ACP 的子会话事件可能晚于持久化完成记录，或在后台续接后缺失。此时 Ait 的
live 子项仍显示运行；ACP `session/list` 不携带执行结果，重放历史也不能证明当前执行已结束。

对已经由 ACP task 结果验证父子关系的子项，Ait 在列出子 Agent 时可只读查询 OpenCode
V2 的 `session_v2`。只读取 `parent_id`、`directory`、`time_busy`、`time_idle` 和
`idle_outcome`，并同时核对父身份、工作目录以及最后一次空闲时间不早于忙碌时间。
只有 `succeeded`、`failed`、`interrupted` 三种明确的持久终态会分别映射为
`completed`、`failed`、`canceled`。缺失数据库、旧版 schema、无终态或正在续接
时保留 ACP 观测，不按静默时长推断完成。数据库以只读模式打开，不迁移或修改。
数据库位置与 OpenCode 一致：`OPENCODE_DB` 的相对路径解析到 OpenCode 数据目录，
绝对路径原样使用，`:memory:` 不核对。

核对出的终态同时写回该父 Agent 的 live 子项跟踪器；此后父会话关闭时只把仍在运行的
子项标记为 `canceled`，不会覆盖已核对的终态。

客户端在存在运行中的已观测 OpenCode 子项时每 15 秒重新列取；定时器只随“是否存在
此类子项”变化重置，不受同级子项的流式更新影响。连接断开时仍按现行规则标记为
stale。执行、审批、取消、历史和关系证明继续由 ACP 拥有。该例外只用于终态核对。
