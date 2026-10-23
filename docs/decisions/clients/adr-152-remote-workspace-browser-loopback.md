# ADR-152：在线中继 Workspace 的浏览器回环转发

状态：已实现。日期：2026-10-16。

目标地址限制及连接范围已由 [ADR-186](adr-186-workspace-browser-egress.md) 修订：本地
Workspace 直连，远端 Workspace 的所有目标统一经对应 daemon 出站，含 TCP/SSH 连接。

## 背景

Workspace 所在 Host 可以通过在线服务中继连接，但内置浏览器的 WebContents 始终在
Electron 客户端执行。此前直接加载 `http://127.0.0.1:4322/` 会访问客户端电脑，无法访问
远程 Workspace 的开发服务，即使该服务已在远程 Host 正常监听。

## 决策

- `browser` crate 拥有 `browser.tunnel.open/read/write.request`，只连接 daemon 所在机器的
  回环 IP；`localhost` / `*.localhost` 映射到 IPv4 回环，不执行任意 DNS 解析。端口必须为
  1–65535。它是浏览器网络适配器，不改变 domain 或新增持久化。
- 每条 TCP 流占用现有物理连接的一个 subscription 槽，最多 16 条；读写分片最多 64 KiB，
  读操作最多等待 30 秒，连接和写入最多等待 10 秒。读写独立，支持半关闭；释放 subscription、
  连接断开和 daemon 停机会取消进行中的 I/O 并关闭流。异步 I/O 不占住 RPC 分发队列。
- Electron 主进程复用 `AccountTransportManager` 的一次性 visit、目标 server/instance 校验、
  认证、输出预算和关闭流程，独立协商这三个方法。账户凭据不交给网页或 renderer。
  按需建立 visit，同一 Host 的浏览器共用一条连接；断线关闭旧 TCP 流，下次网页请求重新连接。
- 每个在线 Host 使用独立、持久的浏览器 profile。Workspace 首次导航和 Agent 创建后台标签页
  都先完成 profile 的网络配置；窗口只能附加自己已申请的 profile。多个窗口共享同一 Host 的
  路由，最后一个窗口关闭时释放代理；弹窗继承来源网页的 session，清除浏览器数据包含远程 profile。
- Profile 使用仅绑定客户端 `127.0.0.1` 随机端口的 SOCKS5 适配器。浏览器保留原始 URL，
  HTTP、HTTPS 和 WebSocket 的字节流都可以转发，TLS 校验仍由 Chromium 完成。
  只有回环目标经过中继，其他地址直接连接；远程 profile 不继承系统 HTTP 代理规则。
- 使用固定代理与有序 bypass 规则 `*;<-loopback>;169.254.0.0/16;fe80::/10`：公共和 link-local
  地址直接连接，回环地址经过 SOCKS。不能改成 PAC：Chromium 的 PAC 无法取消隐式回环绕过，
  见 [Chromium 代理说明](https://chromium.googlesource.com/chromium/src/+/3aa4be28eb12e1ef45ec44e6d69ef7777e9e2df1/net/docs/proxy.md)。

## 兼容与验证

需要同时更新客户端和远程 daemon；旧 daemon 无法协商转发方法，不能回退到客户端回环。
本次仅接入在线服务中继，直接 TCP / SSH 与本机浏览器沿用原有路由。

`crates/browser/src/tunnel/tests.rs` 验证真实回环 HTTP、半关闭、释放取消、容量、目标约束及
分片预算；桌面单元测试验证 SOCKS 与 WebSocket 升级字节保留，前端测试验证导航前等待配置。
`apps/desktop/e2e/browser-relay-network.e2e.mjs` 使用独立 Electron profile、真实 Rust daemon 和
本地配对 relay fixture，验证 Chromium 代理选择、200 KB 网页读取与断线后重新建立 visit。
