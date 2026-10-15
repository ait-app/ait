# Ait 文档

- [ADR-142：PTY 拥有基础终端设备属性回复](decisions/daemon/adr-142-terminal-primary-device-attributes.md)：节点直接完成 DA1 握手，避免 fish 启动等待客户端订阅。

- [ADR-140：Headless 本地 API 凭据](decisions/daemon/adr-140-headless-local-api-token.md)：启动自动生成、持久化与环境覆盖。
- [ADR-132：管理网站中的浏览器客户端](decisions/clients/adr-132-web-console-client.md)：同源会话、浏览器中继、Web 导出与客户端入口。

本文档索引只覆盖当前 Ait：Rust daemon、Electron desktop、共享 Web/移动界面和仓库内 SDK。
当前源码布局与依赖见[当前架构](architecture/README.md)，目录命名规则见 [ADR-072](decisions/daemon/adr-072-workspace-names-and-documentation.md)。

## 开始使用与开发

- [daemon MCP 接口](operations/mcp.md)：Streamable HTTP、40 个编排工具、鉴权与 Workspace 上下文。
- [项目说明](../README.md)：开发入口、workspace 和验证命令。
- [桌面端](../apps/desktop/README.md)：Electron 启动、构建和打包。
- [移动端与 Web](../apps/mobile/README.md)：Expo、浏览器连接与原生构建。
- [Relay 客户端登录保持](plans/relay-client-session-persistence.md)：14 天登录、退出凭据保护、网页重开恢复与待实施的自动续期方案。
- [Workspace 定时任务](operations/schedules.md)：任务归属、目标、触发规则、运行历史与旧任务兼容；[设计与验证报告](reports/daemon/workspace-schedules-verification-2026-10-12.md)、[列表与统一创建验证](reports/clients/workspace-schedule-timeline-2026-10-12.md)。
- [Linux/macOS 无人值守 daemon](operations/headless-daemon.md)：双模式授权、凭据恢复和脚本启动。
- [daemon 使用与连接协议](operations/daemon.md)：配置、鉴权、数据目录和生命周期。
- [当前架构](architecture/README.md)：能力归属、依赖边界和数据所有权。
- [Bot 后端设计](plans/bot-backend.md)：Workspace Bot、独占 Channel、隐藏 session、内置 tab 对话与 Telegram、飞书、Email 接入方案。

## 架构决策

- [ADR-152：在线中继 Workspace 的浏览器回环转发](decisions/clients/adr-152-remote-workspace-browser-loopback.md)：按 Host 的浏览器 profile、认证 TCP 通道和导航前配置。

- [ADR-151：Android 边缘手势进入移动面板导航](decisions/clients/adr-151-android-panel-edge-gestures.md)：左右方向、原生适配边界、面板展开时返回优先关闭与旧系统限制。

- [ADR-150：所有 Provider 的上下文压缩生命周期提示](decisions/providers/adr-150-provider-compaction-lifecycle.md)：原生开始／结束映射、稳定身份配对、失败提示与历史回放。

- [ADR-149：Codex 高权限档位自动批准 computer use 访问](decisions/providers/adr-149-codex-computer-use-access-auto-approval.md)：Auto-review / Full Access 的原生访问批准、显式覆盖与档位切换。

- [ADR-148：daemon 的无状态 MCP 编排接口](decisions/daemon/adr-148-daemon-mcp.md)：Rust RPC 复用与 Paseo 适配；[验证报告](reports/daemon/mcp-implementation-2026-10-14.md)。
- [ADR-147：DSH 原生插件情景模式与权限分离](decisions/providers/adr-147-dsh-native-plugin-presets.md)：动态预设目录、用户插件、profile 隔离及会话专属控件。

- [ADR-146：Workspace 重置清除变更请求关联](decisions/workspace/adr-146-reset-workspace-change-request-association.md)：本地分支重置标记、旧 PR/MR 过滤与远程跟踪清理。

- [ADR-145：Bot 执行期间自动拒绝权限请求](decisions/bot/adr-145-bot-permission-denial.md)：统一原生拒绝、禁止手动批准、关闭原生自动审核与移除 Bot 审批界面。
- [ADR-144：Provider 安全诊断与持久失败提示](decisions/providers/adr-144-provider-error-reporting.md)：所有内置 Provider 的结构化错误日志、公共安全分类与刷新后保留的聊天失败通知。
- [ADR-143：OpenCode 2.0.25 兼容与 ACP 审批边界](decisions/providers/adr-143-opencode-2-0-25-compatibility.md)：修正最低版本、多行命令与大文件审批、创建失败清理；[验证报告](reports/providers/opencode-2.0.25-compatibility-2026-10-13.md)。

- [ADR-141：客户端测试功能开关与 Workspace Bot](decisions/clients/adr-141-experimental-workspace-bots.md)：默认关闭的全局客户端设置、Bot 创建／对话／配置、持久回执与标签页恢复。

- [ADR-140：Codex MCP 超大字段使用展示预览](decisions/providers/adr-140-codex-mcp-display-previews.md)：按 JSON 字节预算生成明确标记的预览，保留原生完整结果与实时／历史一致性。
- [ADR-139：Workspace Bot 与独占 Channel](decisions/bot/adr-139-workspace-bots-and-owned-channels.md)：独立 bot crate、单向依赖 provider、会话隔离与本地记忆。
- [ADR-138：定时任务固定归属于 Workspace](decisions/workspace/adr-138-workspace-owned-schedules.md)：固定归属、Agent 目标校验、独立执行与版本兼容。
- [ADR-137：清理已移除功能的共享协议与 SDK](decisions/clients/adr-137-remove-retired-feature-protocol.md)：移除 Hub、Plugin 管理、Chat 与 Loop 的旧消息和 SDK 方法，保留有效订阅与时间线显示项。
- [ADR-136：拍平单功能区层并收口重复助手](decisions/workspace/adr-136-single-area-layer-flattening.md)：去掉只承载一个功能区的 `<layer>::<area>` 中间段，并收口重复的 RPC/ACP 助手。
- [ADR-135：OpenCode 目录就绪与配置确认](decisions/providers/adr-135-opencode-catalog-readiness.md)：异步配置确认、动态目录发现与并发冷启动协调；最低版本由 ADR-143 修正为 2.0.25。
- [ADR-134：已接纳的普通 RPC 等待共享任务额度](decisions/daemon/adr-134-admitted-rpc-job-waiting.md)：并发能力组共享许可、停机取消与 RPC 拒绝上下文。
- [ADR-133：先显示持久 Timeline 页面，后台校验原生历史](decisions/providers/adr-133-cached-timeline-first.md)：尾页优先、有界上翻、原生校验去重与旧主机回退。
- [ADR-132：Checkout 刷新独立准入与有界输出背压](decisions/workspace/adr-132-checkout-refresh-admission-and-backpressure.md)：隔离前台 Git 任务，diff 拥堵时等待输出额度并保持连接。

- [ADR-131：metadata 按能力组组织模块](decisions/workspace/adr-131-metadata-capability-groups.md)：server/daemon/push/directory/labels/automation/worktrees 七组沿用分层，组间仅经 ports/protocol 协作，并由模块边界测试约束。
- [ADR-130：Workspace 与 Project 方法归 metadata](decisions/workspace/adr-130-workspace-methods-owned-by-metadata.md)：worktree、恢复与 GitHub clone 的服务和协议迁入 metadata，filesystem 只保留本地适配器，端口下沉 model/domain。
- [ADR-129：Agent Skills 能力组归 provider](decisions/providers/adr-129-provider-owned-skills.md)：整组迁移、主机级安装与事务兼容，保留文件操作队列。
- [ADR-128：桌面内置 daemon 可在退出后继续运行](decisions/clients/adr-128-desktop-daemon-keep-running.md)：退出时交接 0600 记录、下次启动经认证与 PID 校验后重新接管，交接失败照常停止。
- [ADR-127：移除 daemon 配对 offer RPC](decisions/daemon/adr-127-remove-daemon-pairing-offer.md)：删除方法清单、路由、共享协议和 SDK；旧请求返回未知方法。
- [ADR-126：按身份读取目标 Workspace，完整目录后台同步](decisions/workspace/adr-126-workspace-target-read.md)：只读目标状态、缓存优先、连接和事件竞争保护。
- [ADR-125：持久化展示投影索引与按页读取 Timeline](decisions/providers/adr-125-durable-timeline-projection-pages.md)：事务内增量维护、重启复用、源覆盖游标与字节回退。
- [ADR-124：登录后自动添加全部在线服务主机](decisions/clients/adr-124-automatic-online-host-import.md)：移除账户主机选择，统一浏览器登录后导入全部主机，并保留已有连接与持久主机记录。
- [ADR-123：客户端仅保留浏览器统一登录](decisions/clients/adr-123-browser-only-account-login.md)：删除移动端与 Desktop 的原 AIT 密码表单、命令和共享登录方法，服务能力检查失败不再回退。
- [ADR-122（Provider）：Codex Auto-review 的 computer use 工具批准](decisions/providers/adr-122-codex-computer-use-approval.md)：原生插件逐工具预批准；档位范围与访问批准由 ADR-149 修订。
- [ADR-122（客户端）：在线服务主机进入客户端持久注册表](decisions/clients/adr-122-persistent-online-service-hosts.md)：显式添加、多主机恢复、服务绑定及独立删除。
- [ADR-121：ACP 轮次关联与原生历史结算](decisions/providers/adr-121-acp-turn-history-reconciliation.md)：实时与重放共享轮次身份，完整原生历史在终态前原子校正消息顺序。
- [ADR-120：单连接启动请求的有界排队](decisions/daemon/adr-120-single-connection-startup-admission.md)：为正常启动突发保留 16 个组内槽位，维持超量拒绝、消息顺序和 Ping 可用性。
- [ADR-119：原生账号用量与 Codex 速度目录](decisions/providers/adr-119-native-account-usage-and-codex-speed.md)：会话实际账号、只读凭据边界、主机缓存与原生速度档位。
- [ADR-118：摘要接口归 model，配置文件适配归 persistence](decisions/daemon/adr-118-summary-contracts-and-persistence-configuration.md)：`SummaryGenerator`/`SummaryConfiguration` 移入 `model::summary`，daemon 注入 persistence 的配置适配器并删除自有实现。
- [ADR-117：终端配色查询与外观更新生命周期](decisions/clients/adr-117-terminal-palette-and-appearance-lifetime.md)：显示端回答配色查询，workspace 和终端实例在外观变化时保持存活。
- [ADR-116：库 crate 只公开被其他 crate 使用的项](decisions/daemon/adr-116-crate-visibility.md)：默认私有，启用 `unreachable_pub`，删除收缩后暴露的未使用代码与转发 `pub use`。
- [ADR-115：OpenCode 官方 ACP Provider](decisions/providers/adr-115-opencode-acp-provider.md)：1.x / 2.x 官方 stdio 协议、原生会话身份与结构化元数据、按原生能力处理问答和辅助会话、审批、取消和历史重放，取代私有 HTTP/SSE adapter。
- [ADR-114：iOS 终端组合输入归 UIKit](decisions/clients/adr-114-ios-terminal-ime.md)：原生组合范围识别、确认文字提交与终端控制键分离。
- [ADR-113：filesystem 按能力组组织模块](decisions/workspace/adr-113-filesystem-capability-groups.md)：能力组沿用原分层，组间仅经 ports/protocol 协作，并由模块边界测试约束；skills 与 worktrees 归属由 ADR-129、ADR-130 修订。
- [ADR-112：file 瘦身为 persistence，宿主专用文件归 daemon](decisions/daemon/adr-112-persistence-crate.md)：启动配置、故障证据与 server identity 迁回 daemon，metadata 经端口定位项目配置，仅 daemon 在生产代码中依赖 persistence。
- [ADR-111：纯业务数据归 domain，连接协议并入 model::server](decisions/daemon/adr-111-domain-values-and-server-protocol.md)：移除 protocol crate，数据值与运行资源分离，消费者直接依赖类型所属 crate。
- [ADR-110：OpenCode 双版本私有协议边界](decisions/providers/adr-110-opencode-private-protocol-boundary.md)：协议差异收口、统一文本身份，以及原生权限拒绝与主动取消的独立结算。
- [ADR-109：本地故障证据与 Harness 日志采集](decisions/daemon/adr-109-local-incident-evidence.md)：后台取证、脱敏、保留与文件导出。
- [ADR-108：Relay RPC 与基础连接方法归所属 crate](decisions/daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)：relay 拥有控制 RPC 并仅依赖 model；metadata 声明始终可用的基础连接方法，API 保留传输与跨能力协调。
- [ADR-107：共享组件直接从所属 crate 导入](decisions/daemon/adr-107-direct-imports-from-owning-crates.md)：删除迁移用转发模块，调用处直接引用 model/domain/file，provider 和 schedule 仅在测试中依赖 file。
- [ADR-106：具体文件持久化归 file，model 仅声明契约](decisions/daemon/adr-106-concrete-file-persistence.md)：迁移 registry、创建回执和 JSON 文件适配器，依赖调整为 file → model/domain。
- [ADR-105：File 工具、通用 Registry 与启动配置独立成 crate](decisions/daemon/adr-105-file-tools-and-startup-config.md)：单文件读写与监听、共享 Registry 引擎及启动配置归 file（后改名 persistence，见 ADR-112），业务服务继续拥有 schema 和事务。
- [ADR-104：Filesystem 仅通过 model 契约协作](decisions/workspace/adr-104-filesystem-model-collaboration.md)：共享观察和纯投影归 model，项目登记、命名与 setup 通过接口注入，移除最后一条功能 crate 间依赖。
- [ADR-103：Terminal 仅依赖 model 的共享契约](decisions/daemon/adr-103-terminal-model-dependency.md)：registry、Workspace 活动和连接事件直接使用 model，移除 metadata 依赖。
- [ADR-102：共享协作契约归 model，Provider 不依赖 metadata](decisions/providers/adr-102-provider-metadata-independence.md)：共享记录、协议、事件、创建回执和存储下沉，Workspace 业务通过接口协作。
- [ADR-101：Provider 拥有摘要生成能力](decisions/providers/adr-101-provider-summary-generator.md)：有界生成实现归 provider；接口与配置适配归属由 ADR-118 修订。
- [ADR-100：功能 crate 作为完整服务安装](decisions/daemon/adr-100-crate-level-service-installation.md)：各功能 crate 通过具体服务类型按 crate 整体安装；基础连接声明归属由 ADR-108 修订。
- [ADR-099：Provider 自选辅助小模型](decisions/providers/adr-099-provider-owned-auxiliary-models.md)：能力声明、原生认证与用户覆盖。
- [ADR-098：客户端消息分块与文件上传背压](decisions/clients/adr-098-acknowledged-client-chunks.md)：原图消息有界重组、文件逐块确认与兼容性。
- [ADR-097：Google Play 内部测试手动发布](decisions/clients/adr-097-google-play-internal-release.md)：签名 AAB、远端版本计数与内部测试草稿或发布。
- [ADR-096：Desktop 内置 daemon 默认同步与手动下线](decisions/clients/adr-096-desktop-default-host-sync.md)：登录后默认注册本机，持久保留手动下线选择，并统一在线服务添加主机表单风格。
- [ADR-095：组件自行声明方法元数据](decisions/daemon/adr-095-component-method-declarations.md)：移除中心目录和 `MethodGroup`，功能 crate 仅提供两种方法元数据接口，API 聚合名称并校验。
- [ADR-094：客户端统一使用 Ait 标准方法名](decisions/clients/adr-094-canonical-ait-client-methods.md)：应用、SDK、消息校验和方法目录使用标准名称，事件订阅参数转换集中在共享协议层。
- [ADR-093：以可消费 Context 逐级处理请求](decisions/daemon/adr-093-consumable-request-context.md)：组件方法向上组合，仅 `NotImplemented` 继续分发并断言 Context 未消费。
- [ADR-092：工作区重置同步 origin 同名分支](decisions/workspace/adr-092-reset-same-named-remote-branch.md)：存在远端初始分支时一次强制推送同步，并校验远端并发变更。
- [ADR-091：会话独立执行与 Provider 后台发现](decisions/providers/adr-091-independent-session-execution.md)：按稳定身份保序、后台 discovery、独立读取与 wait、级联屏障和全局资源预算。
- [ADR-090：Timeline 单项 768 KiB 与按字节分页](decisions/providers/adr-090-timeline-entry-and-page-budgets.md)：展示条目预算、整页响应预算与连续 source 游标。
- [ADR-089：内置 Provider 装配归 provider crate](decisions/providers/adr-089-provider-owned-composition.md)：内置列表、启动配置与辅助生成能力由 provider 自己组装。
- [ADR-088：Provider 发现与启动历史同步隔离](decisions/providers/adr-088-provider-catalog-startup-isolation.md)：有界 catalog 通道与本机连接就绪后的恢复。
- [ADR-087：Antigravity CLI 原生 Provider](decisions/providers/adr-087-antigravity-cli-provider.md)：官方与 Homebrew 安装发现、NDJSON 会话、权限模式与恢复边界。
- [ADR-087：中心设备管理与 Linux/macOS daemon 的 JWT 授权](decisions/clients/adr-087-headless-device-authorization.md)：固定中心，支持 Web 与 CLI token 授权；[授权与发布验收](plans/linux-headless-device-auth.md)覆盖脚本安装、JWT 续期、撤销与 relay。
- [ADR-086：桌面、Android 与 iOS 的 Authing 浏览器登录](decisions/clients/adr-086-authing-native-login.md)：浏览器认证、手机号/邮箱账号、一次性代码与原生 PKCE 回传；[配置与验收](operations/authing-client-login.md)。
- [ADR-085：Daemon 同步的稳定节点身份](decisions/clients/adr-085-stable-daemon-publication.md)：重复注册复用节点、旧 Host 绑定迁移、删除主机后的客户端登录及通用桌面 IPC 命名。
- [ADR-084（客户端）：iOS 在线服务账户与主机中继](decisions/clients/adr-084-ios-account-relay.md)：iOS 安全存储、原生账户会话、票据中继与下载。
- [ADR-084（工作区）：工作区主按钮按提交与 PR 生命周期推进](decisions/workspace/adr-084-workspace-primary-git-action.md)：同名远端分支比较、PR 重用和归档推荐条件。
- [ADR-083（客户端）：在线服务登录与逐主机同步分离](decisions/clients/adr-083-online-service-host-sync.md)：二级登录入口、应用账户设置及每台 daemon 的独立同步与租约。
- [ADR-083（工作区）：工作区重置到 origin 的最新默认分支](decisions/workspace/adr-083-reset-workspace-to-origin-default.md)：重置按钮、初始分支名、已有同名分支的检出和 Git 执行顺序。
- [ADR-082（工作区）：Workspace 可空字段使用单级 Option](decisions/workspace/adr-082-canonical-nullable-workspace-fields.md)：缺失字段规范化为 `null` 的 wire 行为。
- [ADR-082（Provider）：DeepSeek Harness 原生交互 Host](decisions/providers/adr-082-deepseek-harness-native-host.md)：权限切换、结构化问题、用户消息持久化与原生历史恢复。
- [ADR-081：Workspace 与 Agent 目录主动推送](decisions/workspace/adr-081-directory-change-push.md)：变更唤醒和事件推送。
- [ADR-080：Android APK 独立手动发布](decisions/clients/adr-080-standalone-android-release.md)：统一测试与正式入口，桌面发布不再调用 Android。
- [ADR-079：移动端统一使用 Expo EAS 构建](decisions/clients/adr-079-mobile-eas-builds.md)：沿用原有 profile，Android 发布通用 APK，iOS 提交 TestFlight。
- [ADR-078：Android 发布改为手动可选](decisions/clients/adr-078-optional-android-release.md)：标签发布只构建桌面，Android 默认关闭。
- [ADR-077：Android APK 的 GitHub Release 发布](decisions/clients/adr-077-android-apk-release.md)：APK 构建、安装包命名与附件职责。
- [ADR-076：Android 账户与中继客户端](decisions/clients/adr-076-android-account-relay.md)：共享账户会话、Android 安全存储、原生认证连接与下载。
- [ADR-075：Relay 协议定义与连接执行分离](decisions/clients/adr-075-relay-protocol-modules.md)：中继类型化消息、WebSocket 收发边界与单连接协商标识。
- [ADR-074（客户端）：账户发现与按需反向中继](decisions/clients/adr-074-account-host-relay.md)：邮箱密码登录、主机注册、独立的控制与数据 WebSocket，以及单连接调度；[初版验证与覆盖率报告](reports/clients/account-host-relay-validation.md)。
- [ADR-074（Provider）：OpenCode 原生 Provider](decisions/providers/adr-074-opencode-native-provider.md)：接入范围、审批、外部会话发现与导入恢复；当前 ACP 传输见 ADR-115。
- [ADR-073：桌面主窗口导航边界](decisions/clients/adr-073-desktop-renderer-navigation.md)：应用 preload 的来源限制。

[ADR 分类索引](decisions/README.md)按 daemon、工作区、Provider、客户端和品牌整理全部决策。
决策文档说明具体行为及其修订关系；当前目录与依赖图以[当前架构](architecture/README.md)为准。

## 运维与发布

- [故障诊断与证据导出](operations/diagnostics.md)：日志来源、保留策略和附件分享。
- [Relay 重连诊断](operations/relay-diagnostics.md)：会话关联、断连方向、错误分类及排查证据；[连接加速验证](reports/clients/relay-connect-latency.md)记录复用、TLS 缓存、NODELAY 与恢复退避。

- [Android APK 发布](operations/android-releases.md)：独立手动构建测试或正式通用 APK。
- [Google Play 内部测试](operations/google-play-internal-testing.md)：首次账号配置、测试者安装与手动 AAB 发布 CI。
- [发布指南](operations/releasing.md)：桌面 Release、Android Internal Testing、iOS TestFlight。
- [Ait 0.1.4 Draft 发布说明](reports/releases/release-0.1.4.md)：Provider 压缩提示、连接刷新和 HTTPS 修复，附本次全 Rust workspace 覆盖率证据并停在 Draft review。
- [Ait 0.1.3 同版本重建 Draft 准备](reports/releases/release-0.1.3-rebuild-2026-10-15.md)：纳入 computer use 批准、启动历史加载与 headless 锁释放修复，保留原标签并等待 review。
- [Ait 0.1.3 Draft 发布说明](reports/releases/release-0.1.3.md)：daemon MCP 编排、DeepSeek Harness 插件情景模式和原生配额／余额，完成 Draft 后停下等待 review。
- [Ait 0.1.2 Draft 发布说明](reports/releases/release-0.1.2.md)：图片输入、OpenCode 2.0.25、连接和 Bot 权限修复，完成 Draft 后停下等待 review。
- [Ait 0.1.1 Draft 发布说明](reports/releases/release-0.1.1.md)：Workspace 定时任务、实验性 Bot、无人值守 Host 和 Draft review 门禁。
- [Ait 0.1.0 发布说明](reports/releases/release-0.1.0.md)：沿用 0.0.29 功能的版本升级与正式安装包发布。
- [Ait 0.0.29 发布说明](reports/releases/release-0.0.29.md)：OpenCode 目录与配置确认、桌面警告和 RPC 诊断修复，以及官方服务域名迁移。
- [Ait 0.0.28 发布说明](reports/releases/release-0.0.28.md)：会话历史与订阅恢复、Changes 刷新修复和共享登录的浏览器客户端支持。
- [Ait 0.0.27 发布说明](reports/releases/release-0.0.27.md)：退出后保留 daemon、OpenCode 与登录恢复、Workspace/Timeline 加速和 CI 修复。
- [Ait 0.0.26 发布说明](reports/releases/release-0.0.26.md)：终端崩溃、启动请求突发与 OpenCode 目录校验修复。
- [Ait 0.0.25 发布说明](reports/releases/release-0.0.25.md)：OpenCode ACP、用量与 Codex 速度档位，以及桌面交互改进。
- [Ait 0.0.24 发布说明](reports/releases/release-0.0.24.md)、[Ait 0.0.23 发布说明](reports/releases/release-0.0.23.md)；全部版本见[发布验证索引](reports/README.md#发布验证)。
- [Ait 0.0.23-beta.1 发布说明](reports/releases/release-0.0.23-beta.1.md)：beta 通道与最新 main 修复。
- [Ait 0.0.22 发布说明](reports/releases/release-0.0.22.md)：远端分支重置、会话并发、Antigravity 与实时 Timeline 通知修复。
- [Ait 0.0.21 发布准备与失败记录](reports/releases/release-0.0.21.md)：成品 Timeline 门禁阻止发布。
- [Ait 0.0.20 发布说明](reports/releases/release-0.0.20.md)：统一浏览器登录、iOS 认证窗口与工作区重置分支复用。
- [Ait 0.0.19 发布说明](reports/releases/release-0.0.19.md)：daemon 注册身份、同步重试、桌面 IPC 命名与工作区重置路径修复。
- [Ait 0.0.18 发布说明](reports/releases/release-0.0.18.md)：Codex 截图时间线、在线服务与主机同步、工作区 Git 操作。
- [Ait 0.0.17 发布说明](reports/releases/release-0.0.17.md)：OpenCode 工具顺序、Workspace 可空字段与 Rust 清理。
- [Ait 0.0.16 发布说明](reports/releases/release-0.0.16.md)：目录推送、终端与 Codex 修复，以及桌面发布。
- [Ait 0.0.15 发布说明](reports/releases/release-0.0.15.md)：OpenCode、账户主机中继、Android APK 与稳定性修复。
- [Ait 0.0.14 发布说明](reports/releases/release-0.0.14.md)：Diff 语法高亮、侧边栏统计与 Codex 推理等级。
- [Apple 本机构建](operations/apple-builds.md)：DMG、模拟器和 IPA。
- [Claude Code](operations/claude-code.md)：认证、原生会话与审批。
- [DeepSeek Harness](operations/deepseek-harness.md)：原生 Host、DeepSeek 账号余额、会话导入、权限模式、question 与 ACP 兼容配置。
- [Antigravity CLI](operations/antigravity.md)：AGY 安装、登录、权限模式、会话恢复与账号额度查询。
- [语音与听写](operations/speech.md)：离线模型、后端配置和限制。

## 工程规范与验证

- [Ait 0.1.3 启动与会话查询诊断](reports/clients/ait-0.1.3-startup-diagnostics-2026-10-14.md)：Timeline 读取排队、草稿提示词索引就绪与 Codex 模型目录告警归因。

- [DSH 插件情景模式验证](reports/providers/dsh-plugin-presets.md)：动态目录、独立权限、恢复、profile 和本机 Host 验证。
- [Antigravity 账号额度接入](reports/providers/antigravity-account-quota-2026-10-14.md)：已登录仍不可用的复现、原生 `/usage` 接入、主机与会话账号隔离和失败回归。
- [DSH DeepSeek 余额接入](reports/providers/dsh-deepseek-balance-2026-10-14.md)：API Key 与原生账号登录的区别、DeepSeek 官方余额查询、人民币展示及本机验证。

- [提问弹层响应超时与重试](reports/clients/question-response-timeout-2026-10-13.md)：5 秒恢复按钮、保留填写内容、清理权限请求占用，以及真实丢包重试验证。
- [定时任务列表 Host 展示去重](reports/clients/schedule-host-filter-2026-10-12.md)：按主机筛选后省略重复主机名，以及返回全部主机视图的浏览器回归。
- [OpenCode 模型目录与执行失败修复](reports/providers/opencode-catalog-readiness-2026-10-10.md)：2.0.20 复现、配置确认、数据库冷启动竞争修复与原生流程验证。
- [Ait 0.0.28 客户端告警修复与诊断验证](reports/clients/release-0.0.28-warning-fixes-2026-10-11.md)：共享许可等待、事件协商、界面告警与错误上下文。
- [本机 Changes 刷新断线：修复验证](reports/workspace/changes-refresh-transport-2026-10-11.md)：Git 任务额度隔离、有界 diff 输出等待与控制响应回归。
- [Ait 0.0.27 安装后诊断与修复](reports/clients/release-0.0.27-installed-diagnostics-2026-10-11.md)：原生事件校验、启动订阅与更新诊断误报。

- [Rust style guide](policy/rust.md)：Rust 代码、测试、lint 与覆盖率规范。
- [文档规范](policy/documentation.md)：分类、维护和历史资料清理规则。
- [OpenCode 使用问题追踪](reports/providers/opencode-feedback-2026-10-09.md)：Build 图标与原生权限管理问题。
- [在线服务主机持久保存与 OpenCode 时间线验证](reports/clients/persistent-hosts-timeline-validation-2026-10-10.md)：重启恢复、顺序与时间回归、默认并行测试及本次覆盖率证据。
- [Codex computer use 请求与 Auto-review 批准验证](reports/providers/codex-computer-use-approval-2026-10-10.md)：请求处理边界、原生配置验证和定向回归。
- [周期任务按变更驱动：PR 验证](reports/daemon/periodic-loops-pr-validation-2026-10-10.md)：Provider 调度与会话节奏、终端/定时任务/diff 轮询减负、完整测试与可比基线覆盖率。
- [Rust 测试提速：提交验证](reports/daemon/rust-test-speed-validation-2026-10-10.md)：nextest、调试信息、CI 缓存与并发不稳定测试的修复。
- [Paseo desktop/app 选定改动移植](reports/clients/paseo-desktop-app-port-2026-10-10.md)：17 项推荐与产品增强、原生适配和定向验证。
- [摘要契约与目录准入：提交验证](reports/daemon/summary-contracts-directory-admission-validation-2026-10-10.md)：摘要草稿接入、目录预算隔离、完整测试与可比基线覆盖率。
- [Crate 可见性与最新 main 整合验证](reports/daemon/crate-visibility-pr-validation-2026-10-09.md)：PR #239 的 ACP 冲突处理、完整测试、逐 crate 覆盖率与源码证据。
- [OpenCode 官方 ACP 迁移验证](reports/providers/opencode-acp-2026-10-09.md)：真实原生问答、审批、恢复、无工具摘要及覆盖率证据。

- [Persistence 与 filesystem 能力边界 PR 验证](reports/daemon/persistence-filesystem-pr-validation-2026-10-09.md)：13-crate 完整测试、覆盖率与原样保留的未接入摘要草稿。
- [Domain 数据与 Server 协议归属 PR 验证](reports/daemon/domain-values-server-protocol-pr-validation-2026-10-09.md)：13-crate workspace 测试、覆盖率与源码证据。

- [Crate 边界与 main 整合验证](reports/daemon/crate-boundaries-main-rebase-2026-10-07.md)：完整测试、覆盖率与依赖守卫结果。
- [DSH 辅助生成验证](reports/providers/dsh-auxiliary-generation.md)：无工具、无持久化的原生 headless 通道与覆盖率。
- [OpenCode 辅助生成验证](reports/providers/opencode-auxiliary-generation.md)：私有、禁用工具的原生通道与清理边界。
- [原生会话预览验证](reports/providers/native-session-previews.md)：OpenCode / DSH 的首尾 prompt、只读查询及降级行为。
- [DSH 模型发现验证](reports/providers/dsh-readonly-discovery.md)：只读模型目录与会话初始化隔离。
- [附件分块传输验证](reports/clients/attachment-chunks.md)：原图消息、200 MiB 文件上传、背压及覆盖率。

- [File、共享契约与 RPC 边界 PR 验证](reports/daemon/file-and-rpc-boundaries-pr-validation-2026-10-08.md)：14-crate workspace 测试、覆盖率与源码证据。
- [Rust workspace 每 crate 95% 覆盖率验证](reports/daemon/crate-coverage-95-2026-10-14.md)：15 个 crate 的可比基线、最终计数与未覆盖行证据。
- [Daemon 测试扩展与覆盖率验证（历史）](reports/daemon/crate-coverage-rebase.md)：rebase 验证、逐 crate 证据与历史测量索引。
- [Provider 能力清单](plans/provider-parity.md)：当前能力与后续工作。
- [Antigravity CLI 验证](reports/providers/antigravity-cli.md)：协议夹具、安装路径和真实 AGY 验证范围。
- [Provider 装配边界验证](reports/providers/provider-composition.md)：内置注册、辅助生成、DSH 简称与覆盖率证据。
- [远程 Workspace 打开性能优化方案](plans/remote-workspace-open-performance.md)：连接、目录同步、Provider 排队与历史投影的代码分析、分阶段改动和验收指标。
- [Paseo 并发模型对齐修改清单](plans/paseo-agent-concurrency.md)：Provider 独立发现、按 Agent 保序、事件与加载去重、只读和 wait 隔离的实施列表。
- [Paseo 并发改造验证](reports/providers/paseo-concurrency-validation.md)：并发竞态、客户端兼容、格式/lint 与构建限制。
- [大 Diff 加载与超限降级](reports/workspace/large-diff-loading.md)：输出预算、连接保持与验证结果。
- [Agent 创建任务预算竞争](reports/providers/agent-creation-resource-contention.md)：DSH 创建报错复现与后台 fetch 失败隔离验证。
- [OpenCode 工具与结论顺序修复](reports/providers/opencode-tool-order.md)：流式前序条目发布与旧历史修复。
- [OpenCode 上游 PR 验证](reports/providers/opencode-upstream-pr.md)：上游整合、冲突处理与测试范围。
- [验证报告分类索引](reports/README.md)：实现、兼容性、发布及覆盖率记录。
- [2026-10-03 仓库审计](reports/daemon/repository-audit-2026-10-03.md)：审计范围、已修复问题与验证限制。
- [移动端 E2E](../apps/mobile/e2e/README.md)、[Maestro](../apps/mobile/maestro/README.md)：真实 daemon 连接验证。
- [品牌资产](../assets/brand/README.md)、[Paseo 来源](../paseo/README.md)：资产与许可证。

已移除实现的 CLI 流程、旧 daemon/worker 架构、旧 SQLite Project 设计和实验文档已清理。
相关历史保存在 Git 中；保留报告中的测试结果仅对应各报告注明的提交与测量范围。
