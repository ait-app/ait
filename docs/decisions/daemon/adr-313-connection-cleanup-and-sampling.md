---
id: ADR-313
status: accepted
date: 2026-11-26
amends:
  - clients/adr-074-account-host-relay.md
  - daemon/adr-247-host-resource-monitor.md
paths:
  - crates/api/src/connection
  - crates/model/src/outbound.rs
  - crates/metadata/src/daemon
  - packages/client
  - apps/mobile/src/runtime
  - apps/mobile/src/components/permission-request-card.tsx
  - apps/mobile/src/components/timeline-sync-status.tsx
  - apps/mobile/src/agent-stream
  - apps/mobile/src/task-board
  - apps/mobile/src/panels/agent-panel.tsx
  - apps/desktop/src/daemon/account-ipc.ts
  - docs/architecture/connection.md
---

# ADR-313：连接清理独立准入、采样隔离与有界恢复

## 背景

普通业务队列拥堵时，订阅释放也会被拒绝。客户端把释放失败立即升级为主动断链，
恢复订阅又形成下一轮请求突发。同步主机采样同时占用短任务额度，并使后续监控读取
等待采样锁。审批的界面等待与底层请求期限不一致，超时后的再次提交还可能重复执行。

## 决策

修订 ADR-074 的单连接调度生命周期：四个 worker 分别持有独立的有界清理队列，
清理在当前操作结束后优先于下一项业务。全连接最多接纳 32 个未完成释放操作，
全部 worker 实际释放后才确认；业务队列、出站额度和心跳路径保持原预算。

客户端以连接代次和订阅 ID 拥有清理任务。资源拒绝使用专用抖动退避，
从 250 毫秒增长至最多 5 秒，停止通用准入重试叠加；总期限为 60 秒。
期限内的拥堵不主动断链，到期只触发一次受控重连。传输关闭即终结旧代清理，
旧响应及定时器不得影响新连接。真实传输失败继续使用现有恢复路径。

修订 ADR-247 的采样执行归属：主机监控独占一个受追踪的阻塞采样任务，
不使用共享短任务额度。缓存发布与采样分别加锁；读取只返回最近完成的快照，
保留原始采样时间。首次没有结果时明确不可用，不等待采样，也不制造时间戳。
采样仍由读取触发，采样间隔与历史预算不变；客户端每主机合并在途监控读取。

订阅恢复归 SDK，每主机最多四个并发，连接事件和会话时间线先于目录，
监控等待业务订阅恢复。释放、断链及切换需求取消尚未开始的恢复；旧代快照不可投递。
现有内容和草稿继续由客户端存储持有，恢复期间标为待更新。

审批使用共享的 60 秒确认预算；结构化问题的独立期限保留。发送与审批不自动重发。
未知结果由 SDK 持有，跨组件重挂载和重连禁止再次提交，提供只读核对入口。
对应的权限解决事件、带解决结果的 RPC 或匹配请求 ID 的计划解决时间线才是确认依据；
权限从快照消失不构成成功证据。明确拒绝允许用户重试。
计划实施继续使用原有稳定输入 ID 和服务端持久去重。

一次恢复只展示一条延迟两秒的中性连接提示，全局提示覆盖会话内连带同步错误，
恢复结束自动消失。Provider 的明确执行失败仍归对应会话。
诊断记录连接代次、来源、原因、恢复与清理耗时，并分别计量请求排队、执行、响应。
重复恢复失败汇总计数；脱敏、轮转和证据保留上限不变。

## 后果

业务 RPC 和快照结构保持兼容。旧 daemon 从客户端清理退避受益，新 daemon
额外提供独立清理容量；发布时先更新 daemon，再更新客户端。
非计划审批缺少可重放解决证据时允许一直待确认，不能用乐观推断解锁重发。
原生 Codex 操作超时和磁盘 I/O 故障继续独立诊断，连接修复不替代其根因处理。
