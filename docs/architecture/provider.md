# Provider 现行规格

本页列出 `crates/provider` 及其在 `model`、`api`、`bins/daemon` 中的对接契约当前生效的规则，
供修改 Provider 代码前核对。每条规则只陈述现状；决策背景、被修订的旧规则和验证记录见所引 ADR。
组件总览见[当前架构](README.md)。

## 边界与装配

- `provider` 的生产依赖只有 `provider-core`、`provider-native`、`domain` 与 `model`；`persistence` 仅作为测试依赖（dev 依赖，以及只由
  `test-support` 启用的可选依赖），不依赖 `metadata`，
  也不反向依赖 `bot`。依赖守卫是 `bins/daemon/tests/dependencies.rs`
  （[ADR-102](../decisions/providers/adr-102-provider-metadata-independence.md)、
  [ADR-089](../decisions/providers/adr-089-provider-owned-composition.md)、
  [ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。
- 内置 adapter 列表只在 `crates/provider-builtin`：`provider_builtin::providers` 先用
  `provider::Providers::configured` 构建仍在 `provider` 内的 Codex 与
  Pi，再用 `Providers::with` 追加 `provider-antigravity` 的 `AntigravityClient`、
  `provider-claude` 的 `ClaudeClient`、`provider-deepseek-harness` 的 `DeepSeekHarnessClient`、`provider-minimax` 的 `MiniMaxClient`、`provider-omp` 的 `OmpClient` 与 `provider-opencode` 的 `OpenCodeClient`，共享同一图片目录（`OpenCodeClient` 与迁出前一样不接收该目录）；daemon 在 `compose_providers` 中只调用这一入口。`Providers` 是运行时的 adapter 容器，
  负责注册与摘要生成器装配。`provider-builtin` 的生产依赖只有 `provider`、`provider-antigravity`、`provider-claude`、`provider-deepseek-harness`、`provider-minimax`、`provider-omp` 与 `provider-opencode`。
  可执行文件默认值与 `AIT_SERVER_*_BIN` 覆盖随 adapter 列表维护（OMP 的经 `provider_omp::installed_program`）；
  构造与注册不启动原生进程，也不创建存储
  （[ADR-089](../decisions/providers/adr-089-provider-owned-composition.md)、
  [ADR-270](../decisions/providers/adr-270-provider-crate-family.md)、
  [daemon 手册](../operations/daemon.md#provider-与扩展能力)）。
- 原生 adapter 共享的进程、沙箱、ACP、图片、诊断与用量工具在 `crates/provider-native`，生产依赖只有
  `provider-core` 与 `domain`，只公开 adapter 用到的项；运行时代码不使用它，`provider` 依赖它只因部分 adapter
  仍在 crate 内（[ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。
- Antigravity 的 adapter 在 `crates/provider-antigravity`，生产依赖为 `provider-core`、`provider-native` 与
  `domain`（`model`、`persistence` 与带 `test-support` 的 `provider` 仅测试使用），只公开 `AntigravityClient`；
  `provider` 与 daemon 都不依赖它。`provider-builtin` 用 `Providers::with` 追加它：设置
  `AIT_SERVER_ANTIGRAVITY_BIN` 时直接使用该程序，否则用 `AntigravityClient::installed` 查找已安装的 `agy`。
  Antigravity 测试与 AGY 假程序、协议回放样本和工具卡片样本归 `provider-antigravity`，需要 `AgentManager`
  或 Timeline 的场景使用 `provider::testing`
  （[ADR-087](../decisions/providers/adr-087-antigravity-cli-provider.md)、
  [ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。
- Claude Code 的 adapter 在 `crates/provider-claude`，生产依赖为 `provider-core`、`provider-native` 与
  `domain`（`model`、`persistence` 与带 `test-support` 的 `provider` 仅测试使用），只公开 `ClaudeClient`；
  `provider` 与 daemon 都不依赖它。`provider-builtin` 用 `Providers::with` 追加它，可执行文件默认 `claude`，
  由 `AIT_SERVER_CLAUDE_BIN` 覆盖。Claude 测试与 Claude Code、钥匙串假程序归 `provider-claude`，需要执行
  worker、`AgentManager` 或 Timeline 的场景使用 `provider::testing`
  （[ADR-050](../decisions/providers/adr-050-claude-code-provider.md)、
  [ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。
- DeepSeek Harness 的 adapter 在 `crates/provider-deepseek-harness`，生产依赖为 `provider-core`、`provider-native`
  与 `domain`（`model`、`persistence` 与带 `test-support` 的 `provider` 仅测试使用），只公开
  `DeepSeekHarnessClient`；`provider` 与 daemon 都不依赖它。`provider-builtin` 用 `Providers::with` 追加它：
  设置 `AIT_SERVER_DEEPSEEK_HARNESS_BIN` 时直接使用该程序，否则用 `DeepSeekHarnessClient::installed` 查找已安装的
  CLI、桌面包或 npx 安装；`AIT_SERVER_DEEPSEEK_HARNESS_PROFILE` 选择 Web Host profile，
  `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp` 改用 ACP。DSH 测试与 ACP、Web Host、元数据假程序归
  `provider-deepseek-harness`，需要执行 worker、`AgentManager` 或 Timeline 的场景使用 `provider::testing`；
  共享的 Agent MCP 注入检查在 `provider::testing::agent_mcp`
  （[ADR-082](../decisions/providers/adr-082-deepseek-harness-native-host.md)、
  [ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。
- MiniMax Code 的 adapter 在 `crates/provider-minimax`，生产依赖为 `provider-core`、`provider-native` 与
  `domain`（`model`、`persistence` 与带 `test-support` 的 `provider` 仅测试使用），只公开 `MiniMaxClient`；
  `provider` 与 daemon 都不依赖它。`provider-builtin` 用 `Providers::with` 追加它，可执行文件默认 `mcode`，
  由 `AIT_SERVER_MINIMAX_CODE_BIN` 覆盖。MiniMax 测试归 `provider-minimax`，需要执行 worker 或
  `AgentManager` 的场景使用 `provider::testing`
  （[ADR-197](../decisions/providers/adr-197-minimax-code-acp.md)、
  [ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。
- Oh My Pi 的 adapter 在 `crates/provider-omp`，生产依赖为 `provider-core`、`provider-native` 与 `domain`
  （`model`、`persistence` 与带 `test-support` 的 `provider` 仅测试使用），只公开 `OmpClient` 与
  `installed_program`；`provider` 与 daemon 都不依赖它，它也不依赖 daemon。`provider-builtin` 用
  `Providers::with` 追加 `OmpClient`，共享同一图片目录。OMP 从 `provider-core` 导入 ports 与 protocol 值，
  从 `provider-native` 导入共享工具。OMP 测试归
  `provider-omp`；需要 `AgentManager` 或 Timeline 的场景使用 `provider::testing`，它只在 dev-only 的
  `test-support` feature 下编译，非 dev 依赖边启用或转发该 feature 会被依赖守卫拒绝
  （[ADR-249](../decisions/providers/adr-249-omp-adapter-crate.md)，已由
  [ADR-248](../decisions/providers/adr-248-omp-paseo-native-rpc.md) 修订）。
- OpenCode 的 adapter 在 `crates/provider-opencode`，生产依赖为 `provider-core`、`provider-native` 与
  `domain`（`model`、`persistence` 与带 `test-support` 的 `provider` 仅测试使用），只公开 `OpenCodeClient`；
  `provider` 与 daemon 都不依赖它。`provider-builtin` 用 `Providers::with` 追加它，可执行文件默认 `opencode`，
  由 `AIT_SERVER_OPENCODE_BIN` 覆盖。OpenCode 测试与 ACP、观测服务、子会话假程序和工具卡片、工具历史样本归
  `provider-opencode`，需要执行 worker、`AgentManager`、Timeline、所有权 lane 或 Provider 目录的场景使用
  `provider::testing`
  （[ADR-115](../decisions/providers/adr-115-opencode-acp-provider.md)、
  [ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。
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
- 原生角色与事件类型由 adapter 解释，客户端不按文本猜测用途；例如 OMP `async-result` 的
  `<system-notice>` 投影为 `notification`
  （[ADR-196](../decisions/providers/adr-196-provider-output-semantics.md)，OMP 部分已由
  [ADR-248](../decisions/providers/adr-248-omp-paseo-native-rpc.md) 修订）。
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

## Harness 用量账本

- 每台 Host 在 `agents/usage.sqlite3` 记录调用与请求级 token 用量；请求用量可累加，ADR-052 的
  `AgentUsage` 快照仍是替换值（`lastUsage`、`usage_updated` 不变）。账本经 `UsageLedger` 端口写入，
  失败只记日志，不影响轮次（[ADR-246](../decisions/providers/adr-246-harness-usage-ledger.md)）。
- 桶互斥：`input` 不含缓存读写，`reasoning` 与 `cache_write_1h` 为子集、不计入合计；请求以
  `(agent_id, segment_id)` 幂等，汇总仅在插入生效时更新。
- `usage_status`：每个请求均为 `request` 边界且 `complete`、且至少一个，为 `exact`；无请求或提供方不上报
  token 用量为 `missing`；其余为 `lower_bound`。失败、取消或被中断的调用至多为 `lower_bound`。
  只存提供方报告的费用。
- `calls` 与 `requests` 保留 90 天，`daily_usage`（UTC 日）无限期；不含文本、路径或凭据，不进入 Hub outbox。
  查询为 `provider.usage.ledger`，客户端按 `features.providerUsageLedger` 协商并逐 Host 汇总。

## 测试归属

- Provider 协议、发现、执行、权限、原生历史、恢复与回退的测试归所属 adapter 的 crate
  （`crates/provider`，Antigravity 为 `crates/provider-antigravity`，Claude Code 为 `crates/provider-claude`，DeepSeek Harness 为 `crates/provider-deepseek-harness`，MiniMax Code 为 `crates/provider-minimax`，Oh My Pi 为 `crates/provider-omp`，OpenCode 为 `crates/provider-opencode`；共享工具的测试归 `crates/provider-native`），放在所属模块的测试子文件；daemon 只测宿主生命周期、传输和跨能力组合，断言的能力边界决定归属
  （[ADR-179](../decisions/providers/adr-179-provider-owned-tests.md)、
  [ADR-249](../decisions/providers/adr-249-omp-adapter-crate.md)）。
- 需要多个真实 adapter 与真实运行时协作的装配测试归 `crates/provider-builtin`，经 `provider::testing`
  读取装配结果；运行时测试用桩 adapter 验证目录与容器行为，adapter 自身的端口值在该 adapter 的测试中断言
  （[ADR-270](../decisions/providers/adr-270-provider-crate-family.md)）。

## 各原生适配器的特有约束

- DeepSeek Harness 默认启动原生 Web Host；仅在 `AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp` 时使用
  ACP；两种模式都接受 Ait 每会话 MCP，原生 Host 经 `--patch` 叠加层、ACP 经 `mcpServers`
  （[ADR-232](../decisions/providers/adr-232-agent-mcp-session-delivery.md)；[ADR-082](../decisions/providers/adr-082-deepseek-harness-native-host.md) 取代
  [ADR-067](../decisions/providers/adr-067-deepseek-harness-acp.md) 的默认传输；
  [DSH 手册](../operations/deepseek-harness.md)）。
- OpenCode 通过官方 `opencode acp` 运行；接受 1.x 与 2.0.25 及以上版本，其余版本报告不可用
  （[ADR-115](../decisions/providers/adr-115-opencode-acp-provider.md)，最低版本已由
  [ADR-143](../decisions/providers/adr-143-opencode-2-0-25-compatibility.md) 修订）。
  已验证归属的子会话可只读核对 V2 持久终态，以修复遗漏的完成通知
  （[ADR-237](../decisions/providers/adr-237-opencode-child-terminal-readback.md)）。
- Codex 历史优先读取 `thread/timeline/list`，只在 `-32601` 或精确的 unknown-variant `-32600` 时
  回退 `thread/read`，其他错误保持可见
  （[ADR-191](../decisions/providers/adr-191-codex-native-realtime-voice.md)，历史限制已由
  [ADR-192](../decisions/providers/adr-192-codex-incremental-history-projection.md) 修订）。
- Oh My Pi 每个会话由一个 `omp --mode rpc-ui` 进程经原生 JSONL RPC 驱动，不经过 ACP；整轮在
  `prompt_result` 加 `session_settled` 或 `get_state.isSettled` 核对后完成，最低版本 18.12.6。
  完整规则见 [Oh My Pi 现行规格](omp.md)
  （[ADR-180](../decisions/providers/adr-180-omp-native-rpc-provider.md)、
  [ADR-194](../decisions/providers/adr-194-omp-settlement-reconciliation.md)、
  [ADR-198](../decisions/providers/adr-198-omp-interaction-progress.md)，均已由
  [ADR-248](../decisions/providers/adr-248-omp-paseo-native-rpc.md) 修订）。
- Pi 的模型与 thinking 修改在下一条 prompt 前应用；slash 命令每次请求用临时
  `pi --mode rpc --no-session` 进程探测，不缓存结果
  （[ADR-156](../decisions/providers/adr-156-pi-native-rpc-provider.md)，已由
  [ADR-211](../decisions/providers/adr-211-pi-next-turn-reasoning-controls.md)、
  [ADR-219](../decisions/providers/adr-219-pi-slash-command-discovery.md) 修订）。
