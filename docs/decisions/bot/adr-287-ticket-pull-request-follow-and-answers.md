---
id: ADR-287
status: accepted
date: 2026-11-14
amends:
  - bot/adr-271-bot-notification-inbox-and-wake-turns.md
  - bot/adr-274-bot-tickets-assignments-and-reports.md
  - bot/adr-280-bot-recall-reads-and-spill.md
  - providers/adr-276-ait-input-envelope-and-origin.md
  - workspace/adr-066-gitlab-forge.md
paths:
  - crates/domain/src/forge.rs
  - crates/domain/src/bot/ticket.rs
  - crates/domain/src/bot/notification.rs
  - crates/domain/src/input_envelope.rs
  - crates/bot
  - crates/filesystem/src/forge
  - crates/provider/runtime/src/timeline_projection.rs
  - crates/provider/runtime/src/connection.rs
  - crates/provider/runtime/src/dispatch/requests.rs
  - crates/provider/runtime/src/dispatch/agent_execution.rs
  - crates/api/src/connection.rs
  - crates/api/src/mcp
  - bins/daemon/src/host/follow.rs
  - bins/daemon/src/host.rs
  - packages/protocol/src/bot
  - packages/protocol/src/agent-types.ts
  - packages/protocol/src/messages.ts
  - packages/client/src
  - apps/mobile/src/bots
---

# ADR-287：工单跟随拉取请求、直接投递给工作者，以及结构化提问与回答

- 状态说明：接受。本 ADR 落地工单与拉取请求（下称 PR，GitLab 为合并请求）的绑定、daemon 侧的 PR 跟随器、
  跟随结果的路由（直接投递给工作者或作为结果唤醒 Bot）、PR 合并后工单结束与之后的恢复，
  以及工作者 `blocked` 时的结构化提问与所有者或 Bot 的回答。
- 关系：扩展 [ADR-274](adr-274-bot-tickets-assignments-and-reports.md) 的工单记录、汇报种类、`report_progress` 与 `bot_ticket_update` 的参数、客户端 RPC；
  扩展 [ADR-271](adr-271-bot-notification-inbox-and-wake-turns.md) 的通知类型；扩展 [ADR-280](adr-280-bot-recall-reads-and-spill.md) 的工单页面；
  扩展 [ADR-276](../providers/adr-276-ait-input-envelope-and-origin.md) 的信封种类；为 [ADR-066](../workspace/adr-066-gitlab-forge.md) 的 Forge 端口增加按编号读取 PR；
  新增的方法在 [ADR-282](../daemon/adr-282-single-connection-capability-budget.md) 的单连接能力预算之内（第 12 节）。
  [ADR-222](adr-222-single-agent-mode-no-bot-workspaces.md) 与所有者对本阶段的决定不变：Bot 不创建 Workspace，也不为工单创建 worktree，
  Bot 启动的每个 Agent 都在 Bot 自己的检出中工作；工单只因工作者或 Bot 声明才知道哪个 PR 属于它。

## 背景

ADR-274 之后，工作者开出 PR 时没有办法把它和工单联系起来；CI 失败与审查意见没有人收到，除非有人告诉 Bot；
PR 合并后工单仍然打开，直到 Bot 或用户关闭；工作者报 `blocked` 后，只有 Bot 用 `send_agent_prompt` 转述才能得到回答。

DeepSeek 公布的 super agent 设计（`dsh-superagent-prototype` 的 `docs/001-domain.md`、`002-authorization.md`、`003-execution.md`）中，
工单**订阅**线程（包括工作者开出的 PR），订阅线程之后的活动（审查意见、CI 结果、合并或关闭）作为上下文**直接扇出给工单的执行者**，
不经过路由、不重新授权；路由只听到结果。PR 全部合并或关闭的工单以该结果**结束**，并保留订阅，之后有人评论时可以恢复。
需要人的执行者提出**结构化问题**（Attention），面板显示它，人的回答**直接到达执行者**。本 ADR 按 Ait 的记录、权限与 Forge 实现同样的行为。

## 决策

### 1. 中性的 PR 事实：`domain::forge`

新模块 `domain::forge` 保存 Forge 读出的 PR 事实，既不属于 Bot 也不属于 filesystem，二者都依赖 `domain`：

```rust
pub enum PullState { Open, Merged, Closed }
pub enum CheckStatus { Pending, Success, Failure, Skipped }
pub struct PullCheck { pub name: String, pub status: CheckStatus, pub url: Option<String>, pub run: Option<String> }
pub enum ReviewState { Approved, ChangesRequested, Commented }
pub enum PullItemKind { Comment, ReviewComment, Review(ReviewState) }
pub enum AuthorAccess { Writer, Unconfirmed, Other, Bot }
pub struct PullItem { pub id: String, pub kind: PullItemKind, pub author: String, pub own: bool,
                      pub access: AuthorAccess, pub body: String, pub url: String,
                      pub created_at: DateTime<Utc>, pub path: Option<String>, pub line: Option<u64> }
pub const CHECK_LIMIT: usize = 100;
pub struct PullSnapshot { pub number: u64, pub url: String, pub title: String, pub state: PullState,
                          pub head: Option<String>, pub checks: Vec<PullCheck>, pub items: Vec<PullItem> }
```

`own` 表示条目由 Forge CLI 当前登录的账号所写（第 7 节）。`access` 是作者在仓库中的权限：`Writer` 能写入仓库
（GitHub 的 `authorAssociation` 为 `OWNER`，或权限查询确认能写入的作者；GitLab 为 Developer（30）及以上的项目成员）；
`Unconfirmed` 是 GitHub 的 `MEMBER`（所属组织的成员，不论在本仓库的角色）与 `COLLABORATOR`（可能只有 read 或 triage 角色），
是否能写入要由权限查询确认（第 4 节）；`Bot` 是机器人或应用账号，其余为 `Other`（例如公开仓库上的陌生人）。`run` 是检查的运行 id（GitHub check run 的 `databaseId`、GitLab job id），没有时为空。
`CHECK_LIMIT` 是一次读取至多返回的检查数，两个 Forge 都按它限制读取。
`CheckStatus::from_forge` 从 Forge 适配器已有的类型化检查状态（`domain::workspace::protocol::workspace::CheckStatus`）得出，`cancelled` 归入 `Skipped`。

### 2. Forge 端口：按编号读取 PR

`filesystem::forge::service::Forge::pull_request(cwd, number) -> Result<PullSnapshot, ForgeRuntimeError>`（公开），
端口 `ForgeRuntime::pull_request` 默认返回“不支持”，`LocalForge` 实现：

- **GitHub**：一次 `gh api graphql`，仓库为 `cwd` 的 origin（`project_path`），读取 `viewer.login`、PR 的编号、URL、标题、状态（`mergedAt` 不为空即为合并）、
  `headRefOid`、最后一个提交的 `statusCheckRollup`（至多 `CHECK_LIMIT` = 100 个检查，按现有 `parse_checks` 归一），以及最新的 30 条审查、50 条评论、
  30 个审查线程各最新的 10 条评论（`last:`，与现有时间线的 `first:` 不同，跟随只关心最新的条目；合计至多 380 条，低于绑定保留的 512 个已处理 id）。
  每个条目在同一查询中带 `viewerDidAuthor`（即 `own`）、`authorAssociation` 与 `author { __typename }`（`Bot` 即机器人或应用账号），由此得出 `access`：
  `OWNER` 为 `Writer`，`MEMBER`、`COLLABORATOR` 为 `Unconfirmed`，其余为 `Other`。
  对话评论按 `publishedAt`（没有时 `createdAt`）定日期。审查线程中的评论只按 `publishedAt` 定日期（随审查一起发布，可能晚于起草时间），
  并带 `pullRequestReview { state }`：`publishedAt` 为空或审查状态为 `PENDING` 的评论（所有者用同一账号起草、尚未提交的审查，GitHub 会把它显示给本账号）不读出；
  没有日期的审查（未提交）同样不读出。
  PR 不存在时 GraphQL 返回 `null`（或 `NOT_FOUND` 错误），为 `NotFound`；有其他错误而没有 PR（例如速率限制）时为读取失败，不当作不存在。
- **GitLab**：尽量少的 `glab` 调用：`glab mr view <n> -F json`（状态、标题、URL、`sha`，只调用一次）；有流水线时 `glab ci get`（检查，至多 `CHECK_LIMIT` 个作业）；
  `api …/notes?sort=desc&order_by=created_at&per_page=100` 读取最新的 100 条备注（GitLab 备注 API 文档写明 `sort` 可为 `asc`／`desc`、默认 `desc`，
  `order_by` 默认 `created_at`；讨论 API 没有排序参数，也不保证顺序，因此不再用讨论的第一页），跳过系统备注，有 `position` 的为行内评论；
  `api user` 取当前登录名以判定 `own`；`api projects/<id>/members/all?user_ids[]=…` 一次读取这些备注作者中的项目成员与权限（成员 API 文档的 `user_ids` 过滤）。
  备注作者没有机器人标记，因此按 GitLab 的命名判定机器人：项目与组访问令牌的机器人用户 `project_<id>_bot_…`、`group_<id>_bot_…`，`service_account…`，以及以 `-bot` 结尾的名字。
  合并请求状态 `locked`（合并进行中的短暂状态）视为打开。任何一次调用失败（流水线、用户、成员或备注）都使整次读取失败，不会以缺少一部分的读取替代。
- **写入权限查询**：`Forge::can_write(cwd, login) -> Result<bool, ForgeRuntimeError>`（端口 `ForgeRuntime::can_write`，默认“不支持”）。
  GitHub 以 `gh api repos/<owner>/<repo>/collaborators/<login>/permission` 读取，`role_name` 或 `permission` 为 `admin`、`maintain` 或 `write` 时能写入；
  GitHub 不认识该登录名（HTTP 404）或登录名不是合法的 GitHub 登录名（只允许字母、数字与 `-`，因此不能改变 API 路径）时为不能写入；其他失败（例如速率限制）为查询失败。
  GitLab 的读取已按成员权限给出 `Writer` 或 `Other`，从不产生 `Unconfirmed`，因此不实现查询。
- 失败沿用 `ForgeRuntimeError` 的分类：没有 CLI、未登录、不在 Git 仓库、没有支持的远端、PR 不存在。读取是阻塞的 CLI 调用，调用方必须在阻塞线程上执行。

### 3. 绑定 PR

**引用**：`"12"`、`"#12"`，或 PR 的 URL（GitHub `…/<owner>/<repo>/pull/12`，GitLab `…/-/merge_requests/12`）。
`domain::bot::follow::PullReference::parse` 取出编号与可选 URL；读取总在根 Workspace 的检出中按编号进行，
给出 URL 时须与读出的 PR URL 相同（忽略大小写与末尾 `/`），否则拒绝为“属于另一个仓库”。

**记录**：`Ticket` 新增 `pulls`（为空时不写出），每项 `PullBinding`：

| 字段                     | 说明                                                                                                    |
| ------------------------ | ------------------------------------------------------------------------------------------------------- |
| `number`、`url`、`title` | PR 编号、URL 与标题（标题至多 256 字节，跟随时更新）                                                    |
| `boundBy`、`agentId`     | `agent`（工作者经 `report_progress`，`agentId` 为它）或 `bot`（经 `bot_ticket_update`，无 `agentId`）   |
| `boundAt`                | 绑定时间                                                                                                |
| `state`                  | 最近读到的 `open`、`merged`、`closed`                                                                   |
| `head`                   | 最近读到的头提交                                                                                        |
| `checks`                 | 最近读到的每个检查的状态，按名称；读取至多 `CHECK_LIMIT` 个，全部保留                                   |
| `failed`                 | 当前头提交上已报告过的失败，键为检查名与运行 id（没有运行 id 时为头提交）；见第 5 节                    |
| `failing`                | 自上一次全部通过的读取以来是否有检查失败过（粘滞，只由全部通过的读取清除）                              |
| `mark`                   | 高水位：已处理的最新条目（创建时间与 id）；更早的条目永不投递                                           |
| `seen`                   | 已处理的条目 id（内部，不进入任何投影）：高水位宽限窗口内的条目，加上更早的，至多 `SEEN_LIMIT = 512` 个 |
| `confirmed`              | 已查询过写入权限的作者（登录名到能否写入），至多 `CONFIRMED_LIMIT = 64` 个；内部，不进入投影            |
| `lookupFailures`         | 还没有答复的作者的写入权限查询失败（或因预算跳过）的次数，至多 64 个；见第 4 节；内部，不进入投影       |
| `polledAt`               | 最近一次成功读取的时间                                                                                  |
| `failures`、`error`      | 连续读取失败次数与最近一次的错误文本（至多 512 字节）；成功读取后清除                                   |
| `missing`                | 连续“PR 不存在”的读取次数；其他结果清零，达到 `MISSING_LIMIT = 8` 时停止跟随（第 5 节）                 |

`Ticket` 另有 `pending`（待投递，第 6 节）、`followUntil`（PR 结束工单后继续跟随到何时，第 5 节）与 `inputTurns`（投递的输入开始的工作者轮次，第 6 节），为空时不写出。

**入口**：

- 工作者 `report_progress` 新增可选 `pullRequest`（字符串，1–512 字符）。
- Bot `bot_ticket_update` 新增可选 `pullRequest`，用于 Bot 自己得知的 PR。
- 两者都先经 `bot::service::Bots::read_pull(root, reference)` 用跟随器的读取端口（第 4 节）读取并校验，再把读到的快照随汇报或更新一起交给 Bot worker，
  在同一次工单写入中绑定，因此校验失败时汇报与更新都不记录。绑定记录一条 `kind: "pull"` 的汇报（来源为绑定者），文本为“Following pull request #12: <标题>”。
- 同一工单再次绑定同一编号只更新标题与 URL，不重复；每个工单至多 `PULL_LIMIT = 4` 个 PR；每个 Bot 正在跟随的 PR（第 5 节的跟随规则）
  至多 `FOLLOW_LIMIT = 16` 个，这是每个 Workspace 的上限。超出返回 `ResourceExhausted`。重新打开一个不再跟随的工单（Bot、用户或 `send_agent_prompt` 的自动重新打开）
  会使它的 PR 重新被跟随，因此同样检查这个上限，超出时拒绝重新打开（`bot_ticket_update` 给出说明两种上限的引导文本）。
  以 `done` 或 `failed` 关闭、仍在跟随的工单占用名额；Bot 或用户把它改为 `cancelled`（第 5 节）即停止跟随并释放名额，PR 合并后的窗口内也可以。
  超出上限时的引导文本（`PULL_LIMIT_MESSAGE`）与 `bot_ticket_update` 的描述都写明这一操作。
- 绑定时的读取即基线：读到的条目全部记为 `seen`，检查与状态记为当前值，因此不会把绑定之前的评论投递给工作者。
- `confirmed` 的结果在绑定的整个生命期内有效（PR 打开期间加上结束后至多 `FOLLOW_AFTER`），不因之后权限被撤销或授予而更新。
  已保存 64 个作者后，新的作者不再保存：他们之后的每条新条目都要重新查询（并按第 4 节计数失败），已处理的条目不再查询。

**拒绝**（工具返回可操作的引导文本，`bot::follow::PullRefusal::message`）：引用不能解析；读取端口未安装；
Forge CLI 缺失、未登录或不在 Git 仓库中（提示 `gh auth status`／`glab auth status`）；PR 不存在于该仓库；URL 属于另一个仓库；
已达 `PULL_LIMIT` 或 `FOLLOW_LIMIT`；工单已关闭。

### 4. 跟随器

**端口**（`bot::follow`）：

```rust
#[async_trait] pub trait PullSource: Send + Sync {
    /// 在 Workspace `workspace` 的检出中读取 PR `number`；实现必须在阻塞线程上运行 CLI。
    async fn read(&self, workspace: &str, number: u64) -> Result<PullSnapshot, PullReadError>;
    /// `login` 能否写入该检出的仓库（第 2 节的写入权限查询）；Forge 明确拒绝告知时为 false，
    /// 暂时失败时返回错误，调用方暂缓该作者的条目并计数（见下）。
    async fn can_write(&self, workspace: &str, login: &str) -> Result<bool, PullReadError>;
}
#[async_trait] pub trait Workers: Send + Sync {
    /// 带 `ait.bot.ticket = ticket` 标签、未归档的 Agent。
    async fn live(&self, ticket: TicketId) -> Result<Vec<String>, ErrorCode>;
    /// 这些 Agent 最近的时间线中工具调用发送的内容（参数，不含用户消息、投递的输入与工具输出），第 7 节。
    async fn inputs(&self, agents: &[String]) -> Vec<String>;
    /// 只在 `agent` 空闲时以 `message_id` 把 `text` 作为新一轮发送；正在执行时返回 `Busy`，不发送。
    async fn deliver(&self, agent: &str, message_id: &str, text: &str) -> Result<Delivered, ErrorCode>;
}
pub enum Delivered { Started, Busy }
```

daemon 在组合时以 `Bots::attach_follow(runtime, pulls, workers)` 安装两个端口并启动跟随任务（`bins/daemon/src/host/follow.rs`）：
`PullSource` 由 Workspace registry 解析检出目录，以 `spawn_blocking` 调用第 2 节的读取，同一 daemon 内的读取经一个互斥量串行；
`Workers` 由 Agent registry 判定存活与空闲（`lastStatus` 为 `idle`、`closed` 或 `error` 为空闲，`running`、`initializing` 为忙），
经 `AgentExecution` 调用 `agent.timeline.get.request`（尾部 200 条）与 `agent.message.send.request`。
webhook 等其他来源以后只需产生同样的 `PullSnapshot` 交给同一个 `Bots::observe_pull`，路由不变。

**跟随集合与零成本**：Bot worker 在每条命令后发布跟随集合（`watch`）：跟随 PR 的工单（第 5 节）与有待投递的工单。
集合为空时跟随任务只等待集合变化或服务停止，不读取 Forge、不读 Agent 目录、不写盘。

**节奏**：

| 常量               | 值      | 说明                                                                |
| ------------------ | ------- | ------------------------------------------------------------------- |
| `POLL_INTERVAL`    | 60 秒   | 工单仍有打开的 PR 时两次读取的间隔                                  |
| `ENDED_INTERVAL`   | 10 分钟 | 工单的 PR 全部结束时的读取间隔（包括 PR 结束后恢复的工单）          |
| `BACKOFF_LIMIT`    | 15 分钟 | 读取失败后的间隔为 `POLL_INTERVAL × 2^失败次数`，至多此值           |
| `PENDING_INTERVAL` | 15 秒   | 等待工作者空闲的待投递输入两次尝试的间隔                            |
| `READ_BUDGET`      | 30      | 整个 daemon 所有 Bot 合计每 `READ_WINDOW`（60 秒）至多的 Forge 读取 |
| `FOLLOW_AFTER`     | 7 天    | PR 结束工单后继续跟随的时间                                         |
| `FOLLOW_LIMIT`     | 16      | 每个 Bot（Workspace）同时跟随的 PR                                  |

跟随任务是单个异步任务，服务这个 daemon 的所有 Bot，逐个处理到期的 PR，因此整个 daemon 同一时刻至多一个 Forge 读取，同一仓库的读取自然串行；
读取预算用尽时到期的读取推迟到预算恢复，使跟随的 PR 不会耗尽所有者 `gh` 账号的 API 额度（GitHub GraphQL 每小时 5000 点，预算至多每小时 1800 次读取）；
读取在阻塞线程上执行，异步线程从不等待 CLI。下次读取时间只在内存中，重启后立即读取一次。读取失败经 `Bots::pull_failed` 记录在绑定上
（`failures` 加一、`error`），按退避重试；从不使工单失败或改变其状态。

**写入权限查询**：每次读取后，跟随器经 Bot worker 向绑定询问需要查询的作者（`PullBinding::authors_to_confirm`：读取中值得阅读、
不早于高水位宽限窗口、尚未处理、作者为 `Unconfirmed` 且绑定的 `confirmed` 中还没有的条目的作者；绑定的 `seen` 不在内存索引中，因此由 Bot worker 读取工单文件判定），
对每个作者调用一次 `PullSource::can_write`，每次查询同样计入 `READ_BUDGET`；结果随读取交给 Bot worker。确定的答复（能写入、不能写入、
Forge 明确拒绝告知）写入绑定的 `confirmed`（持久，至多 64 个），以后的读取不再查询同一作者。

- **不能确认时按外部作者处理**：GitHub 对没有推送权限的账号（例如 `gh pr create` 把分支推到了 fork，`origin` 仍是上游；或组织的基础权限只有 read）
  以及应用令牌，对该接口返回 HTTP 403（“Must have push access to view collaborator permission”）。这是不会因重试改变的答复，
  GitHub 适配器把它（以及 404）当作“不能确认能写入”返回 `false`；只有速率限制的 403 仍是失败。该作者因此立即按 `Other` 处理并缓存：
  Bot 收到常规的外部作者通知（作者与链接，不含正文），条目不到达工作者，以后不再查询。只有这样的确定答复会被缓存为 `false`。
- **其他失败有次数上限**：查询失败（网络、速率限制等），或这次读取的预算没有余量而跳过的查询，都记为该作者的一次尝试，
  计在绑定的 `lookupFailures` 中（持久，至多 64 个作者）；期间条目**暂缓**：既不投递、不通知，也不记为已处理，高水位不越过其中最早的一条，下一次读取再查询。
  同一作者第 `LOOKUP_ATTEMPTS = 3` 次尝试失败时（或 `lookupFailures` 已满、无法计数时立即）放弃：已暂缓的条目在这次读取中按 `Other`
  作为外部作者的条目通知 Bot，并像其他条目一样记为已处理；清除该作者的计数，但**不**写入 `confirmed`，因为失败不是答复。
  因此同一作者之后的新条目会再次查询（同样至多暂缓三次读取），预算或速率限制恢复后维护者的审查照常到达工作者；已放弃的条目不再查询。
  查询成功时清除该作者的计数。
- 因此预算饱和也不能无限期暂缓：跳过的查询同样计为尝试（本阶段选择计数而不是在读取前预留预算），最坏情况下三次读取后条目以外部作者通知到达 Bot。
  一次读取中跟随器被要求查询的每个作者，都恰好出现在结果的 `answered` 或 `failed` 中（`domain::bot::follow::Lookups`）。

### 5. 事件与路由

`PullBinding::observe(snapshot, authored, lookups, at)`（纯函数，`domain::bot::follow`）比较快照与绑定，返回事件并更新绑定：

- **新条目**：id 不在 `seen` 中、且不早于高水位 `mark` 减去宽限 `MARK_GRACE`（2 分钟）的评论、审查评论，以及请求修改或带正文的审查。
  Forge 按写入时间给条目定日期，一条条目可能比稍新的条目晚出现在读取中，宽限窗口内的条目仍算新的；更早的条目永不投递，
  因此删除评论使旧评论回到读取窗口、或一次读取较短时，旧条目都不会再次投递。每次读取把最新条目提升为高水位，把宽限窗口内的条目加入 `seen`。
  没有正文的“已评论”审查与没有正文的批准不产生事件，只记为已处理。作者能写入仓库的产生 `Item`（给工作者），其他人的产生 `Outside`（只给 Bot），
  机器人与应用账号（以及第 7 节认领的条目）只记为已处理。`Unconfirmed` 的作者按 `confirmed` 与这次的查询结果判定；仍不知道时条目暂缓（第 4 节）。
- **检查失败**：一次失败按检查名与运行 id（没有时为头提交）识别，报告一次：同一失败在之后的读取中不再报告；重跑或新提交上的再次失败是新的失败。
  读取至多 `CHECK_LIMIT` 个检查，全部保留，因此任何检查都不会每次读取都被当作新的失败。同一头提交上已报告的失败一直保留：
  检查在某次读取中缺席（超过 100 个检查时顺序不稳定，或汇总暂时缺项）后以同一运行 id 再出现，不再报告；检查再次出现而未失败（例如没有运行 id 的重跑进行中）时忘记它；
  新的头提交清空这些键。每个头提交至多保留 `2 × CHECK_LIMIT` 个键，超出时只保留这次读取的失败。
- **检查恢复**：`failing` 为真（自上次全部通过以来有过失败）且这次读取的检查全部通过或跳过。失败之后经过进行中（例如推送修复后 CI 重新运行）再全部通过时同样报告，
  每次恢复报告一次。
- **合并**：状态从非合并变为 `merged`（包括从 `closed`）。**关闭**：从 `open` 变为 `closed`。关闭后重新打开只更新状态。
- **不再能读取的 PR**：读取返回“PR 不存在”（`PullReadError::NotFound`）时，绑定的 `missing` 加一，其他失败与成功的读取把它清零。
  GitHub 上 PR 编号不存在（“Could not resolve to a PullRequest”）与仓库本身无法解析（“Could not resolve to a Repository”：仓库被删除，
  或主机账号看不到它，GitHub 对无权访问的私有仓库同样这样答复）都算不存在，后者只在跟随的读取中这样归类；GitLab 对删除或看不到的项目返回 404
  （“404 Project Not Found”），`mr view` 的 404 同样算不存在。连续 `MISSING_LIMIT = 8` 次（按读取失败的退避，约 75 分钟到一个半小时）后
  工单停止跟随这个 PR：删除该绑定、释放名额，记一条系统汇报说明原因；工单未关闭时另以 `pull_activity` 唤醒 Bot，已关闭的工单不唤醒
  （此后也不再因这个 PR 免于淘汰）。已排队的输入保留。
- **暂时失去访问**也会这样结束跟随：主机上 `gh auth switch` 到另一个账号、令牌被撤销或未经 SSO 授权，持续约 75 分钟以上时，
  私有仓库的 PR 读起来就是“不存在”，跟随随之结束。未关闭的工单唤醒 Bot，访问恢复后可以重新绑定；已关闭的工单静默停止跟随、之后可能被淘汰，
  不会自动恢复。这是有意的取舍：daemon 无法区分仓库被删除与账号暂时看不到它。

路由（Bot worker 中的 `State::observe_pull`，一次写入工单）：

| 事件                                 | 工单未关闭，有存活的工作者                                          | 工单未关闭，没有存活的工作者 | 工单已关闭但仍跟随（见下）                                     |
| ------------------------------------ | ------------------------------------------------------------------- | ---------------------------- | -------------------------------------------------------------- |
| 写入者的新条目（未被认领）、检查失败 | 投递给工作者                                                        | 通知 Bot：`pull_activity`    | 未达 `OPEN_LIMIT` 且有存活工作者时重新打开后投递，否则通知 Bot |
| 不能写入仓库的人的新条目             | 通知 Bot：`pull_activity`（合并为一条，只列出作者与链接，不含正文） | 同左                         | 同左，不重新打开                                               |
| 机器人或应用账号的新条目             | 只记为已处理                                                        | 同左                         | 同左                                                           |
| 检查恢复                             | 通知 Bot：`pull_checks_passed`                                      | 同左                         | 同左                                                           |
| 合并                                 | 通知 Bot：`pull_merged`；全部结束时结束工单                         | 同左                         | 通知 Bot；全部结束时开始 `FOLLOW_AFTER` 窗口                   |
| 关闭（未合并）                       | 通知 Bot：`pull_closed`（不自动取消）                               | 同左                         | 通知 Bot                                                       |

- **哪些工单跟随**（`Ticket::follows_pulls`）：有 PR 且未关闭（`open`、`waiting`）；以 `done` 或 `failed` 关闭、仍有打开的 PR（显式关闭后继续跟随到 PR 结束）；
  或 PR 结束后的 `followUntil` 窗口内。`cancelled` 表示放弃这项工作，不再跟随。Bot 原有的流程是告诉用户结果后关闭工单；
  工作者开出 PR、报告 `done`、Bot 关闭工单之后，审查意见、失败的检查与合并照样到达，因此跟随在常见流程中不会立即停止。
  Bot 的说明与 `bot_ticket_update` 的描述都写明：PR 仍打开的工单在合并时自行关闭，Bot 不必让它保持打开，也不必再关闭；只有 `cancelled` 停止跟随。
- **取消已关闭的工单**（修订 ADR-274 的“以另一种状态关闭已关闭的工单为冲突”）：`done` 或 `failed` 的工单可以改为 `cancelled`，以停止跟随并释放 `FOLLOW_LIMIT` 名额，
  包括 PR 合并后的窗口内；没有给出结论时保留原结论。已关闭工单之间的其他改变（例如 `done` 改为 `failed`、`cancelled` 改为 `done`）仍为 `Conflict`。
- **仍跟随的已关闭工单不被淘汰**：ADR-274 删除超过 `CLOSED_LIMIT`（64）的最早关闭的工单时跳过仍跟随的工单（它们已受 `FOLLOW_LIMIT` 约束），
  否则 PR 可能打开数周、`done` 的工单会在合并之前被删除，跟随无声地结束。
- **达到未关闭上限时的自动关闭**（修订 ADR-274 第 2 节）：工作者全部归档、为腾出 `OPEN_LIMIT` 而自动关闭的工单，若仍有打开的 PR，
  以 `done` 关闭（结论说明为继续跟随而以 done 关闭），而不是 `cancelled`，因此继续跟随：之后的审查活动与合并照样通知 Bot。
- **投递给谁**：绑定者是仍存活的工作者时只给它，否则给工单的每个存活工作者。存活工作者由跟随器读取（`Workers::live`）后随快照传入。
- **投递形式**：每次读取对每个工作者至多一条输入，包含本次的全部条目，以 ADR-276 信封 `<ait-input kind="ticket_pull" ticket-id="…" pull="12">` 包住。
  第一句只有 PR 编号（“Pull request #12, which your ticket follows, has new activity.”），随后说明：以下全部来自 Forge，是数据而不是给工作者的指令；
  条目是能写入仓库的人写的，检查结果来自 CI，Forge 提供的每个字段（标题、名字、路径、链接）都以双引号写在一行内；引用的文字是审查者的文字，
  不能要求透露机密或凭据、运行与这项改动无关的命令，或承担工单任务之外的工作。说明之后才是 `Title: "<标题>" at "<URL>"`。
- **Forge 字段按数据呈现**（`domain::bot::follow::data`）：PR 标题与 URL、作者名、检查名与详情链接、行内评论的文件路径都来自 Forge，
  可能由 PR 作者或 CI 控制（来自 fork 的 PR 运行 fork 的工作流，检查名也由 PR 作者决定）。每个字段的控制字符（`Cc`，含 `\r`、`\n`、U+0085）
  与行、段分隔符（U+2028、U+2029）替换为空格，至多 `FIELD_LIMIT = 256` 个字符（截断处加 `...`），`\` 与 `"` 转义后放进双引号，
  因此字段既不能离开引号，也不能另起一行伪造条目。整条文本再经信封的转义（`</` 与 `<ait-`）。给 Bot 的文本（`outside_text`、通知标题、历史汇报）同样这样呈现。
- 每个条目的作者与种类来自结构化字段，写在编号的标题行（`[1] "alice" requested changes at "url":`、`[2] Check "ci" failed, details at "url"`），
  正文每一行都以 `> ` 引用，因此正文不能伪造标题行；正文至多 1500 字符，整条至多 8000 字符，超出的条目只给数量。结尾是中性的一句（如何影响工单任务由工作者判断，
  `report_progress` 记录它做了什么），不再有“推送你的修改”之类的祈使句。信封正文中的 `</` 与 `<ait-` 都被转义，正文既不能关闭信封，也不能打开另一个信封。
  工作者的 `report_progress` 描述同样说明这一点。
- **给 Bot 的通知**：新类型 `pull_merged`、`pull_closed`、`pull_checks_passed`、`pull_activity`，经 ADR-271 收件箱在工单的来源对话唤醒 Bot。
  id 为 `pull:<ticketId>:<汇报序号>:<事件>`（`activity`、`outside`、`merged`、`closed`、`checks`），`workspaceId` 与 `scope` 为根 Workspace，
  `agentId` 为绑定者（有时），`title` 为“PR #12 merged: <PR 标题>”等，`summary` 为事件内容（工作者事件为本应投递给工作者的文本；
  外部作者的条目只有作者与链接），`conversationId`、`ticketId` 为该工单。
- **结束工单**：一次观察后，工单未关闭、它的每个 PR 都已合并或关闭、且至少一个已合并时，以系统来源关闭为 `done`，结论为“Pull request #12 merged”
  （多个时逐个列出），并设置 `followUntil = 关闭时间 + FOLLOW_AFTER`。已关闭的工单保持原状态，只开始窗口。全部关闭而没有合并时不关闭，只通知 Bot（`pull_closed`），由 Bot 决定。
- **恢复**：`followUntil` 内，写入者的一条非本账号新条目在有存活工作者时使工单以系统来源重新打开（汇报说明是哪个 PR 的新活动），
  `followUntil` 保留并至少延长到恢复时间加 `FOLLOW_AFTER`，然后按未关闭工单路由。达到 `OPEN_LIMIT` 或没有存活工作者时不重新打开，改为通知 Bot（`pull_activity`）。
  恢复后的工单（PR 全部结束、仍有窗口）按 `ENDED_INTERVAL` 读取；它的工作者报告 `done`，或窗口过去（在下一次读取或读取失败时检查）时，再次以系统来源关闭为 `done`，保留窗口。
  Bot 或用户显式关闭工单时清除 `followUntil`；显式重新打开后照常跟随（PR 全部结束时按 `ENDED_INTERVAL`）。

### 6. 恰好一次，只在空闲时投递

- 事件只由快照与持久的绑定状态决定；通知 id 与投递的 `messageId` 取自同一次观察中记录的汇报序号，因此重算得到相同的 id。
- 一次观察的写入顺序：先提交给 Bot 的通知（ADR-271 按 id 去重），再写工单：新的 `seen`、高水位、检查、状态、系统汇报、状态变化，以及要投递给工作者的输入
  放入工单的 `pending`（`{id:"follow:<ticketId>:<序号>:<agentId>", agentId, text}`，至多 `PENDING_LIMIT = 16` 条，满时新的投递改为通知 Bot）。
  写工单之前崩溃时下次读取重算出相同的事件与 id，通知按 id 去重；写工单之后，事件不会再产生。
- **只在空闲时投递**：输入一直留在 `pending` 中，直到工作者空闲（Agent registry 的状态不是 `running`、`initializing`）。空闲时以普通输入发送
  （`agent.message.send.request`，确定的 `messageId`），开始新的一轮；忙时 `Workers::deliver` 返回 `Busy`，不发送，跟随器每 `PENDING_INTERVAL` 重试。
  只有 Provider 接受它作为新的一轮之后，才以 `Bots::delivered` 从 `pending` 删除。因此输入从不进入 Provider 的输入队列，不会排在用户自己的 steer 或打断消息之前，
  也不会在工作者归档时随队列被取消而丢失。发送时使用 `steer`：只在检查与发送之间的瞬间工作者恰好开始了一轮时起作用，此时输入加入那一轮而不打断它。
- 投递之后、删除之前崩溃时重启后再投递一次，Provider 按 `messageId` 识别同一输入（持久的 `input_receipts`），不会开始第二轮。
  投递失败的条目留在 `pending` 中重试；重试时它的 Agent 已不存活（归档或不存在）则改为通知 Bot（`pull_activity`）后删除。
  存活的 Agent 不能接收同一条输入（`Workers::deliver` 返回错误）的失败按“一轮”计数：与上一次失败相隔超过 `DELIVERY_FAILURE_WINDOW = 10` 分钟的失败开始新的一轮，
  `Busy` 的答复（Agent 可达，只是正在执行）同样开始新的一轮。一轮中至少 `DELIVERY_FAILURE_LIMIT = 8` 次失败、且这一轮已持续 10 分钟时，
  同样改为通知 Bot 后删除，并记一条系统汇报说明原因（条目的 `failures`、`failingSince`、`failedAt` 持久保存在 `pending` 中），不再每 15 秒无限重试。
  10 分钟远长于 Provider 重启，因此重启期间的连续失败不会使输入改道；分散在数天中的偶发失败也不会累积。
- 同一工单输入的投递互斥：每个工单一把锁（`Ports::delivery_lock`，没有投递持有或等待的锁随即丢弃），跟随器的投递与回答（RPC 或 Bot 代答）
  触发的投递经它串行，发送与记录结果在同一把锁内完成，且回答的投递在锁内重新读取 `pending`。因此一条输入被一次投递接受、记录删除之后，
  另一次投递不会再为它计入失败而把它改道给 Bot。不同工单的投递互不等待。
- 已知窗口：Provider 的输入回执被清理（Agent 删除）后再重投同一 `messageId` 会被当作新输入，但 `pending` 只在投递与删除之间的毫秒级窗口内保留已投递的条目。
- **PR 输入开始的轮次结束不唤醒 Bot**：`delivered` 在工单的 `inputTurns` 中为每个工作者记下最近一次 **PR 输入**（`follow:` 开头的 id）的发送时间 `sent`
  （跟随器调用 `Workers::deliver` 之前取的时间）与被接受的时间 `at`。回答（`answer:`）不记录：回答之后的轮次照常结束并唤醒 Bot。
  ADR-274 的轮次结束规则据此扩展：一个正常结束的轮次，若它的开始时间在 `sent` 与 `at` 之间、结束时间不早于 `at`，就是这条输入开始的轮次；
  这个轮次中工作者没有报告会唤醒 Bot 的结果（`blocked`、`done`、`failed`）时，结束不唤醒 Bot；报告了结果时按原规则处理（结果的汇报已经唤醒了 Bot）。
  失败的轮次照常唤醒（“有疑问时唤醒”），其他轮次的规则不变；合并、关闭、检查恢复照常作为结果唤醒 Bot。
- 工作者的汇报从不因输入轮次而不唤醒 Bot：在任何轮次中报告的 `done` 都唤醒 Bot（只有轮次的**结束**可能被省略），因此没有 PR 的工单在回答之后完成时，Bot 照样得知。
- **记录的边界**：`delivered` 运行时，若该工作者在 `sent` 之后的轮次结束已经记录（轮次很快结束，或崩溃后重投时那一轮早已结束），不再记录；
  该工作者在 `sent` 之后的任何轮次结束被记录时清除这条记录。因此一条记录不会覆盖它没有开始的轮次：之后由 Bot 或用户开始的轮次开始时间晚于 `at`，不匹配。
  Provider 的发送回执只有 `accepted`，没有轮次 id，因此用发送时间与 daemon 记录的轮次开始时间（`lastUserMessageAt`）界定轮次。
- **剩余的竞争窗口**：空闲检查与发送不是 Agent 管理器中的一步（这需要改变 Provider 的 steer／interrupt 行为，本阶段不改）。
  在检查与发送之间的毫秒内由 Bot 或用户开始的一轮会收到这条输入：支持 steer 的 Provider 把它并入那一轮，steer 把轮次开始时间更新为发送时间，
  那一轮的结束可能被省略（其中的 `done` 等汇报照常唤醒 Bot）；不支持 steer 的 Provider 退回到打断那一轮。这个窗口只有毫秒级。

### 7. 本账号写的评论

Forge CLI 以主机上的账号登录，工作者用同一个 `gh` 发评论；而所有者通常也用同一账号在浏览器里审查。全部丢弃本账号的评论会丢掉所有者的审查，
全部投递又会把工作者自己的评论喂回给它，可能形成循环。规则：

- 读出的 `own` 条目、比绑定的高水位新的（更早的已在 `seen` 中或已处理），跟随器取工单的存活工作者最近 200 条时间线中**工具调用发送的内容**
  （`Workers::inputs`：每个 `tool_call` 条目的 `detail` 中除结果以外的字符串，如 shell 的 `command`、未知工具的 `input`；不含 `output`、`result`、`log`，
  不含读取与搜索返回的 `content`，也不含用户消息、助手消息与投递的输入）。显示时间线保留工具参数至每个字段的预览上限（Claude 为 32 KiB），远大于一条 Forge 评论。
- 评论正文只保留字母与数字、转为小写后**整体**包含在某个工具调用的内容（同样归一）中，即视为工作者自己写的，记为已处理、不投递；正文归一后不足 16 个字符时不判定。
  引用工作者评论再加上自己意见的评论（例如 GitHub 的“Quote reply”）不整体包含在工作者发送的内容中，照常投递。
- 其余 `own` 条目照常投递给工作者，并标明“由本主机 Forge CLI 的账号所写；可能是你自己的评论”。
  这包括工作者自己写的短评论：例如 Bot 关闭工单后工作者的轮次仍在继续，在仍打开的 PR 上写了“PTAL”或“Fixed.”（归一后不足 16 个字符，不判定）。
  这样的评论会重新打开已关闭的工单，并带“可能是你自己的评论”的标注回送给那个工作者。
- 已关闭但仍有打开的 PR 的工单（例如 Bot 在工作者报告 `done` 后关闭了它）被本账号的条目重新打开并投递，同样带上述标注：
  所有者最常用同一账号在浏览器里审查。只有 PR 全部结束之后，本账号的条目才不恢复工单（那是所有者对已完成工作的备注），只记为已处理；
  这时只有写入者的非本账号条目恢复工单。
- 限制：工作者通过不进入时间线的方式发评论（例如 `--body-file` 写的文件内容不在命令中）、正文在 shell 中被转义（`\n` 等）时，本账号的评论会以上述标注投递给工作者；
  所有者的评论不会因此被丢弃。

### 8. 结构化提问与回答

- `report_progress` 在 `kind: "blocked"` 时接受可选 `question`（1–512 字符，一个具体的问题）；其他种类带 `question` 返回引导文本。
  `Waiting` 新增可选 `question`（为空时不写出），工单等待该工作者时出现在概览、`bot_read`、`ticket_read`、`bot.ticket.list/get` 与客户端；
  等待交给另一个受阻的工作者时随之换成它的问题。外部对话看到的别人的工单存根不含问题。
- **所有者回答**：新的客户端 RPC `bot.ticket.answer.request`：`{"workspaceId","ticketId","agentId","answer":"…"}`（1–2048 字符）→ `{"ticket":完整投影,"delivered":bool}`。
  回答指名它回答的工作者（客户端显示问题时的 `waiting.agentId`）；要求工单仍在等待这个受阻的工作者，否则 `BotConflict`，客户端说明等待可能已转到另一个工作者并重新加载。
  这样，两个工作者都受阻、等待在所有者回答之前转到另一个工作者时，回答不会送给另一个问题。记录 `source: "user"`、`kind: "answer"`、`agentId` 为该工作者的汇报，
  按 ADR-274 第 4 节解除该工作者的受阻（没有其他受阻者时回到 `open`），并在 `pending` 中放入发给它的输入
  （信封 `<ait-input kind="ticket_answer" ticket-id="…">`，含问题与回答，`messageId` 为 `answer:<ticketId>:<序号>`），随后立即投递，失败时由跟随器重试（第 6 节）。
  `delivered` 为立即投递是否成功。暂停的 Bot 也接受（用户的显式操作）。访客与 Webhook 自动化不能调用（不在 node 访客的只读白名单中）。
- **Bot 代为回答**：`bot_ticket_update` 新增可选 `answer: {agentId, text}`（`text` 1–2048 字符），用户在对话中回答后由 Bot 转交：同样的记录与投递，来源为 `bot`，
  因此历史能区分回答来自哪里。工单不再等待该工作者时返回引导文本（等待可能已转到另一个工作者，读取工单后回答 `waiting.agentId`）。
- 回答与 PR 输入一样放入 `pending`，只在工作者空闲时投递（第 6 节）。暂停的 Bot 的工单接受回答，但跟随集合不含暂停的 Bot，回答在恢复后投递。
- 客户端：任务详情显示等待中的问题、回答输入框与发送按钮（只对能写的连接显示），以及 PR 列表（编号、标题、状态、最近一次读取失败）；SDK 方法 `answerWorkspaceBotTicket`，
  特性 `workspaceBotTicketAnswers` 由 Host 是否实现 `bot.ticket.answer.request` 决定。

### 9. 隔离

ADR-280 第 4 节的规则同样适用：外部对话的 Bot 只能对自己发起的工单绑定 PR 与回答（`bot_ticket_update` 已先检查来源）；
别人的工单存根不含 `pulls` 与问题；工作者只能绑定自己的工单（凭据中的工单）。跟随器的投递只发给该工单的 Agent，通知回到工单的来源对话。

### 10. 投影

- 客户端投影（`bot.ticket.list/get/update/answer`）与 `bot_tickets`：`pulls` 为 `[{number,url,title,boundBy,agentId,boundAt,state,checks:{failing,pending,total},polledAt,error}]`，
  不含 `seen`、`head` 与 `pending`；`waiting` 带 `question`。
- `bot_read ticket/<id>`、`ticket_read`：同样的 `pulls` 与 `waiting.question`；概览的工单条目带 `pulls`（只有编号、状态与 `error`）。
- 汇报种类新增 `pull`、`answer`。

### 11. 不改变 Provider 的投递

本阶段曾为 `agent.message.send.request` 增加 `activeTurnBehavior: "queue"`，让输入排在正在执行的一轮之后。它会过早交出输入（工作者归档时随队列取消而丢失），
并且排队期间用户自己的 steer 会退化为打断、打断消息排在 PR 输入之后。现在跟随器只在工作者空闲时投递（第 6 节），`queue` 已删除，
Provider 的 `interrupt` 与 `steer` 与之前完全相同；没有其他调用者使用 `queue`。

### 12. 单连接能力预算

本阶段只新增一个方法 `bot.ticket.answer.request`。[ADR-282](../daemon/adr-282-single-connection-capability-budget.md) 已把单连接能力提议预算
`model::server::single::MAX_CAPABILITIES` 从 ADR-074 的 256 提高到 512，本 ADR 不再修改它：加入这个方法后整份目录仍在预算之内，
由 ADR-282 在 `crates/api` 的余量测试守护。`@ait/client` 在单连接模式下提议的能力加上这个方法后仍低于旧上限 256，
因此新客户端连接 ADR-282 之前的 daemon 不受影响。分连接每条至多 64 个的规则不变，`bot.ticket.answer.request` 所在的第 3 条连接仍在 64 以内
（`@ait/client` 的测试检查每条连接）。

### 13. 降级兼容

- 工单的 `pulls`、`pending`、`followUntil` 与 `Waiting.question` 是新增字段，旧 daemon 忽略（工单记录不拒绝未知字段）。
- 新的汇报种类 `pull`、`answer` 与通知类型 `pull_merged`、`pull_closed`、`pull_checks_passed`、`pull_activity` 会使旧 daemon 无法解析含有它们的工单文件、
  通知日志与唤醒轮次，Bot 存储加载失败（与 ADR-274 第 13 节相同）。降级前需删除 `<data>/workspace-bots/`。
- 旧客户端忽略 `pulls` 与 `question`；不支持回答的 Host 不报告 `bot.ticket.answer.request`，客户端不显示回答框。
- 旧客户端的用户消息 `origin` 是封闭的联合，不认识 `ticket_pull`、`ticket_answer`；一个这样的条目会使整页时间线（`agent.timeline.get`）解析失败。
  因此 daemon 只把这两种来源发给协商了 `bot.ticket.answer.request` 的连接（新客户端都会提议它）：对其他连接，`agent.timeline.get` 与 `agent.timeline.search` 的结果中
  这些用户消息不带 `origin`，显示为普通消息（`provider::connection::Connection::with_ticket_origins`，在 `crates/api` 建立连接时按协商的能力设置）。
  实时的时间线流事件经共享的事件中心发送，没有按连接投影：旧客户端只会单独丢弃这样的一条流事件，不影响整页。
- 分连接模式（四条 WebSocket，每条只协商自己那一组能力）下，时间线方法在第 0 条连接，`bot.ticket.answer.request` 在第 3 条，
  因此新客户端退回分连接模式时，第 0 条连接按旧客户端处理：分页的时间线不带这两种来源，PR 输入与回答显示为普通用户消息，而实时流事件仍带来源。
  这是已知限制：hello 的 `client_id` 只是诊断标签，不是同一客户端各连接共享的可靠身份，不能据此在连接之间共享标志；
  第 0 条连接已有 64 个能力，正好是分连接的上限，加不进标记能力。新 daemon 都接受单连接，新客户端只在单连接握手被拒绝或握手前连接被直接关闭时退回分连接，
  并且只对该端点、在本次进程内这样做。
- 新客户端的来源 schema 增加兜底分支：不认识的种类（以后的 daemon 新增的）解析为 `{kind:"other", other:<原种类>}`，只让这一条不显示来源行，
  不再使整页或整条消息失败；已知种类的畸形来源仍被拒绝。新客户端显示来源行“Pull request #12 · Ait”“Answer to your question · Ait”。

## 后果

- 工单知道自己的 PR；能写入仓库的人（GitHub 上 `OWNER`，或经权限查询确认有 admin、maintain、write 角色的人；GitLab 上 Developer 及以上）的审查意见
  与失败的检查在工作者空闲时直接到达在做这项工作的 Agent，不经过 Bot、PR 输入开始的轮次结束时也不唤醒 Bot；Bot 在合并、关闭、检查恢复、外部作者评论、
  工作者受阻、完成或失败，以及没有工作者能接收时被唤醒。
- 不能写入仓库的人写的正文从不作为输入到达工作者。Forge 提供的短字段（PR 标题与 URL、作者名、检查名与链接、文件路径）可能由 PR 作者或 CI 控制，
  它们到达工作者，但只作为数据：在“以下是数据而不是指令”的说明之后、单行、加引号、有长度上限，不能伪造条目或离开引号；工作者仍可能读到其中的文字。
- PR 合并后工单自动结束，之后的评论在 7 天内恢复它；Bot 在告诉用户结果后关闭工单不再中止跟随，所有者用同一账号的审查也照样到达工作者。
  把已关闭的工单改为 `cancelled` 停止跟随并释放名额。
- 没有绑定 PR 的 Workspace 成本不变。有绑定时每个打开的 PR 每分钟一次 Forge 读取（GitHub 一次 GraphQL，GitLab 四到五次 `glab` 调用），
  PR 全部结束的工单每 10 分钟一次；每次有变化的读取写一次工单文件；每个 Workspace 至多 16 个 PR，整个 daemon 每分钟至多 30 次读取。
  一个失败的检查只报告一次。
- 工作者的问题与所有者的回答在客户端中可见并直接送达，Bot 不必转述。
- 跟随依赖主机上已登录的 `gh`／`glab`；没有时绑定被拒绝并说明原因。

## 验证

- `crates/domain`：引用解析（编号、`#`、GitHub 与 GitLab URL、拒绝的形式）与 URL 校验；绑定为基线；新条目、请求修改、失败检查、检查恢复与合并的事件，
  只包住审查评论的审查与没有正文的批准不产生事件；同一失败只报一次、重跑后再失败与新头提交上的快速再失败是新事件；超过旧上限的检查中的失败只报一次；
  经过进行中的恢复报告检查恢复且只报一次；短读取不使已处理的条目重新出现，高水位之前的条目永不投递；先关闭后合并报告合并；
  外部作者的条目为 `Outside`、机器人账号只记为已处理；工作者写的条目只记为已处理；失败读取计数与清除；投递文本的标注、引用、截断与上限，正文不能伪造条目；
  整体匹配（引用回复不被认领）；工单上的提问、再次受阻换问题、交接时带上下一个问题、回答解除受阻并排入投递、回答必须指名仍在等待的工作者、
  Bot 代答与不能与 `status` 同用；绑定的幂等、上限与已关闭拒绝；投递只给绑定的工作者且不通知 Bot；许多检查中的一个失败只排一个输入；
  没有存活工作者或待投递已满时改为通知；外部作者合并为一条通知且不含正文、不恢复已结束的工单；合并结束工单、本账号评论不恢复、他人评论恢复并投递；
  以 `done` 关闭仍打开 PR 的工单继续跟随、审查意见重新打开、合并结束，取消停止跟随；恢复的工单在 `done` 或窗口过去后再次结束；
  投递开始的轮次结束不唤醒 Bot 而失败的轮次与之后的轮次照常；达到未关闭上限时不重新打开；关闭只通知；全部结束才结束；
  新字段的往返与缺省；信封 `ticket_pull`、`ticket_answer` 的往返与转义，正文不能打开嵌套信封。
  第二次复审的回归测试（`ticket/tests/regressions.rs` 与 `follow/tests.rs`）：PR 仍打开时本账号的评论重新打开已关闭的工单并投递，达到上限时通知 Bot；
  回答不记录输入轮次，回答后没有汇报就结束的轮次唤醒 Bot；有结果汇报的 PR 输入轮次按原规则；轮次结束先于 `delivered` 记录时不记录；
  开始早于发送的轮次不省略；发送之后的轮次结束清除记录；自动关闭仍有打开 PR 的工单为 `done` 并继续跟随；已关闭的工单只能改为 `cancelled`（包括合并后的窗口内）；
  Forge 字段以单行、加引号的数据出现在说明之后，控制字符与分隔符不能另起一行，过长的字段被截断；缺席一次后以同一运行 id 再出现的失败不再报告、新头提交上照常报告；
  `Unconfirmed` 作者的条目在查询前暂缓、不越过高水位，查询确认后投递一次并保存结果，确认不能写入时为 `Outside`。
- `crates/filesystem`：GitHub 的 GraphQL 投影（状态、检查、本账号标记、作者权限与机器人、`MEMBER`／`COLLABORATOR` 为 `Unconfirmed`、发布日期、未提交审查不读出，
  `PENDING` 审查或没有 `publishedAt` 的审查评论不读出）；写入权限查询的路径、角色判定、404 为不能写入、其他失败为失败、非法登录名不查询；合并与关闭、PR 不存在为 `NotFound`、
  其他 GraphQL 错误为失败；查询读取 `CHECK_LIMIT` 个检查；经伪 `gh` 按 origin 与编号读取；CLI 缺失为不可用；
  GitLab 经伪 `glab` 只调用一次 `mr view`，按 `sort=desc` 读取最新备注，按成员权限与命名判定写入者与机器人，行内评论带位置；流水线或用户读取失败使整次读取失败；
  `locked` 为打开；404 为 `NotFound`。
- `crates/provider/runtime`：对未协商新来源的连接，时间线页面去掉 `ticket_pull`、`ticket_answer` 来源而保留其他来源。
- `crates/bot`（Bot 状态）：没有绑定时跟随集合为空；汇报绑定后的投影与跟随集合；暂停的 Bot 不跟随；每个 Workspace 的上限与重新打开时的上限；
  投递排队而不唤醒 Bot；外部作者唤醒 Bot 而不投递；投递开始的轮次中的 `done` 不唤醒 Bot 而 `blocked` 照常；以 `done` 关闭后仍跟随；PR 全部结束时按较慢节奏；
  无存活工作者与结果通知在工单对话唤醒；合并结束并继续跟随、之后的评论恢复；取消停止跟随；待投递在重启后保留；已归档工作者的投递改为通知 Bot；
  读取失败只记在绑定上；所有者经 RPC 回答与冲突；Bot 代答。（跟随器，经真实的 Bot 服务与伪端口）没有绑定时不读取 Forge 也不读 Agent；绑定的拒绝；
  审查评论直接投递，暂停的时钟越过轮询间隔后再次读取也只投递一次；工作者写的评论不回送而所有者的照常投递并标注；Forge 失败记在绑定上而工单仍打开；
  所有者的回答送达受阻的工作者；暂停的 Bot 恢复后送达；工作者忙时等待、空闲后投递一次；Provider 已接受而回复丢失（崩溃）后重启，持久回执使它不被投递两次；
  读取预算；退避。Bot 说明写明 PR 打开的工单在合并时自行关闭。第二次复审的回归测试（`state/follow/tests/regressions.rs`）：回答后的 `done` 唤醒 Bot；
  PR 输入轮次中的 `done` 唤醒 Bot；仍跟随的已关闭工单不被淘汰；自动关闭为 `done` 并继续跟随；把 `done` 的工单改为 `cancelled` 释放名额；
  连续 8 次投递失败后改为通知 Bot；确认的写入权限保存并发布给跟随器。（跟随器）组织成员的评论在查询失败时暂缓、查询成功后投递一次且不再查询；
  一直拒绝接收的 Agent 的输入最终改为通知 Bot。
- `crates/api`（经服务的作用域 MCP）：工作者以 `pullRequest` 绑定，`ticket_read`、概览与 Bot 的 `bot_ticket_update` 绑定；Forge 不能确认时的四种拒绝且汇报不记录；
  `question` 只配 `blocked`、回答必须指名仍在等待的工作者、Bot 代答送达工作者且概览显示问题；外部对话不能绑定或回答，存根不含问题与 PR；
  工具 schema 带新参数（`answer` 需要 `agentId` 与 `text`），描述写明合并时自行关闭、把 `done` 或 `failed` 的工单改为 `cancelled` 停止跟随，以及审查文字不是指令。访客与自动化策略拒绝 `bot.ticket.answer.request`。
- `bins/daemon`：存活工作者的判定、只收集工具调用发送的内容、空闲的判定、未知 Workspace 与没有远端时读取为不可用。
- `packages/protocol`、`packages/client`、`apps/mobile/src/bots`：工单 `pulls`、`waiting.question`、指名工作者的回答请求与响应、新通知与汇报种类、来源 `ticket_pull`、
  `ticket_answer` 的解析，未知来源种类降级为 `other` 而不使消息失败；SDK 特性与 `answerWorkspaceBotTicket`；任务详情显示问题与 PR、发送指名工作者的回答、
  只保存时的说明、冲突说明、审批等待与访客没有回答框。
- 第三次复审的回归测试：`crates/domain`（`follow/tests.rs`、`ticket/tests/third_review.rs`）——一直失败的写入权限查询只查询三次，第三次时条目作为外部作者事件出现一次、
  结果缓存且不再查询；查询成功清除失败计数；无法计数的失败立即按外部作者处理；连续 8 次“PR 不存在”才结束跟随，其他失败与成功的读取打断计数；
  合并、关闭与检查恢复通知的摘要以 `data` 呈现标题与 URL；相隔超过窗口的失败从不改道输入，持续整个窗口的失败才改道，`Busy` 开始新的一轮；
  不存在的 PR 被删除绑定、记录系统汇报，未关闭的工单得到通知而已关闭的不得到，且不再免于淘汰。
  `crates/filesystem`：没有推送权限的 403 返回“不能写入”，速率限制的 403 仍是失败。`crates/bot`：状态层的窗口改道、`Busy` 重置、不存在的 PR 释放名额并唤醒 Bot；
  跟随器中一直失败的查询三次读取后通知 Bot 且不再查询，预算不足而跳过的查询记为失败，一直不存在的 PR 停止读取，拒绝接收的 Agent 在窗口内不改道、恢复后投递一次。
- 验证复核的回归测试：`crates/filesystem`——`gh` 以“Could not resolve to a Repository”退出的跟随读取为 `NotFound`（与 PR 编号不存在相同，502 不是），
  GitLab 的“404 Project Not Found”为 `NotFound`；`crates/domain`——因失败放弃的作者不写入 `confirmed`、已放弃的条目不再查询、之后的新条目再次查询并在答复为能写入时投递给工作者；
  `crates/bot`——每个工单一把投递锁、不用的锁被丢弃，确定的答复不再查询。
- 未验证：真实 GitHub／GitLab 仓库上的读取（包括 GitLab `user_ids[]` 的查询编码、GitHub 权限接口对非协作者的实际响应、未提交审查评论的 `publishedAt`）、真实 Provider 会话中工作者是否报告 PR 与问题、
  daemon 的 `AgentWorkers` 对真实 `AgentExecution` 的空闲判定与投递（daemon 测试没有带伪 Provider 的执行环境）、旧客户端对新 daemon 的端到端连接、
  任务界面在真机与浏览器端到端用例中的表现。

## Test coverage

未测量：本次未要求覆盖率统计。
