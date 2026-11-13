# 使用 Ait daemon MCP

daemon 在现有监听地址提供 Streamable HTTP MCP：`http://127.0.0.1:7316/mcp/agents`。
外部客户端使用 daemon 的本地 API token，发送 `Authorization: Bearer <token>`。
该 token 来自显式 `AIT_SERVER_TOKEN` 或自动保存的 `<data-dir>/local-api.token`，见[daemon 配置](daemon.md#启动与配置)。

新配置中 `mcp.enabled` 和 `mcp.injectIntoAgents` 默认均为 `true`。
`mcp.enabled` 通过 `daemon.config.set.request` 修改即时生效，关闭时入口返回 404。
已有配置中的显式 `injectIntoAgents: false` 保持关闭，不会在升级时覆盖。
服务端边界见 [ADR-148](../decisions/daemon/adr-148-daemon-mcp.md)，自动注入见
[ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)。

## Provider 自动注入

daemon 为 Codex、Claude、DSH（ACP 与原生 Host）、MiniMax Code、OpenCode 和 Pi 自动添加名为 `ait` 的 HTTP MCP 配置，使用本机实际
监听端口和 MCP 专用 Bearer 令牌。客户端完成协议握手、发现工具并把工具提供给模型；
调用自动携带 `X-Ait-Agent-Id`，模型无需填写调用者身份或令牌。专用令牌只被 MCP 路由接受，
不能用于 daemon 信息或 WebSocket 接口；外部客户端仍可使用原有 daemon 凭证。

配置只进入原生会话的运行时设置，不写入 Ait 的 Agent 配置、持久化 handle 或对外快照。
后续对话和原生配置更新校验沿用打开会话时的连接；daemon 重启恢复、导入及重新建立原生会话时重新取得当前
地址、身份和令牌。专用令牌每次 daemon 启动都会更换。只读历史会话不注入。

`injectIntoAgents` 控制后续原生会话的打开，不强制重建已有会话。已有用户配置中的 `ait`
项目优先，其余用户 MCP 项保留。OMP 和 Antigravity 尚未接入这条自动注入路径：两者只从
用户或项目配置文件读取 MCP，没有已确认的单会话、不写文件的入口，会保持原有启动行为。

各 Provider 的交付方式（[ADR-232](../decisions/providers/adr-232-agent-mcp-session-delivery.md)）：

| Provider      | 交付方式                                                                                                                                       | 凭据位置                                            |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| OpenCode      | ACP `session/new`、`session/load`、`session/resume` 的 `mcpServers`；每个 `opencode acp` 进程自带私有 server，运行时添加，不写 `opencode.json` | 仅协议消息                                          |
| Pi            | 每个原生进程以 `--extension` 加载私有扩展，扩展调用 `pi.registerMcpServer`，不写 `mcp.json`                                                    | 属主专用临时目录（0700，文件 0600），进程结束后删除 |
| DSH 原生 Host | 每个会话的私有 Web Host 以 `--patch` 叠加层插入 `dsh-mcp-client` 插件，不写 profile 的 `cordis.patch.yml`                                      | 属主专用临时文件（0600），Host 结束后删除           |

这三个 Provider 的会话在打开时连接 MCP；运行中的会话拒绝修改 `mcpServers`，需新建会话。
原生配置文件中同名的 `ait` 项按原生规则处理：Pi 的 `mcp.json` 同名项优先于注册项；DSH profile 中
`serverName` 为 `ait` 的插件会与注入项冲突；OpenCode 的运行时添加会替换同名项。

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

父目录须为当前用户拥有的真实目录（不是符号链接），权限为 `0700`，否则启动失败。
这保证 socket 从创建开始就不可被其他用户访问；可先运行 `chmod 700 /absolute/private-directory`。
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

## Bot 与子树凭据

Workspace Bot 的隐藏会话，以及 Bot 通过 `create_agent` 启动的 Agent（带保留标签 `ait.bot.scope`），不使用共享令牌，而使用 daemon 签发的作用域凭据，
会替换用户配置的 `ait` 项。Bot 会话不受 `injectIntoAgents` 控制，子树会话仍受其控制；无法交付凭据时移除 `ait` 项，不回退到共享令牌。
凭据为 64 个十六进制字符，只在 daemon 内存中以 SHA-256 摘要保存，每次启动重新生成；同一 Agent 重新打开会话、Bot 暂停／移除／被取代、
会话关闭或归档时作废。Claude、Pi 和 DSH 原生 Host 经属主专用（0600）临时文件取得凭据，原生进程结束时删除；允许运行命令的 Agent 仍可能读到它，见 [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 的已接受风险。

- 调用携带的 `X-Ait-Agent-Id` 若存在必须与凭据所属 Agent 一致，否则 `-32602`。共享令牌通过该头指向带 `ait.bot.scope` 的 Agent 同样被拒绝，该 Agent 必须使用自己的凭据。
- 每次请求按 Workspace 当前 Bot 的授权过滤：未授权工具不出现在 `tools/list`，调用返回 `-32602 "Unknown or unavailable tool"`；
  作用域或模式上限违规返回 `isError: true` 的 `Forbidden` 结果。
- 目标限于 Bot 的作用域（Bot 所在 Workspace 与它创建的未归档 worktree），目标 id 解析为规范 id 后再执行；`cwd` 参数被拒绝。
- `create_agent`、`set_agent_mode`、`update_agent`、`create_schedule`、`update_schedule` 的 Provider 与模式须在 Bot 的模式上限内；
  `update_schedule` 只能编辑带 `botScope` 的定时任务。
- 任何调用者都不能写入 `ait.bot.` 前缀的标签，作用域调用者不能写入任何 `ait.` 前缀的标签。作用域调用者不能使用 `create_workspace`。
- 没有非退役 Bot 时子树凭据降为只读观察。
- 工单（[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md)）：Bot 会话的每次 `create_agent` 打开一个工单，或以可选参数 `ticketId`
  （只在 Bot 会话看到的输入 schema 中，其他调用者传入时 `-32602`）加入已有工单；Agent 带 daemon 写入的标签 `ait.bot.ticket`，结果带 `ticketId`，
  其子 Agent 继承同一工单。达到 32 个未关闭工单时先自动关闭 Agent 全部已归档的工单（包括等待中的）。`bot_tickets` 列出工单，`bot_ticket_update` 关闭、重新打开、改名或备注；
  Bot 会话的 `send_agent_prompt` 会重新打开目标 Agent 已关闭的工单，达到上限而不能重新打开时结果带 `ticketNote`。
  属于工单的 Agent 用 `report_progress`（`kind` 为 `progress`、`blocked`、`done`、`failed`，`text` 必填）汇报，工单与 Agent 取自凭据；`blocked`、`done`、`failed` 唤醒 Bot。
  Claude 工作者的会话预授权 `report_progress`（只此一个工具，持久配置不变），不再为它请求审批；Codex 与其他 Provider 的工作者没有预授权，
  在会请求审批的模式中调用 `report_progress` 可能先请求审批，见 [Workspace Bot](bots.md#任务)。
- 作用域 `create_agent` 在单独的任务中执行：60 秒调用超时只停止等待，Agent 创建与工单收尾照常完成；该任务占用调用额度直到结束，至多 5 分钟。

分组与工具对应关系见 [Workspace Bot](bots.md#权限)。

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
| 脚本               | `list_workspace_scripts`, `start_workspace_script`, `stop_workspace_script`, `get_workspace_script_run`, `wait_for_workspace_script`                                                            |
| 终端               | `list_terminals`, `create_terminal`, `kill_terminal`, `capture_terminal`, `send_terminal_keys`                                                                                                  |
| 定时任务           | `create_schedule`, `list_schedules`, `inspect_schedule`, `pause_schedule`, `resume_schedule`, `update_schedule`, `schedule_logs`, `run_schedule_once`, `delete_schedule`                        |
| 心跳               | `create_heartbeat`, `delete_heartbeat`                                                                                                                                                          |
| Profile / Provider | `list_profiles`, `list_providers`, `list_models`, `inspect_provider`                                                                                                                            |
| 权限               | `list_pending_permissions`, `respond_to_permission`                                                                                                                                             |
| Workspace Bot      | `workspace_overview`, `bot_memory_read`, `bot_memory_update`, `bot_tickets`, `bot_ticket_update`（只对 Bot 会话）；`report_progress`（只对属于工单的 Agent）                                    |

准确参数由 `tools/list` 的 `inputSchema` 定义。部分能力未安装时，对应工具不出现在目录中。
Bot 会话与其启动的 Agent 只能看到已授权的工具，见上文“Bot 与子树凭据”。

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
      "provider": "<provider ID>",
      "model": "<list_models 返回的完整模型 ID>",
      "title": "修复登录问题",
      "cwd": "/absolute/project/path",
      "initialPrompt": "修复登录问题并运行相关测试",
      "idempotencyKey": "login-fix-001"
    }
  }
}
```

`create_agent` 优先使用独立的 `provider` 和 `model` 字段；两种格式均去除 Provider 与模型 ID 的首尾空白，保留模型 ID 内部的命名空间和斜杠。
兼容旧的 `provider/model` 合并值，此时省略 `model` 或设为 `null`；两种格式不能混用，也不自动选择默认模型。
`create_schedule` 继续使用合并的 `provider/model`，不接受独立 `model` 字段；`update_schedule` 的独立字段遵循下述补丁语义。

`update_schedule` 使用补丁语义：省略 `model` 且 Provider 不变时保留已存模型；切换 Provider 且未指定模型时清除旧模型，使用新 Provider 默认值（若 `provider` 使用合并格式，则以其中的模型更新）。
宿主切换 Provider 时必须显式提供 `mode`，否则拒绝保存；`mode: null` 显式选择新 Provider 原生默认模式。Bot 未指定模式时使用权限上限允许的新 Provider 默认模式。
非空字符串替换模型（空串和纯空白拒绝），`model: null` 清除模型并使用 Provider 默认值；这与 `create_agent` 将 `null` 视为未指定不同。
独立字段和合并值中的模型字符串均在比较和存储前去除首尾空白，保留模型 ID 内部的命名空间和斜杠。
同时传入合并的 `provider/model` 与独立 `model` 时，两处模型必须相同；合并值与 `model: null` 冲突并拒绝。
模型补丁不在保存时验证 Provider 是否支持该模型，实际可用性由运行时 Provider 判断；更新前应使用
`list_models` 查询有效 Provider 的模型。发现列表不作为严格白名单，以保留自定义模型支持。
例如 `{"id":"x","model":null}` 清除模型，`{"id":"x","provider":"codex","model":null,"mode":null}` 更换 Provider、清除模型并显式选择原生默认模式，
而 `{"id":"x","provider":"codex/gpt","model":null}` 同时要求设置与清除模型，因此无效。

创建和 prompt 在输入接纳后返回。用 `get_agent_status` / `get_agent_activity` 检查进展，
或用 `wait_for_agent` 等待，默认 30 秒、最多 45 秒。超时返回 `status: "timeout"`，
不会取消执行；通过 `cancel_agent` 显式取消。重试创建复用原 `idempotencyKey`，prompt 重试复用 `messageId`。

Agent 调用可发送 `X-Ait-Agent-Id: <agent-id>`，继承调用者 cwd 和 Workspace 并记录父子关系。
传入另一个 `workspaceId` 可以将子 Agent 放在另一个 Workspace。需要隔离时先
`create_workspace`，选择 `isolation: "worktree"`，再使用返回的 Workspace ID。
`mode` 支持 `branch-off`、`checkout-branch` 和 `checkout-pr`。`isolation: "sandboxed-worktree"`
接受同样的参数，并在 macOS/Linux 上限制该 Workspace 中受支持 Agent 进程的写入范围，
详见 [ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)；Linux 要求安装 bubblewrap 并允许非特权 user namespace，详见 [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)；macOS/Linux 以外的平台会拒绝创建。

脚本位于 Workspace 的 `ait.json`，按 `scriptName` 启停。每次启动记录一次运行并返回 `script.runId`；
声明了 `acceptsArgs` 的脚本可以传入字符串数组 `args`，每项作为一个位置参数传给脚本，不经 shell 解析。
`wait_for_workspace_script` 等待运行结束（默认 30 秒、最多 45 秒，超时不停止运行），
`get_workspace_script_run` 按 `runId` 或脚本名读取提交、结果、耗时和有界输出；两者属于 Bot 的
`observe` 组，作用域与 `list_workspace_scripts` 相同。字段与生命周期见
[Workspace 自动化](workspace-automation.md)。终端 `keys` 可以是 `Enter`、`C-c`
等令牌；`literal: true` 发送原样文本。

Schedule 归属于固定 Workspace；外部调用提供 `workspaceId`、`provider`，Agent 调用可以继承。
Heartbeat 需要调用者身份，定时向该 Agent 发送 prompt。Ait 的 Heartbeat 会持久化，完成后需
`delete_heartbeat`。`list_schedules` 与其余 Schedule 工具只操作 new-agent 任务。
