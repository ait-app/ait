# ADR-115：OpenCode 官方 ACP Provider

- 状态：Accepted，2026-10-09。
- 范围：`crates/provider` 的 OpenCode adapter；修订 [ADR-074](adr-074-opencode-native-provider.md) 和 [ADR-110](adr-110-opencode-private-protocol-boundary.md) 的传输与生命周期实现。
- 约束：Rust、现有 `AgentClient` / `AgentSession` ports、原生认证与原生历史所有权保持不变。

## 背景

私有 HTTP/SSE adapter 需要自行协调 OpenCode 1.x / 2.x 的事件、审批、问答和执行状态。
原生 question 已经等待用户回答时，遗漏问答协议会令 Ait 保持 running，却没有可回答的表单。
本地累计 token 预算还可能在 OpenCode 能继续运行时提前终止任务。

[OpenCode 官方 ACP 文档](https://opencode.ai/v2/docs/cli/acp/) 提供统一的 stdio JSON-RPC、
会话加载、列表、取消、配置选项、审批和 `elicitation/create` 表单。
1.x 也提供官方 ACP。实际检查 1.18.4 / 2.0.20 的上游实现发现它们尚未提供表单回调；2.0.26 已提供。

## 决策

1. OpenCode 仅通过 `opencode acp` 执行，使用 ACP protocol version 1。
   兼容 OpenCode 1.x / 2.x，按原生握手检查会话能力；不因缺少可选表单或删除能力拒绝普通会话。
   版本号只用于识别原生配置格式和已验证的表单实现，不按小版本号整体拒绝。
   模型、模式、会话加载、列表与删除按实际原生响应处理；界面仅显示当前安装的能力，不呈现与另一版的比较或兼容模式。
   没有 HTTP/SSE 回退或自动重发。
   OpenCode 自己拥有内部 server、模型调用、原生工具、认证和原生存储。
   Ait 不连接或修改用户正在运行的原生进程，不向项目写入工具、配置或脚本。
2. 有界 stdio transport 与 DSH 的既有 ACP profile 共用，launcher 参数保持各 provider 自有。
   DSH 默认 native Host 不变。子进程组、读取任务、消息队列、写入期限和控制请求期限都有所有者。
   模型 prompt 不设宿主运行时限，也不再因适配器自行累计输入、缓存或推理 token 而终止。
3. 一个 Session 同时最多提交一个 `session/prompt`，后台执行器处理双向请求。
   原生 `session/request_permission` 的 once / always / reject 选项直接映射到既有审批。
   `elicitation/create` form 的 schema 映射到既有 question UI；保留字段 key、单选、多选、类型约束和自定义字段。
   不支持的表单返回 ACP `cancel`；不隐式批准，不替用户回答。
   1.x 保留原生 ACP 不提供 question 工具的行为，并关闭强制启用该工具的环境开关。
   尚无表单回调的早期 2.0.x 在该子进程内拒绝 question，不替原生实现表单协议。
   拒绝审批与主动取消分别发送各自协议；终态以原生 prompt 的 `stopReason` 为准。
4. `session/cancel` 的写入成功不等于任务已结束。等待原生 prompt 响应，并重放持久化历史后，才发布终态和释放下一个输入。
   未知接纳结果、协议错误或重放失败使该连接失效，不重发 prompt。
   关闭时排空事件背压、尝试原生取消并回收进程；无法确认结算时返回失败。
5. `session/load` 是完整历史来源，加载过程不提交输入、不修改模型或权限。
   原生 `messageId` / `toolCallId` 驱动稳定的 `native:opencode:acp-v1:` 展示身份。
   旧 Ait 句柄中的 session ID 和 client message 映射继续可读；OpenCode 自己负责原生存储版本兼容，Ait 不迁移或改写其数据库。
   已有私有协议展示历史在首次重放时按既有 timeline reconcile 规则替换展示 generation。
6. 工具输入和结果仅保存有界展示预览，完整内容留在原生 transcript。
   长文本按 UTF-8 边界分为至多 96 KiB 的块，给 JSON 控制字符转义与元数据留出空间，保持单项低于 768 KiB。
   历史通知增量消费，不把完整历史挤进 128 项控制通知队列。
7. 模型、模型专属 effort 和原生 primary agent 从 ACP config options 获取。
   因现行原生 `models` 命令在验证配置下返回空目录，发现使用无 prompt 的临时 ACP 会话。
   查询结束后调用原生 `session/delete`；取消未来任务时仍安排清理，不保留查询会话。
   摘要也使用独立、禁用工具、单步的 ACP 会话，结束或取消后删除原生历史。
   1.x 未声明 delete 时，发现使用该版本原生 `models --verbose`、`agent list` 和 `debug agent` 只读命令，过滤 hidden / subagent；不创建临时会话，不猜测默认模型。
   2.x 未声明 delete 时，目录查询不可用，不套用 1.x 的 CLI 格式。
   会话控件使用该会话返回的原生模式目录，避免静态 build / plan 或其他项目的模式污染当前会话。
   缺少 delete 时摘要返回不可用，不向用户原生历史写入无法清理的辅助会话。
   自动摘要选型跳过未声明 delete 的原生进程；不为另一版补造辅助通道。
8. 用户显式选择的 permission 或 system prompt 仅通过该 ACP 子进程的内存环境配置传入。
   未选择时继承原生配置；配置改变时先关闭旧连接，再恢复同一原生 session，并在提交下一次输入前应用选项。
   不再写私有 HTTP session permission 路由，也不写用户配置文件。
   启动配置按原生格式编码：1.x 使用 `permission` / `agent.<mode>.prompt`，2.x 使用 `permissions` / `agents.<mode>.system`。

## 协议限制

- ACP 重放不提供每条消息的创建时间。展示条目使用收到通知的时间；timeline 对已有身份保留首次存储时间。
  原生会话活动时间来自 `session/list.updatedAt`。导入的 creation 时间在原生协议缺失时使用观察时间，不能当作原生历史创建时间。
- ACP 不提供完整的外部 writer 状态查询。原生 prompt 拒绝或错误必须直接结算，不能通过重新提交来探测或补救。
- 原生 MCP 配置继承 OpenCode 项目设置。Ait 仍不宣称支持每个会话动态注入 MCP server 或原生 rewind。
- 原生存储兼容与升级属于 OpenCode；升级 CLI 前应遵循其官方说明，Ait 不替换本机安装。

## 验证

使用不可变、无网络的 ACP 可执行夹具验证并发、旧句柄、审批、表单、取消、历史、输出预算和清理。
真实 CLI 验证使用隔离 XDG 目录与确定性的 loopback 模型，不读取用户认证，不访问真实模型服务。
验证结果与 Test coverage 记录见 [ACP 迁移报告](../../reports/providers/opencode-acp-2026-10-09.md)。
