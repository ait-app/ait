# Ait 文档

本文档索引只覆盖当前 Ait：Rust daemon、Electron desktop、共享 Web/移动界面和仓库内 SDK。
源码布局与命名见 [ADR-072](decisions/daemon/adr-072-workspace-names-and-documentation.md)。

## 开始使用与开发

- [项目说明](../README.md)：开发入口、workspace 和验证命令。
- [桌面端](../apps/desktop/README.md)：Electron 启动、构建和打包。
- [移动端与 Web](../apps/mobile/README.md)：Expo、浏览器连接与原生构建。
- [daemon 使用与连接协议](operations/daemon.md)：配置、鉴权、数据目录和生命周期。
- [当前架构](architecture/README.md)：能力归属、依赖边界和数据所有权。

- [OpenCode 使用问题追踪](reports/providers/opencode-feedback-2026-10-09.md)：Build 图标与原生权限管理问题。

## 架构决策

- [ADR-119：原生账号用量与 Codex 速度目录](decisions/providers/adr-119-native-account-usage-and-codex-speed.md)：会话实际账号、只读凭据边界、主机缓存与原生速度档位。
- [ADR-118：摘要接口归 model，配置文件适配归 persistence](decisions/daemon/adr-118-summary-contracts-and-persistence-configuration.md)：`SummaryGenerator`/`SummaryConfiguration` 移入 `model::summary`，daemon 注入 persistence 的配置适配器并删除自有实现。
- [ADR-117：终端配色查询与外观更新生命周期](decisions/clients/adr-117-terminal-palette-and-appearance-lifetime.md)：显示端回答配色查询，workspace 和终端实例在外观变化时保持存活。
- [ADR-116：库 crate 只公开被其他 crate 使用的项](decisions/daemon/adr-116-crate-visibility.md)：默认私有，启用 `unreachable_pub`，删除收缩后暴露的未使用代码与转发 `pub use`。
- [ADR-115：OpenCode 官方 ACP Provider](decisions/providers/adr-115-opencode-acp-provider.md)：1.x / 2.x 官方 stdio 协议、按原生能力处理问答和辅助会话、审批、取消和历史重放，取代私有 HTTP/SSE adapter。

- [ADR-114：iOS 终端组合输入归 UIKit](decisions/clients/adr-114-ios-terminal-ime.md)：原生组合范围识别、确认文字提交与终端控制键分离。
- [ADR-113：filesystem 按能力组组织模块](decisions/workspace/adr-113-filesystem-capability-groups.md)：git/forge/worktrees/files/skills 五组沿用原分层，组间仅经 ports/protocol 协作，并由模块边界测试约束。
- [ADR-112：file 瘦身为 persistence，宿主专用文件归 daemon](decisions/daemon/adr-112-persistence-crate.md)：启动配置、故障证据与 server identity 迁回 daemon，metadata 经端口定位项目配置，仅 daemon 在生产代码中依赖 persistence。
- [ADR-111：纯业务数据归 domain，连接协议并入 model::server](decisions/daemon/adr-111-domain-values-and-server-protocol.md)：移除 protocol crate，数据值与运行资源分离，消费者直接依赖类型所属 crate。
- [ADR-110：OpenCode 双版本私有协议边界](decisions/providers/adr-110-opencode-private-protocol-boundary.md)：协议差异收口、统一文本身份，以及原生权限拒绝与主动取消的独立结算。
- [ADR-109：本地故障证据与 Harness 日志采集](decisions/daemon/adr-109-local-incident-evidence.md)：后台取证、脱敏、保留与文件导出。
- [ADR-108：Relay RPC 与基础连接方法归所属 crate](decisions/daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)：relay 拥有控制 RPC 并仅依赖 model；metadata 声明始终可用的基础连接方法，API 保留传输与跨能力协调。
- [ADR-107：共享组件直接从所属 crate 导入](decisions/daemon/adr-107-direct-imports-from-owning-crates.md)：删除迁移用转发模块，调用处直接引用 model/domain/file，provider 和 schedule 仅在测试中依赖 file。
- [ADR-106：具体文件持久化归 file，model 仅声明契约](decisions/daemon/adr-106-concrete-file-persistence.md)：迁移 registry、创建回执和 JSON 文件适配器，依赖调整为 file → model/domain。
- [ADR-105：File 工具、通用 Registry 与启动配置独立成 crate](decisions/daemon/adr-105-file-tools-and-startup-config.md)：单文件读写与监听、共享 Registry 引擎及启动配置归 file，业务服务继续拥有 schema 和事务。
- [ADR-104：Filesystem 仅通过 model 契约协作](decisions/workspace/adr-104-filesystem-model-collaboration.md)：共享观察和纯投影归 model，项目登记、命名与 setup 通过接口注入，移除最后一条功能 crate 间依赖。
- [ADR-103：Terminal 仅依赖 model 的共享契约](decisions/daemon/adr-103-terminal-model-dependency.md)：registry、Workspace 活动和连接事件直接使用 model，移除 metadata 依赖。
- [ADR-102：共享协作契约归 model，Provider 不依赖 metadata](decisions/providers/adr-102-provider-metadata-independence.md)：共享记录、协议、事件、创建回执和存储下沉，Workspace 业务通过接口协作。
- [ADR-101：Provider 拥有摘要生成能力](decisions/providers/adr-101-provider-summary-generator.md)：有界生成实现归 provider；接口与配置适配归属由 ADR-118 修订。
- [ADR-100：功能 crate 作为完整服务安装](decisions/daemon/adr-100-crate-level-service-installation.md)：各功能 crate 通过具体服务类型按 crate 整体安装；基础连接声明归属由 ADR-108 修订。
- [ADR-097：Google Play 内部测试手动发布](decisions/clients/adr-097-google-play-internal-release.md)：签名 AAB、远端版本计数与内部测试草稿或发布。
- [ADR-099：Provider 自选辅助小模型](decisions/providers/adr-099-provider-owned-auxiliary-models.md)：能力声明、原生认证与用户覆盖。
- [ADR-098：客户端消息分块与文件上传背压](decisions/clients/adr-098-acknowledged-client-chunks.md)：原图消息有界重组、文件逐块确认与兼容性。

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
- [ADR-086：桌面、Android 与 iOS 的 Authing 浏览器登录](decisions/clients/adr-086-authing-native-login.md)：浏览器认证、一次性代码与原生 PKCE 回传；[配置与验收](operations/authing-client-login.md)。
- [ADR-085：Daemon 同步的稳定节点身份](decisions/clients/adr-085-stable-daemon-publication.md)：重复注册复用节点、旧 Host 绑定迁移、删除主机后的客户端登录及通用桌面 IPC 命名。
- [ADR-084：iOS 在线服务账户与主机中继](decisions/clients/adr-084-ios-account-relay.md)：iOS 安全存储、原生账户会话、票据中继与下载。
- [ADR-083：在线服务登录与逐主机同步分离](decisions/clients/adr-083-online-service-host-sync.md)：二级登录入口、应用账户设置及每台 daemon 的独立同步与租约。
- [ADR-080：Android APK 独立手动发布](decisions/clients/adr-080-standalone-android-release.md)：统一测试与正式入口，桌面发布不再调用 Android。
- [ADR-079：移动端统一使用 Expo EAS 构建](decisions/clients/adr-079-mobile-eas-builds.md)：沿用原有 profile，Android 发布通用 APK，iOS 提交 TestFlight。
- [ADR-078：Android 发布改为手动可选](decisions/clients/adr-078-optional-android-release.md)：标签发布只构建桌面，Android 默认关闭。
- [ADR-077：Android APK 的 GitHub Release 发布](decisions/clients/adr-077-android-apk-release.md)：APK 构建、安装包命名与附件职责。
- [ADR-076：Android 账户与中继客户端](decisions/clients/adr-076-android-account-relay.md)：共享账户会话、Android 安全存储、原生认证连接与下载。
- [ADR-075：Relay 协议定义与连接执行分离](decisions/clients/adr-075-relay-protocol-modules.md)：中继类型化消息、WebSocket 收发边界与单连接协商标识。
- [ADR-074：账户发现与按需反向中继](decisions/clients/adr-074-account-host-relay.md)：邮箱密码登录、主机注册、独立的控制与数据 WebSocket，以及单连接调度；[初版验证与覆盖率报告](reports/clients/account-host-relay-validation.md)。

[ADR 分类索引](decisions/README.md)按 daemon、工作区、Provider、客户端和品牌整理。
决策文档说明具体行为及其修订关系；当前目录与依赖图以当前架构和 ADR-072 为准。
[ADR-081：Workspace 与 Agent 目录主动推送](decisions/workspace/adr-081-directory-change-push.md)记录变更唤醒和事件推送。
[ADR-082：Workspace 可空字段使用单级 Option](decisions/workspace/adr-082-canonical-nullable-workspace-fields.md)记录缺失字段规范化为 `null` 的 wire 行为。
[ADR-083：工作区重置到 origin 的最新默认分支](decisions/workspace/adr-083-reset-workspace-to-origin-default.md)记录重置按钮、初始分支名、已有同名分支的检出和 Git 执行顺序。
[ADR-084：工作区主按钮按提交与 PR 生命周期推进](decisions/workspace/adr-084-workspace-primary-git-action.md)记录同名远端分支比较、PR 重用和归档推荐条件。

- [桌面主窗口导航边界](decisions/clients/adr-073-desktop-renderer-navigation.md)：应用 preload 的来源限制。

## 运维与发布

- [故障诊断与证据导出](operations/diagnostics.md)：日志来源、保留策略和附件分享。

- [Android APK 发布](operations/android-releases.md)：独立手动构建测试或正式通用 APK。
- [Google Play 内部测试](operations/google-play-internal-testing.md)：首次账号配置、测试者安装与手动 AAB 发布 CI。
- [发布指南](operations/releasing.md)：桌面 Release、Android Internal Testing、iOS TestFlight。
- [Ait 0.0.25 发布说明](reports/releases/release-0.0.25.md)：OpenCode ACP、用量与 Codex 速度档位，以及桌面交互改进。
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
- [DeepSeek Harness](operations/deepseek-harness.md)：原生 Host、会话导入、权限模式、question 与 ACP 兼容配置。
- [Antigravity CLI](operations/antigravity.md)：AGY 安装、登录、权限模式与会话恢复。
- [语音与听写](operations/speech.md)：离线模型、后端配置和限制。

- [DSH 原生交互 Host](decisions/providers/adr-082-deepseek-harness-native-host.md)：权限切换、结构化问题、用户消息持久化与原生历史恢复。
- [OpenCode 原生 Provider](decisions/providers/adr-074-opencode-native-provider.md)：接入范围、审批、外部会话发现与导入恢复；当前 ACP 传输见 ADR-115。

## 工程规范与验证

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

- [Rust style guide](policy/rust.md)：Rust 代码、测试、lint 与覆盖率规范。
- [File、共享契约与 RPC 边界 PR 验证](reports/daemon/file-and-rpc-boundaries-pr-validation-2026-10-08.md)：14-crate workspace 测试、覆盖率与源码证据。
- [Daemon 测试扩展与覆盖率验证](reports/daemon/crate-coverage-rebase.md)：最新 rebase 验证、逐 crate 证据与历史测量索引。
- [文档规范](policy/documentation.md)：分类、维护和历史资料清理规则。
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
