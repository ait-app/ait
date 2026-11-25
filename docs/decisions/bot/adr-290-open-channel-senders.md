---
id: ADR-290
status: accepted
date: 2026-11-19
supersedes:
  - bot/adr-289-qq-group-mention-admission.md
amends:
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-224-feishu-scan-onboarding-and-stored-secrets.md
  - bot/adr-225-weixin-and-qq-channels.md
  - bot/adr-226-photon-imessage-channel.md
paths:
  - crates/domain/src/bot.rs
  - crates/bot
  - packages/protocol/src/bot
  - apps/mobile/src/bots
---

# ADR-290：所有消息 Channel 对任意发送者开放

后续修订：[ADR-306](adr-306-shared-bot-context-and-chat-reception.md)。

## 背景

发送者白名单会静默丢弃群友的提及和私聊消息。只开放 QQ 群提及仍留下其他平台及普通消息的相同限制。
用户明确要求所有支持平台的群聊和私聊都开放，不再逐人授权。

## 决策

- Telegram、飞书、Lark、微信、QQ 和 Photon iMessage 均不按发送者列表过滤。
  这修订 ADR-139 第 8 条、ADR-224 的扫码者限制、ADR-225 第 1 和 4 条以及 ADR-226 第 6 和 10 条。
- 保留 `allowedSenders` 作为可省略的旧配置兼容字段，缺省为空数组；旧值仍可读写，但不参与配置校验和消息接纳。
  不迁移或清空已保存配置。客户端不再要求用户填写发送者 ID。
- 移除 QQ 的发送者配对码、内存状态、加白操作及其 RPC 与界面。扫码或手动验证平台凭据后即可聊天。
- 保留平台凭据与账号独占。Telegram 的 Start 验证和 Photon 的号码、代码验证用于建立连接，继续保留；
  验证者不会成为运行时的唯一发送者。微信和 iMessage 继续只支持单聊。
- 群聊仍由 `allowGroups` 控制。Telegram 保留提及或回复机器人触发；飞书与 Lark 保留提及触发。
  QQ 接纳任意群成员的提及，也接纳平台开启全量推送后投递的普通文字。关闭群聊仍拒绝全部群消息。
- 平台自身消息可见范围、机器人循环过滤、文本与消息标识校验、去重、预算及按 Channel、位置、发送者和线程隔离的
  Conversation 继续有效。新发送者使用 Bot 已配置的权限，不获得额外工具或角色。

## 后果

升级后现有 Channel 立即使用新接纳规则，无需重新扫码。所有能通过平台发来消息的人都能使用 Bot 已授予的能力，
群内回复对群成员可见，模型调用量可能增加。平台权限仍决定消息能否送达 daemon。
配置格式可兼容读取，回退旧版会恢复其旧白名单限制；新版客户端不再为旧主机提供 QQ 加白入口。

定向测试覆盖未知发送者的私聊与群聊、平台触发与关闭群聊、原群回复、去重、游标恢复、会话隔离、旧配置重载和无发送者表单。
