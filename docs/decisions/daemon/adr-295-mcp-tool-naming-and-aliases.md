---
id: ADR-295
status: accepted
date: 2026-11-21
amends:
  - daemon/adr-148-daemon-mcp.md
paths:
  - crates/api/src/mcp
  - crates/api/src/mcp.rs
  - crates/domain/src/bot/permissions.rs
  - crates/bot/src/instructions.rs
  - packages/protocol/src/paseo-tool-presentation
  - skills/ait-orchestration
---

# ADR-295：MCP 工具按“域在前”命名，旧名字作为别名保留

- 后续修订：[ADR-305](adr-305-cross-host-mcp-peer-sessions.md) 在域列表中增加 `host`（`host_list`，只对主机凭据的调用者）。

## 背景

[ADR-148](adr-148-daemon-mcp.md) 的 40 个工具沿用 Paseo 的“动词\_名词”（`create_agent`），之后加入的 Bot、工单与脚本运行工具
多为“名词在前”（`bot_memory_read`、`ticket_read`），目录里两套命名并存，无法按前缀归类。参数也不一致：标题有 `title` 与 `name`，
目标 id 有 `agentId` 与裸 `id`，模式有 `mode` 与 `modeId`。同一个工具名还要写进目录、访问类别表、作用域规则、分发、Bot 指令、
Skill 与客户端展示目录，改名不能一次切换：运行中的原生会话、Bot 写给工作者的提示、用户保存的允许清单和外部脚本都持有旧名字。

## 决策

- 工具名为 `<域>[_<子对象>]_<动作>`，动作取自固定词表（`create`、`list`、`get`、`update`、`delete`、`read`、`write`、`wait`、`start`、`stop`、
  `pause`、`resume`、`run`、`cancel`、`archive`、`rename`、`set`、`send`、`prompt`、`capture`、`kill`、`respond`、`report`）。
  域为 `agent`、`workspace`、`script`、`terminal`、`schedule`、`heartbeat`、`provider`、`profile`、`permission`、`notepad`、`bot`、`ticket`；
  `bot` 只对 Bot 会话，`ticket` 只对工单的工作者。`bot_overview` 是唯一以返回内容命名的工具。测试按词表核对每个名字。
- 参数约定：目标 id 为 `<域>Id`，不使用裸 `id`；Agent 与 Workspace 的显示名为 `title`，定时任务、终端与心跳为 `name`；模式为 `modeId`。
  目录用 `limit`/`cursor` 分页，文本用 `offset`/`limit` 分页。新工具可恢复的失败用 `accepted: false` 加 `error` 文本，并带上重试所需的字段。
  这次调整了 `agent_update`（`name`→`title`）、定时任务工具（`id`→`scheduleId`）、`schedule_update`（`mode`→`modeId`）与
  `heartbeat_delete`（`id`→`heartbeatId`）；工具结果的形状不变。
- 目录条目（`catalog.json`）增加 `title`、`aliases` 与 `destructive`。`tools/list` 的每个工具带 `title`，注解增加 `title` 与 `destructiveHint`
  （只有归档、终止、删除类工具为 `true`）。域不单独存储，由名字的首段得到。
- **别名。** `aliases` 列出旧名字及其旧参数名到新参数名的映射。`tools/list` 只返回新名字；`tools/call` 在授权判定、Schema 校验和
  作用域规则之前把别名解析为工具并改写旧参数（新参数已存在时以新参数为准），所以访问类别表、作用域规则与分发只认新名字，
  旧名字不比它的工具多授予任何权限。新名字不接受旧参数。
- 别名的每次调用计数，首次及每次翻倍时写一条 `retired MCP tool name called` 日志。别名不早于 2026-12-22 移除，且以日志不再出现调用为条件；
  移除时删除目录中的 `aliases` 与解析代码。
- `kill_agent` 并入 `agent_archive`：两者在执行层同样级联到子 Agent，并以 `AgentRuntimeDirectory::archive` 结束。经别名调用 `kill_agent`
  得到归档的结果形状（`agentId`、`archivedAt`），未知的 Agent 返回错误而不是 `success: false`。
- 新增 `heartbeat_list`：返回目标为调用者 Agent 的心跳。此前 `schedule_list` 排除心跳，Agent 丢失 id 后无法找回。
- `agent_mode_set` 与 `agent_update.settings.modeId` 暂不合并：两者对应不同的执行操作（`Mode` 与 `Configure`），作用域也分别校验模式上限。
- 客户端的工具展示目录同时识别新旧名字，旧名字长期保留以显示历史会话；未知名字按下划线转成标题。
  Claude 工作者的预授权由 `worker_tools()` 生成，随改名自动使用新名字。

## 后果

- 运行中的会话、已有提示和外部脚本在别名期内不受影响；`tools/list` 的消费者立刻看到新名字。
- 用户在自己的客户端里按旧名字保存的允许清单（如 Claude 的 `mcp__ait__create_agent`）无法迁移，需要自行改为新名字。
- 新增工具时只需在目录、访问类别表与作用域规则中登记一个名字；测试保证三者一致，不再写死工具数量。
- 记事本工具按本约定加入，见 [ADR-296](../workspace/adr-296-workspace-notepad.md)。

## 对照表

未列出的工具名字不变：`bot_memory_read`、`bot_memory_update`、`bot_ticket_update`、`bot_read`、`ticket_read`。

| 旧名字                      | 新名字                |
| --------------------------- | --------------------- |
| `create_agent`              | `agent_create`        |
| `send_agent_prompt`         | `agent_prompt`        |
| `get_agent_status`          | `agent_get`           |
| `get_agent_activity`        | `agent_activity_read` |
| `list_agents`               | `agent_list`          |
| `wait_for_agent`            | `agent_wait`          |
| `update_agent`              | `agent_update`        |
| `set_agent_mode`            | `agent_mode_set`      |
| `cancel_agent`              | `agent_cancel`        |
| `archive_agent`             | `agent_archive`       |
| `kill_agent`                | `agent_archive`       |
| `create_workspace`          | `workspace_create`    |
| `list_workspaces`           | `workspace_list`      |
| `rename_workspace`          | `workspace_rename`    |
| `archive_workspace`         | `workspace_archive`   |
| `list_workspace_scripts`    | `script_list`         |
| `start_workspace_script`    | `script_start`        |
| `stop_workspace_script`     | `script_stop`         |
| `get_workspace_script_run`  | `script_run_get`      |
| `wait_for_workspace_script` | `script_run_wait`     |
| `list_terminals`            | `terminal_list`       |
| `create_terminal`           | `terminal_create`     |
| `kill_terminal`             | `terminal_kill`       |
| `capture_terminal`          | `terminal_capture`    |
| `send_terminal_keys`        | `terminal_send`       |
| `create_schedule`           | `schedule_create`     |
| `list_schedules`            | `schedule_list`       |
| `inspect_schedule`          | `schedule_get`        |
| `update_schedule`           | `schedule_update`     |
| `pause_schedule`            | `schedule_pause`      |
| `resume_schedule`           | `schedule_resume`     |
| `run_schedule_once`         | `schedule_run`        |
| `schedule_logs`             | `schedule_log_read`   |
| `delete_schedule`           | `schedule_delete`     |
| `create_heartbeat`          | `heartbeat_create`    |
| `delete_heartbeat`          | `heartbeat_delete`    |
| `list_providers`            | `provider_list`       |
| `list_models`               | `provider_model_list` |
| `inspect_provider`          | `provider_get`        |
| `list_profiles`             | `profile_list`        |
| `list_pending_permissions`  | `permission_list`     |
| `respond_to_permission`     | `permission_respond`  |
| `workspace_overview`        | `bot_overview`        |
| `bot_tickets`               | `bot_ticket_list`     |
| `report_progress`           | `ticket_report`       |
