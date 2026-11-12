---
id: ADR-248
status: accepted
date: 2026-11-11
amends:
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-213-single-workspace-manager-bot.md
paths:
  - apps/mobile/src/bots
  - bins/daemon/src/host/bot.rs
  - crates/api
  - crates/bot
  - crates/domain/src/bot.rs
  - crates/filesystem/src/git/local/worktrees
  - packages/protocol/src/bot
---

# ADR-248：GitHub App Channel 执行冻结版本的 PR Review

- 状态：Accepted
- 日期：2026-11-11

## 背景

Workspace Bot 需要服务多个 GitHub 仓库，并在 PR 评论中通过 `@bot review` 召唤，或在 PR
把 GitHub App 账号选为 reviewer 时自动执行 review。GitHub 事件由公网 webhook 到达，而
Bot 执行依赖本地 Project、Provider 隐藏会话和受 daemon 管理的 worktree。

事件接纳与执行之间可能经过排队，PR 分支也可能继续更新。若仅按分支名检出，模型实际审阅
的版本可能不同于触发事件；若评论和 reviewer 指派分别执行，同一版本还会重复计费和发言。

## 决策

每个 GitHub Channel 对应一个 GitHub App installation，并可包含多个 `repository -> projectId`
绑定。一个 Bot 可拥有多个 Channel，因此既支持一个 installation 下的多仓库，也支持多个
GitHub 组织或 installation。每个仓库单独配置允许触发 review 的 GitHub login。

公网入口由 center 为 Channel 签发，`ait-server` 校验 700 KiB 请求上限和 GitHub
`X-Hub-Signature-256` HMAC，再将原始 body 作为持久化 delivery 排队。Host 上线后，center 只通过
受限 service session 的 `bot.github.webhook.receive.request` 转发；daemon 按 workspace 和 Channel
重新解析并校验相同签名、installation、仓库和发送者允许列表。GitHub App private key 只保存环境
凭据引用；center 签发的 webhook secret 只保存于 server 与 Host 的私有 credential store，不写入
公开的 Bot Channel 记录。

创建 GitHub Channel 使用 `bot.channel.github.create.request`。center 原子签发 URL 和一次性显示的
secret；Host 成功持久化 Channel 与 secret 后才完成创建，失败则回收 center endpoint。删除 Channel
时 Host 同步回收 endpoint。center 以 GitHub delivery ID 做持久化去重和重试，daemon 继续以
`channel + repository + pull number + head SHA` 避免同一代码版本重复执行 review。

Channel 接纳两种事件：

- `issue_comment.created`，且 PR 评论包含 `@<app> review`；其后文本作为附加 review mandate。
- `pull_request.review_requested`，且 `requested_reviewer` 是当前 App bot。
- `issues.assigned`，该 Issue 是 PR 且 `assignee` 是当前 App bot。

接纳时固定 PR 的 base/head SHA。幂等键为 `channel + repository + pull number + head SHA`；同一
head 先到的评论召唤或 reviewer 指派拥有该轮次，后续触发复用已有回执。新 head 会产生新的
review。

执行前由 metadata 的 `WorktreeProvisioning` 创建 daemon 管理的临时 Workspace。GitHub 查询
和 Git fetch 都必须与接纳的 base/head SHA 一致，否则轮次失败，不审阅漂移后的代码。Provider
隐藏会话在临时 Workspace 中以只读 mandate 执行，并要求返回 summary 与可定位 finding 的
结构化 JSON；原生支持 output schema 的 Provider 同时使用 schema 约束，其余 Provider 使用同一
JSON prompt 并在无法解析时降级为 summary。Bot 使用 GitHub pull-request review API 发布总评和 inline comments；行定位失效
导致 422 时降级为普通 PR 评论。投递成功后归档临时 Workspace。执行准备失败也在 PR 留下安全
的失败说明。

## 结果

- GitHub App 权限至少需要读取 metadata/content/pull requests，以及写 pull requests/issues；
  订阅 Issue comments 与 Pull request 事件。
- GitHub App installation 是 Channel 的账号身份；同一 daemon 不允许两个启用 Channel 同时
  占用同一个 installation。
- daemon 不暴露无需 token 的 GitHub HTTP 入口；center 是唯一公网入口，并以平台签名认证、持久化
  队列和受限 Host service session 形成两段信任边界。
- Bot 不 merge、approve、request changes、push 或修改 checkout；后续若增加这些能力，需要新的
  显式授权与决策。
- 临时 Workspace 与冻结 SHA 扩展了 Bot、worktree 和 API 的边界，故由本 ADR 记录并在文档索引
  中公开。
