# Workspace 现行规格

本页覆盖 Project/Workspace 登记与生命周期、本地 Git/Forge/worktree 适配器、checkout 刷新、
侧边栏运行时摘要、沙箱化 worktree 和定时任务的 Workspace 归属，只陈述当前生效的规则。
决策背景与修订历史见所引 ADR；ADR 被后续 ADR 修订时，本页按修订后的规则陈述并同时引用两者。

## 归属与依赖

- `metadata` 与 `filesystem` 的生产代码只依赖 `domain`、`model`，`persistence` 仅限测试；两者互不依赖，
  由 [`dependencies.rs`](../../bins/daemon/tests/dependencies.rs) 守卫
  （[ADR-104](../decisions/workspace/adr-104-filesystem-model-collaboration.md)、
  [ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)）。
- 全部 `workspace.*`/`project.*` 方法归 `metadata`，包括 worktree、recovery 与 GitHub clone；
  `filesystem` 只实现 `model::workspace::{worktrees,provisioning}` 中的端口（`LocalManagedWorktrees`、
  `LocalGithubProjects`、`LocalForge`），由 daemon 注入 metadata
  （[ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)）。
- 两个 crate 都按能力组组织（filesystem 为 `git`、`forge`、`files`），组之间只能引用对方的
  `ports` 与 `protocol`，`support` 不依赖任何组；各自的 `tests/module_boundaries.rs` 强制执行
  （[ADR-113](../decisions/workspace/adr-113-filesystem-capability-groups.md)、
  [ADR-131](../decisions/workspace/adr-131-metadata-capability-groups.md)、
  [ADR-136](../decisions/workspace/adr-136-single-area-layer-flattening.md)）。
- 单连接调度中，只有归属 crate 已声明的 `workspace.worktree.`、`workspace.recovery.`、
  `workspace.github.`、`project.github.` 方法进入文件操作队列，长时间 clone/checkout 不阻塞目录读取
  （[ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)）。

## 登记与生命周期

- `workspace.create.request` 以 `idempotencyKey` 写持久创建回执：相同意图重放已有结果（包括重启后），
  同一 key 携带不同意图返回 `idempotency_conflict`
  （[ADR-060](../decisions/workspace/adr-060-workspace-create-worktree.md)）。
- Worktree 创建先执行 Git，再登记 Workspace；调用方预留的 `workspaceId` 已存在时拒绝，登记失败时移除
  刚创建的 worktree（[ADR-060](../decisions/workspace/adr-060-workspace-create-worktree.md)）。
- 只有首次成功创建 worktree 才启动 metadata `workspace_automation` 中的 setup；setup 失败不撤销创建，
  创建完成也不表示 setup 已完成（[ADR-060](../decisions/workspace/adr-060-workspace-create-worktree.md)、
  [ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)）。
- `workspace.get.request` 按准确 ID 读取单个 Workspace，返回 `ready`、`archived` 或 `missing`；
  不创建、不恢复，也不枚举完整目录（[ADR-126](../decisions/workspace/adr-126-workspace-target-read.md)）。
- 归档托管 worktree 时先把 Workspace 记录标记为已归档，再由宿主实现的 `WorktreeArchiveCleanup` 关闭
  相关资源；清理失败保留 checkout，仍有未归档 Workspace 引用时也不删除 checkout
  （[ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)）。

## Git 操作与重置

- `checkout.reset_workspace.request` 只接受托管 worktree，且 `workspaceId`、`cwd`、未归档状态和
  创建时分支名（记录的 `display_name`）都必须与登记记录一致。远端查询或 fetch 在本地变更前完成，
  失败时本地分支、HEAD 和文件保持原状
  （[ADR-083](../decisions/workspace/adr-083-reset-workspace-to-origin-default.md)、
  [ADR-092](../decisions/workspace/adr-092-reset-same-named-remote-branch.md)）。
- 重置把工作区切回初始分支，`git reset --hard` 到 origin 默认分支，再执行 `git clean -ffd`
  删除未跟踪文件与嵌套仓库；不使用 `-x`，忽略文件保留
  （[ADR-083](../decisions/workspace/adr-083-reset-workspace-to-origin-default.md) 2026-10-19 修订）。
- 只有本地变更前的 `ls-remote` 观察到 origin 同名分支时，才用 `--force-with-lease` 和 `--no-follow-tags`
  推送到该提交，不创建远端分支。推送失败时本地重置不回滚，错误说明远端未完成；只有完整成功才更新
  记录中的分支（[ADR-092](../decisions/workspace/adr-092-reset-same-named-remote-branch.md)）。
- 重置成功后写入 `branch.<initial>.ait-reset-at`，并清除该分支的 `remote`、`merge`、`pushRemote`、
  `gh-merge-base`。Forge 读取（GitHub `createdAt`、GitLab `created_at`）不再关联创建时间早于或等于
  该时间、或缺少创建时间的 PR/MR
  （[ADR-146](../decisions/workspace/adr-146-reset-workspace-change-request-association.md)）。
- 后台 fetch 只服务于已激活的目录订阅，按规范化 `git-common-dir` 合并仓库，每 180 秒执行一次
  `git fetch origin --prune --quiet`。全进程最多两个并发，单次限 120 秒并禁用凭据提示；
  它从不修改 HEAD、索引或工作树。无 origin 的目录跳过
  （[ADR-069](../decisions/workspace/adr-069-background-git-fetch.md)）。

## Checkout 刷新与背压

- 前台 Checkout 操作经 `model::Runtime::run_checkout` 使用独立的单任务额度排队，后台轮询另有
  `checkout_poll_jobs`；等待或执行 Git 都不占用全局 `jobs`
  （[ADR-132](../decisions/workspace/adr-132-checkout-refresh-admission-and-backpressure.md)）。
- Checkout 成功响应和 diff 更新经 `Outbound::send_waiting` 等待已有输出预算，拥堵时不断开连接；
  超出单帧预算立即拒绝，断线或取消结束等待
  （[ADR-132](../decisions/workspace/adr-132-checkout-refresh-admission-and-backpressure.md)）。
- Diff 语法 token 由 filesystem 的 checkout adapter 生成，只携带与主题无关的语法角色，颜色由客户端决定
  （[ADR-071](../decisions/workspace/adr-071-checkout-diff-syntax-highlighting.md)）。

## 运行时摘要与唤醒

- 目录服务只从 `model::workspace::runtime::WorkspaceRuntimeSource` 读取缓存，不阻塞也不执行 Git。
  daemon 在组合根安装一个共享的 `filesystem::workspace_runtime::LocalWorkspaceRuntime`
  （[ADR-068](../decisions/workspace/adr-068-workspace-runtime-summaries.md)）。
- 缓存按规范化 cwd 最多保留 1024 项。Git 与 Forge 各最多两个后台 worker，按 FIFO 读取；超过 5 秒
  无人请求的排队任务跳过，没有读者时不刷新
  （[ADR-068](../decisions/workspace/adr-068-workspace-runtime-summaries.md)）。
- 刷新间隔按 cwd 指数退避：Git 从 2 秒到 30 秒，Forge 从 30 秒到 120 秒，结果未变就翻倍，变化即
  回到起点；Git 身份变化同时重置 Forge
  （[ADR-068](../decisions/workspace/adr-068-workspace-runtime-summaries.md) 2026-11-07 修订、
  [ADR-217](../decisions/workspace/adr-217-event-driven-workspace-runtime.md)）。
- 只有改变已发布事实的读取（或首次完成）才唤醒，并且只通知 `Directory::workspace_changes`。
  checkout 事实不唤醒 Agent 目录或 Hub 投影
  （[ADR-217](../decisions/workspace/adr-217-event-driven-workspace-runtime.md)）。
- 仅 macOS 用 `FSEvents` 把缓存条目标记为 dirty；全部根路径都被监视的条目，Git 轮询上限放宽到
  5 分钟。其他平台只轮询，上限保持 30 秒。侧边栏 Git 读取使用 `--no-optional-locks`，自身不写仓库
  （[ADR-217](../decisions/workspace/adr-217-event-driven-workspace-runtime.md)）。

## 沙箱

- 隔离级别是 Workspace 记录、描述符和 worktree source 上的可选字段 `sandbox: worktree_writes`，
  不新增 Workspace kind；缺省时不序列化
  （[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)）。
- 只有 macOS/Linux 且装有 metadata 的 daemon 声明 `workspace-sandbox-v1`。其他平台上，带 sandbox 的
  worktree 创建在任何 Git 操作前就被 metadata 拒绝
  （[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)）。
- MCP `create_workspace` 的 `isolation` 只接受 `local`、`worktree`、`sandboxed-worktree`；
  未知值，或 `local` 搭配 worktree 参数，返回 `InvalidMessage`
  （[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)）。
- 每次创建、恢复或 rewind 原生进程前，`provider::service::agent_manager::confinement` 从 Registry
  重新推导 confinement（优先 `worktreeRoot`），不随 Agent 持久化。Registry 读取失败或适配器未声明
  `supports_write_confinement` 时拒绝启动；目前声明支持的有 Claude Code、Codex、Pi、Oh My Pi、
  OpenCode、MiniMax Code（[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)）。
- `provider_native::sandbox::command` 是唯一平台接缝：macOS 用 `/usr/bin/sandbox-exec -p` 加内联
  profile，Linux 用 `/usr/bin/bwrap`（需支持 `--disable-userns`），其他平台返回 `Unavailable`。
  任何失败都不退化为不受限运行（[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)）。
- `domain::workspace::sandbox::WritePolicy::worktree` 允许写 worktree、私有与公共 Git 目录，并保持
  `.git` 指针、公共 `config`/`hooks`、私有 `config.worktree` 与 `commondir` 只读；Provider 配置文件
  另行保护（[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)）。
- 受限会话中 Codex sandbox 固定为 `danger-full-access`，Claude Code 合并 `sandbox.enabled: false`，
  以避免嵌套沙箱失败；写入边界由外层沙箱提供
  （[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)）。

## 定时任务归属

- 新建任务必须提供 `workspaceId`，已绑定的 `workspaceId` 不能改成另一个。目标变更由宿主的
  `schedule::ports::Runner::resolve` 校验归属，运行期间修改目标返回 `schedule_running`
  （[ADR-138](../decisions/workspace/adr-138-workspace-owned-schedules.md)、
  [任务手册](../operations/schedules.md)）。
- 已绑定的新 Agent 任务在现有 Workspace 的实际目录中创建 Agent，不创建 Workspace 或 worktree。
  `archiveOnFinish` 只归档该次新建的 Agent，所属 Workspace 不被归档；只有旧版未绑定任务保留
  创建并归档独立 Workspace 的分支（[ADR-138](../decisions/workspace/adr-138-workspace-owned-schedules.md)、
  [任务手册](../operations/schedules.md)）。
