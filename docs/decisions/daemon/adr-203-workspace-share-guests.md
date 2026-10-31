# ADR-203：群组内资源共享与访客会话

状态：采用。中心协议见中心仓库 `docs/hub-protocol.md` §6.1 与 `docs/hub-services.md`「资源共享」，中心侧决策见其 ADR-005。

## 背景

中心新增群组：账号可以把本机的一个 workspace、一个项目或整个节点（Host）共享给群组成员。访客通过 Hub 向分享人的 serve 链路开会话。Hub 只转发字节，看不到业务 RPC，所以「访客只能用被分享的资源」只能由 daemon 执行。

## 决定

**分享对象。** 每条分享由 `(resourceKind, resourceKey, groupId)` 唯一确定：

| `resourceKind` | `resourceKey`         | 访客可触达的范围                     |
| -------------- | --------------------- | ------------------------------------ |
| `workspace`    | workspace ID          | 这一个 workspace                     |
| `project`      | 项目 ID（如 `prj_…`） | 该项目现有和以后新建的全部 workspace |
| `node`         | 空字符串              | 本机所有项目和 workspace             |

**分享设置以 Host 为准。**

- 设置保存在 `<data_dir>/hub/shares.json`，由本地 API 拥有，实现 `model::share::ShareBook`。读取时逐条解析，无法识别的条目（例如改名前写入的旧格式条目、没有 `settingId` 的条目）被丢弃，不会让 daemon 无法启动。
- 用户新建一条设置时生成 `settingId`，修改设置时保持不变，用户不能指定。中心把撤销记在这个 ID 上，所以撤销之后，无论是重试旧请求还是离线后重新上线，这条设置都不会恢复；用户重新分享会得到新的 ID。
- 所有者通过本地方法 `share.list/set/remove/groups.list.request` 管理；访客连接不能调用这些方法。`share.set` 校验：workspace 必须存在且未归档，项目必须存在且未删除，节点分享的 key 必须为空。
- 被所有者在其他设备上撤销的设置保留为状态 `revoked` 的本地记录，让所有者看得到它为什么停了。这条记录不再发布，也不准入任何访客，不占 64 条分享的名额，最多保留 64 条（先删最旧的）。对同一 `(resourceKind, resourceKey, groupId)` 再次 `share.set` 会用新的 `settingId` 替换它；`share.remove`、归档 workspace 和删除项目都会删掉它。
- 归档 workspace 时删除该 workspace 的分享；删除项目时删除该项目的分享，以及项目下各 workspace 的分享。

**发布由 daemon 负责**（`bins/daemon/src/host/hub/shares.rs`）。

- 发布条件：每条新逻辑链路建立时，以及每次本地设置变化后。只在中心声明了 `hub.shares.publish` 时发布，发布内容是完整列表：`{revision, shares:[{resourceKind, resourceKey, groupId, permission, audience: "group"|"members", recipients?, displayName?, projectName?, settingId}], visitors:[{shareId, userId}]}`。
- `visitors` 是本机保存着状态（终端、Agent）的访客授权，最多 4096 项。它的变化不触发发布：中心会自己加上之后准入的访客。
- daemon 重启后，访客授权的内存记录会清空。第一次发布之前，daemon 按 `ait.guest.share-id` 和 `ait.guest.user-id` 标签列出访客创建的 Agent（最多 50 页），把这些 `(shareId, userId)` 恢复为没有连接的授权，所以它们仍会出现在 `visitors` 里，中心报告 `denied` 时照样清理；本地已不再接受的分享对应的授权会立即被撤销。重启前的终端随 daemon 一起结束，不需要恢复。
- `revision`：内容不变时沿用原值，所以重试和换链路都是幂等的；内容变化时取 `max(上次 + 1, 当前毫秒数)`，daemon 重启后也只增不减。中心返回 `conflict` 时换一个新的 revision。
- 中心的处理结果 `{revision, accepted, rejected, denied, groups}`，以及中心之后主动推送的 `share_status`，都交给 `ShareBook::apply`：
  - `revision` 小于已应用结果的被忽略；`accepted`/`rejected` 的条目只作用于 `settingId` 相同的本地设置，迟到的旧结果不会影响用户重新创建的设置；
  - 记录 `shareId` 和 `missingRecipients`，暂时不是成员的接收人不被准入；
  - 中心返回 `revoked` 时把对应的本地设置改成 `revoked` 记录（见上），不再发布；其他拒绝码（`group_inactive`、`not_member`、`missing_name`、`no_recipients`，以及中心复查发现分享已失效时的 `inactive`）保留设置并标记为被拒绝，下次发布时由中心重新判断；
  - `denied` 列出分享仍被接受、但访客本人已失去授权的 `(shareId, userId)`，这些访客不再被准入；
  - 不再被接受的分享和被 `denied` 的访客，立即结束其访客会话并按下文「撤销」清理，访客离线时也一样。

**Hub 帧只发给已经声明支持的 Host。**

- serve 端认识两种新内容：`open.visitor` 字段和 `share_status` 帧。
- 中心只在 serve 链路发布过分享之后，才发送这两种内容。旧版 Host 遇到未知帧会断开链路，而且会忽略 `visitor` 字段，所以永远不会收到访客会话。

**访客连接的身份。**

- 桥接访客会话时，Hub 在连接本机 `/v1/ws` 的请求中带上 `x-ait-guest` 头（`model::share::GUEST_HEADER`，内容是 visitor `{user_id, display_name, username?, group_id, share_id, resource_kind, resource_key, permission}` 的 base64url JSON）。`display_name` 和 `username` 只用来标注操作人，不参与授权；老中心不发 `username`。
- 本地 API 只在同时带有 daemon 自己 bearer 凭据时才认这个头，用浏览器票据连接时返回 403。
- 准入条件：workspace 分享要求 workspace 存在且没有归档；项目分享要求项目存在且没有删除；节点分享要求 key 为空。另外本地必须有一条 `(resource_kind, resource_key, group_id)` 相同、状态为已接受、`shareId` 与 visitor 一致、并且允许该访客的分享设置。
- 准入的连接被登记到该分享的授权下面，撤销分享时一并结束。
- 本机拒绝访客升级时，Hub 以 `share_revoked` 关闭会话。

**访客策略默认拒绝**（`crates/api/src/guest/policy*`）。请求、事件和二进制帧都先经过策略检查，再进入原有的校验和分发流程。拒绝时返回 `forbidden`，不透露范围之外的资源是否存在。

- 范围内的 workspace：workspace 分享只有它自己；项目分享是 `projectId` 等于该项目的全部 workspace（包括已归档的）；节点分享是全部 workspace。
- 路径（文件、Git、forge、下载、终端、Agent 的 `cwd`）：workspace 分享要求绝对路径在解析符号链接和 `..` 之后位于 workspace 目录内；项目分享要求位于项目根目录内，或位于该项目任一未归档 workspace 的目录内（受管 worktree 通常在项目根目录之外）；节点分享不检查路径。上传被拒绝。每条连接最多同时持有 16 个未过期的下载令牌。
- 下载令牌绑定访客：访客连接签发的令牌在回复离开前绑定到该访客的 `user_id`。访客下载只走旧的 relay，中心为分享下载在 `open_data` 里带上 `visitor_user_id`（UUID，所有者自己的下载和老中心不带），daemon 的 relay 把它放进本机 `/api/files/download` 请求的 `x-ait-download-visitor` 头（`model::share::DOWNLOAD_VISITOR_HEADER`）。绑定到访客 U 的令牌只在下载人是 U 时兑现；所有者签发的令牌在声明了访客时一律拒绝；不匹配不会消耗令牌。没有这个头的本机请求只能兑现所有者的令牌。
- workspace 方法（get、setup、script、attention 等）只能作用于范围内的 workspace。
- 列表：
  - workspace 分享和项目分享的 `workspace.list` 私下执行，去掉同步游标，结果只保留范围内的 workspace；订阅被拒绝，因为订阅事件覆盖整个目录。项目分享把查询过滤为该项目并保留访客的分页，分页因此只覆盖该项目，`emptyProjects` 也只保留该项目。workspace 分享逐页读取目录直到找到该 workspace（最多 100 页），结果最多一项。
  - 项目分享的 `project.list` 只返回该项目；workspace 分享不能列项目。
  - 节点分享原样转发两种列表，包括订阅。
- 项目分享还可以：
  - `workspace.create`：来源目录（`path` 或 worktree 的 `cwd`）必须在范围内，`projectId` 被强制为该项目；
  - `workspace.worktree.create`：`cwd` 必须在范围内，`projectId` 被强制为该项目；
  - `workspace.open`：只能打开该项目某个未归档 workspace 的目录。打开其他目录可能登记出新的项目，因此在项目根目录下新建 workspace 需要用 `workspace.create`；
  - 重命名、归档范围内的 workspace，归档其 worktree（`workspace.worktree.archive` 的 workspace 和路径都必须在范围内），列 worktree，读取该项目的图标和配置。
  - 不能重命名、删除、配置项目或设置图标（`project.rename/remove/config.write/icon.set`），不能添加项目，也不能触达其他项目。
- 节点分享还可以添加和创建项目（`project.add`、`project.create_directory`、`project.github.clone`），重命名、删除、配置项目，置顶、标签、恢复和 setup 运行等 workspace 管理。
- 终端：新建时必须归属到范围内的 workspace（workspace 分享强制为该 workspace；项目分享必须给出该项目的 `workspaceId`，未给 `cwd` 时使用该 workspace 的目录；节点分享不改写）；列表在 workspace 和项目分享下必须指定范围内的 workspace。访客可以订阅、截取范围内的任何终端（节点分享是全部终端），但**只能操作自己通过本分享创建的终端**：`terminal.input`、二进制输入和尺寸帧、`terminal.kill`、`terminal.rename`、`agent.items.close` 中的终端都要求是自己创建的，同一分享的其他访客和所有者的终端都不行，因为所有者的 shell 可能已经 `cd` 到范围之外，或者缓存了 sudo 凭据。订阅别人的终端时，请求里的尺寸声明（`restore.size`）会被去掉，访客不会接管终端尺寸。访客创建的终端按 `(shareId, userId)` 记录，换连接后仍然归这个访客。
- Agent：
  - 新建时打上 `ait.guest.share-id` 和 `ait.guest.user-id` 标签，禁止访客自带 `env`、`mcpServers`、`providerOptions` 和访客标签，`callerAgentId` 必须在范围内。workspace 分享强制归属到该 workspace；项目分享必须给出该项目的 `workspaceId`，并禁止 worktree、git 等新建位置的选项；节点分享可以放到任意 workspace，也可以新建 worktree。
  - `workspace.create` 不能附带初始 Agent，因为它会绕过上面的 Agent 规则并获得 daemon MCP。
  - 带访客标签的 Agent 不注入 daemon MCP（ADR-202），因为这个 MCP 能访问所有 workspace。
  - 访客可以读取范围内所有 Agent（get、timeline 等），但只能驱动自己通过本分享创建的 Agent（发送消息、取消、权限决定、模型和模式设置等），同时校验 `ait.guest.share-id` 和 `ait.guest.user-id`；同一分享的其他访客创建的 Agent 也只能读取。所有者的 Agent 带有 daemon MCP，不能由访客下达指令。Agent 列表在 workspace 和项目分享下被强制过滤为范围内的 workspace。
- 宿主级能力一律拒绝，节点分享也一样：`daemon.*`、`server.restart/shutdown`、诊断、daemon 配置和更新、schedule、bot、browser、relay、publication、push、语音和听写、编辑器、`share.*`、Agent 定义和技能的配置、导入、刷新、用量统计、`provider.sessions.recent`，以及全局会话事件订阅 `session.events.set_subscription`（它覆盖 Host 上的全部会话事件，保持拒绝最简单也最安全）。

**撤销。** 本地删除分享、中心撤销、成员关系变化（含中心以 `denied` 报告的单个访客）、workspace 归档或项目删除时：

- 关闭该授权下的访客连接；
- 结束访客创建的终端；
- 取消访客创建的 Agent 当前进行中的回合，并拒绝它们待处理的权限请求；
- Agent 的历史记录保留。

## 后果

- `operate` 允许在被分享的范围内使用终端和 Agent，等于以 Host 用户的身份执行任意命令。上面的策略限制的是 RPC 能触达的范围，不是沙箱。节点分享几乎等同于把这台机器交给访客，只是不能管理 daemon 本身。具体来说：
  - workspace 分享里，`checkout.*` 以整个仓库为单位，访客能读到 workspace 目录之外的仓库文件；
  - 项目分享的访客可以归档项目里任何 workspace；
  - 节点分享的访客可以删除项目。
    群组里有不完全信任的成员时，需要以后的只读权限，或者操作系统层面的隔离。
- Git 操作会作用于整个仓库，forge 调用使用所有者的凭据。
- 项目分享的目录范围在连接内缓存；新建的 workspace 会在下一次未命中时重新读取，刚归档的 workspace 目录在下一次重新读取前仍算在范围内。
- 访客授权和终端记录只保存在内存中；daemon 重启后按 Agent 标签恢复授权（见上），终端归属不恢复，重启前的终端随 daemon 一起结束。
- Host 的中心地址固定为生产中心（`host_link::CENTER`），本地中心无法与当前 daemon 做端到端联调；中心仓库的真实 runtime 测试需要一个可配置中心地址的构建。
