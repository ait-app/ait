---
id: ADR-222
status: accepted
date: 2026-11-08
amends:
  - bot/adr-215-bot-permissions-and-scoped-mcp.md
paths:
  - crates/bot
  - apps/mobile
---

# ADR-222：每个 Provider 单一 Agent 模式，Bot 不创建 Workspace

- 关系：修订 [ADR-215](adr-215-bot-permissions-and-scoped-mcp.md) 的 `workspaces` 分组与模式上限。

## 背景

ADR-215 允许为每个 Provider 勾选 1–16 个模式，并提供 `workspaces` 分组让 Bot 创建 worktree。
设置项过多，用户只需要决定 Bot 启动的 Agent 使用哪个模式；Bot 作为 Workspace 管理者也不应再创建新的 Workspace。

## 决策

1. **单一模式。** 设置页与向导中每个 Provider 只有一个选择器，选项为该 Provider 的模式和“不允许”。
   `modes` 的线上形状仍为 `{ [provider]: string[] }`，以兼容旧客户端和已保存的配置；只有第一项生效。
   保存时截断为第一项；未重新保存的旧配置在执行时也只放行第一项，请求其他模式返回 `Forbidden`。
2. **Bot 不能创建 Workspace。** `create_workspace` 改为 `HostOnly`，对 Bot 和它启动的 Agent 都不可见、不可调用，也不会预授权给 Bot 会话。
   `create_agent` 本身只能使用作用域内已有的 Workspace。用户和未带作用域凭据的调用者不受影响。
3. **`workspaces` 分组退役。** 为读取已保存的配置，协议和存储仍接受该值，但它不授予任何工具，保存时被移除，界面不再显示。
4. **已记录的 worktree 保留。** 升级前 Bot 创建并记录的未归档 worktree 仍在作用域内，已在其中工作的 Agent 不受影响；不再新增记录。

## 后果

- 模式上限不再区分“默认”和“允许”：Bot 不能把 Agent 切换到所选模式以外的模式。
- 旧配置中第二项及之后的模式立即失效，存储了这些模式的 Bot 定时任务在运行时被拒绝，需改用所选模式。
- 允许运行命令的模式下，Agent 仍可通过原生命令执行 `git worktree`；本决策只约束 Ait MCP。

## 验证

按仓库规则只运行受影响范围的测试：`domain` 权限、`bot`、`api` MCP 作用域、移动端 Bot 组件与 `workspace-bots.spec.ts`；未运行完整 Rust Workspace 测试。

## Test coverage

未测量覆盖率：本次未请求覆盖率统计。
