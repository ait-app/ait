# ADR-131：metadata 按能力组组织模块

- 状态：接受
- 日期：2026-10-11
- 修订：[ADR-130](adr-130-workspace-methods-owned-by-metadata.md) 中 metadata 的内部模块路径；
  crate 名称、依赖、对外服务和方法集合不变。沿用
  [ADR-113](adr-113-filesystem-capability-groups.md) 的组织方式与边界约定。

## 背景

[ADR-130](adr-130-workspace-methods-owned-by-metadata.md) 将 worktree、恢复与 GitHub clone 迁入
metadata 后，crate 约 2 万行，仍按“层 → 功能区”组织（`service/directory`、
`rpc/worktrees`、`connection/push` 等）。同一能力的协议、服务、RPC 和连接代码分散在六个
层目录中；`connection/creation` 直接调用 directory 的 RPC，功能区之间的引用没有约定；
`rpc.rs` 同时承担模块声明和全 crate 共享的错误码。

## 决策

1. 目录改为“能力组 → 层 → 功能区”，即 `crate::<group>::<layer>::<area>`，组内沿用
   `ports`/`protocol`/`service`/`rpc`/`connection`/`local` 分层：

   | 组           | 功能区                                                                                                         |
   | ------------ | -------------------------------------------------------------------------------------------------------------- |
   | `server`     | server（info、ping、heartbeat）、session、creation、editor                                                     |
   | `daemon`     | daemon、diagnostics                                                                                            |
   | `push`       | push                                                                                                           |
   | `directory`  | directory、workspace_creation、project_config、project_icon、workspace_state、workspace_names、github_projects |
   | `labels`     | workspace_labels                                                                                               |
   | `automation` | workspace_automation                                                                                           |
   | `worktrees`  | worktrees、workspace_recovery                                                                                  |

   `github_projects` 的 clone 本质是 Project 登记，直接持有 `Directory`，归入 `directory` 组。

2. 顶层保留组装与跨组模块：`installation`、`capabilities`、`dispatch`、`connection`，以及
   实现 model 协作端口、组合 directory/automation/names 的 `collaboration`（原
   `service::workspace_collaboration`）。`support::error` 为组间共享的 `ErrorCode`（原
   `rpc::ErrorCode`），不依赖任何组或顶层模块。
3. 组间约定与 filesystem 相同：组与组之间只引用对方的 `ports` 和 `protocol`；只有顶层模块
   可以同时组合多个组的具体类型；组内测试可以构造其他组的具体类型作为夹具。
4. 为满足约定，`workspace.create.request` 的连接处理从 `connection::creation` 移到
   `directory::connection::workspace_creation`；`server::connection::creation` 只保留
   `creation.subscribe.request`。`Connection::session` 改为 crate 内可见，供
   `server::connection::session` 使用。
5. `crates/metadata/tests/module_boundaries.rs` 扫描源码，拒绝组之间对
   `service`/`rpc`/`connection`/`local` 的引用以及 `support` 对组或顶层模块的引用，
   并用合成样例验证检查本身。

## 后果与验证

单个能力的全部分层位于同一目录，跨组耦合由测试约束。外部调用路径随之变化，例如
`metadata::service::directory` 变为 `metadata::directory::service::directory`，
`metadata::ports::worktrees` 变为 `metadata::worktrees::ports::worktrees`，
`metadata::service::workspace_collaboration` 变为 `metadata::collaboration`；`Service`、
`Dependencies`、`dispatch`、`capabilities`、`connection::Connection` 不变。不保留旧路径转发。
协议方法、分发顺序、持久格式和运行行为不变，依赖图不变。

验证覆盖 metadata 全部单元测试与模块边界测试、API 与 daemon 的编译和 clippy，以及受路径
变化影响的 API 测试。
