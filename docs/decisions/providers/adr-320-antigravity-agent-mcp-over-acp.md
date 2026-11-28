---
id: ADR-320
status: accepted
date: 2026-11-28
amends:
  - providers/adr-087-antigravity-cli-provider.md
  - providers/adr-232-agent-mcp-session-delivery.md
  - providers/adr-312-antigravity-acp-migration.md
paths:
  - crates/provider/antigravity
---

# ADR-320：Antigravity 经 ACP 接收会话级 HTTP MCP

## 背景

[ADR-232](adr-232-agent-mcp-session-delivery.md) 未接入 Antigravity：当时主会话是 headless CLI，
MCP 只能来自用户级 `mcp_config.json` 和插件目录，没有单会话入口。
[ADR-312](adr-312-antigravity-acp-migration.md) 把主会话迁到 ACP 后，`session/new` 与 `session/resume`
带有 `mcpServers`，但该决策把“不注入 MCP”列入权限边界，adapter 固定发送空列表并拒绝带 MCP 的配置。

AGY 1.5.18 的 ACP `initialize` 不声明 `mcpCapabilities`。实测它仍接受 HTTP 项：两种打开方式下都带着
配置的请求头连接，`session/new` 下与 daemon 完成 `initialize` 并读取 `tools/list`。
模型实际调用工具和旧版 AGY 的行为没有验证。

## 决策

修订 ADR-087 决策 7、ADR-232 的“不接入”一条和 ADR-312 的“配置切换与权限边界”一条；其余不变。

- Antigravity 声明 `supports_agent_mcp`，与其他 adapter 一样接收 runtime 的绑定
  （[ADR-202](adr-202-agent-mcp-injection.md)、[ADR-215](../bot/adr-215-bot-permissions-and-scoped-mcp.md)）。
- 会话配置中的 MCP 只接受 HTTP 项，原样转为 ACP `mcpServers` 随 `session/new` 与 `session/resume`
  发送；stdio 与 SSE 未经验证，继续拒绝。凭据只在协议消息中，不进入原生 handle。
- 不以 `mcpCapabilities.http` 为前提。OpenCode 在原生未声明时拒绝打开会话；AGY 从不声明，
  照此处理等于永不交付。这是只属于 Antigravity adapter 的例外，不改变其他 ACP adapter 的规则。
- `mcpServers` 变化沿用 ADR-312 的配置切换：在轮次边界按同一 conversation 重连，不单独拒绝。

## 后果

- 默认配置下 Antigravity 会话获得 Ait 编排工具，内置 adapter 全部接入自动注入。
- AGY 没有承诺这一能力：上游可能改为拒绝未声明的传输，旧版也可能拒绝。届时会话无法打开，
  关闭 `mcp.injectIntoAgents` 恢复普通会话的原行为；Bot 会话不受该项控制，此前因不可交付而被移除 `ait`，
  现在总会收到作用域凭据，只有关闭 `mcp.enabled` 才能停止。按版本或握手结果降级留待出现实际失败时再定。
- ADR-312 对原生交互审批的拒绝不变：AGY 要求审批的 `ait` 调用被拒绝，只有 Full Access 放行；
  Antigravity 不声明追加式预授权，Bot 工单工作者的 `ticket_report` 同样如此。
- “如实标识 Ait、不读取凭据、不修改全局设置、不追加 system prompt”仍然有效。
