# GitHub Bot PR Review

一个 Workspace Bot 可以创建多个 GitHub Channel；一个 Channel 对应一个 GitHub App
installation，并可绑定该 installation 中的多个仓库。每个仓库必须映射到 daemon 已登记的
Project。

## GitHub App

创建 GitHub App，并配置：

- Repository permissions：Metadata read、Contents read、Pull requests read/write、Issues
  read/write。
- Subscribe to events：Issue comment、Issues、Pull request。
- Webhook URL：`https://<public-host>/v1/bots/github/<channelId>/webhook`。
- Webhook secret：生成随机 secret，并只放入 daemon 环境。

公网代理必须原样转发请求体和 `X-GitHub-*`、`X-Hub-Signature-256` headers。该入口不接受
daemon bearer token，以 GitHub HMAC signature 认证，并限制请求体为 1 MiB。

安装 App 后，从 installation 页面或 webhook payload 取得 numeric installation ID。App ID、
installation ID 和 App slug 不是秘密；private key 和 webhook secret 是秘密。

## daemon 凭据

启动 daemon 前注入：

```sh
export AIT_SERVER_CREDENTIAL_GITHUB_APP_PRIVATE_KEY='-----BEGIN RSA PRIVATE KEY-----
...
-----END RSA PRIVATE KEY-----'
export AIT_SERVER_CREDENTIAL_GITHUB_WEBHOOK_SECRET='<random-secret>'
```

也支持 GitHub 下载的 PKCS#8 `PRIVATE KEY`。Bot 记录只保存 `env:` 引用，不保存 secret 或短期
installation token。

## Channel 配置

在 Bot 的 `Settings → Channels` 中点击 `Add GitHub Channel`，填写 App ID、installation ID、
Bot mention 和两个 `env:AIT_SERVER_CREDENTIAL_*` 凭据引用。然后为 installation 中的每个
GitHub 仓库选择当前 Host 已登记的 Ait Project，并设置允许召唤 review 的 GitHub login。

界面会生成包含 Channel ID 的 webhook URL，并显示连接状态及最近一次通过签名验证的 webhook。
当客户端通过 socket、SSH 或在线 relay 连接 daemon 时，界面只显示 `<public-host>` 占位符；需
先将 daemon 通过公网 HTTPS 反向代理暴露，再把占位符替换为实际域名。界面只保存凭据引用，
不会读取或显示 private key 与 webhook secret。

也可通过认证 WebSocket 直接配置：

先创建 Bot 和预分配的 `channelId`，再通过认证 WebSocket 调用：

```json
{
  "type": "request",
  "request_id": "github-channel-1",
  "method": "bot.channel.create.request",
  "params": {
    "botId": "<bot-uuid>",
    "channelId": "<channel-uuid>",
    "enabled": true,
    "config": {
      "kind": "github",
      "appId": "123456",
      "privateKeyRef": "env:AIT_SERVER_CREDENTIAL_GITHUB_APP_PRIVATE_KEY",
      "webhookSecretRef": "env:AIT_SERVER_CREDENTIAL_GITHUB_WEBHOOK_SECRET",
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

同一 installation 在一个 daemon 中只能由一个未删除的 Channel 占用。需要连接其他组织或另一个
installation 时，在同一个 Bot 下创建另一个 Channel。

## 触发与结果

允许列表中的用户可以：

- 在 PR 评论 `@ait-review review`，后续文字会作为附加 review mandate；
- 把 `ait-review[bot]` request 为 reviewer；
- 把 PR assign 给 `ait-review[bot]`。

Bot 固定事件中的 base/head SHA，创建临时 worktree，返回 summary 与 inline findings，成功投递
后归档 worktree。同一 `repository + PR + head SHA` 在同一 Channel 只执行一次；push 新提交后
head SHA 改变，可再次触发。无法创建冻结 checkout 或 Provider 执行失败时，Bot 会在 PR 留下安全
的失败说明，不会 merge、approve、request changes 或 push。
