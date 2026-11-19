---
id: ADR-289
status: accepted
date: 2026-11-19
amends:
  - bot/adr-139-workspace-bots-and-owned-channels.md
  - bot/adr-225-weixin-and-qq-channels.md
paths:
  - crates/bot
  - apps/mobile/src/bots/bot-channels.tsx
---

# ADR-289：QQ 群聊接纳所有成员对机器人的提及

QQ 原来对私聊和群聊共同使用发送者白名单，因此开启群聊后，未配对的群友即使 @ 机器人也被忽略。
用户要求机器人接收所有群友的 @ 消息。开启 `allowGroups` 现在表示允许 QQ 群中的任意成员通过提及机器人发起对话；
不再要求每个群成员先配对。这修订 ADR-139 第 8 条与 ADR-225 第 4 条对 QQ 群提及消息的允许列表要求。

## 决策

- `GROUP_AT_MESSAGE_CREATE` 是 QQ 确认的机器人提及事件，开启群聊后接纳任意发送者。
- 群主开启 QQ 全量消息推送后，提及消息也可能由 `GROUP_MESSAGE_CREATE` 投递；仅当
  `mentions` 中有布尔值 `is_you: true` 时，同样免去发送者白名单检查。判定依据见
  [QQ 官方群消息文档](https://bot.q.qq.com/wiki/develop/api-v2/autogen/event/group_message_create.html)。
  不从任意 `<@...>` 或 `@名字` 文本推断机器人身份，避免把提及其他人、其他机器人或普通文本扩大为提及当前机器人。
- 私聊继续使用 `allowedSenders`。全量推送中未提及当前机器人的普通消息仍按原白名单接纳；不开放给未配对群友。
  `allowGroups: false` 继续拒绝全部群消息。
- 有效配对码优先于模型执行，私聊、群提及与全量群消息中的配对继续可用。普通群提及不自动把发送者加入白名单。
- 群成员接纳后使用 Bot 已配置的权限，沿用按 Channel、群、发送者和线程隔离的 Conversation；回复发回原群。
  不增加新的凭据、工具或角色权限。平台消息仍作为不可信输入处理。

## 后果

开启群聊后，未配对成员也能通过 @ 使用 Bot 已授予的能力，模型调用量可能增加；群内回复对群成员可见。
没有群 ID 白名单的新设置，范围由 QQ 中机器人已加入的群及现有 `allowGroups` 控制。
没有协议或存储格式迁移；已有配置升级后即采用此接纳规则，回退恢复旧过滤行为。

回归测试覆盖两种群事件的未配对成员收发、消息去重、原群回复、发送者归属、私聊与普通群消息过滤、关闭群聊及配对优先级。
