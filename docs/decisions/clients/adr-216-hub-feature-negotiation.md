# ADR-216：Hub feature 协商收敛在线连接

- 状态：已实现
- 日期：2026-11-06
- 修订：[ADR-204](adr-204-hub-only-online-business-transport.md)、[ADR-180](../daemon/adr-180-hub-serve-endpoint.md)

`ait-hub-v1` 保持帧格式和版本号，新增五个由中心声明、双方协商的 feature：`lease`、`hosts`、`state`、`credit`、`download`。中心在 `register` 和 `hub-tickets` 响应中返回 `hub_features`；客户端只在收到该字段后在 `hello.f` 中带上交集，`welcome.f` 为最终结果，resume 必须保持相同集合。中心没有声明时，daemon 和客户端的行为与本 ADR 之前完全一致。

- `lease`：Hub link 在线或处于宽限期时由中心续租，daemon 和客户端停止 HTTP renew。设备授权的 daemon 每次轮换 JWT 后仍发一次 renew，使授权期限前移。
- `hosts`：访问端从 `hosts` 帧（全量 + 按 `ver` 连续的增量）取得 Host 列表与在线状态，不再连接 `/v1/relay/channel`；`select()` 使用同一列表。
- `state`：可见性与焦点改为 link 级 `state` 帧，只在变化和重连后发送，不再调用 `session.heartbeat`；桌面主进程合并多个窗口。
- `credit`：发起方和 serve 端都声明时，会话按窗口流控，发送方等待 credit 而不是因缓冲满失败；未启用时保留原有行为。
- `download`：目标 Host 的 `hosts.f` 含 `download` 时，下载作为 `k:"download"` Hub 会话运行，消息沿用原下载协议，token 仍由 daemon RPC 签发；否则继续使用独立下载 Relay。

中心同时声明 `lease` 和 `download` 时，daemon 不再建立 `/v1/relay/control`；中心负责让旧访问端照常看到该 Host 在线，并把旧下载请求桥接为 Hub 下载会话。中心未声明时 daemon 保留 relay control。

Host 端 Hub 会话改为进程内接入 API 连接处理，不再为每个会话拨本地 WebSocket；访客身份与准入规则不变。直连、SSH 和本机 daemon 连接优先使用 `connection.single.v1` 单连接，daemon 拒绝时回退四连接。桌面连接自带 daemon 只走本机直连，不为本机导入 accountRelay；移动端进入后台后只断开物理连接并保留 resume token。

兼容范围：新旧中心、daemon（含 headless 运行）与客户端任意组合都能连接和工作；只支持旧 Relay、早于 Hub 的 Host 仍按 ADR-204 不可用。

## Test coverage

未测量：用户未请求覆盖率。定向验证无 `hub_features` 时的原有行为、feature 协商与 resume、lease 下停止 renew 与 JWT 轮换后的 renew、跳过 relay control、credit 双向背压与重连补发、Hub 下载与取消、访客下载、进程内会话（所有者与访客）、`hosts` 增量与版本缺口、`state` 去重、单连接回退和桌面不为本机建立 relay；测试执行结果与覆盖率测量分开报告。
