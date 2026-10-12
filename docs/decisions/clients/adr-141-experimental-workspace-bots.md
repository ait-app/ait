# ADR-141：客户端测试功能开关与 Workspace Bot 标签页

- 状态：已实现
- 日期：2026-10-12
- 关系：延续 [ADR-139](../bot/adr-139-workspace-bots-and-owned-channels.md)。

## 背景

Bot 后端已提供持久配置、对话和输入回执。客户端需要一个默认关闭、可以全局读取的测试功能开关，以及 Workspace 中的创建、对话和设置入口。

## 决策

- `AppSettings.experimentalFeatures` 由现有客户端设置存储及共享查询缓存持有，默认 `false`；缺失或无效的旧值按关闭处理。组件通过 `useAppSettings` / `useSettings` 读取，非组件通过 `loadAppSettingsFromStorage` 读取，不引入第二份状态。
- 开关位于“设置 → 关于 → 发布通道”下方。没有发布通道的客户端也在“关于”显示开关。
- 开启后 Workspace 新建菜单显示 Bot。关闭后隐藏入口，已恢复的 Bot 标签页显示启用提示，不发起 Bot 查询；关闭客户端开关不会暂停或删除服务器 Bot。
- Workspace 标签页以 `botId` 为持久身份，创建、列表、设置是同一类面板的视图；底层 Provider session 身份不进入客户端标签页。
- 共享 TypeScript 协议声明后端已有的 21 个 Bot 请求与响应，SDK 通过同一套校验、请求归属、错误处理和 Rust transport 调用。功能同时检查服务器 `workspace-bots-v1` 及完整方法清单；旧 Host 显示升级提示。
- 首个界面提供创建、列表、内置对话、审批、取消、新对话、配置、启停、共享记忆及归档。指令与执行配置遵循后端 generation 语义，新对话后生效；记忆更新使用 revision 校验，避免覆盖其他客户端修改。
- 对话使用持久输入回执观察最终回复。首次按页恢复历史，之后只刷新未完成回执及新增输入；轮询随面板活动状态停止。输入重试复用 `messageId`，关闭标签页不取消服务器任务。
- 外部 Channel 管理界面和流式输出继续作为后续功能；现有后端能力和所有权边界不变。

## 验证

定向测试覆盖设置默认值与持久化、标签页身份、协议校验、SDK 请求关联、增量回执及历史分页；浏览器流程使用隔离的 Rust daemon 与离线 Codex Provider，检查创建、对话、设置、记忆和刷新恢复，以及原生权限问答与记忆并发修改冲突。未改动 Rust 代码，按仓库规则不运行 Rust 测试。

## Test coverage

未测量覆盖率。受影响 TypeScript 测试共 194 项通过（客户端应用 191 项、协议 2 项、SDK 1 项）；两个 Bot 浏览器流程通过。SDK 构建、应用类型检查、格式、lint、文档链接检查及离线 daemon 构建通过。
