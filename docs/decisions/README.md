# 架构决策分类索引

文档按当前能力归属分类，每类按编号从新到旧排列。
ADR-001–024 已随旧架构删除，可从 Git 历史查阅；旧 ADR 中对这些编号的引用指向历史版本。
ADR-074、082、083、084、122 在不同类别中各有两份，引用时以路径区分。

## Bot

- [ADR-145：Bot 执行期间自动拒绝权限请求](bot/adr-145-bot-permission-denial.md)
- [ADR-139：Workspace Bot 与独占 Channel](bot/adr-139-workspace-bots-and-owned-channels.md)

## Daemon 与协议

- [ADR-181：Host 向 Hub 发布事件与目录摘要](daemon/adr-181-hub-host-publication.md)
- [ADR-148：daemon 的无状态 MCP 编排接口](daemon/adr-148-daemon-mcp.md)
- [ADR-142：PTY 拥有基础终端设备属性回复](daemon/adr-142-terminal-primary-device-attributes.md)

- [ADR-134：已接纳的普通 RPC 等待共享任务额度](daemon/adr-134-admitted-rpc-job-waiting.md)
- [ADR-127：移除 daemon 配对 offer RPC](daemon/adr-127-remove-daemon-pairing-offer.md)
- [ADR-120：单连接启动请求的有界排队](daemon/adr-120-single-connection-startup-admission.md)
- [ADR-118：摘要接口归 model，配置文件适配归 persistence](daemon/adr-118-summary-contracts-and-persistence-configuration.md)
- [ADR-116：库 crate 只公开被其他 crate 使用的项](daemon/adr-116-crate-visibility.md)
- [ADR-112：file 瘦身为 persistence，宿主专用文件归 daemon](daemon/adr-112-persistence-crate.md)
- [ADR-111：纯业务数据归 domain，连接协议并入 model::server](daemon/adr-111-domain-values-and-server-protocol.md)
- [ADR-109：本地故障证据与 Harness 日志采集](daemon/adr-109-local-incident-evidence.md)
- [ADR-108：Relay RPC 与基础连接方法归所属 crate](daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)
- [ADR-107：共享组件直接从所属 crate 导入](daemon/adr-107-direct-imports-from-owning-crates.md)
- [ADR-106：具体文件持久化归 file，model 仅声明契约](daemon/adr-106-concrete-file-persistence.md)
- [ADR-105：File 工具、通用 Registry 与启动配置独立成 crate](daemon/adr-105-file-tools-and-startup-config.md)
- [ADR-103：Terminal 仅依赖 model 的共享契约](daemon/adr-103-terminal-model-dependency.md)
- [ADR-100：功能 crate 作为完整服务安装](daemon/adr-100-crate-level-service-installation.md)
- [ADR-095：组件自行声明方法元数据](daemon/adr-095-component-method-declarations.md)
- [ADR-093：以可消费 Context 逐级处理请求](daemon/adr-093-consumable-request-context.md)
- [ADR-072：Workspace 名称与当前文档边界](daemon/adr-072-workspace-names-and-documentation.md)
- [ADR-064：默认离线语音与模型准备](daemon/adr-064-offline-speech.md)
- [ADR-047：移除 Plugin 接口并独立实现 Schedule 与 Browser](daemon/adr-047-daemon-schedule-browser.md)
- [ADR-045：移除 Hub、Chat 与 Loop 接口](daemon/adr-045-remove-hub-chat-loop.md)
- [ADR-042：连接级语音、听写与双后端](daemon/adr-042-daemon-voice.md)
- [ADR-038：protocol 仅依赖公共 model](daemon/adr-038-daemon-protocol-dependencies.md)
- [ADR-037：公共 Context 与具体 crate 分发](daemon/adr-037-daemon-model-context.md)
- [ADR-036：请求先进入所属 crate 再分发到能力组](daemon/adr-036-daemon-crate-dispatch.md)
- [ADR-035：能力分组与安装规则归所属 server crate](daemon/adr-035-daemon-capability-groups.md)
- [ADR-033：独立 terminal 与完整 Terminal 方法分组](daemon/adr-033-daemon-terminal.md)
- [ADR-026：规范化 Paseo WebSocket 接口并按能力分期接入](daemon/adr-026-canonical-paseo-websocket-surface.md)

## 工作区、文件与 Git

- [ADR-168：Setup 全局摘要与面板日志订阅分离](workspace/adr-168-scoped-setup-observation.md)

- [ADR-138：定时任务固定归属于 Workspace](workspace/adr-138-workspace-owned-schedules.md)

- [ADR-132：Checkout 刷新独立准入与有界输出背压](workspace/adr-132-checkout-refresh-admission-and-backpressure.md)：隔离前台 Git 任务，diff 拥堵时等待输出额度并保持连接。

- [ADR-136：拍平单功能区层并收口重复助手](workspace/adr-136-single-area-layer-flattening.md)
- [ADR-131：metadata 按能力组组织模块](workspace/adr-131-metadata-capability-groups.md)
- [ADR-130：Workspace 与 Project 方法归 metadata，filesystem 只保留本地适配器](workspace/adr-130-workspace-methods-owned-by-metadata.md)
- [ADR-126：按身份读取目标 Workspace，完整目录后台同步](workspace/adr-126-workspace-target-read.md)
- [ADR-113：filesystem 按能力组组织模块](workspace/adr-113-filesystem-capability-groups.md)
- [ADR-104：Filesystem 仅通过 model 契约协作](workspace/adr-104-filesystem-model-collaboration.md)
- [ADR-092：工作区重置同步 origin 同名分支](workspace/adr-092-reset-same-named-remote-branch.md)
- [ADR-084：工作区主按钮按提交与 PR 生命周期推进](workspace/adr-084-workspace-primary-git-action.md)
- [ADR-083：工作区重置到 origin 的最新默认分支](workspace/adr-083-reset-workspace-to-origin-default.md)
- [ADR-082：Workspace 可空字段使用单级 Option](workspace/adr-082-canonical-nullable-workspace-fields.md)
- [ADR-081：由状态变更唤醒 Workspace 与 Agent 目录订阅](workspace/adr-081-directory-change-push.md)
- [ADR-071：在 Rust checkout adapter 生成 Diff 语法 token](workspace/adr-071-checkout-diff-syntax-highlighting.md)
- [ADR-069：活跃工作区的后台 Git fetch](workspace/adr-069-background-git-fetch.md)
- [ADR-068：Workspace 侧边栏运行时摘要](workspace/adr-068-workspace-runtime-summaries.md)
- [ADR-066：通过 glab 支持 GitLab Forge](workspace/adr-066-gitlab-forge.md)
- [ADR-065：Paseo 目录、时间线与会话 API 兼容行为](workspace/adr-065-paseo-directory-and-timeline-projections.md)
- [ADR-060：统一 Workspace 创建入口支持 Worktree](workspace/adr-060-workspace-create-worktree.md)
- [ADR-043：独立 server 的 Skills 选择与文件安装](workspace/adr-043-daemon-skills.md)
- [ADR-030：纵向拆出 filesystem](workspace/adr-030-daemon-filesystem.md)
- [ADR-029：统一 Paseo Project 并纵向拆出 metadata](workspace/adr-029-daemon-metadata.md)
- [ADR-028：GitHub 仓库发现与独立 Project 克隆注册](workspace/adr-028-github-project-provisioning.md)
- [ADR-025：先移植 Paseo Project / Workspace 模型与 registry](workspace/adr-025-paseo-registry.md)

## Provider、Agent 与会话

- [ADR-190：Ait ACP 桥接官方 OMP 与通用插件接口](providers/adr-190-omp-acp-settlement.md)

- [ADR-185：各 Provider 保留原生失败原因](providers/adr-185-native-provider-failure-details.md)

- [ADR-180：Oh My Pi 原生 RPC Provider](providers/adr-180-omp-native-rpc-provider.md)

- [ADR-179：Provider 自身行为的测试归 provider crate](providers/adr-179-provider-owned-tests.md)

- [ADR-178：OpenCode 轻量可用性探测与冷恢复预算](providers/adr-178-opencode-startup-probe-and-restoration.md)

- [ADR-177：Codex 宿主异常重启后的继续执行与失败上报](providers/adr-177-codex-restart-continuation.md)

- [ADR-175：独立恢复 Provider 错误详情与重启收尾](providers/adr-175-restore-provider-recovery.md)
- [ADR-162：Codex 失败展示原生原因与 HTTP 状态](providers/adr-162-codex-error-details.md)

- [ADR-176：Pi 用户图片的共享时间线投影](providers/adr-176-pi-user-image-projection.md)
- [ADR-174：Pi 原生会话导入](providers/adr-174-pi-session-import.md)

- [ADR-167：用量缓存按原生账号与成功快照隔离](providers/adr-167-account-scoped-successful-usage.md)

- [ADR-157：整图输入与 Codex 用户图片的跨设备展示](providers/adr-157-codex-user-image-projection.md)
- [ADR-156：Pi 原生 RPC Provider](providers/adr-156-pi-native-rpc-provider.md)
- [ADR-153：OpenCode ACP 冷启动预算](providers/adr-153-opencode-acp-cold-start-budget.md)
- [ADR-150：所有 Provider 的上下文压缩生命周期提示](providers/adr-150-provider-compaction-lifecycle.md)
- [ADR-149：Codex 高权限档位自动批准 computer use 访问](providers/adr-149-codex-computer-use-access-auto-approval.md)

- [ADR-147：DSH 原生插件情景模式与权限分离](providers/adr-147-dsh-native-plugin-presets.md)

- [ADR-144：Provider 安全诊断与持久失败提示](providers/adr-144-provider-error-reporting.md)
- [ADR-143：OpenCode 2.0.25 兼容与 ACP 审批边界](providers/adr-143-opencode-2-0-25-compatibility.md)
- [ADR-140：Codex MCP 超大字段使用展示预览](providers/adr-140-codex-mcp-display-previews.md)
- [ADR-135：OpenCode 目录就绪与配置确认](providers/adr-135-opencode-catalog-readiness.md)
- [ADR-133：先显示持久 Timeline 页面，后台校验原生历史](providers/adr-133-cached-timeline-first.md)
- [ADR-129：Agent Skills 能力组归 provider](providers/adr-129-provider-owned-skills.md)
- [ADR-125：持久化展示投影索引与按页读取 Timeline](providers/adr-125-durable-timeline-projection-pages.md)
- [ADR-122：Codex Auto-review 的 computer use 工具批准](providers/adr-122-codex-computer-use-approval.md)
- [ADR-121：ACP 轮次关联与原生历史结算](providers/adr-121-acp-turn-history-reconciliation.md)
- [ADR-119：原生账号用量与 Codex 速度目录](providers/adr-119-native-account-usage-and-codex-speed.md)
- [ADR-115：OpenCode 官方 ACP Provider](providers/adr-115-opencode-acp-provider.md)
- [ADR-110：OpenCode 双版本私有协议边界](providers/adr-110-opencode-private-protocol-boundary.md)
- [ADR-102：共享协作契约归 model，Provider 不依赖 metadata](providers/adr-102-provider-metadata-independence.md)
- [ADR-101：Provider 拥有摘要生成能力](providers/adr-101-provider-summary-generator.md)
- [ADR-099：Provider 自选辅助小模型](providers/adr-099-provider-owned-auxiliary-models.md)
- [ADR-091：会话独立执行与 Provider 后台发现](providers/adr-091-independent-session-execution.md)
- [ADR-090：Timeline 单项 768 KiB 与按字节分页](providers/adr-090-timeline-entry-and-page-budgets.md)
- [ADR-089：内置 Provider 装配归 provider crate](providers/adr-089-provider-owned-composition.md)
- [ADR-088：Provider 发现与启动历史同步隔离](providers/adr-088-provider-catalog-startup-isolation.md)
- [ADR-087：Antigravity CLI 原生 Provider](providers/adr-087-antigravity-cli-provider.md)
- [ADR-082：DeepSeek Harness 原生交互 Host](providers/adr-082-deepseek-harness-native-host.md)
- [ADR-074：OpenCode 原生 Provider](providers/adr-074-opencode-native-provider.md)
- [ADR-067：DeepSeek Harness ACP Provider](providers/adr-067-deepseek-harness-acp.md)
- [ADR-058：Server 的有界 metadata generation](providers/adr-058-daemon-metadata-generation.md)
- [ADR-052：Native provider capability completion](providers/adr-052-native-provider-capabilities.md)
- [ADR-050：独立 server 的 Claude Code Provider](providers/adr-050-claude-code-provider.md)
- [ADR-046：Codex 增量显示历史与运行中追加输入](providers/adr-046-codex-streaming-and-steering.md)
- [ADR-041：Agent 原生控制与 Provider 诊断、用量](providers/adr-041-agent-controls-provider-inspection.md)
- [ADR-040：原生 Session 发现、导入、刷新与上下文导出](providers/adr-040-native-session-import-refresh-context.md)
- [ADR-039：Agent Timeline、Provider 发现与创建过程订阅](providers/adr-039-agent-timeline-provider-creation.md)
- [ADR-034：Agent 后续 turn 配置与 Session 事件/心跳](providers/adr-034-agent-config-session-events.md)
- [ADR-032：独立 server 接通 Codex 原生文本执行](providers/adr-032-daemon-native-provider-execution.md)
- [ADR-031：拆出 provider 并统一 Workspace 自动化与 state 入口](providers/adr-031-daemon-provider.md)
- [ADR-027：独立 AgentSession 与 AgentManager 生命周期边界](providers/adr-027-independent-agent-session-manager.md)

## 客户端、连接与品牌

- [ADR-187：GitHub 手动入口用专用标签触发 iOS App Store 构建](clients/adr-187-ios-app-store-cloud-trigger.md)（GitHub 触发入口已撤销）

- [ADR-186：内置浏览器按 Workspace daemon 选择出站位置](clients/adr-186-workspace-browser-egress.md)

- [ADR-182：客户端接入 Hub 账号服务](clients/adr-182-hub-account-client.md)

- [ADR-177：移动端本地通知与 APNs／FCM 直连](clients/adr-177-native-local-and-remote-notifications.md)

- [ADR-175：Google Play AAB 在 GitHub runner 上构建和提交](clients/adr-175-android-play-gradle-release.md)

- [ADR-173：移动端终端输入复用 React Native](clients/adr-173-shared-native-terminal-input.md)

- [ADR-170：完整回滚 PR #381](clients/adr-170-revert-error-diagnostics-and-recovery.md)
- [ADR-169：统一 Markdown 渲染与移动面板编辑生命周期](clients/adr-169-shared-markdown-and-composer-lifetimes.md)
- [ADR-166：后台错误只记录、不弹错误提示](clients/adr-166-background-errors-without-toasts.md)
- [ADR-165：macOS 通过 AIT Server 检查与下载更新](clients/adr-165-macos-ait-server-updates.md)
- [ADR-164：移除 Workspace 服务代理 URL 字段](clients/adr-164-remove-workspace-service-proxy-urls.md)
- [ADR-163：nightly 附带独立 daemon 归档](clients/adr-163-nightly-standalone-daemon.md)
- [ADR-160：清理旧公钥中继协议与测试残留](clients/adr-160-remove-legacy-relay-protocol.md)
- [ADR-154：账号状态通过共享客户端 Channel 推送](clients/adr-154-account-status-channel.md)
- [ADR-152：在线中继 Workspace 的浏览器回环转发](clients/adr-152-remote-workspace-browser-loopback.md)
- [ADR-151：Android 边缘手势进入移动面板导航](clients/adr-151-android-panel-edge-gestures.md)
- [ADR-137：清理已移除功能的共享协议与 SDK](clients/adr-137-remove-retired-feature-protocol.md)
- [ADR-132：管理网站中的浏览器客户端](clients/adr-132-web-console-client.md)
- [ADR-128：桌面内置 daemon 可在退出后继续运行](clients/adr-128-desktop-daemon-keep-running.md)
- [ADR-124：登录后自动添加全部在线服务主机](clients/adr-124-automatic-online-host-import.md)

- [ADR-123：客户端仅保留浏览器统一登录](clients/adr-123-browser-only-account-login.md)
- [ADR-122：在线服务主机进入客户端持久注册表](clients/adr-122-persistent-online-service-hosts.md)
- [ADR-117：终端配色查询与外观更新生命周期](clients/adr-117-terminal-palette-and-appearance-lifetime.md)
- [ADR-114：iOS 终端组合输入归 UIKit](clients/adr-114-ios-terminal-ime.md)（已由 ADR-173 取代）
- [ADR-098：客户端消息分块与文件上传背压](clients/adr-098-acknowledged-client-chunks.md)
- [ADR-097：Google Play 内部测试手动发布](clients/adr-097-google-play-internal-release.md)
- [ADR-096：Desktop 内置 daemon 默认同步与手动下线](clients/adr-096-desktop-default-host-sync.md)
- [ADR-094：客户端统一使用 Ait 标准方法名](clients/adr-094-canonical-ait-client-methods.md)
- [ADR-087：中心设备管理与 Linux/macOS daemon 的 JWT 授权](clients/adr-087-headless-device-authorization.md)
- [ADR-086：桌面、Android 与 iOS 的 Authing 浏览器登录](clients/adr-086-authing-native-login.md)
- [ADR-085：Daemon 同步的稳定节点身份](clients/adr-085-stable-daemon-publication.md)
- [ADR-084：iOS 在线服务账户与主机中继](clients/adr-084-ios-account-relay.md)
- [ADR-083：在线服务登录与逐主机同步分离](clients/adr-083-online-service-host-sync.md)
- [ADR-080：Android APK 独立手动发布](clients/adr-080-standalone-android-release.md)
- [ADR-079：移动端统一使用 Expo EAS 构建](clients/adr-079-mobile-eas-builds.md)
- [ADR-078：Android 发布改为手动可选](clients/adr-078-optional-android-release.md)
- [ADR-077：Android APK 的 GitHub Release 发布](clients/adr-077-android-apk-release.md)
- [ADR-076：Android 账户与中继客户端](clients/adr-076-android-account-relay.md)
- [ADR-075：Relay 协议定义与连接执行分离](clients/adr-075-relay-protocol-modules.md)
- [ADR-074：账户发现与按需反向中继](clients/adr-074-account-host-relay.md)
- [ADR-073：桌面主窗口的导航信任边界](clients/adr-073-desktop-renderer-navigation.md)
- [ADR-070：Ait iOS TestFlight 手动发布工作流](clients/adr-070-ios-testflight-release.md)
- [ADR-063：Ait 本地 UI 包与原生测试连接](clients/adr-063-ait-local-ui-packages.md)
- [ADR-062：Ait 运行路径与项目配置文件](clients/adr-062-ait-runtime-paths.md)
- [ADR-061：App E2E 使用 Ait server，移除 relay 与插件运行功能](clients/adr-061-app-ait-e2e-remove-relay-plugin.md)
- [ADR-056：Ait 与 Paseo 的共享宿主资源隔离](clients/adr-056-paseo-coexistence.md)
- [ADR-055：Ait 与 Paseo 的桌面数据和链接隔离](clients/adr-055-desktop-profile-isolation.md)
- [ADR-054：桌面服务监听配置与网络地址](clients/adr-054-desktop-daemon-listen.md)
- [ADR-053：Ait 0.0.7 桌面发布切换到 apps/desktop](clients/adr-053-paseo-desktop-release.md)
- [ADR-049：独立 App 的 Rust server 浏览器连接](clients/adr-049-app-rust-browser-transport.md)
- [ADR-048：Paseo workspace 与 Rust 桌面服务启动](clients/adr-048-paseo-desktop-rust-launcher.md)
- [ADR-044：Paseo 前端适配 Rust server 协议](clients/adr-044-paseo-client-rust-transport.md)

## 品牌与视觉

- [ADR-051：AIT 品牌识别与日间视觉系统](branding/adr-051-ait-brand-identity.md)
