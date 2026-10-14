# ADR-122：Codex Auto-review 的 computer use 工具批准

状态：已接受

日期：2026-10-10

## 背景

Codex 的 `approvalPolicy` 和 `approvalsReviewer` 管理会话的原生审批行为，
插件 MCP 工具另有逐工具审批配置。Ait 的 Auto-review 原来仅设置会话审批者，
没有为 `unified-computer-use@openai-bundled` 中的 `cua_repl.js` 设置预批准。
普通 `mcp_servers` 的工具配置也不能代替插件所属的配置路径。

## 决策

- Auto-review 在线程配置中设置
  `plugins["unified-computer-use@openai-bundled"].mcp_servers.cua_repl.tools.js.approval_mode`
  为 `approve`。由 Codex 执行工具批准，Ait 不解析 JavaScript，也不自动回答
  `requestUserInput` 或 MCP elicitation。
- 不增加独立开关。默认权限、Full Access 和只读模式不注入此设置。
  Auto-review 继续使用 `workspace-write`、`on-request` 和原生 `auto_review` 审批者。
- 允许 `providerOptions.plugins` 提供逐插件、逐服务器、逐工具的审批模式；
  显式设置优先于档位默认值。该入口不接受插件启用、进程配置或服务器级整体预批准。
- 进入或退出 Auto-review 时，重建原生进程并恢复同一线程，让线程级工具配置生效。
  会话身份和原生历史保持不变，设置不写入用户的全局 Codex 配置。
- 应用访问、网站访问、身份验证和操作系统授权继续由原生服务处理。
  预批准 MCP 工具不代表取得这些独立权限。

此决策延续 [ADR-052](adr-052-native-provider-capabilities.md) 的边界：
provider 适配原生配置和交互，原生 CLI 持有工具与权限执行逻辑。

## 后果

Auto-review 会直接批准 `cua_repl.js` 工具调用。仍然到达 Ait 的结构化问答需由用户回答，
从而不会把普通决策问题或独立访问请求当成工具预批准。
验证范围与支持限制见 [实施报告](../../reports/providers/codex-computer-use-approval-2026-10-10.md)。

配置路径来自 [OpenAI 官方配置参考](https://developers.openai.com/codex/config-reference/)，
已用本机 Codex `0.160.0` 的无模型 `thread/start` 请求验证。
