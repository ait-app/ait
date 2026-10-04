# ADR-082：DeepSeek Harness 原生交互 Host

Status: Accepted

Date: 2026-10-04

## 背景

DSH 官方 ACP 是 automation-only profile，不提供权限模式和 user questions。
在 Ait 侧模拟 ACP mode 或把 question 当作新 prompt，不能完成原生待决交互。
本决策替代 [ADR-067](adr-067-deepseek-harness-acp.md) 的默认传输选择；旧 ACP 实现保留为显式兼容入口。

契约依据为官方 DSH `0.1.5-rc.2` 发布包中的 `dsh-api-session-controller`、
`dsh-api-remotes`、`dsh-user-approval`、`dsh-user-questions` 和 `dsh-token-meter` 类型及实现。
[官方源码](https://github.com/deepseek-ai/deepseek-harness)继续拥有模型、工具、权限和会话语义。

## 决策

- `provider::local::deepseek_harness::native` 实现既有 `AgentClient` / `AgentSession` ports。
  默认运行 `dsh --profile web --no-open --host 127.0.0.1 --port 0`，每个 Ait 会话拥有独立 Host 进程。
  OpenCode 保持 HTTP/SSE，Codex 的接入与实现不变。
- 验证启动 URL 仅指向指定回环地址，再用一次性 launch token 换取 cookie。
  HTTP RPC 与 `/api/remote.mux` WebSocket 使用同一 cookie；禁止代理和自动跳转。
  token/cookie 不进入日志、handle、snapshot 或提交。请求与响应有大小和时间上限。
- 模型目录和模式选择由原生 Host 验证。内建权限菜单提供 `read-only`、`workspace-write`、
  `danger-full-access`；实际切换使用 `/permission` 原生命令。配置按既有 Ait 语义在下一次输入前应用。
  模型保留 `[provider, model]` JSON opaque ID，推理等级保留 native ID 和原生默认值。
- `$events` waterfall 通过现有审批 port 映射。工具仅允许一次或拒绝，不合成永久授权。
  交互与工具历史跨逻辑流到达时，等待对应工具输入再展示审批。
  question 保留原生 ID、单/多选和自由文本，回答发送到确切 `clientId/eventId`，不作为新用户输入。
  其他 agent 或未知 waterfall 委派给原生下一处理器。重复、过期和篡改答案拒绝。
- 共享 question 表单新增显式 `answerFormat: "array"` 和 `answerKey`，DSH 使用此格式保留含逗号选项。
  原有 provider 的字符串回答路径不变。
- `session/follow` 按连续 durable seq 投影消息和工具；只消费本次运行已接纳回合的新事件。
  图片通过原生 attachment RPC 读取后进入现有私有内容寻址存储，并保持历史顺序。
  contextPressure 经 `session/control` 更新上下文占用，不把累计 token 数当作当前上下文。
- 取消调用原生 session/cancel，等待 turn/end。通信失败不重发不确定是否已接纳的输入。
  close 优先终止 owned Host 并等待持久化清理，超时再回收进程组。
- 保留 Ait 已保存的展示时间线，不导入其他前端历史。恢复原生 session ID/cwd；
  已安装 CLI 的隔离测试确认可接续旧 ACP handle。默认模式不自动回退 ACP。

## 兼容与边界

`AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp` 显式启用旧 adapter，保留旧 stdio/HTTP MCP override，
但没有权限模式与 question。带 native-host 标记的 handle 不允许交给 ACP。
原生 Host 使用 DSH 自己配置的 MCP；当前没有等价的每会话 MCP override RPC，Ait 显式拒绝该配置。
这不影响 DSH web profile 自带 MCP。原先保存 override 的会话需继续使用 ACP 或改在 DSH 配置 MCP。

暂不提供原生会话导入、其他客户端历史同步、rewind、steer、slash command 列表和结构化输出约束。
展示使用已落盘的 assistant/message，与旧 ACP 一样不声明逐 token 延迟保证。
原生模型选择可能依照 DSH 自身行为更新其默认模型。模型发现会创建 probe session，保留策略归 DSH。

## 验证

[验证报告](../../reports/providers/deepseek-harness-native-host.md)区分离线 HTTP/WebSocket 集成、
真实 CLI 无推理烟雾测试、覆盖率和待执行桌面场景。未把模拟 Host 当作真实模型或桌面测试。
