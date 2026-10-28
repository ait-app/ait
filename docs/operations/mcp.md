# 使用 Ait daemon MCP

daemon 在现有监听地址提供 Streamable HTTP MCP：`http://127.0.0.1:7316/mcp/agents`。
客户端从运行时环境取得令牌并发送 `Authorization: Bearer <AIT_SERVER_TOKEN>`。

`<data-dir>/config.json` 的 `mcp.enabled` 默认 `true`；通过 `daemon.config.set.request`
修改即时生效，关闭时入口返回 404。MCP 客户端需显式配置连接；本次没有接入
`mcp.injectIntoAgents` 的 Provider 自动注入。实现边界见 [ADR-148](../decisions/daemon/adr-148-daemon-mcp.md)。

## 验证连接

使用已启动 daemon 的环境令牌发现工具：

```sh
curl --fail-with-body http://127.0.0.1:7316/mcp/agents \
  -H "Authorization: Bearer $AIT_SERVER_TOKEN" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-11-25' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

标准客户端先发送 `initialize` 和 `notifications/initialized`，再调用 `tools/list` / `tools/call`。
返回 JSON，不保存跨请求 session，也不提供独立 SSE 流。执行错误使用 `result.isError`；
无效 JSON-RPC 或参数使用 `error.code`。

daemon 内置并在启动时分发 [ait-orchestration Skill](../../skills/ait-orchestration/SKILL.md)，包含 Profile 选择、
Agent 编排、Workspace 隔离、终端与调度操作说明。默认允许自动发现；是否加载由原生 Provider 决定。
没有已连接 MCP 工具时，可参考 Skill 的 HTTP 调用说明，使用运行时提供的连接信息访问相同入口。
安装目录、更新和停用见 [Skill 自动分发](agent-skills.md)。Skill 分发尚未接入 Provider MCP 自动注入。

## 可用工具

| 能力               | 工具                                                                                                                                                                                            |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Agent              | `create_agent`, `send_agent_prompt`, `get_agent_status`, `list_agents`, `cancel_agent`, `archive_agent`, `kill_agent`, `update_agent`, `get_agent_activity`, `set_agent_mode`, `wait_for_agent` |
| Workspace          | `create_workspace`, `list_workspaces`, `rename_workspace`, `archive_workspace`                                                                                                                  |
| 脚本               | `list_workspace_scripts`, `start_workspace_script`, `stop_workspace_script`                                                                                                                     |
| 终端               | `list_terminals`, `create_terminal`, `kill_terminal`, `capture_terminal`, `send_terminal_keys`                                                                                                  |
| 定时任务           | `create_schedule`, `list_schedules`, `inspect_schedule`, `pause_schedule`, `resume_schedule`, `update_schedule`, `schedule_logs`, `run_schedule_once`, `delete_schedule`                        |
| 心跳               | `create_heartbeat`, `delete_heartbeat`                                                                                                                                                          |
| Profile / Provider | `list_profiles`, `list_providers`, `list_models`, `inspect_provider`                                                                                                                            |
| 权限               | `list_pending_permissions`, `respond_to_permission`                                                                                                                                             |

准确参数由 `tools/list` 的 `inputSchema` 定义。部分能力未安装时，对应工具不出现在目录中。
Bot 内部会话遵守已有可见性和权限规则。

## 编排流程

先用 `list_profiles` 读取保存的 profile 和 notes，用 `list_models` 确认模型。
外部创建 Agent 使用以下调用：

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "create_agent",
    "arguments": {
      "provider": "codex/<list_models 返回的模型 ID>",
      "title": "修复登录问题",
      "cwd": "/absolute/project/path",
      "initialPrompt": "修复登录问题并运行相关测试",
      "idempotencyKey": "login-fix-001"
    }
  }
}
```

创建和 prompt 在输入接纳后返回。用 `get_agent_status` / `get_agent_activity` 检查进展，
或用 `wait_for_agent` 等待，默认 30 秒、最多 45 秒。超时返回 `status: "timeout"`，
不会取消执行；通过 `cancel_agent` 显式取消。重试创建复用原 `idempotencyKey`，prompt 重试复用 `messageId`。

Agent 调用可发送 `X-Ait-Agent-Id: <agent-id>`，继承调用者 cwd 和 Workspace 并记录父子关系。
传入另一个 `workspaceId` 可以将子 Agent 放在另一个 Workspace。需要隔离时先
`create_workspace`，选择 `isolation: "worktree"`，再使用返回的 Workspace ID。
`mode` 支持 `branch-off`、`checkout-branch` 和 `checkout-pr`。

脚本位于 Workspace 的 `ait.json`，按 `scriptName` 启停。终端 `keys` 可以是 `Enter`、`C-c`
等令牌；`literal: true` 发送原样文本。

Schedule 归属于固定 Workspace；外部调用提供 `workspaceId`、`provider`，Agent 调用可以继承。
Heartbeat 需要调用者身份，定时向该 Agent 发送 prompt。Ait 的 Heartbeat 会持久化，完成后需
`delete_heartbeat`。`list_schedules` 与其余 Schedule 工具只操作 new-agent 任务。
