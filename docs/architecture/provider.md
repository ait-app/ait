# Provider 现行规格

本页列出 `crates/provider` 及其在 `model`、`api`、`bins/daemon` 中的对接契约当前生效的规则，
供修改 Provider 代码前核对。每条规则只陈述现状；决策背景、被修订的旧规则和验证记录见所引 ADR。
组件总览见[当前架构](README.md)。

## 边界与装配

- `provider` 的生产依赖只有 `domain` 与 `model`；`persistence` 仅作为测试依赖，不依赖 `metadata`，
  也不反向依赖 `bot`。依赖守卫是 `bins/daemon/tests/dependencies.rs`
  （[ADR-102](../decisions/providers/adr-102-provider-metadata-independence.md)、
  [ADR-089](../decisions/providers/adr-089-provider-owned-composition.md)）。
- `provider::Providers` 是内置 adapter 的唯一装配入口：Codex、Claude Code、Antigravity、OpenCode、
  DeepSeek Harness、Pi、Oh My Pi、MiniMax Code。可执行文件默认值与 `AIT_SERVER_*_BIN` 覆盖由
  provider 维护；构造与注册不启动原生进程，也不创建存储
  （[ADR-089](../decisions/providers/adr-089-provider-owned-composition.md)、
  [daemon 手册](../operations/daemon.md#provider-与扩展能力)）。
- 摘要生成器只收录实现结构化辅助生成的 adapter，辅助会话不进入前台 Agent registry；生成接口
  `SummaryGenerator` 的共享契约在 `model::summary`
  （[ADR-101](../decisions/providers/adr-101-provider-summary-generator.md)、
  [ADR-099](../decisions/providers/adr-099-provider-owned-auxiliary-models.md)）。

## 执行与会话所有权

- 执行器在准入时把 wire 方法转换为封闭的 `Operation` 枚举；`access()` 与 `target()` 对每个操作
  穷举分类，新增操作不能按名称前缀获得读权限。子会话列表与子时间线以 `parentAgentId` 定位，
  归父 Agent 的会话所有者处理（[ADR-205](../decisions/providers/adr-205-provider-execution-contracts.md)）。
- 每个稳定 Agent 身份由一个会话 lane 独占原生 writer，同一 lane 内的修改保序；get/list/wait、
  Catalog 与发现查询不进入会话 lane；archive、delete、items.close 和 Workspace 退役作为相关 lane
  的屏障执行（[ADR-091](../decisions/providers/adr-091-independent-session-execution.md)，修订
  [ADR-088](../decisions/providers/adr-088-provider-catalog-startup-isolation.md)、
  [ADR-032](../decisions/providers/adr-032-daemon-native-provider-execution.md) 的全局串行）。
- 执行器使用专用 Tokio current-thread runtime 驱动原生 I/O，会话操作与存储读取经
  `spawn_blocking` 离开 reactor
  （[ADR-091](../decisions/providers/adr-091-independent-session-execution.md)）。
- 预算：命令接纳 64、Catalog 响应槽 64、原生会话 32、历史读取 8、直接读取 16、显式 Provider
  查询 16；Catalog 每 Provider 并发 4、总并发 16、待发现 64，60 秒内的条目直接复用，进行中的发现
  合并重复刷新（[ADR-091](../decisions/providers/adr-091-independent-session-execution.md)）。
- 独立读取只在 Provider、原生持久句柄、配置和 runtime 信息完全一致时复用会话所有者发布的
  动态模式与功能；比较用的配置只存在内存中，不进入响应或事件
  （[ADR-205](../decisions/providers/adr-205-provider-execution-contracts.md)）。
- 目录与单条读取使用 registry 的 `snapshot()` / `get_shared()` 已提交不可变视图
  （[ADR-210](../decisions/providers/adr-210-committed-runtime-read-snapshots.md)）。
- 经 adapter 验证的原生后代登记为只读 observed Agent，身份由 Provider、根原生 session 与原生子 ID
  派生。带 `agentId` 的写操作在原生副作用前返回 `UnsupportedCapability`；未加载的 observed 历史
  路由到根 Agent 的 lane，打开子会话不会启动子 writer
  （[ADR-209](../decisions/providers/adr-209-observed-subagent-normalization.md)，修订 ADR-041/052
  的子会话边界）。
- 子 Agent 的引导与停止是父 Agent 拥有的类型化操作：`agent.subagents.guide/stop.request` 以
  `parentAgentId` 定位，`Operation::ChildGuide/ChildStop` 属 `Access::Session`、`Target::Parent`，
  在父会话 lane 内保序执行。写前依次校验父记录为受管、未归档、非内部且本进程持有 live writer
  （不 live 即拒绝，不 resume），子记录有 `observation` 且 `parentAgentId`/`rootAgentId` 指向该父、
  `ready`、`health == Current`、可选 `expectedRevision` 匹配，引导另需子级 `Running`。客户端只发
  Ait 身份，原生子 ID 由服务端从观测记录解析。这是 observed 写保护的唯一窄豁免：对子 Agent 自身
  id 的其他写操作仍一律拒绝，也不为子级创建 writer。端口
  `AgentSession::guide_child/stop_child` 默认 `Rejected`，成功时只返回**收据**
  `ChildControlReceipt { token }`：原生命令已写出，最终结果作为
  `AgentTurnEvent::ChildControl { token, outcome }`
  （`ChildGuideOutcome::{Accepted, NotRunning}`、`ChildStopOutcome::{Cancelled, NotRunning}`、
  或 `ChildControlFailure::{Rejected, Uncertain}`）经既有事件流异步到达。
  因此控制等待期间父 lane 仍继续处理排队中的权限问题与读取；RPC 回复由 lane 在收到该事件后结算，
  不占用 writer，也不为控制另起任务或第二个 writer。
  `accepted/cancelled` 只表示原生接受，终态由只读子会话生命周期事件观察，`not_running` 不是成功，
  `Uncertain` 永不自动重发。目前仅 OMP 实现（父进程内 `get_subagents` 复核后
  `steer_subagent`/`cancel_subagent`）；未实现端口的 adapter 走默认 `Rejected`，RPC 呈现为
  `UnsupportedCapability`，能力标志也不出现。父级不 live、父链不符、观测过期、版本不符、引导目标
  非运行中统一为 `UnsupportedCapability`，缺记录为 `AgentNotFound`，空白引导文本为
  `InvalidMessage`。
  host feature `subagentControls` 由两个方法存在推导，父快照
  `supportsChildGuide`/`supportsChildStop` 来自 adapter 已验证事实
  （[ADR-248](../decisions/providers/adr-248-parent-owned-subagent-controls.md)，修订
  [ADR-205](../decisions/providers/adr-205-provider-execution-contracts.md)、
  [ADR-209](../decisions/providers/adr-209-observed-subagent-normalization.md)）。
- Bot 隐藏会话（`internal` 且有 `owner`）的权限请求在发布前直接回复原生 deny；请求缺少 ID 或
  拒绝失败时该轮按失败结算。只有 get、timeline、时间线搜索和 prompts 可以按完整 ID 寻址隐藏会话
  （[ADR-145](../decisions/bot/adr-145-bot-permission-denial.md)，第 3、4 条已由
  [ADR-213](../decisions/bot/adr-213-single-workspace-manager-bot.md)、
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 修订；
  [ADR-221](../decisions/bot/adr-221-read-only-bot-session-inspection.md)；
  [Bot 手册](../operations/bots.md)）。
- 执行器启动、接纳请求前收尾没有本进程 writer 的 `running` 记录：未归档、非内部、有有效句柄
  且无待处理持久权限的 Codex 会话向持久 FIFO 放入一次身份稳定的继续指令（已有排队输入时不追加）；
  其余写入固定的重启中断错误并转为 `error`。原始用户输入从不重放，写盘失败使启动失败
  （[ADR-175](../decisions/providers/adr-175-restore-provider-recovery.md) 第 2 项已由
  [ADR-177](../decisions/providers/adr-177-codex-restart-continuation.md) 对 Codex 修订）。
- 沙箱 Workspace 中，adapter 未声明 `supports_write_confinement` 时创建或恢复返回
  `ConfinementUnsupported`，失败关闭；目前 Antigravity 与 DeepSeek Harness 未声明
  （[ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)、
  [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)）。共享的 slash 命令发现路径
  不传入写入限制（[ADR-219](../decisions/providers/adr-219-pi-slash-command-discovery.md)）。
- 秘密问答的协商状态从 `Context.secret_questions` 经私有 `PermissionContext` 进入
  `Operation::Permission`，与请求 JSON 分离；会话 lane 根据当前 pending 请求拒绝未协商连接的
  secret ALLOW，DENY 不受此限制。连接事件和 get/list/目录订阅投影不修改共享状态；
  secret resolution 回包脱敏，持久 Hub permission 通知只含失效标记。该能力是兼容性而非授权
  （[ADR-243](../decisions/providers/adr-243-omp-structured-native-interactions.md)，proposed）。

## 时间线与历史投影

- 原生会话、认证与历史由原生程序持有；provider 的 Timeline SQLite 是可重建的展示索引。
  历史校验时未变的原生前缀只追加；改写则在同一事务中退役旧 epoch、换新 epoch，保留宿主失败通知
  与 plugin 条目，提交后才发布 `agent.timeline.replacement`
  （[ADR-040](../decisions/providers/adr-040-native-session-import-refresh-context.md)、
  [ADR-125](../decisions/providers/adr-125-durable-timeline-projection-pages.md)）。
- 单个序列化 `NativeItem` 上限 768 KiB，Timeline 查询响应上限 900 KiB；超限返回
  `ResourceExhausted`，不截断条目也不返回无法推进游标的空页
  （[ADR-090](../decisions/providers/adr-090-timeline-entry-and-page-budgets.md)，修订 ADR-039；
  Codex MCP 超大字段预览见 [ADR-140](../decisions/providers/adr-140-codex-mcp-display-previews.md)）。
- `allowCached` 仅在已有持久条目且没有未完成的 rewind 替换时走独立读取，响应以
  `historyNeedsRefresh` 标明尚未完成原生校验；daemon 以 `agent-timeline-cached-reads-v1` 宣告
  （[ADR-133](../decisions/providers/adr-133-cached-timeline-first.md)）。
- 服务端投影按 `callId` 合并同一 turn 的工具生命周期，只合并相邻且同一 turn 的 assistant/reasoning
  片段（assistant 还需 `messageId` 一致）；跨工具调用的每轮单一 Thinking 由共享客户端组合
  （[ADR-125](../decisions/providers/adr-125-durable-timeline-projection-pages.md)、
  [ADR-196](../decisions/providers/adr-196-provider-output-semantics.md)）。
- 原生角色与事件类型由 adapter 解释，客户端不按文本猜测用途；例如 OMP `async-result` 投影为
  `background:<jobId>` 的 `tool_call`
  （[ADR-196](../decisions/providers/adr-196-provider-output-semantics.md)）。
- adapter 识别原生压缩开始/结束并输出 `compaction` 条目；原生执行结束时仍未关闭的压缩标为
  `failed`（[ADR-150](../decisions/providers/adr-150-provider-compaction-lifecycle.md)）。

## 错误与失败展示

- 会话失败时，协调层在关闭原生会话前取得安全诊断与说明，提交一条 key 以
  `host:provider-failure:` 开头的 `error` Timeline 条目，再写 `lastError` 与 `turn_failed.error`；
  条目身份在重试间稳定。原生声明仍在重试的诊断不作为最终失败原因
  （[ADR-144](../decisions/providers/adr-144-provider-error-reporting.md)，展示策略已由
  [ADR-185](../decisions/providers/adr-185-native-provider-failure-details.md) 修订）。
- 原生失败说明只取明确的错误字段：检查上限 32 KiB，原因上限 3072 字节，脱敏凭据与 URL；
  最终消息不超过 4096 字节且不含控制字符。日志只记录结构化事实，`Failure` 的 `Debug` 不输出说明
  （[ADR-185](../decisions/providers/adr-185-native-provider-failure-details.md)，延续
  [ADR-162](../decisions/providers/adr-162-codex-error-details.md)；Pi thinking 拒绝的专用说明见
  [ADR-211](../decisions/providers/adr-211-pi-next-turn-reasoning-controls.md)）。

## 技能与 MCP 注入

- `ait-orchestration` Skill 编译进 provider。daemon 启动时先恢复事务，自动分发开启时按保存的
  selection（缺省 `all`）只新增和更新、不删除；失败记录告警并继续启动
  （[ADR-129](../decisions/providers/adr-129-provider-owned-skills.md)、
  [ADR-199](../decisions/providers/adr-199-bundled-skill-distribution.md)、
  [Skill 手册](../operations/agent-skills.md)）。
- daemon MCP 只在交互式打开会话时注入，且仅限声明 `supports_agent_mcp` 的 adapter：Codex、Claude、
  MiniMax Code 与 DSH ACP。注入配置只进入原生运行时设置，不写入 record 或持久句柄；策略在会话
  打开时读取，用户自定义的 `ait` 项优先于共享令牌
  （[ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)、[MCP 手册](../operations/mcp.md)）。
- 注入用的令牌每次 daemon 启动生成，API 只在 `/mcp/agents` 接受它
  （[ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)）。
- Bot 会话与 Bot 子树改用作用域凭据并覆盖用户的 `ait` 项；Bot 会话不受 `injectIntoAgents` 控制，
  无法交付时移除 `ait` 项而不回退共享令牌。带访客分享标签的 Agent 一律移除 `ait` 项
  （[ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md) 已由
  [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 修订；
  [ADR-208](../decisions/daemon/adr-208-workspace-share-guests.md)）。
- MCP 配置不得出现在原生进程参数中；Claude 的 `--mcp-config` 只接收 transport 持有的私有临时文件
  路径（[ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)）。

## 账号用量

- Host 用量缓存归 `AgentManager`，账号身份是只在内存比较的 SHA-256 摘要。60 秒内直接返回，
  60–300 秒返回旧快照并后台刷新，错误快照最多复用 15 秒；整个 Host 最多四路原生查询，共用
  25 秒截止时间（[ADR-119](../decisions/providers/adr-119-native-account-usage-and-codex-speed.md)
  的缓存策略已由 [ADR-167](../decisions/providers/adr-167-account-scoped-successful-usage.md)、
  [ADR-219](../decisions/providers/adr-219-shared-account-usage-cache.md) 修订）。

## 测试归属

- Provider 协议、发现、执行、权限、原生历史、恢复与回退的测试归 `crates/provider`，放在所属模块
  的测试子文件；daemon 只测宿主生命周期、传输和跨能力组合，断言的能力边界决定归属
  （[ADR-179](../decisions/providers/adr-179-provider-owned-tests.md)）。

## 各原生适配器的特有约束

- DeepSeek Harness 默认启动原生 Web Host；仅在 `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp` 时使用
  ACP，且只有 ACP 模式接受 Ait 每会话 MCP
  （[ADR-082](../decisions/providers/adr-082-deepseek-harness-native-host.md) 取代
  [ADR-067](../decisions/providers/adr-067-deepseek-harness-acp.md) 的默认传输；
  [DSH 手册](../operations/deepseek-harness.md)）。
- OpenCode 通过官方 `opencode acp` 运行；接受 1.x 与 2.0.25 及以上版本，其余版本报告不可用
  （[ADR-115](../decisions/providers/adr-115-opencode-acp-provider.md)，最低版本已由
  [ADR-143](../decisions/providers/adr-143-opencode-2-0-25-compatibility.md) 修订）。
- Codex 历史优先读取 `thread/timeline/list`，只在 `-32601` 或精确的 unknown-variant `-32600` 时
  回退 `thread/read`，其他错误保持可见
  （[ADR-191](../decisions/providers/adr-191-codex-native-realtime-voice.md)，历史限制已由
  [ADR-192](../decisions/providers/adr-192-codex-incremental-history-projection.md) 修订）。
- Pi 的模型与 thinking 修改在下一条 prompt 前应用；slash 命令每次请求用临时
  `pi --mode rpc --no-session` 进程探测，不缓存结果
  （[ADR-156](../decisions/providers/adr-156-pi-native-rpc-provider.md)，已由
  [ADR-211](../decisions/providers/adr-211-pi-next-turn-reasoning-controls.md)、
  [ADR-219](../decisions/providers/adr-219-pi-slash-command-discovery.md) 修订）。
- OMP 的 Ait 会话直接持有原生 `Session`；外部 ACP 转换仍归 provider，经独立 `ait-omp-acp` CLI 暴露。
  内部不再使用 ACP 客户端或 `_omp/rpc`。旧句柄/别名恢复不重写原生历史，创建、恢复及配置重开
  保留调用方精确 confinement；会话作用域探测同样遵守它
  （[ADR-237](../decisions/providers/adr-237-omp-native-session-and-standalone-acp.md) 修订 ADR-190 的内部入口）。
