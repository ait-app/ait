# 2026-11-02 仓库审查遗留事项

来源：2026-11-02 全仓库审查。已确认的缺陷已在同一 PR 中修复；本清单只记录复核后确认、
但当时尚未修复的问题。每项保留原始触发条件，完成后勾选并记录修复与回归测试。

2026-11-04：核对当前实现后补齐下列已勾选项；仍需真实 OpenCode 和中心协议确认的两项继续保留。

## Desktop

- [x] **未就绪的子进程仍会被交接给下一次会话。**
      `apps/desktop/src/daemon/rust-daemon.ts` 的 `retainForNextSession` 不检查
      `pendingReadiness`。daemon 一直没写出 `daemon ready` 就退出 Desktop 时，挂起的进程会被
      写进 `desktop-daemon.json`，下次启动只能反复探测失败。建议只在就绪确认后交接，
      或在退出时停止未就绪的进程（需修订 ADR-207）。
      修复：交接前检查 `pendingReadiness`，从未输出就绪日志的子进程由现有退出流程停止；日志就绪后的握手失败仍允许交接。已修订 [ADR-207](../decisions/clients/adr-207-desktop-daemon-readiness-ownership.md)。回归：`does not hand over a child that never reported ready` 及原有交接、退出生命周期测试。

## 节点授权与发布（`crates/host-link`）

- [x] **非授权类协议错误会永久停止 Hub/Relay 发布。**
      `bins/daemon/src/device/http.rs` 把重定向（如需要网页登录的 Wi-Fi）、非 JSON 的 200
      响应、无错误码的 4xx 和取票 403 都映射为 `Error::Protocol`；`Authority::maintain` 遇到它就返回，
      `Service::maintain` 随后进入等待命令状态。由于 `authorized()` 仍为真，Desktop 的自动
      授权会被忽略，Hub 与 Relay 保持离线直到重启或显式重新授权。建议把传输层协议错误
      视为可重试并按现有退避重试，只有 `Unauthorized`、`Denied`、`Storage` 终止。
      修复：初始刷新和授权维护均将 `Protocol` 按现有退避重试。回归：`protocol_failures_retry_initial_refresh_with_the_same_request`、`protocol_failures_retry_registration_and_renewal_without_reauthorization`。

- [x] **access 401 恢复后约 14 分钟内的下一次 401 直接要求重新登录。**
      `controller/authority.rs` 中 `access_recovery` 在刷新后变为 `Retried`，成功续租不会把它
      重置为 `Ready`，只有下一次计划刷新才会。窗口内任何无关的 401（包括 Hub 取票）都会返回
      `Unauthorized`。直接在成功续租后重置会破坏「持续 401 只探测一次」的保证
      （`persistent_access_refusal_probes_refresh_once_then_stops`）；需要区分「刷新后的 JWT
      已被中心接受」与「同一 JWT 再次被拒」，例如记录被拒 JWT 的 generation。
      修复：按令牌 generation 与拒绝端点记录已恢复证据，成功续租或该端点接受新令牌后恢复探测预算；持续拒绝仍只刷新一次。回归：`accepted_successor_lease_can_recover_a_later_access_refusal`、`accepted_successor_ticket_can_recover_a_later_access_refusal`、`persistent_hub_ticket_refusal_is_not_hidden_by_successful_renewals`。

## daemon MCP socket

- [x] **socket 在 `chmod 0600` 之前短暂按 umask 权限可连接。**
      `bins/daemon/src/host/mcp_socket.rs` 先 `bind` 再按路径 `set_permissions`，后者跟随
      符号链接。父目录可被他人写入时存在竞争窗口。所有请求仍需 Bearer 令牌，影响较低。
      建议在 `bind` 前临时设置 umask `0o177`，或要求父目录归当前用户所有且为 0700。
      修复：绑定前验证父目录不是符号链接、归当前用户所有且权限为 0700；不修改进程全局 umask。已修订 [ADR-203](../decisions/daemon/adr-203-mcp-unix-socket.md)，回归位于 `host/mcp_socket/tests.rs`。

## Provider

- [x] **OMP：被拒绝的 steer 可能让下一条 prompt 带上错误的客户端消息 ID。**
      `crates/provider/src/local/omp/session.rs` 中 steer 进入 `pending_inputs` 后，回合结束时
      其迟到的拒绝被当作旧回合忽略，而 `finish()` 不清理 `pending_inputs`。建议把客户端 ID
      与每个输入请求绑定，在过期或失败响应时移除。
      修复：把客户端消息 ID 绑定到输入请求 ID，迟到或失败回执只清理对应输入，回合结束清理未消费输入。回归：`late_steering_rejection_cannot_claim_the_next_prompts_client_message_id`。

- [x] **OpenCode：列出子 Agent 会长时间占用父 Agent 队列。**
      `service/agent_manager/controls.rs` 每次列表请求都会启动新的 OpenCode 进程，回放目录内
      全部会话（最多 4096 个）及其后代，并在父 Agent 的执行队列中运行，最长 120 秒；超时
      时 `transport.close()` 被 `?` 跳过，只由 `Drop` 结束进程。建议 live 会话已有子 Agent 时
      跳过发现，并在所有退出路径上关闭 transport。
      修复：已有 live 子 Agent 时复用观测结果，发现的成功、错误和超时路径均关闭 transport。回归：`opencode_subagents_are_persisted_and_queryable_after_host_restart`、`timed_out_child_discovery_reaps_its_native_process_before_returning`。

- [x] **OpenCode：替换 transport 后后台子 Agent 仍显示为运行中。**
      `opencode/session.rs` 的 `prepare()` 在权限或系统提示变化时关闭旧 ACP 进程，后台子 Agent
      随之结束，但没有调用 `children.stopped()`。建议在关闭旧进程后标记已观测的子 Agent 为取消。
      修复：关闭旧 transport 后调用 `children.stopped()`。回归：`replacing_the_native_process_cancels_its_observed_background_children`。

- [x] **OpenCode / Codex：未知的子事件类型或状态会让父会话失败。**
      `opencode/subagents/live.rs` 对未知 frame 类型或状态返回错误，`codex/subagents.rs` 对未知
      activity 类型同样如此；与 5b072810 修复的 `completed` 属于同一类问题。建议忽略未知值并记录
      调试日志。
      修复：未知 frame、状态和 activity 类型记录调试日志后忽略，保留既有子会话状态和父会话。回归：`unknown_child_frames_and_statuses_leave_observed_children_unchanged`、`unknown_activity_kinds_do_not_fail_the_parent_or_change_child_lifecycle`。

- [ ] **OpenCode（待确认）：恢复后的子会话通知可能使回合失败。**
      `reload` 在同一连接上加载子会话；回合中来自这些子会话的 `session/update` 只接受命令或
      配置更新，`usage_update` 等会使回合失败，子会话的权限请求在 `capture` 与 `poll_idle`
      中也会被拒绝。需先用真实 OpenCode 确认加载后是否会发送这些通知。
      2026-11-04：本机 OpenCode 为 2.0.32，早于 ADR-206 所需的子会话扩展版本，不能据此关闭该项。
      2026-11-10：本机 OpenCode 已升级到 2.2.5，晚于 ADR-206 引用的 2.1.4 子会话事件，版本不再是阻碍；
      尚未用真实会话复现 `reload` 后的子会话通知，仍不能关闭。

## 资源共享与访客会话（ADR-208）

具备 `operate` 权限的访客能以 Host 用户身份执行命令，因此下列问题属于 RPC 范围与撤销清理的
缺口，而非绕过操作系统边界。工作区与项目范围的 ID 绑定、路径校验和终端所有权检查已复核通过。

- [x] **访客终端可能逃出追踪，撤销时不会被关闭。**
      `crates/api/src/guest/policy/private.rs` 在 `terminal.create.request` 返回后才调用
      `record_terminal`。单连接模式下访客发出创建请求后立即断开，或撤销恰好发生在创建过程中，
      `process_message` future 会被丢弃；终端服务的阻塞任务仍会启动进程，但记录与「撤销时
      已启动」的回退清理都不会执行。建议在终端服务创建时直接打上 `(shareId, userId)` 标记并按
      标记撤销（与 Agent 标签一致），至少把「创建并记录」放进不受连接取消影响的独立任务。
      修复：在受运行时跟踪的独立任务中完成创建、归属记录与撤销检查；连接取消不再丢弃清理。回归：`terminal_creation_keeps_tracking_after_the_request_is_dropped`、`already_revoked_creation_never_starts_a_process`。

- [x] **较早的运行中终端会被挤出所有权列表。**
      `crates/api/src/shares/guests.rs` 的 `record_terminal` 在达到 `MAX_TERMINALS = 128` 时
      `pop_front`，且已结束的终端从不移除。访客先开一个长期终端，再反复创建并关闭 128 个，
      第一个终端就不再受撤销清理（访客也失去其输入权限）。建议在终端结束时移除 ID，按存活状态
      清理而非淘汰最旧项；Host 最多 32 个存活终端，列表仍有界。
      修复：不再淘汰最老的 ID；对已记录 ID 查询存活状态，仅移除确认退出的终端，保留查询期间新增的 ID。回归：`terminal_tracking_keeps_live_owners_and_prunes_only_confirmed_exits`。

- [x] **撤销只取消前 200 个访客 Agent。**
      `crates/api/src/shares/cleanup.rs` 的 `cancel_agents` 调用 `agent.list.request` 时未传
      `page`，默认上限 200，也不跟随 `pageInfo.nextCursor`。建议按 `guest_grants` 的方式分页。
      修复：按固定 `created_at` 排序跟随目录游标，避免取消导致更新时间变化而漏页；个别取消失败不阻断后续 Agent。回归：`revocation_cancels_agents_beyond_the_first_directory_page`、`one_failed_cancellation_does_not_skip_later_agents`。

- [x] **撤销期间创建的 Agent 首个回合不会被取消。**
      已排入 provider 队列、带 `initialPrompt` 的 `agent.create` 不会因连接断开而停止；若它在
      `cancel_agents` 列出 Agent 之后才注册，首个回合照常运行。建议在以访客标签创建 Agent 后
      复核 `Book::grants`，授权已撤销则取消该回合。
      修复：创建独立任务保留进度转发并在结果返回后复核授权，已撤销则取消新增 Agent 的回合。回归：`agents_created_after_revocation_are_cancelled_in_both_response_shapes`。

- [x] **节点访客创建的 Bot 与定时任务在撤销后仍会运行。**
      `guest/policy/node.rs` 允许节点访客调用 `bot.*` 与 `schedule.*`（例如把 Bot Channel 指向
      访客自己的聊天账号，或创建启动无标签 Agent 的定时任务）；撤销只处理连接、访客终端和带
      访客标签的 Agent。建议记录创建者并在撤销时删除或暂停，或在 ADR-208 中明确说明该边界。
      修复：后台自动化写操作仅限所有者；节点访客保留 Bot 与定时任务读取，客户端同步只读入口。边界记录在 [ADR-208](../decisions/daemon/adr-208-workspace-share-guests.md)。回归：`node_guests_can_read_but_cannot_start_persistent_automation` 及客户端只读回归。

- [x] **项目访客可永久删除所有者的其他分享设置。**
      `workspace.archive` 属于 `WorkspaceManage`。项目访客归档项目内另有独立工作区分享的工作区时，
      `connection/workspace_archive.rs` 经 `shares.rs` 的 `forget` 从 `shares.json` 删除该设置并
      断开其他访客，取消归档也无法恢复；分享管理本应仅限所有者。建议访客触发的归档改为暂停这些
      设置，或拒绝归档自带分享的工作区。
      修复：项目访客归档自带分享设置的工作区时被拒绝，包括 worktree 归档路径；设置仍由所有者管理。回归：`project_guests_cannot_archive_an_independently_shared_workspace`。

- [x] **路径校验使用未裁剪的 `cwd`（潜在问题）。**
      访客策略检查原始 `cwd`（`guest.rs`、`policy.rs`），而 `fs.explorer` 与下载令牌处理
      （`crates/filesystem/src/files/rpc.rs`）先 `trim`。工作区同时存在名为 `link `（尾随空格）
      的目录和指向 `/` 的符号链接 `link` 时，`cwd="/ws/link "` 通过检查，实际读取 `/`；
      两者都可经仓库内容带入。引入只读分享后即成为越界读取。建议拒绝未裁剪的 `cwd`，或检查
      裁剪后的值。
      修复：在 scoped 路径检查中拒绝与 `trim` 后不同的 cwd。回归：`trailing_whitespace_and_symlink_aliases_do_not_bypass_path_checks`。

- [x] **两处 RPC 范围缺口。**
      `checkout.pr.timeline` 与 `checkout.*.get_check_details`（`forge/rpc.rs`）接受任意
      `repoOwner`/`repoName`，访客可读取所有者 `gh` 登录可见的任意仓库 PR 时间线与检查详情；
      策略只校验 `cwd`。项目访客被禁止 `project.config.write`，却可通过 `fs.file.write` 直接写
      `<项目根>/ait.json`。建议将仓库参数绑定到 `cwd` 的远端，并禁止访客写项目配置文件。
      修复：通过 filesystem 的 Forge 端口核对 checkout 远端与显式仓库身份；文件写入、创建、重命名和删除均保护项目配置及其别名。回归：`explicit_forge_identity_requires_a_matching_checkout`、`guest_repository_identity_must_match_the_checkout_origin`、`scoped_file_mutations_cannot_replace_project_configuration`。

- [ ] **（待确认）被拒访客可能在下一次结果中被重新放行。**
      `shares.rs` 的 `apply_outcome` 用每次新结果替换 `book.denied`，被拒授权又已从发布的
      `visitors` 中移除，下一次结果可能清空拒绝记录。仅当中心仍会为被拒访客路由会话时才有影响，
      需确认中心行为。

## Client

- [x] **计划列表只查询共享主机组的代表成员。**
      `apps/mobile/src/hooks/use-schedules.ts` 使用 `groupHosts(hosts).map(h => h.serverId)`，
      而历史与计划页过滤期望组内全部成员。若访客授权可以列出计划，只能经非代表授权访问的计划会
      缺失，代表授权离线时也会一直显示连接中。
      修复：复核 node 授权允许读取计划，查询和连接状态跟踪展开到组内全部成员；非代表授权在线时也能返回计划。回归：`use-schedules.hosts.test.tsx`。

- [x] **原生端按压高亮与状态徽标描边颜色不一致。**
      `use-sidebar-row-press.ts` 在原生端始终返回 `isPressed: false`，但
      `getSidebarRowBackdrop({ isPressed })` 仍决定项目图标状态徽标的镂空颜色；`PressHighlight`
      绘制 `surface2` 时徽标周围会出现 `surfaceSidebar` 色的光圈。需要从 `PressHighlight`
      暴露按压状态（共享值或回调）。
      修复：PressHighlight 从原生手势开始和结束回调同步高亮状态，徽标使用相同背景；行底色仍由原生覆盖层负责。回归：`use-sidebar-row-press.test.tsx`、`sidebar-press-feedback.test.tsx`。

- [x] **重连提示不会在连接恢复时关闭。**
      `apps/mobile/src/runtime/use-online-service-connection-notice.ts` 的加载态提示使用固定时长
      （重连使用默认 2.2 秒，不可用为 3.2 秒）；连接很快恢复时仍显示「正在重连」。建议在状态回到
      `online` 时主动关闭当前提示。
      修复：toast 返回只作用于自身的关闭回调；连接恢复、退出账号、切换连接或卸载时关闭提示，不会关闭后来出现的其他提示。回归：`use-online-service-connection-notice.test.tsx`、`toast-host.test.tsx`。

## 2026-11-13 全仓库审查遗留事项

来源：2026-11-13 的全仓库审查（Rust 工作区 + 移动端、Web 与桌面端）。已确认且改动较小的缺陷
已在同一批改动中修复；下列各项经复核后确认但尚未修复，各自保留原始触发条件。

- [ ] **一个无法落盘的定时任务运行会冻结全部定时任务。**
      `crates/schedule/src/service.rs` 的 tick 只在 `pending` 为空时才调用 `engine.due()`。
      一次结算失败（存储写不进去）的运行会永久留在 `pending` 里，之后所有定时任务都不再启动，
      `schedule.run_once.request` 也一律返回 `Error::Storage`。
      建议：`due()` 与 `begin()` 已按「运行中的 occurrence」拒绝重复运行，tick 无需再用 `pending`
      挡住全部调度。

- [ ] **定时任务的运行记录不会裁剪，文档可能超过存储上限后无法再写。**
      `crates/schedule/src/engine.rs` 只限制单个任务的运行数量（4096），不裁剪历史；一次运行最多
      保留 64 KiB 输出。文档超过 `crates/persistence/src/storage/schedule.rs` 的 16 MiB 上限后，
      `save` 返回 `Error::Conflict`，该任务的运行停在 `Running`，删除与修改也被拒绝。
      建议：保存冲突时丢弃最旧的已结算运行（含输出）后重试一次。

- [ ] **一条损坏的定时任务记录会让 daemon 无法启动。**
      `Engine::open` 是全有或全无：提示超过 64 KiB、`maxRuns` 为 0、Agent id 不是 UUID 或
      时区无法解析的记录会让整个打开失败，错误经 `Schedules::spawn` 冒泡到 daemon 启动，
      此时 API 尚未起来，没有任何修复入口。
      建议：隔离这一条记录（标记为已结束）并保留其余记录。

- [ ] **权限答复在追问失败时不会送达 Provider。**
      `crates/provider/runtime/src/service/agent_manager/controls.rs` 先解析追问、再把追问答复
      投递给 Provider，最后才调用 `respond_permission`。追问投递失败（队列忙、Provider 拒绝）
      时提前返回，Provider 永远收不到这次权限答复，Agent 会一直停在 `permission` 状态。
      建议：先答复 Provider，再投递追问；追问失败不应跳过答复。

- [ ] **lane 失败时不会关闭其原生会话。**
      `crates/provider/runtime/src/service/agent_execution/lane.rs` 只在关停、通道关闭和空闲
      过期时调用 `close(state)`；出错分支直接跳出，`ExecutionState` 连同其中的原生会话被丢弃。
      子进程会随 `Drop` 结束，但持久状态仍是 `Running`，lane 的路由别名也不会释放。
      建议：出错分支同样关闭 lane 的状态。

- [ ] **`agent.items.close.request` 会吞掉归档失败。**
      `crates/provider/runtime/src/rpc/agent_runtime.rs` 用 `filter_map` 丢弃归档错误，答复里
      的 `agents` 列表因此可能短于请求的 id；调用方无法区分「没有可关闭的」和「关闭失败」。
      同一路径还直接使用请求里的原始 id，而目录查询支持前缀解析，短 id 会静默关闭不到任何东西。
      建议：先解析 id，再逐个报告失败。

- [ ] **运行时为每个被改动的 Agent 保留一条永久别名。**
      `Owners::agent` 会在每次改动时插入 `id -> agent:{id}`，只有 lane 拆除时才会移除；
      `loaded_timelines` 也只在硬删除时清理，存活进程内的 Agent 越多，两个集合越大。
      建议：作用域查询不生成路由别名；归档时一并清理历史标记。

- [ ] **排队输入的 Agent 列表被截断在 1024 个。**
      `crates/provider/runtime/src/storage/timeline/inbox.rs` 的 `queued_agents` 用
      `ORDER BY agent LIMIT 1024`，超出部分在没有活跃 lane 时不会获得 lane，其排队输入一直
      得不到投递。
      建议：对身份查询分页，或去掉该上限（它不读取提示正文）。

- [ ] **DSH 的 `cancel_turn` 在回合真正结束前就返回。**
      `crates/provider/deepseek-harness/src/native/session.rs` 只发送 `session/cancel`，
      `self.active` 要等原生 `turn/end` 才清空；原生不再发这个事件时，运行时又会因为
      `interruption_requested` 放弃重试，该会话此后的每次 `start` 都被拒绝。
      建议：在有界超时内等待回合结束，超时后清空 `self.active` 并给出取消事件。

- [ ] **ACP 未答复的权限请求被丢弃，却告诉宿主已经解决。**
      `crates/provider/native/src/acp_streaming.rs` 的 `settle` 把待答复的权限请求直接转成
      `PermissionResolved` 事件，没有给原生会话发送 ACP 答复；原生若在权限未决时中止了回合，
      它的请求 id 永远收不到答复。OpenCode 适配器发送 `cancelled` 才是正确做法。
      建议：结算前先撤回待答复的权限。

- [ ] **自动化运行时从不释放每个工作区的状态。**
      `crates/metadata/src/automation/local.rs` 的 `scripts`、`runs`、`setups` 与
      `workspace_ports` 只有插入没有删除；工作区归档或删除后，其运行历史（每个运行最多 64 KiB
      输出缓冲）和脚本进程仍然常驻，daemon 的内存随见过的工作区数量单调增长。
      建议：工作区归档或删除时调用一个释放该工作区状态的入口。

- [ ] **创建回执永不淘汰，每次创建都会重写整个文件。**
      `crates/persistence/src/storage/creation.rs` 的 `put` 只插入不删除，回执文件随 daemon
      存活时间单调增长；每次创建都要重新序列化并同步整个数组。
      建议：给回执加上淘汰策略，并在写入时顺带清理过期项。
