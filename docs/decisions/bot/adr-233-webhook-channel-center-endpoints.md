---
id: ADR-233
status: accepted
date: 2026-11-10
amends:
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-213-single-workspace-manager-bot.md
  - daemon/adr-045-remove-hub-chat-loop.md
  - clients/adr-137-remove-retired-feature-protocol.md
paths:
  - crates/bot
  - bins/daemon
  - crates/api/src/automation
  - crates/domain/src/bot.rs
---

# ADR-233：中心托管的 Workspace Bot Webhook Channel

## 背景

外部脚本和服务需要通过 HTTPS 地址向某个 Workspace Bot 发送文本，并通过轮询或回调取得结果。中心有公网入口，daemon 使用现有 serve 链路连接中心；执行、模型和权限继续归 Workspace Bot 管理。

## 决策

1. 只新增 `ChannelConfig::Webhook`（`{kind:"webhook",endpointId,url}`）。每个 Bot 可有多个端点；daemon 不运行传输。用户在 Bot 设置页添加 Webhook，一次获取 URL、密钥和 curl 示例，支持复制、停用和重新生成密钥，覆盖 9 种语言。
2. 创建只走 `bot.channel.webhook.create.request`：daemon 先检查 Bot 和 Channel 上限，再经 Host serve 链路调用 `hub.automation.webhook.create`，保存已启用的 Channel 后返回 `{state:"created",channel,secret}`。保存失败时尽力删除中心端点。普通 `bot.channel.create` 不接受 Webhook 配置；更新只允许切换启用状态。
3. `bot.channel.webhook.rotate.request` 重新调用中心创建方法，保留端点并轮换密钥。密钥仅返回一次，daemon 不保存；中心以 AES-256-GCM 加密保存，用于验证 Bearer 和签名响应回调。端点 URL 要求 HTTPS，本地回环可用 HTTP。
4. 中心不可用时创建和轮换返回 `{state:"failed",reason}`：`centerOffline`、`centerUnsupported`、`automationUnavailable`、`limitReached` 或 `centerError`。删除 Channel 或移除 Bot 时尽力调用 `hub.automation.webhook.delete`；离线残留端点不能访问未发布的 Channel。
5. `hub.automation.publish` v2 每个已启用的 Webhook Channel 发布一项 `{workspaceId,channelId,title,state,kind:"webhook"}`，最多 64 项。仅至少一个处于 active 时允许中心的 `service:{kind:"automation"}` 会话。最后一个可用 Channel 关闭或 Bot 暂停、退役时，立即结束服务会话。
6. 自动化会话默认拒绝，只放行 `bot.webhook.message.send`、`bot.turn.get`、`bot.turn.cancel`、`bot.webhook.delivery.ack` 和 `bot.webhook.conversation.close`，所有目标必须属于当前 Bot 的已启用 Webhook Channel。主人也可调用这三个 Webhook 方法，访客不能调用。
7. 中心输入携带 `workspaceId,channelId,peer,sender,thread,messageId,text`，Bot 按现有会话规则串行执行，并按 messageId 去重。轮次完成后投递状态保持 pending，中心按顺序合并 `turn.messages` 的非空文本，兼容旧 daemon 的 `reply`。poll 在调用方首次读到结束结果时回报 delivered；push 以签名回调的 2xx 为准，失败回报 uncertain。
8. 中心会话关闭时通过 `bot.webhook.conversation.close` 释放 Bot 对话及回执，取消排队输入并关闭隐藏 Provider session；执行中的对话返回 BotConflict。重复关闭和送达确认无副作用。
9. `bot.channel.types` 保持旧客户端可识别的列表；客户端根据创建和轮换方法是否可用显示入口（`features.workspaceBotWebhookSetup`），沿用 ADR-225/226 的方法能力检查。

## 后果

- 本 PR 的新入口只有直接 Webhook；没有独立 Hub Channel、聊天平台路由、Trigger 或中心管理页面。Hub 是现有传输，未恢复上游 Paseo 的通用管理或执行接口。
- 外部调用方的文本仍受 Workspace Bot 当前权限约束，中心会话不能访问其他 Channel、主人对话、文件、终端或 Agent 管理。
- 中心负责端点、加密密钥、幂等输入、限流、poll/push 和历史记录，协议见 ait-server `docs/automation.md`。
- 不带 thread 的调用独立，有 thread 的调用由中心复用对话。当前只提供最终文本结果，不提供进度流和聊天内审批。

## 验证

定向测试覆盖 Webhook 创建、轮换、失败原因、发布、停用与删除；服务会话的准入、五个方法的权限边界、输入幂等、轮次读取、送达确认和会话释放；客户端设置流程及未知 Hub 配置的拒绝。最终检查结果见 PR #633。

## Test coverage

未测量：本次未要求覆盖率统计。使用受影响模块和 crate 的测试验证行为，不扩大到全工作区测试。
