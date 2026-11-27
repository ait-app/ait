---
id: ADR-319
status: accepted
date: 2026-11-27
amends:
  - daemon/adr-201-daemon-owned-publication.md
  - clients/adr-087-headless-device-authorization.md
  - clients/adr-318-official-service-entry.md
paths:
  - bins/daemon/src/device.rs
  - bins/daemon/src/device
  - crates/host-link
  - crates/api/src/managed_relay.rs
---

# ADR-319：daemon 自主选择官方入口

## 背景

客户端的地区推荐只影响账号 access 链路。无人值守机器仍独立授权并建立 Hub serve。
若机器授权请求和 Hub 使用不同数据面，后者可能拒绝前者签发的一次性票据。

## 决策

daemon 在首次需要联系中心时，从两条内置官方入口并行获取免认证地区推荐。优先采用已可达的推荐入口。
两边都没有推荐时优先采用可达的大陆入口。旧入口返回 `404` 时仍可作为候选。两边不可用时，后台发布按既有
网络故障策略重试。显式 `AIT_CENTER_URL` 覆盖跳过探测。本地 API 和离线状态查询无需联网。

同一次 CLI 授权或 daemon 运行时使用同一入口处理机器授权、刷新、注册、租约与 Hub。机器 HTTP
适配器为签发的 serve 票据附上请求的入口，Hub 适配器按此搭建链路。设备批准页面来自本地的官方地址
白名单，沿用旧中心返回大陆批准地址的契约。

## 后果

桌面内置和独立 daemon 按运行所在主机的出口选路，无需继承桌面账号偏好。入口变化不改变已有机器
凭据与 Host 身份。已建立的运行时仍使用当前入口重连，下一次启动会重新探测。
