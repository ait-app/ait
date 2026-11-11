# 连接与传输现行规格

本页列出 daemon 的 HTTP/WebSocket 协议、准入、背压、Hub 在线连接、机器授权、本地入口和客户端 SDK
协议归属中当前生效的规则。每条规则均已对照现有代码；决策背景与修订历史见所引 ADR，
修改传输代码前应先确认不违反本页规则。组件总览见[当前架构](README.md)。

## 协议与握手

- 连接 envelope、协议版本、错误码、ID 校验和公开预算归 `model::server`；方法名称与消息方向
  由各组件以 `model::methods::MethodSpec` 声明，API 汇总并拒绝重复声明，不存在中心方法目录
  （[ADR-111](../decisions/daemon/adr-111-domain-values-and-server-protocol.md)、
  [ADR-095](../decisions/daemon/adr-095-component-method-declarations.md)）。
- 每条物理连接的首条消息必须是 `hello`，当前限时 10 秒；否则 daemon 发送无 `request_id` 的
  `invalid_message` 后关闭。协议主版本须为 1，缺少任一 `required_capabilities` 时拒绝握手；
  成功后返回 `server_info`，包含新的 `connection_id` 与本连接协商结果
  （[daemon 手册](../operations/daemon.md#websocket-与能力)）。
- `info.capabilities` 是全部组件声明的方法加 `connection.single.v1`，`implemented_capabilities`
  只含已安装的完整 crate 服务、基础连接方法及 relay、publication、shares 方法；行为差异用 `features`
  表达，不新增方法名（[ADR-100](../decisions/daemon/adr-100-crate-level-service-installation.md)、
  [ADR-108](../decisions/daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)）。
- 请求校验顺序固定：未知名称 `method_not_found`，方向错误 `invalid_message`，未协商
  `unsupported_capability`，已声明但未安装 `not_implemented`。`server.status.unsubscribe` 沿用
  `server.status.subscribe` 的协商（[ADR-026](../decisions/daemon/adr-026-canonical-paseo-websocket-surface.md)、
  [ADR-095](../decisions/daemon/adr-095-component-method-declarations.md)）。
- `request_id` 只关联本连接内的响应，非空、最多 128 字节且不含控制字符，不是持久幂等键；
  幂等键由具体创建或发送方法定义（[daemon 手册](../operations/daemon.md#websocket-与能力)）。
- 订阅归物理连接所有，断开即释放，重连须重新订阅；`subscription.release.request` 幂等。
  单连接模式下释放请求先送达全部四个 worker，再返回响应
  （[daemon 手册](../operations/daemon.md#websocket-与能力)、
  [ADR-108](../decisions/daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)）。

## HTTP 入口与鉴权

- 全部路由先校验 `Host` 与 `Origin`；除 `/healthz`、`/readyz` 及文件下载、终端活动上报等自带
  凭据的内部入口外，均要求 Bearer。重复的认证头返回 400；URL 查询参数一律拒绝，只有使用一次性
  token 的 `GET /api/files/download` 例外（[daemon 手册](../operations/daemon.md#http-与浏览器认证)）。
- 浏览器以 Bearer 和已允许的 Origin 调用 `POST /v1/auth/ws-ticket`，得到 30 秒有效、绑定 Origin、
  单次使用的票据，再以 `ait.ticket.<ticket>` 子协议升级 `/v1/ws`；长期 Bearer 不进入 URL 或子协议。
  额外页面来源只接受显式配置的规范 HTTP loopback origin
  （[ADR-049](../decisions/clients/adr-049-app-rust-browser-transport.md)）。
- `x-ait-guest` 访客范围只与 daemon 自身 Bearer 同时生效；以浏览器票据升级时携带该头返回 403。
  访客的分享停止准入时，其连接立即结束（[ADR-208](../decisions/daemon/adr-208-workspace-share-guests.md)）。
- WebSocket 单条消息与单帧上限均为 `MAX_MESSAGE_BYTES`（1 MiB），超出部分只能走分块协议；
  实际预算在 `server_info.limits` 公布（[ADR-098](../decisions/clients/adr-098-acknowledged-client-chunks.md)）。

## 准入与分发

- 每个业务连接在升级前取得准入：draining 时返回 503，同时连接数达到 `MAX_CONNECTIONS`（64）时
  返回 429。`/readyz` 在 draining 时同样返回 503（[daemon 手册](../operations/daemon.md#http-与浏览器认证)）。
- 校验通过的请求放入 `Some(Context)`，依次交给 API 跨能力入口和各 crate 的
  `&mut Option<Context>` 入口。只有 `DispatchError::NotImplemented` 进入下一层，且该层必须保留
  Context（`Context::assert_unhandled` 在 release 构建中同样断言）；已匹配的业务错误只响应一次，
  投递失败立即传播；全部入口均未匹配时返回 `not_implemented`
  （[ADR-093](../decisions/daemon/adr-093-consumable-request-context.md)，修订
  [ADR-036](../decisions/daemon/adr-036-daemon-crate-dispatch.md)、
  [ADR-037](../decisions/daemon/adr-037-daemon-model-context.md)）。
- hello 的 `required_capabilities` 含 `connection.single.v1` 时，一个 socket 承载四个有界 worker
  （默认、终端与语音、文件与长耗时操作、Provider）。worker 按所属 crate 的方法声明选择，只负责
  并发与订阅隔离，worker 内仍走同一 Context 处理链（[ADR-093](../decisions/daemon/adr-093-consumable-request-context.md)）。
- 单连接每个 worker 队列容量为 16，组内保持顺序。RPC 超出队列时返回可重试的
  `resource_exhausted`，不自动重放；事件或二进制帧无法入队时结束连接；`connection.ping` 在路由层
  直接回答，不等待业务 worker（[ADR-120](../decisions/daemon/adr-120-single-connection-startup-admission.md)）。
- 已接纳 RPC 的 `Context.call` 排队等待共享短任务额度（3 个）；直接 `Runtime.run` 在额度用尽时拒绝。
  等待可被停机取消，已开始的阻塞任务在客户端断线后仍被追踪完成
  （[ADR-134](../decisions/daemon/adr-134-admitted-rpc-job-waiting.md)）。

## 分块与背压

- 每条连接的出站队列最多 256 条、4 MiB（含正在写出的帧），入队失败即结束该连接；单连接模式的
  四条 lane 公平轮转并共享同一预算。WebSocket 写出超过 5 秒结束连接；进程内 Hub 会话无写超时，
  依靠会话 credit 与有界队列背压（[ADR-120](../decisions/daemon/adr-120-single-connection-startup-admission.md)、
  [ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)）。
- daemon 以 feature `client-message-chunks-v1` 宣告分块。超过 1 MiB 的 JSON envelope 以 `0x30`
  二进制帧发送，每块最多 256 KiB，每条连接同时只组装一条消息；收到 `connection.chunk.ack`
  后才发下一块。完整消息最多 64 MiB，60 秒绝对期限，全 daemon 组装预算 256 MiB，断线不重放
  （[ADR-098](../decisions/clients/adr-098-acknowledged-client-chunks.md)）。
- 文件上传帧使用 `0x40`–`0x42`，daemon 处理后返回 `connection.upload.ack {requestId, opcode}`；
  旧 opcode `0x10`–`0x12` 仍被接受但不确认（[ADR-098](../decisions/clients/adr-098-acknowledged-client-chunks.md)）。

## 在线连接（Hub）

- 在线业务只经 Hub access → 中心 → Hub serve → daemon，没有传统 Relay、Yamux 或其他回退。
  客户端最多等待 Hub 就绪 5 秒；链路不可用、中心不匹配或目标无 serve 节点时明确失败，由既有重连处理
  （[ADR-204](../decisions/clients/adr-204-hub-only-online-business-transport.md)，修订
  [ADR-074](../decisions/clients/adr-074-account-host-relay.md)、
  [ADR-180](../decisions/daemon/adr-180-hub-serve-endpoint.md)）。
- 中心开启 Online Service 会员强制后，Hub access 的票据签发、续传和已建立链路的周期复核都校验会员；
  Hub serve、控制链路、在线发现、登录与账单接口照常可用，本地、TCP、SSH 连接不受限。会员期限只由中心
  按已核实的商店付款累计，App 提交交易凭据，daemon 不持有商店密钥或期限
  （[ADR-236](../decisions/clients/adr-236-online-service-subscription.md)）。
- Host 端每个 Hub 会话由 API 在进程内服务，与本机 WebSocket 共用 `serve_frames`、连接准入和访客
  约束，不再拨本地 WebSocket；访客身份直接传入，不经 `x-ait-guest` 请求头
  （[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)，修订
  [ADR-180](../decisions/daemon/adr-180-hub-serve-endpoint.md)、
  [ADR-208](../decisions/daemon/adr-208-workspace-share-guests.md)）。
- daemon 的 serve 链路始终声明 `lease`、`credit`、`download`，能取得 bulk 票据时再声明 `bulk`；
  中心未声明或未启用其中任一项时链路失败，不降级。客户端要求中心 `hub_features` 含全部六项，
  `welcome.f` 不完整也拒绝（[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)）。
- Hub 会话独立于物理链路：断线后用 `resume_token` 在中心宽限期内续传，未确认重放缓冲上限 8 MiB，
  缓冲满时暂停读取本地会话；续连按 250 ms 至 8 s 退避。收到中心 `replaced` 后让出 5 分钟再接管
  （[ADR-180](../decisions/daemon/adr-180-hub-serve-endpoint.md)、
  [ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)）。
- Hub 链路存在（在线或续传中）时由中心续租，daemon 不发 HTTP renew；链路不存在或刚轮换 JWT 时
  才 renew（[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)）。
- daemon 只以 Hub `k:"download"` 会话提供下载，在中心提示后走独立 bulk 链路；daemon 不再建立
  `/v1/relay/control` 或下载 Relay。客户端只对 `hosts[].f` 不含 `download` 的旧 Host 使用
  `/v1/hosts/{hostId}/relay-downloads`（[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)）。
- 访问端从 Hub `hosts` 帧取得 Host 列表与在线状态，不再连接 `/v1/relay/channel`
  （[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)，修订
  [ADR-154](../decisions/clients/adr-154-account-status-channel.md)）。
- `relay.status/start/stop.request` 与 `/api/relay/control` 只为兼容保留：状态恒为离线，start 被拒绝
  （托管时 `relay_managed`/409，否则 `not_implemented`/410），stop 在自主发布的 daemon 上执行完整断开
  （[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)、
  [ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)）。

## 授权与发布

- 所有启动方式组装同一个 `host-link` 发布服务。无机器授权时本地 API 照常可用，状态为
  `unauthorized`；授权写入后 daemon 自行领取凭据、注册节点、续租并维护 Hub serve
  （[ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)，修订
  [ADR-087](../decisions/clients/adr-087-headless-device-authorization.md)、
  [ADR-200](../decisions/daemon/adr-200-headless-independent-transports.md)）。
- 运行中只能经 Bearer 认证的 `PUT /api/device/authorization` 或已协商的
  `publication.authorize.request` 写入授权；相同 `request_id` 重试幂等。`mode: initial` 不替换已有
  授权，也不覆盖显式断开；客户端只交接 device code，不持有机器 refresh 凭据
  （[ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)、
  [无人值守 daemon](../operations/headless-daemon.md#运行中的授权与统一断开)）。
- `DELETE /api/device/authorization`、兼容的 Hub/Relay control `DELETE`、`publication.stop.request`
  与 `relay.stop.request` 都是完整断开：先停链路与续租，再清除内存和持久化机器凭据，只保留
  `disconnected` 标记；中心撤销最多等 5 秒，失败不阻止本地清除。重启后不自动发布
  （[ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)，修订
  [ADR-140](../decisions/daemon/adr-140-headless-local-api-token.md)、
  [ADR-180](../decisions/daemon/adr-180-hub-serve-endpoint.md)）。
- 发布状态查询（`GET /api/device/authorization`、`publication.status.request`）只返回身份、绑定和
  链路状态，不返回任何秘密（[ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)）。

## 本地入口

- 所有启动方式优先使用 `AIT_SERVER_TOKEN`（32–256 字节可见 ASCII）；未设置时原子生成并复用
  `<data-dir>/local-api.token`（0600，不覆盖已发布文件，拒绝符号链接与过宽权限）。环境覆盖不改写该文件，
  本地 token 与机器授权相互独立（[ADR-140](../decisions/daemon/adr-140-headless-local-api-token.md)，由
  [ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md) 推广到全部启动方式；
  [daemon 手册](../operations/daemon.md#启动与配置)）。
- MCP 位于同一 TCP 监听的 `/mcp/agents`，为无状态 Streamable HTTP，HTTP 上限 65 秒。接受 daemon
  token、Agent MCP 专用 token 或 Bot 作用域凭据；专用 token 只被 MCP 路由接受。`X-Ait-Agent-Id`
  只是调用上下文，权限边界是作用域凭据（[ADR-148](../decisions/daemon/adr-148-daemon-mcp.md)、
  [ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)、
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)、[MCP 手册](../operations/mcp.md)）。
- 可选 MCP Unix socket 只暴露 `/mcp/agents`，HTTP authority 固定为 `localhost`，鉴权与 TCP 的 MCP
  路由相同。父目录须为当前用户所有的 0700 非符号链接目录，socket 为 0600；只回收无监听者的旧
  socket，停止时只删除本次绑定的 device/inode
  （[ADR-203](../decisions/daemon/adr-203-mcp-unix-socket.md)、[MCP 手册](../operations/mcp.md#unix-socket-监听)）。

## 客户端 SDK 归属

- `@ait/client` 原生拥有 Rust v1 握手、能力目录、请求关联、订阅物理归属、错误投影、消息分块和
  上传确认；`transportFactory` 只收发原始帧。`apps/mobile/src/runtime/daemon-transport` 只选择
  Electron IPC、原生 WebSocket、浏览器票据或账号 Hub，不解析业务协议。业务写入不因断线自动重放
  （[ADR-211](../decisions/clients/adr-211-sdk-native-daemon-protocol.md)，修订
  [ADR-044](../decisions/clients/adr-044-paseo-client-rust-transport.md)、
  [ADR-094](../decisions/clients/adr-094-canonical-ait-client-methods.md)）。
- 直连、SSH 和本机 daemon 先以 `connection.single.v1` 建立单连接，daemon 拒绝时回退四连接并按端点
  记住；Hub 会话始终为单连接（[ADR-218](../decisions/clients/adr-218-hub-feature-negotiation.md)，
  修订 [ADR-211](../decisions/clients/adr-211-sdk-native-daemon-protocol.md) 的“直连与 SSH 保留四通道”）。
