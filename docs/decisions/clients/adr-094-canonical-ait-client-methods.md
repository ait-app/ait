# ADR-094：客户端统一使用 Ait 标准方法名

- 状态：Accepted。
- 日期：2026-10-07。
- 范围：`apps/`、仓库内 `@ait/client` 与 `@ait/protocol`。
- 修订：[ADR-044](adr-044-paseo-client-rust-transport.md) 中前端保留 Paseo 消息名称的决策。

## 背景

Rust 已使用 `crates/protocol/src/methods.rs` 中 `PASEO_METHODS.canonical_name` 接收请求。
客户端仍以旧名称索引方法目录、发送 SDK 消息并监听响应，直到传输层才转换名称。
这让 UI、类型校验和测试夹具继续依赖 Paseo 名称，也保留了合并操作的重复入口。

## 决策

应用调用、SDK 消息 discriminator、响应监听和测试夹具统一使用 Ait 标准方法名。
前端方法目录按 Rust catalog 的 canonical name 去重，目录键与发送到服务端的 method 相同。
Agent 创建、Project 图标读取和 Workspace 脚本启动只保留一个标准校验分支。
未启用创建生命周期订阅的 SDK 路径同样使用 `agent.create.request` / `agent.create.response`。
脚本启动从标准结果的 `script.terminalId` 读取终端。

Rust transport 继续负责 envelope、请求关联、功能连接、订阅归属、binary 帧及 SDK 状态 payload，
不再把旧操作名称映射成 Ait 请求。生命周期状态字段和 TypeScript API 标识不属于 RPC method。

事件监听使用标准事件方法名。当前 Rust `SessionEventKind` 的部分订阅参数仍采用独立标识；
这不是请求 method。它们与方法名的对应关系集中在 `@ait/protocol/session-event-kinds`，
传输层在订阅参数和服务端事件入口转换，应用不直接依赖这些参数标识。
Rust API、crate 领域依赖和数据所有权保持原有边界。

## 后果与验证

应用只依赖 Ait 标准操作名，仓库内 SDK 和协议包需要一起重建。
`scripts/check-paseo-client-methods.py` 校验去重后的方法目录与 Rust catalog 一致，
并拒绝 `apps/` 中重新引入旧操作名。相关协议、SDK、transport、目录推送和听写测试
覆盖名称迁移、合并分支、请求关联与事件订阅参数。
