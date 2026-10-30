# ADR-203：组织内工作区共享与访客会话

状态：采用。中心协议见中心仓库 `docs/hub-protocol.md` §6.1 与 `docs/hub-services.md`「工作区共享」，中心侧决策见其 ADR-005。

## 背景

中心新增组织：账号可以把本机的一个 workspace 共享给组织成员。访客通过 Hub 向分享人的 serve 链路开会话。Hub 只转发字节，看不到业务 RPC，所以「访客只能用这一个 workspace」只能由 daemon 执行。

## 决定

**分享设置以 Host 为准。**

- 设置保存在 `<data_dir>/hub/shares.json`，由本地 API 拥有，实现 `model::share::ShareBook`。
- 所有者通过本地方法 `share.list/set/remove/orgs.list.request` 管理；访客连接不能调用这些方法。
- 归档 workspace 或删除项目时，同时删除它的分享设置。

**发布由 daemon 负责**（`bins/daemon/src/host/hub/shares.rs`）。

- 发布条件：每条新逻辑链路建立时，以及每次本地设置变化后。只在中心声明了 `hub.shares.publish` 时发布，发布内容是完整列表。
- `revision`：内容不变时沿用原值，所以重试和换链路都是幂等的；内容变化时取 `max(上次 + 1, 当前毫秒数)`，daemon 重启后也只增不减。中心返回 `conflict` 时换一个新的 revision。
- 中心的处理结果，以及中心之后主动推送的 `share_status`，都交给 `ShareBook::apply`：
  - 记录 `shareId` 和 `generation`，下次发布时带上 `generation`；
  - 中心返回 `revoked` 时删除对应的本地设置；
  - 不再被接受的分享，立即结束它的访客会话。

**Hub 帧只发给已经声明支持的 Host。**

- serve 端认识两种新内容：`open.visitor` 字段和 `share_status` 帧。
- 中心只在 serve 链路发布过分享之后，才发送这两种内容。旧版 Host 遇到未知帧会断开链路，而且会忽略 `visitor` 字段，所以永远不会收到访客会话。

**访客连接的身份。**

- 桥接访客会话时，Hub 在连接本机 `/v1/ws` 的请求中带上 `x-ait-guest` 头（`model::share::GUEST_HEADER`，内容是 visitor 的 base64url JSON）。
- 本地 API 只在同时带有 daemon 自己 bearer 凭据时才认这个头，用浏览器票据连接时返回 403。
- 准入条件：workspace 存在且没有归档，本地有一条状态为已接受、`shareId` 与 visitor 一致、并且允许该访客的分享设置。
- 准入的连接被登记到该分享的授权下面，撤销分享时一并结束。
- 本机拒绝访客升级时，Hub 以 `share_revoked` 关闭会话。

**访客策略默认拒绝**（`crates/api/src/guest/policy*`）。请求、事件和二进制帧都先经过策略检查，再进入原有的校验和分发流程。

- workspace 方法只能作用于被分享的 workspace；`workspace.list` 的结果被过滤成只剩这一个，订阅被拒绝。
- 文件、Git、forge 方法的 `cwd` 必须是绝对路径，并且解析符号链接和 `..` 之后仍在 workspace 根目录内。上传被拒绝。每条连接最多同时持有 16 个未过期的下载令牌。
- 终端：新建时强制归属到这个 workspace；列表强制按这个 workspace 过滤；按 ID 操作的方法和 `terminal.input`，终端必须属于这个 workspace。
- Agent：
  - 新建时强制归属到这个 workspace，并打上 `ait.guest.share-id` 和 `ait.guest.user-id` 标签；禁止访客自带 worktree、git、env、`mcpServers`、`providerOptions` 等选项。
  - 带访客标签的 Agent 不注入 daemon MCP（ADR-202），因为这个 MCP 能访问所有 workspace。
  - 访客可以读取 workspace 内所有 Agent（get、timeline 等），但只能驱动本分享下访客自己创建的 Agent（发送消息、取消、权限决定、模型和模式设置等）。所有者的 Agent 带有 daemon MCP，不能由访客下达指令。
- 宿主级能力一律拒绝：daemon、project、schedule、bot、browser、语音、推送、relay、publication、全局会话事件订阅、`share.*`。
- 拒绝时返回 `forbidden`，不透露其他 workspace 是否存在。

**撤销。** 本地删除分享、中心撤销、成员关系变化或 workspace 归档时：

- 关闭该授权下的访客连接；
- 结束访客创建的终端；
- 取消访客创建的 Agent 当前进行中的回合，并拒绝它们待处理的权限请求；
- Agent 的历史记录保留。

## 后果

- `operate` 允许在被分享的 workspace 内使用终端和 Agent，等于以 Host 用户的身份执行任意命令。上面的策略限制的是 RPC 能触达的范围，不是沙箱。
- Git 操作会作用于整个仓库，forge 调用使用所有者的凭据。
- 访客授权和终端记录只保存在内存中，daemon 重启后清空；访客创建的 Agent 之后仍可以通过标签找到。
- Host 的中心地址固定为生产中心（`host_link::CENTER`），本地中心无法与当前 daemon 做端到端联调；中心仓库的真实 runtime 测试需要一个可配置中心地址的构建。
