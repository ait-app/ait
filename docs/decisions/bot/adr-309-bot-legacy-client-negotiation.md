---
id: ADR-309
status: accepted
date: 2026-11-26
amends:
  - bot/adr-306-shared-bot-context-and-chat-reception.md
paths:
  - crates/bot
  - packages/protocol/src/bot
  - packages/client/src/protocol
---

# ADR-309：保留旧客户端的 Bot 能力协商

## 背景

v0.2.27 客户端把单对话重置包含在整个 Bot 页的必需方法集合中。ADR-306 撤下该方法后，
即使 Host 仍支持 Bot 对话和管理，旧客户端也会隐藏整个页面并提示更新 Host。
客户端与远程 Host 可独立升级，不能要求所有设备同时更新才能继续使用已有 Bot。

## 决策

Host 继续发布并协商旧单对话重置方法，保留已发布客户端要求的核心 Bot 方法集合。
此兼容入口校验工作区与对话归属后返回协议版本不兼容错误，不修改上下文、队列或历史。
旧客户端的单对话确认不能授权重置所有入口共享的上下文；实际重置仍通过 Bot 级接口完成。

后续新增 Bot 能力独立协商，不能通过撤下核心方法使无关的对话和管理能力一起消失。

## 后果

旧客户端可以继续打开 Bot 页、对话和管理 Channel。重置共享上下文与会话接收配置需要更新客户端。
兼容入口保留能力名称，不恢复旧的隔离上下文，也不扩大旧客户端重置请求的作用范围。
