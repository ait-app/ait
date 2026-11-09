# Ait 文档

这里说明当前源码和操作流程。先按任务选择入口；历史实现、测试数字和发布记录见
[验证报告](reports/README.md)与[归档](archive/README.md)，不作为当前能力承诺。

## 使用与开发

| 任务                              | 文档                                                                                   |
| --------------------------------- | -------------------------------------------------------------------------------------- |
| 安装、平台支持、快速开始          | [项目 README](../README.md)                                                            |
| 桌面开发与打包                    | [Desktop](../apps/desktop/README.md)                                                   |
| 移动端、Web 开发与网页镜像        | [Mobile 与 Web](../apps/mobile/README.md)                                              |
| 独立 daemon 安装、登录、后台运行  | [无人值守 daemon](operations/headless-daemon.md)                                       |
| daemon 配置、数据、鉴权和连接协议 | [daemon 手册](operations/daemon.md)                                                    |
| 账号登录、主机上线与登录保持      | [统一登录](operations/authing-client-login.md)                                         |
| 导出故障证据、排查远程连接        | [诊断](operations/diagnostics.md)、[Hub 与下载 Relay](operations/relay-diagnostics.md) |
| SDK 和协议开发                    | [客户端 SDK](../packages/client/README.md)、[协议包](../packages/protocol/README.md)   |

## Agent 与工作区

- [共享套餐缓存与 Codex 查询进程](decisions/providers/adr-219-shared-account-usage-cache.md)：原生调用前的快读、后台刷新、账号失效与进程复用。

- Provider 配置：[Claude Code](operations/claude-code.md)、[DeepSeek Harness](operations/deepseek-harness.md)、
  [Antigravity](operations/antigravity.md)、[Pi](operations/pi.md)、[Oh My Pi](operations/oh-my-pi.md)、
  [MiniMax Code](operations/minimax-code.md)。Codex 和 OpenCode 的启动配置见 [daemon 手册](operations/daemon.md#provider-与扩展能力)。
- [Pi admission 与恢复](operations/pi.md#session-lifecycle-and-limits)：超时和拒绝后的进程生命周期、失败输入的时间线限制；
  [ADR-156](decisions/providers/adr-156-pi-native-rpc-provider.md) 说明 `hasInput`、`observedWork` 与原生会话文件要求。
- [使用 Pi slash commands](operations/pi.md#slash-commands)：命令调用、项目信任、前端目录缓存与逐请求原生探测。
- [Pi 扩展通知](decisions/providers/adr-220-pi-extension-notifications.md)：会话时间线持久化、通知级别与有界的尽力交付。
- [Pi 推理偏好](decisions/providers/adr-211-pi-next-turn-reasoning-controls.md)：下一条 prompt 前应用模型与推理级别，区分请求偏好和原生运行事实。
- [MCP 编排接口](operations/mcp.md)：HTTP、可选 Unix socket、认证和工具发现。
- [沙箱化 Worktree](decisions/workspace/adr-215-sandboxed-worktree-isolation.md)：macOS 使用 Seatbelt；[Linux 使用 bubblewrap](decisions/workspace/adr-223-linux-worktree-sandbox.md)，说明 Agent 写入限制、Git 指针保护和共享 inode 等边界。
- [Agent Skill 分发](operations/agent-skills.md)：内置技能、更新、选择和卸载。
- [查看子 Agent](decisions/providers/adr-209-observed-subagent-normalization.md)：统一会话入口、父子关系与只读观测边界。
- [Workspace 定时任务](operations/schedules.md)、[Workspace Bot](operations/bots.md)（客户端标签页见 [ADR-214](decisions/clients/adr-214-workspace-bot-tab-v2.md)，权限与作用域 MCP 凭据见 [ADR-215](decisions/bot/adr-215-bot-permissions-and-scoped-mcp.md)，内部会话的只读查看见 [ADR-221](decisions/bot/adr-221-read-only-bot-session-inspection.md)，单一 Agent 模式与禁止创建 Workspace 见 [ADR-222](decisions/bot/adr-222-single-agent-mode-no-bot-workspaces.md)）。
- [语音与听写](operations/speech.md)、[移动端通知](operations/mobile-notifications.md)。

## 架构与工程规范

- [客户端协议归属](decisions/clients/adr-211-sdk-native-daemon-protocol.md)：SDK 原生使用 Rust v1，应用只选择平台传输。
- [当前架构](architecture/README.md)：组件职责、依赖、存储和生命周期；[OMP ACP 桥接](architecture/omp-acp-bridge.md)。
- [ADR 分类索引](decisions/README.md)：决策及修订关系，完整清单只在此维护。
- [仓库约定](../AGENTS.md)、[Rust 规范](policy/rust.md)、[文档维护规范](policy/documentation.md)。
- [Agent 共享读视图](decisions/providers/adr-210-committed-runtime-read-snapshots.md)：已提交记录、目录与 Hub 读取的性能边界。
- [Workspace 运行时能耗](decisions/workspace/adr-217-event-driven-workspace-runtime.md)：macOS 文件事件驱动侧边栏 Git 刷新，checkout 事实只唤醒 Workspace 目录。
- [daemon 启动与发布](decisions/daemon/adr-201-daemon-owned-publication.md)：实例锁覆盖本地令牌、诊断采集和运行时重启；
  [访客资源边界](decisions/daemon/adr-208-workspace-share-guests.md)说明撤销清理、配置保护及后台自动化的只读权限。
- [默认数据目录](decisions/clients/adr-216-default-data-directory-ait.md)：依次使用已存在的 `~/.ait`、
  `~/.ait-server-desktop`、`~/.ait-server`，都不存在时创建 `~/.ait`，不移动旧目录。
- [桌面进程交接](decisions/clients/adr-207-desktop-daemon-readiness-ownership.md)：退出交接要求已收到就绪日志；
  [MCP Unix socket](decisions/daemon/adr-203-mcp-unix-socket.md)要求绑定前验证私有父目录。
- [桌面多窗口](decisions/clients/adr-223-desktop-workspace-windows.md)：Workspace 精确路由、独立窗口布局与共享草稿、附件引用的保存边界。
- [移动端 E2E](../apps/mobile/e2e/README.md)、[Maestro](../apps/mobile/maestro/README.md)。
- [品牌资源](../assets/brand/README.md)、[Paseo 来源与许可证](../paseo/README.md)。

## 构建与发布

- [发布指南](operations/releasing.md)：版本准备、桌面 Release、Dev build、平台发布入口。
- [Apple 本地构建](operations/apple-builds.md)：DMG、iOS 模拟器与 IPA。
- [Google Play 内部测试](operations/google-play-internal-testing.md)：Gradle AAB、签名、上传和验收。
- 用户可见变更见 [CHANGELOG](../CHANGELOG.md)；历史发布证据见[报告索引](reports/README.md)。

## 后续工作与历史

- [未完成计划](plans/README.md)：远程打开性能、客户端会话续期、Bot 后续工作和仓库审查遗留事项。
- [验证报告](reports/README.md)：按能力分类的实现与验证证据。
- [归档索引](archive/README.md)：已完成或被取代的方案、早期 Paseo 移植和 0.0.x 发布记录。
