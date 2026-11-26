---
id: ADR-312
status: accepted
date: 2026-11-26
amends:
  - providers/adr-087-antigravity-cli-provider.md
  - providers/adr-311-antigravity-worktree-sandbox.md
paths:
  - crates/provider/antigravity
---

# ADR-312：Antigravity 主会话统一使用官方 ACP

修订 ADR-311 的第 4 条：ACP 会话通过 `session/cancel` 请求取消，不再依赖向原生进程发送 SIGINT。
创建、恢复与配置切换仍使用 ADR-311 的外层 OS 写入隔离及当前进程的终端沙箱覆盖，权限边界保持不变。

修订说明（2026-11-26）：版本门槛已调整为按能力兼容旧版；保留纯 ACP 主会话，支持 ACP 的旧版仍可使用，
缺失功能和旧版收尾限制在相关操作提示升级。1.5.15 仅作为推荐修复版本，不再作为整个 Provider 的最低门槛。
这是 adapter 内部可用性与提示策略的调整，现行行为见 [Provider 规格](../../architecture/provider.md)
与 [操作说明](../../operations/antigravity.md)；下文记录最初的版本门槛决策。

## 背景

Google 官方 AGY CLI 提供 `agy acp` 作为第三方客户端协议。继续在 adapter 内并行维护
`--input-format stream-json` headless 主会话引擎与运行期降级路径，既违背官方个人计划对第三方封装
headless 的限制，也带来双协议状态复杂度与重复子进程探测。

AGY 1.5.15 修复了后台任务或子代理尚未结束时提前完成 ACP prompt，以及取消后工具状态不收尾的问题，
见[官方更新记录](https://antigravity.google/docs/changelog?ide=vscode&tab=extensions)。
低于 1.5.15 的旧版 CLI 不再提供 headless 兜底，改为直接提示用户升级。
原生个人登录与无人值守编排的产品形态保持不变；协议切换本身不承诺消除个人订阅的条款风险。

## 决策

修订 ADR-087 的决策 3、5、6、7；provider ID、客户端 API、认证、模型发现、摘要与额度查询边界保持。

- **主会话全面使用 ACP，移除 headless 主会话实现**：`create_session` 与 `resume_session`
  统一走 `agy acp`，复用 `provider-native` 的 ACP 传输、配置、流投影和权限解析。
  删除 `session.rs`、`streaming.rs`、`routing.rs` 及运行期 `legacy` 降级分支；runtime 不按 provider ID 分支。
- **最低版本门槛与升级提示**：要求 `agy >= 1.5.15`。版本过低或无法识别时，`is_available` 返回 `false`，
  `diagnostic` 返回带升级指引（`1.5.15 or newer`）的提示并通过 provider snapshot 暴露给用户，
  `create_session` / `resume_session` 直接返回 `Unavailable`，不回退 headless，不自动升级 CLI。
- **旧 handle 无缝恢复，不重放历史**：无 `metadata.transport` 标记的旧 handle 直接经 `session/resume`
  按原原生 conversation UUID 恢复并写入 `transport: acp`；ACP 恢复只接受原 conversation，身份变化即失败。
  可选 `acpUsage` 保存已消费 ACP 轮次的累计用量；旧 handle 恢复后从新的 ACP 轮次起累计，不混入旧计数。
- **单次认证与模型预检**：打开会话时仅执行一次只读 `agy models` 完成缓存登录预检与模型/effort 解析，
  随后校验版本并启动 `agy acp`。认证、额度、网络、超时或能力缺失直接失败并保留安全诊断，不触发协议重试。
- **配置切换与权限边界**：配置仅在轮次边界关闭旧进程并按同一 conversation 重连，应用后整体复核最终
  模型、思考等级和模式快照；未确认时保留先前配置且不提交输入。客户端如实标识 Ait，不读取凭据、
  修改全局设置、追加 system prompt 或注入 MCP。普通模式的原生交互审批由 adapter 拒绝，
  只有 Full Access 传递 `--dangerously-skip-permissions`。
- **轮次结算与用量账本**：轮次只由关联 prompt 响应结算；未完成工具、外来会话、过期响应或畸形输出均判失败，
  并统一收尾仍运行中的工具卡片。`usage_update` 只表示上下文占用，prompt usage 是单轮消费且 `thoughtTokens`
  已包含在 `outputTokens` 中，每轮只向账本提交一个 `Turn` segment。
- **保留辅助命令边界**：结构化摘要生成（`summary.rs`，依赖 `--json-schema` 与禁工具隔离）继续使用
  headless `stream-json`；账号额度查询（`quota.rs`）继续使用只读内置命令 `agy --print /usage --output-format json`。

## 后果

- Antigravity 主会话彻底收敛为单一 ACP 路径，不再保留 `agy < 1.5.15` 兼容层与运行期双协议切换。
- AGY 1.5.18 的原生 ACP 仍不返回 headless 提供的子代理 `conversation_id` 与 `log_uri`；
  子代理本身正常执行，卡片仅展示原生实际返回的信息。
- 已覆盖 Linux x86_64 / AGY 1.5.18 的真实录制回放与异常回归；macOS 和 Windows 的真机验收仍待补齐。
  操作说明见 [Antigravity 手册](../../operations/antigravity.md)。
