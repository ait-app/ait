# ADR-204：在线业务连接只使用 Hub

- 状态：已实现
- 日期：2026-10-30
- 修订：[ADR-074](adr-074-account-host-relay.md)、[ADR-155](../daemon/adr-155-persistent-relay-data-tunnel.md)、[ADR-180](../daemon/adr-180-hub-serve-endpoint.md)、[ADR-182](adr-182-hub-account-client.md)

桌面、Android、iOS 和 Web 的在线业务会话统一通过 Hub access → 中心 → Hub serve → daemon 本地 WebSocket。Hub 尚在连接时最多等待五秒；链路不可用、服务地址不匹配或目标没有 serve presence 时明确失败，交由既有客户端重连机制重试。保留 Hub 会话的身份校验、ACK/replay、探活恢复、窗口生命周期和有界输出，不重放业务写入。

删除客户端传统 Relay 会话申请和配对实现，以及 Host 独立业务 WSS、Yamux 持久数据隧道、协商消息和依赖。Host 不再请求中心广告的隧道能力，并拒绝 `ait-rust-single-v1` 的旧 `open_data` 业务命令。只支持旧 Relay 的中心或 Host 不再能够建立普通业务连接。

专用文件下载仍是独立功能：客户端申请 `/v1/hosts/{hostId}/relay-downloads`，经配对的独立 WSS 流式读取 daemon 的 `/api/files/download`，保留分块、完成确认和取消。`/v1/relay/control` 继续为下载调度和节点在线维护服务；`/v1/relay/channel` 继续推送账号主机状态。它们不承担业务 fallback。当前中心注册协议仍要求运行时的 `relay_modes` 为 `ait-rust-single-v1`，因此保留该注册字段；它不重新开启已删除的数据实现。

内置浏览器的 `browser.tunnel.*` 是业务 RPC，经同一 Hub 会话传输，远端 DNS 和 TCP 出口仍由目标 daemon 提供。本次只修改 Ait 仓库；中心仓库的旧入口和部署配置没有在此变更中删除。

## Test coverage

未测量：用户未请求覆盖率。定向验证 Hub 不可用和等待超时、会话续传、身份校验、终端背压、实时语音透明转发、隧道广告被忽略及独立下载；测试执行结果与覆盖率测量分开报告。需要测量时仅覆盖这些受影响模块。
