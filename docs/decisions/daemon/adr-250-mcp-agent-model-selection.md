---
id: ADR-250
status: accepted
date: 2026-11-12
amends:
  - daemon/adr-148-daemon-mcp.md
  - workspace/adr-138-workspace-owned-schedules.md
paths:
  - crates/api/src/mcp.rs
  - crates/api/src/mcp
  - crates/api/src/tests/mcp
  - crates/schedule
  - packages/protocol/src/paseo-tool-presentation
  - packages/protocol/src/schedule
  - skills/ait-orchestration
---

# ADR-250：MCP Agent 创建与定时任务更新的 Provider 和模型字段

- 修订：[ADR-148](adr-148-daemon-mcp.md) 的 `create_agent` 与 `update_schedule` Provider 和模型契约，增加独立字段并兼容旧合并格式。
- 扩展：[ADR-138](../workspace/adr-138-workspace-owned-schedules.md) 的定时任务配置写入契约，明确裸 Provider、模型补丁、模式选择与历史记录兼容规则。

## 背景

模型发现分别返回 Provider 与模型 ID，模型 ID 本身可能包含命名空间和斜杠。
要求调用者拼接 `provider/model` 容易遗漏模型，并导致创建失败。

## 决策

- `create_agent` 增加可选顶层 `model`，推荐与裸 `provider` 一起使用。两种格式均去除 Provider 与模型 ID 的首尾空白，保留模型 ID 内部的命名空间和斜杠。
- Provider ID 当前不得包含 `/`；该字符保留为旧合并格式的分隔符。现有适配器均满足此限制，
  将来若引入带斜杠的 Provider ID，须先修订格式判别契约。模型 ID 不受此限制。
- 为兼容旧客户端，省略 `model` 或设为 `null` 时继续解析 `provider/model`，只按第一个斜杠拆分。
- 显式字符串 `model` 不得与合并格式混用，即使两处模型相同也拒绝；空字符串、空白模型和缺少模型均拒绝，
  不推断默认模型。参数错误提供发现与重试指导。
- API 创建转换与 Bot 模式上限校验使用同一选择解析。作用域、权限、RPC 与持久配置格式不变。
- `update_schedule` 暴露独立的可选 `model` 补丁：省略且 Provider 不变时保留已存模型；切换 Provider 且未指定模型时清除旧模型，使用新 Provider 默认值（合并的 `provider/model` 可更新模型）。
  非空字符串去除首尾空白后替换，`null` 清除模型并使用 Provider 默认值；空串和纯空白拒绝。
  与 `create_agent` 不同，合并格式可与相同的独立模型同时使用；两处模型在去除首尾空白后必须相同，
  合并模型与 `model: null` 冲突并拒绝。模型可用性由运行时 Provider 判断，保存时不以发现列表作为白名单。
- 定时任务的新 Agent 配置切换 Provider 时，必须显式提供 `modeId`，否则拒绝保存，避免静默提升权限；显式 `null` 表示选择新 Provider 原生默认模式。
  Bot 作用域调用仍由权限上限写入新 Provider 的默认模式。补丁中未提供的 `thinkingOptionId` 清除。
  Provider 补丁在比较和保存前去除首尾空白；将历史合并值修复为同一裸 Provider 时保留内嵌模型（已有独立模型或显式模型补丁优先），不视为 Provider 切换。
- 定时任务模型实际改变时，清除补丁中未显式提供的 `thinkingOptionId`；显式提供则按补丁更新。
  未存模型时再次清除模型属于无变化，保留 thinking 选项。此规则适用于原生接口和 MCP。
- 原生接口对新建和完整替换 `target` 要求裸 Provider ID；原样回传历史合并值也会拒绝。
  历史记录仍可加载，修改 prompt、cadence 等非 Agent 配置字段不会迁移配置；任何 `newAgentConfig` 补丁都会先将已存合并值规范化为裸 Provider 与独立模型，已有独立模型优先，再应用补丁。
  因此仅修改 model 或 cwd 也会修复历史 Provider；模型变化及 thinking 选项清除规则以规范化后的模型为基准。
- `create_schedule` 继续使用合并的 `provider/model`，不增加独立 `model`；其他工具保持现有契约。
  工具目录与编排 Skill 同步说明创建和更新的差异。

## 后果

调用者可以直接复制发现结果。旧客户端的 `create_agent` 与合并格式调用继续工作，但
`update_schedule` 不再将空串或纯空白 `model` 视为清除，调用者须改传 `model: null`。
新客户端连接旧服务器时须依据实时 Schema 使用旧合并格式。定时任务更新允许独立替换或清除模型，
调用者须区分创建与补丁的 `null` 和混用语义。
回归覆盖两种输入、带斜杠的模型 ID、混用拒绝与模式上限，以及定时任务的省略、替换、清除与冲突补丁。
