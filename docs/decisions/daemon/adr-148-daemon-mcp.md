# ADR-148：daemon 的无状态 MCP 编排接口

- 状态：接受
- 日期：2026-10-14

## 背景

Paseo 通过 `/mcp/agents` 提供 Agent、Workspace、脚本、终端、定时任务、Provider 和权限工具。
Ait 已经实现这些能力的 Rust RPC，但没有标准 MCP 客户端可以连接的 HTTP 入口。

参考 `getpaseo/paseo` 提交 `9551b2cc11328170416b3d9753c7ec9a10ed35e7` 的
`agent/mcp-server.ts`、`agent/tools/paseo-tools.ts` 和 `bootstrap.ts` MCP 路由；协议依据
[Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)。

## 决策

- MCP 归 `api`，使用现有监听地址的 `/mcp/agents`，复用 Bearer、Host 和 Origin 校验。
  凭据和调用者身份均不进入 URL。可选 `X-Ait-Agent-Id` 请求头提供编排上下文，
  不构成额外权限边界。
- 采用无状态 Streamable HTTP。POST 返回 JSON，通知返回空的 202，GET/DELETE 返回 405。
  支持 `2025-03-26`、`2025-06-18`、`2025-11-25`，不分配 MCP session ID。
- 工具目录只暴露已安装的 Rust 能力。元数据和输入 Schema 编译期嵌入
  `api/src/mcp/catalog.json`，私有校验器检查该静态目录使用的 Schema 子集，拒绝未知参数。
  添加 Schema 关键字时须同步扩展校验器与测试。
- 一次性 RPC adapter 复用 `Option<Context>` 分发链和有界 Outbound，消费匹配的响应，
  请求结束时取消输出通道。不复制业务服务或持久化路径。Workspace 归档继续完成
  Agent、Bot、脚本、终端和 owned worktree 清理。
- 调用进入有界且跟踪的任务，HTTP 断开不会取消已接纳工作。MCP 调用等待上限 60 秒，
  HTTP 上限 65 秒；普通 HTTP 的 10 秒超时不覆盖此路由。停机拒绝新请求并取消 MCP 等待。
- `mcp.enabled` 即时控制入口，默认开启。提供 40 个工具，包含 Ait 的 `wait_for_agent`。
  Provider 自动注入、原生工具注册、Browser/Voice session 工具不属于这次服务端适配；
  `mcp.injectIntoAgents` 不因本次实现而改变 Provider 启动行为。

## Ait 适配

外部创建 Agent 提供 `cwd` 或 `workspaceId`；Agent 调用继承调用者目录和 Workspace，
已有 Provider 创建流程记录父子关系。Provider 使用 `provider/model`，发现接口用于选择可用组合。
重试创建复用 `idempotencyKey`，重试 prompt 复用 `messageId`。

Ait 不暴露未实现的 background Workspace 选项，列表提供有界分页。
Schedule 固定归属于 Workspace。Heartbeat 使用 scheduler 的 Agent target，会持久化，需显式删除。
`kill_agent` 关闭并归档 Agent，保留历史。权限工具复用 Provider 校验及 Bot 自动拒绝规则。
执行失败返回 `isError: true`；结果同时提供 `structuredContent` 和文本。

验证见 [报告](../../reports/daemon/mcp-implementation-2026-10-14.md)，接入见
[MCP 手册](../../operations/mcp.md)。
