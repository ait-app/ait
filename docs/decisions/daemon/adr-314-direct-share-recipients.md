---
id: ADR-314
status: accepted
date: 2026-11-26
amends:
  - daemon/adr-208-workspace-share-guests.md
paths:
  - bins/daemon/src/host/hub/shares.rs
  - crates/api/src/shares
  - crates/model/src/share.rs
  - packages/client/src/hub
  - packages/protocol/src/messages.ts
  - apps/mobile/src/components/sharing
---

# ADR-314：中心授权直接分享的接收人

## 背景

只面向一名接收人的资源分享无需创建群组。接收人接受邀请时 Host 可能离线，
所以授权需要由在线服务保存。资源范围仍由 Host 决定。

## 决策

- Host 可以为一个资源保留一条 `people` 分享，并同时发布该资源的各条群组分享。
  直接分享独立保存设置 ID，省略群组 ID，且接收人字段为空。中心管理邀请以及接受后的访问权。
- daemon 根据分享 ID、资源范围和权限接纳由中心转发的访客。直接分享的访客没有群组 ID。
  中心报告 `denied` 后，daemon 沿用现有的访客会话终止及资源清理流程。
- 已发布的直接分享在客户端打开中心的分享详情页。网页沿用浏览器会话生成、撤回邀请和管理接收人。
- 旧中心会拒绝含直接条目的整个列表。收到 `invalid_request` 时，daemon 在当前逻辑链路重试群组
  条目，并在本地标记直接分享暂不可用。新逻辑链路重新尝试发布完整列表。

## 后果

直接分享保留群组分享的访客资源权限。中心撤销接收人时，Host 无需更新本地设置，即可按推送结束该访客的会话。
