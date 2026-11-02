# ADR-134：已接纳的普通 RPC 等待共享任务额度

- 状态：已实现
- 日期：2026-10-11
- 关系：延续 [ADR-120](adr-120-single-connection-startup-admission.md) 的连接队列和 [ADR-093](adr-093-consumable-request-context.md) 的请求所有权。

## 背景

单连接的四个能力组可以同时执行请求。普通 RPC 已进入有界连接队列后，原先的 `Context.call` 仍立即尝试获取唯一的共享短任务许可。另一个能力组正在使用该许可时，已接纳的正常请求返回 `resource_exhausted`，客户端有限重试也可能耗尽。

## 决策

- API 继续拥有连接、能力组队列、组内顺序和超量拒绝。`Context.call` 使用已有的 `Runtime.run_queued`，等待共享短任务许可后执行。
- 等待不创建 blocking task；停机取消尚未执行的请求。已经开始的 blocking task 继续持有许可并被追踪，响应未来对象被丢弃也不解除其资源所有权。
- 2026-11-01 修订：共享短任务额度从 1 增至 3，减少跨设备和能力组之间的短暂竞争；同一服务仍由其互斥锁保护。普通 RPC 继续等待，直接 `Runtime.run` 在三个名额均被占用时拒绝。不自动重放操作；专用 Git、终端与原生执行保持各自预算。
- 资源拒绝日志携带 `resource` 类别与可用的 `limit`、`available`、`requested` 数值。无请求编号的事件补充方法上下文，自动故障证据保留白名单内的上下文字段。
- RPC 错误日志携带方法、请求 ID、通过协议 ID 校验的目标 Agent/Terminal/Workspace 标识、剩余订阅容量和任务额度快照。连接队列超量拒绝记录能力组与队列容量；事件订阅拒绝记录受支持的事件类别和各生产服务的安装状态，不记录请求正文。

## 后果

正常并发请求可以等待前一项短任务完成，响应耗时包含这段等待。过量输入仍在有限连接队列处拒绝，业务错误也继续返回。等待许可的请求可随连接关闭或停机被取消；已执行的操作不因客户端断线而重放。

## Test coverage

未测量：用户未请求覆盖率。`cargo test --locked --offline -p model --lib runtime::tests`
验证三个短任务同时执行、第四项直接准入被拒绝、等待与取消，以及任务资源释放；
`cargo test --locked --offline -p daemon --bin daemon diagnostics::tests` 验证预算类别、事件上下文与凭据过滤。
另定向验证连接队列超量拒绝、订阅等待、分块上传、终端、后台 fetch 与 Agent 创建准入。
