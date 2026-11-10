---
id: ADR-054
status: accepted
date: 2026-09-27
amends:
  - clients/adr-048-paseo-desktop-rust-launcher.md
  - clients/adr-049-app-rust-browser-transport.md
paths:
  - apps/desktop
---

# ADR-054：桌面服务监听配置与网络地址

- 范围：Paseo/Electron 启动器与独立 Rust server 的 transport 边界
- 修订：ADR-048、ADR-049 中的仅回环监听限制；领域模型与依赖方向保持不变。

## 背景

桌面 Host 概览需要编辑服务监听 IP 和端口，保存后供后续启动使用。原先桌面固定使用
`127.0.0.1:0`，Rust 配置与 API 同时拒绝非回环地址。

## 决策

1. 桌面主进程拥有监听配置，保存在 Electron `userData/desktop-settings.json` 的
   `settings.daemon.listen`。默认仍为 `127.0.0.1:0`；旧配置补默认值。地址接受 IPv4、IPv6
   和映射为 `127.0.0.1` 的 `localhost`，端口范围 0–65535。非法 IPC 更新失败且不写盘。
   配置修改和迁移串行执行，临时文件原子替换保留未知配置项。
2. 每次桌面启动/重启子进程时读取配置，优先级为 `AIT_SERVER_LISTEN` > 保存值 > 默认值，
   通过 `--listen` 交给 Rust server。地址/端口位于现有 Daemon 卡片内，失焦/Enter 自动保存；
   当前监听地址列在 Status 中。界面保留环境覆盖提示。保存本身不重启
   服务；服务自身的 RPC 重启继续沿用当前启动参数。独立运行 server 的 CLI/env/TOML 优先级不变。
3. 端口 0 在首次启动时分配端口；同一桌面进程内配置未变的重启继续复用实际端口。
   配置改变时废弃旧端口缓存。端口占用或不可绑定的 IP 使启动明确失败，不静默改用其他端口。
4. 状态区分 `listen` 和 `connectAddress`：`0.0.0.0` 经 `127.0.0.1` 连接，`::` 经 `::1`
   连接，具体 IP 使用自身。renderer 登记可连接地址；Bearer 仍只在主进程注入当前自有子进程
   的精确端点，LAN listener 不向同端口的 `localhost` 出借凭据。
5. Rust 接受显式网络 IP 和通配 IP，默认仍绑定回环。具体 IP 的 Host 校验仅接受该地址，
   `localhost` 别名仅在回环目的地有效。通配监听通过 Axum `ConnectInfo<LocalAddress>` 获取
   每条已接受 TCP 连接的本地目的地址，以该地址校验 Host/Origin；没有连接信息时拒绝请求。
   等价 IPv6 表示按 IP 比较。端口、Origin、Bearer、浏览器一次性票据规则仍然生效。

## 影响与验证

Host 概览仅为本机桌面托管服务提供该设置，离线时保留入口供修复配置。使用网络 IP 后可被
对应网络接口访问；这不增加 TLS、远端浏览器 origin 配置或凭据分发流程。

验证覆盖配置重载/并发保存/失败写入、实际子进程 IPv4/IPv6/LAN 连接、固定端口、凭据范围、
通配监听 Host/Origin 约束，以及 Electron 跨启动恢复。结果见
[实施报告](../../archive/reports/clients/desktop-server-listen.md)。
