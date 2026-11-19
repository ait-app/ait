---
id: ADR-291
status: accepted
date: 2026-11-19
amends:
  - daemon/adr-120-single-connection-startup-admission.md
paths:
  - crates/api/src/connection/single.rs
  - docs/architecture/connection.md
---

# ADR-291：单连接的活动心跳允许过载丢弃

## 背景

切换工作区或会话时，客户端会报告新的焦点。归档也会改变焦点，并触发
`session.heartbeat`。该事件携带完整的客户端活动快照，客户端会定期重新发送。
当元数据 worker 队列已满时，按普通事件处理活动心跳会关闭整条业务连接，
尚未收到响应的归档请求也会随之失败。

## 决策

单连接 worker 队列满时允许丢弃本条 `session.heartbeat`，继续处理连接上的后续消息。
worker 队列可用时正常处理活动快照。客户端的下一次成功心跳更新焦点、可见性和活动时间。

## 后果

过载期间客户端活动信息可能暂时滞后。其他事件、响应和二进制消息继续遵守原有准入规则。
已接纳的 RPC 继续执行，队列满时被拒绝的 RPC 仍收到可重试的 `resource_exhausted`。
连接存活探测 `connection.ping` 继续由路由层直接处理。
