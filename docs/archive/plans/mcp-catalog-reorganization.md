# MCP 工具目录重组与 Workspace 记事本

**归档说明（2026-11-21）：** 本设计的命名规则、别名、参数约定、`heartbeat_list` 与 Workspace 记事本已实现，现行约定见
[ADR-295](../../decisions/daemon/adr-295-mcp-tool-naming-and-aliases.md)、[ADR-296](../../decisions/workspace/adr-296-workspace-notepad.md) 与
[MCP 手册](../../operations/mcp.md)。与设计的差别：目录条目没有单独的 `domain` 字段（域取自名字首段）；`workspace_list` 等列表的分页与
`agent_mode_set` 的合并未做；设计没有提到客户端已有的 notebook tab（`.ait-notebook.md`），两者的关系仍待决定。
未完成的事项见[未完成工作](../../plans/README.md)。正文保留设计时的表述，对照源码提交 `5451fe2`。

## 现状

daemon 在 `/mcp/agents` 提供一个无状态 Streamable HTTP 入口，目录为编译期嵌入的
[`catalog.json`](../../../crates/api/src/mcp/catalog.json)，共 50 个工具（[MCP 手册](../../operations/mcp.md#可用工具)）。
调用者分四类：主机凭据（daemon 令牌或共享 Agent MCP 令牌）、Bot 会话、Bot 子树 Agent、属于工单的工作者，
可见工具由 [`domain::bot::permissions::TOOLS`](../../../crates/domain/src/bot/permissions.rs) 的访问类别决定。

### 问题

1. **两套命名并存。** ADR-148 的 40 个工具沿用 Paseo 的“动词\_名词”（`create_agent`、`list_schedules`），
   之后加入的工具多为“名词在前”（`bot_memory_read`、`bot_tickets`、`ticket_read`、`workspace_overview`、`schedule_logs`），
   另有不带域名的 `report_progress`。同一目录里无法按前缀归类，也无法从名字推断下一个工具叫什么。
2. **名字与归属不符。** `workspace_overview` 只对 Bot 会话可见，却与只对主机可见的 `create_workspace`、
   `list_workspaces` 共用 `workspace` 一词；`report_progress` 实际写的是工单。
3. **参数不一致。**
   - 标题：`create_agent.title`，`update_agent.name`，`rename_workspace.title`，`create_schedule.name`。
   - 目标 id：Agent、Workspace、终端用 `agentId`、`workspaceId`、`terminalId`，定时任务与心跳只用 `id`。
   - Provider：`create_agent.provider` 是 `provider/model`，`update_schedule` 是分开的 `provider`、`model`、`mode`，
     `update_agent.settings` 用 `model`、`modeId`。
   - 分页：`list_agents`、`list_workspaces` 用 `limit`/`cursor`，`bot_read`、`ticket_read` 用 `offset`，
     `list_schedules`、`list_terminals`、`list_pending_permissions` 不分页。
4. **功能重叠。**
   - `archive_agent` 与 `kill_agent` 分别调用 `agent.archive.request` 与 `agent.items.close.request`，
     两者最终都落到 `AgentRuntimeDirectory::archive`（[`agent_runtime.rs`](../../../crates/provider/runtime/src/rpc/agent_runtime.rs)）。
   - `set_agent_mode` 与 `update_agent.settings.modeId` 都修改模式，走不同的 RPC（`agent.mode.set.request`、`agent.config.apply.request`）。
   - `inspect_schedule` 已包含运行历史，`schedule_logs` 再读一次运行与输出。
5. **缺口。**
   - `list_schedules` 排除心跳，`delete_heartbeat` 需要 id，Agent 丢失 id 后无法找回自己的心跳。
   - 同一 Workspace 的 Agent 之间没有被动的共享通道（见 [Workspace 记事本](#workspace-记事本)）。
6. **工具名分散在多张表里。** 同一个名字要同时出现在 `catalog.json`、访问类别表、
   [`scope::rule`](../../../crates/api/src/mcp/scope.rs)、[`tools::dispatch`](../../../crates/api/src/mcp/tools.rs) 及
   `overview`/`recall`/`tickets` 的分发、`BOT_ARGUMENTS`、[Bot 指令](../../../crates/bot/src/instructions.rs)、
   `initialize` 的 `instructions`、[ait-orchestration Skill](../../../skills/ait-orchestration/SKILL.md)、
   客户端的[工具展示目录](../../../packages/protocol/src/paseo-tool-presentation/catalog.ts)和文档中。
   现有测试只保证目录与访问类别表一致，并在两处写死数量 50。
7. **失败有三种写法。** 结果里的 `error`、`success: false`、`accepted: false` 都会被转成 `isError: true`。
8. **注解只有两个。** 每个工具只带 `readOnlyHint` 和固定为 `false` 的 `openWorldHint`，客户端无法区分“删除”与普通写入。

## 目标与非目标

目标：

- 一条命名规则覆盖全部工具，名字即分组，分组与 Bot 权限分组能对上。
- 相同含义的参数在所有工具里同名同形。
- 加入 Workspace 记事本，并让它从第一天起就符合新规则。
- 旧名字在迁移期继续可用，不让运行中的会话、已保存的提示和用户的允许清单立刻失效。

非目标：

- 不拆分入口，不改变传输、鉴权和无状态约定（ADR-148、ADR-202、ADR-203）。
- 不改变 Bot 的作用域、模式上限和工单规则（ADR-215、ADR-274、ADR-280、ADR-287），只改它们引用的名字。
- 不把多个操作合进一个带 `action` 参数的工具：访问类别、只读注解和作用域规则都是按工具名判定的，
  合并后暂停与编辑这类规则不同的操作无法分开授权。
- 不设计记事本的客户端界面，也不设计记事本的唤醒与投递（见[待决问题](#待决问题)）。

## 命名规则

`<域>[_<子对象>]_<动作>`，全小写下划线，动作在最后。

- 域：`agent`、`workspace`、`script`、`terminal`、`schedule`、`heartbeat`、`provider`、`profile`、`permission`、
  `notepad`、`bot`、`ticket`。域决定谁能看到它：`workspace`、`permission` 的写操作只对主机，
  `bot` 只对 Bot 会话，`ticket` 只对工作者，其余按 Bot 权限分组。
- 动作从固定词表里取：`create`、`list`、`get`、`update`、`delete`、`read`、`write`、`wait`、`start`、`stop`、
  `pause`、`resume`、`run`、`cancel`、`archive`、`rename`、`set`、`send`、`prompt`、`capture`、`kill`、`respond`、`report`。
  `get` 读一条结构化记录，`read` 读可分页的文本，`list` 读目录。

### 工具对照

访问类别沿用现有取值：`observe`、`agents`、`stop`、`schedules`、`scripts`、`terminals` 为 Bot 权限分组，
“主机”“Bot”“工作者”对应 `HostOnly`、`BotOnly`、`Worker`。主机凭据能看到除 Bot 与工作者之外的全部工具。

| 新名字                | 旧名字                        | 访问类别  | 其他变化                                         |
| --------------------- | ----------------------------- | --------- | ------------------------------------------------ |
| `agent_create`        | `create_agent`                | agents    |                                                  |
| `agent_prompt`        | `send_agent_prompt`           | agents    |                                                  |
| `agent_get`           | `get_agent_status`            | observe   |                                                  |
| `agent_activity_read` | `get_agent_activity`          | observe   |                                                  |
| `agent_list`          | `list_agents`                 | observe   |                                                  |
| `agent_wait`          | `wait_for_agent`              | observe   |                                                  |
| `agent_update`        | `update_agent`                | agents    | `name` 改为 `title`                              |
| `agent_mode_set`      | `set_agent_mode`              | agents    | 候选：并入 `agent_update`，见[合并](#合并与补缺) |
| `agent_cancel`        | `cancel_agent`                | stop      |                                                  |
| `agent_archive`       | `archive_agent`、`kill_agent` | stop      | `kill_agent` 只保留为别名，见[合并](#合并与补缺) |
| `workspace_create`    | `create_workspace`            | 主机      |                                                  |
| `workspace_list`      | `list_workspaces`             | 主机      |                                                  |
| `workspace_rename`    | `rename_workspace`            | 主机      |                                                  |
| `workspace_archive`   | `archive_workspace`           | 主机      |                                                  |
| `script_list`         | `list_workspace_scripts`      | observe   |                                                  |
| `script_start`        | `start_workspace_script`      | scripts   |                                                  |
| `script_stop`         | `stop_workspace_script`       | scripts   |                                                  |
| `script_run_get`      | `get_workspace_script_run`    | observe   |                                                  |
| `script_run_wait`     | `wait_for_workspace_script`   | observe   |                                                  |
| `terminal_list`       | `list_terminals`              | observe   |                                                  |
| `terminal_create`     | `create_terminal`             | terminals |                                                  |
| `terminal_kill`       | `kill_terminal`               | terminals |                                                  |
| `terminal_capture`    | `capture_terminal`            | terminals |                                                  |
| `terminal_send`       | `send_terminal_keys`          | terminals |                                                  |
| `schedule_create`     | `create_schedule`             | schedules |                                                  |
| `schedule_list`       | `list_schedules`              | observe   |                                                  |
| `schedule_get`        | `inspect_schedule`            | observe   | `id` 改为 `scheduleId`，以下同                   |
| `schedule_update`     | `update_schedule`             | schedules | `mode` 改为 `modeId`                             |
| `schedule_pause`      | `pause_schedule`              | schedules |                                                  |
| `schedule_resume`     | `resume_schedule`             | schedules |                                                  |
| `schedule_run`        | `run_schedule_once`           | schedules |                                                  |
| `schedule_log_read`   | `schedule_logs`               | observe   |                                                  |
| `schedule_delete`     | `delete_schedule`             | schedules |                                                  |
| `heartbeat_create`    | `create_heartbeat`            | schedules |                                                  |
| `heartbeat_list`      | 无                            | observe   | 新增，只列调用者自己的心跳                       |
| `heartbeat_delete`    | `delete_heartbeat`            | schedules | `id` 改为 `heartbeatId`                          |
| `provider_list`       | `list_providers`              | observe   |                                                  |
| `provider_model_list` | `list_models`                 | observe   |                                                  |
| `provider_get`        | `inspect_provider`            | observe   |                                                  |
| `profile_list`        | `list_profiles`               | observe   |                                                  |
| `permission_list`     | `list_pending_permissions`    | observe   |                                                  |
| `permission_respond`  | `respond_to_permission`       | 主机      |                                                  |
| `notepad_list`        | 无                            | observe   | 新增，见 [Workspace 记事本](#workspace-记事本)   |
| `notepad_read`        | 无                            | observe   | 新增                                             |
| `notepad_write`       | 无                            | notepad   | 新增；`notepad` 为新的 Bot 权限分组，默认开启    |
| `notepad_delete`      | 无                            | notepad   | 新增                                             |
| `bot_overview`        | `workspace_overview`          | Bot       |                                                  |
| `bot_memory_read`     | 不变                          | Bot       |                                                  |
| `bot_memory_update`   | 不变                          | Bot       |                                                  |
| `bot_ticket_list`     | `bot_tickets`                 | Bot       |                                                  |
| `bot_ticket_update`   | 不变                          | Bot       |                                                  |
| `bot_read`            | 不变                          | Bot       |                                                  |
| `ticket_report`       | `report_progress`             | 工作者    |                                                  |
| `ticket_read`         | 不变                          | 工作者    |                                                  |

结果：50 个旧工具中 5 个名字不变，44 个改名，1 个（`kill_agent`）并入；新增 5 个，目录共 54 个工具。

### 合并与补缺

- **`kill_agent` 并入 `agent_archive`。** 两者都以 `AgentRuntimeDirectory::archive` 结束。实施前需确认执行层
  对 `agent.items.close.request` 与 `agent.archive.request` 的包装没有调用者依赖的差别（例如对正在运行的轮次的处理）；
  若有差别，保留为 `agent_kill`。
- **`agent_mode_set` 并入 `agent_update` 是候选，不在改名阶段做。** 两条 RPC 对应不同的执行操作（`Mode` 与 `Configure`），
  Bot 作用域也分别用 `AgentMode` 与 `UpdateAgent` 两条规则校验上限。只有确认两者对运行中会话的生效时机相同，才合并。
- **`schedule_get` 与 `schedule_log_read` 保持分开。** 前者返回配置与运行历史，后者带 `limit` 读取输出，合并会让配置读取变重。
- **新增 `heartbeat_list`。** 返回目标为调用者的心跳（经 `X-Ait-Agent-Id` 或子树凭据确定调用者），补上找回 id 的途径。

## 参数与结果约定

新工具必须遵守；已有工具在改名的同一次变更中调整，旧形状只在别名下继续接受（见[兼容与迁移](#兼容与迁移)）。

| 含义             | 约定                                                                                               |
| ---------------- | -------------------------------------------------------------------------------------------------- |
| 目标 id          | `<域>Id`：`agentId`、`workspaceId`、`terminalId`、`scheduleId`、`heartbeatId`；不使用裸 `id`       |
| 显示名           | Agent 与 Workspace 用 `title`；定时任务、终端、心跳用 `name`（与界面一致）                         |
| Provider 与模型  | 创建时用 `provider`（`provider/model`）；修改设置用 `model`、`modeId`、`thinkingOptionId`          |
| Workspace 默认值 | 省略 `workspaceId` 时取调用者 Agent 的 Workspace；没有调用者身份的外部调用必须显式提供             |
| 目录分页         | `limit`、`cursor` 入参，结果带 `nextCursor`；无上限的列表（定时任务、终端、待处理权限）补上分页    |
| 文本分页         | `offset`、`limit` 入参，结果带 `nextOffset`；被截断的文本为 `{preview, chars, read}`（ADR-280）    |
| 等待             | `timeoutMs`，默认 30000，最大 45000；超时不取消被等待的工作                                        |
| 重试             | 创建类工具接受 `idempotencyKey`；写入类工具用 `expected` 携带读到的修订号                          |
| 失败             | 新工具只用 `accepted: false` 加 `error` 文本表示可恢复的失败，并带上重试所需的字段（如当前修订号） |

目录元数据的调整：

- `catalog.json` 每个条目增加 `domain`（用于文档与测试按域核对前缀）、`aliases`（旧名字）和 `destructive`。
- `tools/list` 的注解增加 `title` 与 `destructiveHint`：归档、终止、删除类工具标为破坏性。
- 增加测试：每个目录工具都在访问类别表和 `scope::rule` 中登记，名字前缀等于 `domain`；去掉写死的数量断言。
- `initialize` 的 `instructions`、Bot 指令与 Skill 中的名字由同一次变更更新。

## Workspace 记事本

### 定位

记事本是每个 Workspace 一份的共享便签集，供同一 Workspace 的 Agent 互相留下信息：约定的接口、
已经做出的决定、正在占用的文件、留给别人的问题和答复。它是被动的：写入不会唤醒任何 Agent，读取由 Agent 自己决定。

现有通道都不覆盖这个用途：

| 通道             | 方向                  | 是否唤醒对方   | 不适合的原因                                        |
| ---------------- | --------------------- | -------------- | --------------------------------------------------- |
| `agent_prompt`   | 点对点                | 是，开启新一轮 | 打断对方；Bot 作用域内还会重新打开工单              |
| `ticket_report`  | 工作者到 Bot          | 是（进度除外） | 只有纵向；同级工作者互相看不到，非工单 Agent 不可用 |
| `bot_memory_*`   | 仅 Bot                | 否             | 存放用户的长期偏好，只对 Bot 会话可见               |
| 检出目录中的文件 | 任意                  | 否             | 进入 Git 工作区，会被提交、被重置清除或与改动冲突   |
| 记事本（本设计） | 同一 Workspace 内任意 | 否             |                                                     |

### 模型

- 一个 Workspace 一份记事本，按 `workspaceId` 寻址。托管 worktree 是独立的 Workspace，各有自己的记事本。
- 记事本由若干**便签**组成，便签用调用者选择的 `key` 寻址（1–64 个字符，`[a-z0-9-]`，如 `api-contract`、`questions`）。
  选用可读的 `key` 而不是服务端 id，是为了让 Agent 在上下文被压缩后仍能凭名字找回便签，并让重复的创建自然落到同一条。
- 便签字段：`key`、`text`、`revision`（从 1 起的整数，每次写入加 1）、`createdBy`、`updatedBy`、`createdAt`、`updatedAt`。
- 记事本自身有整数 `revision`，任何便签的创建、修改、删除都使它加 1；每条便签记录自己最后一次变化时的记事本修订号，
  用于“自某次读取以来改了什么”。
- 作者为 `{kind, agentId?, title?}`，`kind` 取 `agent`、`bot`、`user`、`external`；`title` 是写入时的 Agent 标题快照。
- 上限（初值，可在实施时调整）：每个 Workspace 64 条便签，每条 `text` 16 KiB，不允许 NUL。
  达到上限时写入失败并提示合并或删除旧便签，不自动淘汰。

### 工具

| 工具             | 参数                                                | 行为                                                                                                                          |
| ---------------- | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `notepad_list`   | `workspaceId?`、`since?`                            | 返回记事本 `revision` 与全部便签的摘要（`key`、作者、时间、`revision`、`chars`、开头预览）；带 `since` 时只返回此后变化的便签 |
| `notepad_read`   | `key`、`workspaceId?`、`offset?`、`limit?`          | 返回一条便签的全文与元数据，按文本分页约定续读                                                                                |
| `notepad_write`  | `key`、`text`、`mode?`、`expected?`、`workspaceId?` | 创建、替换或追加，见下                                                                                                        |
| `notepad_delete` | `key`、`expected`、`workspaceId?`                   | 修订号相符时删除                                                                                                              |

`notepad_write` 的规则：

- `mode: "replace"`（默认）：便签不存在时创建，此时不得带 `expected`；已存在时必须带等于当前 `revision` 的 `expected`，
  否则返回 `accepted: false`、当前 `revision` 与 `updatedBy`，调用者重新读取后再决定。这与 `bot_memory_update` 的做法一致，
  避免两个 Agent 互相覆盖。
- `mode: "append"`：在末尾追加一段，不需要 `expected`，便签不存在时创建。追加的段落前由服务端写入一行作者与时间，
  多个 Agent 可以在同一条便签里问答而不发生冲突，也不必自己署名。超过单条上限时失败。
- 结果返回 `key`、便签 `revision` 与记事本 `revision`。

任何能写记事本的调用者都能修改和删除任何便签：记事本是共享的，这些 Agent 本来就能修改同一个检出目录；
`expected` 防止的是无意覆盖，不是权限隔离。

### 身份与权限

- **主机凭据。** `workspaceId` 省略时取 `X-Ait-Agent-Id` 指向的 Agent 的 Workspace；外部调用必须显式提供。
  显式提供时可以读写任意未归档 Workspace 的记事本，与其他以 Workspace 为目标的主机工具一致。
  作者取自 `X-Ait-Agent-Id`，没有时为 `external`。该请求头不是权限边界（ADR-148、ADR-215），所以共享令牌下的署名只是说明，不能当作身份证明。
- **Bot 会话与子树 Agent。** 读取属于 `observe`；写入与删除属于新的 `notepad` 分组，默认开启，用户可以关闭。
  目标必须在授权的 Workspace 内；`workspaceId` 省略时子树 Agent 取自己的 Workspace，Bot 会话取根 Workspace
  （需要一条新的作用域规则，现有的 `RootWorkspace` 总是回落到根）。作者取自凭据：Bot 会话为 `bot`，子树为该 Agent。
  没有非退役 Bot 时子树凭据降为只读，记事本随之只读。
- **受限于外部对话的调用者。** 其他 Channel 的 Bot 对话及其工作者只能触达自己工单的 Agent（ADR-280 第 4 节）。
  记事本是整个 Workspace 共享的，会让不同对话的内容互相可见，所以这类调用者在第一版中不能使用记事本：
  工具返回与其他受限操作相同风格的引导文本。所有者对话及其工作者不受影响。
- **访客。** 访客策略对未登记的 RPC 方法默认拒绝（[`guest/policy.rs`](../../../crates/api/src/guest/policy.rs)），
  记事本方法在第一版不登记，访客不可读写。

### 信任与安全

- 便签是其他 Agent 写的数据，不是指令。一个读过恶意网页或 Issue 的 Agent 可能把诱导内容写进记事本，
  记事本因此是 Agent 之间传播提示注入的通道。工具描述、Skill 与 Bot 指令都要写明这一点，
  读取结果把正文放在带作者的字段里返回，做法与工单跟随拉取请求时对 Forge 文本的处理一致（ADR-287）。
- 记事本对 Workspace 内所有 Agent、Bot 和客户端用户可见，工具描述要求不写入凭据与密钥。
- 记事本存放在 daemon 数据目录，不在检出目录中：不进入 Git，不受 `checkout.reset_workspace.request` 影响，
  沙箱化 worktree 的写入限制也不需要为它放开路径（ADR-215、ADR-223）。

### 存储、RPC 与生命周期

- 新增 `workspace.notepad.list.request`、`workspace.notepad.get.request`、`workspace.notepad.write.request`、
  `workspace.notepad.delete.request`，归 `metadata`（全部 `workspace.*` 方法归它，ADR-130）。MCP 工具只做参数翻译、
  调用者解析与作用域收束，不持有数据；工具仅在这些方法已安装时出现在目录中（现有机制）。
- 作者由 API 层解析后传入：MCP 调用按上一节确定，客户端直接调用 RPC 时为 `user`。
- 持久化到数据目录中的 SQLite，daemon 重启后保留（与创建回执的做法一致，ADR-294）。
- Workspace 归档时删除其记事本；记事本不随 worktree 的创建复制，也不跨 Workspace 合并。
- 客户端订阅用的变更事件与界面不在本设计范围内，RPC 的形状应允许之后增加。

### 让 Agent 知道它

- `initialize` 的 `instructions` 增加一句：同一 Workspace 的 Agent 用记事本共享信息，开始工作前先 `notepad_list`。
- [ait-orchestration Skill](../../../skills/ait-orchestration/SKILL.md) 增加一节：委派多个 Agent 时在提示里约定便签的 `key` 与各自负责的内容。
- Bot 指令在授予 `notepad` 分组时增加一句，`bot_overview` 增加记事本的修订号与便签数量。

### 考虑过的方案

- **整份文本加修订号（同 Bot 记忆）。** 实现最简单，但所有写入都争用同一个修订号，Agent 越多冲突越多，也没有署名。
- **只追加的消息流。** 没有冲突且署名清楚，但过时的内容无法更正，读者必须读完整个历史才能知道当前结论，体积也没有自然上限。
- **按 `key` 的便签加替换与追加两种写入（采用）。** 冲突限制在单条便签内，结论可以更正，问答可以追加，总量有上限。

## 兼容与迁移

工具名被以下各方持有，改名不能一次切换：

- 运行中的原生会话在打开时发现工具，升级前打开的会话仍持有旧名字，直到重新打开。
- Bot 写给工作者的提示里带有 `report_progress` 等名字，已在运行的工作者会继续按旧名字调用。
- 用户在自己的客户端里保存的允许清单（如 Claude 的 `mcp__ait__create_agent`）、定时任务的提示、Profile 备注和 Bot 记忆中的自由文本。
- 通过 HTTP 直接调用的外部脚本。
- 客户端的工具展示目录按名字匹配，客户端与 daemon 的版本可以不同，历史会话里也保留着旧名字。

做法：

1. **别名。** `catalog.json` 条目的 `aliases` 列出旧名字。`tools/list` 只返回新名字；`tools/call` 在授权判定、
   Schema 校验和作用域规则之前把别名解析为规范名字，所以其余各表只认新名字。
2. **别名携带旧参数形状。** 改了参数的工具（`name`→`title`、`id`→`scheduleId`、`mode`→`modeId`）与并入的工具
   （`kill_agent`）在别名下先把旧参数改写成新形状再校验。改写集中在一个模块里，迁移期结束时整体删除。
3. **记录别名的使用。** 按别名计数并写入诊断日志，用来决定何时移除。
4. **客户端展示目录同时保留新旧名字。** 旧名字需要长期保留以显示历史会话；未知名字已有按下划线转成标题的回退，
   旧客户端连接新 daemon 时只是失去定制的标签与摘要。
5. **预授权跟随改名。** Claude 工作者的预授权由 `worker_tools()` 生成，改名后新打开的会话自动使用新名字；
   用户自己的允许清单无法迁移，需要在 CHANGELOG 中说明。

## 实施顺序

1. **决策。** 接受命名规则、别名期限与记事本契约的 ADR。
2. **记事本。** 它是纯新增，直接使用 `notepad_*` 名字，不依赖改名，可以先交付：RPC 与存储、四个工具、
   `notepad` 分组与作用域规则、指令与 Skill、手册。
3. **目录元数据与别名机制。** 增加 `domain`、`aliases`、`destructive` 字段、别名解析与一致性测试，此时所有名字不变。
4. **改名与参数调整。** 一次变更完成目录、访问类别表、作用域规则、分发、Bot 指令、Skill、客户端展示目录、
   手册和 [ait-web 文档](https://github.com/ait-app/ait-web)的更新，旧名字全部转为别名。新增 `heartbeat_list`。
5. **合并候选。** 确认行为后处理 `agent_mode_set`，并为无上限的列表补分页。
6. **移除别名。** 期限到达且使用计数归零后删除别名与参数改写模块。

每个阶段可以独立发布；第 2 阶段与第 3–4 阶段互不依赖。

## 待决问题

1. **别名保留多久。** 发布节奏接近每天一版，按版本数计没有意义。建议至少保留 30 天，并以使用计数归零为移除条件。
2. **是否接受“名词在前”。** 另一种选择是保留 Paseo 的“动词\_名词”，只把 8 个名词在前的工具改过去
   （`read_bot_memory`、`list_bot_tickets`、`read_ticket` 等）。改动更小，但 Bot 与工单工具的名字已经写进运行中的工作者提示，
   两种选择都需要别名；“名词在前”的收益是前缀即分组。
3. **记事本的范围是否只限单个 Workspace。** 常见的编排方式是给每个子 Agent 一个独立的 worktree，
   它们不在同一个 Workspace，默认读不到彼此的记事本。按本设计，主机凭据可以显式传另一个 `workspaceId`，
   Bot 子树可以访问授权范围内的 Workspace。是否需要 Project 级的记事本，取决于这种用法是否足够。
4. **记事本是否需要投递。** 第一版只能主动读取，提问的 Agent 要靠对方下次读取或由编排者用 `agent_prompt` 提醒。
   可选的后续做法是在便签上标注收件 Agent，并在它空闲时作为新输入投递；这会引入轮次消耗与互相唤醒的循环，需要单独设计。
5. **是否记录每个 Agent 的已读位置。** 目前由调用者自己保存 `since`，上下文压缩后会丢失，只能重新读取全部摘要（有上限，成本可接受）。
   服务端记录已读位置可以直接返回“未读”，代价是多一份按 Agent 的状态。
6. **受限于外部对话的调用者是否需要自己的记事本。** 第一版直接拒绝；若需要，可按对话隔离。
7. **用户写入的便签是否有更高的权威。** 作者 `kind` 为 `user` 时可以区分，但是否让 Agent 把它当作指令需要在界面设计时决定。

## 需要的决策

以下变化达到 ADR 门槛，实施前各需一份：

- **MCP 工具命名、别名与参数约定**（修订 [ADR-148](../../decisions/daemon/adr-148-daemon-mcp.md)）：跨组件契约，
  约束 `crates/api`、`crates/domain`、`crates/bot`、`packages/protocol` 与 `skills`。
- **Workspace 记事本**：新的 `workspace.notepad.*` 方法、存储格式、作者身份的来源，
  以及新的 Bot 权限分组和对受限调用者的拒绝（修订 [ADR-215](../../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)、
  [ADR-280](../../decisions/bot/adr-280-bot-recall-reads-and-spill.md)）。

实施时同步更新 [MCP 手册](../../operations/mcp.md)、[Workspace Bot 手册](../../operations/bots.md#权限)、
[Workspace 规格](../../architecture/workspace.md)和 [Bot 规格](../../architecture/bot.md)。
