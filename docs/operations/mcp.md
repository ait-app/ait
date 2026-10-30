# 使用 Ait daemon MCP

daemon 在现有监听地址提供 Streamable HTTP MCP：`http://127.0.0.1:7316/mcp/agents`。
客户端从运行时环境取得令牌并发送 `Authorization: Bearer <AIT_SERVER_TOKEN>`。

新配置中 `mcp.enabled` 和 `mcp.injectIntoAgents` 默认均为 `true`。
`mcp.enabled` 通过 `daemon.config.set.request` 修改即时生效，关闭时入口返回 404。
已有配置中的显式 `injectIntoAgents: false` 保持关闭，不会在升级时覆盖。
服务端边界见 [ADR-148](../decisions/daemon/adr-148-daemon-mcp.md)，自动注入见
[ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)。

## Provider 自动注入

daemon 为 Codex、Claude、DSH ACP 和 MiniMax Code 自动添加名为 `ait` 的 HTTP MCP 配置，使用本机实际
监听端口和 MCP 专用 Bearer 令牌。客户端完成协议握手、发现工具并把工具提供给模型；
调用自动携带 `X-Ait-Agent-Id`，模型无需填写调用者身份或令牌。专用令牌只被 MCP 路由接受，
不能用于 daemon 信息或 WebSocket 接口；外部客户端仍可使用原有 daemon 凭证。

配置只进入原生会话的运行时设置，不写入 Ait 的 Agent 配置、持久化 handle 或对外快照。
后续对话和原生配置更新校验沿用打开会话时的连接；daemon 重启恢复、导入及重新建立原生会话时重新取得当前
地址、身份和令牌。专用令牌每次 daemon 启动都会更换。只读历史会话不注入。

`injectIntoAgents` 控制后续原生会话的打开，不强制重建已有会话。已有用户配置中的 `ait`
项目优先，其余用户 MCP 项保留。OpenCode、DSH native、Pi、OMP 和 Antigravity
尚未接入这条自动注入路径，会保持原有启动行为。

已有配置为 `false` 时，可调用 RPC 方法 `daemon.config.set.request` 开启，参数如下；
随后创建 Agent 或重启 daemon 恢复会话：

```json
{
  "config": { "mcp": { "injectIntoAgents": true } }
}
```

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

## Unix socket 监听

macOS / Linux 可以额外启用一个 Unix domain socket，使用相同的 HTTP MCP 协议、工具目录、
Bearer 鉴权和 `mcp.enabled` 开关。默认不创建 socket；配置绝对文件路径后重启 daemon：

```sh
daemon --mcp-socket /absolute/private-directory/mcp.sock
```

也可设置 `AIT_SERVER_MCP_SOCKET`，或在 daemon 的 `config.toml` 中设置
`mcp_socket = "/absolute/private-directory/mcp.sock"`。优先级为 CLI、环境变量、TOML。
父目录必须存在，建议使用 daemon 数据目录或其他私有目录。

socket 文件权限为 `0600`，仅暴露 `/mcp/agents`。客户端通过 socket 连接，HTTP 地址固定使用
`http://localhost/mcp/agents`；`localhost` 是 HTTP Host，不会建立到本机 TCP 端口的连接：

```sh
curl --fail-with-body --unix-socket /absolute/private-directory/mcp.sock \
  http://localhost/mcp/agents \
  -H "Authorization: Bearer $AIT_SERVER_TOKEN" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-11-25' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

正常停止和重启会删除本次创建的 socket 文件。异常退出遗留的 socket 在确认无监听者后可自动
清理；普通文件、目录、符号链接和仍在监听的 socket 不会被覆盖。非 Unix 平台拒绝此配置。

客户端需要支持 HTTP over Unix socket。Provider 自动注入继续使用 TCP HTTP 地址；只接受
普通 HTTP URL 的客户端需通过本地代理或桥接接入，不能直接把 socket 文件路径作为 MCP URL。
监听与生命周期约定见 [ADR-203](../decisions/daemon/adr-203-mcp-unix-socket.md)。

## Skill 使用指引

daemon 内置并在启动时分发 [ait-orchestration Skill](../../skills/ait-orchestration/SKILL.md)，包含 Profile 选择、
Agent 编排、Workspace 隔离、终端与调度操作说明。默认允许自动发现；是否加载由原生 Provider 决定。
没有已连接 MCP 工具时，可参考 Skill 的 HTTP 调用说明，使用运行时提供的连接信息访问相同入口。
安装目录、更新和停用见 [Skill 自动分发](agent-skills.md)。Skill 提供工具使用指引；连接由上述运行时注入提供。

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
