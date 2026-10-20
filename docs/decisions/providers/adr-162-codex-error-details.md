# ADR-162：Codex 失败展示原生原因与 HTTP 状态

- 状态：Accepted
- 日期：2026-10-19
- 修订：[ADR-144](adr-144-provider-error-reporting.md) 的 Codex 用户可见说明策略。

## 背景

Codex 将余额不足等上游响应包装为连接错误。仅修正固定分类，用户仍然无法看到
`insufficient balance`、Cloudflare 拒绝访问或缺少 `response.completed` 等具体原因。
用户明确要求正确暴露错误。现有 `AgentSession::failure_message()` 已用于其他 adapter，
共享协调层也能将同一说明写入聊天 Timeline、`lastError` 和 `turn_failed.error`。

## 决策

1. Codex adapter 使用已有的 `failure_message()` 返回有界、脱敏后的原生错误说明。
   取值仅限 RPC error、当前轮次的 error 通知或失败终态的 `error.message`；
   若说明本身是 API 错误 JSON，提取内层 `error.message`。
   不将 stderr、完整响应、请求、工具参数或 `additionalDetails` 作为展示说明。
2. 展示包含 Codex 来源、已有的数字 HTTP／RPC 状态，以及原生说明。
   缺少有效说明时才回退到固定分类。明确观察到原生重试后失败，附加重试未恢复的提示；
   只观察到 `willRetry=false` 时说明不会自动重试。不推断重试次数，不重发输入。
3. 展示前移除已知凭据字段、Bearer、常见 API token／JWT；URL 仅保留 origin，
   移除 userinfo、路径、query 和 fragment。清理控制字符；最多检查 32 KiB 文本，
   原因最多保留 3072 字节，按 UTF-8 边界截断并明确标记，最终说明不超过既有 4096 字节预算。
   脱敏不能保证任意服务自定义文本都不含敏感内容，因此仅处理上述原生说明字段，
   不自动收集其他诊断载荷。
4. 数字状态与固定分类继续通过 `Diagnostic` 用于聚合日志，不把原生说明写入后台日志。
   展示状态与日志状态分开，原生重试中的错误不成为最终失败通知；成功恢复不显示失败。
   关闭 native transport 前将最终说明转移到 session，新输入清除旧说明及重试状态。
5. 共享协调层沿用已有持久失败提示与保留规则，各客户端显示同一 `message`。
   不增加 RPC、wire 字段或 domain 依赖，不回写原生 Codex 历史，也不猜测旧泛化提示的原始原因。

## 后果

聊天内可以直接看到具体上游失败原因、HTTP 状态和已知重试结果，刷新后仍能保留。
已有错误日志复制入口使用相同的说明。旧的泛化错误不会因为升级而自动变成具体原生错误。
本决策改善诊断可见性，不修复上游余额、访问限制或模型流断开的外部原因。

## 验证与 Test coverage

定向验证记录在 [Codex 排查报告](../../reports/providers/codex-upstream-errors-2026-10-19.md)。
**not measured**：用户未请求覆盖率；后续测量限定 provider crate 和 Codex 失败展示相关测试。
