# 架构决策分类索引

文档按当前能力归属分类。历史验证记录只对应各文件注明的源码提交与平台。

## Daemon 与协议

- [ADR-116：库 crate 只公开被其他 crate 使用的项](daemon/adr-116-crate-visibility.md)
- [ADR-118：file 瘦身为 persistence，宿主专用文件归 daemon](daemon/adr-112-persistence-crate.md)
- [ADR-111：纯业务数据归 domain，连接协议并入 model::server](daemon/adr-111-domain-values-and-server-protocol.md)
- [ADR-109：本地故障证据与 Harness 日志采集](daemon/adr-109-local-incident-evidence.md)

- [ADR-108：Relay RPC 与基础连接方法归所属 crate](daemon/adr-108-relay-rpc-and-metadata-connection-methods.md)
- [ADR-107：共享组件直接从所属 crate 导入](daemon/adr-107-direct-imports-from-owning-crates.md)
- [ADR-106：具体文件持久化归 file，model 仅声明契约](daemon/adr-106-concrete-file-persistence.md)
- [ADR-105：File 工具、通用 Registry 与启动配置独立成 crate](daemon/adr-105-file-tools-and-startup-config.md)
- [ADR-103：Terminal 仅依赖 model 的共享契约](daemon/adr-103-terminal-model-dependency.md)
- [ADR-100：功能 crate 作为完整服务安装](daemon/adr-100-crate-level-service-installation.md)

- [ADR-095：组件自行声明方法元数据](daemon/adr-095-component-method-declarations.md)
- [ADR-026：规范化 Paseo WebSocket 接口并按能力分期接入](daemon/adr-026-canonical-paseo-websocket-surface.md)
- [ADR-033：独立 terminal 与完整 Terminal 方法分组](daemon/adr-033-daemon-terminal.md)
- [ADR-035：能力分组与安装规则归所属 server crate](daemon/adr-035-daemon-capability-groups.md)
- [ADR-036：请求先进入所属 crate 再分发到能力组](daemon/adr-036-daemon-crate-dispatch.md)
- [ADR-037：公共 Context 与具体 crate 分发](daemon/adr-037-daemon-model-context.md)
- [ADR-038：protocol 仅依赖公共 model](daemon/adr-038-daemon-protocol-dependencies.md)
- [ADR-042：连接级语音、听写与双后端](daemon/adr-042-daemon-voice.md)
- [ADR-045：移除 Hub、Chat 与 Loop 接口](daemon/adr-045-remove-hub-chat-loop.md)
- [ADR-047：移除 Plugin 接口并独立实现 Schedule 与 Browser](daemon/adr-047-daemon-schedule-browser.md)
- [ADR-064：默认离线语音与模型准备](daemon/adr-064-offline-speech.md)
- [ADR-072：Workspace 名称与当前文档边界](daemon/adr-072-workspace-names-and-documentation.md)
- [ADR-093：以可消费 Context 逐级处理请求](daemon/adr-093-consumable-request-context.md)

## 工作区、文件与 Git

- [ADR-113：filesystem 按能力组组织模块](workspace/adr-113-filesystem-capability-groups.md)
- [ADR-104：Filesystem 仅通过 model 契约协作](workspace/adr-104-filesystem-model-collaboration.md)
- [ADR-092：工作区重置同步 origin 同名分支](workspace/adr-092-reset-same-named-remote-branch.md)
- [ADR-025：先移植 Paseo Project / Workspace 模型与 registry](workspace/adr-025-paseo-registry.md)
- [ADR-028：GitHub 仓库发现与独立 Project 克隆注册](workspace/adr-028-github-project-provisioning.md)
- [ADR-029：统一 Paseo Project 并纵向拆出 metadata](workspace/adr-029-daemon-metadata.md)
- [ADR-030：纵向拆出 filesystem](workspace/adr-030-daemon-filesystem.md)
- [ADR-043：独立 server 的 Skills 选择与文件安装](workspace/adr-043-daemon-skills.md)
- [ADR-060：统一 Workspace 创建入口支持 Worktree](workspace/adr-060-workspace-create-worktree.md)
- [ADR-065：Paseo 目录、时间线与会话 API 兼容行为](workspace/adr-065-paseo-directory-and-timeline-projections.md)
- [ADR-066：通过 glab 支持 GitLab Forge](workspace/adr-066-gitlab-forge.md)
- [ADR-068：Workspace 侧边栏运行时摘要](workspace/adr-068-workspace-runtime-summaries.md)
- [ADR-069：活跃工作区的后台 Git fetch](workspace/adr-069-background-git-fetch.md)
- [ADR-071：在 Rust checkout adapter 生成 Diff 语法 token](workspace/adr-071-checkout-diff-syntax-highlighting.md)
- [ADR-081：由状态变更唤醒 Workspace 与 Agent 目录订阅](workspace/adr-081-directory-change-push.md)
- [ADR-082：Workspace 可空字段使用单级 Option](workspace/adr-082-canonical-nullable-workspace-fields.md)
- [ADR-083：工作区重置到 origin 的最新默认分支](workspace/adr-083-reset-workspace-to-origin-default.md)
- [ADR-084：工作区主按钮按提交与 PR 生命周期推进](workspace/adr-084-workspace-primary-git-action.md)

## Provider、Agent 与会话

- [ADR-118：原生账号用量与 Codex 速度目录](providers/adr-118-native-account-usage-and-codex-speed.md)
- [ADR-115：OpenCode 官方 ACP Provider](providers/adr-115-opencode-acp-provider.md)

- [ADR-110：OpenCode 双版本私有协议边界](providers/adr-110-opencode-private-protocol-boundary.md)
- [ADR-102：共享协作契约归 model，Provider 不依赖 metadata](providers/adr-102-provider-metadata-independence.md)
- [ADR-101：Provider 拥有摘要生成能力](providers/adr-101-provider-summary-generator.md)
- [ADR-091：会话独立执行与 Provider 后台发现](providers/adr-091-independent-session-execution.md)
- [ADR-090：Timeline 单项 768 KiB 与按字节分页](providers/adr-090-timeline-entry-and-page-budgets.md)
- [ADR-089：内置 Provider 装配归 provider crate](providers/adr-089-provider-owned-composition.md)
- [ADR-087：Antigravity CLI 原生 Provider](providers/adr-087-antigravity-cli-provider.md)
- [ADR-027：独立 AgentSession 与 AgentManager 生命周期边界](providers/adr-027-independent-agent-session-manager.md)
- [ADR-031：拆出 provider 并统一 Workspace 自动化与 state 入口](providers/adr-031-daemon-provider.md)
- [ADR-032：独立 server 接通 Codex 原生文本执行](providers/adr-032-daemon-native-provider-execution.md)
- [ADR-034：Agent 后续 turn 配置与 Session 事件/心跳](providers/adr-034-agent-config-session-events.md)
- [ADR-039：Agent Timeline、Provider 发现与创建过程订阅](providers/adr-039-agent-timeline-provider-creation.md)
- [ADR-040：原生 Session 发现、导入、刷新与上下文导出](providers/adr-040-native-session-import-refresh-context.md)
- [ADR-041：Agent 原生控制与 Provider 诊断、用量](providers/adr-041-agent-controls-provider-inspection.md)
- [ADR-046：Codex 增量显示历史与运行中追加输入](providers/adr-046-codex-streaming-and-steering.md)
- [ADR-050：独立 server 的 Claude Code Provider](providers/adr-050-claude-code-provider.md)
- [ADR-052: Native provider capability completion](providers/adr-052-native-provider-capabilities.md)
- [ADR-058：Server 的有界 metadata generation](providers/adr-058-daemon-metadata-generation.md)
- [ADR-082：DeepSeek Harness 原生交互 Host](providers/adr-082-deepseek-harness-native-host.md)
- [ADR-067：DeepSeek Harness ACP Provider](providers/adr-067-deepseek-harness-acp.md)

- [ADR-074：OpenCode 原生 Provider](providers/adr-074-opencode-native-provider.md)
- [ADR-088：Provider 发现与启动历史同步隔离](providers/adr-088-provider-catalog-startup-isolation.md)

## 客户端、连接与品牌

- [ADR-117：终端配色查询与外观更新生命周期](clients/adr-117-terminal-palette-and-appearance-lifetime.md)
- [ADR-114：iOS 终端组合输入归 UIKit](clients/adr-114-ios-terminal-ime.md)
- [ADR-097：Google Play 内部测试手动发布](clients/adr-097-google-play-internal-release.md)
- [ADR-098：客户端消息分块与文件上传背压](clients/adr-098-acknowledged-client-chunks.md)

- [ADR-096：Desktop 内置 daemon 默认同步与手动下线](clients/adr-096-desktop-default-host-sync.md)
- [ADR-094：客户端统一使用 Ait 标准方法名](clients/adr-094-canonical-ait-client-methods.md)
- [ADR-086：桌面、Android 与 iOS 的 Authing 浏览器登录](clients/adr-086-authing-native-login.md)
- [ADR-085：Daemon 同步的稳定节点身份](clients/adr-085-stable-daemon-publication.md)
- [ADR-084：iOS 在线服务账户与主机中继](clients/adr-084-ios-account-relay.md)
- [ADR-080：Android APK 独立手动发布](clients/adr-080-standalone-android-release.md)
- [ADR-079：移动端统一使用 Expo EAS 构建](clients/adr-079-mobile-eas-builds.md)
- [ADR-078：Android 发布改为手动可选](clients/adr-078-optional-android-release.md)
- [ADR-077：Android APK 的 GitHub Release 发布](clients/adr-077-android-apk-release.md)
- [ADR-076：Android 账户与中继客户端](clients/adr-076-android-account-relay.md)
- [ADR-083：在线服务登录与逐主机同步分离](clients/adr-083-online-service-host-sync.md)
- [ADR-075：Relay 协议定义与连接执行分离](clients/adr-075-relay-protocol-modules.md)
- [ADR-074：账户发现与按需反向中继](clients/adr-074-account-host-relay.md)

- [ADR-044：Paseo 前端适配 Rust server 协议](clients/adr-044-paseo-client-rust-transport.md)
- [ADR-048：Paseo workspace 与 Rust 桌面服务启动](clients/adr-048-paseo-desktop-rust-launcher.md)
- [ADR-049：独立 App 的 Rust server 浏览器连接](clients/adr-049-app-rust-browser-transport.md)
- [ADR-053：Ait 0.0.7 桌面发布切换到 apps/desktop](clients/adr-053-paseo-desktop-release.md)
- [ADR-054：桌面服务监听配置与网络地址](clients/adr-054-desktop-daemon-listen.md)
- [ADR-055：Ait 与 Paseo 的桌面数据和链接隔离](clients/adr-055-desktop-profile-isolation.md)
- [ADR-056：Ait 与 Paseo 的共享宿主资源隔离](clients/adr-056-paseo-coexistence.md)
- [ADR-061：App E2E 使用 Ait server，移除 relay 与插件运行功能](clients/adr-061-app-ait-e2e-remove-relay-plugin.md)
- [ADR-062：Ait 运行路径与项目配置文件](clients/adr-062-ait-runtime-paths.md)
- [ADR-063：Ait 本地 UI 包与原生测试连接](clients/adr-063-ait-local-ui-packages.md)
- [ADR-070：Ait iOS TestFlight 手动发布工作流](clients/adr-070-ios-testflight-release.md)
- [ADR-073：桌面主窗口的导航信任边界](clients/adr-073-desktop-renderer-navigation.md)

## 品牌与视觉

- [ADR-051：AIT 品牌识别与日间视觉系统](branding/adr-051-ait-brand-identity.md)

- [ADR-099：Provider 自选辅助小模型](providers/adr-099-provider-owned-auxiliary-models.md)
