---
id: ADR-285
status: accepted
date: 2026-11-18
amends:
  - providers/adr-219-shared-account-usage-cache.md
paths:
  - crates/provider/claude
  - crates/provider/runtime
---

# ADR-285：账号用量查询遵守上游重试时间

## 背景

Claude OAuth 用量端点可能返回 HTTP 429，即使本地访问令牌仍有效。此前此响应变成通用读取错误，
错误缓存十五秒后重新查询，手动强刷也会继续请求，无法遵守上游的限流窗口。
此前请求也缺少原生客户端的 `User-Agent`；使用本机 `claude --version` 的版本构造原生格式并
标明 Ait 调用者，避免匿名请求。此适配细节由 Claude adapter 拥有，不改变 Runtime 认证边界。

## 决策

1. Adapter 负责解释原生限流响应。Claude 返回安全的 `error` 报告，使用已有的
   `nextRefreshAt` 表达重试时间，不转发响应正文，也不把限流视为需要重新登录。
   `Retry-After` 支持秒数和 HTTP 日期；缺失、无效、零或过期值至少等待六十秒，最长一小时。
2. Runtime 对带有效未来 `nextRefreshAt` 的错误报告设置内存重试截止时间，最长一小时。
   截止时间在接收报告时转为单调时钟，普通读取、后台刷新和手动强刷都不能提前轮询同一身份。
   截止时间后恢复原有缓存策略；正常报告的 `nextRefreshAt` 不限制手动强刷。
3. 沿用现有凭据代次和账号隔离。Host 只有当前凭据源可验证时才直接复用报告；未知源先准备并
   验证原生身份，同一身份可复用限流报告，无法验证或已切换身份时不继承等待或成功历史。
   限流报告保留同账号 `lastSuccessful`，不修改成功时间；重试状态不持久化。

## 后果

修订 ADR-219 的十五秒错误缓存及强刷例外。Adapter 拥有 HTTP 语义，Runtime 拥有账号缓存和
重试生命周期，客户端继续使用已有错误和快照投影，不增加 RPC 字段或凭据管理。
上游仍可能在重试后返回 429；Ait 能减少重复请求并说明原因，不能保证上游何时解除限流。
