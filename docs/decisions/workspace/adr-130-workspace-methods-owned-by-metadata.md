# ADR-130：Workspace 与 Project 方法归 metadata，filesystem 只保留本地适配器

- 状态：接受
- 日期：2026-10-11
- 修订：[ADR-113](adr-113-filesystem-capability-groups.md) 的 filesystem 能力组集合、
  [ADR-104](adr-104-filesystem-model-collaboration.md) 中 `ProjectRegistration` 与
  worktree setup 的注入方式、[ADR-028](adr-028-github-project-provisioning.md) 的服务归属，
  以及 [ADR-100](../daemon/adr-100-crate-level-service-installation.md) 的服务组成。

## 背景

[ADR-129](../providers/adr-129-provider-owned-skills.md) 之后，filesystem 仍声明七个
`workspace.*`/`project.*` 方法：`workspace.worktree.{list,create,archive}`、
`workspace.recovery.{inspect,restore}`、`workspace.github.search_repositories` 与
`project.github.clone`。其余 `workspace.*`/`project.*` 方法均归 metadata。这些方法的业务
本质是 Project/Workspace 登记的创建、归档和恢复，filesystem 只为此回调 metadata 注入的
`ProjectRegistration`、`WorkspaceNaming` 与 `WorkspaceSetup`，且 API 需要从两个 crate 拼装
同一份 `Worktrees`。真正属于 filesystem 的只有执行 Git、`gh` 和本地目录操作的适配器。
依赖守卫禁止 metadata 与 filesystem 互相依赖，因此只能按层拆分。

## 决策

1. 三组服务、RPC 投影和 wire 协议迁入 metadata 已有的分层：
   `service::{worktrees,workspace_recovery,github_projects}`、
   `rpc::{worktrees,workspace_recovery,github_projects}`、
   `protocol::{worktrees,workspace_recovery,github_projects}`。方法声明、dispatch 分支、
   `metadata::Dependencies` 与 `dispatch::State` 同步增加这三项；不保留旧路径转发。
2. 适配器继续由 filesystem 实现：`LocalManagedWorktrees` 并入 `git::local::worktrees`，
   `LocalGithubProjects` 留在 `forge::local`，`LocalForge` 仍实现 change request 解析。
   filesystem 的 `worktrees` 能力组取消，剩余 `git`、`forge`、`files` 三组。
3. 适配器端口下沉：`ManagedWorktrees`、`ChangeRequestResolver`、`WorkspaceRecoveryRuntime`
   归 `model::workspace::worktrees`，`GithubProjectsRuntime` 归
   `model::workspace::provisioning`；端口交换的值与错误归 `domain::workspace::worktrees`
   和 `domain::workspace::provisioning`，字段改为公开以便跨 crate 构造。daemon 将
   filesystem 的适配器注入 metadata 服务。
4. 只有 API 实现的 `WorktreeArchiveCleanup` 归 `metadata::ports::worktrees`。
   `ProjectRegistration` 只剩 metadata 内部调用，从 model 删除；`GithubProjects` 直接持有
   共享 registry 的 `Directory` 克隆。worktree 创建后的 setup 直接使用 metadata 的
   `workspace_automation`，删除 filesystem 的 `workspace_setup` 字段与 API 的转接适配器。
5. 同一个 `Arc<Mutex<Worktrees>>` 仍由 worktree RPC、`Directory`（经 `WorkspaceWorktrees`）
   与 schedule 共享；API 的归档协调从 `state.metadata.worktrees` 取得该实例。
6. 单连接调度与方法归属分开：先确认方法由 provider 或 metadata 声明，再将
   `agent.skills.`、`workspace.worktree.`、`workspace.recovery.`、`workspace.github.`、
   `project.github.` 前缀送入文件操作队列，避免长时间 clone 或 checkout 阻塞目录读取。
   未声明的同前缀名称不获得该队列。

## 后果与验证

方法名、方向、参数、响应、错误码及创建/归档/恢复顺序不变；正常 daemon 的方法集合不变。
这七个方法改为随 metadata 安装，仅安装 filesystem 的精简 host 不再提供它们。依赖图、
Cargo 依赖边和数据格式不变，filesystem 去掉仅供随机 worktree 名使用的 `getrandom`。

验证覆盖迁入的服务、RPC 和协议测试，改用真实 `Directory` 与文件 registry 的 GitHub 登记
测试，filesystem 适配器与模块边界测试，API 的归属断言、单连接调度、启动突发和完整分发，
以及 daemon 依赖守卫和 worktree、恢复、GitHub clone、归档、PR checkout 的真实进程测试。
