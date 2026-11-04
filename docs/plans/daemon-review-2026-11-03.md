# 2026-11-03 daemon 审查遗留事项

来源：2026-11-03 对 `bins/daemon/src`（源码提交 `850e0075`）的审查，必要时追踪到 `host-link`、
`persistence` 与 `provider` 的调用路径。每项写明触发条件、影响和建议做法，完成后勾选，并附上
修复与回归测试。

与 [2026-11-02 仓库审查遗留事项](repository-review-2026-11-02.md) 不重复；`device/http.rs`
协议错误终止发布、MCP socket 权限窗口等问题以那份清单为准。

2026-11-04：分支 `fix/daemon-review-2026-11-03` 修复了下列已勾选条目；Hub 发布的行为变化记录在
[ADR-181 修订](../decisions/daemon/adr-181-hub-host-publication.md)。剩余条目见文末「仍未完成」。

## 优先处理

- [x] **Hub 已连接时，每条时间线记录都会整体重写并 fsync 发布日志。**
      每条已提交的时间线记录都会发布 `agent_stream`，投影为 `agent.timeline.update` 后经
      `Outbox::enqueue` → `commit` 整体重写 `hub/publications.json` 并 fsync 两次。
      修复：标记改由 `host/hub/markers.rs` 在内存中按 Agent 合并（最多一个 reset 与一个 update），
      在 journal 事件和目录快照之后轮流投递，不再入盘。回归：
      `timeline_markers_skip_the_journal_coalesce_and_follow_journal_events`、
      `failed_marker_delivery_is_retried_and_permanent_rejection_drops_it` 与 `markers` 单元测试。
- [x] **离线期间发布日志写满后，spool 循环停滞。**
      写满后 `spool` 每 2 秒重试必然失败的「outbox full」，不处理链路变化与 reconcile，
      `Ingress::publish` 对每个被丢弃的事件记录 `error!`。
      修复：写满返回类型化的 `OutboxFull`，事件被丢弃并触发重建；补扫 attention 写满时延后；
      ingress 溢出每次只记录一条错误。回归：
      `full_journal_drops_the_event_and_resynchronizes_instead_of_retrying`、
      `a_full_journal_reports_a_typed_error_and_keeps_existing_entries`。
- [x] **目录条目超过 1000 时，发布永久失败并每 2 秒全量重读。**
      修复：`host/hub/sources.rs` 的 `fit_budget` 在预算内原样发布，超出时跳过过大条目并按
      `updatedAt` 保留最近更新的条目。回归：`directories_within_budget_are_published_unchanged`、
      `oversized_directories_keep_the_most_recently_updated_items`。
- [x] **无关的 `GOOGLE_APPLICATION_CREDENTIALS` 会让 daemon 无法启动。**
      修复：显式 `AIT_FCM_SERVICE_ACCOUNT_FILE` 无效仍导致启动失败；回退变量不可读或不是服务账户
      时只记录告警并禁用 FCM，[移动端通知](../operations/mobile-notifications.md)已同步。回归：
      `unusable_ambient_google_credentials_disable_fcm_without_failing_startup`。
- [x] **定时任务启动恢复可能让 daemon 永久无法启动。**
      修复：`recover_interrupted` 把 `AgentNotFound` 视为已恢复，其他归档失败记录告警后继续启动，
      由调度器把该运行结算为失败；整批恢复共用一个运行时。回归（进程测试）：
      `restart_recovery_tolerates_an_interrupted_run_whose_agent_is_gone`，在旧代码上以
      「recover scheduled agent」启动失败。

## 中等

- [x] **本地时钟超前时，`daemon login` 可能无限循环。**
      修复：`restore_login` 最多轮换 `RESTORE_ATTEMPTS = 2` 次（第二次用于取得重放回执的后继），
      仍过期时提示检查系统时钟；`host-link` 的 `Lease::step` 在连续两次得到过期 token 时返回
      `Unavailable`，按 1–30 秒退避并报告 `retrying`，不再每秒轮换。回归：
      `login_stops_rotating_when_every_access_token_is_already_expired`、
      `replayed_expired_receipt_rotates_once_more_and_succeeds`、
      `clock_skew_backs_off_instead_of_rotating_every_second`（修复前 60 秒内轮换 61 次）。
- [x] **两个可能是暂时性的错误码被当作永久错误，会冻结目录发布。**
      修复：隔离仍只在同一链路内生效；新逻辑链路的 reconcile 调用 `Outbox::unblock`，事件与目录快照
      各重试一次。回归：`a_fresh_link_retries_quarantined_events_and_snapshots_once`、
      `a_fresh_link_retries_events_quarantined_by_the_previous_link`。
- [x] **中心不支持的持久化事件会阻塞全部目录发布。**
      修复：`Outbox::next` 只选择当前链路声明的方法。回归：
      `unsupported_events_do_not_hold_back_directory_snapshots`。

## 较低

- [x] **关闭时的错误处理会掩盖真实错误。**
      修复：`serve` 始终执行 `publication.shutdown()`，先返回服务错误，再返回发布管理器错误。
      管理器超时难以稳定构造，未新增专门测试，由现有生命周期测试覆盖正常路径。
- [x] **`daemon status` 在崩溃后显示过期状态。**
      修复：新增 `runtime_process_alive`（基于 `kill(pid, 0)`，PID 可能被复用），
      [无人值守 daemon](../operations/headless-daemon.md)已同步；启动时清理残留的
      `status-*.tmp`。回归：`status_probe_distinguishes_live_and_exited_processes`、
      `stale_status_snapshots_are_removed_without_touching_other_files`。
- [x] **启动失败不会被诊断采集保存。**
      修复：`main` 记录致命错误后调用 `Diagnostics::flush`（最多等待 5 秒），绕过 30 秒采集间隔
      保存尚未保存的事件。回归：`flush_saves_a_fatal_event_that_the_capture_interval_would_defer`。
- [x] **第二个 daemon 在同一端口上报告端口错误而不是数据目录冲突。**
      修复：保留「先绑定端口」的顺序（`binding_failure_has_no_state_side_effects` 要求端口失败不产生
      任何状态）；绑定失败时用 `InstanceLease::held` 只读探测既有锁，被占用则报告数据目录冲突。
      回归：`held_probe_reports_a_live_lease_without_creating_state` 与进程测试
      `signal_shutdown_releases_process_lock_and_preserves_identity`。
- [x] **`--name` 被静默忽略。**
      修复：存在未完成的 Web/enrollment 授权且未传 `--force` 时直接报错；仅有待恢复的 refresh 时
      应用显式名称。回归：`explicit_name_cannot_rename_an_unfinished_approval_without_force`。
- [x] **本地令牌文件带尾随换行时被拒绝，错误提示不明确。**
      修复：容忍单个 `\n` 或 `\r\n`，超长内容仍被拒绝，错误信息说明期望格式。审查中建议的
      `mode & 0o077 == 0` 写法会触发本仓库启用的 `clippy::verbose_bit_mask`，因此保留
      `trailing_zeros() >= 6` 并加注释。回归：
      `hand_edited_credentials_tolerate_a_line_ending_but_not_group_access`。
- [x] **定时任务恢复为每个 Agent 新建 Tokio 运行时。**
      修复：仅在存在中断运行时创建一个运行时，整批共用。
- [x] **等待循环高频轮询。**
      修复：定时任务每次完成等待最长 1 秒且超时后不再额外休眠；语音状态轮询从 25 ms 退避到
      100 ms。

## 2026-11-04 追加修复

- [x] **本地令牌与诊断采集仍早于实例锁。**
      `Config::load` 创建本地令牌、`main` 启动诊断采集（会清理共享的事故目录）都发生在
      `InstanceLease::acquire` 之前。两者均为幂等或有界操作，影响低；如需彻底解决，应把实例锁
      提前到 `main` 并调整「端口失败不产生状态」的约束。
      修复：先验证配置、预绑定端口，再获取进程级实例锁，之后才初始化令牌和诊断；运行时重启
      复用锁与监听端口并生成新的实例 ID，仍保留端口失败不产生状态的约束。已修订
      [ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md)。回归：
      `startup_guard_runs_before_local_credentials_are_created`、
      `restarting_runtime_keeps_the_directory_locked_until_all_generations_drop` 及宿主生命周期测试。
- [x] **`StatusRelay::report` 在异步运行时上做同步文件 I/O。**
      修复：状态文件交给阻塞线程写入，保留实例锁直到写完，串行写入并丢弃过期快照。
      回归：`cancelled_status_writes_keep_the_lease_and_cannot_overwrite_newer_reports`。

## 仍未完成

- [ ] **等待循环没有整体超时。**
      `host/voice.rs` 与 `host/schedule.rs` 的等待仍可无限期进行，依赖取消令牌结束。定时运行可能
      合理地持续很久，加入上限前需要先确定产品语义。
      2026-11-04：保留已有取消令牌机制；还需明确语音与定时任务各自的上限、超时是否取消
      Agent 回合，以及用户可配置方式，不能以任意固定时限中断合法的长任务。
