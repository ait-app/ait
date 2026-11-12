# GitHub Bot PR Review

一个 Workspace Bot 可以创建多个 GitHub Channel；一个 Channel 对应一个 GitHub App
installation，并可绑定该 installation 中的多个仓库。每个仓库必须映射到 daemon 已登记的
Project。

## GitHub App

创建 GitHub App，并配置：

- Repository permissions：Metadata read、Contents read、Pull requests read/write、Issues
  read/write。
- Subscribe to events：Issue comment、Issues、Pull request。
- Webhook URL：创建 GitHub Channel 后由 Ait 界面返回的 center URL。
- Webhook secret：与 URL 同时一次性显示；复制到 GitHub App 后妥善保存。

`ait-server` 原样验证请求体和 `X-Hub-Signature-256`，限制请求体为 700 KiB，并持久化 delivery；
Host 在线后通过受限 service session 接收事件并再次校验签名。无需把 daemon 暴露到公网。

安装 App 后，从 installation 页面或 webhook payload 取得 numeric installation ID。App ID、
installation ID 和 App slug 不是秘密；private key 和 webhook secret 是秘密。

## daemon 凭据

启动 daemon 前注入：

```sh
export AIT_SERVER_CREDENTIAL_GITHUB_APP_PRIVATE_KEY='-----BEGIN RSA PRIVATE KEY-----
...
-----END RSA PRIVATE KEY-----'
```

也支持 GitHub 下载的 PKCS#8 `PRIVATE KEY`。Bot 记录只保存 private key 的 `env:` 引用，不保存
私钥或短期 installation token。Webhook secret 由 center 签发，并保存于 server 与 Host 的私有
credential store。

## Channel 配置

在 Bot 的 `Settings → Channels` 中点击 `Add GitHub Channel`，填写 App ID、installation ID、
Bot mention 和 private key 的 `env:AIT_SERVER_CREDENTIAL_*` 凭据引用。然后为 installation 中的
每个 GitHub 仓库选择当前 Host 已登记的 Ait Project，并设置允许召唤 review 的 GitHub login。

首次保存后，界面显示 center 签发的真实 webhook URL 和仅显示一次的 secret。复制二者并配置到
GitHub App；关闭结果页后不会再次返回 secret。Channel 状态会显示最近一次通过签名验证的
webhook。界面不会读取或显示 private key。

也可通过认证 WebSocket 直接配置：

先创建 Bot 和预分配的 `channelId`，再通过认证 WebSocket 调用：

```json
{
  "type": "request",
  "request_id": "github-channel-1",
  "method": "bot.channel.github.create.request",
  "params": {
    "botId": "<bot-uuid>",
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
- 把 `ait-review[bot]` request 为 reviewer；
- 把 PR assign 给 `ait-review[bot]`。

Bot 固定事件中的 base/head SHA，创建临时 worktree，返回 summary 与 inline findings，成功投递
后归档 worktree。同一 `repository + PR + head SHA` 在同一 Channel 只执行一次；push 新提交后
head SHA 改变，可再次触发。无法创建冻结 checkout 或 Provider 执行失败时，Bot 会在 PR 留下安全
的失败说明，不会 merge、approve、request changes 或 push。
