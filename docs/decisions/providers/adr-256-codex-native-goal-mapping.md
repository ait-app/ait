---
id: ADR-256
status: proposed
date: 2026-11-11
amends:
  - providers/adr-177-codex-restart-continuation.md
paths:
  - crates/provider
  - crates/domain
---

# ADR-256：Codex 原生 Goal 的映射与延续所有权

## 背景

ADR-177 假设 Codex 的继续执行始终由宿主仲裁：重启后向持久 FIFO 放入一条身份稳定的 Continue
指令，原生程序本身不推进目标。codex-cli 0.167.0 起，`thread/goal/set` 让原生运行时自己驱动
后续轮次（实测：`set{active}` 后约 0.1 秒自行 `turn/started`），宿主的 Continue 文本会与原生
自己启动的轮次竞争同一会话写者。

统一 Goal 生命周期由 ADR-245（任务 16，本分支未包含该文件）定义：宿主记录 `GoalRun`，
策略每运行只选一次，回退延续走既有持久 FIFO。本决策只记录 Codex 适配器一侧的映射与边界；
它不新增 wire 方法、协议类型或能力字符串。

## 决策

1. **能力来自已接纳会话，绑定精确运行时。** `AgentSession::goal_capability()` 由会话在
   `initialize` 后读到的原生身份（`codex_cli_rs/x.y.z`）计算，不使用 client/global 探测：
   版本、cwd、有效配置与私有延续设置都不得跨会话泄漏。只有**实测**均匀暂停语义的版本
   （当前 `0.167.0`）返回原生策略；预发布标记与环境版本不继承该证明。其余版本返回
   `FallbackOnly` 并在 `reason` 中记录未证明的事实，运行从开始即用宿主回退。

2. **均匀暂停是原生策略的前提。** 原生暂停必须让在飞轮次正常结束且不再启动下一轮。
   若某运行时在暂停时中止在飞轮次，它只能映射为 `AbortInFlight`，运行固定为回退，
   绝不把中止重标成暂停。停止语义是 `turn/interrupt` 之后显式 `set{status:paused}`：
   实测仅中断会让目标保持 `active`，重启后原生自行恢复追求。

3. **代际标识只用原生修订。** `ThreadGoal` 没有 goal id，且 `createdAt` 在目标被替换后
   不变（A→B→A 会与自身碰撞），因此代际比较用 `updatedAt` 加观察到的 `objective`，
   `createdAt` 只作为观察字段。较旧修订不得覆盖较新状态。目标内容变化是原生事实，
   适配器只投影，不做宿主侧编辑。原生 `thread/goal/set` 没有修订前置条件，因此陈旧目标
   拒绝留在宿主侧（请求携带的 `expectedVersion`）；适配器负责权威读，并在清除前先确认
   目标仍然存在。

4. **错误按精确消息分类。** 实测 goal 错误是 `-32600` 加固定文本，从不是 `-32601`：
   `goals feature is disabled` 表示能力缺失；`no goal exists`、`ephemeral thread does not
   support goals`、`invalid thread id` 是各自独立的事实，不得合并为一个拒绝。传输失败或
   超时是对账中最不确定的一类，绝不当作“目标已清除”。

5. **只读探测是创建前的前提。** `goal_read()` 走一次 `thread/goal/get`，`Ok(None)` 才证明
   缺席；不可用或失败不是缺席证据。`thread/read` 与恢复响应都不携带 goal，因此重启对账
   的唯一权威来源就是这个只读调用，且在 `thread/resume` 之前即可生效。

6. **写入带来源且不新增提示。** 用户发起的创建/暂停/恢复/清除发送 `origin:"user"`，
   宿主仲裁的重发使用 `origin:"automatic"`。暂停、恢复、清除都不是提示操作，适配器不发送
   任何 `turn/start`。`thread/goal/clear` 返回 `{cleared:false}` 表示目标已在前读与清除之间
   消失，适配器按拒绝上报，不把陈旧快照标为已清除。

7. **延续决策仍只有一处。** 原生策略下宿主不发送任何 Continue；重启后的唯一延续决策留在
   ADR-177 的 `recovery.rs` 排队与 `delivery.rs` 派发路径。适配器只提供事实与执行写入：
   已对账的原生运行收到一次 `set{status:active, origin:"automatic"}` 重发，回执
   `Uncertain` 从不重发。`CodexSession` 内部没有计时器、没有第二套仲裁。

8. **计量只展示原生事实。** `tokensUsed`/`timeUsedSeconds` 始终存在、`tokenBudget` 默认空，
   投影为替换而非累加，未知保持未知。实测 `budgetLimited` 在 `tokensUsed=26 > tokenBudget=13`
   时出现，说明原生预算不是硬上限；宿主不设默认预算、不估算、不声称上限。原生计数器只在
   存在原生运行时可靠，回退运行不暴露它们。

9. **回退路径保持 ADR-177 原样。** 无 `GoalRun` 的旧记录、非原生会话与未证明运行时都不触发
   任何 goal RPC；`/goal` 文本命令仍是终端便利入口，身份与回执不变。

## 后果

- ADR-177 的继续仲裁、用户输入优先与 `Uncertain` 不重发继续生效；本决策扩展它的适用前提，
  不改变它对无原生目标的会话的保证。
- 新增的绑定关系是“原生延续所有权绑定到实测运行时”：升级或降级二进制不改变已记录策略，
  能力丢失按类型化失败处理，用户可重新创建运行，不做静默切换。
- 需要每版本实测才能扩大原生策略的适用范围；0.128–0.166 在此之前保持回退。
