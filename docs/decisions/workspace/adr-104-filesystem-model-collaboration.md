# ADR-104：Filesystem 仅通过 model 契约协作

- 状态：接受
- 日期：2026-10-07
- 关联：[ADR-102](../providers/adr-102-provider-metadata-independence.md)、[ADR-103](../daemon/adr-103-terminal-model-dependency.md)

## 背景

filesystem 是剩余直接依赖 metadata 的功能 crate。除了已迁入 model 的记录、Registry、协议和事件，
它还使用目录检查、Git/Forge 快照与观察、摘要消费和分支命名接口，以及身份和 descriptor 纯函数。
GitHub clone 后登记 Project、worktree 创建后命名和启动 setup 则直接持有 metadata 的业务服务。

## 决策

1. filesystem 的共享记录、Registry、worktree 契约、连接事件和协议直接从 model 导入；测试亦然。
   文件 Registry 的更新、归档和恢复仍使用宿主注入的共享资源。
2. `DirectorySource`、`Checkout` 和项目配置文件名归 `model::workspace::provisioning`；
   Workspace Git/Forge 快照与观察归 `model::workspace::runtime` 和 `git`。
   metadata 的配置与图标存储接口仍留在 metadata。
3. `SummarySource` 归 `model::summary`，`WorkspaceBranchNamer` 和 first-Agent 输入编码归
   `model::workspace::naming`；生成器继续归 provider，命名协调、资格判断和写回继续归 metadata。
4. 项目分组、remote 解析等纯身份函数归 `model::workspace::identity`，持久记录到协议的纯投影归
   `model::workspace::protocol::projection`。Workspace ID 统一使用 model 已有生成器。
   metadata 的旧共享路径显式重导出这些定义，保留调用兼容性且不复制实现。
5. `ProjectRegistration` 定义于 `model::workspace::lifecycle`，由 metadata 的 Directory 实现，
   filesystem 的 GitHubProjects 注入该接口。登记失败继续返回已完成的 checkout，不删除它。
6. Worktrees 注入已有 `WorkspaceNaming`；创建后 setup 使用已有 `WorkspaceSetup`。
   API 包装原有 `Arc<Mutex<WorkspaceAutomation>>`，保持相同的任务所有者、信任检查与锁。
   `Runtime::run_shared` 在原有阻塞任务、单个 job 预算、admission、取消和跟踪机制下执行接口，
   已有 `run` 复用该入口，不给业务服务增加新的外层锁。setup 失败仍不撤销成功创建。
7. 删除 filesystem 的 metadata Cargo 依赖，包括开发依赖。依赖守卫检查普通、开发、构建、
   optional、目标平台和重命名边，以及所有 Rust 测试源码。

## 后果

filesystem 的唯一内部依赖为 model，功能 crate 之间没有直接依赖。
宿主/API 协调现有服务，metadata 保留 Project/Workspace 的目录、命名、配置与自动化业务，
filesystem 保留物理目录、Git、Forge 和 worktree 行为。端口迁移没有引入第二套 Registry、
摘要队列、事件通道或 setup runtime。

本地措辞填充模块由 `dispatch::metadata` 改名为 `dispatch::summary`，明确其摘要消费职责。
跨服务行为通过 metadata 的登记适配器测试与 daemon 的 clone/worktree/setup 集成用例验证，
filesystem 单元测试只依赖 model 接口和测试替身。
