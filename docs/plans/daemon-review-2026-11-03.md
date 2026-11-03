# 2026-11-03 daemon 审查遗留事项

来源：2026-11-03 对 `bins/daemon/src`（源码提交 `850e0075`）的审查，必要时追踪到 `host-link`、
`persistence` 与 `provider` 的调用路径。审查未修改代码，也未运行测试；以下结论均基于源码阅读
与调用链追踪。每项写明触发条件、影响和建议做法，完成后勾选，并附上修复 PR 和回归测试。

与 [2026-11-02 仓库审查遗留事项](repository-review-2026-11-02.md) 不重复；`device/http.rs`
协议错误终止发布、MCP socket 权限窗口等问题以那份清单为准。

## 优先处理

- [ ] **Hub 已连接时，每条时间线记录都会整体重写并 fsync 发布日志。**
      每条已提交的时间线记录都会发布 `agent_stream`（`crates/provider/src/storage/timeline.rs`），
      `host/hub/events.rs` 将其投影为 `agent.timeline.update`。`host/hub/outbox.rs` 的
      `enqueue` → `commit` 每次都克隆整个文档（上限 24 MiB）、序列化三次、重写
      `hub/publications.json` 并 fsync 两次（`crates/persistence/src/single.rs`）。活跃 Agent 的
      每次工具调用或消息都触发一次完整写盘，离线积压到 1024 条时写放大接近 O(n²)。
      建议按 Agent 合并时间线标记（只需保留最新 `seq`，客户端会向 Host 拉取历史），
      或改为追加式日志。
- [ ] **离线期间发布日志写满后，spool 循环停滞。**
      `LinkState::supports` 在重连期间仍为真，事件持续入队直到 `MAX_EVENTS = 1024`。之后
      `host/hub.rs` 的 `spool` 每 2 秒重试同一个必然失败的「outbox full」，期间不处理链路状态变化，
      也不执行 reconcile；256 条的 ingress 队列随即写满，`Ingress::publish` 对每个被丢弃的事件
      记录一条 `error!`，诊断采集器因此每 30 秒写一份事故文件。链路恢复后能自愈，但低价值的
      时间线标记会挤占注意力事件。建议与上一项一起处理：合并标记、为不同事件类型分配配额，
      并对「已满」限流记录日志，而不是逐条报错。
- [ ] **目录条目超过 1000 时，发布永久失败并每 2 秒全量重读。**
      `host/hub/sources.rs` 的 `snapshots` 在任一类别超过 1000 项时 `ensure!` 失败（Agent 计入
      所有未归档项）。`reconcile` 随后每 2 秒重试、读取全部注册表并记录告警；注意力事件的恢复
      位于同一个阻塞调用中、排在快照之后，因此也被阻断。建议对超出预算的类别降级处理（截断或跳过
      该类别并记录一次告警），并把注意力恢复与目录快照拆开，互不阻塞。
- [ ] **无关的 `GOOGLE_APPLICATION_CREDENTIALS` 会让 daemon 无法启动。**
      `host/push/auth.rs` 的 `Google::from_environment` 回退读取 `GOOGLE_APPLICATION_CREDENTIALS`；
      文件不存在，或不是服务账号密钥（如 `authorized_user`、`external_account`）时返回错误，
      经 `host.rs` 的 `compose_push(...)?` 导致整个 daemon 退出。很多开发者会为其他工具设置该变量。
      建议只有显式设置的 `AIT_FCM_SERVICE_ACCOUNT_FILE` 出错时才致命；回退变量无效时记录告警并
      禁用 FCM。
- [ ] **定时任务启动恢复可能让 daemon 永久无法启动。**
      `host/schedule.rs` 的 `compose` 对 `futures_recover` 使用 `?`，而对已不存在的 Agent 执行
      `agent.archive.request` 会返回 `NotFound`（`crates/provider/src/service/agent_runtime/archive.rs`）；
      I/O 错误或 observed Agent 同样会中止启动。该运行仍保持 `Running`，之后每次启动都以同样方式
      失败，只能手工编辑 `schedules.json`。建议把 `NotFound` 视为已恢复，其他错误记录日志后继续
      启动，由调度器把该运行结算为失败。

## 中等

- [ ] **本地时钟超前时，`daemon login` 可能无限循环。**
      `device.rs` 的 `restore_login` 循环直到 `access_expires_at > now`；`device/http.rs` 的
      `decode_tokens` 只拒绝过期时间过远的 access token，不拒绝已过期的。本地时钟超前约 15 分钟
      以上时，每轮都会轮换 refresh token 并做两次 fsync 保存，没有退避也没有上限。
      `crates/host-link/src/controller/authority.rs` 的刷新路径有相同模式。建议限制重试次数，
      或把已过期的 access token 视为协议错误并提示检查系统时钟。
- [ ] **两个可能是暂时性的错误码被当作永久错误，会冻结目录发布。**
      `host/hub.rs` 的 `permanent()` 包含 `unauthorized` 与 `forbidden`，它们在令牌轮换期间可能
      只是暂时状态。被隔离的目录快照只有在内容变化后才会恢复：重连时的强制重发不会清除
      `blocked`（`host/hub/outbox.rs` 的 `snapshot`）。被隔离的事件保留 24 小时并占用 1024 条
      配额，加重上面的写满问题。建议这两个码按可重试处理，或在新链路上清除 `blocked`。
- [ ] **中心不支持的持久化事件会阻塞全部目录发布。**
      `Outbox::next` 总是先返回事件、后返回目录快照；若队首事件的方法不再被中心声明，
      `attempt` 返回 `Ok(false)`，目录快照在其后最多等待 24 小时。建议 `next` 只选择当前链路
      支持的方法。

## 较低

- [ ] **关闭时的错误处理会掩盖真实错误。**
      `host.rs` 的 `serve` 中，发布管理器超时会经 `?` 提前返回，覆盖 `result` 中真正的服务错误，
      并跳过 `publication.shutdown()`。建议先完成所有清理，再按优先级返回错误。
- [ ] **`daemon status` 在崩溃后显示过期状态。**
      `device.rs` 的 `status` 直接展示最后写入的阶段（如 `running`），不检查记录的 PID 是否存活。
      `StatusRelay::report` 在异步运行时上做同步文件 I/O，崩溃残留的 `status-*.tmp` 也从不清理。
      建议读取时校验 PID 存活，并在启动时清理临时文件。
- [ ] **启动失败不会被诊断采集保存。**
      `main.rs` 记录致命错误后立即退出，诊断工作线程来不及保存事故文件，而启动失败恰恰最需要
      留证。建议退出前同步刷写一次，或直接写入一份最小事故报告。
- [ ] **部分工作发生在实例锁之前。**
      TCP 监听、本地令牌创建和诊断采集（会清理共享的事故目录）都早于 `InstanceLease::acquire`
      （`host.rs`、`main.rs`）。第二个 daemon 使用同一数据目录和端口时，报错为
      「bind server listener」而不是「数据目录已被占用」。建议先获取实例锁再绑定端口与启动采集。
- [ ] **`--name` 被静默忽略。**
      存在未完成授权且未传 `--force` 时，`daemon login --name X` 不生效（`device.rs`）。
      建议报错提示，或在该情况下同样应用显式名称。
- [ ] **本地令牌权限检查难以阅读，错误提示不明确。**
      `config/local_token.rs` 的 `mode().trailing_zeros() >= 6` 应写为 `mode & 0o077 == 0`；
      手工编辑后带尾随换行的令牌文件会被拒绝，且错误信息没有说明原因。
- [ ] **定时任务恢复为每个 Agent 新建 Tokio 运行时。**
      `host/schedule.rs` 的 `futures_recover` 可改为整批恢复共用一个运行时。
- [ ] **等待循环高频轮询且没有整体超时。**
      `host/voice.rs` 与 `host/schedule.rs` 的等待循环每 25 ms 轮询一次；建议加入退避或整体超时。
