# ADR-154：账号状态通过共享客户端 Channel 推送

- 状态：已实现
- 日期：2026-10-17
- 关系：修订 [ADR-074](adr-074-account-host-relay.md) 的 HTTP 在线发现，沿用 [ADR-083](adr-083-online-service-host-sync.md) 的客户端与 Host 独立租约，以及 [ADR-132](adr-132-web-console-client.md) 的浏览器凭据归属。

## 背景

ait-server 提供 `ait-channel-v1` 推送完整 Host 快照和 Host 可连接状态事件。最初统一到 control 的入口现拆分为 `/v1/relay/channel`（账户状态）与 `/v1/relay/control`（Host 控制），独立鉴权、保活和重连。原客户端每 10 秒请求在线 Host，只能从列表缺失推断下线，无法保留中心记录的离线状态。

## 决策

- `packages/client` 的账户管理器注册客户端节点后，以用户 JWT 请求 `/v1/relay/channel-tickets`，使用一次性票据连接响应中的 `/v1/relay/channel`，不再轮询 `/v1/hosts/online`。服务地址的网关前缀保留；拒绝跨站、带查询参数或非协议规定的票据路径。
- Channel 的帧校验、应用心跳、握手和静默超时由共享 `account-channel` 模块处理。收到 welcome 后采用服务端声明的心跳参数，默认每 20 秒发送 `channel.heartbeat`；以消息的本地接收时间判活，不使用缓存快照的 `observed_at`。未知服务端事件忽略，非法已知消息、错误帧或传输失败关闭状态连接。
- 桌面 `ws` 与 React Native WebSocket 使用 Bearer 票据。浏览器使用 `ait.ticket.<ticket>` subprotocol，由浏览器在票据请求和升级时提供 Origin。JWT 不进入 WebSocket URL、subprotocol 或 daemon。
- 每个完整快照替换账户 Host 缓存，保留 nullable 身份、离线主机及中心 `offline_since`。已绑定 server ID 的离线 Host 可进入持久注册表；只有 `online` 为真、具备 instance ID 且支持业务中继模式的 Host 标记为可连接。资源管理字段 `status` 不参与在线判定；手工创建且未绑定 runtime 的 Host 不生成无效连接。
- `node.online` 表示 Host 已可连接，由 Host control ready 触发，并携带完整 `host`（含 server/instance 身份与中继能力）。客户端校验事件身份一致性后按 `host_id` 添加或替换主机，立即可供访问，包括此前未出现在快照里的新主机。`node.offline` 由 Host 路由清理触发，立即清除对应 Host 的可路由状态；channel 自身上线/下线不广播事件，不再维护独立的节点存在状态。完整快照用于初次与重连恢复以及资源变更对齐，事件不重放。
- Channel 断开保留最后的 Host 列表并标记 stale，采用带抖动的退避申请新票据重连。节点续租独立继续，重连前触发租约校验。手动刷新在健康连接上发送应用心跳；退出、关闭、后台暂停和节点更换释放 Channel，旧异步请求和事件不能覆盖新会话。
- 状态连接故障不直接关闭独立业务连接或下载；账户撤销和原有退出流程仍执行其传输清理。应用前后台切换沿用现有移动端宽限期。
- 生产客户端登录节点与各台已发布 daemon 是不同的节点会话。Channel 属于客户端登录节点，daemon 继续使用独立 Host control 和原有 Ping/Pong；状态 channel 与 control 使用不同 URL，服务端拒绝跨入口使用票据，并移除 control 上的 `channel.subscribe`。同一节点也可以建立两条独立连接；channel 故障不会清理 Host control 或已有数据会话。无需改变 Rust connector 或把用户 JWT 交给 daemon。

## 后果

桌面、Android/iOS 和 Web 复用同一账户发现生命周期。完整快照与实时路由事件共同维护 Host 可连接状态，HTTP 仅继续处理登录、租约和按需数据票据。客户端要求服务端支持独立 `/v1/relay/channel` 入口的 `ait-channel-v1`；旧中心不会静默回退到周期轮询，而会展示连接错误并重试。
