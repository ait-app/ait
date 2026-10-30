# ADR-180：消息 Hub 服务端点（ait-hub-v1）

> 普通业务传输已由 [ADR-204](../clients/adr-204-hub-only-online-business-transport.md) 修订：仅使用 Hub，传统 Relay 仅保留独立下载与控制。

> 启动与控制边界已由 [ADR-201](adr-201-daemon-owned-publication.md) 修订：所有 daemon 自主发布，外部可暂停 Hub。

状态：采用（原型）。中心协议见中心仓库 `docs/hub-protocol.md`；本 ADR 记录 Host 与客户端侧边界。

Headless 的发布调度与端口默认行为由 [ADR-200](adr-200-headless-independent-transports.md) 修订：两条传输独立维护，`ManagedRelay` 必须显式实现 Hub 方法。

## 背景

旧 relay 每次访问都要新建会话：HTTP 申请、客户端 WSS、配对、业务 hello、重新订阅。daemon 的订阅归属单条连接（`ConnectionSubscriptions`），连接断开即释放，因此手机断网恢复后只能全量重来。

中心新增按账号隔离的消息 Hub：每个进程一条常驻 WSS，访问端（`access`）向服务端（`serve`）开会话，链路断线后 120 秒内按序号续传。

## 决定

- Hub 是**新增的独立路径**。旧控制 WSS、`open_data`、持久数据隧道和客户端旧传输均不改变；中心或 Host 不支持 Hub 时，访问继续走旧路径。
- Host 侧实现为 `crates/relay/src/hub/`，由 `relay::Connector::hub()` 提供，与旧 relay 共用本地 daemon 目标、TLS 配置和关停信号。桌面内置 daemon 与 headless daemon 共用这一实现。
- 每个 Hub 会话在本机 daemon 上建立一条独立 WebSocket，业务 hello、server_info、Text/Binary 消息原样转发；daemon 业务代码不感知 Hub。
- **会话生命周期与物理连接解耦**：Hub 链路断线重连期间，本地 WebSocket 和其上的订阅保持；只有收到 `close`、链路失效或连接器停止时才关闭。访问端在宽限期内重连，可直接收到断线期间的增量。
- 链路状态（`hub/link.rs`）维护双向序号、累积确认和最多 8 MiB 的未确认重放缓冲；缓冲满时暂停读取本地会话，形成背压。
- 断线后用首次 welcome 返回的 `resume_token` 重连，无需新票据；中心以 401/403 拒绝或返回 `resume_expired` 时停止，等待新的授权。
- 中心以 `error{code}` 结束链路时（如 `replaced`、`unauthorized`），状态的 `rejected` 字段记录该错误码。收到 `replaced` 说明另一个进程接管了本节点的 serve 链路：headless 的 `hub_active()` 与桌面端 `maintainHub` 都会让出 5 分钟再尝试，避免两个进程互相替换。桌面端对同一次拒绝只等待一次；重新下发授权后，新的替换事件才开始下一次等待。

## 授权入口

| 模式     | 入口                                                                                  | 票据来源                                                                  |
| -------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| 桌面管理 | 本地 `GET/PUT/DELETE /api/hub/control`，与 `/api/relay/control` 平行，使用本地 bearer | 桌面账户会话申请 `POST /v1/node-sessions/{id}/hub-tickets {"ep":"serve"}` |
| Headless | `host-link` 控制器在维持租约时可选启动；失败 60 秒后重试，从不影响旧 relay 发布       | 机器凭据申请同一接口                                                      |

托管（headless）模式下，本地接口对 `PUT`/`DELETE` 返回 409，状态查询可用。`ManagedRelay` 与 `Center` 端口新增的 Hub 方法均有默认实现：不支持 Hub 的实现视为“无需 Hub”。

## 访问端（客户端）

- `@ait/client/internal/hub`（`packages/client/src/hub/`）提供 `HubLink`（每个已登录账号一条，序号、确认、续传、`verify()` 探活）和实现 `DaemonTransport` 的 `HubSession`。
- 访问链路每 20 秒发送一次应用层 ping，3 秒内没有 pong 就关闭物理连接并续传；前台与周期性探活共用同一个在途 ping。停止、重连和链路替换会清理原连接的探活计时器。
- `DaemonTransport` 可选提供 `recoverLiveness()`：业务探活超时后，传输层可在原链路宽限期内恢复同一会话。SDK 在恢复后重试一次业务 ping，再次超时仍按原规则处理；原始 pong 若已随续传到达，则直接完成探活。普通传输不提供恢复能力时保持原来的超时行为。
- Hub 恢复等待随原链路续传、过期、拒绝或停止而结束，不能转移到新链路；Rust 协议适配层与移动端延迟路径选择会转交该能力。这样前台三秒探活和正常业务心跳不会提前拆掉仍可续传的会话。
- Android/iOS 在 `account-transport.ts` 的原生分支选择路径：链路就绪且 presence 中有目标 `host_id` 的 serve 节点时走 Hub，否则使用未改动的旧原生传输；每次选择记录 `hub`、`legacy:no_serve` 或 `legacy:hub_down`。会话建立后不切换路径。
- 桌面账户会话通过 `localHub` 依赖维护内置 daemon 的 serve 链路；Electron 与 Web 的访问端暂时只走旧路径。

## 验证

- `relay` crate：帧编解码、链路序号与重放、会话桥接、模拟中心下的断线续传和拒绝。
- `api`：本地 Hub 控制接口的鉴权、启动、停止和托管冲突。
- `host-link`：Hub 与 relay 并行启动、失败退避。
- 中心仓库 `tests/api/hub_e2e.rs`：真实 daemon 经 Hub 提供会话，访问端断线重连后同一会话继续 RPC，无需新 hello；旧 relay 端到端同时通过。
- 跨实现检查：TS `HubLink`/`HubSession` 连接真实中心与真实 daemon，连续三次强制断开 TCP，断线期间发出的请求在续传后得到回答，会话未关闭、未重复 hello。
