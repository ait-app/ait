---
id: ADR-210
status: accepted
date: 2026-11-03
amends:
  - daemon/adr-106-concrete-file-persistence.md
  - workspace/adr-081-directory-change-push.md
paths:
  - crates/provider
---

# ADR-210：Agent 目录共享已提交的不可变记录

- 补充：[ADR-106](../daemon/adr-106-concrete-file-persistence.md) 的存储端口和
  [ADR-081](../workspace/adr-081-directory-change-push.md) 的变更通知；依赖方向不变。

## 背景

性能交接报告中的 registry 探针与线程栈显示，目录查询和 Workspace activity 会重复深克隆
全部 Agent runtime record。分页发生在匹配之后，小 limit 无法消除这些开销。多个读取方
共享同一 registry mutex，锁内字符串和 JSON 复制还会阻塞其他 Agent。

## 决策

1. `domain::agent_runtime::registry::AgentRuntimeRegistry` 提供 `snapshot()`，返回按插入顺序
   排列的 `Arc<[Arc<PersistedAgentRuntimeRecord>]>`。这是一次已提交状态的不可变读视图，
   不代表 Agent、Workspace、Project 三个存储的联合事务，也不替代目录同步的 generation/seq。
   `get_shared(id)` 提供同样语义的单记录读取。默认实现经 `list()` / `get()` 创建视图，
   兼容现有其他适配器和测试替身。
2. persistence 的通用 file registry 以共享记录保存已提交状态，按需构造并缓存共享数组。
   同一提交状态的 `snapshot()` 只增加引用计数；`get()` 和 `list()` 仍返回独立拥有的数据，
   但在释放 registry 锁后复制记录。成功完成原子写入和所有 commit hook 后，才替换记录并
   失效共享数组。失败、冻结和 no-op 保持旧读视图；旧视图在下一次提交后仍有效。
3. Provider 目录匹配、分页和订阅准备持有共享记录。前台身份解析只复制选中的完整记录。
   Workspace activity 和 attention scan 使用相同的共享读端口；attention 写入仍在 registry
   update 锁内核对原记录，防止消费较新的权限、完成或 placement 状态。原生 lane 的
   reconcile 使用 `get_shared()` 检查删除与归档，不复制完整运行记录。Host Hub 的 Agent
   摘要与 attention 读取复用同一端口，只复制发布字段，不读取完整 provider 配置。
4. Owner observation 的目录 overlay 只复制七个控制字段，包括完整 pending permissions。
   全量完成快照仍供 wait 观察者使用；overlay 复制在释放全局 observation map 锁后执行。
   发布保留与 pending/admitting 相同的串行化边界，未变化的发布只比较一次完整快照，
   不提前比较 final 字段。
5. 不以此读视图缓存依赖其他 registry 的完整 JSON 投影。Workspace/Project placement、权限、
   internal 过滤、remove/upsert、keyset 分页和断线补齐仍通过现有逻辑计算；原生轮询节奏不变。

## 后果与验证

同一已提交状态下的生产 Agent 目录读取不再深克隆 N 条 runtime record，非 sync 订阅的
中间集合复制也只复制记录引用。变更仍需现有的序列化、验证、原子提交与通知；本次未优化
写入时的完整 staged map，也不把减少 clone 解释为消除全部扫描、排序和 JSON 投影。
每条已提交记录增加一次 Arc 分配和引用计数；当前写入仍替换全部记录，因此收益主要来自
多个消费者反复读取同一状态。旧视图的记录在最后一个读取方释放后回收。

定向回归覆盖共享数组复用、旧视图生命周期、失败写入、no-op、重新打开文件、目录过滤与
分页、权限字段、attention 并发语义，以及真实 clone 次数。具体执行结果和限制见
[性能修复验证报告](../../archive/reports/providers/performance-handoff-fixes-2026-11-03.md)。
