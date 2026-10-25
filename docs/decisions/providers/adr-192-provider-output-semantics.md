# ADR-192：Provider 输出分类与每轮 Thinking 展示

- 状态：Accepted，2026-10-25。
- 范围：Provider 原生投影、OMP ACP 桥接与共享客户端展示；沿用 ADR-190 的所有权边界。

## 背景

OMP 将后台任务交付保存为显式可见的 `async-result` 自定义消息。
此前 Ait 将其统一投影成助手正文，导致给模型的继续执行指令与长命令日志直接出现在回复区。
客户端在基线同步前还会把逐个推理片段拆成独立行；已有连续片段合并规则也不能保证
同一轮跨工具调用只有一个 Thinking。

## 决策

- 原生角色和事件类型由 Provider adapter 解释。工具输出进入共享 `tool_call`，
  通知进入共享 `notification`；客户端不根据英文前缀、路径或插件名称猜测消息用途。
- OMP 原生 `async-result` 进入可展开的 Background job 工具结果。
  单任务使用会话内原生 jobId 作为调用身份，缺少元数据和批次消息沿用各自原生消息身份。
  原生执行状态控制工具状态，工具失败不等同于 Provider 或整轮失败。
  模型继续指令和原始日志保留在详情中，原生上下文及会话文件仍由 OMP 持有。
- Extension notify 保留通知级别，不覆盖已有回复摘要。
  纯本地命令只有通知时仍可返回该命令结果。标准 ACP 不提供独立通知类型，
  因此桥接成独立 Provider notification 工具条目，避免丢失内容或冒充助手文本。
- 明确可见的普通插件内容继续尊重原生 display 标记；不将未知自定义消息当作后台任务丢弃。
  Codex 命令结果、Claude tool result、OpenCode ACP tool update 和 Pi bashExecution
  继续沿用各自 adapter 的工具投影。Pi 的普通可见自定义消息仍按插件展示内容处理。
- 原生实时事件与历史读取共用投影。旧正文形式的后台结果经现有 reconcile 更换 Timeline
  generation，保留时间戳与回执，不直接修改用户 SQLite 或 OMP 原生存储。
- 每个 Agent 内，同一显式 turnId 只展示一个 Thinking，身份由 turnId 决定，
  跨历史/live 边界、工具调用、重新打开和分页保持稳定。推理段落按来源顺序合并，
  原始时间线及工具条目保持独立。缺少 turnId 的旧数据只合并连续推理，不能跨已知轮次。
- 历史基线尚未就绪时，实时覆盖区仍是连续的流式投影；不能为每个 delta 重建空 head。

## 验证与 Test coverage

代码基线为 `0ca6889a` 加本次变更，Rust 默认 features。
定向检查覆盖 OMP 实时/历史身份、旧缓存替换、后台任务状态与长输出、通知的 ACP 兼容、
每轮推理分组、混合旧数据、分页边界、重开及基线前的批量增量。
具体通过数量在交付说明中单独记录，不将测试数量当作覆盖率。

**not measured**：用户未要求行覆盖率。需要时使用 `cargo llvm-cov`，仅测量
`provider` 的 `local::omp` 与对应客户端测试；本次未生成覆盖率工件。
没有运行完整 Rust workspace 测试。已安装 CLI 与付费模型场景沿用原有忽略配置。
