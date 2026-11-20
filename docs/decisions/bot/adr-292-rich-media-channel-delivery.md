---
id: ADR-292
status: accepted
date: 2026-11-20
supersedes: []
amends:
  - bot/adr-227-bot-multi-message-output.md
paths:
  - bins/daemon
  - crates/bot
  - docs/operations/bots.md
---

# ADR-292：按平台能力投递 Bot 图片回复

## 背景

Provider 已经通过 Markdown 图片产生可显示的图片路径或 URL。Bot 的外部 Channel 只把回复当作文本发送，导致支持原生图片接口的平台仍收到路径文字。

## 决策

- 统一解析 Provider 回复中的 Markdown 图片语法，保留周围文本，并把图片交给 Channel 的原生发送接口。
- Telegram 使用 `sendPhoto`，飞书与 Lark 先上传图片取得 `image_key`，QQ 先上传文件取得 `file_info` 再发送 `msg_type: 7` 富媒体消息。
- 图片来源接受 HTTPS URL、data URL 和 daemon 数据目录下的 Provider 图片文件。文件路径限制在 `agents/provider-images` 目录内，单张图片上限为 10 MiB。
- 入站图片消息仍按现有文本入站模型处理。微信、iMessage 和 Webhook 没有稳定的当前附件发送契约，继续保持文本投递。

## 后果

Telegram、飞书、Lark 和 QQ 能原生显示 Provider 生成的图片，文本与图片按回复顺序投递。图片上传失败会让该轮投递失败并保留现有投递状态语义。Channel 仍不承诺多附件批量发送或卡片流式回复。
