---
id: ADR-247
status: proposed
date: 2026-11-11
paths:
  - crates/provider
  - packages/protocol
  - apps/mobile
---

# ADR-247：动态发现的模型角色与私有 overlay 生命周期

## 背景

PR612 拆分任务 21 之前，OMP 适配器只暴露四个布尔开关
（`autoCompaction`/`autoRetry`/`fastMode`/`slowMode`），`config::validate` 拒绝任何
`modelRole.*` 键。OMP 原生支持按角色（default/smol/task/plan/…）分配模型与思考档位，
角色集合随用户 profile 变化，并非固定枚举。donor（fd2798e4）用一张 16 项 BUILTINS 表
渲染角色，离线时无论 profile 如何都显示固定 16 角色，并把英文营销标签硬编码进适配器——
这是把固定角色集泄漏进通用 UI 的缺陷。

本决策只覆盖 OMP 适配器的角色发现/校验/overlay；Claude advisor 单槽属任务 22，
原生 preset CRUD 属任务 23，worker_thinking 访问器属任务 33，均不在本切片。

## 决策

1. **角色是 provider 发现的数据，绝非共享枚举。** 唯一跨 provider 契约是既有通用
   `AgentFeatureSelect` 加特征键命名空间。角色集合 = live profile 的
   `modelRoles` ∪ `modelTags` ∪ `cycleOrder` 的并集；标签来自 `modelTags[role].name`，
   否则用原始 role id。发现与离线渲染都**不含 BUILTINS 表**。
2. **持久化复用既有 opaque 映射。** `StoredAgentConfig.feature_values["modelRole.<id>"]`
   取值 `"native"`（继承，overlay 省略该角色）/`"unset"`（显式未分配，overlay 写 null）/
   `"<selector>"` 或 `"<selector>:<effort>"`（显式分配，selector=`provider/id`）。
   清除回继承是 `Set("native")`，不是 JSON null；null 只出现在 overlay 文件，绝不作为存储值。
   `ConfigPatch` 形状不变，`featureValues` 逐键 extend；角色编辑不触碰 model/mode/权限/工具。
3. **新增 `local/omp/roles.rs`（全部 `pub(super)`）**：`Catalog::{read,features,validate,append}`
   与 `roles::{is_key,valid,overrides,same_overlay,stored_features,overlay}`。`read` 串行执行
   `omp config list --json` 与 `omp models --kind all --json`（精确 cwd+环境、有界校验、
   凭据 allowlist）。kind 接受矩阵、`@alias` 解析、`:effort` 生成/校验均为适配器私有。
   每个 shipped 方法都有实际 21 消费者；无 TODO、无 dead allow、无未用脚手架。
4. **私有 overlay，绝不写用户原生配置。** `overlay()` 物化 AIT 拥有的临时文件
   `{"modelRoles":{role:selector|null}}`，经 `--config` 只读传入；全继承时不创建文件。
   角色变更走与 05 mode/systemPrompt 相同的原子重开：同一 identity+confinement、quiet gate
   （非 streaming/compacting、settled、无 pending async、零队列、无 BTW/命令）否则 Busy、
   一次恢复重连、resume sessionFile 并校验 sessionId；沙箱经同一 `connect()` 生效，
   且必须仅允许读取该 overlay 临时文件（运行时证明，见验收）。
5. **未知存储值仅展示、不可应用。** 原生目录无法解析的已存储角色值渲染为自身
   display-only、可见 unresolved 的选项，但**不**作为已验证能力 advertise、**不**可应用：
   适配器拒绝任何触碰该角色的 apply（含重新提交存储值），UI 禁用其提交；仅 `"native"`
   保留为可应用逃生口。一旦原生读取解析成功，恢复为正常已确认选项。
6. **最小适配器内目录缓存，无新基础设施。** 渲染的角色特征搭乘已提交 agent snapshot 的
   `features` 数组（`control_settings`），由既有 `agentId/serverId/cwd` identity 围栏。
   CLI 读取的角色目录用 donor 最小回退：host 级 client 上 `sha256(cwd‖environment)` 键、
   LRU 32、按键串行读取；snapshot refresh 失效匹配键；陈旧 identity 回复丢弃不覆盖。
   无并行 refresh bus、无新 revision 面。
7. **通用 UI 绝不解析 OMP 字符串。** `AgentSelectOption.metadata`（开放 JSON）携带适配器
   计算的展示事实（`nativeSelector`/`resolvedSelector`）；UI 渲染 label/metadata，不解析
   id/selector/alias/:effort。角色 select 经既有通用 select 控件渲染，标注“下一轮生效”
   （适配器提供的 description）。9 个 locale 资源（ar/en/es/fr/ja/ko/pt-BR/ru/zh-CN）；
   角色标签文本保持适配器/native-tag 来源，绝不硬编码角色列表进资源。

## 接口变化与迁移

| 边界         | 旧接口/行为                                              | 新接口/行为                                                           | 影响调用方与兼容性                                                                                                                                 |
| ------------ | -------------------------------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| 特征键       | `config::validate` 拒绝 `modelRole.*`                    | 经 `roles::valid` 有界形状接受；`apply` 跳过角色键（走 overlay/重开） | 其余 provider 的 validate 继续拒绝 `modelRole.*`；非角色键不变                                                                                     |
| 通用 UI DTO  | `AgentFeatureSelect`/`AgentSelectOption`/`AgentMetadata` | 完全复用，无新 TS union、无 providerUi、无新 store                    | 旧客户端把角色 select 当通用 select 渲染（label 为适配器字符串），可按 id 设值；旧主机不返回角色特征，merge 回退存储/发现列表，不解析 id，缺失良性 |
| 持久化       | `feature_values` opaque 映射                             | 同映射存角色值；未知键已存活于 main patch/apply                       | 无存储迁移；字段形状不变                                                                                                                           |
| 重开生命周期 | 05 mode/systemPrompt 原子重开                            | 角色 overlay 变更加入同一重开触发集                                   | 沿用 05 原子/恢复形状；durable 生命周期变化，需旧/新调用方分析                                                                                     |
| 内部事件     | 05 `FeatureValuesUpdated(BTreeMap)` 单写者               | 角色值经同一 `FeatureValuesUpdated` 持久化                            | 无第二写者、无新事件变体（除非角色专属 delta 必需）                                                                                                |

`ConfigPatch::resolved()` 仅在适配器扩展的角色值需折回持久补丁时新增，构建于 05 同一
`NullableSetting`，绝不 fork。`worker_thinking` 访问器（33）、preset（23）、advisor（22）
不在本切片；21 不 ship 未用符号。

## 后果

OMP 角色配置端到端可用：发现随 profile 动态、校验对 live 池、私有 overlay 下一轮生效、
未知值仅展示不可应用、通用 UI 零字符串解析。donor 的固定五角色编辑表与 `stored_features`
BUILTINS 回退两处 provider→UI 泄漏均移除。沙箱 overlay 只读允许需运行时证明后方可验收。
ADR 状态 proposed：随 Draft PR 提交评审，不代表架构评审已通过。现行说明见
[Provider 架构](../../architecture/provider.md) 与 [OMP 操作手册](../../operations/oh-my-pi.md)。
