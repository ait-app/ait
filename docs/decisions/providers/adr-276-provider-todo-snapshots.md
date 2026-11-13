---
id: ADR-276
status: accepted
date: 2026-11-13
paths:
  - crates/provider/native
  - crates/provider/deepseek-harness
  - crates/provider/opencode
  - crates/provider/omp
  - crates/provider/pi
  - crates/provider/claude
  - crates/provider/antigravity
  - crates/provider/runtime
  - packages/protocol/src/messages.ts
  - apps/mobile/src/types/stream.ts
  - apps/mobile/src/timeline/session-stream-reducers.ts
  - apps/mobile/src/stores/session-store.ts
  - apps/mobile/src/composer/task-list
---

# ADR-276：Provider 原生 TODO 统一为完整快照

## 背景

Claude、Codex 和 OMP 已通过 Timeline 的 `todo` 项投影原生任务；客户端也兼容
`TodoWrite`、`todo_write` 与 `update_plan` 工具。ACP 的标准 `plan`、DSH Host 的
`todo/write` 尚未接入同一通道，空列表与历史窗口也可能留下旧任务。若界面继续识别
Provider 名称或各自的任务格式，新 Provider 就需要重复修改客户端。

## 决策

1. 原生 Provider 继续拥有任务及写入行为。adapter 将原生任务映射为已有的
   `{"type":"todo","items":[...]}` 完整快照；没有事件表示没有新信息，`items:[]`
   明确清空列表。Ait 不增加独立任务数据库、不注入模拟 TODO 工具、不从普通文本推断任务。
2. 项目使用已有 `text`、`completed`、可选 `id`、`status` 与 `activeForm` 字段；
   状态为 `pending`、`in_progress`、`completed`，兼容只有 `completed` 的旧快照。
   新增可选 `group:{id,title}`，保留 OMP 原生阶段；阶段索引作为分组身份，重名阶段仍分开。
   adapter 的实时与历史路径复用投影规则，任务显示不依赖 Provider 白名单。
3. ACP v1 的 `sessionUpdate:"plan"` 由 `provider-native::todo::snapshot` 统一转换，
   共享 ACP adapter 和 OpenCode 的独立 ACP stream 复用它。转换接受完整替换和空列表，
   并限制项目数、单项文本字节与最终 JSON 字节；不完整、超限或未知状态按原生协议错误处理。
4. DSH Host 的原生 `todo/write`、OMP 的原生 phases、Pi 官方 TODO 扩展的成功
   `todo` 结果 `details.todos` 使用相同快照语义；各自保持原生事件识别。
   Pi 的任务 ID 保留，失败结果不改变任务。新私有协议仍需在 adapter 中明确映射，原生程序安装了
   任务扩展并不自动证明其传输已经暴露 Ait 可消费的结构化任务。
5. 客户端在接受事件的序列检查后更新当前快照；兼容工具路径保留状态与 active form，
   并更新相同的当前任务存储。历史 hydration 必须保留空快照；尚有更新页的旧窗口不能
   覆盖当前任务，epoch 改变时失效旧快照。Timeline 响应携带独立的当前 `taskSnapshot`
   （epoch、源序号、任务列表），从同一持久投影读取，保证任务早于当前聊天页时也能恢复。
   `null` 表示当前代没有任务记录，缺省字段兼容旧 daemon；源序号防止旧响应覆盖更新的实时任务。
   既有大型记录无法容纳重复快照时，以 `{epoch,seq,entrySeq}` 引用同页的规范 TODO；引用缺失
   或 epoch 不匹配不能清空任务。超大任务和独立大记录无法同时装入既有响应预算时省略可选快照，
   保持历史可读，不能将其当作无任务。
6. 桌面、Web 与移动端复用现有 composer track：非空快照显示 `TODO 已完成/总数`
   胶囊，点击打开 popover 或紧凑屏 sheet。详情显示全部任务文字；清空后移除胶囊。
   原生阶段作为带进度的分组标题，任务在其下缩进。优先保留文本显式编号，对无编号条目生成
   从 1 开始的位置序号；原生 ID 只用于身份，不把 Codex 等 adapter 的零基内部 ID 当作序号。
   仅当文本中的显式父编号存在时添加大纲缩进，不从 ID
   或依赖字段推断父子关系，不拆分一个原生任务为多个可完成子任务。Claude 保留原生列表与
   创建顺序，避免字符串 ID 排序打乱任务。OMP 的未完成提醒不覆盖已知的完整 phases 快照。

## 后果

标准 ACP Provider 可以直接复用映射；其他 Provider 一旦输出规范快照，同一存储、胶囊和
卡片即可展示，无需客户端分支。能力承诺取决于原生传输实际提供的结构化事件，不能仅凭
Provider 宣称支持任务推断已完成适配。任务修改仍由模型使用原生工具完成，卡片只读。

现行支持范围见 [Provider 规格](../../architecture/provider.md#原生-todo)。
