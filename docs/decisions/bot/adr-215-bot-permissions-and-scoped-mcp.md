---
id: ADR-215
status: accepted
date: 2026-11-06
amends:
  - bot/adr-145-bot-permission-denial.md
  - bot/adr-213-single-workspace-manager-bot.md
  - clients/adr-214-workspace-bot-tab-v2.md
  - daemon/adr-148-daemon-mcp.md
  - providers/adr-202-agent-mcp-injection.md
paths:
  - crates/bot
  - crates/api
  - crates/provider
---

# ADR-215：Bot 权限与作用域 MCP 凭据

- 后续修订：[ADR-222](adr-222-single-agent-mode-no-bot-workspaces.md) 将模式上限改为每个 Provider 单一模式，并取消 `workspaces` 分组。
- 关系：延续 [ADR-213](adr-213-single-workspace-manager-bot.md)；
  修订 [ADR-145](adr-145-bot-permission-denial.md) 第 4 条（Bot 会话按 Provider 能力选择最严格模式，取代模式改写）；
  修订 [ADR-202](../providers/adr-202-agent-mcp-injection.md)（作用域凭据覆盖用户 `ait` 项，Bot 会话不受 `injectIntoAgents` 控制）；
  修订 [ADR-148](../daemon/adr-148-daemon-mcp.md)（`X-Ait-Agent-Id` 仍不是权限边界，作用域凭据才是）；
  修订 [ADR-214](../clients/adr-214-workspace-bot-tab-v2.md)（向导与设置增加权限）。
- 计划：[阶段 3 实施计划](../../archive/plans/workspace-bot-manager-phase-3.md)、[Workspace Bot 管理者重设计](../../plans/workspace-bot-manager.md)。

## 背景

ADR-213 的 Bot 只能对话。要让 Bot 作为管理者启动 Agent、建立定时任务，就要让它调用 daemon MCP；
但共享 MCP 令牌拥有 daemon 的全部编排能力，并且 `X-Ait-Agent-Id` 只是调用者自报的上下文。
需要一个由用户配置、由 daemon 在每次请求时强制执行的权限边界，同时覆盖 Bot 启动的所有 Agent。

## 决策

### 1. 权限模型

`BotPermissions`（`crates/domain/src/bot/permissions.rs`）由 `groups` 与 `modes` 组成，线上名为 camelCase。

| 分组         | 工具                                                                                                                                                                                                                                                                                                 |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `observe`    | `list_agents`、`get_agent_status`、`get_agent_activity`、`wait_for_agent`、`list_schedules`、`inspect_schedule`、`schedule_logs`、`list_workspace_scripts`、`list_terminals`、`list_profiles`、`list_providers`、`list_models`、`inspect_provider`、`list_pending_permissions`（总是开启，不可关闭） |
| `agents`     | `create_agent`、`send_agent_prompt`、`update_agent`、`set_agent_mode`                                                                                                                                                                                                                                |
| `stop`       | `cancel_agent`、`archive_agent`、`kill_agent`                                                                                                                                                                                                                                                        |
| `schedules`  | `create_schedule`、`update_schedule`、`pause_schedule`、`resume_schedule`、`run_schedule_once`、`delete_schedule`、`create_heartbeat`、`delete_heartbeat`                                                                                                                                            |
| `scripts`    | `start_workspace_script`、`stop_workspace_script`                                                                                                                                                                                                                                                    |
| `terminals`  | `create_terminal`、`send_terminal_keys`、`capture_terminal`、`kill_terminal`                                                                                                                                                                                                                         |
| `workspaces` | `create_workspace`（只能建当前项目的 worktree）                                                                                                                                                                                                                                                      |

- 永不授权（`HostOnly`）：`respond_to_permission`、`archive_workspace`、`rename_workspace`、`list_workspaces`；未登记的工具名同样按 `HostOnly` 处理。
- Bot 专属（`BotOnly`）：`bot_memory_read`、`bot_memory_update`、`workspace_overview`，只对 Bot 自己的凭据可见，子树不可见。
- `modes: { [provider]: string[] }` 是模式上限：至多 32 个 Provider，每个 1–16 个不重复的模式；列表第一项为默认模式；
  未列出的 Provider 不可由 Bot 或子树使用。Provider 与模式 id 的校验与 `model::valid_id` 等价（非空、不超过 128 字节、无控制字符），
  由 `domain` 内的私有检查实现，因为 `domain` 不能依赖 `model`（见 `bins/daemon/tests/dependencies.rs`）。
- 新 Bot 默认分组为 `{observe, agents, stop, schedules}`、模式上限为空。阶段 1／2 已存在的 Bot 读取时得到同样的默认分组，
  模式上限为空，因此在用户于设置中配置前不能启动任何 Provider 的 Agent。
- `bot.setup` 可携带 `permissions`；`bot.permissions.update.request` 以 `workspaceId` 与完整 `permissions` 整体替换；
  `BotView.permissions` 返回当前配置。访客只读，写入被拒绝。
  服务端不新增能力名：客户端以 `bot.permissions.update.request` 是否实现决定是否显示权限设置，对应客户端特性 `workspaceBotPermissions`。

### 2. 凭据注册表与依赖方向

- `provider` 定义端口 `McpScopes`／`McpAccess`（`Shared`、`Scoped`、`Denied`），`bot` 的 `mcp::Scopes` 实现，`api` 的 MCP 路由校验；依赖始终指向内层。
- 凭据为 64 个十六进制字符（两个 UUIDv4 `simple()` 拼接）。注册表只在 daemon 内存中保存 SHA-256 摘要，不保存明文；
  至多 4096 项，超出时淘汰最早签发的一项。
- 凭据不写入命令行、持久 record、原生持久 handle、日志、`Debug` 输出或 Workspace 文件。每次 daemon 启动都重新生成。
- 轮换与撤销：同一 Agent 再次打开会话时旧凭据作废；Bot 暂停、移除、被取代或不再是当前 Bot 时，Bot 凭据在目录发布时撤销；
  会话关闭、释放（Bot 每轮结束即 `Release`，下一轮重新连接）或归档时撤销；Provider 无法交付时立即撤销刚签发的凭据。
- Provider 绑定（`agent_mcp.rs`）三选一：`Shared`（沿用共享令牌，用户已有的 `ait` 项优先，受 `injectIntoAgents` 控制）；
  `Scoped`（作用域凭据，总是替换任何 `ait` 项）；`Strip`（移除 `ait` 项）。
  被拒绝的会话（`Denied`，例如 Bot 已不是当前 Bot）以及作用域凭据无法交付的会话（MCP 关闭、子树会话在 `injectIntoAgents` 关闭时、
  Provider 不支持 HTTP MCP 或会话不可交付）都使用 `Strip`，不回退到共享令牌。Bot 自己的会话不受 `injectIntoAgents` 控制，子树会话仍受其控制。
- 授权在**每次 MCP 请求**时按 Workspace 当前非 `retired` 的 Bot 读取（`Scopes::grant`）。设置变更在下一次请求生效；
  工具目录在 Bot 下一轮刷新。子树凭据在没有非 `retired` Bot 时降为只读观察（`{observe}`、空模式上限、作用域只含根 Workspace）。
  Bot 凭据只在其 Bot 为 `active` 且仍是该 Workspace 当前 Bot 时有效。
- 暂停的 Bot 仍是非 `retired` Bot（`Scopes::publish` 保留它的授权）：暂停期间只有 Bot 自身的凭据失效，子树 Agent 与 Bot 创建的定时任务
  仍按完整授权运行。移除（`retired`）后降为只读观察。授权按根 Workspace 读取，因此同一 Workspace 设置新 Bot 后，
  新 Bot 的授权适用于该 Workspace 已有的子树 Agent 与定时任务。
- 交付给原生客户端沿用 ADR-202：Codex 与 ACP 类 Provider 经 stdio 协议传递；Claude 经 transport 持有的属主专用（0600）临时 `--mcp-config` 文件传递，
  原生进程结束时删除。

### 3. 子树识别

Bot 通过 `create_agent` 创建的 Agent 带保留标签 `ait.bot.scope = <根 Workspace id>`（常量 `domain::bot::BOT_SCOPE_LABEL`），
其会话据此获得派生的子树凭据；后代 Agent 通过其所在会话同样继承。Bot 创建的 Agent 不记录 Bot 为父级。
创建的定时任务与心跳在记录中带 `botScope`（根 Workspace id），工具 schema 不允许调用者自己提供。
任何 MCP 调用者都不得写入 `ait.bot.` 前缀的标签（`create_agent`、`update_agent` 均拒绝），访客创建 Agent 时同样拒绝该前缀；
作用域调用者（Bot 与子树）不得写入任何 `ait.` 前缀的标签。
共享令牌调用若通过 `X-Ait-Agent-Id` 指向带该标签的 Agent，路由返回 `-32602`（必须使用其自己的凭据）。

### 4. 路由、作用域与模式上限

- 身份：作用域凭据的 `X-Ait-Agent-Id` 若存在必须与凭据所属 Agent 一致，否则 `-32602`。Bot 会话没有公开 Agent 记录，因此没有调用者上下文；
  子树永远以自身作为上下文。
- 过滤：未授权的工具不出现在 `tools/list`，`tools/call` 返回 JSON-RPC `-32602 "Unknown or unavailable tool"`。
  作用域或模式上限违规返回 `isError: true` 的工具结果，文本为 `ErrorCode::Forbidden` 的消息。未显式登记作用域规则的工具永不执行。
- 作用域：Bot 所在 Workspace，加上该 Bot 通过 `create_workspace` 创建且未归档的 worktree Workspace（持久化在 `Bot.workspaces`，至多 64 个，满额返回 `ResourceExhausted`）。
  Workspace 归档时 `Bots::retire` 同时从所有 `Bot.workspaces` 中移除其 id；路由校验目标时也要求 Workspace 未归档。
- 规范 id：所有目标（Agent、Workspace、终端、定时任务）经现有 RPC 解析为规范 id 并回写参数，后续 RPC 不会再把前缀或标题解析成别的目标。
  `cwd` 参数一律拒绝，目录由服务端提供；列表在来源处过滤到作用域内。
- 模式上限：`create_agent` 的 provider 与 `settings.modeId` 必须在上限内，未指定模式时取该 Provider 的默认模式；
  `set_agent_mode` 与 `update_agent` 按**目标 Agent 当前 Provider** 校验；`update_agent` 的 `settings.features` 只允许 `fast_mode`。
  上限约束 Bot／子树设置或创建的每一个模式，不论目标归属。
- `create_workspace`：只允许 `isolation: "worktree"`，不接受 `idempotencyKey`，只能建根 Workspace 所属项目的 worktree，创建后记入 `Bot.workspaces`。
- `create_heartbeat`：仅限子树调用者，目标为其自身；Bot 会话不能作为目标。
- 指挥范围（用户已确定）：`send_agent_prompt`、`update_agent`、`set_agent_mode` 可作用于作用域内的**任何** Agent（含用户启动的），
  `run_schedule_once`、`pause_schedule`、`resume_schedule`、`delete_schedule` 可作用于作用域内的任何定时任务（含用户创建的），只受已授权分组与模式上限限制。
  `update_schedule` 只能编辑带 `botScope` 的任务（Bot 或子树创建的）；用户创建的任务无论修改哪个字段都被拒绝，提示请用户在 Ait 中修改。

### 5. 定时任务

- 调用者创建的新 Agent 任务存储 `botScope`，以及该 Provider 默认模式（`modeId`）。
- 每次运行前（`bins/daemon/src/host/schedule.rs`）按当前授权复核：`schedules` 分组与 `mode(provider, Some(mode))`；不再允许时该次运行失败（任务保持 `active`）。
  Bot 创建而未存储模式的任务按上限的默认模式运行。
- 作用域调用者对任意任务执行 `run_schedule_once`、`resume_schedule` 前，目标任务当前的 Provider 与模式须在上限内。
  不是 Bot 创建且没有存储模式的用户任务会按 Provider 原生默认模式运行，上限无法担保，这些控制被拒绝并提示请用户先设置允许的模式。
- `update_schedule` 只接受 Bot 创建的任务，补丁后的配置须在上限内：补丁中的 `mode: null` 或只改 Provider 时会写入该 Provider 的默认模式，
  因此补丁后的任务总是存有允许的模式；补丁不改 Provider 与模式时按任务当前配置校验。
- Bot 创建的任务属于 Workspace 的管理者角色，`botScope` 记录根 Workspace 而不是某个 Bot。Bot 暂停期间这些任务照常运行；
  没有非 `retired` Bot 时（例如 Bot 已移除），每次运行都因授权降为只读观察而以 `REVOKED`（“Workspace Bot permissions no longer allow this schedule”）失败，
  任务保持 `active`；同一 Workspace 设置新 Bot 后，这些任务按新 Bot 的授权运行。

### 6. Bot 会话能力探测与降级

Bot 自身模型可使用任何 Provider。Provider 通过 `BotSessionSupport` 声明能力，并作为 `botSession` 字段发布在 Provider 快照中：
`instructions`（`SystemPrompt` 或 `TurnPrefix`）、`readOnlyMode`、`toolPreapproval`、`aitTools`。

- 指令投递：支持可更新系统提示的 Provider（Codex、Claude）写入 `system_prompt`；其他 Provider 把内置指令包在 `<ait-bot-instructions>` 中前置到**每一条**输入
  （`TurnPrefix`）：恢复的原生会话不一定保留之前前置的内容，因此不记录投递状态，也不按指令摘要跳过。
- 原生模式：有只读模式的 Provider 使用它（Codex `read-only`；Claude `default`，即仍把写入路由到审批的最严格模式，用户与项目的允许规则照常适用）；
  其他 Provider 使用其默认模式，需要审批的写入由 ADR-145 自动拒绝，Provider 不经审批执行的写入不受阻止。服务端在设置、更新与加载时把 `execution.modeId` 归一化为 `null`，
  旧记录在下一次保存目录时写回；因此以相同参数重新设置旧 Bot 仍返回 `created: false`。
- 工具预授权：只有 Codex、Claude 支持（`tool_policy` 精确预授权 Bot 可能被授予的每个 `ait` 工具，当前授权仍由 MCP 路由在每次请求时强制）。
  其他 Provider 上需要审批的 Ait 工具调用被自动拒绝；Provider 不支持 HTTP MCP 时没有 Ait 工具，Bot 只能对话。
- 已有会话无法达到安全配置时 Provider 返回 `resetRequired`，Bot 轮换对话（新 generation）并重试一次；仍无法达到则本轮失败。
- 已存在的 Bot 继续可用。ADR-145 第 4 条的模式改写（Codex `auto-review`、Claude `auto`）不再用于 Bot 会话，改为上述最严格模式选择。

### 7. Bot 专属工具与指令

- `bot_memory_read`、`bot_memory_update`（携带 revision，冲突时重读）读写 ADR-213 的记忆；`workspace_overview` 汇总作用域内 Workspace、Bot 启动的 Agent 与定时任务，输出上限 24 KiB。
- 内置指令说明已授予的工具分组；记忆被包在 `<workspace-memory>` 标签中并声明为用户数据，只能表达偏好，不能改变指令；记忆内的每个 `</` 被转义为 `<\/`，
  任何大小写或空白变体的关闭标签（包括 `TurnPrefix` 外层的 `</ait-bot-instructions>`）都无法提前结束围栏。

### 8. 生效时机与客户端

- 授权变更在下一次 MCP 请求生效；工具目录与指令在 Bot 下一轮刷新；执行配置仍在新 Conversation 或 reset 后生效。
- 客户端（向导与设置页）展示分组、每个 Provider 的模式上限与能力说明；模式上限部分与 `terminals` 分组下显示可读取本机凭据、绕过 Bot 限制的警告。
  快照中已不可用的 Provider 的上限项仍显示（标注不可用）以便移除；其他设备保存的权限变更会替换设置页的本地草稿。
  不支持 `bot.permissions.update.request` 的旧主机不显示权限设置，并保留 Bot 自身的模式选择。
  实验开关 `experimentalFeatures` 继续门控整个 Bot 客户端功能。

## 后果与已接受风险

作用域是 MCP 层的授权，不是沙箱。以下风险经用户确认接受：

1. **指挥范围超出 Bot 自身限制。** Bot 可以向其作用域内的**任何** Agent 发送提示、重新配置，包括用户启动的 Agent；
   也可在模式上限内运行（但不能编辑）用户创建的定时任务，这些任务启动的 Agent 使用共享令牌。
   用户启动的 Agent 使用共享令牌和各自的原生权限，向它们发送提示可能触及超出 Bot 自身限制的能力；模式上限只约束 Bot 与子树设置或创建的模式。
2. **子树原生权限不受限。** 允许运行命令的模式下，Agent 可以读取本机 daemon 凭据（包括 Claude 的临时 MCP 配置文件与 `local-api.token`）并绕过 Bot 的 Ait 限制。
   带 `botScope` 的定时任务每次运行按当前授权复核，Bot 运行或恢复模式超出上限的任务被拒绝，且只能编辑自己创建的任务；
   模式上限与设置页警告是缓解，不是隔离。
3. **任意 Provider 可作为 Bot 模型。** 没有只读模式或工具预授权的 Provider 上，Bot 分别可能执行无需审批的操作，或无法使用 Ait 工具；界面以能力说明告知。
4. **凭据在本机可被同用户读取。** 凭据只在 daemon 内存中签发和校验，但交付给 Claude 时经属主专用（0600）临时 MCP 配置文件，
   因此能在本机以同一用户读取文件的 Agent 可以读到运行中会话的凭据或 `local-api.token`。只读模式限制的是写入而不是读取：
   Codex 的 `read-only` 沙箱仍允许读取文件，所以即使以 Codex 为模型、处于 `read-only` 的 Bot 也能读取 `local-api.token` 与这些配置文件，
   没有只读模式的 Provider 上的 Bot 同样如此。这与第 2 项同属已接受风险。
5. **`terminals` 分组可运行任意命令。** 该分组允许 Bot 与子树创建终端并输入命令，与允许运行命令的模式同属一类：
   终端中的命令可读取本机 daemon 凭据并绕过 Bot 的 Ait 限制。设置页与向导在该分组下显示同样的警告。

后续：[让 daemon 令牌不在 Agent 可达范围内](../../plans/README.md)；阶段 4 在 `ait.bot.scope`／`botScope` 上增加来源展示与通知。

## 升级影响

旧 Bot 升级后得到默认分组但模式上限为空，不能启动 Agent，需在设置中配置。非 Codex／Claude 的 Bot 模型按能力降级，并在设置中说明。

## 验证

定向测试覆盖权限校验与工具表、凭据注册表（摘要、轮换、容量淘汰、撤销）、Provider 绑定与降级、MCP 路由的身份／过滤／作用域／模式上限／保留标签、
定时任务运行时复核、专属工具、协议与客户端权限界面，以及进程级端到端。按仓库规则只运行受影响 crate 与包的测试，不运行完整 Workspace 测试。

## Test coverage

未测量覆盖率：本次未请求覆盖率统计。
