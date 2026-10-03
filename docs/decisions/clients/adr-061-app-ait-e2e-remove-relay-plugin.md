# ADR-061：App E2E 使用 Ait server，移除 relay 与插件运行功能

- 状态：Accepted
- 日期：2026-09-28
- 延续：ADR-044、ADR-047、ADR-049
- 用户要求：E2E 针对 Ait server，同时移除 relay、插件功能。

## 决策

1. App Playwright 的 worker 与辅助主机只启动独立 Rust `server`。全局准备构建前端依赖及 `daemon`；可用 `E2E_AIT_SERVER_BIN` 指定已有二进制。
2. 每个服务独占临时数据目录、随机非保留端口和随机 Bearer token，读取 `/v1/server/info` 确认协议和真实 UUID。Node seed client 使用生产 Rust transport adapter，浏览器使用生产一次性票据认证；不得连接开发服务的默认端口。
3. 删除历史 npm Paseo server、supervisor、relay 部署与插件测试服务启动器。进程重启测试使用 Ait 持久化记录，服务重启测试遵循 Rust 的进程内重启语义。
4. App 删除插件目录、动态加载/执行、扩展 UI、路由、侧栏、主题、附件、命令和面板注册，以及 relay 配对链接、扫码入口、offer 导入与 relay 连接实现。直连和 SSH 保留。Pair device 页面只说明直连方式。
5. 旧 relay 主机连接与插件面板在存储恢复时丢弃；普通连接、草稿文本和普通附件保留。历史插件 timeline / tab 类型仅用于识别旧持久化数据，不执行插件。
6. 普通 E2E 的 Codex 进程使用现有离线 stdio 协议 fixture，经过真实 Rust Provider。真实 Provider 测试需显式设置 `E2E_REAL_PROVIDERS=1`；不复制开发者的 Paseo 状态。

## 范围

本次不改变 Rust crate 边界、协议或生产 Provider。仓库共享 SDK 的旧 wire 类型与兼容包暂留，App 不再加载插件或建立 relay 连接。
继承的 UI 用例中仍有旧 mock-provider、创建回执与协议拦截断言；迁移服务目标不等于这些旧能力已实现。定向回归与运行说明见 [App E2E](../../../apps/mobile/e2e/README.md)。

运行路径、项目配置和旧桌面 CLI 的后续清理见 [ADR-062](adr-062-ait-runtime-paths.md)。

共享 client 已移除 relay/E2EE transport、静态导出和 `@getpaseo/relay` 依赖，并拒绝旧 relay URL。App 的 Metro relay 解析特例、扫码依赖和权限也已移除；独立历史包与兼容 wire 类型不属于 App 运行依赖链。
