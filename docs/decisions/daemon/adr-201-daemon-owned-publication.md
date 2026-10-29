# ADR-201：daemon 统一拥有节点发布与机器授权

- 状态：已接受
- 日期：2026-10-29
- 修订：[ADR-087](../clients/adr-087-headless-device-authorization.md)、[ADR-140](adr-140-headless-local-api-token.md)、[ADR-180](adr-180-hub-serve-endpoint.md)、[ADR-200](adr-200-headless-independent-transports.md)

## 背景

原先普通启动由 Desktop 的账号管理器注册节点、续租并取得 Hub/Relay 票据，`run --headless`
才启用 daemon 的机器授权和维护循环。Desktop 退出、账号生命周期或客户端异常会影响节点发布，
即使 daemon 进程仍在运行。两种启动方式也使用不同的本地 API token 生成规则。

## 决策

移除 `--headless`。直接启动、`daemon run` 和 Desktop 启动的 daemon 组装同一个发布服务。
没有机器授权时保持本地 API 可用，状态为 `unauthorized`；授权写入后自行领取/轮换机器凭据、
注册节点、续租及维护 Hub serve 和下载 Relay。授权失效时停止远程链路，保留本地控制以便重新授权。
[ADR-200](adr-200-headless-independent-transports.md) 的独立续租与链路重试继续生效。

机器授权写入只有两个入口，共用独占凭据存储与原子提交：

- 停止同目录 daemon 后执行 `daemon login`，可恢复原授权或开始新的 Web/enrollment 授权。
- 运行中使用经过 daemon Bearer 认证的 `PUT /api/device/authorization`，或经过会话认证和能力协商的
  `publication.authorize.request`。接受 enrollment、已批准的 device code、或明确交接的机器 refresh
  credential。先保存请求再消费一次性凭据；调用方用相同 `request_id` 重试，重复写入不会回滚已轮换的凭据。

Desktop/原生移动客户端只发起并批准中心 device authorization，再把待领取的 device code 交给 daemon。
账号 JWT 留在客户端；长期机器 refresh 凭据由 daemon 领取并保存。客户端不再为 Host 注册、续租或取票。
客户端自身的 access 节点注册、续租、Hub access 和账号 Channel 仍由账号管理器负责。
退出账号或 Desktop 不撤销、暂停 daemon 的发布；中心显式撤销机器授权仍会使它下线。

Hub 与 Relay 始终使用同一份机器授权（grant/binding、refresh token、access JWT）和同一节点会话。
登录只产生一份共同授权，daemon 为两种协议分别领取一次性票据；两种票据不能互换，也不能独立
写入或替换某一条链路的身份。

`DELETE /api/device/authorization`、兼容的 `DELETE /api/hub/control`、`DELETE /api/relay/control`、
`publication.stop.request` 和
`relay.stop.request` 都表示完整断开 ait-server。发布服务先取消并收拢续租、刷新及取票任务，
关闭 Hub、Relay 和数据通道，尝试关闭节点会话，然后清除内存及持久化 snapshot 中的机器授权、
refresh 凭据、待领取 enrollment/device code、待恢复刷新请求和交接 ID。已取得的机器凭据用于
一次有界的中心撤销；网络故障不会阻止本地清除，也不为撤销重试保留凭据。

只保留机器身份和不含秘密的 `disconnected` 标记，重启仍不发布；Desktop 自动同步也不得重新授权。
`daemon login` 或显式 API 授权清除该标记，重新建立两条链路。设置页断开同时记录自动授权偏好。
控制面只保留授权和断开，不再单独 start/resume，也不允许外部写入 Hub/Relay 票据。
自动初始化授权使用 `mode: initial`，由 daemon 在处理命令时原子判断：已有授权或显式断开均不接受自动替换。
显式登录默认 `mode: explicit`，持久化成功后立即启动发布，不需要额外连接命令。
磁盘写入失败返回错误，进程内仍保持断开，调用方需修复磁盘后重试清除。
`publication.status.request` 和 `GET /api/device/authorization` 只返回身份、绑定及链路状态，不返回秘密。

所有启动方式优先使用 `AIT_SERVER_TOKEN`；未配置时由 daemon 原子生成并复用
`<data-dir>/local-api.token`（0600）。环境覆盖不改写该文件。Desktop 不再生成每次启动的临时 token，
在 daemon ready 后读取保存的 token，或使用其继承的显式环境覆盖。机器授权与本地 API token 相互独立。

短暂故障按指数退避降低请求频率：注册/续租与 refresh、Relay 取票从 1 秒增长到 30 秒；
Hub 取票及首次授权领取从 5 秒增长到 60 秒。异步连接任务启动不算恢复，只有实际握手在线才重置
链路退避；已连接时每秒仅检查本地状态，不重复取票。Hub 使用 resume token 的物理续连继续采用
250、500、1000、2000、4000、8000 毫秒的独立退避，并受中心续传宽限期约束。
正常租约续期、心跳和中心指定的 device code 轮询间隔不被错误退避延迟。
机器授权领取的网络失败在 daemon 中持续重试；CLI 登录单次等待最多十分钟，超时保留原请求供下次恢复。
短期 access JWT 返回 401 时先关闭链路并用 refresh 凭据探测一次；新 JWT 仍被拒绝或 refresh 被拒绝才要求重新登录。
正常到期刷新重新开放该恢复机会，持续网络故障沿用退避。轮询已过期的 device code 仍使用原请求 ID 向中心
恢复已消费请求的回执；由中心判断是否过期。完整停止 Hub 时清除旧连接的 replaced/yield 等待，防止新登录被旧状态抑制。

## 原因与取舍

发布服务生命周期与 daemon 进程一致，消除外部 GUI 的租约职责和重试依赖。一次性 device code 交接
复用中心现有接口，避免在客户端长期保存机器秘密。外部断开只有一个语义，避免 Hub 已断开但 Relay
仍在线或旧授权在重启后重新发布。daemon 本地 API 与任务继续可用。

旧 `--headless` 启动脚本需要删掉参数。新客户端要求支持自主发布的 daemon 才能操作发布；旧 managed
状态继续只读兼容。已有机器 snapshot 无需迁移，可选 `authorization_id` 用于交接去重，缺省为 false 的 `disconnected` 标记记录显式退出。
客户端旧账号会话不会被复制成机器凭据，升级后首次自动同步需要中心 device authorization 已部署并启用。
