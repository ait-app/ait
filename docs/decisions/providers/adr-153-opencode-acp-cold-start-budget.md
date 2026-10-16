# ADR-153：OpenCode ACP 冷启动预算

- 状态：Accepted，2026-10-16。
- 范围：Provider catalog、OpenCode ACP 启动与原生目录发现；细化 ADR-135 的目录就绪和并发启动协调。

## 背景

OpenCode 首次运行会初始化原生数据库、项目配置和插件。Ait 的 ACP `initialize`、`session/new`、
只读 CLI 查询和 Provider catalog 原先各有独立的 30 秒上限。即使原生进程持续正常工作，
任一较慢阶段也会被误判为失败；catalog 的 30 秒总预算还会截断可用性检查和后续目录发现。

## 决策

1. OpenCode 版本查询、2.x 启动 gate 和 ACP `initialize` 共用 120 秒启动截止时间。初始化成功后，
   普通控制请求仍使用 30 秒上限；首次 `session/new` 单独允许 120 秒，让原生项目配置完成。
2. 原生只读 CLI 目录查询允许 120 秒，但不修改 OpenCode 安装、认证、配置或模型目录。
3. `AgentClient` 可声明包括排队、可用性检查和发现的 catalog 总预算。默认保留 30 秒；
   OpenCode 使用 5 分钟，覆盖两次 ACP 初始化和模型读取。其他 Provider 独立调度。
4. 超时、取消或协议错误不重放请求。ACP 连接归适配器所有，失败时关闭并回收其子进程。

## 后果

OpenCode 首次目录加载可能显示更久，但不会因旧的 30 秒上限提前失败。普通会话控制和前台
prompt 的超时语义不变。离线 ACP 夹具验证初始化及会话创建跨过旧上限仍能成功；catalog
回归验证较慢的 OpenCode 不会阻塞其他 Provider。
