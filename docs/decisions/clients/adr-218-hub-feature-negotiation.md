---
id: ADR-218
status: accepted
date: 2026-11-06
amends:
  - clients/adr-154-account-status-channel.md
  - clients/adr-204-hub-only-online-business-transport.md
  - daemon/adr-180-hub-serve-endpoint.md
paths:
  - packages/client
  - crates/relay
---

# ADR-218：Hub feature 协商收敛在线连接

- 修订：[ADR-154](adr-154-account-status-channel.md)、[ADR-204](adr-204-hub-only-online-business-transport.md)、[ADR-180](../daemon/adr-180-hub-serve-endpoint.md)

`ait-hub-v1` 保持帧格式和版本号，新增六个由中心声明、双方协商的 feature：`lease`、`hosts`、`state`、`credit`、`download`、`bulk`。中心在 `register` 和 `hub-tickets` 响应中返回 `hub_features`，新中心始终声明全部六项。新客户端（桌面、手机、网页和 daemon）只连接新中心：`hello.f` 固定带上自己实现的全部 feature（main link 为 `lease`、`hosts`、`state`、`credit`、`download`，bulk link 只含 `credit`、`download`），resume 保持相同集合；`register` 或 `hub-tickets` 响应缺少 `hub_features` 或其中任一 feature、或 `welcome.f` 不含所有已声明的 feature 时，客户端明确报错（提示账号服务版本过低，需要更新服务），不静默降级，也不保留对旧中心的任何回退路径。

- `lease`：Hub link 在线或处于宽限期时由中心续租，daemon 和客户端不发 HTTP renew；link 尚未建立（如首次连接前或中心暂不可达）时，访问端仍用 HTTP renew 保持 node session，因为旧 Host 的下载 Relay 依赖它。设备授权的 daemon 每次轮换 JWT 后仍发一次 renew，使授权期限前移。
- `hosts`：访问端从 `hosts` 帧（全量 + 按 `ver` 连续的增量）取得 Host 列表与在线状态，访问端不再连接 `/v1/relay/channel`，也不处理 `presence` 帧；`select()` 使用同一列表。
- `state`：可见性与焦点改为 link 级 `state` 帧，只在变化和重连后发送，不再调用 `session.heartbeat`；桌面主进程合并多个窗口。
- `credit`：访问端的 `open` 始终携带窗口 `w`；Host 的 `accept` 带 `w` 时会话按窗口流控，发送方等待 credit 而不是因缓冲满失败；旧 Host 的 `accept` 不带 `w`，该会话保持原有无流控行为。
- `download`：目标 Host 的 `hosts.f` 含 `download` 时，下载作为 `k:"download"` Hub 会话运行，消息沿用原下载协议，token 仍由 daemon RPC 签发；旧 Host（`f` 不含 `download`）继续走 `/v1/hosts/{hostId}/relay-downloads` 独立下载 Relay。
- `bulk`：下载会话改走独立的 bulk link（票据 `lane:"bulk"`，`hello.f` 只含 `credit`、`download`），不再和 RPC 共用一条 TCP 和中心写队列。访问端第一次下载时才建立，60 秒没有下载就关闭；bulk link 建立失败时下载退回 main link，60 秒内不再尝试。Host 的 main link 声明 `bulk`，收到中心的 `lane` 提示后才建立 bulk link；它由中心在空闲 60 秒后以 `idle` 关闭，Host 不自行关闭，避免中心把新下载派给正在关闭的连接；中心最多等 5 秒，超时就在 main link 上投递。

新 daemon 不再建立 `/v1/relay/control`；中心负责让旧访问端照常看到该 Host 在线，并把旧下载请求桥接为 Hub 下载会话。

Host 端 Hub 会话改为进程内接入 API 连接处理，不再为每个会话拨本地 WebSocket；访客身份与准入规则不变。直连、SSH 和本机 daemon 连接优先使用 `connection.single.v1` 单连接，daemon 拒绝时回退四连接。桌面连接自带 daemon 只走本机直连，不为本机导入 accountRelay；移动端进入后台后只断开物理连接并保留 resume token。

## 兼容范围

- 新客户端要求新中心：连接旧中心（不声明全部 `hub_features`）会明确失败，不提供降级。
- 中心保持对旧客户端和旧 daemon 的全部兼容：旧访问端仍可用 `/v1/relay/channel`、`session.heartbeat`、`presence` 帧和旧下载路由，旧 Host 仍可用 relay control；这些兼容只存在于中心。
- 新客户端保持对旧 Host（如 aitd 0.1.30）的兼容，按 Host 逐个判断：`hosts[].f` 不含 `download` 时走旧下载路由；Host 的 `accept` 不带 `w` 时该会话不做流控；RPC 会话不带 `k`。
- 本机 daemon 直连的 `connection.single.v1` 回退与中心无关，保持不变。

只支持旧 Relay、早于 Hub 的 Host 仍按 ADR-204 不可用。

## Test coverage

未测量：用户未请求覆盖率。定向验证中心响应缺少 `hub_features` 或其中任一 feature 时明确失败、`welcome.f` 不完整时拒绝链路、feature 协商与 resume、lease 下停止 renew 与 JWT 轮换后的 renew、跳过 relay control、credit 双向背压与重连补发、旧 Host 的 accept 不带窗口与旧下载路由、Hub 下载与取消、访客下载、进程内会话（所有者与访客）、`hosts` 增量与版本缺口、`state` 去重、单连接回退和桌面不为本机建立 relay；测试执行结果与覆盖率测量分开报告。
