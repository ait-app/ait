# ADR-161：移除 Workspace 服务代理 URL 字段

- 状态：已实现
- 日期：2026-10-19
- 关系：修订 [ADR-160](adr-160-remove-legacy-relay-protocol.md) 对旧服务链接字段兼容测试的保留决定；保留 [ADR-152](adr-152-remote-workspace-browser-loopback.md) 的在线中继浏览器回环转发。

## 决策

当前 Rust daemon 始终为空的 `proxyUrl`、`localProxyUrl`、`publicProxyUrl` 不再属于 Workspace 服务协议。本次删除 metadata 的输出字段、domain 的协议字段、共享 TypeScript schema、前端缓存存取、URL 分类回退及对应的旧测试。旧 `public`/`paseo` 路由选择器、偏好存储及翻译一起移除。

服务访问地址由运行服务的端口和当前主机连接生成。在线中继在代码中使用 `accountRelay` 类型，继续通过现有桌面浏览器的回环转发访问远端 `localhost` 服务；主机发现、票据、transport 和转发协议不变。本机和直连主机继续使用现有的服务地址生成方式。

Workspace 缓存投影版本从 1 升为 2，旧工作区行及其目录游标会失效并重新同步；Agent、项目和时间线缓存保持原有格式。没有增加旧字段解析或迁移回退。

## 来源证据

固定上游 `paseo-workspace.json` 及其生成器仍保留原始协议作为审计来源。domain 的对照测试只比较当前保留的字段，跳过上游专属的三个字段验证用例；另用回归测试确认当前解析结果和服务端输出均不暴露这三个字段。上游快照不代表当前 Ait 支持旧代理 URL。

Provider 配置中的 `proxy_url` 属于模型提供商的网络配置，与 Workspace 服务链接无关，不在本次移除范围。

## Test coverage

未测量：用户未请求覆盖率。定向验证 metadata 的脚本输出、domain 的 Workspace 协议、共享消息 schema、工作区缓存失效和服务链接，以及现有账户中继；不运行全 Rust workspace 测试。后续如需测量，只覆盖这些受影响模块。

基于 `50d40091` 及本次工作区修改的验证结果：

- `RUSTC_WRAPPER= cargo test --locked --offline -p domain workspace::protocol::workspace::tests`：2 项通过，包含保留字段与固定上游样本的对照。
- `RUSTC_WRAPPER= cargo test --locked --offline -p metadata automation::`：50 项通过。脚本用例需要绑定本地临时端口，已在允许该操作的环境下重跑通过。
- 移动端 11 个直接相关测试文件共 146 项通过；协议 `messages.workspaces.test.ts` 和 `relay.test.ts` 共 40 项通过。
- 协议构建、客户端及移动端类型检查、受影响代码的 lint、格式和文档链接检查通过；`RUSTC_WRAPPER= cargo clippy --locked --offline -p domain -p metadata --all-targets -- -D warnings` 通过。
