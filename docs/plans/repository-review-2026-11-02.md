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
