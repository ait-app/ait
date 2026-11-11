# 架构决策分类索引

文档按能力归属分类，每类按编号从新到旧排列。ADR 保存决策背景，不逐篇充当当前使用手册。
当前关系先看[架构概览](../architecture/README.md)及其中按能力的现行规格，操作步骤见[文档入口](../README.md)。
旧决策按状态、后续修订与取代关系阅读；保留原路径以便代码和历史报告引用。
ADR-001–024 已随旧架构删除，可从 Git 历史查阅；旧 ADR 中对这些编号的引用指向历史版本。
ADR-074、082、083、084、087、122、132、140、156、175、177、180、211、215、219、223 各有两份，引用时以路径区分；
`npm run check:docs` 拒绝新的重号。

本页由 `node scripts/adr.mjs index` 根据各 ADR 的 front matter 生成，只手工维护本段说明；
条目后的“修订”“取代”来自其他 ADR 的 `amends` 与 `supersedes`。字段与校验见[文档维护规范](../policy/documentation.md#adr-元数据)。
`node scripts/adr.mjs governs <路径>` 列出约束某个文件或目录的现行 ADR，加 `--all` 包含已取代或撤销的决策。

## Bot

- [ADR-233：中心托管的 Workspace Bot Webhook Channel](bot/adr-233-webhook-channel-center-endpoints.md)
- [ADR-227：Bot 按消息身份保留 Provider 的多条回复](bot/adr-227-bot-multi-message-output.md)
- [ADR-226：通过 Photon Beta 2.0 接入 iMessage Channel](bot/adr-226-photon-imessage-channel.md)
- [ADR-225：微信与 QQ Channel 的扫码接入](bot/adr-225-weixin-and-qq-channels.md)
- [ADR-224：飞书扫码接入与 daemon 保存的 Channel 凭据](bot/adr-224-feishu-scan-onboarding-and-stored-secrets.md)
- [ADR-222：每个 Provider 单一 Agent 模式，Bot 不创建 Workspace](bot/adr-222-single-agent-mode-no-bot-workspaces.md)
- [ADR-221：以子 Agent 形式只读查看 Bot 内部会话](bot/adr-221-read-only-bot-session-inspection.md)
- [ADR-215：Bot 权限与作用域 MCP 凭据](bot/adr-215-bot-permissions-and-scoped-mcp.md)（经 [ADR-222](bot/adr-222-single-agent-mode-no-bot-workspaces.md) 修订）
- [ADR-213：单 Workspace 管理者 Bot 与 v2 接口](bot/adr-213-single-workspace-manager-bot.md)（经 [ADR-215](bot/adr-215-bot-permissions-and-scoped-mcp.md)、[ADR-221](bot/adr-221-read-only-bot-session-inspection.md)、[ADR-227](bot/adr-227-bot-multi-message-output.md)、[ADR-233](bot/adr-233-webhook-channel-center-endpoints.md) 修订）
- [ADR-145：Bot 执行期间自动拒绝权限请求](bot/adr-145-bot-permission-denial.md)（经 [ADR-213](bot/adr-213-single-workspace-manager-bot.md)、[ADR-215](bot/adr-215-bot-permissions-and-scoped-mcp.md) 修订）
- [ADR-139：Workspace Bot 与独占 Channel](bot/adr-139-workspace-bots-and-owned-channels.md)（经 [ADR-145](bot/adr-145-bot-permission-denial.md)、[ADR-213](bot/adr-213-single-workspace-manager-bot.md)、[ADR-224](bot/adr-224-feishu-scan-onboarding-and-stored-secrets.md)、[ADR-226](bot/adr-226-photon-imessage-channel.md)、[ADR-233](bot/adr-233-webhook-channel-center-endpoints.md) 修订）

## Daemon 与协议

- [ADR-208：群组内资源共享与访客会话](daemon/adr-208-workspace-share-guests.md)
- [ADR-203：MCP 的可选 Unix socket 监听](daemon/adr-203-mcp-unix-socket.md)
- [ADR-201：daemon 统一拥有节点发布与机器授权](daemon/adr-201-daemon-owned-publication.md)
- [ADR-200：Headless 独立维护 Hub、Relay 与节点租约](daemon/adr-200-headless-independent-transports.md)（经 [ADR-201](daemon/adr-201-daemon-owned-publication.md) 修订）
- [ADR-181：Host 向 Hub 发布事件与目录摘要](daemon/adr-181-hub-host-publication.md)
- [ADR-180：消息 Hub 服务端点（ait-hub-v1）](daemon/adr-180-hub-serve-endpoint.md)（经 [ADR-181](daemon/adr-181-hub-host-publication.md)、[ADR-200](daemon/adr-200-headless-independent-transports.md)、[ADR-201](daemon/adr-201-daemon-owned-publication.md)、[ADR-204](clients/adr-204-hub-only-online-business-transport.md)、[ADR-218](clients/adr-218-hub-feature-negotiation.md) 修订）
- [ADR-155：可协商的持久 Relay 数据隧道](daemon/adr-155-persistent-relay-data-tunnel.md)（经 [ADR-204](clients/adr-204-hub-only-online-business-transport.md) 修订）
- [ADR-148：daemon 的无状态 MCP 编排接口](daemon/adr-148-daemon-mcp.md)（经 [ADR-202](providers/adr-202-agent-mcp-injection.md)、[ADR-215](bot/adr-215-bot-permissions-and-scoped-mcp.md) 修订）
- [ADR-142：PTY 拥有基础终端设备属性回复](daemon/adr-142-terminal-primary-device-attributes.md)
- [ADR-140：Headless daemon 自动保存本地 API 凭据](daemon/adr-140-headless-local-api-token.md)（经 [ADR-201](daemon/adr-201-daemon-owned-publication.md) 修订）
- [ADR-134：已接纳的普通 RPC 等待共享任务额度](daemon/adr-134-admitted-rpc-job-waiting.md)
- [ADR-127：移除 daemon 配对 offer RPC](daemon/adr-127-remove-daemon-pairing-offer.md)
- [ADR-120：单连接启动请求的有界排队](daemon/adr-120-single-connection-startup-admission.md)
- [ADR-118：摘要接口归 model，配置文件适配归 persistence](daemon/adr-118-summary-contracts-and-persistence-configuration.md)
- [ADR-116：库 crate 只公开被其他 crate 使用的项](daemon/adr-116-crate-visibility.md)
- [ADR-112：file 瘦身为 persistence，宿主专用文件归 daemon](daemon/adr-112-persistence-crate.md)
- [ADR-111：纯业务数据归 domain，连接协议并入 model::server](daemon/adr-111-domain-values-and-server-protocol.md)
- [ADR-109：本地故障证据与 Harness 日志采集](daemon/adr-109-local-incident-evidence.md)（经 [ADR-112](daemon/adr-112-persistence-crate.md) 修订）
- [ADR-108：Relay RPC 与基础连接方法归所属 crate](daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)
- [ADR-107：共享组件直接从所属 crate 导入](daemon/adr-107-direct-imports-from-owning-crates.md)（经 [ADR-112](daemon/adr-112-persistence-crate.md) 修订）
- [ADR-106：具体文件持久化归 file，model 仅声明契约](daemon/adr-106-concrete-file-persistence.md)（经 [ADR-107](daemon/adr-107-direct-imports-from-owning-crates.md)、[ADR-111](daemon/adr-111-domain-values-and-server-protocol.md)、[ADR-112](daemon/adr-112-persistence-crate.md)、[ADR-210](providers/adr-210-committed-runtime-read-snapshots.md) 修订）
- [ADR-105：File 工具、通用 Registry 与启动配置独立成 crate](daemon/adr-105-file-tools-and-startup-config.md)（经 [ADR-106](daemon/adr-106-concrete-file-persistence.md)、[ADR-112](daemon/adr-112-persistence-crate.md) 修订）
- [ADR-103：Terminal 仅依赖 model 的共享契约](daemon/adr-103-terminal-model-dependency.md)（经 [ADR-111](daemon/adr-111-domain-values-and-server-protocol.md) 修订）
- [ADR-100：功能 crate 作为完整服务安装](daemon/adr-100-crate-level-service-installation.md)（经 [ADR-108](daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)、[ADR-129](providers/adr-129-provider-owned-skills.md)、[ADR-130](workspace/adr-130-workspace-methods-owned-by-metadata.md) 修订）
- [ADR-095：组件自行声明方法元数据](daemon/adr-095-component-method-declarations.md)（经 [ADR-100](daemon/adr-100-crate-level-service-installation.md) 修订）
- [ADR-093：以可消费 Context 逐级处理请求](daemon/adr-093-consumable-request-context.md)
- [ADR-072：Workspace 名称与当前文档边界](daemon/adr-072-workspace-names-and-documentation.md)（经 [ADR-111](daemon/adr-111-domain-values-and-server-protocol.md)、[ADR-112](daemon/adr-112-persistence-crate.md) 修订）
- [ADR-064：默认离线语音与模型准备](daemon/adr-064-offline-speech.md)（经 [ADR-191](providers/adr-191-codex-native-realtime-voice.md) 修订）
- [ADR-047：移除 Plugin 接口并独立实现 Schedule 与 Browser](daemon/adr-047-daemon-schedule-browser.md)（经 [ADR-138](workspace/adr-138-workspace-owned-schedules.md) 修订）
- [ADR-045：移除 Hub、Chat 与 Loop 接口](daemon/adr-045-remove-hub-chat-loop.md)（经 [ADR-233](bot/adr-233-webhook-channel-center-endpoints.md) 修订）
- [ADR-042：连接级语音、听写与双后端](daemon/adr-042-daemon-voice.md)（经 [ADR-191](providers/adr-191-codex-native-realtime-voice.md) 修订）
- [ADR-038：protocol 仅依赖公共 model](daemon/adr-038-daemon-protocol-dependencies.md)（已由 [ADR-111](daemon/adr-111-domain-values-and-server-protocol.md) 取代）
- [ADR-037：公共 Context 与具体 crate 分发](daemon/adr-037-daemon-model-context.md)（经 [ADR-038](daemon/adr-038-daemon-protocol-dependencies.md)、[ADR-093](daemon/adr-093-consumable-request-context.md) 修订）
- [ADR-036：请求先进入所属 crate 再分发到能力组](daemon/adr-036-daemon-crate-dispatch.md)（经 [ADR-037](daemon/adr-037-daemon-model-context.md)、[ADR-093](daemon/adr-093-consumable-request-context.md) 修订）
- [ADR-035：能力分组与安装规则归所属 server crate](daemon/adr-035-daemon-capability-groups.md)（经 [ADR-036](daemon/adr-036-daemon-crate-dispatch.md)、[ADR-100](daemon/adr-100-crate-level-service-installation.md) 修订）
- [ADR-033：独立 terminal 与完整 Terminal 方法分组](daemon/adr-033-daemon-terminal.md)（经 [ADR-035](daemon/adr-035-daemon-capability-groups.md)、[ADR-037](daemon/adr-037-daemon-model-context.md) 修订）
- [ADR-026：规范化 Paseo WebSocket 接口并按能力分期接入](daemon/adr-026-canonical-paseo-websocket-surface.md)（经 [ADR-045](daemon/adr-045-remove-hub-chat-loop.md)、[ADR-095](daemon/adr-095-component-method-declarations.md)、[ADR-127](daemon/adr-127-remove-daemon-pairing-offer.md) 修订）

## 工作区、文件与 Git

- [ADR-230：归档 Worktree 时运行 teardown](workspace/adr-230-worktree-teardown.md)
- [ADR-229：Workspace 脚本运行记录、输出与参数](workspace/adr-229-workspace-script-runs.md)
- [ADR-223：Linux 沙箱化 Worktree](workspace/adr-223-linux-worktree-sandbox.md)
- [ADR-217：事件驱动的 Workspace 运行时事实与低能耗 Git 读取](workspace/adr-217-event-driven-workspace-runtime.md)
- [ADR-215：沙箱化 Worktree 隔离级别](workspace/adr-215-sandboxed-worktree-isolation.md)（经 [ADR-223](workspace/adr-223-linux-worktree-sandbox.md) 修订）
- [ADR-168：Setup 全局摘要与面板日志订阅分离](workspace/adr-168-scoped-setup-observation.md)
- [ADR-146：Workspace 重置清除变更请求关联](workspace/adr-146-reset-workspace-change-request-association.md)
- [ADR-138：定时任务固定归属于 Workspace](workspace/adr-138-workspace-owned-schedules.md)
- [ADR-136：拍平单功能区层并收口重复助手](workspace/adr-136-single-area-layer-flattening.md)
- [ADR-132：Checkout 刷新独立准入与有界输出背压](workspace/adr-132-checkout-refresh-admission-and-backpressure.md)
- [ADR-131：metadata 按能力组组织模块](workspace/adr-131-metadata-capability-groups.md)（经 [ADR-136](workspace/adr-136-single-area-layer-flattening.md) 修订）
- [ADR-130：Workspace 与 Project 方法归 metadata，filesystem 只保留本地适配器](workspace/adr-130-workspace-methods-owned-by-metadata.md)（经 [ADR-131](workspace/adr-131-metadata-capability-groups.md) 修订）
- [ADR-126：按身份读取目标 Workspace，完整目录后台同步](workspace/adr-126-workspace-target-read.md)
- [ADR-113：filesystem 按能力组组织模块](workspace/adr-113-filesystem-capability-groups.md)（经 [ADR-129](providers/adr-129-provider-owned-skills.md)、[ADR-130](workspace/adr-130-workspace-methods-owned-by-metadata.md)、[ADR-136](workspace/adr-136-single-area-layer-flattening.md) 修订）
- [ADR-104：Filesystem 仅通过 model 契约协作](workspace/adr-104-filesystem-model-collaboration.md)（经 [ADR-111](daemon/adr-111-domain-values-and-server-protocol.md)、[ADR-113](workspace/adr-113-filesystem-capability-groups.md)、[ADR-130](workspace/adr-130-workspace-methods-owned-by-metadata.md)、[ADR-139](bot/adr-139-workspace-bots-and-owned-channels.md) 修订）
- [ADR-092：工作区重置同步 origin 同名分支](workspace/adr-092-reset-same-named-remote-branch.md)
- [ADR-084：工作区主按钮按提交与 PR 生命周期推进](workspace/adr-084-workspace-primary-git-action.md)
- [ADR-083：工作区重置到 origin 的最新默认分支](workspace/adr-083-reset-workspace-to-origin-default.md)（经 [ADR-092](workspace/adr-092-reset-same-named-remote-branch.md) 修订）
- [ADR-082：Workspace 可空字段使用单级 Option](workspace/adr-082-canonical-nullable-workspace-fields.md)
- [ADR-081：由状态变更唤醒 Workspace 与 Agent 目录订阅](workspace/adr-081-directory-change-push.md)（经 [ADR-126](workspace/adr-126-workspace-target-read.md)、[ADR-156](clients/adr-156-setup-and-activity-events.md)、[ADR-210](providers/adr-210-committed-runtime-read-snapshots.md) 修订）
- [ADR-071：在 Rust checkout adapter 生成 Diff 语法 token](workspace/adr-071-checkout-diff-syntax-highlighting.md)
- [ADR-069：活跃工作区的后台 Git fetch](workspace/adr-069-background-git-fetch.md)（经 [ADR-083](workspace/adr-083-reset-workspace-to-origin-default.md) 修订）
- [ADR-068：Workspace 侧边栏运行时摘要](workspace/adr-068-workspace-runtime-summaries.md)（经 [ADR-217](workspace/adr-217-event-driven-workspace-runtime.md) 修订）
- [ADR-066：通过 glab 支持 GitLab Forge](workspace/adr-066-gitlab-forge.md)
- [ADR-065：Paseo 目录、时间线与会话 API 兼容行为](workspace/adr-065-paseo-directory-and-timeline-projections.md)（经 [ADR-082](workspace/adr-082-canonical-nullable-workspace-fields.md) 修订）
- [ADR-060：统一 Workspace 创建入口支持 Worktree](workspace/adr-060-workspace-create-worktree.md)
- [ADR-043：独立 server 的 Skills 选择与文件安装](workspace/adr-043-daemon-skills.md)（经 [ADR-056](clients/adr-056-paseo-coexistence.md)、[ADR-129](providers/adr-129-provider-owned-skills.md)、[ADR-199](providers/adr-199-bundled-skill-distribution.md) 修订）
- [ADR-030：纵向拆出 filesystem](workspace/adr-030-daemon-filesystem.md)（经 [ADR-031](providers/adr-031-daemon-provider.md)、[ADR-035](daemon/adr-035-daemon-capability-groups.md)、[ADR-037](daemon/adr-037-daemon-model-context.md)、[ADR-113](workspace/adr-113-filesystem-capability-groups.md) 修订）
- [ADR-029：统一 Paseo Project 并纵向拆出 metadata](workspace/adr-029-daemon-metadata.md)（经 [ADR-030](workspace/adr-030-daemon-filesystem.md)、[ADR-031](providers/adr-031-daemon-provider.md)、[ADR-034](providers/adr-034-agent-config-session-events.md)、[ADR-035](daemon/adr-035-daemon-capability-groups.md)、[ADR-037](daemon/adr-037-daemon-model-context.md) 修订）
- [ADR-028：GitHub 仓库发现与独立 Project 克隆注册](workspace/adr-028-github-project-provisioning.md)（经 [ADR-130](workspace/adr-130-workspace-methods-owned-by-metadata.md) 修订）
- [ADR-025：先移植 Paseo Project / Workspace 模型与 registry](workspace/adr-025-paseo-registry.md)（经 [ADR-029](workspace/adr-029-daemon-metadata.md) 修订）

## Provider、Agent 与会话

- [ADR-236：按 Host 记录的 Harness 用量账本](providers/adr-236-harness-usage-ledger.md)
- [ADR-232：OpenCode、Pi 与 DSH 原生 Host 的 Agent MCP 单会话交付](providers/adr-232-agent-mcp-session-delivery.md)
- [ADR-220：Pi 扩展通知与会话时间线](providers/adr-220-pi-extension-notifications.md)
- [ADR-219：Pi 原生 slash-command 发现](providers/adr-219-pi-slash-command-discovery.md)
- [ADR-219：共享套餐缓存与 Codex 查询进程](providers/adr-219-shared-account-usage-cache.md)
- [ADR-211：Pi 推理偏好在下一条 prompt 前应用](providers/adr-211-pi-next-turn-reasoning-controls.md)
- [ADR-210：Agent 目录共享已提交的不可变记录](providers/adr-210-committed-runtime-read-snapshots.md)
- [ADR-209：通过只读登记统一子 Agent 展示](providers/adr-209-observed-subagent-normalization.md)
- [ADR-206：OpenCode 的 ACP 子 Agent 观测与历史归属](providers/adr-206-opencode-acp-subagents.md)
- [ADR-205：Provider 执行归属与公共端口契约](providers/adr-205-provider-execution-contracts.md)
- [ADR-202：Agent MCP 运行时自动注入](providers/adr-202-agent-mcp-injection.md)（经 [ADR-215](bot/adr-215-bot-permissions-and-scoped-mcp.md)、[ADR-232](providers/adr-232-agent-mcp-session-delivery.md) 修订）
- [ADR-199：内置 Skill 随 daemon 启动分发](providers/adr-199-bundled-skill-distribution.md)
- [ADR-198：OMP 交互等待期间持续发布事件](providers/adr-198-omp-interaction-progress.md)
- [ADR-197：MiniMax Code 原生 ACP Provider](providers/adr-197-minimax-code-acp.md)
- [ADR-196：Provider 输出分类与每轮 Thinking 展示](providers/adr-196-provider-output-semantics.md)
- [ADR-195：OpenCode 独立取消信号与确认期限](providers/adr-195-opencode-independent-cancellation.md)
- [ADR-194：OMP 原生空闲状态核对与有界取消](providers/adr-194-omp-settlement-reconciliation.md)（经 [ADR-198](providers/adr-198-omp-interaction-progress.md) 修订）
- [ADR-193：Codex 原生策略、事件与恢复兼容](providers/adr-193-codex-native-protocol-compatibility.md)
- [ADR-192：Codex 历史按页投影后计算展示预算](providers/adr-192-codex-incremental-history-projection.md)
- [ADR-191：Codex Provider 拥有原生实时语音](providers/adr-191-codex-native-realtime-voice.md)（经 [ADR-192](providers/adr-192-codex-incremental-history-projection.md) 修订）
- [ADR-190：Ait ACP 桥接官方 OMP 与通用插件接口](providers/adr-190-omp-acp-settlement.md)（经 [ADR-194](providers/adr-194-omp-settlement-reconciliation.md)、[ADR-198](providers/adr-198-omp-interaction-progress.md) 修订）
- [ADR-189：Pi 不暴露 Host 模式](providers/adr-189-pi-without-host-mode.md)
- [ADR-185：各 Provider 保留原生失败原因](providers/adr-185-native-provider-failure-details.md)（经 [ADR-211](providers/adr-211-pi-next-turn-reasoning-controls.md) 修订）
- [ADR-180：Oh My Pi 原生 RPC Provider](providers/adr-180-omp-native-rpc-provider.md)（经 [ADR-190](providers/adr-190-omp-acp-settlement.md)、[ADR-194](providers/adr-194-omp-settlement-reconciliation.md) 修订）
- [ADR-179：Provider 自身行为的测试归 provider crate](providers/adr-179-provider-owned-tests.md)
- [ADR-178：OpenCode 轻量可用性探测与冷恢复预算](providers/adr-178-opencode-startup-probe-and-restoration.md)
- [ADR-177：Codex 宿主异常重启后的继续执行与失败上报](providers/adr-177-codex-restart-continuation.md)
- [ADR-176: Pi user images in the shared timeline projection](providers/adr-176-pi-user-image-projection.md)
- [ADR-175：独立恢复 Provider 错误详情与重启收尾](providers/adr-175-restore-provider-recovery.md)（经 [ADR-177](providers/adr-177-codex-restart-continuation.md) 修订）
- [ADR-174: Pi native session import](providers/adr-174-pi-session-import.md)（经 [ADR-176](providers/adr-176-pi-user-image-projection.md)、[ADR-211](providers/adr-211-pi-next-turn-reasoning-controls.md) 修订）
- [ADR-167：用量缓存按原生账号与成功快照隔离](providers/adr-167-account-scoped-successful-usage.md)
- [ADR-162：Codex 失败展示原生原因与 HTTP 状态](providers/adr-162-codex-error-details.md)
- [ADR-159：OpenCode 优先使用 ACP，按缺失能力补充原生观测](providers/adr-159-opencode-acp-native-observation.md)（经 [ADR-178](providers/adr-178-opencode-startup-probe-and-restoration.md) 修订）
- [ADR-157：整图输入与 Codex 用户图片的跨设备展示](providers/adr-157-codex-user-image-projection.md)（经 [ADR-176](providers/adr-176-pi-user-image-projection.md) 修订）
- [ADR-156: Pi native RPC provider](providers/adr-156-pi-native-rpc-provider.md)（经 [ADR-174](providers/adr-174-pi-session-import.md)、[ADR-185](providers/adr-185-native-provider-failure-details.md)、[ADR-189](providers/adr-189-pi-without-host-mode.md)、[ADR-211](providers/adr-211-pi-next-turn-reasoning-controls.md)、[ADR-219](providers/adr-219-pi-slash-command-discovery.md)、[ADR-220](providers/adr-220-pi-extension-notifications.md) 修订）
- [ADR-153：OpenCode ACP 冷启动预算](providers/adr-153-opencode-acp-cold-start-budget.md)（经 [ADR-178](providers/adr-178-opencode-startup-probe-and-restoration.md) 修订）
- [ADR-150：所有 Provider 的上下文压缩生命周期提示](providers/adr-150-provider-compaction-lifecycle.md)
- [ADR-149：Codex 高权限档位自动批准 computer use 访问](providers/adr-149-codex-computer-use-access-auto-approval.md)
- [ADR-147：DSH 原生插件情景模式与权限分离](providers/adr-147-dsh-native-plugin-presets.md)
- [ADR-144：Provider 安全诊断与持久失败提示](providers/adr-144-provider-error-reporting.md)（经 [ADR-162](providers/adr-162-codex-error-details.md)、[ADR-185](providers/adr-185-native-provider-failure-details.md) 修订）
- [ADR-143：OpenCode 2.0.25 兼容与 ACP 审批边界](providers/adr-143-opencode-2-0-25-compatibility.md)
- [ADR-140：Codex MCP 超大字段使用展示预览](providers/adr-140-codex-mcp-display-previews.md)
- [ADR-135：OpenCode 目录就绪与配置确认](providers/adr-135-opencode-catalog-readiness.md)（经 [ADR-143](providers/adr-143-opencode-2-0-25-compatibility.md)、[ADR-153](providers/adr-153-opencode-acp-cold-start-budget.md) 修订）
- [ADR-133：先显示持久 Timeline 页面，后台校验原生历史](providers/adr-133-cached-timeline-first.md)
- [ADR-129：Agent Skills 能力组归 provider](providers/adr-129-provider-owned-skills.md)
- [ADR-125：持久化展示投影索引与按页读取 Timeline](providers/adr-125-durable-timeline-projection-pages.md)
- [ADR-122：Codex Auto-review 的 computer use 工具批准](providers/adr-122-codex-computer-use-approval.md)（经 [ADR-149](providers/adr-149-codex-computer-use-access-auto-approval.md) 修订）
- [ADR-121：ACP 轮次关联与原生历史结算](providers/adr-121-acp-turn-history-reconciliation.md)
- [ADR-119：原生账号用量与 Codex 速度目录](providers/adr-119-native-account-usage-and-codex-speed.md)（经 [ADR-167](providers/adr-167-account-scoped-successful-usage.md) 修订）
- [ADR-115：OpenCode 官方 ACP Provider](providers/adr-115-opencode-acp-provider.md)（经 [ADR-121](providers/adr-121-acp-turn-history-reconciliation.md)、[ADR-135](providers/adr-135-opencode-catalog-readiness.md)、[ADR-143](providers/adr-143-opencode-2-0-25-compatibility.md)、[ADR-159](providers/adr-159-opencode-acp-native-observation.md)、[ADR-195](providers/adr-195-opencode-independent-cancellation.md)、[ADR-206](providers/adr-206-opencode-acp-subagents.md) 修订）
- [ADR-110：OpenCode 双版本私有协议边界](providers/adr-110-opencode-private-protocol-boundary.md)（经 [ADR-115](providers/adr-115-opencode-acp-provider.md) 修订）
- [ADR-102：共享协作契约归 model，Provider 不依赖 metadata](providers/adr-102-provider-metadata-independence.md)（经 [ADR-105](daemon/adr-105-file-tools-and-startup-config.md)、[ADR-106](daemon/adr-106-concrete-file-persistence.md)、[ADR-107](daemon/adr-107-direct-imports-from-owning-crates.md)、[ADR-111](daemon/adr-111-domain-values-and-server-protocol.md) 修订）
- [ADR-101：Provider 拥有摘要生成能力](providers/adr-101-provider-summary-generator.md)（经 [ADR-102](providers/adr-102-provider-metadata-independence.md)、[ADR-107](daemon/adr-107-direct-imports-from-owning-crates.md)、[ADR-118](daemon/adr-118-summary-contracts-and-persistence-configuration.md) 修订）
- [ADR-099：Provider 自选辅助小模型](providers/adr-099-provider-owned-auxiliary-models.md)
- [ADR-091：会话独立执行与 Provider 后台发现](providers/adr-091-independent-session-execution.md)
- [ADR-090：Timeline 单项 768 KiB 与按字节分页](providers/adr-090-timeline-entry-and-page-budgets.md)（经 [ADR-140](providers/adr-140-codex-mcp-display-previews.md) 修订）
- [ADR-089：内置 Provider 装配归 provider crate](providers/adr-089-provider-owned-composition.md)（经 [ADR-099](providers/adr-099-provider-owned-auxiliary-models.md)、[ADR-101](providers/adr-101-provider-summary-generator.md)、[ADR-102](providers/adr-102-provider-metadata-independence.md)、[ADR-179](providers/adr-179-provider-owned-tests.md) 修订）
- [ADR-088：Provider 发现与启动历史同步隔离](providers/adr-088-provider-catalog-startup-isolation.md)（经 [ADR-091](providers/adr-091-independent-session-execution.md) 修订）
- [ADR-087：Antigravity CLI 原生 Provider](providers/adr-087-antigravity-cli-provider.md)
- [ADR-082：DeepSeek Harness 原生交互 Host](providers/adr-082-deepseek-harness-native-host.md)（经 [ADR-147](providers/adr-147-dsh-native-plugin-presets.md) 修订）
- [ADR-074：OpenCode 原生 Provider](providers/adr-074-opencode-native-provider.md)（经 [ADR-110](providers/adr-110-opencode-private-protocol-boundary.md)、[ADR-115](providers/adr-115-opencode-acp-provider.md) 修订）
- [ADR-067：DeepSeek Harness ACP Provider](providers/adr-067-deepseek-harness-acp.md)（经 [ADR-082](providers/adr-082-deepseek-harness-native-host.md) 修订）
- [ADR-058：Server 的有界 metadata generation](providers/adr-058-daemon-metadata-generation.md)（经 [ADR-089](providers/adr-089-provider-owned-composition.md)、[ADR-099](providers/adr-099-provider-owned-auxiliary-models.md)、[ADR-101](providers/adr-101-provider-summary-generator.md) 修订）
- [ADR-052: Native provider capability completion](providers/adr-052-native-provider-capabilities.md)（经 [ADR-236](providers/adr-236-harness-usage-ledger.md) 修订）
- [ADR-050：独立 server 的 Claude Code Provider](providers/adr-050-claude-code-provider.md)（经 [ADR-052](providers/adr-052-native-provider-capabilities.md) 修订）
- [ADR-046：Codex 增量显示历史与运行中追加输入](providers/adr-046-codex-streaming-and-steering.md)
- [ADR-041：Agent 原生控制与 Provider 诊断、用量](providers/adr-041-agent-controls-provider-inspection.md)（经 [ADR-193](providers/adr-193-codex-native-protocol-compatibility.md) 修订）
- [ADR-040：原生 Session 发现、导入、刷新与上下文导出](providers/adr-040-native-session-import-refresh-context.md)
- [ADR-039：Agent Timeline、Provider 发现与创建过程订阅](providers/adr-039-agent-timeline-provider-creation.md)（经 [ADR-088](providers/adr-088-provider-catalog-startup-isolation.md)、[ADR-090](providers/adr-090-timeline-entry-and-page-budgets.md) 修订）
- [ADR-034：Agent 后续 turn 配置与 Session 事件/心跳](providers/adr-034-agent-config-session-events.md)（经 [ADR-035](daemon/adr-035-daemon-capability-groups.md) 修订）
- [ADR-032：独立 server 接通 Codex 原生文本执行](providers/adr-032-daemon-native-provider-execution.md)（经 [ADR-034](providers/adr-034-agent-config-session-events.md)、[ADR-088](providers/adr-088-provider-catalog-startup-isolation.md)、[ADR-091](providers/adr-091-independent-session-execution.md) 修订）
- [ADR-031：拆出 provider 并统一 Workspace 自动化与 state 入口](providers/adr-031-daemon-provider.md)（经 [ADR-032](providers/adr-032-daemon-native-provider-execution.md)、[ADR-035](daemon/adr-035-daemon-capability-groups.md)、[ADR-037](daemon/adr-037-daemon-model-context.md)、[ADR-089](providers/adr-089-provider-owned-composition.md) 修订）
- [ADR-027：独立 AgentSession 与 AgentManager 生命周期边界](providers/adr-027-independent-agent-session-manager.md)（经 [ADR-031](providers/adr-031-daemon-provider.md)、[ADR-032](providers/adr-032-daemon-native-provider-execution.md)、[ADR-041](providers/adr-041-agent-controls-provider-inspection.md) 修订）

## 客户端、连接与品牌

- [ADR-236：GitHub Release 附带 Google 签名的 Android APK](clients/adr-236-google-signed-apk-release-asset.md)
- [ADR-235：稳定版 Release 通过 API 启动 iOS 内部 TestFlight 构建](clients/adr-235-ios-testflight-in-release-workflow.md)
- [ADR-234：稳定版 Release 自动提交 Google Play 内部测试](clients/adr-234-play-internal-in-release-workflow.md)（经 [ADR-236](clients/adr-236-google-signed-apk-release-asset.md) 修订）
- [ADR-223：桌面多窗口与 Workspace 状态归属](clients/adr-223-desktop-workspace-windows.md)
- [ADR-218：Hub feature 协商收敛在线连接](clients/adr-218-hub-feature-negotiation.md)
- [ADR-216：默认数据目录统一为 `~/.ait`](clients/adr-216-default-data-directory-ait.md)
- [ADR-214：客户端 Workspace Bot 标签页 v2](clients/adr-214-workspace-bot-tab-v2.md)（经 [ADR-215](bot/adr-215-bot-permissions-and-scoped-mcp.md)、[ADR-221](bot/adr-221-read-only-bot-session-inspection.md) 修订）
- [ADR-212：打包的 daemon 可执行文件统一命名为 aitd](clients/adr-212-packaged-daemon-name-aitd.md)
- [ADR-211：SDK 原生使用 daemon 协议](clients/adr-211-sdk-native-daemon-protocol.md)
- [ADR-207：桌面 daemon 就绪探测与进程生命周期分离](clients/adr-207-desktop-daemon-readiness-ownership.md)
- [ADR-204：在线业务连接只使用 Hub](clients/adr-204-hub-only-online-business-transport.md)（经 [ADR-218](clients/adr-218-hub-feature-negotiation.md) 修订）
- [ADR-188：网页客户端以独立镜像部署在网站根路径](clients/adr-188-standalone-web-client-image.md)
- [ADR-187：GitHub 手动入口用专用标签触发 iOS App Store 构建](clients/adr-187-ios-app-store-cloud-trigger.md)（经 [ADR-235](clients/adr-235-ios-testflight-in-release-workflow.md) 修订）
- [ADR-186：内置浏览器按 Workspace daemon 选择出站位置](clients/adr-186-workspace-browser-egress.md)
- [ADR-184：Windows Desktop 不附带内置 daemon](clients/adr-184-windows-desktop-without-daemon.md)
- [ADR-183：iOS CI 使用 Xcode Cloud 归档与内部分发](clients/adr-183-ios-xcode-cloud-release.md)（经 [ADR-187](clients/adr-187-ios-app-store-cloud-trigger.md)、[ADR-235](clients/adr-235-ios-testflight-in-release-workflow.md) 修订）
- [ADR-182：客户端接入 Hub 账号服务](clients/adr-182-hub-account-client.md)（经 [ADR-204](clients/adr-204-hub-only-online-business-transport.md) 修订）
- [ADR-177：移动端本地通知与 APNs／FCM 直连](clients/adr-177-native-local-and-remote-notifications.md)
- [ADR-175：Google Play AAB 在 GitHub runner 上构建和提交](clients/adr-175-android-play-gradle-release.md)（经 [ADR-234](clients/adr-234-play-internal-in-release-workflow.md)、[ADR-236](clients/adr-236-google-signed-apk-release-asset.md) 修订）
- [ADR-173：移动端终端输入复用 React Native](clients/adr-173-shared-native-terminal-input.md)
- [ADR-172：Android APK 在 GitHub runner 上用 Gradle 构建](clients/adr-172-android-gradle-ci-build.md)（已由 [ADR-175](clients/adr-175-android-play-gradle-release.md) 取代）
- [ADR-171：iOS 窗口由 UIScene 管理](clients/adr-171-ios-scene-lifecycle.md)
- [ADR-170：完整回滚 PR #381](clients/adr-170-revert-error-diagnostics-and-recovery.md)（经 [ADR-175](providers/adr-175-restore-provider-recovery.md) 修订）
- [ADR-169：统一 Markdown 渲染与移动面板编辑生命周期](clients/adr-169-shared-markdown-and-composer-lifetimes.md)
- [ADR-166：后台错误只记录、不弹错误提示](clients/adr-166-background-errors-without-toasts.md)（已由 [ADR-170](clients/adr-170-revert-error-diagnostics-and-recovery.md) 取代）
- [ADR-165：macOS 通过 AIT Server 检查与下载更新](clients/adr-165-macos-ait-server-updates.md)
- [ADR-164：移除 Workspace 服务代理 URL 字段](clients/adr-164-remove-workspace-service-proxy-urls.md)
- [ADR-163：nightly 附带独立 daemon 归档](clients/adr-163-nightly-standalone-daemon.md)（经 [ADR-212](clients/adr-212-packaged-daemon-name-aitd.md) 修订）
- [ADR-160：清理旧公钥中继协议与测试残留](clients/adr-160-remove-legacy-relay-protocol.md)（经 [ADR-164](clients/adr-164-remove-workspace-service-proxy-urls.md) 修订）
- [ADR-158：Release 附带独立 daemon 归档](clients/adr-158-standalone-daemon-release.md)（经 [ADR-163](clients/adr-163-nightly-standalone-daemon.md)、[ADR-212](clients/adr-212-packaged-daemon-name-aitd.md) 修订）
- [ADR-156：初始化进度与活动事件使用 Ait 标准名称](clients/adr-156-setup-and-activity-events.md)
- [ADR-154：账号状态通过共享客户端 Channel 推送](clients/adr-154-account-status-channel.md)（经 [ADR-218](clients/adr-218-hub-feature-negotiation.md) 修订）
- [ADR-152：在线中继 Workspace 的浏览器回环转发](clients/adr-152-remote-workspace-browser-loopback.md)（经 [ADR-186](clients/adr-186-workspace-browser-egress.md) 修订）
- [ADR-151：Android 边缘手势进入移动面板导航](clients/adr-151-android-panel-edge-gestures.md)
- [ADR-141：客户端测试功能开关与 Workspace Bot 标签页](clients/adr-141-experimental-workspace-bots.md)（经 [ADR-145](bot/adr-145-bot-permission-denial.md)、[ADR-213](bot/adr-213-single-workspace-manager-bot.md)、[ADR-214](clients/adr-214-workspace-bot-tab-v2.md) 修订）
- [ADR-137：清理已移除功能的共享协议与 SDK](clients/adr-137-remove-retired-feature-protocol.md)（经 [ADR-233](bot/adr-233-webhook-channel-center-endpoints.md) 修订）
- [ADR-132：管理网站中的浏览器客户端](clients/adr-132-web-console-client.md)（经 [ADR-188](clients/adr-188-standalone-web-client-image.md) 修订）
- [ADR-128：桌面内置 daemon 可在退出后继续运行](clients/adr-128-desktop-daemon-keep-running.md)（经 [ADR-207](clients/adr-207-desktop-daemon-readiness-ownership.md) 修订）
- [ADR-124：登录后自动添加全部在线服务主机](clients/adr-124-automatic-online-host-import.md)
- [ADR-123：客户端仅保留浏览器统一登录](clients/adr-123-browser-only-account-login.md)
- [ADR-122：在线服务主机进入客户端持久注册表](clients/adr-122-persistent-online-service-hosts.md)（经 [ADR-124](clients/adr-124-automatic-online-host-import.md) 修订）
- [ADR-117：终端配色查询与外观更新生命周期](clients/adr-117-terminal-palette-and-appearance-lifetime.md)
- [ADR-114：iOS 终端组合输入归 UIKit](clients/adr-114-ios-terminal-ime.md)（已由 [ADR-173](clients/adr-173-shared-native-terminal-input.md) 取代）
- [ADR-098：客户端消息分块与文件上传背压](clients/adr-098-acknowledged-client-chunks.md)
- [ADR-097：Google Play 内部测试手动发布](clients/adr-097-google-play-internal-release.md)（经 [ADR-175](clients/adr-175-android-play-gradle-release.md)、[ADR-234](clients/adr-234-play-internal-in-release-workflow.md) 修订）
- [ADR-096：Desktop 内置 daemon 默认同步与手动下线](clients/adr-096-desktop-default-host-sync.md)（经 [ADR-201](daemon/adr-201-daemon-owned-publication.md) 修订）
- [ADR-094：客户端统一使用 Ait 标准方法名](clients/adr-094-canonical-ait-client-methods.md)（经 [ADR-211](clients/adr-211-sdk-native-daemon-protocol.md) 修订）
- [ADR-087：中心设备管理与 Linux/macOS daemon 的 JWT 授权](clients/adr-087-headless-device-authorization.md)（经 [ADR-201](daemon/adr-201-daemon-owned-publication.md) 修订）
- [ADR-086：桌面、Android 与 iOS 的 Authing 浏览器登录](clients/adr-086-authing-native-login.md)（经 [ADR-087](clients/adr-087-headless-device-authorization.md)、[ADR-123](clients/adr-123-browser-only-account-login.md)、[ADR-201](daemon/adr-201-daemon-owned-publication.md) 修订）
- [ADR-085：Daemon 同步的稳定节点身份](clients/adr-085-stable-daemon-publication.md)（经 [ADR-201](daemon/adr-201-daemon-owned-publication.md) 修订）
- [ADR-084：iOS 在线服务账户与主机中继](clients/adr-084-ios-account-relay.md)（经 [ADR-086](clients/adr-086-authing-native-login.md)、[ADR-122](clients/adr-122-persistent-online-service-hosts.md) 修订）
- [ADR-083：在线服务登录与逐主机同步分离](clients/adr-083-online-service-host-sync.md)（经 [ADR-085](clients/adr-085-stable-daemon-publication.md)、[ADR-096](clients/adr-096-desktop-default-host-sync.md)、[ADR-124](clients/adr-124-automatic-online-host-import.md)、[ADR-201](daemon/adr-201-daemon-owned-publication.md) 修订）
- [ADR-080：Android APK 独立手动发布](clients/adr-080-standalone-android-release.md)
- [ADR-079：移动端统一使用 Expo EAS 构建](clients/adr-079-mobile-eas-builds.md)（已由 [ADR-183](clients/adr-183-ios-xcode-cloud-release.md)、[ADR-172](clients/adr-172-android-gradle-ci-build.md) 取代）
- [ADR-078：Android 发布改为手动可选](clients/adr-078-optional-android-release.md)
- [ADR-077：Android APK 的 GitHub Release 发布](clients/adr-077-android-apk-release.md)（经 [ADR-078](clients/adr-078-optional-android-release.md) 修订）
- [ADR-076：Android 账户与中继客户端](clients/adr-076-android-account-relay.md)（经 [ADR-084](clients/adr-084-ios-account-relay.md)、[ADR-086](clients/adr-086-authing-native-login.md)、[ADR-122](clients/adr-122-persistent-online-service-hosts.md) 修订）
- [ADR-075：Relay 协议定义与连接执行分离](clients/adr-075-relay-protocol-modules.md)（经 [ADR-098](clients/adr-098-acknowledged-client-chunks.md) 修订）
- [ADR-074：账户发现与按需反向中继](clients/adr-074-account-host-relay.md)（经 [ADR-075](clients/adr-075-relay-protocol-modules.md)、[ADR-076](clients/adr-076-android-account-relay.md)、[ADR-083](clients/adr-083-online-service-host-sync.md)、[ADR-086](clients/adr-086-authing-native-login.md)、[ADR-108](daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)、[ADR-120](daemon/adr-120-single-connection-startup-admission.md)、[ADR-122](clients/adr-122-persistent-online-service-hosts.md)、[ADR-154](clients/adr-154-account-status-channel.md)、[ADR-201](daemon/adr-201-daemon-owned-publication.md)、[ADR-204](clients/adr-204-hub-only-online-business-transport.md) 修订）
- [ADR-073：桌面主窗口的导航信任边界](clients/adr-073-desktop-renderer-navigation.md)
- [ADR-070：Ait iOS TestFlight 手动发布工作流](clients/adr-070-ios-testflight-release.md)（已由 [ADR-183](clients/adr-183-ios-xcode-cloud-release.md) 取代）
- [ADR-063：Ait 本地 UI 包与原生测试连接](clients/adr-063-ait-local-ui-packages.md)
- [ADR-062：Ait 运行路径与项目配置文件](clients/adr-062-ait-runtime-paths.md)（经 [ADR-216](clients/adr-216-default-data-directory-ait.md) 修订）
- [ADR-061：App E2E 使用 Ait server，移除 relay 与插件运行功能](clients/adr-061-app-ait-e2e-remove-relay-plugin.md)（经 [ADR-074](clients/adr-074-account-host-relay.md)、[ADR-160](clients/adr-160-remove-legacy-relay-protocol.md) 修订）
- [ADR-056：Ait 与 Paseo 的共享宿主资源隔离](clients/adr-056-paseo-coexistence.md)
- [ADR-055：Ait 与 Paseo 的桌面数据和链接隔离](clients/adr-055-desktop-profile-isolation.md)（经 [ADR-056](clients/adr-056-paseo-coexistence.md)、[ADR-073](clients/adr-073-desktop-renderer-navigation.md)、[ADR-216](clients/adr-216-default-data-directory-ait.md) 修订）
- [ADR-054：桌面服务监听配置与网络地址](clients/adr-054-desktop-daemon-listen.md)
- [ADR-053：Ait 0.0.7 桌面发布切换到 apps/desktop](clients/adr-053-paseo-desktop-release.md)（经 [ADR-055](clients/adr-055-desktop-profile-isolation.md) 修订）
- [ADR-049：独立 App 的 Rust server 浏览器连接](clients/adr-049-app-rust-browser-transport.md)（经 [ADR-054](clients/adr-054-desktop-daemon-listen.md) 修订）
- [ADR-048：Paseo workspace 与 Rust 桌面服务启动](clients/adr-048-paseo-desktop-rust-launcher.md)（经 [ADR-054](clients/adr-054-desktop-daemon-listen.md)、[ADR-128](clients/adr-128-desktop-daemon-keep-running.md)、[ADR-184](clients/adr-184-windows-desktop-without-daemon.md)、[ADR-207](clients/adr-207-desktop-daemon-readiness-ownership.md) 修订）
- [ADR-044：Paseo 前端适配 Rust server 协议](clients/adr-044-paseo-client-rust-transport.md)（经 [ADR-045](daemon/adr-045-remove-hub-chat-loop.md)、[ADR-094](clients/adr-094-canonical-ait-client-methods.md)、[ADR-211](clients/adr-211-sdk-native-daemon-protocol.md) 修订）

## 品牌与视觉

- [ADR-051：AIT 品牌识别与日间视觉系统](branding/adr-051-ait-brand-identity.md)
