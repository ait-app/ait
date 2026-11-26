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
  [ADR-136](../decisions/workspace/adr-136-single-area-layer-flattening.md)、
  [ADR-283](../decisions/workspace/adr-283-shared-rpc-codecs.md)）。RPC JSON 编解码统一从
  `model::rpc` 导入。
- 单连接调度中，只有归属 crate 已声明的 `workspace.worktree.`、`workspace.recovery.`、
  `workspace.github.`、`project.github.` 方法进入文件操作队列，长时间 clone/checkout 不阻塞目录读取
  （[ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)）。

## 登记与生命周期

- `workspace.create.request` 以 `idempotencyKey` 写持久创建回执：相同意图重放已有结果（包括重启后），
  同一 key 携带不同意图返回 `idempotency_conflict`（[ADR-060](../decisions/workspace/adr-060-workspace-create-worktree.md)）。
  回执按键存入 `creations/receipts.sqlite3`。首次启动在同一事务中导入旧 JSON，保留原文件作为升级前备份
  （[ADR-294](../decisions/daemon/adr-294-sqlite-creation-receipts.md)）。
- Worktree 创建先执行 Git，再登记 Workspace；调用方预留的 `workspaceId` 已存在时拒绝，登记失败时移除
  刚创建的 worktree（[ADR-060](../decisions/workspace/adr-060-workspace-create-worktree.md)）。
- 只有首次成功创建 worktree 才启动 metadata `workspace_automation` 中的 setup；setup 失败不撤销创建，
  创建完成也不表示 setup 已完成（[ADR-060](../decisions/workspace/adr-060-workspace-create-worktree.md)、
  [ADR-130](../decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)）。
- `workspace.get.request` 按准确 ID 读取单个 Workspace，返回 `ready`、`archived` 或 `missing`；
  不创建、不恢复，也不枚举完整目录（[ADR-126](../decisions/workspace/adr-126-workspace-target-read.md)）。
- 打开 Workspace 的目录读取（`project.list`、同步模式 `workspace.list` 的冷读取与续读、`workspace.get`）
  有本地线路预算：`crates/metadata/src/directory/service/directory/tests/wire_budgets.rs` 用 200 个带 Git
  与 PR 事实的 Workspace 测量编码后的结果字节与返回行数，读法和提高上限的方式见
  [Provider 规格](provider.md#时间线与历史投影)。同步模式忽略 `page.limit`，冷读取随目录规模增长。
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
- 客户端右上角的 Git 控制是一个分体按钮，下拉面板同时是变更面板的 Git 菜单
  （[ADR-304](../decisions/workspace/adr-304-workspace-git-control-grouped-panel-and-rebase.md)）。主按钮始终可按，取第一个匹配的状态：
  冲突、提交、分叉时的变基到远端、仅落后时的拉取、状态未知、打开的 PR 上的推送或合并 PR、查看 PR、创建 PR、基础分支上的推送、重置工作区。
  冲突、状态未知以及一键修复不适用的分叉与落后只显示状态名并打开面板；动作不能执行时落到下一行（需要推送而推送不可用的打开 PR 显示“查看 PR”，不显示“合并 PR”）。
  按钮上的拉取要求 `upstreamRef` 等于 `branchStatus.remoteRef`，否则显示的计数与拉取的引用会不一致。合并方式按宿主和项目记在设备上，不进入项目设置。
  PR 已合并、工作区干净、与同名远端分支一致且没有活动 Agent 时推荐重置，但只限可以执行重置的托管 worktree
  （[ADR-084](../decisions/workspace/adr-084-workspace-primary-git-action.md)、
  [ADR-293](../decisions/workspace/adr-293-archive-in-workspace-menu-reset-recommendation.md)）。
  归档不属于 Git 操作：入口在左侧栏的 Workspace 菜单、标题左侧的 Workspace 操作菜单、命令面板和快捷键，非 Git Workspace 同样可用。
- `checkout.rebase.request` 把分支变基到 `branchStatus` 使用的优先远端同名分支，客户端不传引用，从公布的方法列表判断支持。
  宿主在进行中的操作、不在分支上、没有远端、已跟踪文件有改动、推送会发往该远端分支以外的位置（按 `push` 的解析追到回退到 `origin`）、fetch 后远端分支不存在时拒绝，
  各用独立错误。它执行 `git rebase --rebase-merges --no-fork-point`，关闭 `rebase.updateRefs`；成功后若被替换的提交已被远端跟踪引用包含，就重置回原位并拒绝，
  因此推送后又在本地变基到基础分支的分支不会被重复发布，但也不会被修复。它不推送，成功后由普通推送完成
  （[ADR-304](../decisions/workspace/adr-304-workspace-git-control-grouped-panel-and-rebase.md)）。
- `Forge::pull_request(cwd, number)` 按编号读取 `cwd` 的 origin 仓库中的一个 PR/MR，供跟随它的 Bot 工单使用：
  状态（合并、关闭、打开；GitLab 的 `locked` 为打开）、头提交、至多 100 个检查与最新的审查活动，本账号写的条目带标记，
  每个条目带作者在仓库中的权限（能写入、其他人、机器人）；结果为中性的 `domain::forge::PullSnapshot`。
  GitHub 为一次 `gh api graphql`，GitLab 为一次 `glab mr view`、流水线、按 `sort=desc` 的最新备注、`glab api user` 与备注作者的成员权限，
  其中任何一次失败都使整次读取失败。PR 不存在为 `NotFound`，
  CLI 缺失、未登录或没有支持的远端为“不可用”；调用会阻塞，调用方在阻塞线程上执行。该读取不受 ADR-146 的重置时间过滤，
  因为调用方点名了编号（[ADR-287](../decisions/bot/adr-287-ticket-pull-request-follow-and-answers.md) 第 2 节）。
- `forge.search.request` 在 `cwd` 的 origin 仓库中搜索 Issue 与 PR/MR，默认只返回打开的条目（输入区的选择器与链接
  自动附加使用）。可选的 `state: "all"` 同时返回已合并和已关闭的条目（GitHub 为 `--state all`，GitLab 为 `--all`），
  供客户端按编号查找对话中链接指向的条目；不识别该字段的旧 daemon 仍只搜索打开的条目。条目可带作者登录名与头像地址，
  PR 另带增删行数与变更文件数（GitHub 取自 `gh` 列表字段，头像为 `https://<host>/<login>.png`，应用账号无头像；
  GitLab 列表不提供行数与文件数）。这些字段均可缺省，旧客户端忽略它们。
- 后台 fetch 只服务于已激活的目录订阅，按规范化 `git-common-dir` 合并仓库，每 180 秒执行一次
  `git fetch origin --prune --quiet`。全进程最多两个并发，单次限 120 秒并禁用凭据提示；
  它从不修改 HEAD、索引或工作树。无 origin 的目录跳过
  （[ADR-069](../decisions/workspace/adr-069-background-git-fetch.md)）。
- Agent 一轮以完成或失败结束时，daemon 读取该 Agent `cwd` 的本地 checkout 状态（不 fetch），
  有变化时发布 `checkout.status.update`，让客户端及时切换未提交变更与基准比较。同一目录至少间隔
  60 秒读取一次，间隔内再次结束的轮次合并为间隔到期时的一次读取；与后台 fetch 共用“上次发布”
  记录，任一来源只在状态变化时发布。

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
- MCP `workspace_create` 的 `isolation` 只接受 `local`、`worktree`、`sandboxed-worktree`；
  未知值，或 `local` 搭配 worktree 参数，返回 `InvalidMessage`
  （[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)）。
- 每次创建、恢复或 rewind 原生进程前，`provider::service::agent_manager::confinement` 从 Registry
  重新推导 confinement（优先 `worktreeRoot`），不随 Agent 持久化。Registry 读取失败或适配器未声明
  `supports_write_confinement` 时拒绝启动；目前声明支持的有 Claude Code、Codex、Pi、Oh My Pi、
  OpenCode、MiniMax Code、Antigravity（[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-311](../decisions/providers/adr-311-antigravity-worktree-sandbox.md)）。
- `provider_native::sandbox::command` 是唯一平台接缝：macOS 用 `/usr/bin/sandbox-exec -p` 加内联
  profile，Linux 用 `/usr/bin/bwrap`（需支持 `--disable-userns`），其他平台返回 `Unavailable`。
  任何失败都不退化为不受限运行（[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)）。
- `domain::workspace::sandbox::WritePolicy::worktree` 允许写 worktree、私有与公共 Git 目录，并保持
  `.git` 指针、公共 `config`/`hooks`、私有 `config.worktree` 与 `commondir` 只读；Provider 配置文件
  另行保护（[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)）。
- 受限会话中 Codex sandbox 固定为 `danger-full-access`，Claude Code 合并 `sandbox.enabled: false`，
  Antigravity 传入 `--settings {"enableTerminalSandbox":false}`，以避免嵌套沙箱失败；写入边界由外层沙箱提供
  （[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)、
  [ADR-311](../decisions/providers/adr-311-antigravity-worktree-sandbox.md)）。

## Workspace 记事本

每个 Workspace 有且只有一份记事本：daemon 数据目录下的一个 Markdown 文件，不在检出目录中；人和 Agent 读写的是同一个文件
（[ADR-296](../decisions/workspace/adr-296-workspace-notepad.md)、[ADR-297](../decisions/workspace/adr-297-notepad-file-in-data-directory.md)）。

- 文件为 `<data-dir>/notepads/<Workspace ID>/notepad.md`（ID 中小写字母、数字、`_`、`-` 以外的字节写作 `%XX`），经
  `model::workspace::notepad::NotepadStore` 注入，由 `persistence` 的 `FileNotepadStore` 实现。它不进入 Git，不受重置和沙箱写入限制影响。
  文件在第一次读取时创建；此时检出目录中若有非空的 `.ait-notebook.md`（更早的存放位置），以它的正文作为初始内容，不修改该文件。
  Workspace 归档期间记事本保留但不可经 `workspace.notepad.*` 读写，恢复后内容仍在；Workspace 记录被移除时删除它的目录。
- 文件的内容就是记事本的全部状态：`revision` 是由正文算出的不透明字符串，`updatedAt` 是文件的修改时间。
- `workspace.notepad.get.request` 与 `workspace.notepad.write.request` 归 `metadata` 的 `notepad` 能力组，只接受未归档的 Workspace，
  否则返回 `WorkspaceNotFound`。`get` 返回 `path`、`directory`、`revision`、`text` 与 `updatedAt`；文件大于 1 MiB 或不是 UTF-8 时失败。
- `write` 的 `replace`（默认）要求 `expected` 等于当前 `revision`（记事本为空时可省略），允许空文本；`append` 在末尾追加一段并由服务端写入
  `[作者 · 时间]`，不需要 `expected`。修订号不符或写入后超过 256 KiB 返回 `accepted: false`、`error` 与当前 `revision`。
  写入以临时文件加重命名完成，重命名前发现文件已被别人改动时按新的内容重新决定。
- 作者为可选的 `author`，缺省为 `user`，只用于 `append` 的署名；MCP 层按凭据或 `X-Ait-Agent-Id` 填写，主机凭据下只是说明而不是身份证明。
- 访客按 Workspace 分享使用这两个方法，请求中的 `author` 被移除；访客的 `fs.explorer.request`、`fs.file.subscribe.request` 与
  `fs.file.write.request` 还可以把所分享 Workspace 的记事本目录作为 `cwd`。Bot 作用域内两个工具总是可用，受限于外部对话的调用者除外，
  见 [Bot 规格](bot.md#工具分组与模式上限)；工具见 [MCP 手册](../operations/mcp.md#workspace-记事本)。
- 客户端提供一个由 `experimentalFeatures` 开关保护的 `notebook` Workspace tab，每个 Workspace 最多一个，tab 布局保留在客户端。
  它用 `get` 取得记事本文件的位置，然后用与打开文件相同的文件面板打开它：实时更新、带条件的保存、冲突提示和 Markdown 预览都来自文件面板。
  Web 与桌面端默认进入编辑，原生移动端的文件面板没有编辑器，只显示渲染后的预览。主机没有 `workspaceNotepad` 特性时提示需要更新主机。

## 定时任务归属

- 新建任务必须提供 `workspaceId`，已绑定的 `workspaceId` 不能改成另一个。目标变更由宿主的
  `schedule::ports::Runner::resolve` 校验归属，运行期间修改目标返回 `schedule_running`
  （[ADR-138](../decisions/workspace/adr-138-workspace-owned-schedules.md)、
  [任务手册](../operations/schedules.md)）。
- 已绑定的新 Agent 任务在现有 Workspace 的实际目录中创建 Agent，不创建 Workspace 或 worktree。
  `archiveOnFinish` 只归档该次新建的 Agent，所属 Workspace 不被归档；只有旧版未绑定任务保留
  创建并归档独立 Workspace 的分支（[ADR-138](../decisions/workspace/adr-138-workspace-owned-schedules.md)、
  [任务手册](../operations/schedules.md)）。
- 每次运行结算写盘后（含打开存储时记为失败的中断运行）调用 `schedule::ports::Runner::settled`；worker 启动时对 15 分钟内结束的运行再调用一次，
  弥补关闭或崩溃前未送达的报告，观察者按运行 id 去重。daemon 据此向 Workspace Bot
  提交运行结果通知（[ADR-271](../decisions/bot/adr-271-bot-notification-inbox-and-wake-turns.md) 第 13 节）。
