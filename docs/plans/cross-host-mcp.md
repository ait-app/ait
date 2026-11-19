# 跨主机 MCP 调用设计

状态：提案，尚未实施。对照源码 `ait@718fb86` 与 `ait-server@7ef22db`。本文同时覆盖两个仓库；
实施时 ait 侧新增 daemon 与 bot 类 ADR，ait-server 侧新增 ADR-007，并更新
`hub-protocol.md`、`hub-services.md`、[连接与传输规格](../architecture/connection.md)和 [MCP 手册](../operations/mcp.md)。

范围假设：“跨主机 MCP 调用”指 **Host A 上的 Agent 或 MCP 客户端调用同账号 Host B 的 Ait daemon MCP 工具**
（例如在构建机上 `create_agent`、读取进展、等待结果）。把任意第三方 MCP Server 隧道到另一台主机不在本期范围内，
但第 5.2 节的会话协议为它留了位置（`open` 载荷中的 `server` 字段）。

## 1. 现状复核

| 事实                                                                                                                                                                                                                               | 位置                                                                                                 | 对设计的影响                                                                                         |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| daemon MCP 是本机的无状态 Streamable HTTP；身份只有 `Host`（daemon 令牌或共享 Agent 令牌）与 `Scoped`（Bot 作用域凭据）；50 个目录工具全部经一次性 RPC 适配器落到本机能力，没有“主机”参数                                          | `crates/api/src/mcp.rs`、`mcp/catalog.json`、`connection/rpc.rs`                                     | 目标端可以直接复用 MCP 层（目录、Schema 校验、作用域约束），只缺一个非 HTTP 入口和第三种身份         |
| Hub 只允许 `access` 发 `open`，`serve` 只能接受会话与广播；serve 发 `open` 是致命的 `forbidden`                                                                                                                                    | ait-server `src/hub/account.rs`（`receive` 的 `Inbound::Open` 分支）、`docs/hub-protocol.md` 第 1 节 | daemon 之间今天没有任何通路                                                                          |
| 机器凭据只能申请 `serve` 票据，申请 `access` 返回 403；这是有意的边界：“机器授权不能借此获得账号查询权限”                                                                                                                          | ait-server `src/hub/mod.rs`（`auth.is_device()`）、ADR-001、ADR-004                                  | 跨主机调用必然放宽这条边界，必须有显式、可撤销、由目标 Host 决定的授权                               |
| daemon 不知道同账号还有哪些 Host：`presence` 与 `hosts` 帧只发给 access 链路                                                                                                                                                       | ait-server `src/hub/account/hosts.rs`、`src/hub/hosts.rs`                                            | 需要给 serve 链路一个受限的对端视图                                                                  |
| 中心 `call` 按账号串行执行（`call_running`），每账号最多 32 个待处理调用；daemon 侧调用 30 秒超时、最多 16 个并发                                                                                                                  | ait-server `src/hub/account/services.rs`；ait `crates/relay/src/hub/calls.rs`                        | MCP 调用最长等待 60 秒（`wait_for_agent` 45 秒），不能走 `call` 帧，否则阻塞整个账号的事件发布和推送 |
| 中心已有三种“自己向 serve 开会话”的先例：下载桥接、用量采集（`create_bridge`）、自动化（`create_service`，`service:{kind:"automation"}`）。三者都只发给声明或发布过对应能力的 serve 链路，因为旧 daemon 忽略未知字段并按所有者服务 | ait-server `src/hub/bridge.rs`、`usage.rs`、`service.rs`、`account/automation.rs`                    | 新会话同样必须由目标链路显式声明后才能路由                                                           |
| daemon 的 `SessionKind` 带 `#[serde(other)] Unsupported`，未知 `k` 以 `unsupported` 拒绝；但 `Inbound` 没有兜底变体，未知的 `t` 会使整条链路解析失败                                                                               | ait `crates/relay/src/hub/wire.rs`                                                                   | 新会话类型对新 daemon 安全；新帧类型（serve 收到 `accept`）只能发给声明了新 feature 的链路           |
| Host 端进程内服务 Hub 会话，准入分 `Owner`、`Guest`、`Automation`，自动化会话采用默认拒绝的方法白名单                                                                                                                              | ait `crates/relay/src/hub/bridge.rs`、`crates/api/src/automation/policy.rs`                          | 新增 `Peer` 准入的落点和模式都已存在                                                                 |
| Bot 作用域凭据已经提供“每次请求读取当前授权、按工具分组过滤目录、规范化目标 id”的执行引擎；未登记的工具名按 `HostOnly` 处理                                                                                                        | ait `crates/api/src/mcp/scope.rs`、`crates/domain/src/bot/permissions.rs`、ADR-215                   | 对端授权可以沿用同一套过滤点；新增的网关工具对 Bot 默认不可见                                        |
| 分享设置以 Host 为准、由 Host 发布、中心只保存身份并路由                                                                                                                                                                           | ait `crates/api/src/shares.rs`；ait-server ADR-005                                                   | 对端授权采用同一所有权模型                                                                           |
| 资源身份是 `(hostId, id)`，不同主机的相同 Agent ID 不合并                                                                                                                                                                          | ait-server `docs/hub-services.md`；[任务总览](../operations/task-board.md)                           | 工具结果必须带来源 Host，调用端不解析远端 id                                                         |

## 2. 目标与非目标

目标：

1. 连接到 Host A `/mcp/agents` 的 Agent 或外部客户端可以发现可调用的其他 Host，并调用其 Ait MCP 工具。
2. 只使用现有 Hub 链路：不新增入站端口，不要求主机之间直连，不要求任何客户端在线。
3. 默认拒绝。只有账号所有者在目标 Host 上显式允许某个来源 Host 后才可调用；来源 Host 不能给自己授权。
4. 目标端在 MCP 层执行并强制授权，不暴露 daemon 的原始 RPC 面。
5. 旧 daemon、旧中心行为不变；任何对端会话都不会到达会把它当作所有者的 daemon。

非目标（本期）：跨账号（群组分享）的 MCP 调用、MCP 通知与 SSE 流、第三方 MCP Server 隧道、
经 Open API Key 访问的中心 MCP 入口（第 9 节阶段 3）、Bot 与子树凭据使用跨主机工具（阶段 2）。

## 3. 方案比较

| 方案                                                                            | 结论     | 原因                                                                                                                                                                  |
| ------------------------------------------------------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A. 中心 `call` 帧中转（`hub.mcp.call`）                                         | 否       | 账号级串行队列与 30 秒超时不适合最长 60 秒的工具调用；中心要解析并缓冲业务载荷，违背 ADR-004“普通会话载荷原样转发”                                                    |
| B. 允许机器凭据建立 access 链路，daemon 像客户端一样开普通会话                  | 否       | 目标端按所有者服务，暴露终端、文件等全部 RPC；任何一台机器的凭据泄露等于全部主机失守                                                                                  |
| C. 只做中心 HTTP MCP 入口，给每个 Agent 会话再注入一个指向中心的 MCP 配置       | 否       | 需要把中心可用的凭据交付进 Agent 会话并为 7 个 Provider 改注入；Agent 必须能直连中心；中心要成为 MCP 执行方。作为面向外部 API Key 的入口留到阶段 3，复用方案 D 的路由 |
| D. **对端会话**：serve 链路向同账号另一条 serve 链路开 `k:"mcp"` 会话，载荷透明 | **采用** | 沿用会话的路由、续传、流控和撤销；中心只做身份与策略判断；目标端在 MCP 层以新身份执行；调用端只增加三个网关工具，不改 Provider 注入                                   |

## 4. 总览

```text
Agent（Host A） ──HTTP MCP──▶ daemon A /mcp/agents
                               │ 网关工具：list_hosts / list_host_tools / call_host_tool
                               ▼
                 relay::hub（A 的 serve 主链路）
                               │ open{k:"mcp",to:{node:B}} ＋ msg（JSON-RPC）
                               ▼
                 中心：同账号、双方声明 peer、B 发布的名单包含 A → 填写 peer 身份后转发
                               ▼
                 relay::hub（B 的 serve 主链路）──进程内──▶ api::mcp（Principal::Peer）
                               │ 目录过滤 → Schema 校验 → 授权 → 现有工具实现
                               ◀── msg（JSON-RPC 结果）沿原路返回，网关工具把它作为工具结果交给 Agent
```

## 5. 设计

### 5.1 Hub feature `peer`

- 中心在 `hub_features` 中增加 `peer`。serve 主链路在中心声明了它时才在 `hello.f` 中带上（与 `bulk` 相同的可选声明方式，
  不属于 daemon 要求中心必须具备的 feature）。bulk 链路和 access 链路不能声明。
- 中心只对声明了 `peer` 的链路使用本节的帧和字段；未声明的链路行为逐字节不变，serve 发 `open` 仍是致命的 `forbidden`。

### 5.2 `k:"mcp"` 对端会话

| 方向         | 帧                                                        | 载荷                                                                                              |
| ------------ | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| A → 中心     | `open{seq,s,to:{node},k:"mcp",w?}`                        | `{"v":1,"server":"ait"}`；`server` 目前只有 `ait`                                                 |
| 中心 → B     | `open{seq,s,from,k:"mcp",peer:{host_id,node_id,name},w?}` | 同上；`peer` 由中心按认证后的链路填写                                                             |
| B → 中心 → A | `accept{seq,s,w?}`                                        | `{"v":1,"protocolVersion":"2025-11-25","serverInfo":{…},"instanceId":"…","permission":"observe"}` |
| 双向         | `msg{seq,s,pt:"text"}`                                    | 一条 JSON-RPC 2.0 消息；A 只发请求，B 只发响应。不支持批量、通知和二进制                          |
| 任一方或中心 | `close{seq,s,code}`                                       | 无                                                                                                |

- 语义与 HTTP 入口一致：无状态，不需要 `initialize`，支持 `ping`、`tools/list`、`tools/call`。一个会话内以 JSON-RPC `id` 复用多个并发请求，
  `id` 由调用端 daemon 生成，与 Agent 自己的请求 `id` 无关。
- 流控沿用 `credit`（两端的 serve 主链路总是声明它）；续传、宽限期、8 MiB 链路缓冲与 `overflow` 规则与其他会话相同。
  单条消息受 Hub 1 MiB 载荷上限约束，目标端对超限结果返回 JSON-RPC 错误而不是截断。
- 每个有序的 `(A, B)` 至多一个会话，按需建立，没有在途请求 60 秒后由调用端关闭。

中心收到 serve 链路的 `open` 时按顺序检查，前两项不满足时为协议错误，其余以 `close{s,code}` 拒绝且不影响链路：

1. 发起链路是声明了 `peer` 的 serve 主链路；`k` 为 `mcp`，不带 `share`，`to.ep` 为空。
2. `to.node` 不是自己。
3. 目标节点有 serve 主链路（已连接或在宽限期内），否则 `unknown_target`。
4. 目标链路声明了 `peer`，否则 `unsupported`。
5. 目标链路最近一次 `hub.peers.publish` 的名单包含发起方的 `host_id`，否则 `peer_denied`。
6. 发起链路的对端出站会话少于 16、目标链路的对端入站会话少于 16，否则 `capacity`。两者单独计数，
   不占用所有者的 16 个会话、访客的 64 个和自动化的 4 个。

会话表从 `{access, serve}` 推广为 `{opener, target}`；`accept` 只接受来自 `target`。链路删除时会话以 `peer_gone` 结束。
目标重新发布的名单不再包含发起方时，中心以 `close{code:"peer_revoked"}` 结束该对会话。

### 5.3 授权：设置在目标 Host，中心只路由

沿用 ADR-005 的所有权：**谁可以调用我、能做什么，保存在被调用的 Host 本地**；中心只知道“允许哪些 Host”，用来路由和撤销。

本地文件 `<data_dir>/hub/peers.json`：

```json
{
  "revision": 3,
  "inbound": [{ "hostId": "HOST_A", "permission": "operate", "settingId": "SETTING_UUID" }],
  "outbound": { "enabled": true }
}
```

- 新增 daemon RPC `peer.list.request`、`peer.set.request`，由 `api` 声明（与分享设置相同）。只有 `Owner` 准入可以调用：
  本机 daemon 令牌的连接，或账号所有者经 access 链路开的普通 Hub 会话。访客、自动化和对端会话一律拒绝，也不进入 MCP 工具目录。
  机器凭据拿不到 access 票据，因此 **Host A 无法修改 Host B 的名单**。
- 首版权限只有两级，与分享首版只有 `operate` 的取舍相同：

  | `permission` | 可用工具                                                                                                             |
  | ------------ | -------------------------------------------------------------------------------------------------------------------- |
  | `observe`    | `observe` 分组的工具，加 `list_workspaces`                                                                           |
  | `operate`    | 另加 `agents`、`stop`、`schedules`、`scripts`、`terminals` 分组与 `create_workspace`                                 |
  | 永不         | `respond_to_permission`、`archive_workspace`、`rename_workspace`、Bot 专属与工单工作者工具、三个网关工具（不可转发） |

- 每次建立 serve 链路后以及设置变化时调用中心方法 `hub.peers.publish`（serve），没有名单也发布空列表：

  ```json
  { "revision": 3, "accept": ["HOST_A"] }
  ```

  `revision` 规则与 `hub.shares.publish` 相同。名单随链路保存在中心内存中（与 `hub.automation.publish` 相同），不需要数据库迁移；
  中心重启后由重连的 Host 恢复。名单最多 64 项，其中不属于本账号有效 Host 的项在结果的 `rejected` 中返回。

- 结果与之后的 `peers` 帧（只发给发布过的 serve 链路，未送达的旧快照被新快照替换）是同一种完整快照：

  ```json
  {
    "revision": 3,
    "hosts": [
      {
        "hostId": "HOST_B",
        "nodeId": "NODE_B",
        "name": "build-box",
        "platform": "linux",
        "appVersion": "…",
        "state": "online",
        "admitsMe": true,
        "admitted": false
      }
    ],
    "rejected": []
  }
  ```

  快照 **只包含与本 Host 有授权关系的 Host**（它允许我，或我允许它）。机器凭据因此看不到没有授权关系的主机，
  ADR-004 的边界只按用户的显式授权放宽。对端上线、下线、名单变化或 feature 变化时重新推送。

### 5.4 目标端（Host B）

- `relay`：`SessionKind::Mcp`；`Inbound::Open` 增加 `peer`；带 `peer` 的会话必须是 `mcp`，否则 `unsupported`。
  会话循环把每条文本消息交给新的进程内端口（`LocalApi::mcp(peer, message)`），至多 8 个在途请求，超出时回 JSON-RPC `-32000`。
  对端入站会话有自己的 16 个名额，不占用 `MAX_SESSIONS`。
- `api`：把 `mcp::handle` 中与 HTTP 无关的部分提取为 `mcp::serve(state, principal, message)`，HTTP 路由与 Hub 入口共用。
  新增 `Principal::Peer { host_id, name }`：
  - 打开会话时按本地设置复核来源 Host（中心的索引可能滞后），不允许时以 `peer_denied` 关闭；`mcp.enabled` 为 `false` 时以 `unsupported` 关闭。
  - **每次请求**读取当前 `permission`（与 ADR-215 相同的生效时机），未授权的工具不出现在 `tools/list`，调用返回 `-32602 "Unknown or unavailable tool"`。
  - 按外部调用者处理：没有 `X-Ait-Agent-Id` 上下文，`create_agent` 必须提供 `workspaceId` 或 `cwd`。
  - 不能写入 `ait.` 前缀的标签。daemon 为对端创建的 Agent、定时任务写入 `ait.peer.host=<来源 hostId>`（取自中心填写的 `peer`），
    并把请求 `_meta["ait/callerAgentId"]` 记为 `ait.peer.agent`（来源自报，仅用于展示跨主机父子关系）。
  - 与 HTTP 入口共用调用额度（`state.connections`）、60 秒调用上限和受追踪任务：会话中断不取消已接纳的工作。
- 每次对端调用记录来源 Host、工具名与结果类别，不记录参数。

### 5.5 调用端（Host A）

- `model` 增加端口（暂名 `model::peer::Peers`）：观察对端快照、`call(host_id, message) -> Result<Value, PeerError>`。
  `relay::hub::Connector` 实现它：管理出站会话、关联 JSON-RPC `id`、限制每个目标 8 个在途请求、处理 `accept` 和关闭码。
  daemon 的 `Inbound` 增加 `Accept` 与 `Peers`；`api` 只依赖端口，不依赖传输。
- 工具目录增加三个网关工具，只对 `Principal::Host` 可见（未登记的工具名对 Bot 作用域凭据本来就是 `HostOnly`）：

  | 工具              | 参数                                  | 结果                                                                                     |
  | ----------------- | ------------------------------------- | ---------------------------------------------------------------------------------------- |
  | `list_hosts`      | 无                                    | 本机 `hostId` 与名称；每个 `admitsMe` 的 Host 的 id、名称、平台、状态、`permission`      |
  | `list_host_tools` | `hostId`                              | 远端 `tools/list` 的结果（对本机可见的工具及其 `inputSchema`），按远端 `instanceId` 缓存 |
  | `call_host_tool`  | `hostId`、`name`、`arguments`（对象） | `{hostId, result}`，`result` 为远端的 `structuredContent`；远端 `isError` 原样传递       |

  选择网关工具而不是给 50 个工具各加一个 `hostId` 参数：目录不随主机数量膨胀；远端版本不同时由远端按自己的 Schema 校验；
  现有 Schema、校验器和 Bot 的作用域规则都不用改。同版本下远端工具的名称和参数与模型已经看到的本机工具相同。

- 网关工具在以下条件同时满足时才出现在 `tools/list`：链路启用了 `peer`、本地 `outbound.enabled` 为真（默认真）、
  最近的快照中至少有一个 `admitsMe` 的 Host。没有配置跨主机授权的用户看不到任何变化。
- `skills/ait-orchestration` 增加多主机一节：Agent、Workspace、终端和定时任务的 id 只在所属 Host 内有效，必须与 `hostId` 成对保存；
  `cwd` 是目标主机上的路径；先用远端的 `list_workspaces`、`list_models` 发现再创建。

### 5.6 错误、超时与幂等

| 情况                                    | `call_host_tool` 的结果（均为 `isError: true` 的工具结果，不是 JSON-RPC 错误） |
| --------------------------------------- | ------------------------------------------------------------------------------ |
| `unknown_target`、`peer_gone`           | 目标主机离线或刚断开，稍后重试                                                 |
| `unsupported`                           | 目标主机的 Ait 版本不支持跨主机调用，或它关闭了 MCP                            |
| `peer_denied`、`peer_revoked`           | 目标主机没有允许本机；请用户在 Ait 中该主机的设置里允许                        |
| `capacity`、在途请求已满                | 可重试的容量错误                                                               |
| 本机没有在线的 Hub 链路或中心不支持     | 跨主机调用不可用                                                               |
| 60 秒内没有响应、链路在响应前换了新链路 | 结果未知：远端可能已经执行。提示先用只读工具核对，或以同一幂等键重试           |

`create_agent`、`create_workspace` 的 `idempotencyKey` 与 `send_agent_prompt` 的 `messageId` 在远端照常生效，
因此“结果未知”后的重试是安全的；其他有副作用的工具按技能中已有的规则先核对再重试。物理断线在宽限期内续传时请求不丢失。

## 6. 安全分析

| 风险                               | 处理                                                                                                                                                                                                                                                                                   |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Host A 的机器凭据或本机令牌泄露    | 只能到达明确允许了 A 的 Host，且受其 `permission` 限制；A 不能修改别的 Host 的名单，也看不到没有授权关系的 Host                                                                                                                                                                        |
| 旧 daemon 把对端会话当作所有者会话 | 中心只向声明了 `peer` 且发布过名单的链路路由；会话总是带 `k:"mcp"`，旧 daemon 即使收到也以 `unsupported` 拒绝                                                                                                                                                                          |
| 转发链与环路（A→B→C、A→B→A）       | 对端身份没有网关工具，一次调用至多一跳                                                                                                                                                                                                                                                 |
| `operate` 的实际含义               | 等同于在目标主机上以所有者系统身份执行命令（创建 Agent、终端）。被创建的 Agent 是目标主机上的普通 Agent，使用共享令牌，也可以使用目标主机自己的出站授权。设置界面必须按分享“整台节点”的力度提示。阶段 2 以 `ait.peer.host` 标签派生子树凭据（复用 ADR-215 的机制）后，才提供更细的授权 |
| Agent 被提示注入后操作其他主机     | 受 `permission` 限制；`call_host_tool` 标注为非只读，在需要审批的模式下由 Provider 请求审批；`respond_to_permission` 永不对对端开放，远端的审批仍由用户在客户端处理                                                                                                                    |
| 资源耗尽                           | 中心的出入站会话名额、每会话 8 个在途请求、目标端调用额度、`credit` 窗口与 8 MiB 链路缓冲                                                                                                                                                                                              |
| 中心看到业务内容                   | 载荷对中心透明，不解析、不落库、不写日志，与普通会话相同；中心对路由身份的可信程度也与今天相同                                                                                                                                                                                         |
| 授权撤销的延迟                     | 本地设置在下一次请求生效；名单发布后中心立即关闭会话；Host 停用、节点会话吊销时链路删除，会话以 `peer_gone` 结束                                                                                                                                                                       |

## 7. 兼容

| 组合                         | 行为                                                                           |
| ---------------------------- | ------------------------------------------------------------------------------ |
| 旧中心 ＋ 新 daemon          | `hub_features` 没有 `peer`，daemon 不声明、不发布，网关工具不出现              |
| 新中心 ＋ 旧 daemon 作为目标 | 不声明 `peer`，永远收不到对端会话，也不出现在任何快照中                        |
| 新中心 ＋ 旧 daemon 作为来源 | 不会发 `open`；行为不变                                                        |
| 新旧客户端                   | 以 `peer.set.request` 是否实现决定是否显示设置入口；未升级的客户端只是没有入口 |
| 中心重启                     | 链路失效，daemon 重连后重新发布；在途调用按“结果未知”返回                      |

## 8. 改动清单

**ait-server**

- `src/hub/wire.rs`：feature `peer`、`Kind::Mcp`、`Outbound::Open.peer`、`Outbound::Peers`。
- `src/hub/account.rs`：serve 发起的 `open` 分支、`{opener, target}` 会话表、对端名额、新关闭码；新增 `account/peers.rs`（名单、快照、撤销）。
- 新增 `src/hub/peers.rs`：`hub.peers.publish`，在 `src/hub/mod.rs` 的 `start_call` 中与分享、自动化方法一样由 Hub 自己处理（需要读写账号路由状态）；加入 `center_methods`。
- `src/metrics`：对端会话的建立、拒绝（按关闭码）和当前数量。
- 文档：`hub-protocol.md` 新增 6.3 节与 feature 表、`hub-services.md` 新增方法、ADR-007（修订方向规则，并说明 ADR-001、ADR-004 中机器凭据边界的受控放宽）。
- 不需要数据库迁移。

**ait**

- `crates/model`：`peer` 端口与值类型。`crates/domain`：`PeerPermission` 与工具可见性。
- `crates/relay`：`wire`（`peer` feature、`SessionKind::Mcp`、`Accept`、`Peers`）、入站 MCP 会话、出站会话管理、`hub.peers.publish` 发布。
- `crates/api`：`mcp::serve` 提取、`Principal::Peer`、三个网关工具、`peers.json` 设置与 `peer.*` RPC（仅所有者）、访客、自动化与对端策略中的拒绝项。
- `crates/persistence`：`peers.json` 的原子存储。`bins/daemon`：组装端口。
- `packages/protocol`、`packages/client`、`apps/mobile`：主机设置页“允许这些主机通过 MCP 调用本机”，逐个 Host 选择 `observe` 或 `operate`，带风险提示。
- `skills/ait-orchestration`、[MCP 手册](../operations/mcp.md)、[连接与传输规格](../architecture/connection.md)、新 ADR（修订 ADR-148、ADR-218，沿用 ADR-215 的过滤点）。

## 9. 分期

| 阶段 | 内容                                                                                                                                                          |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | ait-server：`peer` feature、会话路由、`hub.peers.publish` 与 `peers` 帧。可以先于 daemon 部署，没有 daemon 声明时无任何行为变化                               |
| 1    | ait：目标端入口与 `Peer` 身份、调用端网关工具、本地设置与 RPC、客户端设置页、技能与文档。权限为 `observe`／`operate`，范围为整台主机                          |
| 2    | 细粒度授权（复用 `BotPermissions` 的分组与模式上限，按项目或 Workspace 限定范围）、`ait.peer.host` 子树凭据、Bot 分组 `hosts`、任务总览显示跨主机父子关系     |
| 3    | 中心 HTTP MCP 入口（Open API scope `mcp:call`），以服务链路 `service:{kind:"mcp"}` 复用同一路由，由 Host 另行发布是否接受；经群组分享的跨账号调用（访客身份） |

## 10. 验证计划

- ait-server：`account` 单元测试覆盖 serve 发起 `open` 的允许与拒绝矩阵、未声明 feature 的链路仍为 `forbidden`、名额、名单撤销关闭会话、
  宽限期续传、`credit`、快照只含有授权关系的 Host、链路删除后的 `peer_gone`。
- ait：`relay` 以假中心测试出站会话的建立、复用、空闲关闭、`id` 关联和各关闭码；入站会话的并发上限。
  `api` 测试 `Peer` 身份的目录过滤、`observe`／`operate` 边界、保留标签、设置 RPC 对非所有者准入的拒绝、网关工具的出现条件与错误映射。
- 端到端：两个 daemon 加 ait-server 的测试栈（`compose.test.yaml`，dev-login），覆盖默认拒绝、授权后 `list_hosts` → `list_host_tools` → `call_host_tool(create_agent)` →
  `wait_for_agent`、运行中撤销、目标离线、与旧 daemon 混跑。按[仓库约定](../../AGENTS.md)只运行受影响 crate 的测试。

## 11. 待确认

1. 范围假设是否正确：调用其他主机的 Ait MCP，而不是隧道任意 MCP Server。
2. 首版只有 `observe` 与 `operate` 两级、范围为整台主机是否可接受；`operate` 是否需要排除 `terminals`。
3. Bot 是否需要在首版就能使用网关工具（它是最自然的跨主机编排者，但现有作用域规则只覆盖本机 Workspace）。
4. 网关工具的出现条件是否保持“至少有一台主机允许本机”，还是改为显式开关。
5. 阶段 3 的中心 MCP 入口与跨账号调用的优先级。
