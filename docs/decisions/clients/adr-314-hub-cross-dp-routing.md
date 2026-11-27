---
id: ADR-314
status: accepted
date: 2026-11-26
amends:
  - clients/adr-182-hub-account-client.md
  - clients/adr-204-hub-only-online-business-transport.md
  - clients/adr-218-hub-feature-negotiation.md
paths:
  - packages/client/src/hub
  - packages/client/src/account-session.ts
---

# ADR-314：Hub 跨数据面路由——`redirect` 与到其它 DP 的链路

- 后续修订：[ADR-318](adr-318-official-service-entry.md)。
- 修订：[ADR-182](adr-182-hub-account-client.md)、[ADR-204](adr-204-hub-only-online-business-transport.md)、[ADR-218](adr-218-hub-feature-negotiation.md)

中心一侧见 `ait-server` 仓库的 `docs/cluster.md`（“数据面之间不共享任何东西”一节）。

## 背景

账号中心拆成控制面（CP）和多个数据面（DP），例如主站 `aitapp.cn` 与日本站 `hub.ait-app.com`。Host 与客户端各连自己区域的 DP，
DP 之间不中继会话。中心为此提供可选 Hub feature `redirect`：声明它的 access 链路在 `hosts` 中看到其它 DP 服务的 Host，
状态为在线并带 `dp`、`hub_url`、`center`；对这类 Host 的 `open` 被以 `close{code:"redirect", redirect:{node, hub_url, center, epoch}}` 拒绝。
未声明的客户端只看到这些 Host 离线，`open` 得到 `unknown_target`。客户端此前既不声明也不处理 `redirect`，所以跨 DP 的 Host 一律显示离线。

## 决策

- **协商。** `redirect` 是可选 feature，与 `peer` 一样不进入中心必须声明的六项（[ADR-218](adr-218-hub-feature-negotiation.md)）。
  票据（`register` 与 `hub-tickets` 响应）的 `hub_features` 含它时，新的 main access 链路才在 `hello.f` 中声明；
  是否生效只看 `welcome.f`。DP 关闭开关时链路照常上线，只是其它 DP 的 Host 仍显示离线。
  一条链路的 `hello.f` 在所有续传中不变（中心拒绝集合不同的续传）。bulk 链路从不声明。
- **主动路由。** 账号链路（home）保存 `hosts` 中的 `dp`、`hub_url`、`center`。对其它 DP 的 Host 打开会话时，home 链路按 `center`
  建立或复用一条到该 DP 的 main access 链路，连接 `hub_url`，会话在那里打开。不先在本 DP 打开再等拒绝：
  本 DP 上 Host 旧链路处于宽限期时，`open` 会排队等到它过期，而不是被重定向。
- **票据来自账号的 center。** 第二条链路的票据仍向账号登录的 center 申请：票据由 CP 签发，任何 DP 都能兑换；
  浏览器同源申请，不需要跨域请求。续传令牌只对签发它的 DP 有效，因此每个 DP 一条独立链路，各自续传。
- **跟随 `redirect`。** 会话在 accept 之前被拒绝时，转到 `redirect.center` 对应的链路（可能就是 home 链路），沿用同一会话 ID，
  以保存的业务 hello 重发 `open`。只在此时跟随：拒绝方只收到过 `open`，accept 之前的消息还在会话队列里，accept 之后才发出。
  目标节点必须与原 `open` 相同；`epoch` 小于已跟随过的视为过期；指回拒绝方或超过三次都以 `redirect` 关闭会话，交由既有重连处理。
  home 链路重建期间保留它的 DP 地址，供其它链路识别归来的会话。`epoch` 只在一次打开内比较，不持久化（CP 重启后从零开始）。
- **第二条链路的边界。** 它只承载到该 DP 的会话：不上报 `state`、不订阅分享、不发中心调用，事件、Host 列表、
  分享与可见性仍只来自 home 链路；中心按设备合并各 DP 的可见性，推送免打扰不受影响。
  它与 home 链路一起暂停、恢复和停止，应用回到前台时各链路独立探测；60 秒没有会话即关闭，下一个会话再建立。
  首次握手前取消会话时丢弃待发的 `open`。连不上时，等待中的会话立即失败，待发的 `open` 一并丢弃。
- **下载。** 其它 DP 的 Host 的下载会话在到该 DP 的 main 链路上打开，不经本 DP 的 bulk 链路（bulk 链路只到本 DP 的 Host）。
- **应用不变。** 路由在共享的 `HubLink` 内完成：移动端、Web 控制台和桌面主进程仍通过 `serveFor` 与 `openSession` 打开会话，
  连接配置里的 center 仍是账号 center。

## 后果

- 中心为 CP 与所有 DP 打开 `redirect` 后，新客户端可以访问任一 DP 上的 Host。旧客户端行为不变：其它 DP 的 Host 仍显示离线。
- 浏览器跨站连接其它 DP 时带着本站 Origin，该 DP 的浏览器 Origin 白名单必须包含它，例如日本站 DP 允许 `https://aitapp.cn`，主站 DP 允许 `https://hub.ait-app.com`。
- 客户端修订 [ADR-204](adr-204-hub-only-online-business-transport.md) 的“服务地址不匹配即失败”：连接配置的 center 仍须是账号 center，
  会话的物理链路可以在同一中心的其它 DP 上；修订 [ADR-182](adr-182-hub-account-client.md) 的“每个进程一条 access 链路”：
  账号链路之外，按需为其它 DP 各增加一条只承载会话的链路。
- 分享给访客的资源仍只列出访客所在 DP 的 Host（中心的已知限制）；跟随 `redirect` 让访客在 Host 迁移的竞态中仍能打开会话。
- daemon 之间的 `peer` 会话不跨 DP（中心只在本 DP 查找目标），本决策不涉及 serve 链路。
