---
id: ADR-248
status: proposed
date: 2026-11-11
amends:
  - providers/adr-205-provider-execution-contracts.md
  - providers/adr-209-observed-subagent-normalization.md
paths:
  - crates/provider
  - packages/protocol
  - packages/client
  - apps/mobile
---

# ADR-248：父 Agent 拥有的类型化子 Agent 引导与停止

- 修订：[ADR-209](adr-209-observed-subagent-normalization.md) 的“observed 一律拒绝写入”边界，
  在父级作用域内开一个窄口；沿用 [ADR-205](adr-205-provider-execution-contracts.md) 的封闭
  `Operation` 与访问/目标分类。
- 状态为 proposed：本决策随 Draft PR 提交评审，不代表架构验收；既有 accepted ADR 不被改写。

## 背景

ADR-209 把经 adapter 验证的原生后代登记为只读 `observed` Agent，并在原生副作用之前拒绝其一切
写入（send、cancel、审批、配置、rewind、refresh、detach、resume/import、finish-wait）。这对展示
是正确的，但用户在统一的子 Agent 树里看到运行中的原生子任务时，没有任何受支持的干预路径：
既不能给子 Agent 追加一条引导，也不能只停止这一个子任务。唯一可用的“停止”会打到父 Agent 的整轮。

历史上（PR612 donor）的解法是一个通用网关 `agent.native_action.request {agentId, action, arguments}`：
客户端拼 native action 字符串（`steer_subagent`/`cancel_subagent`），服务端把 `Value` 原样转给
adapter，父级与子级校验全部落在 adapter 的原生成员检查上；父 Agent 不在本进程写入时还会被自动
resume。这条路径把 native 命令名、参数形状与错误字符串泄露给通用 UI，也绕开了 Ait 自己的
observed 父链校验，并可能为控制请求创建第二个 writer。

## 决策

1. 新增两个**类型化** wire 方法，作为既有 `agent.subagents.list.request` 的兄弟，不引入通用
   native action 网关：
   - `agent.subagents.guide.request {parentAgentId, childAgentId, message, expectedRevision?}`
     → `{parentAgentId, childAgentId, disposition: "accepted"|"not_running", error}`
   - `agent.subagents.stop.request {parentAgentId, childAgentId, expectedRevision?}`
     → `{parentAgentId, childAgentId, disposition: "cancelled"|"not_running", error}`
   - 客户端只发送 Ait 身份；原生子 ID 由服务端从已登记的 observed 记录解析，永不来自请求。
   - 两者在 SDK 的 `METHODS` 中登记到服务通道（channel 3），不进入 Agent 通道：Agent 通道
     已占满 Rust hello 的 64 项能力上限（ADR-044），再增两项会让分体模式握手被服务端拒绝。
2. 执行器新增 `Operation::ChildGuide` / `Operation::ChildStop`，`access()` 为 `Access::Session`、
   `target()` 为 `Target::Parent`，与子会话列表/历史一样在**父 Agent 的会话 lane** 内保序执行。
   没有按名称前缀推断的权限；方法 spec 与 execution 分支白名单同步登记，未登记即 `MethodNotFound`。
3. 端口沿用既有默认拒绝约定（同 `steer_turn`），但只同步报告**已写出**：
   `AgentSession::guide_child(&mut self, native_child_id, message) -> Result<ChildControlReceipt, AgentSessionError>`
   与 `stop_child(&mut self, native_child_id) -> Result<ChildControlReceipt, AgentSessionError>`，
   默认 `Err(Rejected)`。`ChildControlReceipt { token }` 是派发收据，最终结果作为
   `AgentTurnEvent::ChildControl { token, outcome }` 经既有事件流异步到达，
   `outcome` 为 `ChildGuideOutcome::{Accepted, NotRunning}`、
   `ChildStopOutcome::{Cancelled, NotRunning}` 或 `ChildControlFailure::{Rejected, Uncertain}`。
   结果类型仍不是 `Value` 进出的 action 字符串。
   这一拆分是必需的：若端口在 `&mut self` 上等待最终结果，父 lane 在下一次消息前无法处理排队的
   权限问题与读取（见第 9 条）。
4. `AgentManager::guide_child/stop_child` 在任何原生写入之前完成校验，顺序固定：
   父记录存在且为 `managed`、未归档、非内部，并且本进程持有其 live writer（`live_snapshot`）；
   子记录存在且有 `observation`；`observation.parent_agent_id` 与 `root_agent_id` 都等于请求的父
   Agent；`ready` 为真且 `health == Current`；提供 `expectedRevision` 时必须与观测版本一致；
   引导额外要求 `status == Running`（原生 steer 只对 running/pending 子级有效）。不满足一律
   在写入前拒绝，且不向调用方披露其他 Agent 的存在。**父级不 live 即拒绝，不 resume、不重建
   writer**；UI 在父 writer 就绪前把控制显示为不可用。
5. 窄豁免的实现方式是作用域，而不是放行名单：observed 请求守卫与 `require_managed` 都以请求里的
   `agentId` 为目标，而这两个操作只携带 `parentAgentId`/`childAgentId`，子级仅作为被校验的观测记录，
   因而永远不成为 lane 目标。对子 Agent 自身 id 的 send/cancel/配置/rewind/refresh/detach/resume
   仍被原有守卫拒绝；不调用 `require_managed(child)`，也不为子级创建 writer 或第二份流。
6. 结果语义显式：引导 `accepted` 表示原生运行时接受了这条 steering 用户消息，`not_running`
   表示没有投递；停止 `cancelled` 表示原生运行时接受了硬终止，`not_running` 表示没有取消任何
   东西——两者都不是“已停止”的成功证明，终态仍由既有只读子会话生命周期事件观察。接受结果
   不确定时不自动重发。
7. OMP 是首个也是本阶段唯一实现：在父级已管理的原生 RPC 进程内，先用 `get_subagents` 复核成员
   身份，再发原生 `steer_subagent {subagentId, message}` / `cancel_subagent {subagentId}`。
   其他 adapter 保持端口默认拒绝；两个子控制 RPC 在写入前把它呈现为
   `UnsupportedCapability`，其余操作仍沿用既有 `map_manager` 映射；
   不做全 provider 假转发，也不新增子级 writer。
8. 控制等待期间 lane 必须继续工作（第 2.6/2.9 节的要求）：
   `Operation::ChildGuide/ChildStop` 在 lane 内只完成校验与一次原生命令写出，随后把 RPC 回复
   （`DeferredChildControl { parent_agent_id, child_agent_id, reply }`）挂在收据 token 上；
   同一 drain 或后续 poll 收到 `AgentTurnEvent::ChildControl` 时由
   `ExecutionState::settle_child_controls` 结算该回复。因此排队中的
   `agent.permission.resolve.request` 与读取在控制等待期间正常完成，控制也不需要另起任务、
   第二个 writer 或新的 lane。适配器侧的同类拆分：同步事件循环只登记待写命令
   （`service_controls` 异步钩子写出），答案按各自的原生 frame 关联；超时与关闭都以
   `Uncertain` 结算，绝不重发。
9. 能力协商保持加法式：host feature `subagentControls` 由两个方法都存在推导（同
   `unifiedSubagentAgents` 的做法）；会话级新增可选布尔 `supportsChildGuide`/`supportsChildStop`，
   只从 adapter 已验证的事实写到**父** Agent 快照；observed 子级的所有写能力标志继续为 false。

## 接口变化与迁移（完整 ledger）

| 边界              | 旧接口/调用链                                             | 新接口/调用链                                                                                  | 迁移与兼容性                                                                  |
| ----------------- | --------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| daemon wire       | 无子级控制方法                                            | `agent.subagents.guide.request` / `agent.subagents.stop.request` 及其 response                 | 纯新增；旧客户端不发即不受影响，旧 host 返回 `MethodNotFound`                 |
| 执行器分类        | 子级列表/历史为 `Access::Session` + `Target::Parent`      | 追加 `Operation::ChildGuide/ChildStop`，类别相同                                               | 封闭枚举穷举新增；无隐式权限，未登记方法仍 `MethodNotFound`                   |
| 会话端口          | 无子级控制；`steer_turn`/`cancel_turn` 默认拒绝           | `guide_child`/`stop_child` 默认 `Err(Rejected)`，返回 `ChildControlReceipt`；结果经 `AgentTurnEvent::ChildControl` 异步到达 | 现有 adapter 不实现即保持拒绝；不改变既有方法签名；新增 `service_controls` 默认空实现 |
| manager           | 子级写入一律 `require_managed(child)` 拒绝                | `guide_child/stop_child(parent, child, …)` 校验父链+live parent+观测事实后经父 session 执行    | 无既有调用方；observed 通用写守卫不变，仅新增两个父作用域入口                 |
| 公共 SDK          | donor `runNativeAction(agentId, action, args)`（main 无） | `guideSubagent(parent, child, message, opts)` / `stopSubagent(parent, child, opts)`            | 不引入 `agent.native_action.request`；原生 ID、命令名与错误字符串不出 adapter |
| host feature      | 无 `subagentControls`                                     | `serverInfo.features.subagentControls`（由两方法存在推导）                                     | 可选布尔；缺席即隐藏控制，无必填字段                                          |
| 会话能力标志      | 无子级控制标志                                            | 父快照 `supportsChildGuide`/`supportsChildStop`（adapter 已验证，OMP 专属）                    | 加法式开放索引；observed 子级写标志继续全 false                               |
| 移动端子 Agent 行 | donor 组件用 native action 字符串 + 免确认停止            | 类型化 SDK 调用；运行中/待定才可操作，停止需命名目标的确认，结果分 accepted / not_running 展示 | 组件出现在统一子 Agent 行；无 vibe 分支、无原生 ID 输入、无原始 JSON 卡片     |

旧 host + 新客户端：feature 缺席即不显示控制；直接调用得到 `MethodNotFound`，不回退到任何其他执行
路径。新 host + 旧客户端：新方法不被调用，既有 observed 列表与时间线不变。
新 host + 未实现该端口的 adapter：端口默认 `Rejected`，两个 RPC 在写入前把它映射为
`UnsupportedCapability`（只作用于这两个方法，其他操作的既有 `map_manager` 映射不变），能力标志不出现。
客户端以能力标志而非错误码判断是否显示控制，未把该码当作可重试信号。

## 后果

子 Agent 的干预收敛到父 Agent 的单一 writer，保持 ADR-091/ADR-209 的单 writer 与只读观测边界；
停止是原生硬终止（单子任务、父回合与兄弟子任务不受影响），不是优雅取消，也不是进程组信号。
引导只对运行中的子级有效；完成的子级没有“恢复后引导”的假路径。控制结果的不确定性由
`ChildControlFailure::Uncertain` 明确表达，不自动重发，也不伪造成功。关闭父会话时既有的 owned-process
清理仍是 Ait 侧唯一回收路径。定向验收覆盖：wrong-parent/stale-owner 在原生写入前拒绝、子身份
稳定、引导/停止各恰好到达原生一次、`not_running` 不被当作成功、非 live 父级拒绝、非 OMP adapter
默认拒绝、审批在控制等待期间继续，以及旧 host/旧 adapter 的降级；具体命令与结果随 Draft PR 描述
记录，本 ADR 不预填验证证据。当前说明见 [Provider 规格](../../architecture/provider.md)。
