---
id: ADR-305
status: accepted
date: 2026-11-24
amends:
  - daemon/adr-148-daemon-mcp.md
  - clients/adr-218-hub-feature-negotiation.md
  - daemon/adr-295-mcp-tool-naming-and-aliases.md
paths:
  - crates/api/src/mcp
  - crates/api/src/mcp.rs
  - crates/relay/src/hub
  - crates/model/src/peer.rs
  - bins/daemon/src/host/hub
---

# ADR-305：跨主机 MCP——peer 会话与对端调用者

账号下的多台主机可以经账号 Hub 互相调用 13 个 MCP 工具，让一个 Agent 在另一台主机上创建、提示并观察子 Agent。
本决策记录两条持久边界：主机之间的 **peer 会话**（跨组件契约），以及接收方对**对端调用者**的授权（权限边界）。
行为现状见 [MCP 手册](../../operations/mcp.md#跨主机调用)与[连接规格](../../architecture/connection.md#跨主机-peer-会话)。

- 账号中心一侧的对应决策是 `ait-server` 仓库的 `docs/decisions/adr-008-hub-peer-sessions.md`，协议见该仓库 `docs/hub-protocol.md` 第 11.8 节与 `docs/hub-services.md`。

## 背景

一台主机上的 Agent 只能编排本机：[ADR-148](adr-148-daemon-mcp.md) 的 MCP 没有主机的概念，daemon 之间也没有通道。
主机之间只经账号 Hub 相遇，而 Hub 的方向规则是“只有 `access` 能开会话，目标是其他节点的 `serve`”：daemon 持有的是 `serve` 链路，
机器凭据申请 `access` 返回 403，桌面版的 `access` 链路属于界面进程，所以 daemon 既不能向兄弟主机开会话，也看不到账号下有哪些主机。

## 决策

### 1. peer 会话：新的跨组件契约

- **传输。** 新 Hub feature `peer`（[ADR-218](../clients/adr-218-hub-feature-negotiation.md) 的协商机制）与会话类型 `k:"peer"`，由 main serve 链路在中心声明了它时才声明；
  `peer` 不进入“中心必须声明”的集合，旧中心上链路照常上线，只是没有跨主机能力。中心填写转给目标的 `open` 里的对端身份
  （`host_id`、`server_id`、`name`），发起方不能自带；接收方的授权只凭这个身份，不凭请求内容。
  不采用的做法：让 daemon 另建一条 `access` 链路。接收方分不清 daemon 与用户的客户端，被盗的 headless 机器凭据会得到已登录客户端的全部方法而不是 13 个工具。
- **发现。** 中心方法 `hub.peers.list` 让 serve 链路读取本账号各 Host 的名称、主机名、平台、版本与在线状态，`hub.peers.publish` 发布本机是否接受。
  这放宽了中心“机器凭据不能读取其他 Host”的规则，只放宽到这几个字段，是有意的边界变化（中心的决策见上）。
- **会话内的小协议。** 转发的是 MCP 工具调用本身：请求 `{id, tool, arguments, caller?}`，回复 `{id, result}` 或 `{id, error}`，`result` 就是本地 `tools/call` 的结果。
  接收方用自己的 MCP 层执行，所以授权与校验只有一处，两端版本不同时由接收方的 Schema 决定。不重写成发起方的第二套实现，也不走客户端业务握手。
- **接收方自己产生的错误码**只有三个（数值见[连接规格](../../architecture/connection.md#跨主机-peer-会话)），其余 `error` 一律是工具调用本身被拒绝：
  “本机当前不接受”（随后关闭会话）、“回复超过消息上限”（调用**已执行**）、“此刻无法执行”（调用额度已满或正在停机，调用**未执行**）。
  把后两者分成两个码，是为了让发起方知道该不该重试。
- **接收方的守卫是逐次调用的检查。** 接收方接受所有格式正确的 peer 会话，在每次调用时读取当前的 `mcp.enabled` 与 `mcp.acceptPeers`；
  中心的发布状态可能晚于开关的变化，会话上排队的请求仍会到达。发布因此要在每条新链路上与每次变化后重复，并在超时时重试：中心把每条新链路都当作不接受。
- **调用的结果语义。** 一次转发调用只受一个期限约束，且短于 MCP 的调用超时，使链路先于 API 放弃并知道请求是否已发出。请求发出之后被放弃
  （期限、停机、会话或链路丢失）的调用结果为 `outcome_unknown`，**从不自动重放**：`agent_create` 可能已经执行，调用方先用 `agent_list`／`agent_get` 检查，
  或用同一个 `idempotencyKey` 重试。请求发出之前的失败不会到达目标。
- **资源上限。** peer 会话单独计数，不占用普通会话的额度，使跨主机调用不会挤掉用户的客户端；发起方打开会话时声明接收窗口，使不超过消息上限的回复总能送达。
  各项数值与期限见[连接规格](../../architecture/connection.md#跨主机-peer-会话)。
- **依赖方向。** `api` 的 MCP 层只依赖 `model::peer::Peers` 端口（列出主机、向某节点发一次调用），由 `relay` 的适配器实现、daemon 组装；
  接收方向是本机 API 上“执行一次对端工具调用”的入口。`api` 不依赖 Hub 帧。

### 2. 对端调用者：接收方的权限边界

对端调用在接收方是第三类 MCP 调用者，与主机凭据和 Bot 作用域凭据并列：

- 只能调用目录中带跨主机标记的 13 个工具（`agent_create`、`agent_prompt`、`agent_get`、`agent_activity_read`、`agent_list`、`agent_wait`、`agent_cancel`、
  `agent_archive`、`workspace_create`、`workspace_list`、`provider_list`、`provider_model_list`、`profile_list`），其余是 `Unknown or unavailable tool`；
  参数必须符合主机凭据的 Schema，再出现 `hostId` 即拒绝：不做第二跳。
- 没有调用者 Agent 可继承：`agent_create` 必须带 `workspaceId` 且不接受 `cwd`，`workspace_create` 必须给出来源。
- 对端不能写入 `ait.` 或 `paseo.` 前缀的标签。远程父 Agent 记在两个标签 `ait.parent.server-id` 与 `ait.parent.agent-id` 里，只由接收方 daemon 按认证的对端身份写入；
  前缀 `ait.parent.` 对主机凭据调用者、Bot 作用域调用者、对端与访客会话一律保留。不复用 `paseo.parent-agent-id`：它在本机表示本机父 Agent，
  会触发归档级联与子 Agent 控制，而且两台主机上的 Agent id 不应被当作同一个。不增加快照字段，客户端按 `(serverId, agentId)` 从标签关联。
- **权限上限（2026-11-24 由用户确认）。** 除以上各条，对端的可见与操作范围等同于接收方上一个用主机凭据调用这 13 个工具的本机 Agent：
  可以读取、提示、取消、归档任意 Agent，可以用 `workspace_create` 打开任意目录，并以任意 `modeId`（包括无人值守模式）创建 Agent。
  因此**打开 `mcp.acceptPeers` 就是信任本账号的每一台主机**：任何一台被攻破，或其 headless 机器凭据被盗，攻击者可以在本机执行代码。
  对端不能事后提高模式：`agent_mode_set` 与 `agent_update` 不在 13 个工具内。开关默认关闭，由被调用的一方决定，没有“允许本机调用其他主机”的开关。
- **未采用：固定上限。** 对端不能选择无人值守模式、`workspace_create` 只能在已有项目上创建 worktree。理由：只要远程 Agent 能运行命令，上限就是在接收方执行代码，
  模式上限改变不了这一点；而它会让只有无人值守这一种免审批模式的 Provider 的远程工作者停在审批上，父 Agent 又不能替它审批（`permission_respond` 不跨主机）。
  代价是开关的含义更重，设置页与手册必须写明。

### 3. 调用侧：`host` 域与 `hostId`

- [ADR-295](adr-295-mcp-tool-naming-and-aliases.md) 固定了工具名的域词表；本决策增加域 `host`（`host_list`，动作 `list`）。`host_list` 无参数、只读。
- `hostId` 是 daemon 的 `serverId`，客户端已用它区分主机，父子关系也记录它。13 个工具的输入 Schema 增加可选的 `hostId`；省略或等于本机时与改动前逐字节相同。
- `host_list` 与 `hostId` 只对主机凭据的调用者存在：Bot 会话、Bot 子树和工单工作者的目录里没有 `host_list`，Schema 没有 `hostId`。
  调用者 Agent 位于沙箱化 Worktree，或由访客会话创建时拒绝跨主机调用（`caller_not_allowed`）。
  `X-Ait-Agent-Id` 不是权限边界（[ADR-215](../bot/adr-215-bot-permissions-and-scoped-mcp.md) 的已接受风险），所以这一条只是尽力而为。
- 中心不在线或不支持时 `host_list` 只返回本机，本机的 `hostId` 调用照常；中心未列出本机时仍以 `self` 返回本机。中心返回的没有节点或 `server_id` 的 Host 不列出。

## 后果

- 修订 [ADR-148](adr-148-daemon-mcp.md)：MCP 增加第三类调用者和一个跨主机路由；原有两类调用者与省略 `hostId` 的行为不变。
- 修订 [ADR-218](../clients/adr-218-hub-feature-negotiation.md)：协商的 feature 增加可选的 `peer`，不在“必须声明”集合内。
- 修订 [ADR-295](adr-295-mcp-tool-naming-and-aliases.md)：域列表增加 `host`；旧名字别名不变。
- 打开开关的主机把自己的安全等级降到账号内最弱主机的等级；这是接受的风险，不是缺陷。
- 完成通知、级联归档或取消、跨账号、多跳、按来源主机的允许名单都不在范围内；父 Agent 用带 `hostId` 的 `agent_wait`。
- 仓库里没有可供两个 daemon 进程共用的中心替身，所以端到端覆盖在 `relay` 测试里用按协议转发的测试中心完成；真实中心上的两主机验收记在 PR 描述里。
