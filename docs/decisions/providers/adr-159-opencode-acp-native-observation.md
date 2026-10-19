# ADR-159：OpenCode 优先使用 ACP，按缺失能力补充原生观测

- 状态：Accepted，2026-10-19。
- 范围：`provider::local::opencode`，修订 [ADR-115](adr-115-opencode-acp-provider.md)
  的观测来源，保留 ACP 执行、配置、审批和权威历史边界。

## 背景

OpenCode 1.18.4 在 ACP 中只于整轮结束时发送用量；最后一条空 assistant 占位消息被取消后，
它还会发送零计数。ACP 不转发该版本已有的原生压缩事件，且把原生 `finish=unknown`
转换为正常 `end_turn`。客户端无法根据这些 ACP 更新还原事实。

## 决策

1. ACP 是唯一写入和控制通道。消息、工具、模式、审批、取消、会话关闭、原生列表和完整
   文本／工具历史继续走现有 ACP adapter，不通过 HTTP 重试或补发输入。
   保持一次接纳、原生历史结算和 4096 条用户身份映射预算。
2. 仅对已验证的 OpenCode 1.18 原生协议 profile 补充只读观测，真实验证版本为 1.18.4。
   其他版本优先使用 ACP 的 `usage_update` 与 compaction marker，不猜测 HTTP 协议。
   OpenCode 2.x 不自动启用这条补充通道。
3. 观测连接同一个受管 `opencode acp` 子进程内部的官方本地 server，不启动第二个原生
   进程，不附着用户已有的外部 server。显式绑定 `127.0.0.1`、禁用 mDNS，使用每次启动
   独立的随机 Basic Auth 密码。端口预约紧接在启动前释放；只有 ACP 初始化成功后才连接。
   密码只传子进程环境与敏感 HTTP Header，不进入 argv、句柄、配置文件或日志。
4. 原生 API 只用于 GET `/event`、`/config/providers`、`/session/:id` 和
   `/session/:id/message`。不访问原生数据库、不读取认证文件、不修改原生配置。
   原生 JSON、版本识别、认证、SSE 和分页细节全部留在 OpenCode 私有适配器内；
   其他 crate、AgentManager、RPC、共享协议和客户端不增加 OpenCode 专有规则。
5. 用量按最新有效 assistant 的原生 input + cache.read 上报，与 ACP 的计数口径一致。
   不累计各工具步骤，不按文字长度估算，不让零计数占位消息覆盖最近有效请求。
   窗口上限来自原生模型目录；累计费用来自原生 session 元数据，不累加重复更新。
   压缩后的较小正数仍是新的有效用量。有效 ACP 用量直接消费。
6. 原生 compaction part 与 summary assistant 的 parentID 提供稳定压缩身份。
   实时投影开始和终态，恢复时只回放原生终态，不补造过去的开始事件。
   压缩专用的空用户消息也关联到接纳输入，使 marker 和摘要在继续、重放及恢复后保留
   正确轮次。原生未闭合的压缩在 ACP 确认结束后显示失败。
7. 原生 `finish=unknown` 仅在本轮产生新 assistant、ACP 返回 `end_turn` 且读取到明确
   原生结束事实时显示固定失败原因。旧轮次的 outcome 不影响新输入，不解析文本判断完成。
8. 订阅先于元数据快照建立，断开后重新订阅并校正快照。刷新按事务提交，取消或失败不会
   留下半次统计更新。快照、分页、队列和帧有界；循环游标拒绝，HTTP 不跟随重定向或系统代理。
   观测失败不取消 ACP 模型请求；保留可用事实并记录固定诊断。观测任务随 Session／受管
   连接关闭而取消。模型执行继续不设运行时限。

## 验证

使用离线 ACP、认证 loopback HTTP/SSE 夹具和隔离 XDG 的真实 1.18.4 验证。
验证覆盖工具执行中的实时统计、压缩生命周期、继续、历史回放、恢复、未知结束原因、
身份预算、其他会话隔离、分页边界、错误脱敏和原生进程清理。
测试与覆盖率状态见 [排查和验证报告](../../reports/providers/opencode-auto-compaction-2026-10-19.md)。

原生接口依据：
[ACP 启动](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/cli/cmd/acp.ts)、
[Server API](https://opencode.ai/docs/server/)、
[原生分页](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts)、
[原生压缩](https://github.com/anomalyco/opencode/blob/v1.18.4/packages/opencode/src/session/compaction.ts)。
