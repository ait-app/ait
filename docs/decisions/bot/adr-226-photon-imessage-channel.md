# ADR-226：通过 Photon Beta 2.0 接入 iMessage Channel

- 状态：接受；已实现，2026-11-09 用真实 Photon Beta 项目和 iPhone 完成单聊往返验收，遗留项见下文“待真实账号验证”
- 日期：2026-11-09
- 扩展：[ADR-139](adr-139-workspace-bots-and-owned-channels.md) 第 6 条的 Channel 传输；沿用 [ADR-224](adr-224-feishu-scan-onboarding-and-stored-secrets.md) 的 Secret 保存与状态驱动接入。

## 背景

用户希望直接在 iMessage 中使用 Workspace Bot。Apple 没有面向第三方的 iMessage API，Photon（photon.codes）提供托管线路，开发者不需要自备 Mac。

Photon 有两代接口。Stable（Spectrum）明确没有公开的 HTTP 发送接口，只能使用 Node/Bun SDK，这与 ADR-139 第 7 条“不新增 Node/Go 辅助进程”冲突。Beta 2.0 公开了 OpenAPI 契约（`https://api.photon.codes`）和可由任意语言实现的 Socket.IO 事件流。

daemon 通常没有公网地址，因此不能依赖 Webhook，这和 Telegram 长轮询、飞书长连接的约束相同。

本决策依据 2026-11-09 的 Photon 文档、OpenAPI 契约和 Photon 开源的 [`chat-adapter`](https://github.com/photon-hq/chat-adapter)，尚未用真实账号验证。事件信封 `{ id, type, data: { object } }`、事件名 `message.received` 和 `apiVersion` 取值来自该适配器。

## 决策

1. 新增 `ChannelConfig::PhotonImessage`（`kind: "photon_imessage"`），字段为 `projectId`、`keyRef`、`allowedSenders`，界面名称为 iMessage。旧客户端会拒绝 `bot.channel.types.request` 中的未知值，因此此类型不列入其中，客户端按三个配对方法是否存在来显示入口。范围只有 iMessage；Photon 的 SMS、WhatsApp、Telegram 等平台不随本决策接入。
2. Adapter 位于 `bot` crate，只使用 Beta 2.0，用现有的 `reqwest` 和 `tokio-tungstenite` 实现，不引入 Photon 生成的客户端，也不增加 Node 辅助进程。项目 API Key（`pho_sk_...`）通过 `Authorization: Bearer` 发送。
3. 接收使用 daemon 主动建立的 Socket.IO 连接：Engine.IO v4，仅 WebSocket 传输，路径 `/v1/projects/{projectId}/events/ws`，默认命名空间。每次连接发送一次 `subscribe`，携带 `startSeq` 和代码中固定的 `apiVersion`（`2026-07-01`）。adapter 只实现所需的 Socket.IO 子集，因为 `rust_socketio` 自 2024-04 起没有新版本。
4. `delivery` 事件没有处理确认，进度由 daemon 保存。`metadata.seq` 只在 Bot 接纳或明确丢弃该消息后写入 `Channel.cursor`。`startSeq` 取最后处理的序号，没有时取 `0`（只接收新事件）。这里保存的是最后处理的序号，不同于 Telegram 保存的下一个 offset，且序号可能不连续，不能自行推算。现有 `channels::run` 在重连时沿用启动时的 Channel 快照，此 adapter 必须在各次重连之间保留最新序号。
5. 全局积压时不推进游标，断开连接并等待，之后从已保存的序号恢复，避免等待期间阻塞 Socket.IO 心跳。此重连在接收循环内部完成，发送循环继续运行，既不打断正在发送的回复，也能消化积压。致命的 `delivery_error` 或断线交给现有的退避重连。事件至少投递一次，以 Photon 的 `message.id` 作为 `Incoming.message_id` 去重。
6. 只接纳同时满足以下条件的入站消息，其余事件不接纳并推进游标：事件类型为 `message.received`，`platform` 为 `imessage`，`status` 为 `received`，`sentBy.kind` 不是 `agent`，`space.kind` 为 `direct`，`content.type` 为 `text`，`sentBy.handle` 在 `allowedSenders` 中（忽略 ASCII 大小写）。再次发送的配对代码也不接纳。不支持群聊和附件。
7. Conversation 的 `peer` 保存 `<space.id>/<receivedBy.id>`，`sender` 保存 `sentBy.handle`，`thread` 为空。发送接口要求 `senderId`，其值是 Bot 在该对话中的 Photon 用户 ID。分配线路的响应不包含它，文档给出的来源是入站消息的 `receivedBy.id`，共享线路也不保证一个 Channel 只有一个 Bot 身份。放入 `peer` 无需修改持久化结构；缺少 `receivedBy` 的消息无法回复，记录告警后跳过。
8. 发送使用 `POST /v1/projects/{id}/messages`，正文包含 `spaceId`、`senderId` 和纯文本 `content`，不设置 `parentMessageId`。长回复分段发送，`Idempotency-Key` 为 `<turnId>:<分段序号>`。
9. Photon 返回 `202` 即记为 `Delivered`，含义与 Telegram 的发送成功相同：平台已接受，不代表已到达手机。第一版不追踪之后的送达或失败状态。
10. 接入沿用 ADR-224 的状态驱动模型，没有后台任务，daemon 重启后需重新开始：
    - 用户填写 Project ID、项目 API Key 和自己的发送身份（E.164 手机号或 iMessage 邮箱）。Key 需要 `project:read`、`project:write`、`platforms:read`、`platforms:write`。
    - daemon 调用 `POST /v1/projects/{id}/platforms/imessage/assignments` 分配共享线路，并带上由本次配对派生的 `Idempotency-Key`。收到 `409 ASSIGNMENT_EXISTS` 时列出已有分配，在客户端按身份匹配后复用；收到 `409 IMESSAGE_MODE_DEDICATED` 时明确失败，第一版只支持共享线路。
    - daemon 随机生成高熵的一次性代码，展示分配的号码、代码及预填代码的链接和二维码。用户必须从该身份发送代码，因为共享线路不能主动发送首条消息，首条也必须是 iMessage 而不是短信。
    - 状态请求调用 `GET /v1/projects/{id}/messages`，参数为 `direction=received`、`platform=imessage` 和留有时钟余量的 `createdSince`。该接口不能按发送者或正文过滤，因此每次读取最新的 50 条，并要求发送者（忽略 ASCII 大小写）和去除首尾空白后的代码都匹配。匹配后创建已启用的 Channel，允许的发送者只有该身份。代码 10 分钟后过期。
    - 这样可以防止手误把陌生人的号码加入允许列表，并确认该身份的消息能到达 Photon 项目；事件流和发送要到第一轮对话才得到验证。Channel 的事件流从新事件开始，代码消息不会成为第一条输入。
11. API Key 按 ADR-224 第 5 条保存，也可使用 `env:` 引用。账号身份为 `photon:<projectId>`：事件流按项目推送，同一 daemon 内一个项目只能属于一个活动 Channel。

## 未采用的方案

- **Stable SDK 加 Node 辅助进程**：Hermes 采用此方式，但需要随 daemon 分发和管理 Node 运行时。
- **Webhook**：需要公网 HTTPS 地址，并拒绝 localhost 和私有地址。
- **用 `listMessages` 轮询接收消息**：Photon 建议长期运行的 Agent 使用实时事件，这里只用于短暂的配对阶段。

## 待真实账号验证

2026-11-09 的真实验收用模拟模型代替 Provider，走通了分配线路、重复分配时复用（`ASSIGNMENT_EXISTS`）、配对、事件流订阅、入站消息接纳、回复发送和游标保存。Photon 记录该回复为 `delivered`，耗时约 2 秒。实测确认：`receivedBy.id` 可作为 `senderId`，`sentBy.handle` 与分配时的身份一致，`sentBy.kind` 为 `human`，`space.kind` 为 `direct`，`seq` 不从 1 开始（首条消息即为 87019 量级）。

以下内容仍需确认：

- `startSeq` 超出保留范围时的行为，以及事件保留时长。在确认前，游标失效时保持重连状态并显示错误，不自动重置为 `0`，避免静默丢失消息。
- `startSeq` 为 `0` 时订阅确认中返回的 `startSeq` 含义，以及它能否作为首个游标。
- 事件流实际需要的 Key 权限（文档写明 `project:read`）、单条文本长度上限、`limits.maxFrameBytes` 的取值，以及长时间连接下的心跳和重连表现。
- 配对链接能否在 iPhone 上打开 iMessage 并预填代码，以及移动端设置页在真机上的表现。验收时代码是手动输入的，配对也不是从客户端发起的。

iMessage Channel 在验收前保持在“设置 → 关于”的测试功能开关之后。

## 后果

- 消息内容经过 Photon 的云服务，用户需要接受这一第三方。API Key 以明文保存在 daemon 数据目录，边界与 ADR-224 相同。
- Beta 2.0 被 Photon 标记为预览版，接口可能出现破坏性变更；由于 iMessage 没有官方第三方 API，长期可用性取决于 Photon。服务中断时 Channel 只显示重连状态，不影响其他 Channel。
- 回复的最终失败只能在 Photon 控制台看到，Bot 中仍显示已投递。
- daemon 离线超过事件保留时长时，期间的消息可能丢失。配对完成到首次订阅之间，以及保存首个游标之前的断线期间，发来的消息也会丢失。
- 免费版每个项目 25 个共享线路分配，Pro（每月 25 美元）为 100 个，Photon 不按 iMessage 消息数收费。删除 Channel 不释放线路分配，用户需在 Photon 控制台自行释放。
- 旧版 daemon 无法解析未知的 Channel `kind`，加载目录失败会使整个 daemon 无法启动，而不只是 Bot 服务。Channel 删除是软删除，配置仍留在目录中，因此先删除 Channel 也无法回退。回退只能恢复创建此 Channel 之前的 `<data>/workspace-bots` 备份，或放弃其中的 Bot 数据。
- iMessage 离开测试功能开关前，目录加载必须能容忍未知的 Channel 类型；这只保护之后的版本，已发布的版本仍无法回退。未升级的客户端与微信、QQ 一样无法解析包含此类型的 Channel 列表（见 ADR-225）。
- 桌面端的外部链接打开器不允许 `sms:`，桌面端只显示号码、代码和二维码。

## 参考

- [Photon Beta 概览](https://photon.codes/docs/beta/photon.md)
- [Beta WebSocket 会话](https://photon.codes/docs/beta/websocket/session.md)与[游标](https://photon.codes/docs/beta/websocket/cursors.md)
- [Photon API OpenAPI 契约](https://github.com/photon-hq/api)
- [Beta 套餐与计费](https://photon.codes/docs/beta/plans-and-billing.md)
