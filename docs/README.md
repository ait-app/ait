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

- Provider 配置：[Claude Code](operations/claude-code.md)、[DeepSeek Harness](operations/deepseek-harness.md)、
  [Antigravity](operations/antigravity.md)、[Pi](operations/pi.md)、[Oh My Pi](operations/oh-my-pi.md)、
  [MiniMax Code](operations/minimax-code.md)。Codex 和 OpenCode 的启动配置见 [daemon 手册](operations/daemon.md#provider-与扩展能力)。
- [Pi 推理偏好](decisions/providers/adr-211-pi-next-turn-reasoning-controls.md)：下一条 prompt 前应用模型与推理级别，区分请求偏好和原生运行事实。
- [MCP 编排接口](operations/mcp.md)：HTTP、可选 Unix socket、认证和工具发现。
- [Agent Skill 分发](operations/agent-skills.md)：内置技能、更新、选择和卸载。
- [OMP 模型角色与原生操作](operations/oh-my-pi.md)：角色配置、异步模型目录及官方已导出操作；
  [原生 UI 与拥有者边界](decisions/providers/adr-218-native-ui-and-actions.md)、[验收报告](reports/providers/native-delivery-and-omp-roles-2026-11-02.md)。
- [原生控制结果兼容](decisions/providers/adr-214-native-control-result-cards.md)：旧客户端文本封装与当前结果卡片共用实时及历史出口。
- [查看子 Agent](decisions/providers/adr-209-observed-subagent-normalization.md)：统一会话入口、父子关系与只读观测边界。
- [Workspace 定时任务](operations/schedules.md)、[Workspace Bot](operations/bots.md)。
- [语音与听写](operations/speech.md)、[移动端通知](operations/mobile-notifications.md)。

## 架构与工程规范

- [客户端协议归属](decisions/clients/adr-211-sdk-native-daemon-protocol.md)：SDK 原生使用 Rust v1，应用只选择平台传输。
- [当前架构](architecture/README.md)：组件职责、依赖、存储和生命周期；[OMP ACP 桥接](architecture/omp-acp-bridge.md)。
- [ADR 分类索引](decisions/README.md)：决策及修订关系，完整清单只在此维护。
- [仓库约定](../AGENTS.md)、[Rust 规范](policy/rust.md)、[文档维护规范](policy/documentation.md)。
- [Agent 共享读视图](decisions/providers/adr-210-committed-runtime-read-snapshots.md)：已提交记录、目录与 Hub 读取的性能边界。
- [daemon 启动与发布](decisions/daemon/adr-201-daemon-owned-publication.md)：实例锁覆盖本地令牌、诊断采集和运行时重启；
  [访客资源边界](decisions/daemon/adr-208-workspace-share-guests.md)说明撤销清理、配置保护及后台自动化的只读权限。
- [桌面进程交接](decisions/clients/adr-207-desktop-daemon-readiness-ownership.md)：退出交接要求已收到就绪日志；
  [MCP Unix socket](decisions/daemon/adr-203-mcp-unix-socket.md)要求绑定前验证私有父目录。
- [移动端 E2E](../apps/mobile/e2e/README.md)、[Maestro](../apps/mobile/maestro/README.md)。
- [品牌资源](../assets/brand/README.md)、[Paseo 来源与许可证](../paseo/README.md)。

## 构建与发布

- [发布指南](operations/releasing.md)：版本准备、桌面 Release、nightly、平台发布入口。
- [Apple 本地构建](operations/apple-builds.md)：DMG、iOS 模拟器与 IPA。
- [Google Play 内部测试](operations/google-play-internal-testing.md)：Gradle AAB、签名、上传和验收。
- 用户可见变更见 [CHANGELOG](../CHANGELOG.md)；历史发布证据见[报告索引](reports/README.md)。

## 后续工作与历史

- [未完成计划](plans/README.md)：远程打开性能、客户端会话续期、Bot 后续工作和仓库审查遗留事项。
- [验证报告](reports/README.md)：按能力分类的实现与验证证据。
- [归档索引](archive/README.md)：已完成或被取代的方案、早期 Paseo 移植和 0.0.x 发布记录。
