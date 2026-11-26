---
id: ADR-318
status: accepted
date: 2026-11-26
amends:
  - clients/adr-314-hub-cross-dp-routing.md
paths:
  - packages/client/src/account-session.ts
  - apps/desktop/src/daemon/account-ipc.ts
  - apps/mobile/src/runtime/account-state.ts
  - apps/mobile/src/runtime/host-runtime.ts
  - apps/mobile/src/types/host-connection.ts
---

# ADR-318：官方服务的入口推荐与账号身份

## 背景

大陆和海外节点使用不同域名，账号、设备和 Host 由同一个控制面管理。客户端按域名保存入口和 Host 绑定，
会导致切换入口后重复导入已保存的 Host，也无法在新入口复用本机的授权流程。

## 决策

- 由服务端根据请求源 IP 提供地区推荐。桌面端与移动端持有两份官方 API 地址，并接受用户固定入口。
  原生端按所选入口发起登录，并在同一入口完成授权码兑换。推荐失效时只在打开浏览器前尝试备用官方入口。
- 已保存的官方 Host 绑定沿用大陆地址作为稳定服务标识，网络连接使用实际账号入口或 Hub 公布的 Host 位置。
  自建服务继续以规范化 API 地址标识。官方白名单同时用于复用设备授权和已有客户端会话。
- Web 登录保持站点同源。与 ADR-314 一致，工作区会话走 Host 当前所在的数据面。切换账号入口会释放旧节点
  会话并重新建立 access 链路。手动选择自建服务可在下次登录应用，用户会话凭据仅用于签发它的服务。

## 后果

用户可保留已保存主机和登录状态切换官方入口。应用显示的当前连接地址可能与所保存的服务标识不同。
数据面之间的续传令牌各自有效，切换时按新链路重新连接。
