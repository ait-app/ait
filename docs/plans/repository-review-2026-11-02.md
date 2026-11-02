# 2026-11-02 仓库审查遗留事项

来源：2026-11-02 全仓库审查。已确认的缺陷已在同一 PR 中修复；本清单只记录复核后确认、
但尚未修复的问题。每项写明触发条件、影响和建议做法，完成后勾选，并附上修复 PR 和回归测试。

## Desktop

- [ ] **未就绪的子进程仍会被交接给下一次会话。**
      `apps/desktop/src/daemon/rust-daemon.ts` 的 `retainForNextSession` 不检查
      `pendingReadiness`。daemon 一直没写出 `daemon ready` 就退出 Desktop 时，挂起的进程会被
      写进 `desktop-daemon.json`，下次启动只能反复探测失败。建议只在就绪确认后交接，
      或在退出时停止未就绪的进程（需修订 ADR-207）。

## 节点授权与发布（`crates/host-link`）

- [ ] **非授权类协议错误会永久停止 Hub/Relay 发布。**
      `bins/daemon/src/device/http.rs` 把重定向（如需要网页登录的 Wi-Fi）、非 JSON 的 200
      响应、无错误码的 4xx 和取票 403 都映射为 `Error::Protocol`；`Authority::maintain` 遇到它就返回，
      `Service::maintain` 随后进入等待命令状态。由于 `authorized()` 仍为真，Desktop 的自动
      授权会被忽略，Hub 与 Relay 保持离线直到重启或显式重新授权。建议把传输层协议错误
      视为可重试并按现有退避重试，只有 `Unauthorized`、`Denied`、`Storage` 终止。
- [ ] **access 401 恢复后约 14 分钟内的下一次 401 直接要求重新登录。**
      `controller/authority.rs` 中 `access_recovery` 在刷新后变为 `Retried`，成功续租不会把它
      重置为 `Ready`，只有下一次计划刷新才会。窗口内任何无关的 401（包括 Hub 取票）都会返回
      `Unauthorized`。直接在成功续租后重置会破坏「持续 401 只探测一次」的保证
      （`persistent_access_refusal_probes_refresh_once_then_stops`）；需要区分「刷新后的 JWT
      已被中心接受」与「同一 JWT 再次被拒」，例如记录被拒 JWT 的 generation。

## daemon MCP socket

- [ ] **socket 在 `chmod 0600` 之前短暂按 umask 权限可连接。**
      `bins/daemon/src/host/mcp_socket.rs` 先 `bind` 再按路径 `set_permissions`，后者跟随
      符号链接。父目录可被他人写入时存在竞争窗口。所有请求仍需 Bearer 令牌，影响较低。
      建议在 `bind` 前临时设置 umask `0o177`，或要求父目录归当前用户所有且为 0700。

## Provider

- [ ] **OMP：被拒绝的 steer 可能让下一条 prompt 带上错误的客户端消息 ID。**
      `crates/provider/src/local/omp/session.rs` 中 steer 进入 `pending_inputs` 后，回合结束时
      其迟到的拒绝被当作旧回合忽略，而 `finish()` 不清理 `pending_inputs`。建议把客户端 ID
      与每个输入请求绑定，在过期或失败响应时移除。
- [ ] **OpenCode：列出子 Agent 会长时间占用父 Agent 队列。**
      `service/agent_manager/controls.rs` 每次列表请求都会启动新的 OpenCode 进程，回放目录内
      全部会话（最多 4096 个）及其后代，并在父 Agent 的执行队列中运行，最长 120 秒；超时
      时 `transport.close()` 被 `?` 跳过，只由 `Drop` 结束进程。建议 live 会话已有子 Agent 时
      跳过发现，并在所有退出路径上关闭 transport。
- [ ] **OpenCode：替换 transport 后后台子 Agent 仍显示为运行中。**
      `opencode/session.rs` 的 `prepare()` 在权限或系统提示变化时关闭旧 ACP 进程，后台子 Agent
      随之结束，但没有调用 `children.stopped()`。建议在关闭旧进程后标记已观测的子 Agent 为取消。
- [ ] **OpenCode / Codex：未知的子事件类型或状态会让父会话失败。**
      `opencode/subagents/live.rs` 对未知 frame 类型或状态返回错误，`codex/subagents.rs` 对未知
      activity 类型同样如此；与 5b072810 修复的 `completed` 属于同一类问题。建议忽略未知值并记录
      调试日志。
- [ ] **OpenCode（待确认）：恢复后的子会话通知可能使回合失败。**
      `reload` 在同一连接上加载子会话；回合中来自这些子会话的 `session/update` 只接受命令或
      配置更新，`usage_update` 等会使回合失败，子会话的权限请求在 `capture` 与 `poll_idle`
      中也会被拒绝。需先用真实 OpenCode 确认加载后是否会发送这些通知。

## 资源共享与访客会话（ADR-208）

具备 `operate` 权限的访客能以 Host 用户身份执行命令，因此下列问题属于 RPC 范围与撤销清理的
缺口，而非绕过操作系统边界。工作区与项目范围的 ID 绑定、路径校验和终端所有权检查已复核通过。

- [ ] **访客终端可能逃出追踪，撤销时不会被关闭。**
      `crates/api/src/guest/policy/private.rs` 在 `terminal.create.request` 返回后才调用
      `record_terminal`。单连接模式下访客发出创建请求后立即断开，或撤销恰好发生在创建过程中，
      `process_message` future 会被丢弃；终端服务的阻塞任务仍会启动进程，但记录与「撤销时
      已启动」的回退清理都不会执行。建议在终端服务创建时直接打上 `(shareId, userId)` 标记并按
      标记撤销（与 Agent 标签一致），至少把「创建并记录」放进不受连接取消影响的独立任务。
- [ ] **较早的运行中终端会被挤出所有权列表。**
      `crates/api/src/shares/guests.rs` 的 `record_terminal` 在达到 `MAX_TERMINALS = 128` 时
      `pop_front`，且已结束的终端从不移除。访客先开一个长期终端，再反复创建并关闭 128 个，
      第一个终端就不再受撤销清理（访客也失去其输入权限）。建议在终端结束时移除 ID，按存活状态
      清理而非淘汰最旧项；Host 最多 32 个存活终端，列表仍有界。
- [ ] **撤销只取消前 200 个访客 Agent。**
      `crates/api/src/shares/cleanup.rs` 的 `cancel_agents` 调用 `agent.list.request` 时未传
      `page`，默认上限 200，也不跟随 `pageInfo.nextCursor`。建议按 `guest_grants` 的方式分页。
- [ ] **撤销期间创建的 Agent 首个回合不会被取消。**
      已排入 provider 队列、带 `initialPrompt` 的 `agent.create` 不会因连接断开而停止；若它在
      `cancel_agents` 列出 Agent 之后才注册，首个回合照常运行。建议在以访客标签创建 Agent 后
      复核 `Book::grants`，授权已撤销则取消该回合。
- [ ] **节点访客创建的 Bot 与定时任务在撤销后仍会运行。**
      `guest/policy/node.rs` 允许节点访客调用 `bot.*` 与 `schedule.*`（例如把 Bot Channel 指向
      访客自己的聊天账号，或创建启动无标签 Agent 的定时任务）；撤销只处理连接、访客终端和带
      访客标签的 Agent。建议记录创建者并在撤销时删除或暂停，或在 ADR-208 中明确说明该边界。
- [ ] **项目访客可永久删除所有者的其他分享设置。**
      `workspace.archive` 属于 `WorkspaceManage`。项目访客归档项目内另有独立工作区分享的工作区时，
      `connection/workspace_archive.rs` 经 `shares.rs` 的 `forget` 从 `shares.json` 删除该设置并
      断开其他访客，取消归档也无法恢复；分享管理本应仅限所有者。建议访客触发的归档改为暂停这些
      设置，或拒绝归档自带分享的工作区。
- [ ] **路径校验使用未裁剪的 `cwd`（潜在问题）。**
      访客策略检查原始 `cwd`（`guest.rs`、`policy.rs`），而 `fs.explorer` 与下载令牌处理
      （`crates/filesystem/src/files/rpc.rs`）先 `trim`。工作区同时存在名为 `link `（尾随空格）
      的目录和指向 `/` 的符号链接 `link` 时，`cwd="/ws/link "` 通过检查，实际读取 `/`；
      两者都可经仓库内容带入。引入只读分享后即成为越界读取。建议拒绝未裁剪的 `cwd`，或检查
      裁剪后的值。
- [ ] **两处 RPC 范围缺口。**
      `checkout.pr.timeline` 与 `checkout.*.get_check_details`（`forge/rpc.rs`）接受任意
      `repoOwner`/`repoName`，访客可读取所有者 `gh` 登录可见的任意仓库 PR 时间线与检查详情；
      策略只校验 `cwd`。项目访客被禁止 `project.config.write`，却可通过 `fs.file.write` 直接写
      `<项目根>/ait.json`。建议将仓库参数绑定到 `cwd` 的远端，并禁止访客写项目配置文件。
- [ ] **（待确认）被拒访客可能在下一次结果中被重新放行。**
      `shares.rs` 的 `apply_outcome` 用每次新结果替换 `book.denied`，被拒授权又已从发布的
      `visitors` 中移除，下一次结果可能清空拒绝记录。仅当中心仍会为被拒访客路由会话时才有影响，
      需确认中心行为。

## Client

- [ ] **计划列表只查询共享主机组的代表成员（待确认访客权限）。**
      `apps/mobile/src/hooks/use-schedules.ts` 使用 `groupHosts(hosts).map(h => h.serverId)`，
      而历史与计划页过滤期望组内全部成员。若访客授权可以列出计划，只能经非代表授权访问的计划会
      缺失，代表授权离线时也会一直显示连接中。
- [ ] **原生端按压高亮与状态徽标描边颜色不一致。**
      `use-sidebar-row-press.ts` 在原生端始终返回 `isPressed: false`，但
      `getSidebarRowBackdrop({ isPressed })` 仍决定项目图标状态徽标的镂空颜色；`PressHighlight`
      绘制 `surface2` 时徽标周围会出现 `surfaceSidebar` 色的光圈。需要从 `PressHighlight`
      暴露按压状态（共享值或回调）。
- [ ] **重连提示不会在连接恢复时关闭。**
      `apps/mobile/src/runtime/use-online-service-connection-notice.ts` 的加载态提示使用固定时长
      （重连使用默认 2.2 秒，不可用为 3.2 秒）；连接很快恢复时仍显示「正在重连」。建议在状态回到
      `online` 时主动关闭当前提示。
