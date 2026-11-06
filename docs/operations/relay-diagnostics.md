# Hub 与下载 Relay 连接诊断

在线业务使用 Hub，文件下载使用独立 Relay；账号 Channel 负责主机状态。
先区分故障通道，避免把下载成功或 WebSocket `101` 当作业务会话健康证明。
当前边界见 [ADR-204](../decisions/clients/adr-204-hub-only-online-business-transport.md)。

## 先检查状态

1. 从客户端导出[应用诊断](diagnostics.md)，记录时间、主机、平台和故障操作。
2. 在目标主机使用本地 API token 查询 `GET /api/device/authorization`，区分机器授权、Hub 和下载 Relay 状态。
   `aitd status --json` 是带时间戳的本地缓存，不能证明此刻在线。
3. 检查 daemon `info` 日志。桌面管理的日志在数据目录的 `daemon.log`；独立服务由启动脚本收集 stdout/stderr。
4. 将同一时间窗口的客户端、daemon 与中心日志按下表关联，再定位最早失败事件。

授权恢复和显式断开见[无人值守 daemon](headless-daemon.md)。客户端退出不会撤销机器授权；
完整断开会清除它，不能用反复断开替代暂时网络故障的诊断。

## 当前日志与链路

| 通道         | daemon 关联字段                                            | 排查重点                                              |
| ------------ | ---------------------------------------------------------- | ----------------------------------------------------- |
| Hub serve    | `hub_link`：`generation`、`node_session_id`                | 取票、握手、续传、连接被替换                          |
| Hub 业务会话 | `hub_session`：`session`                                   | 本地 daemon 握手、输出、会话关闭与容量限制            |
| 下载控制     | `relay_control`：`generation`、`node_session_id`           | 下载调度、控制连接、取消                              |
| 下载数据     | `relay_session`：`relay_session_id`、`epoch`、`elapsed_ms` | `/relay/sessions/<id>/client` 与 `/daemon` 的同次传输 |
| 账号状态     | 客户端 `ait-channel-v1`                                    | Host 快照、可连接状态与应用心跳；不承载业务 RPC       |

字段来源见 [`relay::hub`](../../crates/relay/src/hub/mod.rs) 和
[`relay`](../../crates/relay/src/lib.rs)。`center_cancelled` 表示中心取消下载；控制连接结束还可能
取消其所属下载任务。随后发生的清理事件不一定是另一个独立故障。

Hub 链路不可用、服务地址不匹配或目标没有 serve presence 时业务连接明确失败，按客户端重连机制重试；
不会退回旧 Relay 业务会话。物理链路续传只在中心允许的恢复窗口内保留逻辑会话，不重放业务写入。
独立下载成功不能证明 Hub 正常；账号列表里存在已保存主机也不能证明该主机当前可连接。

## 故障证据的限制

日志中的安全错误类别、阶段和系统错误应与之前的事件结合使用。`eof`、关闭帧或短连接时长
本身不能区分主动关闭、应用异常和网络中断。不要通过记录票据、Bearer、原始消息或带凭据 URL 补充证据。

旧 `relay_endpoint`、业务配对、Yamux 及 2026-10-12 Firefox 事件的说明已移到
[历史诊断记录](../archive/operations/relay-diagnostics.md)，只适用于对应旧版本。
当前重连行为以 [Hub 客户端](../../packages/client/src/hub/link.ts)、
[Host 连接](../../crates/relay/src/hub/mod.rs)和[机器授权流程](headless-daemon.md)为准。
