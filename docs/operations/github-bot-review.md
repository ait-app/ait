# GitHub Bot PR Review

一个 Workspace Bot 可以创建多个 GitHub Channel；一个 Channel 对应一个 GitHub App
installation，并可绑定该 installation 中的多个仓库。每个仓库必须映射到 daemon 已登记的
Project。

## GitHub App

创建 GitHub App，并配置：

- Repository permissions：Metadata read、Pull requests read/write、Issues read/write（发布失败
  说明时使用 issue comment）。
- Subscribe to events：Issue comment、Pull request。PR 的 assign 属于 Pull request 事件。
- Webhook URL：创建 GitHub Channel 后由 Ait 界面返回的 center URL。必须配置为 GitHub App 的
  webhook：仓库级 webhook 的事件不带 `installation`，会被忽略。
- Webhook secret：与 URL 同时一次性显示；复制到 GitHub App 后妥善保存。

拉取 PR 代码不使用 App 的 installation token，而是使用 Host 上已登录的 GitHub CLI（`gh auth
login`）和 Project 的 git remote。该账号必须能读取每个绑定的仓库；私有仓库和来自 fork 的 PR 也
一样。

GitHub 不会自动重投失败的投递。center 暂时无法接收（例如 502、429）或 Host 离线超过 30 分钟时，
在 GitHub App 的 `Advanced → Recent Deliveries` 中手动 Redeliver；delivery ID 不变，center 会把
已失败的记录重新排队。

`ait-server` 原样验证请求体和 `X-Hub-Signature-256`，限制请求体为 700 KiB，并持久化 delivery；
Host 在线后通过受限 service session 接收事件并再次校验签名。无需把 daemon 暴露到公网。

安装 App 后，从 installation 页面或 webhook payload 取得 numeric installation ID。App ID、
installation ID 和 App slug 不是秘密；private key 和 webhook secret 是秘密。

## daemon 凭据

启动 daemon 前注入：

```sh
export AIT_SERVER_CREDENTIAL_GITHUB_APP_PRIVATE_KEY="$(cat app.private-key.pem)"
```

GitHub 下载的是 PKCS#1 `RSA PRIVATE KEY`；转换后的 PKCS#8 `PRIVATE KEY` 也可以。私钥可以保留
原有换行，也可以写成单行并以字面 `\n` 表示换行；长度上限 16 KiB。Bot 记录只保存 private key 的 `env:` 引用，不保存
私钥或短期 installation token。Webhook secret 由 center 签发，并保存于 server 与 Host 的私有
credential store。

## Channel 配置

在 Bot 的 `Settings → Channels` 中点击 `Add channel`，Channel 类型选择 GitHub，填写 App ID、installation ID、
Bot mention 和 private key 的 `env:AIT_SERVER_CREDENTIAL_*` 凭据引用。然后为 installation 中的
每个 GitHub 仓库选择当前 Host 已登记的 Ait Project，并设置允许召唤 review 的 GitHub login。

首次保存后，界面显示 center 签发的真实 webhook URL 和仅显示一次的 secret。复制二者并配置到
GitHub App；关闭结果页后不会再次返回 secret。界面不会读取或显示 private key。

也可通过认证 WebSocket 直接配置：

先为 Workspace 创建 Bot 并预分配 `channelId`，再通过认证 WebSocket 调用：

```json
{
  "type": "request",
  "request_id": "github-channel-1",
  "method": "bot.channel.github.create.request",
  "params": {
    "workspaceId": "<workspace-id>",
    "channelId": "<channel-uuid>",
    "enabled": true,
    "config": {
      "kind": "github",
      "appId": "123456",
      "privateKeyRef": "env:AIT_SERVER_CREDENTIAL_GITHUB_APP_PRIVATE_KEY",
      "webhookSecretRef": "env:AIT_SERVER_CREDENTIAL_STORED_<CHANNEL_UUID_WITHOUT_DASHES>",
      "installationId": "789012",
      "mention": "ait-review",
      "repositories": [
        {
          "fullName": "example/backend",
          "projectId": "project-backend",
          "allowedSenders": ["maintainer", "release-manager"]
        },
        {
          "fullName": "example/frontend",
          "projectId": "project-frontend",
          "allowedSenders": ["maintainer"]
        }
      ]
    }
  }
}
```

响应中的 `url` 与 `secret` 只用于首次配置 GitHub App。请求中的 `webhookSecretRef` 是协议兼容字段；
Host 会覆盖为该 Channel 的私有 stored credential reference，调用方不能指定实际 secret。

同一 installation 在一个 daemon 中只能由一个未删除的 Channel 占用。需要连接其他组织或另一个
installation 时，在同一个 Bot 下创建另一个 Channel。

## 触发与结果

允许列表中的用户可以：

- 在 PR 评论 `@ait-review review`，后续文字会作为附加 review mandate；
- 把 PR assign 给 `ait-review[bot]`。

GitHub 不允许把 App 的 bot 账号请求为 reviewer：`requested_reviewers` 接口会返回成功但不添加
（2026-11-28 实测）。daemon 仍接受指向该 bot 的 `pull_request.review_requested`，以便 GitHub 日后
支持时直接生效。

review 会话只读取冻结检出，不提供 Ait MCP 工具，使用独立的 reviewer 指令（Workspace memory
仍然适用）。Provider 自带的连接器（例如 Codex 的 GitHub app）不受 Ait 管理，取决于该 Provider
账号的配置。

Bot 固定事件中的 base/head SHA，创建临时 worktree，返回 summary 与 inline findings，成功投递
后归档 worktree。同一 `repository + PR + head SHA` 在同一 Channel 只执行一次；push 新提交后
head SHA 改变，可再次触发。base 分支在接纳后继续前进时，只要冻结的 base 仍在 base 分支历史中，
就按接纳时的 `base...head` 审阅。head 必须与接纳时完全一致。

Bot 以带 inline finding 的 PR review 发布结果。执行或投递结果无法确认时，可推送新提交后再触发。
Bot 不会 merge、approve、request changes 或 push。

发布时遇到 GitHub 限流（429，或带 `Retry-After` / `x-ratelimit-remaining: 0` 的 403）、installation
token 被拒（401）或连接失败，Bot 按 GitHub 给出的等待时间重试，单次最多 5 次、每次最多等待 60 秒；
这些情况下 GitHub 尚未处理请求，不会重复发布。5xx 等结果不明的失败不重试。review 被 422 拒绝时
（常见于 inline 位置失效），先把 findings 并入正文重发 review，仍被拒绝才改为普通 PR 评论。
installation token 缓存 55 分钟。

## 状态评论

Bot 在 PR 上创建一条状态评论。每个提交有一段，每次变化追加一行，包含 emoji 和 UTC 时间。
已有状态行全部保留，失效时加删除线。推送新提交时，旧提交追加 ⏭️ 已过期，正在运行的审阅仍可
完成，并将结果发布到原提交。

状态包括 📥 已请求、⏳ 已排队、📦 准备检出、🔍 审阅中、⏸️ 限流重试、✅ 已发布、❌ 失败、
⚠️ 结果无法确认和 🚫 已取消。发布时显示各级别的问题数量，以及 GitHub 返回的 review 链接。
失败原因和无法确认的提示也追加到状态评论。通过评论触发时，Bot 会给触发评论加 👀。

状态评论语言在创建 Channel 时选择，默认英文，也可选简体中文。daemon 重启后按隐藏标记或已保存的
评论 ID 继续同步。评论达到 48 KiB 时折叠较早的提交，接近 60 KiB 时另起评论续写并互相链接。
更新最多每 3 秒合并一次。状态同步失败时，review 仍可发布。
