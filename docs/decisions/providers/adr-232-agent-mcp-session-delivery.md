---
id: ADR-232
status: accepted
date: 2026-11-10
amends:
  - providers/adr-202-agent-mcp-injection.md
paths:
  - crates/provider/src/local/opencode
  - crates/provider/src/local/pi
  - crates/provider/src/local/deepseek_harness
---

# ADR-232：OpenCode、Pi 与 DSH 原生 Host 的 Agent MCP 单会话交付

- 修订：[ADR-202](adr-202-agent-mcp-injection.md) 的 Provider 范围与“其余 Provider 不经文件”交付规则。
- 关系：沿用 [ADR-215](../bot/adr-215-bot-permissions-and-scoped-mcp.md) 的作用域凭据与移除规则。

ADR-202 首阶段只为 Codex、Claude、DSH ACP 和 MiniMax Code 注入 `ait`。本决定只接入能从原生 CLI
自身的协议、帮助或文档确认“单会话生效且不写用户配置”的 Provider；无法确认的不注入，也不猜测。

- OpenCode：ACP `initialize` 声明 `mcpCapabilities.http`，`session/new`、`session/load`、
  `session/resume` 的 `mcpServers` 被添加到该 `opencode acp` 进程自带的私有 server（运行时集合，
  进程结束即消失）。凭据只在协议消息中。原生未声明 HTTP 时拒绝打开带 HTTP 服务器的会话。
  原生 handle 的 `config` 不再包含 `mcpServers`。
- Pi：每个原生进程以 `--extension` 加载私有扩展，扩展读取同目录的服务器定义并调用
  `pi.registerMcpServer`（官方文档说明注册不保存）。扩展源码不含凭据（jiti 可能缓存源码）；
  定义文件位于属主专用临时目录（0700，文件 0600），进程回收后删除。Ait 的值是字面量，
  写入前转义 Pi 的 `$`、`${}` 和前导 `!` 解析。
- DSH 原生 Host：每个会话独占的 Web Host 以 `--patch` 叠加层插入 `@deepseek-ai/dsh-mcp-client`
  插件行；叠加层为属主专用临时文件（0600），Host 停止后删除，profile 的 `cordis.patch.yml`
  不变。服务器名必须符合插件 `serverName` 规则。
- 不接入：OMP 只从用户/项目 `mcp.json` 读取 MCP，扩展 API 与 RPC 未提供单会话注册；
  Antigravity 只从用户级 `mcp_config.json` 和插件目录读取，CLI 没有单会话参数。二者继续保持原行为。

三者的 MCP 都在打开原生会话时连接，因此运行中的会话拒绝修改 `mcpServers`，后续输入沿用同一绑定；
daemon 重启、导入和重新打开时重新取得地址与令牌。Ait 配置中的用户 MCP 项与 `ait` 一起交付，
SSE 和需要 `PATH` 查找的 stdio 命令被拒绝。临时文件方式与 Claude 相同，允许运行命令的 Agent
仍可能读到，属于 ADR-215 已接受的风险。原生配置中的同名 `ait` 项按原生规则处理
（Pi 文件项优先、DSH 同名插件冲突、OpenCode 运行时替换），见 [MCP 手册](../../operations/mcp.md)。

验证覆盖：各适配器经 AgentManager 与离线 fake 检查注入内容、重启/导入重绑、持久记录与 handle
不含令牌、用户 `ait` 优先、`injectIntoAgents=false`、作用域凭据与拒绝时移除、只读历史不注入，
以及临时文件权限和随进程删除；并在本机以 OpenCode 2.2.5、Pi 1.5.0、DSH 0.2.0-rc.2 和本地假 MCP
服务完成握手与 `tools/list`，未发送模型请求。
