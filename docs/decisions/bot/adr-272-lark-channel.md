---
id: ADR-272
status: accepted
date: 2026-11-11
amends:
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-224-feishu-scan-onboarding-and-stored-secrets.md
paths:
  - crates/domain/src/bot.rs
  - crates/bot
  - packages/protocol
  - packages/client
  - apps/mobile/src/bots
---

# ADR-272：国际版 Lark Channel 与区域隔离

## 背景

现有飞书 Channel 固定使用中国区接口，无法接入国际版 Lark。两者使用相同的消息与长连接帧协议，
但凭据验证、API 地址和连接端点必须保持区域一致。本决策扩展 ADR-139 的外部 Channel，
修订 ADR-224 第 7 条仅支持中国区飞书的限制；飞书扫码流程仍限中国区。

## 决策

1. 新增独立的 `ChannelConfig::Lark`，序列化为 `kind: "lark"`，字段与飞书相同：
   `appId`、`secretRef`、`allowedSenders`、`allowGroups`。既有 `feishu` 配置和默认行为不变。
   Channel 类型与 App ID 创建后不能更换；同一 daemon 内账号独占按 `(kind, appId)` 判断，
   飞书和 Lark 的同名 App ID 不冲突。
2. 复用 `bot::channels::feishu` 的收发、持久接纳、去重、线程回复和重连流程。
   飞书固定使用 `https://open.feishu.cn`，Lark 固定使用 `https://open.larksuite.com`。
   服务发现返回的 WebSocket 必须使用 `wss`，且只接受所属区域的 `.feishu.cn` 或
   `.larksuite.com` 子域；拒绝其他区域、用户信息和片段。不提供任意 API 地址配置。
3. 新增可选方法 `bot.channel.lark.connect.request`：验证手动输入的凭据后，沿用 ADR-224
   的凭据存储和清理规则创建已启用的 Channel。界面同时支持原有 `env:` 引用方式。
   Lark 首版采用手动创建自建应用，需要在 Lark 后台启用长连接和消息权限；不复用飞书二维码。
4. 继续保留 `bot.channel.types` 的旧枚举，避免旧客户端无法解析。客户端根据新方法推导
   `workspaceBotLarkSetup` 并显示 Lark 入口，新方法不加入核心 `BOT_METHODS` 要求。
   访客不可接入或修改 Channel。

## 后果

- 新的 Channel 类型进入持久化目录和列表响应后，旧版 daemon 和客户端不能解析；软删除仍保留类型，
  不能作为降级迁移手段。此限制与微信、QQ、iMessage 相同。
- 当前支持允许列表内用户的文本消息；群聊需开启并 @ 机器人，不增加富媒体或流式卡片。
- 验证使用本地 HTTP/WebSocket 模拟服务，真实 Lark 账号、应用权限及区域长连接可用性仍需验收。

接口选择参考官方 [API 区域定义](https://github.com/larksuite/oapi-sdk-go/blob/v3_main/client.go)
和 [WebSocket 的域名配置](https://github.com/larksuite/oapi-sdk-go/blob/v3_main/ws/client.go)。
