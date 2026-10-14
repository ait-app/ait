# ADR-137：清理已移除功能的共享协议与 SDK

- 状态：已实现
- 日期：2026-10-12
- 关系：延续 [ADR-045](../daemon/adr-045-remove-hub-chat-loop.md)、[ADR-061](adr-061-app-ait-e2e-remove-relay-plugin.md) 的功能移除决策与 [ADR-127](../daemon/adr-127-remove-daemon-pairing-offer.md) 的协议清理方式。

## 背景

Rust daemon 没有 Hub、Plugin 管理、Chat 与 Loop 的实现或方法声明，但共享协议仍接受 34 个旧入站消息，SDK 仍暴露 Hub 与 Plugin 管理方法。调用只能在 transport 映射阶段失败，协议声明与实际能力不一致。

## 决策

- 删除这 34 个请求及配套响应、Hub 执行事件、旧状态 schema 和相关导出类型；删除独立 Chat、Loop、Plugin 管理 schema 与安装、版本检查工具。
- 删除 SDK 的 19 个公开 Hub/Plugin 管理方法、3 个私有助手、相关导入与功能门控。客户端不再声明 Hub 身份或 `hub.execute` 权限。
- 删除旧功能的 server-info 标志、Plugin 配置的显式类型与 4 个旧事件订阅项。`session.events.set_subscription.request` 保留，只接受当前 daemon 支持的 13 个事件类别。
- 保留已实现的 `agent.timeline.append.request`、插件来源时间线显示项、相关能力标志与 Agent Skills。这些入口不依赖已移除的 Plugin 产品功能。
- 固定上游契约快照、排除清单和历史报告继续作为来源证据，不能重新用于恢复当前入口。配置和未知状态的通用前向兼容解析保持原有策略，旧持久配置不会因此被破坏。

## 后果

共享入站协议从 206 个消息收敛到 172 个，与客户端 Rust transport 的 172 个映射一致。旧请求和响应在共享协议中视为未知消息，SDK 不再提供只能失败的调用入口。Rust 方法清单、分发与实现不变；daemon 继续对旧方法返回 `method_not_found`。

## Test coverage

未测量：用户未请求覆盖率。定向回归验证全部旧请求/响应的 discriminator 拒绝、生成的出站校验、旧订阅项与 SDK 方法移除，并保留有效订阅和 Plugin 时间线行为测试。另验证协议/SDK 构建、客户端类型与方法映射。没有 Rust 代码变更，按仓库规则跳过 Rust 测试；若后续请求覆盖率，将测量受影响的 TypeScript 协议与 SDK 测试并提供报告。
