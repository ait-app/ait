---
id: ADR-315
status: accepted
date: 2026-11-26
amends:
  - daemon/adr-314-direct-share-recipients.md
paths:
  - apps/mobile/src/components/sharing
  - apps/mobile/src/runtime/account-state.ts
  - apps/desktop/src/daemon/account-ipc.ts
  - packages/client/src/account-session.ts
---

# ADR-315：AIT 通过当前账号签发分享邀请

## 背景

分享设置由 Host 发布，邀请由中心签发。在 AIT 中填完接收人即可复制链接，网页继续提供邀请和接收人管理。

## 决策

- AIT 取得中心确认的分享 ID 后，使用当前账号会话调用中心的分享邀请接口。桌面主进程负责请求，渲染进程只获得生成的链接。
- 账号会话以自己的中心地址为准。请求期间如果账号变化，旧会话的结果失效。链接把一次性 token 放在 URL 片段里，接收方登录网页后接受。
