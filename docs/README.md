# Ait 文档

这里说明当前源码和操作流程。先按任务选择入口；历史实现、测试数字和发布记录见
[历史验证报告](archive/reports/README.md)与[归档](archive/README.md)，不作为当前能力承诺。

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
- Pi：[admission 与恢复](operations/pi.md#session-lifecycle-and-limits)、[slash commands](operations/pi.md#slash-commands)。
- [MCP 编排接口](operations/mcp.md)：HTTP、可选 Unix socket、认证和工具发现。
- [Agent Skill 分发](operations/agent-skills.md)：内置技能、更新、选择和卸载。
- [任务总览](operations/task-board.md)：跨主机任务看板、父子任务、审批、停止、已读和归档。
- [Workspace 定时任务](operations/schedules.md)、[Workspace Bot](operations/bots.md)。
- [主机资源监控](architecture/connection.md#主机资源监控)：侧栏读数、历史与采样生命周期。
- [语音与听写](operations/speech.md)、[移动端通知](operations/mobile-notifications.md)。

## 架构与工程规范

- [当前架构](architecture/README.md)：组件职责、依赖、存储和生命周期。
- 现行规格：[Provider](architecture/provider.md)、[Workspace](architecture/workspace.md)、
  [Bot](architecture/bot.md)、[连接与传输](architecture/connection.md)，逐条写明生效的约束及其 ADR；
  [OMP ACP 桥接](architecture/omp-acp-bridge.md)。
- [ADR 分类索引](decisions/README.md)：决策及修订关系，完整清单只在此维护；
  `node scripts/adr.mjs governs <路径>` 列出约束某个文件或目录的现行 ADR。
- [仓库约定](../AGENTS.md)、[Rust 规范](policy/rust.md)、[文档维护规范](policy/documentation.md)。
- [移动端 E2E](../apps/mobile/e2e/README.md)、[Maestro](../apps/mobile/maestro/README.md)。
- [品牌资源](../assets/brand/README.md)、[Paseo 来源与许可证](../paseo/README.md)。

## 构建与发布

- [发布指南](operations/releasing.md)：版本准备、桌面 Release、Dev build、平台发布入口。
- [Apple 本地构建](operations/apple-builds.md)：DMG、iOS 模拟器与 IPA。
- [Google Play 内部测试](operations/google-play-internal-testing.md)：Gradle AAB、签名、上传和验收。
- 用户可见变更见 [CHANGELOG](../CHANGELOG.md)；历史发布证据见[报告索引](archive/reports/README.md)。

## 后续工作与历史

- [未完成计划](plans/README.md)：远程打开性能、客户端会话续期、Bot 后续工作和仓库审查遗留事项。
- [历史验证报告](archive/reports/README.md)：2026-11-10 前按能力分类的验证证据；新的验证证据写在 PR 描述中。
- [归档索引](archive/README.md)：已完成或被取代的方案、早期 Paseo 移植和 0.0.x 发布记录。
