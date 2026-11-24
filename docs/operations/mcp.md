# 使用 Ait daemon MCP

daemon 在现有监听地址提供 Streamable HTTP MCP：`http://127.0.0.1:7316/mcp/agents`。
外部客户端使用 daemon 的本地 API token，发送 `Authorization: Bearer <token>`。
该 token 来自显式 `AIT_SERVER_TOKEN` 或自动保存的 `<data-dir>/local-api.token`，见[daemon 配置](daemon.md#启动与配置)。

新配置中 `mcp.enabled` 和 `mcp.injectIntoAgents` 默认均为 `true`。
`mcp.enabled` 通过 `daemon.config.set.request` 修改即时生效，关闭时入口返回 404。
已有配置中的显式 `injectIntoAgents: false` 保持关闭，不会在升级时覆盖。
服务端边界见 [ADR-148](../decisions/daemon/adr-148-daemon-mcp.md)，自动注入见
[ADR-202](../decisions/providers/adr-202-agent-mcp-injection.md)，同账号其他主机的调用见[跨主机调用](#跨主机调用)。

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

Workspace Bot 的隐藏会话，以及 Bot 通过 `agent_create` 启动的 Agent（带保留标签 `ait.bot.scope`），不使用共享令牌，而使用 daemon 签发的作用域凭据，
会替换用户配置的 `ait` 项。Bot 会话不受 `injectIntoAgents` 控制，子树会话仍受其控制；无法交付凭据时移除 `ait` 项，不回退到共享令牌。
凭据为 64 个十六进制字符，只在 daemon 内存中以 SHA-256 摘要保存，每次启动重新生成；同一 Agent 重新打开会话、Bot 暂停／移除／被取代、
会话关闭或归档时作废。Claude、Pi 和 DSH 原生 Host 经属主专用（0600）临时文件取得凭据，原生进程结束时删除；允许运行命令的 Agent 仍可能读到它，见 [ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 的已接受风险。

- 调用携带的 `X-Ait-Agent-Id` 若存在必须与凭据所属 Agent 一致，否则 `-32602`。共享令牌通过该头指向带 `ait.bot.scope` 的 Agent 同样被拒绝，该 Agent 必须使用自己的凭据。
- 每次请求按 Workspace 当前 Bot 的授权过滤：未授权工具不出现在 `tools/list`，调用返回 `-32602 "Unknown or unavailable tool"`；
  作用域或模式上限违规返回 `isError: true` 的 `Forbidden` 结果。
- 目标限于 Bot 的作用域（Bot 所在 Workspace 与它创建的未归档 worktree），目标 id 解析为规范 id 后再执行；`cwd` 参数被拒绝。
- `agent_create`、`agent_mode_set`、`agent_update`、`schedule_create`、`schedule_update` 的 Provider 与模式须在 Bot 的模式上限内；
  `schedule_update` 只能编辑带 `botScope` 的定时任务。
- 任何调用者都不能写入 `ait.bot.` 前缀的标签，作用域调用者不能写入任何 `ait.` 前缀的标签。作用域调用者不能使用 `workspace_create`。
- 没有非退役 Bot 时子树凭据降为只读观察。
- 工单（[ADR-274](../decisions/bot/adr-274-bot-tickets-assignments-and-reports.md)）：Bot 会话的每次 `agent_create` 打开一个工单，或以可选参数 `ticketId`
  （只在 Bot 会话看到的输入 schema 中，其他调用者传入时 `-32602`）加入已有工单；Agent 带 daemon 写入的标签 `ait.bot.ticket`，结果带 `ticketId`，
  其子 Agent 继承同一工单。达到 32 个未关闭工单时先自动关闭 Agent 全部已归档的工单（包括等待中的）。`bot_ticket_list` 列出工单，`bot_ticket_update` 关闭、重新打开、改名或备注；
  Bot 会话的 `agent_prompt` 会重新打开目标 Agent 已关闭的工单，达到上限而不能重新打开时结果带 `ticketNote`。
  属于工单的 Agent 用 `ticket_report`（`kind` 为 `progress`、`blocked`、`done`、`failed`，`text` 必填）汇报，工单与 Agent 取自凭据；`blocked`、`done`、`failed` 唤醒 Bot。
  Claude 工作者的会话预授权 `ticket_report` 与只读的 `ticket_read`（只此两个工具，持久配置不变），不再为它们请求审批；Codex 与其他 Provider 的工作者没有预授权，
  在会请求审批的模式中调用 `ticket_report` 可能先请求审批，见 [Workspace Bot](bots.md#任务)。
- 拉取请求与提问（[ADR-287](../decisions/bot/adr-287-ticket-pull-request-follow-and-answers.md)）：`ticket_report` 另接受可选 `pullRequest`
  （PR 编号、`#编号` 或 URL，1–512 字符）与只配 `blocked` 的 `question`（1–512 字符）；`bot_ticket_update` 另接受 `pullRequest` 与 `answer`（`{agentId,text}`，`text` 1–2048 字符，不能与 `status` 同用）。
  `pullRequest` 先用主机的 `gh`／`glab` 在 Bot 的检出中读取，读不到（CLI 缺失或未登录、PR 不存在、属于别的仓库、超过每工单 4 个或每 Workspace 16 个）时返回说明，
  汇报或更新都不记录；超过跟随上限时的说明写明把仍跟随的 `done` 或 `failed` 工单改为 `cancelled` 可以腾出名额。之后 Ait 跟随该 PR：
  能写入仓库的人（GitHub 上所有者或经权限查询确认的 admin、maintain、write 角色）的审查意见、请求修改与失败的检查在 Agent 空闲时作为新的一轮直接发给工单的 Agent
  （全部标为来自 Forge 的数据而不是指令，Forge 提供的字段以带引号的单行出现），其他人的评论只通知 Bot，合并、关闭与检查恢复唤醒 Bot。PR 仍打开的工单在合并时自行关闭，Bot 以 `done` 或 `failed`
  关闭它后仍继续跟随，`cancelled` 停止跟随，已关闭的 `done` 或 `failed` 工单也可以改为 `cancelled`。`answer` 只在工单仍等待 `agentId` 这个受阻的 Agent 时接受，在它空闲时送达，结果带 `delivered`。
- 按 id 回读（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md)）：Bot 会话用 `bot_read`（`resource`、可选 `offset`、`limit`）读取工单、对话、轮次、通知与 Agent，
  属于工单的 Agent 用 `ticket_read` 只读自己的工单。列表默认 20 条、至多 50 条，文本页默认 4000 字符、至多 8000 字符，以 `nextOffset` 续读；
  被截断的文本为 `{"preview","chars","read"}`，`read` 即读取全文的资源。Workspace 所有者在 Ait 中的对话可读 Bot 的全部对话，
  其他 Channel 的对话只能读自己及自己发起的工单与通知，`bot_overview`、`bot_ticket_list` 与 `bot_ticket_update` 对它们套用同一规则。
- Agent 的触达（[ADR-280](../decisions/bot/adr-280-bot-recall-reads-and-spill.md) 第 4 节）：其他 Channel 的 Bot 会话只能触达自己工单的 Agent，
  子树凭据按它的工作来自的对话受同样的限制（它的工单的来源对话，没有时为它的 `ait.bot.conversation`）；
  Agent 带属于别的对话的 `ait.bot.ticket` 时总是不能触达，没有这种工单时按 `ait.bot.conversation` 判定。
  对其他 Agent，`agent_get`、`agent_activity_read`、`agent_wait`、`agent_prompt`、`agent_update`、`agent_mode_set`、`agent_cancel`、`agent_archive`
  返回不含标题或 id 的引导文本，解析不到的 id 也返回同一文本；`agent_list` 与 `permission_list` 只列能触达的 Agent，
  受限的调用者的 `agent_list` 读最新 200 个 Agent 后过滤，`nextCursor` 为 `null`，以 `truncated` 表示没有列全；
  `agent_create` 的 `ticketId` 不能是别的对话的工单。所有者对话、所有者对话启动的 Agent 与不带对话的子树凭据不受此限制。
- 作用域 `agent_create` 在单独的任务中执行：60 秒调用超时只停止等待，Agent 创建与工单收尾照常完成；该任务占用调用额度直到结束，至多 5 分钟。

分组与工具对应关系见 [Workspace Bot](bots.md#权限)。

## Skill 使用指引

daemon 内置并在启动时分发 [ait-orchestration Skill](../../skills/ait-orchestration/SKILL.md)，包含 Profile 选择、
Agent 编排、Workspace 隔离、终端与调度操作说明。默认允许自动发现；是否加载由原生 Provider 决定。
没有已连接 MCP 工具时，可参考 Skill 的 HTTP 调用说明，使用运行时提供的连接信息访问相同入口。
安装目录、更新和停用见 [Skill 自动分发](agent-skills.md)。Skill 提供工具使用指引；连接由上述运行时注入提供。

## 可用工具

工具名为 `<域>[_<子对象>]_<动作>`（[ADR-295](../decisions/daemon/adr-295-mcp-tool-naming-and-aliases.md)），域同时表示谁能看到它。

| 域                     | 工具                                                                                                                                                               |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `agent`                | `agent_create`, `agent_prompt`, `agent_get`, `agent_activity_read`, `agent_list`, `agent_wait`, `agent_update`, `agent_mode_set`, `agent_cancel`, `agent_archive`  |
| `workspace`            | `workspace_create`, `workspace_list`, `workspace_rename`, `workspace_archive`                                                                                      |
| `script`               | `script_list`, `script_start`, `script_stop`, `script_run_get`, `script_run_wait`                                                                                  |
| `terminal`             | `terminal_list`, `terminal_create`, `terminal_kill`, `terminal_capture`, `terminal_send`                                                                           |
| `schedule`             | `schedule_create`, `schedule_list`, `schedule_get`, `schedule_update`, `schedule_pause`, `schedule_resume`, `schedule_run`, `schedule_log_read`, `schedule_delete` |
| `heartbeat`            | `heartbeat_create`, `heartbeat_list`, `heartbeat_delete`                                                                                                           |
| `host`                 | `host_list`（只对主机凭据的调用者，见[跨主机调用](#跨主机调用)）                                                                                                   |
| `provider` / `profile` | `provider_list`, `provider_model_list`, `provider_get`, `profile_list`                                                                                             |
| `permission`           | `permission_list`, `permission_respond`                                                                                                                            |
| `notepad`              | `notepad_read`, `notepad_write`                                                                                                                                    |
| `bot`                  | `bot_overview`, `bot_memory_read`, `bot_memory_update`, `bot_ticket_list`, `bot_ticket_update`, `bot_read`（只对 Bot 会话）                                        |
| `ticket`               | `ticket_report`, `ticket_read`（只对属于工单的 Agent）                                                                                                             |

准确参数由 `tools/list` 的 `inputSchema` 定义。部分能力未安装时，对应工具不出现在目录中。
Bot 会话与其启动的 Agent 只能看到已授权的工具，见上文“Bot 与子树凭据”。
每个工具带 `title`，注解中 `readOnlyHint` 标出只读工具，`destructiveHint` 只对归档、终止和删除类工具为 `true`。

参数约定：目标 id 为 `<域>Id`（`agentId`、`workspaceId`、`terminalId`、`scheduleId`、`heartbeatId`）；Agent 与 Workspace 的显示名为 `title`，
定时任务、终端与心跳为 `name`；模式为 `modeId`。目录用 `limit`/`cursor` 分页并返回 `nextCursor`，文本用 `offset`/`limit` 分页并返回 `nextOffset`。

### 旧工具名

0.2.23 及更早版本的名字（`create_agent`、`list_schedules`、`report_progress` 等）不再出现在 `tools/list` 中，但 `tools/call` 仍然接受：
daemon 在授权判定和 Schema 校验之前把旧名字解析为现在的工具，并把旧参数名（`update_agent.name`、定时任务与心跳的 `id`、`update_schedule.mode`）
改写为现在的名字。`kill_agent` 解析为 `agent_archive`，结果为归档的形状（`agentId`、`archivedAt`）。旧名字不比它对应的工具多授予任何权限。
每个旧名字的调用次数在首次及每次翻倍时写入日志（`retired MCP tool name called`），用来判断何时移除；移除不早于 2026-12-22，
且以日志中不再出现调用为条件。用户在自己的客户端里按旧名字保存的允许清单（如 Claude 的 `mcp__ait__create_agent`）需要自行改为新名字。
对照表见 [ADR-295](../decisions/daemon/adr-295-mcp-tool-naming-and-aliases.md#对照表)。

## 跨主机调用

同一账号下的多台主机可以经账号 Hub 互相调用 13 个工具：一台主机上的 Agent 用 `host_list` 找到其他主机，再给这些工具传 `hostId`，
在那里创建、提示并观察 Agent。边界与理由见 [ADR-305](../decisions/daemon/adr-305-cross-host-mcp-peer-sessions.md)，
传输规则见[连接规格](../architecture/connection.md#跨主机-peer-会话)。省略 `hostId`，或它等于本机，调用与之前逐字节相同。

### 接受开关：打开即信任本账号的全部主机

被调用的一方决定是否接受：配置键 `mcp.acceptPeers`（缺省 `false`，已有配置中缺失时同样为 `false`），经 `daemon.config.set.request` 修改，即时生效并重新向中心发布。
实际接受要求 `mcp.enabled` 与 `mcp.acceptPeers` 同时为 `true`。没有“允许本机调用其他主机”的开关。

```json
{
  "config": { "mcp": { "acceptPeers": true } }
}
```

**打开它等于信任本账号的每一台主机。** 对端的权限与本机一个用主机凭据调用这 13 个工具的 Agent 相同：可以读取、提示、取消、归档本机任意 Agent，
可以用 `workspace_create` 打开本机任意目录，并以任意 `modeId`（包括无人值守模式）创建 Agent。也就是说，账号里任何一台主机被攻破，
或它的 headless 机器凭据被盗，攻击者都可以在本机执行代码。对端不能事后提高模式（`agent_mode_set` 与 `agent_update` 不在 13 个工具内）。
只在确信账号内所有主机都可信时打开。

### `host_list`

无参数、只读，只对主机凭据的调用者可见。Bot 会话、Bot 子树 Agent 与工单工作者的目录里没有它，它们的工具也没有 `hostId`。
结果：

- `accepting`：本机现在是否接受对端调用（读取当前配置，不是最近一次发布的值）。
- `hosts`：本机排第一（`self: true`，总是 `available`），之后是账号的其他主机。每项含 `hostId`（daemon 的 `serverId`，`hostId` 参数就用它）、
  `name`、`hostname`、`platform`、`appVersion`、`state`（`online`、`reconnecting`、`offline`）、`self`、`available`；不可用时带 `reason`，
  按此顺序判定：`offline`（不是 `online`）、`unsupported`（它的版本或中心不支持）、`not_accepting`（它没有打开开关）。
- `note`：没有 Hub 链路、中心不可达、或中心太旧不支持时只返回本机，并带一条说明。中心没有列出本机时，本机仍以 `self` 返回，只有 `hostId` 与版本已知，其余为 `null`。
  中心返回的没有节点或 `serverId` 的主机（从未连接过）不列出。

主机列表缓存 5 秒；没有“目标未知时刷新”的步骤，刚上线的主机最多晚 5 秒出现。

### 可跨主机的工具

`agent_create`、`agent_prompt`、`agent_get`、`agent_activity_read`、`agent_list`、`agent_wait`、`agent_cancel`、`agent_archive`、
`workspace_create`、`workspace_list`、`provider_list`、`provider_model_list`、`profile_list`。名单是目录条目上的标记，测试核对它。
终端、脚本、定时任务、心跳、权限审批、记事本、Bot 与工单工具不跨主机，所以没有完成通知，也不级联：父 Agent 用带 `hostId` 的 `agent_wait`，
归档或取消父 Agent 不影响其他主机上的子 Agent。

转发成功时结果与目标主机的结果相同，`structuredContent.hostId` 给出目标主机。远程创建必须给目标主机上未归档的 `workspaceId`（不接受 `cwd`），
`workspace_create` 必须给出 `path` 或 `projectId`，`provider` 含模型照常必填；对端不能写 `ait.` 或 `paseo.` 前缀的标签。
目标主机上被创建的是普通 Agent，它自己发起的跨主机调用是新的调用；但带 `hostId` 的调用不会被再转发一跳。

### 失败码

转发失败时结果为 `isError: true`，`structuredContent` 带 `hostId`、`code` 与 `error` 文本。

| `code`               | 含义                                                                                                                                  | 调用是否执行 |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| `hub_unavailable`    | 本机没有 Hub 链路、中心不可达，或剩余时间已不够发送                                                                                   | 否           |
| `host_unknown`       | 账号里没有这个 `hostId`                                                                                                               | 否           |
| `host_offline`       | 目标不在线，或链路在会话建立前就超时                                                                                                  | 否           |
| `host_not_accepting` | 目标没有打开开关（包括它刚关闭、中心的发布尚未更新时由它自己拒绝）                                                                    | 否           |
| `host_unsupported`   | 目标的版本不支持                                                                                                                      | 否           |
| `capacity`           | 本机同时的跨主机调用已满，或请求或回复超过 1 MiB 消息上限；回复过大时调用**已经执行**                                                 | 视原因       |
| `outcome_unknown`    | 请求已发出，但回复没有到达（期限、停机、会话或链路丢失）                                                                              | **可能**     |
| `caller_not_allowed` | 调用者 Agent 在沙箱化 Worktree，或由访客会话创建                                                                                      | 否           |
| `host_rejected`      | 目标收到并拒绝了这次调用本身（如参数不符、工具不存在、`workspaceId` 缺失），`error` 是目标给出的文本，`remoteCode` 是它的 JSON-RPC 码 | 否           |

整个转发调用共用一个 55 秒期限，短于 60 秒的 MCP 调用超时，所以链路先于 API 放弃；目标上的 `agent_wait` 上限 45 秒，在期限之内。
转发占用本机的调用额度。**`outcome_unknown` 不会自动重放**：`agent_create` 可能已经执行，先用带同一个 `hostId` 的 `agent_list`／`agent_get` 检查，
或复用原 `idempotencyKey` 重试。

目标主机自己产生的会话级错误码：`-32001` 目标当前不接受对端调用（调用方得到 `host_not_accepting`，目标随后以 `peer_disabled` 关闭会话）；
`-32000` 回复超过消息上限（`capacity`，调用已执行）；`-32002` 目标此刻无法执行，调用额度已满或正在停机，调用未执行，
调用方得到 `host_rejected`，`remoteCode` 为 `-32002`。其余错误码是工具调用本身的拒绝（如 `-32602`），同样成为 `host_rejected`。

### 沙箱与访客限制只是尽力而为

调用者 Agent 位于沙箱化 Worktree 时拒绝跨主机调用：沙箱限制的是本机的写入范围，另一台主机在它之外。访客会话创建的 Agent（带 `ait.guest.share-id` 标签）
本来就不注入 daemon MCP，带该标签的调用者同样被拒绝。但调用者是由 `X-Ait-Agent-Id` 指向的 Agent 判断的，这个请求头不是权限边界
（[ADR-215](../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 的已接受风险）：持有主机凭据的进程可以不带它，或指向别的 Agent。
这两条限制防止误用，不能挡住恶意的本机进程。

### 远程父 Agent

经 `agent_create` 在目标主机上创建的 Agent，带两个由目标主机 daemon 写入的标签：`ait.parent.server-id`（调用方主机的 `serverId`）与 `ait.parent.agent-id`（调用方 Agent）。
它们不是 `paseo.parent-agent-id`：在目标主机上该 Agent 是顶层 Agent，提醒、未读、原生推送与自动归档都按顶层处理，不触发归档级联。
前缀 `ait.parent.` 对所有 MCP 调用者保留：主机凭据、Bot 作用域、对端与访客会话都不能写。
客户端同时连着两台主机时，按 `(serverId, agentId)` 把它显示为调用方 Agent 的子 Agent；没有连着目标主机时不显示。

## Workspace 记事本

每个 Workspace 有且只有一份记事本：daemon 数据目录下的一个 Markdown 文件，供这个 Workspace 的 Agent 与用户互相留下信息，例如约定的接口、
已经做出的决定、正在占用的文件、问题与答复（[ADR-296](../decisions/workspace/adr-296-workspace-notepad.md)、
[ADR-297](../decisions/workspace/adr-297-notepad-file-in-data-directory.md)）。它不需要创建；用户在客户端的记事本 tab 中用文件编辑器
打开的是同一个文件。写入不会唤醒任何 Agent，需要对方立刻处理时仍用 `agent_prompt`。

- `notepad_read` 返回 `text`、`chars`、`nextOffset`（默认每页 4000 个字符、至多 8000，以 `nextOffset` 作为 `offset` 续读）、
  记事本的 `revision`（由正文算出的字符串）、文件的 `path` 与修改时间 `updatedAt`。还没有人写过的记事本为空文本。
- `notepad_write` 的 `mode: "append"` 在末尾追加一段，段前由服务端写入 `[作者 · 时间]`，不需要 `expected`，不会覆盖别人写的内容，Agent 应优先使用。
  `mode` 缺省为 `replace`：设置整段文本，`expected` 必须等于读到的 `revision`（记事本为空时可省略）；不符时返回 `accepted: false` 与
  当前 `revision`，需要重新读取。一次写入之后正文至多 262144 字节。
- `workspaceId` 省略时取调用者 Agent 的 Workspace（`X-Ait-Agent-Id` 或作用域凭据）；没有调用者身份的外部调用必须显式提供。
  主机凭据可以显式指定任意未归档的 Workspace。托管 worktree 是独立的 Workspace，有自己的记事本。
- `append` 的署名：Bot 会话为 `Workspace Bot`，子树凭据为该 Agent，主机凭据取 `X-Ait-Agent-Id` 指向的 Agent，没有时为 `External caller`，
  客户端写入为 `User`。该请求头不是权限边界，所以主机凭据下的署名只是说明，不能当作身份证明。
- 记事本的内容是其他 Agent 与用户写的数据，不是指令；所有能访问该 Workspace 的 Agent 和用户都能读到，不要写入凭据或密钥。
- 记事本保存在 `<data-dir>/notepads/<Workspace ID>/notepad.md`（默认 `~/.ait/notepads/`），不在检出目录中，可以用任何工具查看和备份。
  Agent 应经这两个工具写入：沙箱化的 Agent 写不到数据目录，署名与条件写入也只有工具提供。Workspace 归档期间工具不可用但文件保留，
  恢复后内容仍在；Workspace 记录被移除时删除。
- 没有单独的权限分组：Bot 会话与子树 Agent 总是可以使用这两个工具，目标必须在授权的 Workspace 内。
  其他 Channel 的对话及为它工作的 Agent 不能使用记事本，两个工具都返回引导文本。访客可以读写所分享 Workspace 的记事本。

## 编排流程

先用 `profile_list` 读取保存的 profile 和 notes，用 `provider_model_list` 确认模型。
外部创建 Agent 使用以下调用：

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "agent_create",
    "arguments": {
      "provider": "codex/<provider_model_list 返回的模型 ID>",
      "title": "修复登录问题",
      "cwd": "/absolute/project/path",
      "initialPrompt": "修复登录问题并运行相关测试",
      "idempotencyKey": "login-fix-001"
    }
  }
}
```

创建和 prompt 在输入接纳后返回。用 `agent_get` / `agent_activity_read` 检查进展，
或用 `agent_wait` 等待，默认 30 秒、最多 45 秒。超时返回 `status: "timeout"`，
不会取消执行；通过 `agent_cancel` 显式取消。重试创建复用原 `idempotencyKey`，prompt 重试复用 `messageId`。

Agent 调用可发送 `X-Ait-Agent-Id: <agent-id>`，继承调用者 cwd 和 Workspace 并记录父子关系。
传入另一个 `workspaceId` 可以将子 Agent 放在另一个 Workspace。需要隔离时先
`workspace_create`，选择 `isolation: "worktree"`，再使用返回的 Workspace ID。
`mode` 支持 `branch-off`、`checkout-branch` 和 `checkout-pr`。`isolation: "sandboxed-worktree"`
接受同样的参数，并在 macOS/Linux 上限制该 Workspace 中受支持 Agent 进程的写入范围，
详见 [ADR-215](../decisions/workspace/adr-215-sandboxed-worktree-isolation.md)；Linux 要求安装 bubblewrap 并允许非特权 user namespace，详见 [ADR-223](../decisions/workspace/adr-223-linux-worktree-sandbox.md)；macOS/Linux 以外的平台会拒绝创建。

脚本位于 Workspace 的 `ait.json`，按 `scriptName` 启停。每次启动记录一次运行并返回 `script.runId`；
声明了 `acceptsArgs` 的脚本可以传入字符串数组 `args`，每项作为一个位置参数传给脚本，不经 shell 解析。
`script_run_wait` 等待运行结束（默认 30 秒、最多 45 秒，超时不停止运行），
`script_run_get` 按 `runId` 或脚本名读取提交、结果、耗时和有界输出；两者属于 Bot 的
`observe` 组，作用域与 `script_list` 相同。字段与生命周期见
[Workspace 自动化](workspace-automation.md)。终端 `keys` 可以是 `Enter`、`C-c`
等令牌；`literal: true` 发送原样文本。

Schedule 归属于固定 Workspace；外部调用提供 `workspaceId`、`provider`，Agent 调用可以继承。
Heartbeat 需要调用者身份，定时向该 Agent 发送 prompt。Ait 的 Heartbeat 会持久化，完成后需
`heartbeat_delete`；`heartbeat_list` 返回调用者自己的心跳及其 id。`schedule_list` 与其余 Schedule 工具只操作 new-agent 任务。
