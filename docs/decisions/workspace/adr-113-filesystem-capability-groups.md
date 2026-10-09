# ADR-113：filesystem 按能力组组织模块

- 状态：接受
- 日期：2026-10-09
- 修订：[ADR-030](adr-030-daemon-filesystem.md) 与
  [ADR-104](adr-104-filesystem-model-collaboration.md) 中 filesystem 的内部模块路径；
  crate 名称、依赖和对外服务不变。

## 背景

filesystem 约 3.5 万行，按“层 → 功能区”组织（`service/checkout`、`rpc/forge`、
`local/worktrees` 等）。Git、Forge、worktree、文件传输和技能安装五类能力的代码分散在
六个层目录中，阅读一个能力需要跨目录跳转；功能区之间的引用也没有约定，`local/worktrees`
直接持有 `LocalForge` 具体类型，多个 `local`/`rpc` 模块共享的 Git runner 与输出预算挂在
某个层目录下。

## 决策

1. 目录改为“能力组 → 层 → 功能区”，即 `crate::<group>::<layer>::<area>`，组内沿用原有
   `ports`/`protocol`/`service`/`rpc`/`connection`/`local` 分层，子树内部文件结构不变：

   | 组          | 功能区                                     |
   | ----------- | ------------------------------------------ |
   | `git`       | checkout、git_fetch、provisioning          |
   | `forge`     | forge、github_projects                     |
   | `worktrees` | worktrees、workspace_recovery              |
   | `files`     | files、uploads、transfer、file_transfer    |
   | `skills`    | skills                                     |

2. 顶层保留组装与跨组模块：`installation`、`capabilities`、`dispatch`、`connection`，以及
   组合 Git 与 Forge 缓存的 `workspace_runtime`（原 `local::workspace_runtime`）。
   `support` 为组间共享的私有工具：`budget`（响应输出预算）、`git_command`（有界 Git
   runner）和 `error`（宿主可见的分发错误码，经 `filesystem::ErrorCode` 导出）。
3. 组间约定：
   - 组与组之间只引用对方的 `ports` 和 `protocol`；
   - 只有顶层模块可以同时组合多个组的具体类型；
   - `support` 不依赖任何组或顶层模块。
   组内的测试可以构造其他组的具体适配器作为夹具。
4. 为满足约定：`worktrees::ports::worktrees` 新增 `ChangeRequestResolver`，`LocalForge`
   实现它，`LocalManagedWorktrees::new` 接收 `Arc<dyn ChangeRequestResolver>`，由 daemon
   注入；checkout 方法声明移到 `git::rpc::checkout::METHODS`，与其他组一致；
   filesystem 内重复的 `protocol::valid_id` 删除，统一使用 `model::valid_id`。
5. `crates/filesystem/tests/module_boundaries.rs` 扫描源码，拒绝组之间对
   `service`/`rpc`/`connection`/`local` 的引用以及 `support` 对组或顶层模块的引用，
   并用合成样例验证检查本身。

## 后果与验证

单个能力的全部分层位于同一目录，跨组耦合仅剩端口与 wire 类型，且由测试约束。
外部调用路径随之变化，例如 `filesystem::service::worktrees` 变为
`filesystem::worktrees::service::worktrees`，`filesystem::local::workspace_runtime` 变为
`filesystem::workspace_runtime`；`Service`、`Dependencies`、`dispatch`、`capabilities`、
`connection::Connection` 不变。协议方法、持久格式和运行行为不变，依赖图不变。
同一功能区名会在路径中重复出现（如 `skills::service::skills`），换取迁移完全机械、
与原结构一一对应。

验证覆盖 filesystem 全部测试、API 测试、daemon 单元/依赖守卫/进程测试，以及模块边界测试。
