# 跨主机 MCP 与子 Agent

**归档说明（2026-11-25）：** 本设计已按计划实施（账号中心 `ait-server` 的 Hub `peer`、daemon 的 `host_list` 与 `hostId`、客户端的远程子 Agent 与设置开关）。
实现与下文有出入之处见文末“实施中的修订”。现行说明见 [ADR-305](../../decisions/daemon/adr-305-cross-host-mcp-peer-sessions.md)、
[MCP 手册](../../operations/mcp.md#跨主机调用)与[连接规格](../../architecture/connection.md#跨主机-peer-会话)；
尚未完成的验收与范围外事项见[未完成工作](../../plans/README.md)。正文保留设计时的表述，对照源码提交 `5f3ac5eb3`（本仓库）与 `7ef22db`（账号中心 `ait-server`）。
实施步骤记录在[实施计划](cross-host-mcp-plan.md)。

## 意图

一台主机上的 Agent 目前只能编排本机：daemon 的 MCP（[MCP 手册](../../operations/mcp.md)）没有主机的概念，daemon 之间也没有任何通道。
用户在同一账号下有多台主机（工作站、服务器、笔记本）时，只能自己在客户端里切换主机、手工转述。

目标是让 Agent 自己发现同账号的其他主机，在那里创建、提示并观察 Agent，并让这些 Agent 作为它的子 Agent 出现在客户端里。
上游 Paseo 已有同类能力（`list_hosts` 加 7 个工具上的 `serverId`），本设计与它对照，但传输和信任模型按 Ait 的账号 Hub 重做。

## 现状

- MCP 入口 `/mcp/agents` 无状态，工具目录是 [`catalog.json`](../../../crates/api/src/mcp/catalog.json)，
  调用者分主机凭据与 Bot 作用域凭据两类（[`mcp.rs`](../../../crates/api/src/mcp.rs)）。
- 主机之间只经账号 Hub 相遇。Hub 的方向规则是“只有 `access` 能开会话，目标是同账号其他节点的 `serve`”；
  daemon 持有的是 `serve` 链路；机器凭据（headless）申请 `access` 返回 403，桌面版的 `access` 链路属于界面进程而不是 daemon
  （`ait-server` 的 `docs/hub-protocol.md` 第 1、2 节）。`serve` 链路只能调用发布类的中心方法，不能读取其他 Host
  （`docs/hub-services.md`“身份与方向”）。所以 daemon 现在不能向兄弟主机开会话，也看不到账号下有哪些主机。
- serve 链路已有两种可复用的机制：显式 `call` 调用中心方法（`hub.automation.publish`、`hub.shares.publish`），
  以及由中心按“最近一次发布为 enabled”才投递的服务会话（自动化会话，`hub-protocol.md` 6.2 节）。
- Hub 会话进入本机 API 时带一个准入身份：所有者、访客或自动化
  （[`bridge.rs`](../../../crates/relay/src/hub/bridge.rs) 的 `Admission`）。
- 父子关系存在子 Agent 的标签 `paseo.parent-agent-id` 里，快照以 `parentAgentId` 给出；归档级联、子 Agent 控制和客户端的
  子 Agent 轨道、任务总览都把它当作**同一主机**上的 Agent id（[任务总览](../../operations/task-board.md)）。

## 目标、非目标与反目标

目标：

- `host_list`：Agent 看到本机和同账号的其他主机，以及每台能否被调用、不能时的原因。
- 13 个工具接受可选的 `hostId`，省略时行为与现在逐字节相同。
- 在另一台主机上创建的 Agent 记录它的父 Agent（主机加 Agent id），客户端同时连着两台主机时把它显示为父 Agent 的子 Agent。
- 接收方主机自己决定是否接受，默认不接受。

非目标：

- 终端、脚本、定时任务、心跳、权限审批、记事本、Bot 与工单工具不跨主机。
- 不做完成通知（Paseo 的 `notifyOnFinish`）：Ait 没有这条机制，父 Agent 用带 `hostId` 的 `agent_wait`。
- 归档或取消父 Agent 不级联到其他主机上的子 Agent。
- 不跨账号（访客与分享不参与），不做多跳转发，不做按来源主机的允许名单，不保存任何主机凭据或地址。
- 不改旧工具名的别名与移除期限（[ADR-295](../../decisions/daemon/adr-295-mcp-tool-naming-and-aliases.md) 照旧）。

反目标（出现即算失败）：

- 机器凭据因此获得通用的 `access` 能力：读取其他主机的文件、终端、配置，或调用 13 个工具以外的任何方法。
  （在**开启了接受开关**的主机上创建 Agent 等同于在那台主机上执行代码，这正是开关的含义，见第 4 节“权限上限”。）
- 没有开启接受开关的主机，或版本不支持的主机，执行了对端的调用。
- 对端经这条路径得到的权限多于接收方主机上一个普通 Agent 用主机凭据调用 MCP 时的权限。
- 新 daemon 连接尚未部署该能力的中心时无法上线，或省略 `hostId` 的本地调用出现任何行为变化。

## 方案比较

**主机如何到达另一台主机**

| 方案                                 | 取舍                                                                                                             |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| 经账号 Hub（已选）                   | 同账号主机无需配置即可发现，穿透 NAT；需要改 `ait-server`，并依赖中心在线                                        |
| Paseo 式保存主机（地址加凭据，直连） | 只改本仓库、逐对显式授权；但每台主机要手工添加并保存凭据，且要求网络直达。与 Ait“主机挂在账号下”的产品形态不一致 |

**daemon 以什么身份在 Hub 上发起**

| 方案                                                   | 取舍                                                                                                                                                            |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 新 Hub feature `peer`（已选）                          | serve 链路可开 `k:"peer"` 会话，中心只投递给发布了“接受”的主机；接收方能区分对端 daemon 与用户客户端；机器凭据仍只能申请 `serve`                                |
| 让 daemon 另建一条 `access` 链路（对机器凭据放开 403） | 中心改动最小；但接收方分不清 daemon 与用户自己的客户端，被盗的 headless 机器凭据等于一个已登录客户端（全部方法，而不是 13 个工具），占用每节点 4 条 access 配额 |

**会话里传什么**

| 方案                                                     | 取舍                                                                                                    |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| 转发 MCP 工具调用本身（已选）                            | 接收方用自己的 MCP 层执行，结果形状与本地一致，校验与授权只有一处；两端版本不同时由接收方的 Schema 决定 |
| Paseo 式：开普通客户端会话，在发起方逐个工具重写远程实现 | 每个工具两套实现，且发起方要用 Rust 实现一遍客户端业务协议                                              |

## 设计

下文把发起调用的主机称为 A，被调用的主机称为 B。

### 1. 中心：Hub `peer`（`ait-server`）

- 新 feature `peer`，只由 main serve 链路声明，照 `hub-protocol.md` 第 11 节的规则协商；bulk 链路不参与。
- 新中心方法（经 `welcome.center_methods` 发现，只对声明了 `peer` 的 serve 链路可用）。其中 `hub.peers.list` 放宽了
  `hub-services.md` 现有的两条规则（“`serve` 只能调用发布方法”“机器凭据不能通过中心方法读取其他 Host 的资源”）：
  机器凭据从此可以读到本账号各 Host 的名称、主机名、平台、版本与在线状态，仅此而已。这是有意的边界变化，需写进该仓库的 ADR。
  - `hub.peers.publish`：参数 `{version, enabled}`，记录这条链路是否接受 peer 会话，语义与 `hub.automation.publish` 相同
    （每条新链路重新发布，最近一次为准）。
  - `hub.peers.list`：返回本账号全部未删除的 Host，字段取自 `hosts` 帧的 Host 对象
    （`host_id`、`node_id`、`server_id`、`name`、`hostname`、`platform`、`app_version`、`state`），另加
    `peer: {supported, enabled}`：`supported` 为该 Host 当前 serve 链路启用了 `peer`，`enabled` 为它最近一次发布为接受。
- 方向规则增加一条：声明了 `peer` 的 main serve 链路可以发 `open{k:"peer", to:{node}}`，目标必须是同账号**其他**节点的 main serve 链路。
  `access` 链路发 `k:"peer"` 返回 `forbidden`。其余方向规则不变。
- 中心按顺序拒绝：目标不存在或不在线 `unknown_target`；目标是自己 `forbidden`；目标链路未启用 `peer` `unsupported`；
  目标最近一次发布不是 `enabled:true` `peer_disabled`；超出配额 `capacity`。
- 转给目标的帧为 `open{seq, s, from, k:"peer", peer:{host_id, server_id, name}, w?}`，`peer` 由中心按发起方 node session 填写，
  发起方不能自带。只发给声明了 `peer` 的链路，所以旧 Host 不会收到未知字段。
- peer 会话单独计数，不占用每链路 16 个会话：每条链路发起的与接受的各至多 8 个（数值由实现确定，需写入协议文档）。
- B 发布 `enabled:false`、链路被删除或 Host 被停用时，中心以 `close{code:"peer_disabled"}` 或既有的 `peer_gone` 结束它名下的 peer 会话。
- `accept`、`msg`、`close`、`credit`、宽限期续传与溢出处理沿用普通会话。中心不解析 peer 会话的 payload。
- 票据规则不动：机器凭据仍只能申请 `serve`。
- 中心的 `hosts` 表对 `(owner_user_id, server_id)` 有唯一约束，所以同一账号下 `server_id` 可以唯一确定一个 Host。
- 桌面版的界面与内置 daemon 共用一个节点；“目标是自己”的规则按节点判定，所以 daemon 不会向自己的节点开 peer 会话。

### 2. daemon：peer 会话

- **声明。** 中心的 `hub_features` 含 `peer` 时，main 链路在 `hello.f` 中声明它；不含时不声明，链路照常建立。
  `peer` 不进入“中心必须声明”的集合（[`wire.rs`](../../../crates/relay/src/hub/wire.rs) 的 `FEATURES`）。
- **发布。** daemon 在每条新链路上以及配置变化后调用 `hub.peers.publish`，`enabled` 为 `mcp.enabled && mcp.acceptPeers`，
  做法与 [`automation.rs`](../../../bins/daemon/src/host/hub/automation.rs) 相同。
- **发起。** serve 链路新增发起方向：发 `open`，处理中心发来的 `accept` 以及自己发起的会话上的 `msg`、`close`、`credit`。
  对每台目标主机至多保持一个 peer 会话，按需打开，多个调用以请求 id 复用它，空闲约 60 秒后由发起方关闭。
- **接收。** `k:"peer"` 的 `open` 以新的准入身份（对端主机，带中心填写的 `host_id`、`server_id`、`name`）交给本机 API；
  没有 `peer` 字段、或本机当前不接受时以 `close` 拒绝。
- **会话内的消息**是独立的小协议，不走客户端业务握手：`open` 与 `accept` 的 payload 为版本声明；之后每条文本 `msg` 是一个请求
  `{id, tool, arguments, caller?}` 或它的回复 `{id, result}`／`{id, error}`。`result` 就是本地 `tools/call` 的结果对象
  （`content`、`structuredContent`、`isError`）。单条消息受 Hub 的 1 MiB 上限约束；超出时回复错误，不分块。
- **依赖方向。** `api` 的 MCP 层只依赖一个端口（列出主机、向某主机发一次调用），由 `relay` 的适配器实现、daemon 组装；
  接收方向则是本机 API 端口上多一个“执行一次对端工具调用”的入口。`api` 不直接依赖 Hub 帧。

### 3. MCP：`host_list` 与 `hostId`

- 新工具 `host_list`（域 `host`，动作取自 ADR-295 的词表），无参数，只读。返回本机是否接受对端调用，以及主机数组：
  `hostId`、`name`、`hostname`、`platform`、`appVersion`、`state`（`online`／`reconnecting`／`offline`）、`self`、
  `available`，不可用时带 `reason`（`offline`、`not_accepting`、`unsupported`）。
  本机没有 Hub 链路或中心不支持时只返回本机，并带一条说明原因的文本。
- **`hostId` 是 daemon 的 `serverId`**：客户端已经用它区分主机，中心的 Host 对象里也有它，父子关系记录的也是它。
  A 用 `hub.peers.list` 把它解析为目标节点，结果可短暂缓存，遇到 `unknown_target` 时刷新一次。
- 以下 13 个工具的输入 Schema 增加可选的 `hostId`：`agent_create`、`agent_prompt`、`agent_get`、`agent_activity_read`、`agent_list`、
  `agent_wait`、`agent_cancel`、`agent_archive`、`workspace_create`、`workspace_list`、`provider_list`、`provider_model_list`、`profile_list`。
  哪些工具可跨主机是目录条目上的一个标记，发起方加参数与接收方放行都读它，测试核对这份名单。
- 调用时：`hostId` 省略或等于本机时，去掉该参数后走现有本地路径；否则去掉该参数，把工具名、其余参数和调用者 Agent
  （`X-Ait-Agent-Id` 解析到的本机 Agent 的 id）转发给 B，原样返回 B 的结果，并在 `structuredContent` 中加 `hostId`。
- **谁能用。** 只有主机凭据的调用者（daemon 令牌或共享 Agent MCP 令牌）能看到 `host_list` 与 `hostId`。
  Bot 会话、Bot 子树与工单工作者的目录里没有 `host_list`，它们看到的 Schema 没有 `hostId`，传入时按参数不符拒绝。
  调用者 Agent 位于沙箱化 Worktree 时拒绝跨主机调用：沙箱限制的是本机的写入范围，另一台主机在它之外。
  访客会话创建的 Agent（带 `ait.guest.share-id` 标签）本来就不注入 daemon MCP
  （[`agent_mcp.rs`](../../../crates/provider/runtime/src/service/agent_mcp.rs)）；调用者 Agent 带该标签时同样拒绝跨主机调用。
  `X-Ait-Agent-Id` 不是权限边界（[ADR-215](../../decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md) 的已接受风险），所以这一条只是尽力而为，文档需如实说明。
- 转发占用 A 的 MCP 调用额度，并受 A 的 60 秒调用超时约束；B 上的 `agent_wait` 上限 45 秒，在此之内。
- `initialize` 的 `instructions`、[ait-orchestration Skill](../../../skills/ait-orchestration/SKILL.md) 和客户端的工具展示目录补上 `host_list` 与 `hostId`。

### 4. B 上的权限

对端调用在 B 上是第三类调用者（与主机凭据、Bot 作用域凭据并列），由中心填写的对端身份认证，不凭请求内容。

- 只能调用带跨主机标记的 13 个工具，其余返回 `Unknown or unavailable tool`。`host_list` 不可经对端调用。
- 参数中再出现 `hostId` 即拒绝：不做第二跳。B 上被创建的 Agent 是普通 Agent，它自己发起的跨主机调用是新的调用，不受此限。
- 没有调用者上下文可继承：`agent_create` 必须带 `workspaceId`（B 上未归档的 Workspace），不接受 `cwd`；
  `workspace_create` 必须显式给出来源。（`provider` 含模型是 `agent_create` 对所有调用者的既有要求。）
- 对端不能写入 `ait.` 或 `paseo.` 前缀的标签；父子关系的标签由 B 的 daemon 按认证身份写入（见第 5 节）。
- 除以上各条外，对端对 B 上 Agent 的可见范围与操作范围等同于 B 上一个用主机凭据调用这 13 个工具的普通 Agent：
  可以读取、提示、取消和归档 B 上的任意 Agent，不限于自己创建的。这与本机现状一致，也与 Paseo 一致。
- **权限上限（2026-11-24 确认）。** 按上一条，对端可以用 `workspace_create` 打开 B 上任意目录，
  并以任意 `settings.modeId`（包括无人值守模式）创建 Agent：只有 Bot 作用域才有模式上限。
  也就是说，B 打开接受开关即表示信任本账号的每一台主机，A 被攻破（或 A 的 headless 机器凭据被盗）时，攻击者可以在 B 上执行代码。
  对端不能事后提高模式：`agent_mode_set` 与 `agent_update` 不在 13 个工具内。
  考虑过的另一种做法是固定上限（对端不能选择无人值守模式，`workspace_create` 只能在已有项目上创建 worktree），未采用：
  只要远程 Agent 能运行命令，上限就是在 B 上执行代码，模式上限改变不了这一点；而它会让只有无人值守这一种免审批模式的 Provider
  的远程工作者停在审批上，父 Agent 又不能替它审批（`permission_respond` 不跨主机）。设置页必须写明这个开关的含义。
- 每次调用重新读取 `mcp.enabled` 与 `mcp.acceptPeers`；任一为 `false` 时拒绝，即使中心的发布状态尚未更新。
- 对端调用与本机 MCP 调用共用 B 的调用额度与超时。

### 5. 父子关系

- A 转发 `agent_create` 时带上调用者 Agent 的 id。B 把父 Agent 的地址（A 的 `serverId` 加 Agent id）写在**新的**标签 `ait.parent.server-id` 与 `ait.parent.agent-id` 里，
  不写 `paseo.parent-agent-id`：那个标签在 B 上表示本机父 Agent，会触发归档级联与子 Agent 控制，而且两台主机上的 id 不应被当作同一个。
- 不增加快照字段：标签本来就在快照里，客户端已经从标签读取本机父 Agent（`getParentAgentIdFromLabels`），远程父 Agent 同样从这两个标签读取。
  这两个标签的前缀对所有 MCP 调用者保留（本机主机凭据的调用者也不能写），只有 B 的 daemon 在处理对端的 `agent_create` 时写入。
- A 不保存远程子 Agent 的记录。客户端同时连着 A 和 B 时自行关联：父 Agent 的子 Agent 轨道和任务总览把其他已连接主机上
  远程父 Agent 指向它的 Agent 归入它，并标出所在主机；打开、审批、停止、归档都走子 Agent 所在主机的会话。
  客户端没有连着 B 时，这些子 Agent 不显示在 A 的父 Agent 之下；它们在 B 上照常作为独立任务显示。
- 对 B 的 daemon 而言，远程子 Agent 就是一个顶层 Agent：提醒、未读、原生推送和 `autoArchive` 都按顶层 Agent 处理，
  daemon 的提醒与通知路径不读取远程父 Agent 标签。同时连着两台主机的客户端在任务总览里把它归入 A 上的父任务并参与父任务的状态聚合，
  不再单独列出，做法与本机子任务相同。
- `agent_create` 的结果带 `hostId`，父 Agent 之后用同一个 `hostId` 调用 `agent_wait`、`agent_get`、`agent_activity_read`。

### 6. 配置与客户端

- daemon 配置增加 `mcp.acceptPeers`，布尔，缺省与已有配置中缺失时均为 `false`；经 `daemon.config.set.request` 修改即时生效并重新发布。
- 主机设置页的 MCP 一节增加开关“允许同账号其他主机上的 Agent 调用本机”，说明它的含义：其他主机上的任意 Agent
  可以在本机创建和操作 Agent。没有单独的“允许本机调用其他主机”开关：是否接受由被调用的一方决定。
- 客户端另需：协议里的配置字段与读取远程父 Agent 标签的辅助函数；子 Agent 轨道与任务总览的跨主机关联；工具展示目录。

### 7. 失败与兼容

- 目标离线、未接受、版本不支持或中心不可用时，A 返回 `isError: true` 的工具结果，带 `hostId` 与原因；调用没有到达 B。
- 请求已发出但会话在回复前结束时，结果未知。A 返回错误并写明“结果未知，重试前先用 `agent_list`／`agent_get` 检查”；
  `agent_create` 的 `idempotencyKey` 原样转发，调用方用同一个 key 重试是安全的。A 不自动重放。
- 旧中心：daemon 不声明 `peer`，其余功能不受影响；`host_list` 只有本机。旧 daemon：在 `host_list` 中为 `unsupported`。
- 未登录账号或已断开发布的 daemon：`host_list` 只有本机，带 `hostId` 的调用（非本机）返回明确错误。

## 与 Paseo 的对照

| 方面       | Paseo                                                    | Ait（本设计）                                                  |
| ---------- | -------------------------------------------------------- | -------------------------------------------------------------- |
| 发现       | `list_hosts`，来自 daemon 保存的 `hosts.json`            | `host_list`，来自账号下的 Host                                 |
| 选择参数   | 7 个工具上的 `serverId`                                  | 13 个工具上的 `hostId`（多出等待、取消、归档与 Provider 查询） |
| 传输       | daemon 之间的直连链路，保存地址与凭据                    | 账号 Hub 的 `peer` 会话，不保存凭据                            |
| 远程权限   | 所有者权限，与链路当前权限取交集                         | 固定的工具名单，接收方开关默认关闭                             |
| 远程创建   | 必须给目标 Workspace 与显式 Provider/模型                | 相同                                                           |
| 完成通知   | 发起方 daemon 持久订阅并通知父 Agent                     | 无，父 Agent 用 `agent_wait`                                   |
| 父子标记   | 标签 `paseo.parent-agent-id` 加 `paseo.parent-server-id` | 独立的远程父 Agent 标签（`ait.parent.*`）                      |
| 受限调用者 | 锁定工作目录的调用者不能路由                             | Bot 作用域调用者与沙箱化 Worktree 中的 Agent 不能路由          |
| 主机管理   | CLI 与 SDK 增删主机                                      | 无，主机即账号下的 Host                                        |

## 交付顺序

1. **`ait-server`**：`peer` feature、两个中心方法、方向规则与配额，更新 `hub-protocol.md`、`hub-services.md` 并记一条该仓库的 ADR；先部署。
   旧 daemon 不声明 `peer`，不受影响。
2. **daemon**（本仓库 `crates/relay`、`crates/model`、`crates/api`、`bins/daemon`、配置存储与协议）：声明与发布、双向 peer 会话、
   `host_list`、`hostId`、对端调用者、远程父 Agent 标签、`mcp.acceptPeers`。
3. **客户端与文档**（`packages/protocol`、`apps/mobile`、Skill、文档）：设置开关、跨主机子 Agent 显示、工具展示。
   第 2、3 步可以在同一个版本发布；没有界面时也能用 `daemon.config.set.request` 打开开关验收第 2 步。

## 测试

- 中心：协商与兼容（旧链路逐字节不变）、每个拒绝码、发布关闭时结束会话、配额、`access` 链路发 `k:"peer"` 被拒。
- `relay`：对着测试中心发起与接收 peer 会话、请求复用与空闲关闭、链路续传期间的调用、会话中断时未决调用以“结果未知”结束。
- `api`：以假的主机端口测试 `host_list`、`hostId` 路由与参数剥离、作用域调用者与沙箱调用者被拒；
  对端调用者的工具名单、第二跳、缺少 `workspaceId`、保留标签、开关关闭；目录测试核对跨主机标记的名单。
- 端到端：`relay` 测试里两个 serve 链路经一个按协议转发的测试中心完成一次对端调用。仓库里没有可供两个 daemon 进程共用的中心替身，
  所以不写双 daemon 进程测试；真实中心上的两主机验收（`agent_create` 加 `agent_wait`，并核对子 Agent 的远程父 Agent 标签）记在 PR 描述里。
- 客户端：子 Agent 选择与任务总览模型的跨主机关联；设置开关。
- 测试等待事件而不是时间（[仓库约定](../../../AGENTS.md)）。

## 文档与 ADR

- 本仓库新增一条 ADR（`daemon` 分类，编号在实施时扫描全部分支后取，当前最大为 304），修订
  [ADR-148](../../decisions/daemon/adr-148-daemon-mcp.md) 与 [ADR-218](../../decisions/clients/adr-218-hub-feature-negotiation.md)：
  记录 peer 会话这一新的跨组件契约，以及“对端调用者”这一新的权限边界。
- 更新 [MCP 手册](../../operations/mcp.md)、[连接规格](../../architecture/connection.md)、[架构概览](../../architecture/README.md)、
  [任务总览](../../operations/task-board.md)、[daemon 手册](../../operations/daemon.md) 与 CHANGELOG；实施完成后本文件归档。

## 留给实现者

- 中心方法的版本字段与错误文本；peer 会话的配额数值、主机列表缓存时长与空闲关闭时长。
- 发起方需要 `close` 的 `code`（现在 [`wire.rs`](../../../crates/relay/src/hub/wire.rs) 的 `Inbound::Close` 不解析它）来区分
  `unknown_target`、`peer_disabled` 等原因；daemon 侧的每链路 16 个会话上限也要与中心的单独计数保持一致。
- 对端调用者在 [`scope.rs`](../../../crates/api/src/mcp/scope.rs) 中如何表示（现有 `Scope` 是 Bot 专用的），前提是授权判定仍只有一处。
- 远程子 Agent 在轨道和任务总览里的具体呈现，以及“归档已完成的子 Agent”是否包含远程子 Agent。

## 实施中的修订

以下各处实现与上文设计不同；权威描述是 [ADR-305](../../decisions/daemon/adr-305-cross-host-mcp-peer-sessions.md)、[MCP 手册](../../operations/mcp.md#跨主机调用)与[连接规格](../../architecture/connection.md#跨主机-peer-会话)。

- **主机列表：** 目标未知时不刷新（设计写“遇到 `unknown_target` 时刷新一次”），缓存 5 秒，刚上线的主机可能稍后才出现。
- **没有节点或 `server_id` 的主机：** 中心返回的从未连接过的主机不列入 `host_list`。
- **错误码：** 接收方自己产生的错误码是 `-32001`（当前不接受）、`-32000`（回复过大等）与 `-32002`（此刻无法执行，调用未执行）；调用侧把目标在协议层的拒绝报告为 `host_rejected`。
- **期限：** `Peers::call` 带时间限制，一次转发调用（包括路由）共用一个 55 秒期限，而不是只受 60 秒调用超时约束。
- **接受与拒绝：** 接收方接受会话，在每次调用时拒绝，而不是在建立会话时拒绝；关闭开关后已有会话里的下一次调用得到 `host_not_accepting`。
- **访客会话：** 访客会话不能写父标签。
