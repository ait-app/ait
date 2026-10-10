# ADR-127：移除 daemon 配对 offer RPC

- 状态：已实现
- 日期：2026-10-11
- 关系：修订 [ADR-026](adr-026-canonical-paseo-websocket-surface.md) 的 daemon 方法清单，延续 [ADR-061](../clients/adr-061-app-ait-e2e-remove-relay-plugin.md) 的配对范围决策。

## 背景

`daemon.get_pairing_offer.request` 仍在能力清单、协议和 SDK 中，但 Rust 实现只返回 `unsupported_capability`，没有产品调用方。

## 决策

- 删除 metadata 的方法声明、分发和执行分支。服务端不再公布或协商该能力，旧请求返回 `method_not_found`。
- 删除共享协议的请求、响应 schema 和响应类型，以及 SDK 的 `getDaemonPairingOffer`、options 和 payload 类型。Rust transport 不再声明或发送该请求。
- 将固定上游契约快照中的方法标记为已移除；保留上游 schema 和指纹作为来源证据。更新排除清单和目录计数。
- 生成的上游契约快照使用审计脚本的 `JSON.stringify` 格式，排除通用 oxfmt 重排；方法映射检查继续验证所有保留入口。

## 后果

旧客户端不能再调用该 RPC；依赖旧 SDK 方法或导出类型的代码需要删除调用。daemon 状态、配置、诊断和生命周期请求继续使用各自的入口。

## Test coverage

未测量：用户未请求覆盖率。定向回归验证能力清单、方法归属、协商拒绝、真实 daemon 的未知方法错误，以及 Zod 和编译后消息校验。若后续请求测量，将使用 `cargo llvm-cov` 覆盖受影响的 metadata、API 和 daemon 测试，并提供可审阅报告。
