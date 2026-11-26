---
id: ADR-311
status: accepted
date: 2026-11-26
amends:
  - workspace/adr-215-sandboxed-worktree-isolation.md
  - workspace/adr-223-linux-worktree-sandbox.md
  - providers/adr-087-antigravity-cli-provider.md
paths:
  - crates/provider/antigravity
  - crates/provider/native
---

# ADR-311：Antigravity 沙箱化 Worktree 写入隔离

后续修订：[ADR-312](adr-312-antigravity-acp-migration.md) 将主会话取消改为 ACP `session/cancel`，
创建、恢复与配置切换继续保留本决策的 OS 写入隔离。

## 背景

[ADR-215](../workspace/adr-215-sandboxed-worktree-isolation.md) 与
[ADR-223](../workspace/adr-223-linux-worktree-sandbox.md) 为沙箱化 Worktree（`sandbox: worktree_writes`）
提供了基于 macOS Seatbelt 与 Linux `bubblewrap` 的进程树写入隔离，但 `provider-antigravity`
最初未声明 `supports_write_confinement`，导致在沙箱化 Worktree 中创建或恢复 Antigravity 会话直接被
拒绝（`ConfinementUnsupported`）。

Google 官方 `agy` CLI 1.5.13 实测具有以下运行时与隔离特征：

1. **状态与配置分层**：CLI 将对话存储（`conversations/`、`conversation_summaries.db`）、轨迹与附件
   （`brain/`、`implicit/`）、认证与设备状态（`jetski_state.pbtxt`、`installation_id`）、缓存（`cache/`）
   及日志（`log/`、`cli.log`）写入 `$HOME/.gemini/antigravity-cli`；跨产品的全局配置、共享权限、MCP、
   插件、sidecar 与项目级覆盖位于 `$HOME/.gemini/config`。`agy` 不读取外部环境变量重定向这两个根目录。
2. **首次初始化**：不受限的 `agy models` 探测（`session::connect` 在每次创建/重连会话前均先执行）会解出内置
   资源 `$HOME/.gemini/antigravity-cli/builtin` 并初始化 `$HOME/.gemini/config`；此后在 `$HOME/.gemini/config`
   与 CLI 配置文件全部只读的条件下，headless 会话正常启动。
3. **嵌套终端沙箱**：`agy` 持久化配置支持开启内置终端沙箱（`enableTerminalSandbox`），在外层 Seatbelt 或
   `bwrap --disable-userns` 下会因嵌套沙箱或禁止新建 user namespace 失败；`--settings` 支持传入仅对当前
   进程生效、不落盘的内联 JSON 覆盖。
4. **Linux 取消信号**：[ADR-087](adr-087-antigravity-cli-provider.md) 通过向子进程发送 `SIGINT` 并等待原生
   `INTERRUPTED` 结果帧确认取消。Linux 下外层子进程是带 `--die-with-parent --new-session` 的 `bwrap`
   监控进程，直接向监控进程发 `SIGINT` 会终止监控进程并触发内核 `SIGKILL` 原生进程，使取消确认丢失。

## 决策

1. `AntigravityClient` 声明 `supports_write_confinement() -> true`，创建、恢复与会话内重连（取消后重连、
   模型/模式/思考等级切换）均透传 `AgentSessionSpec::confinement`，经 `provider_native::sandbox::command`
   包装交互会话进程。只读探测（`agy models`、`/usage`）与摘要生成沿用 ADR-215 保持不受限。
2. `provider_native::sandbox::ANTIGRAVITY_STATE` 允许写入 `$HOME/.gemini/antigravity-cli`，并将其下的
   `builtin`、`hooks.json`、`plugins`、`settings.json` 列为只读保护条目（Linux 将 `builtin` 加入初始化为空目录
   的受保护目录集合）。`$HOME/.gemini/config` 不进入可写清单，整体保持可读、只读。
3. 当 `confinement` 存在时，会话启动参数追加 `--settings {"enableTerminalSandbox":false}`，仅在当前受限会话中
   关闭 `agy` 内置终端沙箱，由外层 Seatbelt 或 `bubblewrap` 统一承担写入边界。
4. Linux `provider_native::sandbox::linux::process` 新增 `interrupt_native`，沿用 `terminate_native` 的
   `/proc` 与 `NSpid` 查找逻辑向命名空间内 PID 2 的原生进程直接发送 `SIGINT`，保留 `bwrap` 监控进程与
   PID 1 reaper 直至收到原生取消确认并正常关闭连接。

## 后果

- 沙箱化 Worktree 中可正常创建、取消、重连和恢复 Antigravity 会话，目前仅剩 DeepSeek Harness 未声明写入隔离支持。
- 受限会话无法篡改 CLI 的 `settings.json`、`hooks.json`、`plugins`、`builtin` 以及 `$HOME/.gemini/config`
  下的全局规则、MCP、插件与项目级权限配置；对话库、日志与缓存刷新保持正常。
- 若主机从未运行过不受限的 `agy`（包括未执行过模型发现）就直接以缺失状态目录启动受限会话，Linux 占位创建的空
  `builtin` 目录与全局只读的 `.gemini/config` 会在下一次不受限探测时补齐；常规路径下 `connect` 先跑不受限的
  `agy models`，不会触发该情况。
