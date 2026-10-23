# ADR-185：内置浏览器按 Workspace daemon 选择出站位置

- 状态：已实现
- 日期：2026-10-23
- 关系：修订 [ADR-152](adr-152-remote-workspace-browser-loopback.md) 的回环目标限制及仅在线中继接入范围。

## 背景

远端 Workspace 的浏览器原先仅将回环地址转发给 daemon，公网和局域网请求仍从客户端
发出。同一个页面因此可能使用两台机器的网络，无法一致访问远端的服务及网络资源。

## 决策

- 以 Workspace 的 `serverId` 与桌面主进程持有的本地 daemon 身份比较。本地 Workspace
  使用直连 profile；其他 Workspace 的全部 HTTP、HTTPS、WebSocket 目标通过对应 daemon
  出站，不按目标地址分类，也不在远端连接失败时回退到本机。
- 远端 profile 使用固定 SOCKS5 代理及唯一 bypass 规则 `<-loopback>`，取消 Chromium
  对回环和链路本地地址的隐式直连。IPv4、IPv6 和域名原样经隧道传递，DNS 在 daemon
  执行；`localhost` 和 `*.localhost` 仍映射为 daemon 的 IPv4 回环。
- 在线连接复用账户 transport 的 Hub/relay 身份校验；TCP/SSH 连接复用现有主进程
  transport 的 Bearer 认证和 SSH 生命周期。两者都校验目标 `server_id`，浏览器流量不
  广播到应用 renderer。首次导航和后台标签页创建均等待 profile 配置完成。
- 远端持久 profile 按 daemon 和连接目标隔离，多窗口共享同一目标的网络，最后一个
  窗口释放代理。凭据不写入 profile 标识或网页；本地 profile 明确使用 `direct`。
- `browser` crate 继续拥有 TCP 隧道。放开目标范围并在连接期限内解析 DNS，保留
  16 条流、64 KiB 分片、读写期限、半关闭及 subscription/连接释放取消机制。

## 后果

需要更新桌面客户端和远端 daemon；旧 daemon 仍会拒绝非回环目标。原始 URL 与
Chromium 的 TLS 校验保持不变。浏览器使用的是 daemon 所在机器的 DNS 和 TCP 网络，
不会读取该机器上浏览器的代理设置。domain 和持久化模型不变。

## Test coverage

未测量：用户未请求覆盖率。定向测试覆盖地址解码、daemon 身份、本地/远端 profile、
TCP/SSH 认证复用、真实 Rust TCP 转发及连接释放。Electron 网络 E2E 覆盖各地址类别的
HTTP(S)/WebSocket 路由、200 KB 响应和 relay 重连。如需测量，仅针对这些受影响模块。
