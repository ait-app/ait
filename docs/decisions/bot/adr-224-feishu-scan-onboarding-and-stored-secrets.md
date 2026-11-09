# ADR-224：飞书扫码接入与 daemon 保存的 Channel 凭据

- 状态：接受
- 日期：2026-11-09
- 修订：[ADR-139](adr-139-workspace-bots-and-owned-channels.md) 第 8 条“凭据只保存环境变量引用”。

## 背景

外部 Channel 要求用户预先在启动 daemon 的环境中注入凭据，并手动填写 `env:` 引用和发送者 ID，飞书接入尤其繁琐。
飞书提供基于 OAuth 2.0 设备授权的一键创建应用：扫码后直接返回 App ID、App Secret 和扫码者的 `open_id`，不需要中心服务。
2026-11-09 用个人账号和企业账号实测：扫码创建的应用无需进入开发者后台配置，即可通过长连接收发单聊和群聊 @ 消息，扫码返回的 `open_id` 与消息发送者一致。

## 决策

1. 扫码流程由 daemon 直接访问 `accounts.feishu.cn` 完成，客户端只展示二维码和状态，不接触 App Secret；不增加中心服务。
2. 飞书只为 SDK 记录此流程，daemon 使用与官方 SDK 相同的 HTTP 交换。二维码链接追加 `addons`，始终申请收发消息所需权限和 `im.message.receive_v1`，不依赖默认模板内容。
3. 进度由客户端的状态请求驱动，daemon 对飞书的轮询不超过其要求的间隔；每个 Workspace 只保留一个内存中的扫码，没有后台任务。已同意的扫码在暂时性保存失败后由下一次状态请求重试，请求在文件操作通道执行，不阻塞普通 Workspace 请求。daemon 重启后需重新扫码。
4. 扫码成功后创建已启用的 Channel，允许的发送者只有扫码者；群聊由用户在扫码前选择，默认关闭。
5. Channel 可使用 `env:AIT_SERVER_CREDENTIAL_STORED_<channelId 十六进制>` 引用 daemon 保存的 Secret，文件位于 `<data>/workspace-bots/credentials/<channelId>`，Unix 上目录 `0700`、文件 `0600`。此引用只能指向所在 Channel 自身，客户端不能借用其他 Channel 的 Secret。它有意符合环境变量引用的格式：回退到旧版 daemon 时 Channel 只会显示不可用，不会因目录校验失败而无法启动。其他 `env:` 引用继续有效。
6. 手动接入作为回退：客户端发送 App ID 和 App Secret，daemon 先向飞书验证再保存。Channel 删除或 Bot 退役后删除 Secret，每次启动重试已删除 Channel 的清理。
7. 只支持中国区飞书；扫码账号属于 Lark 时明确失败，因为 Channel 传输只连接 `*.feishu.cn`。

## 后果

- App Secret 以明文保存在 daemon 数据目录。文件权限防止其他系统用户读取，但不防止同一用户运行的进程；能执行命令的 Agent 可以读取它，这与 [ADR-215](adr-215-bot-permissions-and-scoped-mcp.md) 已接受的 `local-api.token` 风险相同。
- 依赖未公开的 HTTP 交换，飞书修改后扫码会失败，手动接入不受影响。
- 删除 Channel 不会删除飞书上已创建的应用，用户需在飞书开发者后台自行删除。
- Telegram 的一键创建需要受信任的托管管理 Bot，而 `aitapp.cn` 能否访问 Telegram 尚未确认，不在本决策范围内。
