---
id: ADR-202
status: accepted
date: 2026-10-29
amends:
  - daemon/adr-148-daemon-mcp.md
paths:
  - crates/provider
  - crates/api
  - crates/provider-minimax
  - crates/provider-claude
  - crates/provider-deepseek-harness
  - crates/provider-opencode
  - crates/provider-pi
---

# ADR-202：Agent MCP 运行时自动注入

- 修订：[ADR-148](../daemon/adr-148-daemon-mcp.md) 的 Provider 自动注入范围与专用路由认证。
- 后续修订：[ADR-215](../bot/adr-215-bot-permissions-and-scoped-mcp.md) 为 Bot 会话及其启动的 Agent 签发作用域凭据：覆盖用户的 `ait` 项，Bot 会话不受 `injectIntoAgents` 控制，无法交付时移除 `ait` 项而不回退到共享令牌。
- 后续修订：[ADR-232](adr-232-agent-mcp-session-delivery.md) 接入 OpenCode、Pi 和 DSH 原生 Host；Pi 与 DSH 原生 Host 经属主专用临时文件交付。

daemon 已经提供 MCP Server，Skill 也已自动分发。连接应由原生客户端建立，因此由 host
将实际监听地址、进程级 MCP 专用令牌和共享配置存储交给 provider 的 `AgentMcp` 模块。
令牌每次进程启动生成，只被 API 的 `/mcp/agents` 路由接受，原有主凭证仍保持兼容。
身份继续使用 `X-Ait-Agent-Id`，不复制 Paseo 的 URL query，保持 Ait 的 URL 限制。

AgentManager 在创建、恢复和导入时，根据原生客户端能力与两个 MCP 开关准备有效配置，
只将该配置传给原生客户端。持久 record 继续保存用户配置；原生持久化 handle 也不包含
内部连接信息。绑定保存在 live Agent 中，执行器 fork 共享 host 配置源，后续输入合并
同一绑定，原生配置更新校验也使用该有效配置，registry 仍只提交用户 patch，避免将启动
配置覆盖为空或误触发 ACP 的配置不一致拒绝。

绑定与原生会话同寿命：新建或重新建立会话读取最新策略和连接信息；现有会话不因配置开关
变化而被强制重启。`mcp.enabled` 仍即时控制 HTTP 入口。只读历史、辅助生成和不支持 HTTP
MCP 的客户端不注入。首阶段支持 Codex、Claude、DSH ACP 和 MiniMax Code；其余 Provider 的原生工具目录
或 MCP 适配后续独立接入，不因本功能开启而拒绝原有 Agent 创建。

新配置默认开启注入，已有显式关闭值保持不变。用户名为 `ait` 的 MCP 项优先，其他外部项
全部保留；只分发 Skill 不会构造连接，Skill 文件中也没有运行时令牌。

MCP 配置可能携带令牌、请求头和环境变量，不得写进原生进程的命令行参数，因为同主机其他用户可通过
`ps` 或 `/proc/<pid>/cmdline` 读取。Claude 的 `--mcp-config` 因此只接收一个由 transport 持有的
私有临时文件路径（0600），原生进程结束、transport 释放时随之删除；其余 Provider 通过 stdio 协议
或原生配置请求传递，不经过进程参数。

参考 [Paseo 当前实现的复核](../../archive/reports/providers/paseo-mcp-injection-review-2026-10-30.md)，
保留其持久与运行时配置分离、按 Provider 能力交付的设计。当前没有内部注入配置自动回流到
Ait record 的路径，因此不采用仅按同名与 URL pathname 删除旧项的规则；未来引入完整原生
配置迁移时，应在导入处通过可证明的内部所有权识别并剥离，保留显式用户配置。

验证覆盖运行时与持久配置分离、能力/策略分支、后续输入、恢复/导入、URL 与身份绑定、
令牌的路由范围，以及离线原生客户端通过真实 daemon 完成握手、发现和工具调用。
操作与生效边界见 [MCP 手册](../../operations/mcp.md)。
