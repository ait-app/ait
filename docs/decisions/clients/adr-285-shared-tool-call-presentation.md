---
id: ADR-285
status: accepted
date: 2026-11-16
paths:
  - packages/protocol/src/paseo-tool-presentation
  - apps/mobile
---

# ADR-285：Paseo 工具调用展示由协议包统一拥有

## 背景

工具调用的时间线展示与展开详情曾维护独立的工具映射。合并时已发现 agent 的
`internal` 字段排名和 `browser_list_tabs` 的字段顺序不同；复制映射会让客户端展示逐渐偏离。

## 决策

`packages/protocol/src/paseo-tool-presentation` 拥有与 UI 框架无关的 Paseo 工具调用展示规则。
`catalog.ts` 是每个工具的名称、摘要上下文及详情规格的唯一映射；共享 formatter 负责字段标签、
值格式化和 fallback。客户端通过 `@ait/protocol/paseo-tool-presentation/index` 消费展示函数和类型，
只负责布局、交互及提供已知的目标 agent 标题，不得维护同一用途的工具映射或字段排序表。

新增工具或调整展示时修改共享模块；行为约束和例外写入[现行规格](../../architecture/tool-call-presentation.md)，
相关回归测试放在该模块。此边界不改变工具执行、协议 payload 或非 Paseo 工具的专用渲染器。

## 后果

当前展示消费者位于 `apps/mobile`，用于移动端和 Web；桌面打包该 Web 构建，
`apps/desktop` 本身没有独立的工具调用渲染器。内部文件重命名不要求消费者更换导入路径。
新增客户端也需要依赖共享展示结果；平台可以改变布局，但不能自行覆盖工具名称或详情顺序。
