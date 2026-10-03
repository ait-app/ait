# ADR-075：Relay 协议定义与连接执行分离

状态：已实现，2026-10-03。

## 背景

[ADR-074](adr-074-account-host-relay.md) 引入了 Rust relay 控制连接、业务转发、独立下载和
daemon 单连接协商。消息类型字符串、JSON 字段读取和 WebSocket 收发原先混在连接执行代码中。
本次只整理该 PR 新增的 Rust 协议代码。

## 决策

- `relay::protocol` 定义控制 hello/welcome、open/cancel 命令、数据模式、配对确认和下载
  headers/end/complete 消息，使用 Serde 类型完成编解码。下载模式必须携带下载令牌，
  数据票据与下载令牌使用 `SecretString`，解码错误不包含原始消息。
- `relay::transport` 负责带鉴权的 WebSocket 建连、消息大小限制、收发超时和 ping/pong。
  `Connector` 管理控制代次、并发连接和取消；`bridge`、`download` 执行对应数据流程。
- 配对确认必须对应当前 `relay_session_id`。业务 hello 只检查消息类别，再原样转发；
  完整业务握手仍由本地 daemon 校验。其余业务帧继续透明转发。
- `protocol::single` 统一单连接 capability、feature 和能力数量上限。
  `Hello::requires_single_connection` 判断是否显式要求该模式，能力协商仍负责最终准入。

## 后果

现有 JSON 字段名、消息标签、下载帧顺序和确认时机保持兼容。缺少配对会话 ID 或 ID 不匹配
的消息现在会被拒绝。未知可选字段继续被忽略；未知消息种类与模式被拒绝。

保持 ADR-074 的 crate 依赖边界：`relay` 没有 workspace 依赖，只由 `api` 持有。
relay 数据模式和 daemon feature 虽然使用相同的线上字符串，分别属于中心路由与 daemon
能力发现两个契约；不因此建立 `relay` 到 `protocol` 的依赖。
原有 daemon 业务方法、参数类型和能力包归属不作调整。

验证范围及测量结果见 [协议重构验证报告](../../reports/clients/relay-protocol-refactor-validation.md)。
