# ADR-145：Bot 执行期间自动拒绝权限请求

- 状态：已实现
- 日期：2026-10-13
- 关系：修订 [ADR-139](adr-139-workspace-bots-and-owned-channels.md) 与 [ADR-141](../clients/adr-141-experimental-workspace-bots.md) 的 Bot 审批行为。

## 背景

Bot 从内置对话和外部 Channel 持续接收消息。执行期间不能通过模型申请或客户端审批获得新增权限，也不能停在等待人工批准的状态。

## 决策

1. Provider 共用的 AgentManager 根据创建时持久化的 `internal` 与 `owner` 识别 Bot session。在发布权限事件前，向原生 Provider 回复 `{"behavior":"deny"}`。所有 Provider 的工具、计划、模式和问答审批回调均遵循此规则，不向客户端发布待审批请求。
2. 原生拒绝失败或请求缺少身份时，关闭原生执行并记录失败。Bot 协调层也会拒绝状态快照中残留的请求，覆盖恢复后尚未生成新事件的情况；拒绝失败则取消当前轮次，不继续等待审批。
3. `bot.permission.resolve.request` 保留兼容，但只接受精确的 `{"behavior":"deny"}`；Bot 与 Provider 两层均拒绝允许、选项替换和权限更新。普通 Agent 的人工审批流程继续使用原有规则。
4. Codex `auto-review` 在 Bot 中按相同沙箱的 `auto` 执行，Claude `auto` 按 `default` 执行，避免原生自动审核先行授予权限。创建和恢复已有 Bot session 均应用此限制。预先配置的工具访问范围继续由 Provider 执行。
5. 移除 Bot 对话的审批组件及不再使用的待审批写入路径。`needs_approval` 与 `pendingPermissions` 保留读取兼容，新增轮次不再写入等待审批状态；旧的中断轮次沿用 uncertain 恢复规则。

## 后果与验证

Bot 只能在现有权限内完成工作。原生 Provider 收到拒绝后可以解释限制或继续执行其他操作；无法处理拒绝时，本轮失败。用户不能从 Bot 审批接口扩大权限。

定向测试覆盖共用入口、手动授权拦截、拒绝失败、Codex 命令／文件／问答／计算机请求、Claude 工具／问答，以及 OpenCode ACP 原生拒绝。浏览器回归检查自动拒绝后显示回复并能继续对话。

合并 `main` 的 `13ce800c` 后，在 macOS 上使用离线原生协议 fixtures 验证本次 PR：

- `cargo test --offline --locked -p bot service::tests`：3 项通过；合并前 bot crate 的全部 21 项测试也已通过。
- `cargo test --offline --locked -p provider --lib` 分别使用 `bot_`、`rpc::agent_execution::bot::tests`、`service::agent_manager::tests::usage`、`service::agent_manager::tests::paseo`、`native_permissions_are_agent_scoped_ephemeral_and_support_questions` 过滤：7 + 1 + 5 + 22 + 1，共 36 项通过，覆盖普通 Agent 审批和上游新增失败诊断的兼容。
- 前端 `bot-chat.test.tsx` 与 `resources.test.ts`：38 项通过。
- `workspace-bots.spec.ts -g "Bot denies approvals"`：1 项通过，覆盖浏览器发送、daemon 拒绝、回复呈现和继续对话，使用隔离 daemon 与离线 Codex。
- daemon 构建、受影响 crate 的 Clippy、Rust／前端格式检查、前端类型检查、lint 和文档链接检查通过。

## Test coverage

未测量覆盖率：本次未请求覆盖率统计。验证使用受影响模块的测试；如后续需要测量，限定 bot 与 Provider 权限相关测试，不扩展为完整 Workspace 测试。
