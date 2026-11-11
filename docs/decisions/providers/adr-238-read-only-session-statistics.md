---
id: ADR-238
status: accepted
date: 2026-11-11
amends:
  - providers/adr-091-independent-session-execution.md
  - bot/adr-221-read-only-bot-session-inspection.md
paths:
  - crates/provider
  - crates/api/src/guest
  - packages/protocol
  - packages/client
  - apps/mobile/src/components
---

# ADR-238：只读会话统计使用已提交事实

## 背景

PR #612 的原生统计结果通过字符串 JSON 猜测字段。不同 Provider 的计数口径不同，不能把
最新 `AgentUsage` 当作整场会话总量，也不能累加重复快照。

当前 Codex 映射取原生 `last` 用量（`crates/provider/src/local/usage.rs`），Pi 取 assistant
message 的 usage（`crates/provider/src/local/pi/streaming.rs`），Claude 取原生 result 报告。
OMP 的 `stream::add_usage` 累计原生消息用量；缺失缓存/费用会令对应累计字段保持未知，
不能把缺失当零或把部分费用展示成完整累计费用，明确报告的零仍然保留。
现有 `AgentUsage` 没有跨 Provider 的统一 turn/session 计数口径或每项观察时间。

## 决策

1. 增加唯一的 `agent.statistics.get.request`，参数为 `agentId`。复用 runtime directory 和
   registry 的已提交读取端口，归 `Operation::Statistics` / `Access::Read`。不恢复原生会话、
   不占用 writer、不发 prompt、不取消当前轮次、不写 Timeline。扩展 ADR-091 的独立读取清单。
2. 响应区分配置模型与原生 runtime 最后确认的有效模型，状态来自持久 runtime record。
   `updatedAt` 是记录时间，不冒充每项指标的采样时间。用量只解码既有 `lastUsage` 类型，
   `usageScope` 固定为 `provider_snapshot`；不根据 Provider 名字推断统一累计口径。
3. 所有缺失指标、配置模型和有效模型显式返回 `null`；已知零保留为零。指标包含输入、缓存输入、
   输出、原生报告美元费用和上下文使用量/上限。不估算价格，不求和快照，不制造缺失的 run 总计。
   原有 Timeline、Todo、Goal 与子 Agent 模型不变；本查询不创建另一份 accounting 状态。
4. 客户端的既有上下文详情入口展示选中会话事实，明确不包含子会话；账号额度继续使用既有账号
   用量资源并独立标注。子会话通过既有子 Agent 入口分别查看。
5. SDK 依据协商能力暴露 `agentStatisticsGet`，旧 Host 不发送不支持的请求并显示不可用。查询使用
   既有 connection epoch/client generation 和 Query cancellation，旧连接迟到结果不进入当前视图。
   不增加 Timeline 联合类型或持久消息，旧客户端仍读取原有消息。
6. 统计沿用 get 的可读身份规则，包括完整 ID 的 Bot 隐藏会话；扩展 ADR-221 的只读操作清单。
   Guest 仍按 `agentId` 校验资源范围，不因此获得写权限。

## 后果

这不是 OMP `/stats` Web 仪表盘或 `/status` extension console，也不提供通用 native-action
通道。无法保证为 session aggregate 的数据只标为最新 Provider 快照；已有报告的累计值会原样
显示而不是被再次累加。读取不需要模型凭据或正在运行的 Provider；持久事实可在重启后读取。
