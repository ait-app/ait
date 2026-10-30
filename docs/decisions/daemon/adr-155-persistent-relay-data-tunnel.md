# ADR-155：可协商的持久 Relay 数据隧道

> 普通业务传输已由 [ADR-204](../clients/adr-204-hub-only-online-business-transport.md) 修订：仅使用 Hub，传统 Relay 仅保留独立下载与控制。

状态：采用。中心仓库为 `cnb.cool/ait-app/ait-server`；中心默认关闭，双方协商后启用。

## 连接与兼容边界

Host 指运行 Rust daemon 的机器：包括桌面端内置 daemon 和 headless daemon。
访问方可以是 Android、iOS、desktop 或浏览器。只有 Host 与中心理解 Yamux，访问方不变。

```text
Android / iOS / desktop / browser
    └─ 原 client_ticket + /relay/sessions/{id}/client WSS
        └─ 中心 ── 一个逻辑 stream / visit ── Host ── 固定本地 daemon WebSocket
                    ╰─ 一条持久数据 WSS + Yamux ─╯
中心 ←──────── 原控制 WSS（票据、在线状态、旧 open_data）────────→ Host
```

每个在线 Host 最多一条 ready 数据 WSS，替换期间允许一个候选连接。保留原控制 WSS。
桌面作为访问方时沿用客户端协议；桌面提供本机 daemon 时和 headless 共用 Rust relay 实现。
不新增原生平台依赖、公开 RPC 或外部 Yamux 二进制。

| 中心                         | Host      | 行为                                                       |
| ---------------------------- | --------- | ---------------------------------------------------------- |
| 旧中心                       | 新 Host   | welcome 无能力声明，不请求隧道；原独立数据 WSS             |
| 新中心，功能关闭             | 任意 Host | welcome 与原协议一致；原独立数据 WSS                       |
| 新中心，功能开启             | 旧 Host   | 忽略额外 welcome 字段；无 ready 隧道，原独立数据 WSS       |
| 新中心，隧道 ready、路由关闭 | 新 Host   | 只预热；访问仍走独立 WSS                                   |
| 新中心，隧道 ready、路由开启 | 新 Host   | 普通访问走 Yamux；下载继续独立 WSS                         |
| 新中心，隧道不可用           | 新 Host   | 新访问回退原独立 WSS；已分配的访问失败，由客户端发起新尝试 |

访问方的 HTTP 请求、票据、Origin 规则、`relay.ready`、业务 hello、文本和二进制消息均保持不变。
不重放已发送的写操作，不把已创建的 Mux visit 偷换到另一条连接。

## 协商与授权

1. Host 原样发送 `control.hello` version 1，不添加新字段（兼容旧中心的严格解析）。
2. 中心仅在 `RELAY_TUNNEL_ENABLED=true` 时，在 welcome 添加
   `supported_tunnel_protocols: ["ait-relay-tunnel-v1"]`；关闭时省略该字段。
3. Host 只在收到这一能力后发送
   `{"type":"tunnel.request","request_id":"UUID","protocol":"ait-relay-tunnel-v1","epoch":"UUID"}`。
4. 中心返回 `tunnel.grant`，包含上述 request/protocol/epoch、`tunnel_id`、
   `tunnel_ticket` 和 `expires_in_seconds:30`；拒绝返回 `tunnel.error`。
5. Host 用 Bearer 票据连接 `GET /v1/relay/tunnel`，不得通过 query 或 subprotocol 传票据。
   票据一次性，绑定 owner、Host、node session、控制 epoch 和 tunnel ID。
6. 第一条 Text 为 `{"protocol":"ait-relay-tunnel-v1","epoch":"UUID","tunnel_id":"UUID"}`。
   中心校验并回显；双方随后仅通过 Binary 消息承载连续 Yamux 字节。

中心在发票、升级、welcome 后发布路由、每次 visit START 前检查相应身份与代际。
数据库查询在 registry 锁外执行，返回后重新检查路由。新票据仅作废旧候选，不中断 ready 连接；
新连接通过握手后才替换旧连接，旧任务清理带 tunnel ID，不能误删新路由。
控制连接失效、注销或授权失效会清理相关票据、隧道与访问。

Yamux 双端固定为 Rust `yamux = 0.14.1`。中心为 Client（发起 stream），Host 为 Server；
中心拒绝 Host 主动发起的 stream。WS 消息边界不是 Yamux frame 或业务消息边界。

## 每次访问

中心照旧鉴权并发 client_ticket，同时打开一个 stream。业务记录为：

```text
u8 kind | u32 big-endian payload_length | payload
```

| kind   |  值 | payload 与上限                  |
| ------ | --: | ------------------------------- |
| OPEN   |   1 | UTF-8 JSON，最多 2048 B         |
| ACCEPT |   2 | 空                              |
| START  |   3 | 空                              |
| TEXT   |   4 | 有效 UTF-8，最多 1 MiB          |
| BINARY |   5 | 最多 1 MiB                      |
| CLOSE  |   6 | 空；不透传 WS close reason/code |

OPEN 仅包含 `relay_session_id`、`source_node_session_id`、`epoch`、`tunnel_id` 和
`mode:"ait-rust-single-v1"`，不允许远端指定本地 URL、端口或凭据。未知字段／记录类型拒绝。
Host 校验后发送 ACCEPT；中心等客户端接入且再次鉴权成功，发送 START 与原 `relay.ready`。
Host 收到原始业务 hello 后才建立本地连接，避免提前耗尽本地 hello 超时。hello 原字节保留。

记录头先校验长度再分配；读取正文持续消耗流窗口，因此 1 MiB 记录能跨越初始 256 KiB 窗口。
读记录的 future 跨 hop-local Ping/Pong 保留，避免取消 `read_exact` 造成截断和串帧。
单个 visit 结束只关闭自己的 stream；Mux 取消不通过独立控制 WSS 发送 `cancel_session`。

## 资源与故障

- 每 Host 最多 16 个访问，独立路径与 Mux 共享 Host 侧许可；每个 Yamux connection 最多 16 streams。
- Yamux receive window 总量 4 MiB，发送按 16 KiB 分段；WS Binary 上限 64 KiB，双向 byte adapter 各 64 KiB。
- 流驱动持续轮询，业务慢流不会阻止连接驱动处理其他 stream/window update。队列有界，不全量缓存大消息。
- 全局按 MiB 预留：每条隧道 16 MiB（包含候选）、每个 Mux visit 4 MiB，访问 task 完全退出后释放。
  这是协议缓冲的准入额度，不是进程 RSS 上限；旧独立路径仍用原配额，TLS/分配器/数据库等开销另计。
- 满额返回 `relay_capacity`，不挤掉既有访问。默认 256 MiB；实际部署按在线 Host、候选替换和并发访问调整。
- 数据 WSS 每 10 秒 Ping，30 秒未收到匹配 Pong 关闭；写操作和升级/OPEN 有 5 秒界限。
  Host 等 START 最多 30 秒，等业务 hello 最多 10 秒。客户端 WSS 保留 20s/60s 心跳。
- 数据隧道失败不改变控制在线状态。重建从 500ms 指数退避到 10s，加 0–255ms jitter；稳定 60s 后复位。
  中心同 Host 发票限速 500ms。隧道重试不延长控制连接 65s 读空闲期限。
- 单 WSS 仍共享 TCP：丢包会产生跨流队头阻塞。Yamux 隔离流窗口，不承诺消除 TCP 队头阻塞。
- 一条 WSS 断开会结束其所有 visit；仅新 visit 可选择恢复后的隧道或旧路径，不恢复旧 stream/写请求。

暂不实现多数据 WSS、无损 stream 迁移或 QUIC。以后通过新能力版本协商扩展，不改变 v1 记录解释。

## 启用与回滚

环境变量已接入 Compose，默认均不切流：

```dotenv
RELAY_TUNNEL_ENABLED=false
RELAY_TUNNEL_ROUTE_VISITS=false
RELAY_TUNNEL_MEMORY_MIB=256
```

`ROUTE_VISITS=true` 要求 `ENABLED=true`；预算范围 20–65536 MiB。修改后需重启服务生效。

1. 先部署中心，保持默认关闭；升级桌面/headless Host 的 AIT daemon。旧访问方无需同步升级。
2. `ENABLED=true, ROUTE_VISITS=false` 预热，检查 `Relay data tunnel ready` 日志、资源和重建情况。
3. 小范围环境开启 `ROUTE_VISITS=true`，检查新访问、下载、后台恢复、断网切换和重连耗时分位数。
4. 回滚设置 `ROUTE_VISITS=false`，必要时关闭 ENABLED。服务重启会断开现有连接，客户端重新连接。
   不需要数据库迁移；回滚旧二进制继续使用原独立数据路径。

现有 CDN/nginx WSS 入口继续使用 `/api/v1/relay/tunnel`（中心内部为 `/v1/relay/tunnel`），
无需裸 TCP/UDP；反代应保留 Upgrade 与 Bearer 头。默认 nginx 的 `/api/` 转发已覆盖此路径。
路由状态仍在单进程内，多副本部署需要保持控制、隧道和访问落在同一个实例。

持久隧道消除了每次访问时 Host→中心的 DNS/TCP/TLS/WS 建连步骤；访问方的 API、WSS 与本地 daemon
握手仍存在。本次没有公网/CDN 或真机基准，不能据此承诺具体重连秒数。
