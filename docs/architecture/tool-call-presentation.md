# 工具调用展示

## 归属与入口

依据 [ADR-285](../decisions/clients/adr-285-shared-tool-call-presentation.md)，
[`paseo-tool-presentation`](../../packages/protocol/src/paseo-tool-presentation/index.ts)
统一提供 Paseo 工具调用的展示语义。客户端通过
`@ait/protocol/paseo-tool-presentation/index` 导入函数和类型；组件只负责布局和交互，
可传入已知的目标 agent 标题，不维护独立的工具名称、摘要或详情排序映射。

[`catalog.ts`](../../packages/protocol/src/paseo-tool-presentation/catalog.ts) 是每个工具的
label、context 和 detail spec 的唯一来源。共享详情 formatter 负责字段标签、值格式化、
MCP 结果解包和未知字段 fallback。新增工具在 catalog 中登记；测试与共享实现放在一起。
非 Paseo 工具的专用渲染器不受此映射约束。

## 保留的详情行为

输入字段先按工具的 `inputOrder` 排列，未排名字段按键名字母序排列；没有指定顺序时全部按字母序。
prompt 字段单独展示。`outputFields` 同时指定结果字段的顺序和白名单；未设置时保留所有结果字段。
MCP 结果优先读取 structuredContent，其次解包单个 text 内容。字段和值继续使用原有格式化规则，
不增加结果截断、分页、空白过滤或输出投影。

整理展示代码时保留当前共享 catalog 的顺序：agent 输入的 `internal` 位于 `notifyOnFinish` 之后、
`settings` 之前；`browser_list_tabs` 使用共用的 browser 字段顺序。工具名称、摘要与 browser 条目保持原状。
折叠行仍通过格式化后的详情判断是否存在可见内容，加载状态规则保持原状。
