---
id: ADR-241
status: proposed
date: 2026-11-11
paths:
  - crates/provider
  - packages/protocol
  - packages/client
  - apps/mobile
---

# ADR-241：原生确认的 Agent 配置提交与字段级部分成功

## 背景

PR612 拆分任务 05 之前，`AgentManager::configure` 只做校验后把补丁写入持久记录，
原生会话要到下一轮 `start_turn` 才应用配置。界面在 RPC 返回后立即把选择写入本地偏好，
即使原生拒绝也显示新值；插件在会话外修改模型/thinking 时，持久记录与原生事实不一致。
OMP 原生 RPC 已能对每个设置字段给出确认或拒绝（任务 01 的原生 Session 边界），
但应用层没有把“原生确认”作为发布前提，也没有字段级部分成功语义。

本决策只覆盖 OMP 适配器的原生确认提交；其余 Provider 没有逐字段确认能力，
保持既有的延迟校验+持久化语义，不声称已确认。

## 决策

1. `AgentSession` 端口新增 `apply_config(proposed) -> ConfigApplyOutcome { config, rejected }`，
   默认返回 `Unavailable`。只有具备原生确认能力的适配器实现它；`Unavailable` 时应用层
   回退到既有延迟提交，不新增第二套配置语义。
2. OMP 适配器逐字段应用 model/thinking/feature，并在每个原生确认到达时立即记录该字段的
   实际确认值（适配器私有进度：已确认 model/thinking/feature 集合 + 拒绝字段列表）。
   原生拒绝的字段保留旧值并记入 `rejected`（稳定 wire 名，如 `modelId`、
   `featureValues.fastMode`）。全部写入成功后必须 `get_state` 回读，model/thinking 以回读
   事实为准（插件改写也如实持久化）。传输错误时不做回读（请求失败即关闭传输）：只把
   错误前已确认的字段作为类型化观察事件（`ModelSelectionUpdated` + `RuntimeInfo` +
   `FeatureValuesUpdated`）排队，由管理器在返回错误前排空持久化；未确认字段一律保留旧值，
   不从提议值推断、不做投机回滚。可选的 service-tier 探测失败只降级开关展示，
   不丢弃已确认事实。
3. mode/systemPrompt 变更，以及清除 model/thinking 宿主覆盖（显式 null），走原子重开：
   原生 setter 没有“取消设置”命令，恢复原生继承只能重开。同一 sessionFile 身份与同一
   confinement，失败时用旧配置做一次恢复重连；不重放用户输入，不放宽沙箱。
4. 原生配置事实分两类，均归 OMP 会话所有，经唯一持久化写者（`validate_config_update`）落盘：
   - **完整 `get_state` 快照**（携带 `sessionId`）只是观察：刷新 state 与 runtime_info（含会话
     身份），但绝不把继承的 `None` 变成显式覆盖，也不回退已存储的覆盖。
   - **显式原生变更事件** `config_update`/`model_changed`/`thinking_level_changed` 是被接受的
     选择更新：把实际 model/thinking 持久化为存储配置，使其在重开/恢复后仍然生效（插件在会话外
     的修改同样持久化）。官方 omp/18.17.0 中这些事件仅在实际变更时发出，`thinking_level_changed`
     携带 `configured`/`resolved` 且 configured 时写入原生 settings——属配置意图，而非被动观察。
   - **ACK 超替（owner 围栏）**：Ait `set_*` 被确认且 `get_state` 回读后，调用 `supersede_buffered`
     丢弃回读前缓冲的同类选择事件（复用 donor transport 的超替模式），使滞后的 pre-ACK 变更不能
     回退新确认值；ACK 之后到达的事件仍权威。无关 turn/interaction 帧保持顺序与字节计数。
     角色目录等后续任务复用该路径，不得另起写者。
5. 配置 RPC 响应在既有 `accepted/error/notice` 之上新增可选 `rejectedFields: string[]`。
   任一字段被拒绝时 `accepted=false` 并给出摘要错误；已确认字段照常持久化并发布快照。
6. 界面只在 RPC 确认后写入本地偏好；提交期间控件显示 busy 并禁止重复提交，
   拒绝时保留原值。通用 UI 不解析 OMP 字符串、别名或 thinking 后缀。

## 接口变化与迁移

| 边界        | 旧接口/行为                                                                                     | 新接口/行为                                                                                                                                          | 影响调用方与兼容性                                                                                                                                                   |
| ----------- | ----------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust 端口   | `AgentSession` 无实时应用方法；配置仅在下一轮应用                                               | `apply_config` 默认 `Unavailable`；OMP 实现逐字段确认                                                                                                | 其余适配器不改实现即保持延迟语义；`AgentManager::configure` 改为 `&mut self` 并返回 `Vec<RejectedConfigField>`，调用方（RPC controls、bot 收敛、权限跟随补丁）已迁移 |
| 内部事件    | `AgentTurnEvent` 无配置观察变体                                                                 | 新增 `ModelSelectionUpdated { model, thinking_option_id }`（`NullableSetting`，省略字段不序列化）与 `FeatureValuesUpdated(BTreeMap)`（仅已确认开关） | 仅 provider 内部与 OMP ACP 桥投影消费；桥对未知变体返回空更新，外部 ACP 客户端不受影响                                                                               |
| daemon wire | `agent.{mode,model,thinking,feature}.set` / `agent.config.apply` 响应仅 `accepted/error/notice` | 同一响应新增可选 `rejectedFields`；部分拒绝时 `accepted=false`                                                                                       | 旧客户端忽略新增字段，仍按 `accepted=false` 显示错误；旧主机不带该字段，SDK 视为空列表。无方法增删，无请求格式变化                                                   |
| 公共 SDK    | 五个配置 setter 拒绝时抛普通 `Error`                                                            | 抛 `AgentConfigRejectedError extends Error`，携带 `rejectedFields`；返回类型不变                                                                     | 既有 `catch (error)`/`error.message` 调用方不受影响；需要字段级展示的新调用方可读取 `rejectedFields`                                                                 |
| UI          | 选择即写本地偏好，拒绝仅 toast                                                                  | 确认后写偏好；busy 防重复；拒绝保留原值并显示本地化摘要+字段标签，原始后端信息作为次要详情                                                           | 仅 `apps/mobile` composer agent-controls；draft 控件保持本地草稿语义不变                                                                                             |
| 持久化      | 记录保存提议值                                                                                  | 记录保存原生确认值；不确定时不写                                                                                                                     | 重启/恢复读取同一记录；旧记录无需迁移，字段形状未变                                                                                                                  |

旧主机降级：客户端连接不支持逐字段确认的主机时，响应无 `rejectedFields`，
行为与今天一致（整体接受或整体拒绝）。本决策不改变 `server_info` 能力协商。

## 后果

OMP 的设置显示与持久记录以原生确认为准，字段级部分成功不再产生“界面新值、原生旧值”
的假象；传输不确定时宁可失败也不发布虚假回滚。其余 Provider 的延迟语义保持不变，
直到各自具备原生确认能力再实现同一端口。ADR 状态为 proposed：随 Draft PR 提交评审，
不代表架构评审已通过。当前说明见 [Provider 架构](../../architecture/provider.md)
与 [OMP 操作手册](../../operations/oh-my-pi.md)。
