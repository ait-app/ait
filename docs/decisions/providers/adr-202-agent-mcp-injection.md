# ADR-202：Agent MCP 运行时自动注入

- 状态：接受
- 日期：2026-10-29
- 修订：[ADR-148](../daemon/adr-148-daemon-mcp.md) 的 Provider 自动注入范围与专用路由认证。

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

参考 [Paseo 当前实现的复核](../../reports/providers/paseo-mcp-injection-review-2026-10-30.md)，
保留其持久与运行时配置分离、按 Provider 能力交付的设计。当前没有内部注入配置自动回流到
Ait record 的路径，因此不采用仅按同名与 URL pathname 删除旧项的规则；未来引入完整原生
配置迁移时，应在导入处通过可证明的内部所有权识别并剥离，保留显式用户配置。

验证覆盖运行时与持久配置分离、能力/策略分支、后续输入、恢复/导入、URL 与身份绑定、
令牌的路由范围，以及离线原生客户端通过真实 daemon 完成握手、发现和工具调用。
操作与生效边界见 [MCP 手册](../../operations/mcp.md)。
