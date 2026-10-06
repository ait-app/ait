# ADR-067：DeepSeek Harness ACP Provider

Status: Superseded in part by [ADR-082](adr-082-deepseek-harness-native-host.md)

默认接入已改为原生交互 Host；本文保留显式 ACP 兼容模式的契约。

## 背景

当前 Rust 服务通过 `AgentClient` / `AgentSession` 驱动 Codex 和 Claude。
Paseo 的 ACP 适配器提供可复用的 stdio、模型选择、审批和事件投影约定；
DeepSeek Harness 的 ACP v1 实现提供会话创建、恢复、关闭、配置选择及工具执行。

参考本地 Paseo `30178c4f5` 的
`packages/server/src/server/agent/providers/acp-agent.ts`，以及 Harness `477b4f4205` 的
`packages/acp/acp/src`。[官方 ACP 契约](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/acp/acp/README.md)
说明 Harness 使用 `session/resume`，不提供 `session/load` 或历史重放。

## 决策

- 在 `provider::local::deepseek_harness` 实现 Rust 原生 ACP stdio adapter，
  注册固定身份 `deepseek-harness`。生产 host 启动 PATH 中的 `dsh --profile acp`，
  `AIT_SERVER_DEEPSEEK_HARNESS_BIN` 可覆盖可执行文件路径；不恢复 Node server。
- 保留向内依赖：adapter 实现既有 ports，application 管理 Agent 生命周期、审批和持久化，
  domain 不引入 ACP、进程或 provider SDK 依赖。
- `initialize` 协商 ACP v1，不声明 client filesystem、terminal 等未实现能力。
  `session/new` / `session/resume` 使用绝对 cwd 和标准 MCP 参数；
  `session/close` 后回收该进程组。控制请求限时 30 秒，prompt 不设置推理时间上限。
- 模型选择值保持 provider 返回的 opaque 字符串，包括分组内的模型；
  `thought_level` 对应 `thinkingOptionId`，保留空字符串代表 provider default 的合法选择。
  模型切换先调用 `session/set_config_option`，再从完整返回状态验证该模型的推理选项。
  目录中的推理选项和 Paseo 一样来自当前模型；实际选择必须再次验证。
- ACP message/thought、tool lifecycle 和 usage 进入现有 `AgentTurnEvent`。
  以独立 turn ID 和 `native:` item key 发布重试稳定的增量与完整项，
  工具更新保留初始输入；输出预览有界，图片使用共享私有内容寻址目录。
- 权限选项保持 native option ID，仅接受行为匹配的选择；通用 Allow/Deny 只选一次性选项。
  持续授权必须显式选择 native 选项；过期或重复响应拒绝。
  取消仅发 `session/cancel`，等 correlated prompt 的 `stopReason` 才发布终态。
- 为 `AgentClient` 增加 `supports_history_replay()`，既有 adapter 默认 `true`，
  Harness 返回 `false`。application 对无重放能力的 provider 保留已持久化时间线，
  不以空 transcript 调用 reconcile。重启后恢复使用原生 session ID 与保存的 cwd。
- 仅保存会话 ID/cwd 和 display timeline；私有 per-Agent env 不进入持久化 handle 或 snapshot。
  Harness 继续拥有认证、工具、MCP 及原生会话存储。
- 前端增加 provider definition、ACP catalog entry 和官方 SVG，入口显示 DeepSeek Harness。

## 当前边界

Harness 没有模式、commands、fork 或 rewind，本 adapter 不声明这些能力。
原生 session/list 虽存在，但缺少完整历史导入，暂不提供 native session discovery/import。
已在 Ait 创建的会话可恢复，并保留 Ait 已观察的展示历史；其他前端写入的历史不会被导入。
systemPrompt、providerOptions、toolPolicy 和 outputSchema 暂无对应 ACP 语义，显式拒绝；
MCP 支持绝对命令的 stdio 和 HTTP，SSE 拒绝，已创建会话不接受 MCP 配置变更。
图片输入仅在 native `promptCapabilities.image` 为真时接纳。

## 验证

离线 stdio peer 覆盖分组模型、动态推理、请求关联、工具更新、一次性审批、
取消/恢复、私有环境、图片/大输出、异常帧与超时。
真实 Rust server 进程通过 WebSocket 验证注册、创建、审批、完成及重启后的时间线稳定性。
测试和覆盖率状态见[交付验证报告](../../reports/providers/deepseek-harness-acp.md)。
